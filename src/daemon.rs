//! Loop principal do daemon: polling de bateria, aplicação de threshold,
//! Unix socket para comunicação com o CLI.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::battery::{Battery, BatteryStatus};
use crate::config::Config;
use crate::systemd;

/// Estado partilhado entre o loop principal e o servidor de socket.
#[derive(Debug, Clone)]
pub struct DaemonState {
    pub last_status: Option<BatteryStatus>,
    pub last_applied_threshold: Option<u8>,
    pub last_poll_ts: u64,
    pub error: Option<String>,
}

impl Default for DaemonState {
    fn default() -> Self {
        Self {
            last_status: None,
            last_applied_threshold: None,
            last_poll_ts: 0,
            error: None,
        }
    }
}

/// Ponto de entrada do daemon. Bloqueia até receber SIGTERM/SIGINT.
pub fn run(config: Config) -> Result<(), DaemonError> {
    let battery = Battery::detect().map_err(DaemonError::Battery)?;

    if !battery.supports_threshold() {
        log::warn!(
            "charge_control_end_threshold não suportado — \
             verifica se o módulo applesmc está carregado"
        );
    }

    let state = Arc::new(Mutex::new(DaemonState::default()));
    let running = Arc::new(AtomicBool::new(true));

    setup_signal_handler(Arc::clone(&running));

    // Inicia o servidor de socket numa thread separada
    let socket_path = config.daemon.socket_path.clone();
    let state_for_socket = Arc::clone(&state);
    let running_for_socket = Arc::clone(&running);
    thread::spawn(move || {
        if let Err(e) = run_socket_server(&socket_path, state_for_socket, running_for_socket) {
            log::error!("socket server error: {e}");
        }
    });

    // Aplica threshold imediatamente no arranque
    apply_threshold(&battery, &config, &state);

    systemd::notify_ready();

    let interval = Duration::from_secs(config.daemon.interval_secs);

    while running.load(Ordering::Relaxed) {
        thread::sleep(interval);

        if !running.load(Ordering::Relaxed) {
            break;
        }

        apply_threshold(&battery, &config, &state);
        systemd::notify_watchdog();
    }

    log::info!("daemon a terminar");
    Ok(())
}

fn apply_threshold(battery: &Battery, config: &Config, state: &Arc<Mutex<DaemonState>>) {
    let target = effective_threshold(config);

    match battery.status() {
        Ok(status) => {
            log::debug!(
                "bateria: {}% | {} | threshold atual: {:?}",
                status.capacity,
                status.status,
                status.charge_control_end_threshold
            );

            // Só escreve se necessário
            if status.charge_control_end_threshold != Some(target) {
                if battery.supports_threshold() {
                    match battery.set_charge_threshold(target) {
                        Ok(()) => log::info!("threshold definido para {target}%"),
                        Err(e) => log::error!("erro ao definir threshold: {e}"),
                    }
                }
            }

            let ts = now_secs();
            let mut s = state.lock().unwrap();
            s.last_status = Some(status);
            s.last_applied_threshold = Some(target);
            s.last_poll_ts = ts;
            s.error = None;
        }
        Err(e) => {
            log::error!("erro ao ler estado da bateria: {e}");
            let mut s = state.lock().unwrap();
            s.error = Some(e.to_string());
        }
    }
}

/// Determina o threshold efetivo: 100% se hoje for "full charge day", senão o configurado.
fn effective_threshold(config: &Config) -> u8 {
    if config.full_charge.enabled && is_full_charge_day(config) {
        100
    } else {
        config.battery.charge_end_threshold
    }
}

