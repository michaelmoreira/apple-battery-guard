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
            log::info!("a iniciar daemon (threshold={}%)", cfg.battery.charge_end_threshold);
            if let Err(e) = daemon::run(cfg) {
                eprintln!("Erro no daemon: {e}");
                std::process::exit(1);
            }
        }

        Command::Status => {
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

        Command::Set { threshold } => {
            match battery::Battery::detect() {
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
            }
        }
    }
}
