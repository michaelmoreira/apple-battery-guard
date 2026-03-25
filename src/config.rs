//! Configuração do apple-battery-guard via TOML.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_CONFIG_PATH: &str = "/etc/apple-battery-guard/apple-battery-guard.toml";

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct Config {
    #[serde(default)]
    pub battery: BatteryConfig,

    #[serde(default)]
    pub daemon: DaemonConfig,

    #[serde(default)]
    pub full_charge: FullChargeConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct BatteryConfig {
    /// Threshold de fim de carga normal (1–100). Default: 80.
    pub charge_end_threshold: u8,
}

impl Default for BatteryConfig {
    fn default() -> Self {
        Self {
            charge_end_threshold: 80,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct DaemonConfig {
    /// Intervalo de polling em segundos. Default: 30.
    pub interval_secs: u64,
    /// Caminho do Unix socket para o CLI. Default: /run/apple-battery-guard/daemon.sock
    pub socket_path: String,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            interval_secs: 30,
            socket_path: "/run/apple-battery-guard/daemon.sock".to_string(),
        }
    }
}

/// Dia da semana (0 = Domingo … 6 = Sábado).
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Weekday {
    Sunday = 0,
    Monday = 1,
    Tuesday = 2,
    Wednesday = 3,
    Thursday = 4,
    Friday = 5,
    Saturday = 6,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct FullChargeConfig {
    /// Ativar "full charge day". Default: false.
    pub enabled: bool,
    /// Dia da semana para carregar a 100%. Default: Sunday.
    pub weekday: Weekday,
}

impl Default for FullChargeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            weekday: Weekday::Sunday,
        }
    }
}

impl Config {
    /// Carrega config a partir de um ficheiro TOML.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let raw = fs::read_to_string(path).map_err(|e| ConfigError::Io {
            path: path.to_owned(),
            source: e,
        })?;
        Self::from_str(&raw)
    }

    /// Carrega a partir de string TOML (útil em testes).
    pub fn from_str(s: &str) -> Result<Self, ConfigError> {
        toml::from_str(s).map_err(|e| ConfigError::Parse(e.to_string()))
    }

    /// Tenta carregar do caminho default; se não existir ou falhar, devolve Config::default().
    /// Em caso de erro de parse (ficheiro existe mas TOML inválido), emite um aviso no log.
    pub fn load_or_default(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        if !path.exists() {
            return Self::default();
        }
        match Self::load(path) {
            Ok(cfg) => cfg,
            Err(e) => {
                log::warn!(
                    "falha ao carregar configuração de '{}': {e} — a usar valores predefinidos",
                    path.display()
                );
                Self::default()
            }
        }
    }

    /// Serializa para TOML e escreve no ficheiro.
    #[allow(dead_code)] // usado em testes e futura subcomando `abg config save`
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), ConfigError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| ConfigError::Io {
                path: parent.to_owned(),
                source: e,
            })?;
        }
        let s = toml::to_string_pretty(self).map_err(|e| ConfigError::Serialize(e.to_string()))?;
        fs::write(path, s).map_err(|e| ConfigError::Io {
            path: path.to_owned(),
            source: e,
        })?;
        Ok(())
    }

    /// Valida os valores da configuração.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let t = self.battery.charge_end_threshold;
        if t == 0 || t > 100 {
            return Err(ConfigError::Validation(format!(
                "charge_end_threshold {t} fora do intervalo válido (1–100)"
            )));
        }
        if self.daemon.interval_secs == 0 {
            return Err(ConfigError::Validation(
                "interval_secs deve ser > 0".to_string(),
            ));
        }
        if self.daemon.socket_path.is_empty() {
            return Err(ConfigError::Validation(
                "socket_path não pode ser vazio".to_string(),
            ));
        }
        Ok(())
    }
}


#[allow(dead_code)] // Serialize só é construído por Config::save
pub enum ConfigError {
    Io { path: PathBuf, source: std::io::Error },
    Parse(String),
    Serialize(String),
    Validation(String),
}

