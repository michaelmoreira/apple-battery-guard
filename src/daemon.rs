//! Loop principal do daemon: polling de bateria, aplicação de threshold,
//! Unix socket para comunicação com o CLI.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
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
#[derive(Debug, Clone, Default)]
pub struct DaemonState {
    pub last_status: Option<BatteryStatus>,
    pub last_applied_threshold: Option<u8>,
    pub last_poll_ts: u64,
    pub error: Option<String>,
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

    // SAFETY: o Arc mantém-se vivo no stack de run() por toda a vida do daemon.
    // O handler apenas lê o ponteiro — não há ownership transfer.
    setup_signal_handler(&running);

    // Inicia o servidor de socket numa thread separada
    let socket_path = config.daemon.socket_path.clone();
    let state_for_socket = Arc::clone(&state);
    let running_for_socket = Arc::clone(&running);
    thread::spawn(move || {
        if let Err(e) = run_socket_server(&socket_path, state_for_socket, running_for_socket) {
            log::error!("socket server error: {e}");
        }
    });

    // Monitor udev: notifica o loop principal via apply_now quando há evento power_supply
    // (resume de suspend, ligação/desligação do carregador, etc.)
    let apply_now = Arc::new(AtomicBool::new(false));
    thread::spawn({
        let apply_now = Arc::clone(&apply_now);
        let running = Arc::clone(&running);
        move || run_udev_monitor(apply_now, running)
    });

    // Aplica threshold imediatamente no arranque
    apply_threshold(&battery, &config, &state);

    systemd::notify_ready();

    let interval = Duration::from_secs(config.daemon.interval_secs);

    // Loop principal com sleep granular de 1s para resposta rápida ao shutdown
    'main: loop {
        let mut remaining = interval;
        while remaining > Duration::ZERO {
            if !running.load(Ordering::Acquire) {
                break 'main;
            }
            let step = remaining.min(Duration::from_secs(1));
            thread::sleep(step);
            remaining = remaining.saturating_sub(step);
            // Evento udev recebido: sair do sleep para aplicar threshold imediatamente
            if apply_now.swap(false, Ordering::AcqRel) {
                log::debug!("uevent power_supply: a aplicar threshold imediatamente");
                break;
            }
        }

        if !running.load(Ordering::Acquire) {
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
            if status.charge_control_end_threshold != Some(target) && battery.supports_threshold() {
                match battery.set_charge_threshold(target) {
                    Ok(()) => log::info!("threshold definido para {target}%"),
                    Err(e) => log::error!("erro ao definir threshold: {e}"),
                }
            }

            let ts = now_secs();
            let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
            s.last_status = Some(status);
            s.last_applied_threshold = Some(target);
            s.last_poll_ts = ts;
            s.error = None;
        }
        Err(e) => {
            log::error!("erro ao ler estado da bateria: {e}");
            let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
            s.error = Some(e.to_string());
        }
    }
}

/// Determina o threshold efetivo: 100% se hoje for "full charge day", senão o configurado.
pub(crate) fn effective_threshold(config: &Config) -> u8 {
    if config.full_charge.enabled && is_full_charge_day(config) {
        100
    } else {
        config.battery.charge_end_threshold
    }
}

/// Verifica se hoje é o dia de carga completa, usando hora local do sistema.
pub(crate) fn is_full_charge_day(config: &Config) -> bool {
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as libc::time_t;

    // SAFETY: localtime_r é thread-safe (ao contrário de localtime).
    // tm é inicializado com zeroed() e preenchido pela syscall.
    let weekday = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now_secs, &mut tm);
        tm.tm_wday // 0 = Sunday … 6 = Saturday, coincide com o enum Weekday
    };

    weekday as u64 == config.full_charge.weekday as u64
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ── Monitor udev via netlink ───────────────────────────────────────────────────

