mod battery;
mod config;
mod daemon;
mod systemd;
mod tui;

use clap::{Parser, Subcommand};
use config::{Config, DEFAULT_CONFIG_PATH};

#[derive(Parser)]
#[command(
    name = "abg",
    about = "Apple Battery Guard — gestão de charge threshold para MacBooks com Linux",
    version
)]
struct Cli {
    /// Caminho para o ficheiro de configuração
    #[arg(short, long, default_value = DEFAULT_CONFIG_PATH)]
    config: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Inicia o daemon em foreground
    Daemon,

    /// Mostra o estado atual da bateria
    Status,

    /// Abre o dashboard TUI
    Tui,

    /// Mostra a configuração efetiva
    Config,

    /// Define o threshold manualmente (requer root ou polkit)
    Set {
        /// Percentagem (1–100)
        threshold: u8,
    },
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();
    let cfg = Config::load_or_default(&cli.config);

    if let Err(e) = cfg.validate() {
        eprintln!("Configuração inválida: {e}");
        std::process::exit(1);
    }

    match cli.command {
        Command::Daemon => {
            log::info!(
                "a iniciar daemon (threshold={}%)",
                cfg.battery.charge_end_threshold
            );
            if let Err(e) = daemon::run(cfg) {
                eprintln!("Erro no daemon: {e}");
                std::process::exit(1);
            }
        }

        Command::Status => {
            // Tenta ler estado do daemon via socket (não requer root)
            if let Some(json) = daemon::query_socket(&cfg.daemon.socket_path) {
                if let Some((cap, st, thr)) = parse_status_json(&json) {
                    println!("Bateria:   {cap}%");
                    println!("Estado:    {st}");
                    println!("Threshold: {thr}%");
                } else {
                    // JSON recebido mas campos ausentes (daemon a arrancar?) — sysfs fallback
                    read_status_from_sysfs();
                }
            } else {
                // Daemon não disponível — leitura direta do sysfs
                read_status_from_sysfs();
            }
        }

        Command::Tui => {
            if let Err(e) = tui::run_tui() {
                eprintln!("Erro no TUI: {e}");
                std::process::exit(1);
            }
        }

        Command::Config => {
            println!("Ficheiro: {}", cli.config);
            println!("Threshold normal:  {}%", cfg.battery.charge_end_threshold);
            println!("Intervalo polling: {}s", cfg.daemon.interval_secs);
            println!("Socket:            {}", cfg.daemon.socket_path);
            println!(
                "Full charge day:   {} ({})",
                cfg.full_charge.enabled,
                if cfg.full_charge.enabled {
                    format!("{:?}", cfg.full_charge.weekday)
                } else {
                    "desativado".to_string()
                }
            );
        }

        Command::Set { threshold } => match battery::Battery::detect() {
            Ok(bat) => match bat.set_charge_threshold(threshold) {
                Ok(()) => println!("Threshold definido para {threshold}%"),
                Err(e) => {
                    eprintln!("Erro ao definir threshold: {e}");
                    std::process::exit(1);
                }
            },
            Err(e) => {
                eprintln!("Bateria não detetada: {e}");
                std::process::exit(1);
            }
        },
    }
}

/// Lê o estado da bateria diretamente do sysfs e imprime no formato padrão.
/// Usado como fallback quando o daemon não está disponível.
fn read_status_from_sysfs() {
    match battery::Battery::detect() {
        Ok(bat) => match bat.status() {
            Ok(s) => {
                println!("Bateria:   {}%", s.capacity);
                println!("Estado:    {}", s.status);
                match s.charge_control_end_threshold {
                    Some(t) => println!("Threshold: {t}%"),
                    None => println!("Threshold: não suportado pelo kernel"),
                }
            }
            Err(e) => {
                eprintln!("Erro ao ler bateria: {e}");
                std::process::exit(1);
            }
        },
        Err(e) => {
            eprintln!("Bateria não detetada: {e}");
            std::process::exit(1);
        }
    }
}

/// Extrai (capacity, status, threshold) do JSON de estado do daemon.
/// Formato esperado: `{"capacity":75,"status":"Discharging","threshold":80,...}`
/// Implementação sem serde_json para manter zero dependências desnecessárias.
fn parse_status_json(json: &str) -> Option<(u8, String, u8)> {
    fn extract_num(json: &str, key: &str) -> Option<u8> {
        let needle = format!("\"{}\":", key);
        let start = json.find(&needle)? + needle.len();
        let rest = json[start..].trim_start();
        if rest.starts_with("null") {
            return None;
        }
        let end = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        if end == 0 {
            return None;
        }
        rest[..end].parse().ok()
    }

    fn extract_str(json: &str, key: &str) -> Option<String> {
        let needle = format!("\"{}\":\"", key);
        let start = json.find(&needle)? + needle.len();
        let mut result = String::new();
        let mut chars = json[start..].chars();
        loop {
            match chars.next()? {
                '"' => break,
                '\\' => match chars.next()? {
                    '"' => result.push('"'),
                    '\\' => result.push('\\'),
                    c => {
                        result.push('\\');
                        result.push(c);
                    }
                },
                c => result.push(c),
            }
        }
        Some(result)
    }

    let capacity = extract_num(json, "capacity")?;
    let status = extract_str(json, "status")?;
    let threshold = extract_num(json, "threshold")?;
    Some((capacity, status, threshold))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_status_json_normal() {
        let json = r#"{"capacity":75,"status":"Discharging","threshold":80,"last_poll":1000000,"error":null}"#;
        let result = parse_status_json(json).unwrap();
        assert_eq!(result.0, 75);
        assert_eq!(result.1, "Discharging");
        assert_eq!(result.2, 80);
    }

    #[test]
    fn parse_status_json_null_capacity_returns_none() {
        let json = r#"{"capacity":null,"status":null,"threshold":null,"last_poll":0,"error":null}"#;
        assert!(parse_status_json(json).is_none());
    }

    #[test]
    fn parse_status_json_escaped_status() {
        let json = r#"{"capacity":50,"status":"Not \"charging\"","threshold":80,"last_poll":0,"error":null}"#;
        let result = parse_status_json(json).unwrap();
        assert_eq!(result.1, r#"Not "charging""#);
    }
}
