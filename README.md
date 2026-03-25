# apple-battery-guard

[🇵🇹 Português](README.pt.md) | 🇬🇧 English

Intelligent battery charge threshold manager for **Intel MacBooks running Linux**.

MacBooks on Linux charge the battery to 100% every time, prematurely degrading it. macOS automatically limits charging to 80% via the Apple SMC. This project replicates that behavior on Linux through sysfs — no kernel patches, no heavy dependencies.

Tested on a **2017 MacBook Air (Intel) running Manjaro**. Works on any systemd-based distribution.

---

## Table of Contents

- [The Problem](#the-problem)
- [How It Works](#how-it-works)
- [Requirements](#requirements)
- [Installation](#installation)
  - [Arch Linux / Manjaro (AUR)](#arch-linux--manjaro-aur)
  - [Other Distributions (Build from Source)](#other-distributions-build-from-source)
- [Configuration](#configuration)
- [Usage](#usage)
- [systemd Service](#systemd-service)
- [Architecture](#architecture)
- [Development](#development)
- [FAQ](#faq)

---

## The Problem

| System | Default behavior | Battery longevity |
|---|---|---|
| macOS | Limits charging to 80% via SMC | Preserved |
| Linux (without this project) | Always charges to 100% | Accelerated degradation |
| Linux (with apple-battery-guard) | Limits to configured threshold | Preserved |

Constantly charging to 100% subjects lithium cells to maximum voltage, accelerating electrochemical degradation. Keeping charge between 20–80% can **double the useful cycle count** of the battery.

The `applesmc` driver on recent kernels (≥ 5.4) exposes the `charge_control_end_threshold` sysfs file, but no daemon manages it intelligently — until now.

---

## How It Works

```
┌─────────────────────────────────────────────┐
│                  abg daemon                  │
│                                              │
│  ┌──────────┐    ┌──────────┐  ┌─────────┐  │
│  │ scheduler│───▶│ battery  │─▶│  sysfs  │  │
│  │  (30s)   │    │  module  │  │ /sys/.. │  │
│  └──────────┘    └──────────┘  └─────────┘  │
│                                              │
│  ┌──────────────────────────────────────┐   │
│  │         Unix socket server            │   │
│  │  /run/apple-battery-guard/daemon.sock │   │
│  └──────────────────────────────────────┘   │
└──────────────────┬──────────────────────────┘
                   │ sd_notify / watchdog
              ┌────▼─────┐
              │ systemd   │
              └──────────┘
```

1. The daemon starts and **immediately applies** the configured threshold.
2. Every 30 seconds (configurable) it checks whether the threshold is correct and reapplies if needed.
3. It communicates with systemd via `sd_notify` (Type=notify + watchdog).
4. It exposes current state via a **Unix socket** — the CLI reads from it without requiring root.
5. It supports a **"full charge day"**: once a week the battery charges to 100% for calibration.

---

## Requirements

### Hardware
- Intel MacBook (Air, Pro, Mini — any Intel model)
- Tested on MacBook Air 2017

### Software
- Linux with systemd
- Kernel ≥ 5.4 with `charge_control_end_threshold` support **or** the `applesmc-next` DKMS module
- Rust ≥ 1.70 (only required to build from source)

### Check Kernel Support

```bash
# If this returns a number (e.g. 80), your kernel already supports it
cat /sys/class/power_supply/BAT0/charge_control_end_threshold

# If "No such file or directory", install applesmc-next:
# Arch/Manjaro: yay -S applesmc-next-dkms
```

---

## Installation

### Arch Linux / Manjaro (AUR)

```bash
# With yay
yay -S apple-battery-guard

# With paru
paru -S apple-battery-guard

# Manual
git clone https://aur.archlinux.org/apple-battery-guard.git
cd apple-battery-guard
makepkg -si
```

The AUR package includes:
- The `abg` binary at `/usr/bin/`
- A sample config at `/etc/apple-battery-guard/apple-battery-guard.toml`
- A systemd service at `/usr/lib/systemd/system/apple-battery-guard.service`

### Other Distributions (Build from Source)

```bash
# 1. Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 2. Clone and build
git clone https://github.com/michaelmoreira/apple-battery-guard.git
cd apple-battery-guard
cargo build --release

# 3. Install
sudo install -Dm755 target/release/abg /usr/local/bin/abg
sudo install -Dm644 config/apple-battery-guard.toml \
    /etc/apple-battery-guard/apple-battery-guard.toml
sudo install -Dm644 systemd/apple-battery-guard.service \
    /etc/systemd/system/apple-battery-guard.service

# 4. Grant write access to sysfs (no permanent root required)
echo 'ACTION=="add", SUBSYSTEM=="power_supply", KERNEL=="BAT[0-9]", \
  RUN+="/bin/chmod 666 /sys%p/charge_control_end_threshold"' \
  | sudo tee /etc/udev/rules.d/99-battery-threshold.rules
sudo udevadm control --reload-rules && sudo udevadm trigger
```

---

## Configuration

File: `/etc/apple-battery-guard/apple-battery-guard.toml`

```toml
[battery]
# Charge end threshold under normal conditions (1–100).
# 80% is the recommended value to maximize battery longevity.
charge_end_threshold = 80

[daemon]
# Polling interval in seconds.
interval_secs = 30

# Unix socket path for CLI communication.
socket_path = "/run/apple-battery-guard/daemon.sock"

[full_charge]
# "Full charge day": charge to 100% once a week.
# Useful for calibration and days with heavy use away from a power source.
enabled = false

# Day of the week: sunday, monday, tuesday, wednesday, thursday, friday, saturday
weekday = "sunday"
```

### Recommended Threshold Values

| Use case | Threshold | Notes |
|---|---|---|
| Sedentary use (always plugged in) | 60–70% | Maximum longevity |
| Mixed use | 80% | Recommended (default) |
| Frequent mobile use | 90% | More runtime, less durability |
| Calibration / travel | 100% | Temporary via full_charge_day |

---

## Usage

```bash
# Show current battery status
abg status

# Example output:
# Battery:   75%
# Status:    Discharging
# Threshold: 80%

# Set threshold manually (requires sysfs write permission)
abg set 80

# Show effective configuration
abg config

# Open the interactive TUI dashboard
abg tui

# Start the daemon in the foreground (normally managed by systemd)
abg daemon

# Use an alternate config file
abg --config ~/.config/abg.toml status
```

### TUI Dashboard

The `abg tui` command opens an interactive terminal dashboard:

```
╔══════════════════════════════════════╗
║         apple-battery-guard          ║
╠══════════════════════════════════════╣
║  Charge                              ║
║  ████████████████████░░░░  75%       ║
╠══════════════════════════════════════╣
║  Status: Discharging  Threshold: 80% ║
╚══════════════════════════════════════╝
  q / Esc: quit
```

Refreshes every 5 seconds. Press `q` or `Esc` to quit.

---

## systemd Service

```bash
# Enable and start
sudo systemctl enable --now apple-battery-guard

# Check status
systemctl status apple-battery-guard

# Follow logs
journalctl -u apple-battery-guard -f

# Restart after changing configuration
sudo systemctl restart apple-battery-guard
```

The service uses `Type=notify` with a watchdog — if the daemon hangs, systemd will automatically restart it after 90 seconds.

---

## Architecture

The project is organized into modules with well-defined responsibilities:

```
src/
├── main.rs      — Entrypoint: CLI argument parsing, subcommand dispatch
├── battery.rs   — sysfs abstraction: detect, status, set_charge_threshold
├── config.rs    — Config struct + TOML deserialization + validation
├── daemon.rs    — Main loop, scheduler, Unix socket server, signal handling
├── systemd.rs   — sd_notify, watchdog keepalive
└── tui.rs       — ratatui dashboard with charge gauge, status, and threshold
```

### Design Decisions

**No tokio.** The daemon uses `std::thread` + `std::sync`. The problem does not warrant an async runtime — there are two threads: the polling loop and the socket server.

**sysfs as the interface.** All hardware interaction goes through `/sys/class/power_supply/`. No ioctls, no direct SMC access, no kernel-specific code.

**Graceful fallback.** I/O errors from sysfs are logged but never crash the daemon. If `charge_control_end_threshold` does not exist, the daemon warns and continues.

**Unix socket for IPC.** The CLI (`abg status`) communicates with the daemon over a Unix socket using a simple line-based JSON protocol. No root required after initial setup.

### Daemon Flow

```
startup
   │
   ▼
Battery::detect()          ← scans /sys/class/power_supply/BAT*
   │
   ▼
apply_threshold()          ← writes charge_control_end_threshold
   │
   ▼
systemd::notify_ready()    ← READY=1
   │
   ▼
loop (every 30s) ──────────────────────────────────┐
   │                                                │
   ├── battery.status()    ← reads capacity + status │
   ├── effective_threshold() ← normal or 100% (FCD) │
   ├── set_charge_threshold() ← only if needed       │
   └── systemd::notify_watchdog() ← WATCHDOG=1 ─────┘
```

---

## Development

```bash
# Build
cargo build

# Run all tests (no hardware required)
cargo test

# Run tests for a specific module
cargo test battery
cargo test config

# Lint (zero warnings tolerated)
cargo clippy -- -D warnings

# Format
cargo fmt --check
cargo fmt
```

### Tests

All tests are unit tests and require no hardware. They simulate sysfs using temporary files via `tempfile`:

```bash
$ cargo test
running 30 tests
test battery::tests::detect_finds_battery ... ok
test battery::tests::detect_ignores_non_battery_entries ... ok
test battery::tests::detect_prefers_bat0_over_bat1 ... ok
test battery::tests::set_threshold_fails_without_sysfs_file ... ok
test battery::tests::set_threshold_rejects_above_100 ... ok
test battery::tests::set_threshold_rejects_zero ... ok
test battery::tests::set_threshold_writes_value ... ok
test battery::tests::status_reads_all_fields ... ok
test battery::tests::status_without_threshold_support ... ok
test battery::tests::supports_threshold_false_when_file_missing ... ok
test battery::tests::supports_threshold_true_when_file_exists ... ok
test config::tests::defaults_are_sane ... ok
test config::tests::empty_toml_uses_all_defaults ... ok
test config::tests::invalid_toml_returns_error ... ok
test config::tests::load_missing_file_returns_error ... ok
test config::tests::load_or_default_returns_default_on_invalid_toml ... ok
test config::tests::load_or_default_returns_default_when_missing ... ok
test config::tests::parse_full_toml ... ok
test config::tests::partial_toml_uses_defaults ... ok
test config::tests::save_and_reload_roundtrip ... ok
test config::tests::validation_rejects_empty_socket_path ... ok
test config::tests::validation_rejects_threshold_above_100 ... ok
test config::tests::validation_rejects_zero_interval ... ok
test config::tests::validation_rejects_zero_threshold ... ok
test daemon::tests::effective_threshold_full_charge_day_disabled ... ok
test daemon::tests::effective_threshold_normal ... ok
test daemon::tests::format_status_json_empty_state ... ok
test daemon::tests::format_status_json_escapes_special_chars_in_status ... ok
test daemon::tests::format_status_json_with_data ... ok
test daemon::tests::json_escape_handles_quotes_and_backslashes ... ok

test result: ok. 30 passed; 0 failed; 0 ignored
```

### Adding Support for Older Kernels (applesmc-next)

If `cat /sys/class/power_supply/BAT0/charge_control_end_threshold` returns an error:

```bash
# Arch / Manjaro
yay -S applesmc-next-dkms
sudo modprobe applesmc

# Verify after installation
cat /sys/class/power_supply/BAT0/charge_control_end_threshold
```

---

## FAQ

**Does the daemon require permanent root?**
No. Only the initial udev rule setup requires root. After that, the daemon runs as a normal user with sysfs write access granted by the udev rule.

**Does it work on Apple Silicon MacBooks (M1/M2/M3)?**
No. This project is specific to Intel MacBooks. Apple Silicon chips have a completely different power management architecture.

**Is the threshold persisted across reboots?**
Yes — the daemon applies the threshold on startup. With the systemd service enabled, it is guaranteed after every boot.

**Can I use it on non-Apple Linux laptops?**
Possibly, if your kernel exposes `charge_control_end_threshold` for your battery. `abg status` will tell you whether it is supported.

**What happens if the daemon crashes?**
systemd restarts it automatically (`Restart=on-failure`). The watchdog restarts it if it hangs for more than 90 seconds.

**Can I have two thresholds — one for home and one for travel?**
Not yet, but it is on the roadmap. Current workaround: `abg set 90` before traveling and `abg set 80` when you return.

---

## License

MIT — see [LICENSE](LICENSE).

---

## Contributing

Issues and PRs are welcome. Before opening a PR, run `cargo test && cargo clippy -- -D warnings` and confirm everything passes cleanly.