/// Escuta eventos do kernel via NETLINK_KOBJECT_UEVENT.
/// Quando deteta `SUBSYSTEM=power_supply`, acende `apply_now` para que o loop
/// principal aplique o threshold imediatamente (ex: após resume de suspend).
///
/// Fallback gracioso: se o socket não puder ser criado (permissões, kernel sem
/// suporte), regista um aviso e retorna — o daemon continua com polling normal.
fn run_udev_monitor(apply_now: Arc<AtomicBool>, running: Arc<AtomicBool>) {
    // SAFETY: socket() retorna um fd gerido por nós; fechado explicitamente no final.
    // SOCK_CLOEXEC evita leaks para processos filhos; SOCK_NONBLOCK evita bloqueio.
    // Protocolo 15 = NETLINK_KOBJECT_UEVENT (constante do kernel, estável desde 2.6).
    let fd = unsafe {
        libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_RAW | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
            libc::NETLINK_KOBJECT_UEVENT,
        )
    };

    if fd < 0 {
        log::warn!(
            "udev monitor: socket() falhou ({}); a usar apenas polling",
            std::io::Error::last_os_error()
        );
        return;
    }

    // SAFETY: sockaddr_nl zeroed é um valor inicial válido; nl_groups=1 é o
    // multicast group para KOBJECT_UEVENT (definido em <linux/netlink.h>).
    let bound = unsafe {
        let mut addr: libc::sockaddr_nl = std::mem::zeroed();
        addr.nl_family = libc::AF_NETLINK as libc::sa_family_t;
        addr.nl_pid = 0;
        addr.nl_groups = 1;
        libc::bind(
            fd,
            &addr as *const libc::sockaddr_nl as *const libc::sockaddr,
            std::mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
        )
    };

    if bound < 0 {
        log::warn!(
            "udev monitor: bind() falhou ({}); a usar apenas polling",
            std::io::Error::last_os_error()
        );
        // SAFETY: fd válido — obtido acima com socket() bem-sucedido.
        unsafe { libc::close(fd) };
        return;
    }

    log::debug!("udev monitor ativo (NETLINK_KOBJECT_UEVENT)");

    let mut buf = [0u8; 4096];

    while running.load(Ordering::Acquire) {
        // SAFETY: recv() com buffer de tamanho fixo; MSG_DONTWAIT + SOCK_NONBLOCK
        // garantem retorno imediato (EAGAIN) quando não há dados pendentes.
        let n = unsafe {
            libc::recv(
                fd,
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
                libc::MSG_DONTWAIT,
            )
        };

        if n < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::WouldBlock {
                thread::sleep(Duration::from_millis(100));
                continue;
            }
            // EINTR — syscall interrompida por sinal, retentar
            if err.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            log::error!("udev monitor: recv() falhou: {err}");
            break;
        }

        // Payload uevent: strings separadas por '\0', ex:
        // "add@/devices/...\0ACTION=add\0SUBSYSTEM=power_supply\0..."
        let has_power_supply = buf[..n as usize]
            .split(|&b| b == 0)
            .any(|token| token == b"SUBSYSTEM=power_supply");

        if has_power_supply {
            log::debug!("udev monitor: evento power_supply detetado");
            apply_now.store(true, Ordering::Release);
        }
    }

    // SAFETY: fd válido e aberto — fechar antes de sair da thread.
    unsafe { libc::close(fd) };
    log::debug!("udev monitor: thread terminada");
}

// ── Cliente de socket (CLI) ────────────────────────────────────────────────────

/// Conecta ao socket do daemon e devolve a resposta JSON de estado.
/// Retorna `None` se o daemon não estiver disponível ou não responder.
pub fn query_socket(socket_path: &str) -> Option<String> {
    let mut stream = UnixStream::connect(socket_path).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    stream.write_all(b"status\n").ok()?;

    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line).ok()?;

    let trimmed = line.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

// ── Unix socket ───────────────────────────────────────────────────────────────

/// Protocolo simples de linha: cliente envia comando, servidor responde em JSON.
fn run_socket_server(
    path: &str,
    state: Arc<Mutex<DaemonState>>,
    running: Arc<AtomicBool>,
) -> Result<(), DaemonError> {
    // Verifica se outra instância já está a correr no mesmo socket
    if Path::new(path).exists() {
        match UnixStream::connect(path) {
            Ok(_) => {
                return Err(DaemonError::Socket(format!(
                    "outra instância do daemon já está ativa (socket '{path}' em uso)"
                )));
            }
            Err(_) => {
                // Socket stale — remover e continuar
                let _ = std::fs::remove_file(path);
            }
        }
    }

    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| DaemonError::Socket(format!("criar diretório socket: {e}")))?;
    }

    let listener =
        UnixListener::bind(path).map_err(|e| DaemonError::Socket(format!("bind {path}: {e}")))?;

    // Restringir acesso ao socket (owner + group apenas)
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o660))
        .map_err(|e| DaemonError::Socket(format!("chmod socket: {e}")))?;

    listener
        .set_nonblocking(true)
        .map_err(|e| DaemonError::Socket(e.to_string()))?;

    log::info!("socket a escutar em {path}");

    while running.load(Ordering::Acquire) {
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
    // Timeout de leitura para evitar threads suspensas indefinidamente
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));

    // Limite de 4 KB para prevenir DoS por payload ilimitado
    let limited = (&stream).take(4096);
    let mut reader = BufReader::new(limited);
    let mut writer = &stream;
    let mut line = String::new();

    if reader.read_line(&mut line).is_err() {
        return;
    }

    let response = match line.trim() {
        "status" => {
            let s = state.lock().unwrap_or_else(|e| e.into_inner());
            format_status_json(&s)
        }
        "ping" => r#"{"pong":true}"#.to_string(),
        other => format!(r#"{{"error":"unknown command: {}"}}"#, json_escape(other)),
    };

    let _ = writer.write_all(response.as_bytes());
    let _ = writer.write_all(b"\n");
}

/// Escapa uma string para inclusão segura num valor JSON.
fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
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
        .map(|st| format!("\"{}\"", json_escape(&st.status)))
        .unwrap_or_else(|| "null".to_string());
    let threshold = s
        .last_applied_threshold
        .map(|t| t.to_string())
        .unwrap_or_else(|| "null".to_string());
    let error = s
        .error
        .as_deref()
        .map(|e| format!("\"{}\"", json_escape(e)))
        .unwrap_or_else(|| "null".to_string());

    format!(
        r#"{{"capacity":{capacity},"status":{status},"threshold":{threshold},"last_poll":{ts},"error":{error}}}"#,
        ts = s.last_poll_ts,
    )
}

