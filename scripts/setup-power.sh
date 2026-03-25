#!/usr/bin/env bash
# setup-power.sh
# Sets up full power management for Intel MacBooks on Linux.
# Installs and configures: auto-cpufreq, mbpfan, tlp
# Does NOT touch battery thresholds — managed by apple-battery-guard.

set -euo pipefail

# Suporta execução tanto do repo como do pacote instalado
if [[ -d "/usr/share/apple-battery-guard" ]]; then
    CONFIG_DIR="/usr/share/apple-battery-guard"
else
    CONFIG_DIR="$(cd "$(dirname "$0")/.." && pwd)/config"
fi

# ── helpers ────────────────────────────────────────────────────────────────────

log()  { echo "  -> $*"; }
ok()   { echo "  [ok] $*"; }
warn() { echo "  [!]  $*"; }
section() { echo; echo "==> $*"; }

require_root() {
    if [[ $EUID -ne 0 ]]; then
        echo "Este script requer root. Corre com: sudo bash scripts/setup-power.sh"
        exit 1
    fi
}

detect_pkg_manager() {
    if command -v pacman &>/dev/null; then echo "pacman"
    elif command -v apt &>/dev/null;   then echo "apt"
    elif command -v dnf &>/dev/null;   then echo "dnf"
    else echo "unknown"; fi
}

install_pkg() {
    local pkg="$1"
    local pm
    pm=$(detect_pkg_manager)
    case "$pm" in
        pacman) pacman -S --noconfirm "$pkg" ;;
        apt)    apt install -y "$pkg" ;;
        dnf)    dnf install -y "$pkg" ;;
        *)      warn "Gestor de pacotes não suportado. Instala '$pkg' manualmente."; return 1 ;;
    esac
}

install_aur_pkg() {
    local pkg="$1"
    if command -v yay &>/dev/null;  then sudo -u "${SUDO_USER:-$USER}" yay -S --noconfirm "$pkg"
    elif command -v paru &>/dev/null; then sudo -u "${SUDO_USER:-$USER}" paru -S --noconfirm "$pkg"
    else
        warn "Nenhum AUR helper encontrado. Instala '$pkg' manualmente:"
        warn "  git clone https://aur.archlinux.org/${pkg}.git && cd ${pkg} && makepkg -si"
        return 1
    fi
}

# ── auto-cpufreq ───────────────────────────────────────────────────────────────

setup_auto_cpufreq() {
    section "auto-cpufreq (CPU frequency management)"

    if command -v auto-cpufreq &>/dev/null; then
        ok "auto-cpufreq já instalado"
    else
        log "A instalar auto-cpufreq..."
        install_aur_pkg auto-cpufreq
    fi

    log "A aplicar configuração para MacBook Intel..."
    cp "${CONFIG_DIR}/auto-cpufreq.conf" /etc/auto-cpufreq.conf

    # Desativar power-profiles-daemon se existir (conflito com auto-cpufreq)
    if systemctl is-active --quiet power-profiles-daemon 2>/dev/null; then
        log "A desativar power-profiles-daemon (conflito com auto-cpufreq)..."
        systemctl disable --now power-profiles-daemon
    fi

    log "A ativar auto-cpufreq..."
    auto-cpufreq --install 2>/dev/null || systemctl enable --now auto-cpufreq

    ok "auto-cpufreq configurado"
}

# ── mbpfan ─────────────────────────────────────────────────────────────────────

setup_mbpfan() {
    section "mbpfan (fan control)"

    if command -v mbpfan &>/dev/null; then
        ok "mbpfan já instalado"
    else
        log "A instalar mbpfan..."
        install_aur_pkg mbpfan
    fi

    log "A configurar módulos do kernel..."
    cat > /etc/modules-load.d/mbpfan.conf << 'EOF'
coretemp
applesmc
EOF
    modprobe coretemp 2>/dev/null || true
    modprobe applesmc 2>/dev/null || true

    log "A aplicar configuração para MacBook Intel..."
    cp "${CONFIG_DIR}/mbpfan.conf" /etc/mbpfan.conf

    log "A ativar mbpfan..."
    systemctl enable --now mbpfan

    ok "mbpfan configurado"
}

# ── tlp ────────────────────────────────────────────────────────────────────────

setup_tlp() {
    section "tlp (power management)"

    if command -v tlp &>/dev/null; then
        ok "tlp já instalado"
    else
        log "A instalar tlp..."
        install_pkg tlp
    fi

    log "A aplicar configuração para MacBook Intel..."
    cp "${CONFIG_DIR}/tlp-macbook.conf" /etc/tlp.d/10-macbook.conf

    log "A ativar tlp..."
    systemctl enable --now tlp
    tlp start

    ok "tlp configurado"
}

# ── verificação ────────────────────────────────────────────────────────────────

verify() {
    section "Verificação"

    echo
    echo "  Serviços activos:"
    for svc in apple-battery-guard auto-cpufreq mbpfan tlp; do
        if systemctl is-active --quiet "$svc" 2>/dev/null; then
            echo "    ✓ $svc"
        else
            echo "    ✗ $svc (inativo)"
        fi
    done

    echo
    echo "  Estado da bateria:"
    /usr/bin/abg status 2>/dev/null || cat /sys/class/power_supply/BAT0/capacity 2>/dev/null

    echo
    echo "  Governor CPU actual:"
    cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor 2>/dev/null || echo "  (não disponível)"

    echo
    ok "Setup completo. O teu MacBook está optimizado para Linux."
}

# ── main ───────────────────────────────────────────────────────────────────────

require_root

echo
echo "apple-battery-guard — Power Management Setup"
echo "Configura: auto-cpufreq + mbpfan + tlp para MacBook Intel"
echo

read -rp "Continuar? [S/n] " answer
case "${answer,,}" in
    n|no) echo "Cancelado."; exit 0 ;;
esac

setup_auto_cpufreq
setup_mbpfan
setup_tlp
verify
