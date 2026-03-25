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
  - [Check Kernel Support](#check-kernel-support)
- [Kernel Module (applesmc-next)](#kernel-module-applesmc-next)
  - [What It Is](#what-it-is)
  - [Automatic Setup (Recommended)](#automatic-setup-recommended)
  - [Manual Installation](#manual-installation)
  - [Keeping the Submodule Up to Date](#keeping-the-submodule-up-to-date)
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
- Kernel ≥ 5.4 with `charge_control_end_threshold` support **or** the `applesmc-next` DKMS module (bundled — see below)
- Rust ≥ 1.70 (only required to build from source)
- `dkms` and kernel headers (only required if `applesmc-next` installation is needed)

### Check Kernel Support

Before installing, check whether your kernel already exposes the required sysfs file:

```bash
cat /sys/class/power_supply/BAT0/charge_control_end_threshold
```

| Output | Meaning |
|---|---|
| A number (e.g. `80`) | Kernel already supports it. Proceed directly to [Installation](#installation). |
| `No such file or directory` | Kernel does not support it. Install `applesmc-next` first — see below. |

---

## Kernel Module (applesmc-next)

### What It Is

`applesmc-next` is a DKMS kernel module that patches the `applesmc` driver to expose `charge_control_end_threshold` on kernels that lack native support (typically kernels < 5.4 or distributions with older kernel configurations).

This repository includes `applesmc-next` as a **git submodule** under `modules/applesmc-next`, so you do not need to find or download it separately. It is kept in sync with the upstream project and tested against new kernel versions as they are released.

> **Upstream:** [github.com/c---/applesmc-next](https://github.com/c---/applesmc-next) — actively maintained, last updated July 2025 (v0.1.6, compatible with kernel 6.15).

### Automatic Setup (Recommended)

The bundled setup script detects whether support is already present. If not, it installs the module via DKMS automatically:

```bash
# 1. Clone the repository with submodules
git clone --recurse-submodules https://github.com/michaelmoreira/apple-battery-guard.git
cd apple-battery-guard

# 2. Run the setup script
bash scripts/setup-kernel-module.sh
```

The script will:
1. Check if `/sys/class/power_supply/BAT0/charge_control_end_threshold` exists
2. If it does — exit immediately, nothing to do
3. If it does not — ask for confirmation, then:
   - Verify that `dkms` is installed (and tell you how to install it if not)
   - Copy the module source from `modules/applesmc-next` to `/usr/src/`
   - Register and build the module via `dkms install`
   - Load the module with `modprobe applesmc`
   - Confirm that the threshold file is now available

**Dependencies required by the script:**

| Distribution | Command |
|---|---|
| Arch / Manjaro | `sudo pacman -S dkms linux-headers` |
| Debian / Ubuntu | `sudo apt install dkms linux-headers-$(uname -r)` |
| Fedora | `sudo dnf install dkms kernel-devel` |

After installation, the module is managed by DKMS and will be **automatically recompiled on every kernel update**.

### Manual Installation

If you prefer to install `applesmc-next` without the script:

**Option A — AUR (Arch / Manjaro only):**
```bash
yay -S applesmc-next-dkms
sudo modprobe applesmc
```

**Option B — From the bundled submodule:**
```bash
# Initialize the submodule if you cloned without --recurse-submodules
git submodule update --init

# Read the version from dkms.conf
VERSION=$(grep "^PACKAGE_VERSION=" modules/applesmc-next/dkms.conf | cut -d= -f2 | tr -d '"')

# Copy source and install
sudo cp -r modules/applesmc-next /usr/src/applesmc-next-${VERSION}
sudo dkms install applesmc-next/${VERSION}
sudo modprobe applesmc

# Verify
cat /sys/class/power_supply/BAT0/charge_control_end_threshold
```

**Option C — From upstream directly:**
```bash
git clone https://github.com/c---/applesmc-next.git
cd applesmc-next
VERSION=$(grep "^PACKAGE_VERSION=" dkms.conf | cut -d= -f2 | tr -d '"')
sudo cp -r . /usr/src/applesmc-next-${VERSION}
sudo dkms install applesmc-next/${VERSION}
sudo modprobe applesmc
```

### Keeping the Submodule Up to Date

The submodule is pinned to a specific commit of `applesmc-next`. To update it when a new upstream version is released:

```bash
# Update the submodule to the latest upstream commit
git submodule update --remote modules/applesmc-next

# Review what changed
git diff modules/applesmc-next

# Commit the updated pointer
git add modules/applesmc-next
git commit -m "chore: update applesmc-next submodule to latest"
```

If a new kernel breaks the module, check the upstream repository for fixes and update the submodule accordingly.

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

> **Note:** `applesmc-next-dkms` is listed as an optional dependency. The AUR helper will ask whether you want to install it. Install it if `cat /sys/class/power_supply/BAT0/charge_control_end_threshold` returns an error.

### Other Distributions (Build from Source)

```bash
# 1. Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 2. Clone with submodules
git clone --recurse-submodules https://github.com/michaelmoreira/apple-battery-guard.git
cd apple-battery-guard

# 3. Check kernel support and install applesmc-next if needed
bash scripts/setup-kernel-module.sh

# 4. Build
cargo build --release

# 5. Install
sudo install -Dm755 target/release/abg /usr/local/bin/abg
sudo install -Dm644 config/apple-battery-guard.toml \
    /etc/apple-battery-guard/apple-battery-guard.toml
sudo install -Dm644 systemd/apple-battery-guard.service \
    /etc/systemd/system/apple-battery-guard.service

# 6. Grant write access to sysfs (no permanent root required)
echo 'ACTION=="add", SUBSYSTEM=="power_supply", KERNEL=="BAT[0-9]", \
  RUN+="/bin/chmod 666 /sys%p/charge_control_end_threshold"' \
  | sudo tee /etc/udev/rules.d/99-battery-threshold.rules
sudo udevadm control --reload-rules && sudo udevadm trigger

# 7. Enable and start the service
sudo systemctl enable --now apple-battery-guard
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

### Project Structure

```
apple-battery-guard/
├── src/
│   ├── main.rs      — Entrypoint: CLI argument parsing, subcommand dispatch
│   ├── battery.rs   — sysfs abstraction: detect, status, set_charge_threshold
│   ├── config.rs    — Config struct + TOML deserialization + validation
│   ├── daemon.rs    — Main loop, scheduler, Unix socket server, signal handling
│   ├── systemd.rs   — sd_notify, watchdog keepalive
│   └── tui.rs       — ratatui dashboard with charge gauge, status, and threshold
├── modules/
│   └── applesmc-next/   — git submodule: DKMS kernel module for older kernels
├── scripts/
│   └── setup-kernel-module.sh  — detects and installs applesmc-next if needed
├── config/
│   └── apple-battery-guard.toml
├── systemd/
│   └── apple-battery-guard.service
└── packaging/
    ├── PKGBUILD
    └── apple-battery-guard.spec
```

### Design Decisions

**No tokio.** The daemon uses `std::thread` + `std::sync`. The problem does not warrant an async runtime — there are two threads: the polling loop and the socket server.

**sysfs as the interface.** All hardware interaction goes through `/sys/class/power_supply/`. No ioctls, no direct SMC access, no kernel-specific code.

**Graceful fallback.** I/O errors from sysfs are logged but never crash the daemon. If `charge_control_end_threshold` does not exist, the daemon warns and continues — it does not prevent the service from starting.

**Unix socket for IPC.** The CLI (`abg status`) communicates with the daemon over a Unix socket using a simple line-based JSON protocol. No root required after initial setup.

**Bundled kernel module.** `applesmc-next` is included as a git submodule rather than an external dependency. This guarantees reproducibility and allows the setup script to install the exact tested version without requiring network access beyond the initial clone.

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
# Clone with submodules
git clone --recurse-submodules https://github.com/michaelmoreira/apple-battery-guard.git
cd apple-battery-guard

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

**Do I need to install applesmc-next?**
Only if your kernel does not expose `charge_control_end_threshold` natively. Run `cat /sys/class/power_supply/BAT0/charge_control_end_threshold` — if it returns a number, you do not need it.

**Will applesmc-next break after a kernel update?**
No. Because it is installed via DKMS, it is automatically recompiled for each new kernel version. If a new kernel changes an internal API that breaks compilation, update the submodule (`git submodule update --remote modules/applesmc-next`) to get the latest upstream fix.

**I cloned the repo but `modules/applesmc-next` is empty. Why?**
You cloned without initializing submodules. Run:
```bash
git submodule update --init
```

**Can I have two thresholds — one for home and one for travel?**
Not yet, but it is on the roadmap. Current workaround: `abg set 90` before traveling and `abg set 80` when you return.

---

## License

MIT — see [LICENSE](LICENSE).

---

## Contributing

Issues and PRs are welcome. Before opening a PR, run `cargo test && cargo clippy -- -D warnings` and confirm everything passes cleanly.