// ── Signal handling ───────────────────────────────────────────────────────────

/// Ponteiro não-owning para o AtomicBool de controlo do daemon.
/// O Arc que o contém vive no stack de `run()` pelo tempo de vida do processo.
static RUNNING_PTR: std::sync::atomic::AtomicPtr<AtomicBool> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

fn setup_signal_handler(running: &Arc<AtomicBool>) {
    // Guardar ponteiro não-owning — o Arc permanece vivo em `run()`
    RUNNING_PTR.store(Arc::as_ptr(running) as *mut AtomicBool, Ordering::SeqCst);

    // SAFETY: sigaction é a API POSIX correta para instalar handlers persistentes.
    // SA_RESTART: evita EINTR em syscalls lentas.
    // O handler apenas escreve num AtomicBool — async-signal-safe.
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = handle_signal as *const () as usize;
        sa.sa_flags = libc::SA_RESTART;
        libc::sigemptyset(&mut sa.sa_mask);

        libc::sigaction(libc::SIGTERM, &sa, std::ptr::null_mut());
        libc::sigaction(libc::SIGINT, &sa, std::ptr::null_mut());
    }
}

extern "C" fn handle_signal(_: libc::c_int) {
    let ptr = RUNNING_PTR.load(Ordering::SeqCst);
    if !ptr.is_null() {
        // SAFETY: o ponteiro é válido enquanto `run()` estiver no stack.
        unsafe { (*ptr).store(false, Ordering::Release) };
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

impl std::error::Error for DaemonError {}

// ── Testes ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, FullChargeConfig, Weekday};

    fn config_with_threshold(t: u8) -> Config {
        let mut cfg = Config::default();
        cfg.battery.charge_end_threshold = t;
        cfg
    }

    #[test]
    fn effective_threshold_normal() {
        let cfg = config_with_threshold(80);
        assert_eq!(effective_threshold(&cfg), 80);
    }

    #[test]
    fn effective_threshold_full_charge_day_disabled() {
        let mut cfg = config_with_threshold(80);
        cfg.full_charge = FullChargeConfig {
            enabled: false,
            weekday: Weekday::Sunday,
        };
        // Mesmo que hoje seja domingo, full_charge está desativado
        assert_eq!(effective_threshold(&cfg), 80);
    }

    #[test]
    fn json_escape_handles_quotes_and_backslashes() {
        assert_eq!(json_escape(r#"say "hi""#), r#"say \"hi\""#);
        assert_eq!(json_escape(r"path\to"), r"path\\to");
        assert_eq!(json_escape(r#"a\"b"#), r#"a\\\"b"#);
    }

    #[test]
    fn format_status_json_with_data() {
        let state = DaemonState {
            last_status: Some(BatteryStatus {
                capacity: 75,
                status: "Discharging".to_string(),
                charge_control_end_threshold: Some(80),
            }),
            last_applied_threshold: Some(80),
            last_poll_ts: 1_000_000,
            error: None,
        };
        let json = format_status_json(&state);
        assert!(json.contains(r#""capacity":75"#));
        assert!(json.contains(r#""status":"Discharging""#));
        assert!(json.contains(r#""threshold":80"#));
        assert!(json.contains(r#""error":null"#));
    }

    #[test]
    fn format_status_json_empty_state() {
        let state = DaemonState::default();
        let json = format_status_json(&state);
        assert!(json.contains(r#""capacity":null"#));
        assert!(json.contains(r#""status":null"#));
        assert!(json.contains(r#""threshold":null"#));
    }

    #[test]
    fn uevent_power_supply_detected() {
        let payload = b"add@/devices/LNXSYSTM:00\0ACTION=add\0SUBSYSTEM=power_supply\0DEVPATH=/power_supply/BAT0\0";
        let found = payload
            .split(|&b| b == 0)
            .any(|token| token == b"SUBSYSTEM=power_supply");
        assert!(found);
    }

    #[test]
    fn uevent_unrelated_subsystem_ignored() {
        let payload = b"change@/devices/pci0000:00\0ACTION=change\0SUBSYSTEM=pci\0";
        let found = payload
            .split(|&b| b == 0)
            .any(|token| token == b"SUBSYSTEM=power_supply");
        assert!(!found);
    }

    #[test]
    fn format_status_json_escapes_special_chars_in_status() {
        let state = DaemonState {
            last_status: Some(BatteryStatus {
                capacity: 50,
                status: r#"Strange "status""#.to_string(),
                charge_control_end_threshold: None,
            }),
            last_applied_threshold: None,
            last_poll_ts: 0,
            error: None,
        };
        let json = format_status_json(&state);
        // Deve conter o status com aspas escapadas
        assert!(json.contains(r#"\"status\""#));
    }
}
