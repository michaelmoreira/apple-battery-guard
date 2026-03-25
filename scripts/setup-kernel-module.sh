#!/usr/bin/env bash
# setup-kernel-module.sh
# Detects whether charge_control_end_threshold is available.
# If not, installs applesmc-next via DKMS using the bundled submodule.

set -euo pipefail

THRESHOLD_PATH="/sys/class/power_supply/BAT0/charge_control_end_threshold"
MODULE_DIR="$(cd "$(dirname "$0")/../modules/applesmc-next" && pwd)"

check_support() {
    [ -f "$THRESHOLD_PATH" ]
}

install_dkms() {
    if ! command -v dkms &>/dev/null; then
        echo "Error: dkms is not installed. Install it first:"
        echo "  Arch/Manjaro: sudo pacman -S dkms linux-headers"
        echo "  Debian/Ubuntu: sudo apt install dkms linux-headers-\$(uname -r)"
        exit 1
    fi

    if [ ! -f "$MODULE_DIR/dkms.conf" ]; then
        echo "Error: submodule not initialized. Run:"
        echo "  git submodule update --init"
        exit 1
    fi

    local version
    version=$(grep "^PACKAGE_VERSION=" "$MODULE_DIR/dkms.conf" | cut -d= -f2 | tr -d '"')

    echo "Installing applesmc-next v${version} via DKMS..."
    sudo cp -r "$MODULE_DIR" "/usr/src/applesmc-next-${version}"
    sudo dkms install "applesmc-next/${version}"
    sudo modprobe applesmc

    if check_support; then
        echo "Success: charge_control_end_threshold is now available."
    else
        echo "Warning: module loaded but threshold file not found. Try rebooting."
    fi
}

if check_support; then
    echo "Kernel already supports charge_control_end_threshold. No action needed."
    exit 0
fi

echo "charge_control_end_threshold not found. The applesmc-next kernel module is required."
echo ""
read -rp "Install it now using the bundled submodule? [y/N] " answer
case "$answer" in
    [yY]) install_dkms ;;
    *)    echo "Skipped. Run this script again when ready."; exit 0 ;;
esac
