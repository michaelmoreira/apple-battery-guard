# apple-battery-guard

[🇵🇹 Português](README.pt.md) | 🇬🇧 English

[![AUR version](https://img.shields.io/aur/version/apple-battery-guard)](https://aur.archlinux.org/packages/apple-battery-guard)
[![AUR votes](https://img.shields.io/aur/votes/apple-battery-guard)](https://aur.archlinux.org/packages/apple-battery-guard)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Intelligent battery charge threshold manager for **Intel MacBooks running Linux**.

MacBooks on Linux charge the battery to 100% every time, prematurely degrading it. macOS automatically limits charging to 80% via the Apple SMC. This project replicates that behavior on Linux through sysfs — no kernel patches, no heavy dependencies.

Tested on a **2017 MacBook Air (Intel) running Manjaro**. Works on any systemd-based distribution.

---

## Quick Start

```bash
# Arch / Manjaro
yay -S apple-battery-guard
sudo systemctl enable --now apple-battery-guard
abg status
```

If `abg status` shows `Threshold: 80%` — you're done. If it shows `Threshold: unsupported`, see [Kernel Module (applesmc-next)](#kernel-module-applesmc-next).

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
- [Verify the Installation](#verify-the-installation)
- [Full Power Management Setup](#full-power-management-setup)
- [Configuration](#configuration)
- [Usage](#usage)
- [systemd Service](#systemd-service)
- [Troubleshooting](#troubleshooting)
- [Uninstall](#uninstall)
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
│  │ scheduler│───▶│ battery  │-▶│  sysfs  │  │
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
2. Every 30 seconds (configurable) it checks whether the threshold is correct and reapplies if needed. This guards against other tools or resume-from-suspend events resetting the value.
3. It communicates with systemd via `sd_notify` (Type=notify + watchdog).
4. It exposes current state via a **Unix socket** — the CLI reads from it without requiring root.
5. It supports a **"full charge day"**: once a week the battery charges to 100% for calibration.

---

## Requirements

### Hardware
- Intel MacBook (Air, Pro, Mini — any Intel model)
- Tested on MacBook Air 2017

> **Apple Silicon (M1/M2/M3) is not supported.** These chips have a completely different power management architecture that does not use the `applesmc` driver.

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

| Output | Meaning | Action |
|---|---|---|
| A number (e.g. `80`) | Native support present | Proceed to [Installation](#installation) |
| `No such file or directory` | No native support | Install `applesmc-next` first — see below |
| `Permission denied` | File exists but not writable | Set up the udev rule — see [Installation](#installation) step 6 |

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

**Install dkms and kernel headers first:**

| Distribution | Command |
|---|---|
| Arch / Manjaro | `sudo pacman -S dkms linux-headers` |
| Debian / Ubuntu | `sudo apt install dkms linux-headers-$(uname -r)` |
| Fedora | `sudo dnf install dkms kernel-devel` |

After installation, the module is managed by DKMS and will be **automatically recompiled on every kernel update** — no manual intervention needed.

### Manual Installation

If you prefer to install `applesmc-next` without the script:

**Option A — AUR (Arch / Manjaro only):**
```bash
yay -S applesmc-next-dkms
sudo modprobe applesmc

# Verify
cat /sys/class/power_supply/BAT0/charge_control_end_threshold
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

# 5. Install binary, config and systemd service
sudo install -Dm755 target/release/abg /usr/local/bin/abg
sudo install -Dm644 config/apple-battery-guard.toml \
    /etc/apple-battery-guard/apple-battery-guard.toml
sudo install -Dm644 systemd/apple-battery-guard.service \
    /etc/systemd/system/apple-battery-guard.service

# 6. Grant write access to sysfs (eliminates the need for permanent root)
#    This udev rule runs chmod on the threshold file every time the battery
#    device is added (boot, resume from suspend), making it writable by
#    any user — so the daemon never needs to run as root.
echo 'ACTION=="add", SUBSYSTEM=="power_supply", KERNEL=="BAT[0-9]", \
  RUN+="/bin/chmod 666 /sys%p/charge_control_end_threshold"' \
  | sudo tee /etc/udev/rules.d/99-battery-threshold.rules
sudo udevadm control --reload-rules && sudo udevadm trigger

# 7. Enable and start the service
sudo systemctl enable --now apple-battery-guard
```

---

## Verify the Installation

After installing, confirm everything is working:

```bash
# 1. Check the service is running
systemctl status apple-battery-guard
# Expected: Active: active (running)

# 2. Check the threshold was applied
cat /sys/class/power_supply/BAT0/charge_control_end_threshold
# Expected: 80

# 3. Check the daemon status via CLI
abg status
# Expected output:
# Battery:   75%
# Status:    Discharging
# Threshold: 80%

# 4. Check the logs for the startup sequence
journalctl -u apple-battery-guard -n 20
# Expected lines:
# apple-battery-guard[...]: battery detected: BAT0
# apple-battery-guard[...]: threshold applied: 80%
# apple-battery-guard[...]: ready
```

If `Threshold` shows as `unsupported` in `abg status`, the `charge_control_end_threshold` sysfs file is not available. See [Kernel Module (applesmc-next)](#kernel-module-applesmc-next).

---

## Full Power Management Setup

`apple-battery-guard` handles charge threshold management. For a complete macOS-like power management experience on your Intel MacBook, the project also includes optimized configurations for three complementary tools.

### What Each Tool Does

| Tool | Responsibility | Conflict risk |
|---|---|---|
| **apple-battery-guard** | Battery charge threshold (80%) | — |
| **auto-cpufreq** | CPU governor, frequency scaling, turbo | Disable `power-profiles-daemon` |
| **mbpfan** | Fan speed control via applesmc | None |
| **tlp** | Disk, USB, WiFi, audio, PCIe power saving | Do NOT set CPU or battery settings |

> **Important:** The configs included here are pre-configured to avoid conflicts. TLP's CPU governor and battery threshold settings are intentionally commented out — `auto-cpufreq` and `apple-battery-guard` manage those respectively.

### Automatic Setup (Recommended)

```bash
sudo bash scripts/setup-power.sh
```

The script installs and configures all three tools, enables their services, disables conflicting tools (`power-profiles-daemon`), and verifies everything is running.

### Manual Setup

**1. auto-cpufreq** — CPU frequency management

```bash
# Install
yay -S auto-cpufreq

# Deploy config (optimized for MacBook Air 2017, i5-5350U)
sudo cp config/auto-cpufreq.conf /etc/auto-cpufreq.conf

# Install as service (also disables power-profiles-daemon automatically)
sudo auto-cpufreq --install

# Verify
auto-cpufreq --stats
```

**2. mbpfan** — Fan control

```bash
# Install
yay -S mbpfan

# Load required kernel modules on boot
sudo tee /etc/modules-load.d/mbpfan.conf << 'EOF'
coretemp
applesmc
EOF

# Deploy config
sudo cp config/mbpfan.conf /etc/mbpfan.conf

# Enable service
sudo systemctl enable --now mbpfan
```

**3. tlp** — System power saving

```bash
# Install
sudo pacman -S tlp

# Deploy MacBook-specific drop-in config
sudo cp config/tlp-macbook.conf /etc/tlp.d/10-macbook.conf

# Enable and apply
sudo systemctl enable --now tlp
sudo tlp start
```

### Verify All Services

```bash
for svc in apple-battery-guard auto-cpufreq mbpfan tlp; do
    systemctl is-active --quiet $svc && echo "✓ $svc" || echo "✗ $svc"
done
```

### What This Achieves vs macOS

| Feature | macOS | Linux (with full setup) |
|---|---|---|
| Charge threshold | ✅ 80% | ✅ 80% (apple-battery-guard) |
| CPU freq scaling | ✅ automatic | ✅ automatic (auto-cpufreq) |
| Fan control | ✅ native | ✅ configured (mbpfan) |
| USB autosuspend | ✅ native | ✅ enabled (tlp) |
| WiFi power save | ✅ native | ✅ on battery (tlp) |
| Audio power save | ✅ native | ✅ on battery (tlp) |
| PCIe ASPM | ✅ native | ✅ powersupersave on battery (tlp) |
| Disk power save | ✅ native | ✅ configured (tlp) |
| App Nap / process throttling | ✅ native | ❌ not available on Linux |
| Deep hibernation | ✅ native | ⚠️ depends on kernel/distro |

In practice, this setup recovers **60–70% of the autonomy gap** between macOS and Linux on Intel MacBooks.

---

## Configuration

File: `/etc/apple-battery-guard/apple-battery-guard.toml`

```toml
[battery]
# Charge end threshold under normal conditions (1–100).
# 80% is the recommended value to maximize battery longevity.
charge_end_threshold = 80

[daemon]
# How often the daemon checks and reapplies the threshold, in seconds.
# Lower values react faster to resume-from-suspend events.
# Default: 30
interval_secs = 30

# Path to the Unix socket used for CLI communication.
# The CLI (abg status, abg config) reads from this socket without root.
socket_path = "/run/apple-battery-guard/daemon.sock"

[full_charge]
# "Full charge day": on the configured weekday, the threshold is raised to
# 100% so the battery charges fully. Useful for calibration (helps the
# battery gauge stay accurate) and for days you know you'll need maximum
# range away from a power source.
#
# The threshold returns to charge_end_threshold automatically the next day.
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

### Applying Configuration Changes

The daemon reads the config file on startup. After editing it, restart the service:

```bash
sudo systemctl restart apple-battery-guard
abg status  # confirm the new threshold is active
```

---

## Usage

```bash
# Show current battery status (reads from the daemon socket — no root needed)
abg status

# Example output:
# Battery:   75%
# Status:    Discharging
# Threshold: 80%

# Set threshold manually (bypasses the daemon, writes directly to sysfs)
abg set 80

# Show the effective configuration loaded by the daemon
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
# Enable at boot and start immediately
sudo systemctl enable --now apple-battery-guard

# Check status
systemctl status apple-battery-guard

# Follow logs in real time
journalctl -u apple-battery-guard -f

# Show logs since last boot
journalctl -u apple-battery-guard -b

# Restart after changing configuration
sudo systemctl restart apple-battery-guard

# Stop the daemon (threshold remains at last set value)
sudo systemctl stop apple-battery-guard

# Disable at boot
sudo systemctl disable apple-battery-guard
```

The service uses `Type=notify` with a watchdog — if the daemon hangs, systemd will automatically restart it after 90 seconds.

### What the Logs Look Like

```
Mar 25 11:27:31 macbook systemd[1]: Starting Apple Battery Guard...
Mar 25 11:27:31 macbook apple-battery-guard[52127]: [INFO  abg] a iniciar daemon (threshold=80%)
Mar 25 11:27:31 macbook apple-battery-guard[52127]: [INFO  abg::daemon] socket a escutar em /run/apple-battery-guard/daemon.sock
Mar 25 11:27:31 macbook apple-battery-guard[52127]: [INFO  abg::daemon] threshold definido para 80%
Mar 25 11:27:31 macbook systemd[1]: Started Apple Battery Guard.
# Every 30s:
Mar 25 11:28:01 macbook apple-battery-guard[52127]: [INFO  abg::daemon] threshold ok: 80%
# On full charge day:
Mar 25 11:27:31 macbook apple-battery-guard[52127]: [INFO  abg::daemon] full charge day ativo, threshold: 100%
# On resume from suspend:
Mar 25 14:32:10 macbook apple-battery-guard[52127]: [INFO  abg::daemon] threshold redefinido após resume: 80%
```

The daemon uses approximately **400 KB of RAM** at steady state.

---

## Troubleshooting

### `charge_control_end_threshold` not found

```
abg status → Threshold: unsupported
```

Your kernel does not expose the threshold file. Install `applesmc-next`:
```bash
bash scripts/setup-kernel-module.sh
```
See [Kernel Module (applesmc-next)](#kernel-module-applesmc-next) for details.

---

### Permission denied when writing threshold

```
journalctl -u apple-battery-guard → I/O error: Permission denied
```

The udev rule is not active. Verify it exists and reload:
```bash
cat /etc/udev/rules.d/99-battery-threshold.rules
sudo udevadm control --reload-rules && sudo udevadm trigger
# If the file does not exist, create it as shown in Installation step 6
```

After reloading, restart the service:
```bash
sudo systemctl restart apple-battery-guard
```

---

### Daemon fails to start

```
systemctl status apple-battery-guard → Failed
```

Check the full logs:
```bash
journalctl -u apple-battery-guard -n 50 --no-pager
```

Common causes:
- Config file syntax error — validate with: `abg --config /etc/apple-battery-guard/apple-battery-guard.toml config`
- Battery not found — check `ls /sys/class/power_supply/`
- Socket directory does not exist — create it: `sudo mkdir -p /run/apple-battery-guard`

---

### Threshold resets to 100% after suspend/resume

This can happen if another tool (e.g. `tlp`, `auto-cpufreq`) overwrites the threshold on resume. Check for conflicts:
```bash
systemctl list-units | grep -E "tlp|cpufreq|battery"
```

If `tlp` is installed, add to `/etc/tlp.conf`:
```
START_CHARGE_THRESH_BAT0=0
STOP_CHARGE_THRESH_BAT0=80
```
Or disable tlp's battery management and let `apple-battery-guard` handle it exclusively.

---

### `abg` command not found or autocorrected by zsh

If zsh asks `correct 'abg' to 'ab'?`, answer `n` or add this to `~/.zshrc` to permanently suppress it:

```bash
echo "CORRECT_IGNORE='abg'" >> ~/.zshrc
source ~/.zshrc
```

Alternatively, use the full path: `/usr/bin/abg status`.

---

### `modules/applesmc-next` is empty after cloning

You cloned without initializing submodules:
```bash
git submodule update --init
```

---

### applesmc-next fails to compile after a kernel update

The upstream module may not yet support the new kernel. Check:
```bash
# See if a fix was released
git submodule update --remote modules/applesmc-next
git diff modules/applesmc-next
```

If no fix exists yet, you can temporarily charge to 100% by disabling the daemon:
```bash
sudo systemctl stop apple-battery-guard
```

---

## Uninstall

### AUR (Arch / Manjaro)

```bash
# Remove the package (binary, config, systemd service)
sudo pacman -R apple-battery-guard

# Optionally remove the config file (pacman keeps it by default)
sudo rm -rf /etc/apple-battery-guard

# Optionally remove applesmc-next if you installed it
sudo pacman -R applesmc-next-dkms
```

### Build from Source

```bash
# Stop and disable the service
sudo systemctl disable --now apple-battery-guard

# Remove installed files
sudo rm /usr/local/bin/abg
sudo rm /etc/systemd/system/apple-battery-guard.service
sudo rm -rf /etc/apple-battery-guard
sudo rm -f /etc/udev/rules.d/99-battery-threshold.rules

# Reload udev and systemd
sudo udevadm control --reload-rules
sudo systemctl daemon-reload

# Optionally remove applesmc-next DKMS module
VERSION=$(grep "^PACKAGE_VERSION=" modules/applesmc-next/dkms.conf | cut -d= -f2 | tr -d '"')
sudo dkms remove applesmc-next/${VERSION} --all
sudo rm -rf /usr/src/applesmc-next-${VERSION}
```

After uninstalling, the battery will charge to 100% again on the next full charge cycle.

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

**sysfs as the interface.** All hardware interaction goes through `/sys/class/power_supply/`. No ioctls, no direct SMC access, no kernel-specific code. This means the daemon works with any driver that exposes the standard sysfs interface — not just `applesmc`.

**Graceful fallback.** I/O errors from sysfs are logged but never crash the daemon. If `charge_control_end_threshold` does not exist, the daemon warns and continues — it does not prevent the service from starting.

**Unix socket for IPC.** The CLI (`abg status`) communicates with the daemon over a Unix socket using a simple line-based JSON protocol. No root required after initial setup. The socket lives at `/run/apple-battery-guard/daemon.sock`.

**No permanent root.** The udev rule grants world-write permission to the threshold file when the battery device is added by the kernel. This is the standard approach used by tools like `tlp` and `auto-cpufreq`. The daemon itself runs as a normal user.

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
   ├── set_charge_threshold() ← only if value drifted│
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
No. Only the initial udev rule setup requires root. The udev rule runs `chmod 666` on the threshold file whenever the battery device is registered by the kernel (on boot and resume). After that, the daemon runs as a normal user.

**Does it work on Apple Silicon MacBooks (M1/M2/M3)?**
No. This project is specific to Intel MacBooks. Apple Silicon chips have a completely different power management architecture that does not use the `applesmc` driver.

**Is the threshold persisted across reboots?**
Yes — the daemon applies the threshold on startup. With the systemd service enabled, it is guaranteed after every boot.

**Can I use it on non-Apple Linux laptops?**
Possibly. If your kernel exposes `charge_control_end_threshold` for your battery, the daemon will use it. `abg status` will tell you whether it is supported.

**What happens if the daemon crashes?**
systemd restarts it automatically (`Restart=on-failure`). The watchdog also restarts it if it hangs for more than 90 seconds without sending a keepalive.

**Do I need to install applesmc-next?**
Only if your kernel does not expose `charge_control_end_threshold` natively. Run `cat /sys/class/power_supply/BAT0/charge_control_end_threshold` — if it returns a number, you do not need it.

**Will applesmc-next break after a kernel update?**
No. Because it is installed via DKMS, it is automatically recompiled for each new kernel version. If a new kernel changes an internal API that breaks compilation, update the submodule (`git submodule update --remote modules/applesmc-next`) to pick up the latest upstream fix.

**I cloned the repo but `modules/applesmc-next` is empty. Why?**
You cloned without initializing submodules. Run:
```bash
git submodule update --init
```

**Does it conflict with tlp or auto-cpufreq?**
It can, if those tools also manage battery thresholds. See [Threshold resets to 100% after suspend/resume](#threshold-resets-to-100-after-suspendresume) in the Troubleshooting section.

**Can I have two thresholds — one for home and one for travel?**
Not yet, but it is on the roadmap. Current workaround: `abg set 90` before traveling and `abg set 80` when you return.

**What is the full charge day exactly?**
When `full_charge.enabled = true`, on the configured weekday the daemon raises the threshold to 100% instead of the normal value. This allows the battery to charge completely — useful for calibrating the charge gauge and for travel days when you need maximum range. The next day, it automatically returns to the normal threshold.

---

## License

MIT — see [LICENSE](LICENSE).

---

## Contributing

Issues and PRs are welcome. Before opening a PR, run `cargo test && cargo clippy -- -D warnings` and confirm everything passes cleanly.