fn is_full_charge_day(config: &Config) -> bool {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Dia da semana: epoch foi quinta-feira (4). (secs / 86400 + 4) % 7
    let weekday = ((secs / 86400) + 4) % 7;
    weekday == config.full_charge.weekday as u64
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ── Unix socket ───────────────────────────────────────────────────────────────

/// Protocolo simples de linha: cliente envia comando, servidor responde em JSON.
fn run_socket_server(
    path: &str,
    state: Arc<Mutex<DaemonState>>,
    running: Arc<AtomicBool>,
) -> Result<(), DaemonError> {
    // Remove socket antigo se existir
    let _ = std::fs::remove_file(path);

    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| DaemonError::Socket(format!("criar diretório socket: {e}")))?;
    }

    let listener = UnixListener::bind(path)
        .map_err(|e| DaemonError::Socket(format!("bind {path}: {e}")))?;

    // Timeout para aceitar conexões, para que possamos verificar `running`
    listener
        .set_nonblocking(true)
        .map_err(|e| DaemonError::Socket(e.to_string()))?;

    log::info!("socket a escutar em {path}");

    while running.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let state = Arc::clone(&state);
                thread::spawn(move || handle_client(stream, state));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(200));
            }
            Err(e) => {
                log::error!("erro socket accept: {e}");
            }
        }
    }

    let _ = std::fs::remove_file(path);
    Ok(())
}

fn handle_client(stream: UnixStream, state: Arc<Mutex<DaemonState>>) {
    let mut reader = BufReader::new(&stream);
    let mut writer = &stream;
    let mut line = String::new();

    if reader.read_line(&mut line).is_err() {
        return;
    }

    let response = match line.trim() {
        "status" => {
            let s = state.lock().unwrap();
            format_status_json(&s)
        }
        "ping" => r#"{"pong":true}"#.to_string(),
        other => format!(r#"{{"error":"unknown command: {other}"}}"#),
    };

    let _ = writer.write_all(response.as_bytes());
    let _ = writer.write_all(b"\n");
}

fn format_status_json(s: &DaemonState) -> String {
    let capacity = s
        .last_status
        .as_ref()
        .map(|st| st.capacity.to_string())
        .unwrap_or_else(|| "null".to_string());
    let status = s
        .last_status
        .as_ref()
        .map(|st| format!("\"{}\"", st.status))
        .unwrap_or_else(|| "null".to_string());
    let threshold = s
        .last_applied_threshold
        .map(|t| t.to_string())
        .unwrap_or_else(|| "null".to_string());
    let error = s
        .error
        .as_deref()
        .map(|e| format!("\"{}\"", e.replace('"', "\\\"")))
        .unwrap_or_else(|| "null".to_string());

    format!(
        r#"{{"capacity":{capacity},"status":{status},"threshold":{threshold},"last_poll":{ts},"error":{error}}}"#,
        ts = s.last_poll_ts,
    )
}

// ── Signal handling ───────────────────────────────────────────────────────────

fn setup_signal_handler(running: Arc<AtomicBool>) {
    // SAFETY: handlers de signal com AtomicBool são o padrão mínimo seguro em Rust.
    // Não fazemos alocação nem chamadas não-reentrantes dentro do handler.
    unsafe {
        libc::signal(libc::SIGTERM, handle_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, handle_signal as *const () as libc::sighandler_t);
    }

    // Guarda o ponteiro globalmente para o handler aceder
    RUNNING_PTR.store(
        Arc::into_raw(running) as *mut (),
        std::sync::atomic::Ordering::SeqCst,
    );
}

static RUNNING_PTR: std::sync::atomic::AtomicPtr<()> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

extern "C" fn handle_signal(_: libc::c_int) {
    let ptr = RUNNING_PTR.load(std::sync::atomic::Ordering::SeqCst);
    if !ptr.is_null() {
        // SAFETY: o ponteiro foi criado por Arc::into_raw e não foi libertado.
        let running = unsafe { &*(ptr as *const AtomicBool) };
        running.store(false, Ordering::Relaxed);
    }
}

// ── Erros ─────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum DaemonError {
    Battery(crate::battery::BatteryError),
    Socket(String),
}

impl std::fmt::Display for DaemonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DaemonError::Battery(e) => write!(f, "battery error: {e}"),
            DaemonError::Socket(s) => write!(f, "socket error: {s}"),
        }
    }
}
