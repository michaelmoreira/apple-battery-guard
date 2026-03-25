//! Leitura e escrita de atributos de bateria via sysfs.
//!
//! Toda a interação com sysfs é fallback-safe: erros de I/O são retornados
//! como `BatteryError` sem crashar o processo.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

const SYSFS_POWER_SUPPLY: &str = "/sys/class/power_supply";

#[derive(Debug)]
pub enum BatteryError {
    NotFound(String),
    Parse(String),
    Io(std::io::Error),
    ThresholdOutOfRange(u8),
}

impl fmt::Display for BatteryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BatteryError::NotFound(p) => write!(f, "sysfs path not found: {p}"),
            BatteryError::Parse(s) => write!(f, "failed to parse value: {s}"),
            BatteryError::Io(e) => write!(f, "I/O error: {e}"),
            BatteryError::ThresholdOutOfRange(v) => {
                write!(f, "threshold {v}% out of range (1–100)")
            }
        }
    }
}

impl std::error::Error for BatteryError {}

impl From<std::io::Error> for BatteryError {
    fn from(e: std::io::Error) -> Self {
        BatteryError::Io(e)
    }
}

/// Representa a bateria encontrada no sysfs.
#[derive(Debug, Clone)]
pub struct Battery {
    #[allow(dead_code)] // campo público da API; lido externamente (ex: TUI, CLI)
    pub name: String,
    base_path: PathBuf,
}

/// Estado atual da bateria.
#[derive(Debug, Clone, PartialEq)]
pub struct BatteryStatus {
    /// Percentagem de carga atual (0–100).
    pub capacity: u8,
    /// "Charging", "Discharging", "Full", "Not charging", "Unknown"
    pub status: String,
    /// Threshold de fim de carga atualmente configurado, se suportado.
    pub charge_control_end_threshold: Option<u8>,
}

impl Battery {
    /// Deteta automaticamente a primeira bateria disponível (BAT0, BAT1, …).
    pub fn detect() -> Result<Self, BatteryError> {
        Self::detect_in(SYSFS_POWER_SUPPLY)
    }

    /// Versão testável: permite injetar o diretório base.
    pub fn detect_in(base: impl AsRef<Path>) -> Result<Self, BatteryError> {
        let base = base.as_ref();
        let entries = fs::read_dir(base).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                BatteryError::NotFound(base.display().to_string())
            } else {
                BatteryError::Io(e)
            }
        })?;

        // Ordenar entradas BAT* por nome para deteção determinística (BAT0 antes de BAT1)
        let mut bat_entries: Vec<_> = entries
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("BAT"))
            .collect();
        bat_entries.sort_by_key(|e| e.file_name());

        for entry in bat_entries {
            let name = entry.file_name().to_string_lossy().to_string();
            let path = entry.path();
            // Verifica que tem o atributo `type = Battery`
            let type_path = path.join("type");
            if let Ok(t) = fs::read_to_string(&type_path) {
                if t.trim().eq_ignore_ascii_case("battery") {
                    return Ok(Battery {
                        name,
                        base_path: path,
                    });
                }
            }
        }

        Err(BatteryError::NotFound(format!(
            "no battery found under {}",
            base.display()
        )))
    }

    /// Lê o estado atual da bateria.
    pub fn status(&self) -> Result<BatteryStatus, BatteryError> {
        let capacity = self.read_u8("capacity")?;
        let status = self.read_string("status")?;
        let threshold = self.read_u8("charge_control_end_threshold").ok();

        Ok(BatteryStatus {
            capacity,
            status,
            charge_control_end_threshold: threshold,
        })
    }

    /// Define o threshold de fim de carga (1–100).
    pub fn set_charge_threshold(&self, pct: u8) -> Result<(), BatteryError> {
        if pct == 0 || pct > 100 {
            return Err(BatteryError::ThresholdOutOfRange(pct));
        }
        let path = self.base_path.join("charge_control_end_threshold");
        // Verificação explícita: o ficheiro não existir significa kernel sem suporte,
        // não um erro de I/O transitório. Em sysfs a existência é controlada pelo kernel.
        if !path.exists() {
            return Err(BatteryError::NotFound(path.display().to_string()));
        }
        fs::write(&path, format!("{pct}\n")).map_err(BatteryError::Io)
    }

    /// Verifica se o kernel suporta `charge_control_end_threshold`.
    pub fn supports_threshold(&self) -> bool {
        self.base_path
            .join("charge_control_end_threshold")
            .exists()
    }

    // ── helpers internos ──────────────────────────────────────────────────────

    fn read_string(&self, attr: &str) -> Result<String, BatteryError> {
        let path = self.base_path.join(attr);
        fs::read_to_string(&path)
            .map(|s| s.trim().to_string())
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    BatteryError::NotFound(path.display().to_string())
                } else {
                    BatteryError::Io(e)
                }
            })
    }

    fn read_u8(&self, attr: &str) -> Result<u8, BatteryError> {
        let s = self.read_string(attr)?;
        s.parse::<u8>()
            .map_err(|_| BatteryError::Parse(format!("{attr}={s:?}")))
    }
}