impl std::fmt::Debug for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io { path, source } => {
                write!(f, "I/O error on {}: {source}", path.display())
            }
            ConfigError::Parse(s) => write!(f, "TOML parse error: {s}"),
            ConfigError::Serialize(s) => write!(f, "TOML serialize error: {s}"),
            ConfigError::Validation(s) => write!(f, "config validation error: {s}"),
        }
    }
}

impl std::error::Error for ConfigError {}

// ── Testes ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn defaults_are_sane() {
        let cfg = Config::default();
        assert_eq!(cfg.battery.charge_end_threshold, 80);
        assert_eq!(cfg.daemon.interval_secs, 30);
        assert!(!cfg.full_charge.enabled);
        cfg.validate().unwrap();
    }

    #[test]
    fn parse_full_toml() {
        let toml = r#"
            [battery]
            charge_end_threshold = 85

            [daemon]
            interval_secs = 60
            socket_path = "/tmp/abg.sock"

            [full_charge]
            enabled = true
            weekday = "friday"
        "#;
        let cfg = Config::from_str(toml).unwrap();
        assert_eq!(cfg.battery.charge_end_threshold, 85);
        assert_eq!(cfg.daemon.interval_secs, 60);
        assert_eq!(cfg.daemon.socket_path, "/tmp/abg.sock");
        assert!(cfg.full_charge.enabled);
        assert_eq!(cfg.full_charge.weekday, Weekday::Friday);
    }

    #[test]
    fn partial_toml_uses_defaults() {
        let toml = r#"
            [battery]
            charge_end_threshold = 90
        "#;
        let cfg = Config::from_str(toml).unwrap();
        assert_eq!(cfg.battery.charge_end_threshold, 90);
        // campos não presentes usam defaults
        assert_eq!(cfg.daemon.interval_secs, 30);
        assert!(!cfg.full_charge.enabled);
    }

    #[test]
    fn empty_toml_uses_all_defaults() {
        let cfg = Config::from_str("").unwrap();
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn invalid_toml_returns_error() {
        let err = Config::from_str("[[[[invalid").unwrap_err();
        assert!(matches!(err, ConfigError::Parse(_)));
    }

    #[test]
    fn validation_rejects_zero_threshold() {
        let mut cfg = Config::default();
        cfg.battery.charge_end_threshold = 0;
        let err = cfg.validate().unwrap_err();
        assert!(matches!(err, ConfigError::Validation(_)));
    }

    #[test]
    fn validation_rejects_threshold_above_100() {
        let mut cfg = Config::default();
        cfg.battery.charge_end_threshold = 101;
        assert!(matches!(cfg.validate(), Err(ConfigError::Validation(_))));
    }

    #[test]
    fn validation_rejects_zero_interval() {
        let mut cfg = Config::default();
        cfg.daemon.interval_secs = 0;
        assert!(matches!(cfg.validate(), Err(ConfigError::Validation(_))));
    }

    #[test]
    fn validation_rejects_empty_socket_path() {
        let mut cfg = Config::default();
        cfg.daemon.socket_path = String::new();
        assert!(matches!(cfg.validate(), Err(ConfigError::Validation(_))));
    }

    #[test]
    fn save_and_reload_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");

        let mut original = Config::default();
        original.battery.charge_end_threshold = 75;
        original.full_charge.enabled = true;
        original.full_charge.weekday = Weekday::Wednesday;

        original.save(&path).unwrap();
        let loaded = Config::load(&path).unwrap();
        assert_eq!(original, loaded);
    }

    #[test]
    fn load_or_default_returns_default_when_missing() {
        let cfg = Config::load_or_default("/tmp/this_file_does_not_exist_abg.toml");
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn load_or_default_returns_default_on_invalid_toml() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bad.toml");
        std::fs::write(&path, "[[[[not valid toml").unwrap();
        // Deve retornar defaults sem panic (emite log::warn internamente)
        let cfg = Config::load_or_default(&path);
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn load_missing_file_returns_error() {
        let err = Config::load("/tmp/this_file_does_not_exist_abg.toml").unwrap_err();
        assert!(matches!(err, ConfigError::Io { .. }));
    }
}