// ── Testes ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn fake_battery(dir: &TempDir, name: &str, capacity: u8, status: &str, threshold: Option<u8>) -> PathBuf {
        let bat = dir.path().join(name);
        fs::create_dir_all(&bat).unwrap();
        fs::write(bat.join("type"), "Battery\n").unwrap();
        fs::write(bat.join("capacity"), format!("{capacity}\n")).unwrap();
        fs::write(bat.join("status"), format!("{status}\n")).unwrap();
        if let Some(t) = threshold {
            fs::write(bat.join("charge_control_end_threshold"), format!("{t}\n")).unwrap();
        }
        bat
    }

    #[test]
    fn detect_finds_battery() {
        let dir = TempDir::new().unwrap();
        fake_battery(&dir, "BAT0", 75, "Discharging", Some(80));
        let bat = Battery::detect_in(dir.path()).unwrap();
        assert_eq!(bat.name, "BAT0");
    }

    #[test]
    fn detect_ignores_non_battery_entries() {
        let dir = TempDir::new().unwrap();
        // AC adapter — não deve ser detetado como bateria
        let ac = dir.path().join("AC0");
        fs::create_dir_all(&ac).unwrap();
        fs::write(ac.join("type"), "Mains\n").unwrap();

        let result = Battery::detect_in(dir.path());
        assert!(matches!(result, Err(BatteryError::NotFound(_))));
    }

    #[test]
    fn detect_prefers_bat0_over_bat1() {
        let dir = TempDir::new().unwrap();
        fake_battery(&dir, "BAT1", 60, "Discharging", None);
        fake_battery(&dir, "BAT0", 75, "Charging", Some(80));
        let bat = Battery::detect_in(dir.path()).unwrap();
        assert_eq!(bat.name, "BAT0");
    }

    #[test]
    fn status_reads_all_fields() {
        let dir = TempDir::new().unwrap();
        fake_battery(&dir, "BAT0", 82, "Charging", Some(80));
        let bat = Battery::detect_in(dir.path()).unwrap();
        let s = bat.status().unwrap();
        assert_eq!(s.capacity, 82);
        assert_eq!(s.status, "Charging");
        assert_eq!(s.charge_control_end_threshold, Some(80));
    }

    #[test]
    fn status_without_threshold_support() {
        let dir = TempDir::new().unwrap();
        fake_battery(&dir, "BAT0", 55, "Discharging", None);
        let bat = Battery::detect_in(dir.path()).unwrap();
        let s = bat.status().unwrap();
        assert_eq!(s.charge_control_end_threshold, None);
    }

    #[test]
    fn set_threshold_writes_value() {
        let dir = TempDir::new().unwrap();
        fake_battery(&dir, "BAT0", 60, "Charging", Some(100));
        let bat = Battery::detect_in(dir.path()).unwrap();
        bat.set_charge_threshold(80).unwrap();
        let written = fs::read_to_string(bat.base_path.join("charge_control_end_threshold")).unwrap();
        assert_eq!(written.trim(), "80");
    }

    #[test]
    fn set_threshold_rejects_zero() {
        let dir = TempDir::new().unwrap();
        fake_battery(&dir, "BAT0", 60, "Charging", Some(100));
        let bat = Battery::detect_in(dir.path()).unwrap();
        let err = bat.set_charge_threshold(0).unwrap_err();
        assert!(matches!(err, BatteryError::ThresholdOutOfRange(0)));
    }

    #[test]
    fn set_threshold_rejects_above_100() {
        let dir = TempDir::new().unwrap();
        fake_battery(&dir, "BAT0", 60, "Charging", Some(100));
        let bat = Battery::detect_in(dir.path()).unwrap();
        let err = bat.set_charge_threshold(101).unwrap_err();
        assert!(matches!(err, BatteryError::ThresholdOutOfRange(101)));
    }

    #[test]
    fn set_threshold_fails_without_sysfs_file() {
        let dir = TempDir::new().unwrap();
        // sem o ficheiro charge_control_end_threshold
        fake_battery(&dir, "BAT0", 60, "Charging", None);
        let bat = Battery::detect_in(dir.path()).unwrap();
        let err = bat.set_charge_threshold(80).unwrap_err();
        assert!(matches!(err, BatteryError::NotFound(_)));
    }

    #[test]
    fn supports_threshold_true_when_file_exists() {
        let dir = TempDir::new().unwrap();
        fake_battery(&dir, "BAT0", 60, "Charging", Some(80));
        let bat = Battery::detect_in(dir.path()).unwrap();
        assert!(bat.supports_threshold());
    }

    #[test]
    fn supports_threshold_false_when_file_missing() {
        let dir = TempDir::new().unwrap();
        fake_battery(&dir, "BAT0", 60, "Charging", None);
        let bat = Battery::detect_in(dir.path()).unwrap();
        assert!(!bat.supports_threshold());
    }
}
