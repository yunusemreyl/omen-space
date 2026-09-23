#!/usr/bin/env bash
# setup.sh — Multi-distro installer for hp-wmi-fan-and-backlight-control
# https://github.com/TUXOV/hp-wmi-fan-and-backlight-control
#
# Supports: Debian/Ubuntu, Fedora/RHEL, Arch, openSUSE, Void, Gentoo
# Usage: sudo ./setup.sh [install|uninstall]

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

MODNAME="hp-omen-extra"
MODVER=$(grep -oP 'PACKAGE_VERSION="\K[^"]+' dkms.conf 2>/dev/null || echo "1.3.5")
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# MOK_DIR — initialised here so it is always defined (avoids unbound variable
# errors in the MOK_PENDING check when Secure Boot is disabled)
MOK_DIR="/var/lib/omen-space/mok"

# modprobe.d file that overrides the stock hp-wmi with our custom DKMS build.
# Created during install, removed during uninstall.
HPWMI_OVERRIDE_CONF="/etc/modprobe.d/omen-space-hpwmi-override.conf"

# ── Kernel version detection ──────────────────────────────────────────────────
KVER_MAJOR=$(uname -r | cut -d. -f1)
KVER_MINOR=$(uname -r | cut -d. -f2)
BOARD_NAME=$(cat /sys/devices/virtual/dmi/id/board_name 2>/dev/null | tr '[:lower:]' '[:upper:]' || echo "")

# Bail out early when kernel modules are clearly missing (pending reboot).
# This catches the common Arch/CachyOS case where the kernel was updated
# but the user hasn't rebooted yet — DKMS would silently build against the
# wrong headers and then fail to load.
if [ ! -e "/lib/modules/$(uname -r)/modules.dep" ] && \
   [ ! -e "/usr/lib/modules/$(uname -r)/modules.dep" ]; then
    echo -e "${RED}[ERROR] Kernel modules for $(uname -r) are missing or broken.${NC}"
    echo -e "${RED}[ERROR] This usually means you updated your kernel but haven't rebooted.${NC}"
    echo -e "${RED}[ERROR] Please reboot your system and run the installer again.${NC}"
    exit 1
fi

info()  { echo -e "${BLUE}[INFO]${NC} $*"; }
ok()    { echo -e "${GREEN}[OK]${NC} $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*"; exit 1; }

# ── Distro detection ──────────────────────────────────────────────────────────

detect_distro() {
    if [ -f /etc/os-release ]; then
        . /etc/os-release
        DISTRO_ID="${ID:-unknown}"
        DISTRO_LIKE="${ID_LIKE:-$DISTRO_ID}"
    elif [ -f /etc/arch-release ]; then
        DISTRO_ID="arch"
        DISTRO_LIKE="arch"
    elif [ -f /etc/gentoo-release ]; then
        DISTRO_ID="gentoo"
        DISTRO_LIKE="gentoo"
    else
        DISTRO_ID="unknown"
        DISTRO_LIKE="unknown"
    fi
}

# ── Dependency installation per distro ────────────────────────────────────────

install_deps() {
    info "Detected distro: ${DISTRO_ID}"

    case "$DISTRO_ID" in
        ubuntu|debian|linuxmint|pop|elementary|zorin|kali)
            info "Installing dependencies (apt)..."
            apt-get update -qq || true
            apt-get install -y dkms build-essential linux-headers-"$(uname -r)"
            ;;
        fedora|nobara)
            info "Installing dependencies (dnf)..."
            dnf install -y dkms kernel-devel kernel-headers gcc make
            ;;
        rhel|centos|rocky|alma)
            info "Installing dependencies (dnf/yum)..."
            if command -v dnf &>/dev/null; then
                dnf install -y dkms kernel-devel kernel-headers gcc make
            else
                yum install -y dkms kernel-devel kernel-headers gcc make
            fi
            ;;
        arch|manjaro|endeavouros|garuda|cachyos)
            info "Installing dependencies (pacman)..."
            local HEADERS_PKG=""
            local RUNNING_KVER
            RUNNING_KVER=$(uname -r)

            if [[ $RUNNING_KVER == *"-cachyos"* ]]; then
                # CachyOS can have multiple kernel flavours (bore, deckify, …)
                local SUFFIX
                SUFFIX=$(echo "$RUNNING_KVER" | sed 's/^[0-9.]*-[0-9]*-\(.*\)/\1/')
                if [[ -n "$SUFFIX" ]] && pacman -Si "linux-$SUFFIX-headers" &>/dev/null 2>&1; then
                    HEADERS_PKG="linux-$SUFFIX-headers"
                elif pacman -Si linux-cachyos-headers &>/dev/null 2>&1; then
                    HEADERS_PKG="linux-cachyos-headers"
                else
                    HEADERS_PKG="linux-headers"
                fi
            elif [[ $RUNNING_KVER == *"-zen"* ]]; then
                HEADERS_PKG="linux-zen-headers"
            elif [[ $RUNNING_KVER == *"-lts"* ]]; then
                HEADERS_PKG="linux-lts-headers"
            elif [[ $RUNNING_KVER == *"-hardened"* ]]; then
                HEADERS_PKG="linux-hardened-headers"
            elif [[ $RUNNING_KVER == *"-rt"* ]]; then
                HEADERS_PKG="linux-rt-headers"
            else
                # FIX #173: Manjaro and vanilla Arch kernels use versioned headers packages
                # (e.g. linux612-headers for 6.12.x-MANJARO, linux61-headers for 6.1.x).
                # The generic 'linux-headers' meta-package on Manjaro resolves to the LTS
                # kernel headers, which may differ from the running kernel — causing DKMS
                # build failures.  Derive the correct package from the running kernel version.
                local KVER_MAJOR KVER_MINOR VERSIONED_PKG
                KVER_MAJOR=$(echo "$RUNNING_KVER" | cut -d. -f1)
                KVER_MINOR=$(echo "$RUNNING_KVER" | cut -d. -f2)
                VERSIONED_PKG="linux${KVER_MAJOR}${KVER_MINOR}-headers"
                if pacman -Si "$VERSIONED_PKG" &>/dev/null 2>&1; then
                    HEADERS_PKG="$VERSIONED_PKG"
                    info "Detected versioned headers package: $VERSIONED_PKG"
                else
                    HEADERS_PKG="linux-headers"
                    warn "Could not find $VERSIONED_PKG — falling back to linux-headers."
                    warn "If DKMS fails, manually install headers for kernel $RUNNING_KVER."
                fi
            fi

            info "Attempting to install: dkms $HEADERS_PKG base-devel"
            if ! pacman -S --needed --noconfirm dkms "$HEADERS_PKG" base-devel; then
                warn "Could not install $HEADERS_PKG. Trying generic linux-headers..."
                pacman -S --needed --noconfirm dkms linux-headers base-devel \
                    || warn "Header installation failed. DKMS might not work without headers."
            fi
            ;;

        opensuse*|suse*)
            info "Installing dependencies (zypper)..."
            zypper install -y dkms kernel-devel kernel-default-devel gcc make
            ;;
        void)
            info "Installing dependencies (xbps)..."
            xbps-install -Sy dkms linux-headers base-devel
            ;;
        gentoo)
            info "Gentoo detected. Ensure sys-kernel/dkms and linux-headers are installed."
            command -v dkms &>/dev/null || error "dkms not found. Install it with: emerge sys-kernel/dkms"
            ;;
        *)
            # Fallback: try ID_LIKE
            case "$DISTRO_LIKE" in
                *debian*|*ubuntu*)
                    info "Debian-like distro detected, using apt..."
                    apt-get update -qq || true
                    apt-get install -y dkms build-essential linux-headers-"$(uname -r)"
                    ;;
                *fedora*|*rhel*)
                    info "Fedora-like distro detected, using dnf..."
                    dnf install -y dkms kernel-devel kernel-headers gcc make
                    ;;
                *arch*)
                    info "Arch-like distro detected, using pacman..."
                    pacman -S --needed --noconfirm dkms linux-headers base-devel
                    ;;
                *suse*)
                    info "SUSE-like distro detected, using zypper..."
                    zypper install -y dkms kernel-devel kernel-default-devel gcc make
                    ;;
                *)
                    warn "Unknown distro '${DISTRO_ID}'. Attempting generic install..."
                    warn "Make sure you have installed: dkms, gcc, make, kernel-headers"
                    command -v dkms &>/dev/null || error "dkms not found. Please install it manually."
                    ;;
            esac
            ;;
    esac
}

# ── Helper: find module path in both /lib and /usr/lib ───────────────────────
# Some distros (Arch, CachyOS, Gentoo, newer Debian/Ubuntu) store kernel
# modules under /usr/lib/modules rather than /lib/modules.  Both paths are
# searched so the functions below work on all distros.

find_module_paths() {
    local pattern="$1"
    local kver="${2:-$(uname -r)}"
    find \
        "/lib/modules/$kver" \
        "/usr/lib/modules/$kver" \
        -name "$pattern" 2>/dev/null | sort -u
}

# ── Stock hp-wmi blacklisting ─────────────────────────────────────────────────
# FIX #211: The old approach of renaming the stock hp-wmi.ko file to .backup
# was fragile — on CachyOS/Arch, modinfo -n hp-wmi can return the DKMS path
# (updates/…) even before our module is installed, causing the backup step to
# silently skip, leaving both the stock and custom .ko files present.  At boot
# the kernel then refuses to load the duplicate and drops to emergency mode.
#
# Solution: use the kernel's own modprobe override mechanism instead.
# Writing `blacklist hp_wmi` + `install hp_wmi /bin/true` to a modprobe.d file
# guarantees:
#   • The stock module is never loaded (not at boot, not by modprobe).
#   • The custom DKMS build placed in updates/ is found first by depmod/modprobe.
#   • Kernel updates cannot re-enable the stock module without the user removing
#     this file (which do_uninstall() does automatically).
#
# This approach works whether the stock module lives in /lib/modules or
# /usr/lib/modules and is immune to the DKMS path-detection race.

install_hpwmi_override() {
    # The previous approach of blacklisting hp_wmi prevents systemd-modules-load
    # from autoloading it at boot. Since DKMS places hp-wmi.ko in updates/ which
    # depmod already prefers over the stock kernel path, we do not need to
    # override or blacklist anything.
    # We explicitly remove the old override if it exists.
    remove_hpwmi_override
}

remove_hpwmi_override() {
    if [ -f "$HPWMI_OVERRIDE_CONF" ]; then
        rm -f "$HPWMI_OVERRIDE_CONF"
        ok "Removed hp-wmi modprobe override ($HPWMI_OVERRIDE_CONF)"
    fi
}

# ── Install ───────────────────────────────────────────────────────────────────

do_install() {
    [[ $EUID -ne 0 ]] && error "This script must be run as root (use sudo)."

    detect_distro
    install_deps

    # Detect Clang-built kernel and set LLVM=1 automatically
    if grep -iq "clang" /proc/version; then
        info "Kernel built with Clang/LLVM detected. Automatically setting LLVM=1 for build..."
        export LLVM=1
    fi

    cd "$SCRIPT_DIR"

    # Create the source directory and copy files first so that dkms remove has a valid dkms.conf to read
    # (otherwise dkms remove fails with 'Missing the module source directory')
    rm -rf "/usr/src/${MODNAME}-${MODVER}"
    mkdir -p "/usr/src/${MODNAME}-${MODVER}"

    # Copy source files into the DKMS tree
    cp "$SCRIPT_DIR/dkms.conf" "$SCRIPT_DIR/Makefile" "$SCRIPT_DIR"/*.c \
       "/usr/src/${MODNAME}-${MODVER}/"
    cp "$SCRIPT_DIR"/*.h "/usr/src/${MODNAME}-${MODVER}/" 2>/dev/null || true

    # Purge stale .ko files from previous installs that may reference
    # removed symbols (e.g. hp_wmi_mutex).  Without this, modprobe may
    # find and load the old .ko instead of the freshly built one.
    info "Purging stale module files..."
    KVER=$(uname -r)
    find /lib/modules/"$KVER" /usr/lib/modules/"$KVER" \
        -name 'hp-omen-extra.ko*' -delete 2>/dev/null || true
    find /lib/modules/"$KVER" /usr/lib/modules/"$KVER" \
        -path '*/updates/*' -name 'hp-wmi.ko*' -delete 2>/dev/null || true
    depmod -a 2>/dev/null || true

    # Remove all existing DKMS entries for this module
    if dkms status "$MODNAME" 2>/dev/null | grep -q "$MODNAME"; then
        warn "Removing existing DKMS entries for $MODNAME..."
        for v in $(dkms status "$MODNAME" | grep -oP "(?<=${MODNAME}[/, ])[^,:]+" | tr -d ' ' | sort -u); do
            [ -z "$v" ] && continue
            dkms remove -m "$MODNAME" -v "$v" --all 2>/dev/null || true
        done
    fi

    # ── Stock hp-wmi override ────────────────────────────────────────────────
    # Must be done BEFORE depmod/dkms so that when the DKMS module lands in
    # updates/ it is the only copy modprobe will ever see.
    # This replaces the old file-backup approach which was unreliable on
    # CachyOS/Arch (see install_hpwmi_override() docblock above).
    install_hpwmi_override

    # ── DKMS build & install — always full (both hp-wmi + hp-omen-extra) ─────
    # We no longer maintain a "RGB-only" mode for kernel >= 7.0.  The custom
    # hp-wmi is always built to preserve gpu_tgp, gpu_ppab, chassis_temp and
    # improved Omen fan-curve support regardless of kernel version.
    info "Building and installing custom hp-wmi + hp-omen-extra via DKMS..."
    if [ "$KVER_MAJOR" -gt 7 ] || { [ "$KVER_MAJOR" -eq 7 ] && [ "$KVER_MINOR" -ge 0 ]; }; then
        info "Kernel $(uname -r) (>= 7.0) detected — custom hp-wmi overrides stock to expose gpu_tgp, gpu_ppab, chassis_temp."
    else
        info "Kernel $(uname -r) (< 7.0) detected — installing both hp-wmi and hp-omen-extra."
    fi

    if ! dkms status "$MODNAME/$MODVER" 2>/dev/null | grep -Eq "^${MODNAME}/${MODVER}([,:]|$)"; then
        dkms add -m "$MODNAME" -v "$MODVER" || true
    else
        info "DKMS source already present for $MODNAME/$MODVER, skipping dkms add."
    fi
    dkms build   -m "$MODNAME" -v "$MODVER"          || error "DKMS build failed. Check logs."
    dkms install -m "$MODNAME" -v "$MODVER" --force   || error "DKMS install failed."

    # Refresh module dependency database so modprobe picks up the new .ko
    depmod -a

    # ── Post-install verification ────────────────────────────────────────────
    # Some distros (Nobara, certain Fedora spins) lack /usr/sbin/weak-modules,
    # which causes DKMS to silently skip the final copy step.  We verify that
    # the compiled .ko is actually reachable by modinfo; if not, force-reinstall
    # targeting the running kernel explicitly.
    info "Verifying DKMS module installation..."
    _dkms_verify_ok=true

    if ! modinfo hp-omen-extra &>/dev/null; then
        warn "hp-omen-extra not found by modinfo after DKMS install — retrying with --force..."
        dkms install -m "$MODNAME" -v "$MODVER" -k "$(uname -r)" --force 2>/dev/null || true
        depmod -a
        if ! modinfo hp-omen-extra &>/dev/null; then
            _dkms_verify_ok=false
            warn "hp-omen-extra STILL not found after forced reinstall."
            warn "Try manually: sudo dkms install $MODNAME/$MODVER -k $(uname -r) --force"
        fi
    fi

    # Verify custom hp-wmi landed in the DKMS/updates path (not the stock path)
    local hpwmi_path
    hpwmi_path=$(modinfo -n hp-wmi 2>/dev/null || true)
    if [[ -n "$hpwmi_path" ]] && [[ "$hpwmi_path" != *"updates"* ]] && \
       [[ "$hpwmi_path" != *"dkms"* ]] && [[ "$hpwmi_path" != *"extra/"* ]]; then
        warn "Custom hp-wmi not found in DKMS/updates path (found: $hpwmi_path) — retrying..."
        dkms install -m "$MODNAME" -v "$MODVER" -k "$(uname -r)" --force 2>/dev/null || true
        depmod -a
    fi

    if $_dkms_verify_ok; then
        ok "DKMS module verified: $(modinfo -n hp-omen-extra 2>/dev/null || echo 'path unknown')"
        ok "Custom hp-wmi at:     $(modinfo -n hp-wmi 2>/dev/null || echo 'path unknown')"
    fi

    # ── Secure Boot handling ─────────────────────────────────────────────────
    SECUREBOOT=false
    if command -v mokutil &>/dev/null; then
        if mokutil --sb-state 2>/dev/null | grep -qi "SecureBoot enabled"; then
            SECUREBOOT=true
        fi
    fi

    if $SECUREBOOT; then
        mkdir -p "$MOK_DIR"

        # Generate MOK key if missing
        if [ ! -f "$MOK_DIR/MOK.priv" ] || [ ! -f "$MOK_DIR/MOK.der" ]; then
            info "Generating MOK key for Secure Boot..."
            openssl req -new -x509 -newkey rsa:2048 \
                -keyout "$MOK_DIR/MOK.priv" \
                -outform DER -out "$MOK_DIR/MOK.der" \
                -days 36500 -subj "/CN=omen-space-mok/" -nodes 2>/dev/null
            chmod 600 "$MOK_DIR/MOK.priv"
        fi

        # Sign installed modules
        KVER=$(uname -r)
        info "Signing custom modules for Secure Boot..."
        SIGN_SCRIPT=$(find \
            "/usr/src/linux-headers-$KVER/scripts" \
            "/usr/src/kernels/$KVER/scripts" \
            "/lib/modules/$KVER/build/scripts" \
            "/usr/lib/modules/$KVER/build/scripts" \
            "/usr/src/linux-headers-$KVER" \
            "/usr/lib/modules/$KVER" \
            -name "sign-file" -type f 2>/dev/null | head -n 1)

        if [ -n "$SIGN_SCRIPT" ]; then
            for MOD_NAME in "hp-omen-extra.ko" "hp-wmi.ko"; do
                MOD_PATH=$(find_module_paths "$MOD_NAME" "$KVER" | grep -v "backup" | head -n 1)
                if [ -n "$MOD_PATH" ]; then
                    "$SIGN_SCRIPT" sha256 "$MOK_DIR/MOK.priv" "$MOK_DIR/MOK.der" "$MOD_PATH" \
                        || warn "Failed to sign $MOD_NAME"
                fi
            done
        else
            warn "sign-file script not found in kernel headers!"
            warn "Modules could not be signed. Secure Boot will block them unless you sign them manually or disable Secure Boot."
        fi

        # Enrol MOK if not yet enrolled
        if mokutil --test-key "$MOK_DIR/MOK.der" 2>/dev/null | grep -qi "not enrolled"; then
            info "Enrolling MOK key..."
            MOK_PASSWORD="omen"
            printf "%s\n%s\n" "$MOK_PASSWORD" "$MOK_PASSWORD" | mokutil --import "$MOK_DIR/MOK.der" 2>/dev/null \
                || warn "Failed to import MOK key."

            echo ""
            echo -e "${YELLOW}╔═══════════════════════════════════════════════════════════╗${NC}"
            echo -e "${YELLOW}║  🔒 Secure Boot is ENABLED                                ║${NC}"
            echo -e "${YELLOW}║                                                           ║${NC}"
            echo -e "${YELLOW}║  A Machine Owner Key (MOK) has been registered to sign    ║${NC}"
            echo -e "${YELLOW}║  the custom drivers.                                      ║${NC}"
            echo -e "${YELLOW}║                                                           ║${NC}"
            echo -e "${YELLOW}║  ${RED}PLEASE REBOOT YOUR SYSTEM NOW.${YELLOW}                           ║${NC}"
            echo -e "${YELLOW}║  Upon reboot, a blue 'Perform MOK management' screen      ║${NC}"
            echo -e "${YELLOW}║  will appear. Follow these exact steps:                   ║${NC}"
            echo -e "${YELLOW}║                                                           ║${NC}"
            echo -e "${YELLOW}║  1. Select 'Enroll MOK'                                   ║${NC}"
            echo -e "${YELLOW}║  2. Select 'Continue'                                     ║${NC}"
            echo -e "${YELLOW}║  3. Select 'Yes'                                          ║${NC}"
            echo -e "${YELLOW}║  4. Enter password: ${GREEN}${MOK_PASSWORD}${YELLOW}$(printf '%*s' $((27 - ${#MOK_PASSWORD})) '')║${NC}"
            echo -e "${YELLOW}║  5. Select 'Reboot'                                       ║${NC}"
            echo -e "${YELLOW}╚═══════════════════════════════════════════════════════════╝${NC}"
            echo ""
            warn "Skipping module load. Modules will load automatically after MOK enrollment."
        else
            ok "MOK key is already enrolled."
        fi
    fi

    # ── Load modules ─────────────────────────────────────────────────────────
    # FIX: MOK_PENDING check now always safe because MOK_DIR is always defined
    MOK_PENDING=false
    if $SECUREBOOT && mokutil --test-key "$MOK_DIR/MOK.der" 2>/dev/null | grep -qi "not enrolled"; then
        MOK_PENDING=true
    fi

    if $MOK_PENDING; then
        info "MOK enrollment pending — skipping module load until reboot."
    else
        info "Loading modules..."
        # Unload any existing hp_wmi / hp_omen_extra first (order matters for deps)
        modprobe -r hp_omen_extra 2>/dev/null || true
        modprobe -r hp_wmi        2>/dev/null || true
        modprobe led_class_multicolor 2>/dev/null || true
        # Load the custom DKMS hp-wmi first (override conf ensures this wins)
        if modprobe hp_wmi 2>/dev/null; then
            ok "Custom hp-wmi loaded successfully"
        else
            warn "Custom hp-wmi could not be loaded — check: dmesg | tail -20"
        fi
        if modprobe hp_omen_extra 2>/dev/null; then
            ok "hp-omen-extra loaded successfully"
        else
            warn "hp-omen-extra could not be loaded."
        fi
        ok "Both hp-wmi and hp-omen-extra installed."
    fi

    echo ""
    info "The module will be automatically rebuilt on kernel updates via DKMS."
    info "Fan control: /sys/devices/platform/hp-wmi/hwmon/hwmon*/pwm1_enable"
    info "Fan speed:   /sys/devices/platform/hp-wmi/hwmon/hwmon*/fan*_target"
    info "GPU TGP:     /sys/devices/platform/hp-wmi/gpu_tgp"

    # Create modules-load.d entry so systemd loads both modules at boot.
    # hp_wmi must be listed first: it is a dependency of hp_omen_extra and
    # listing it explicitly guarantees the DKMS version (in updates/) loads
    # before the dependency resolver runs for hp_omen_extra.
    info "Configuring auto-load on boot..."
    # FIX #4: Never explicitly load hp_wmi via modules-load.d on abort-prone boards
    # (early WMI calls during systemd-modules-load.service hang EC). The hp-omen-extra
    # module exports symbols that pull hp_wmi in automatically when needed.
    if [ -z "${FORCE_EARLY_HP_WMI_LOAD:-}" ]; then
        printf 'hp_omen_extra\n' > /etc/modules-load.d/hp-omen-extra.conf
    else
        # Explicit early load only for Tegra/VM/testing environments that need it
        printf 'hp_wmi\nhp_omen_extra\n' > /etc/modules-load.d/hp-omen-extra.conf
    fi

    echo ""
}

# ── Uninstall ─────────────────────────────────────────────────────────────────

do_uninstall() {
    [[ $EUID -ne 0 ]] && error "This script must be run as root (use sudo)."

    info "Unloading modules..."
    # FIX: modprobe -r handles inter-module dependencies; rmmod does not
    modprobe -r hp_omen_extra 2>/dev/null || true
    modprobe -r hp_wmi          2>/dev/null || true

    info "Removing DKMS entry..."
    if dkms status "$MODNAME/$MODVER" 2>/dev/null | grep -q "$MODNAME"; then
        dkms remove -m "$MODNAME" -v "$MODVER" --all
        rm -rf "/usr/src/${MODNAME}-${MODVER}"
        ok "Uninstalled successfully."
    else
        warn "DKMS entry not found. Nothing to remove."
    fi

    # Remove modules-load.d entry
    rm -f /etc/modules-load.d/hp-omen-extra.conf

    # Remove the modprobe override so the stock hp-wmi is re-enabled
    remove_hpwmi_override

    # Purge any stale .ko files left by previous backup-based installs
    info "Cleaning up any stale module files..."
    KVER=$(uname -r)

    # Remove old-style .backup files created by previous installer versions
    while IFS= read -r BU_FILE; do
        ORIG_FILE="${BU_FILE%.backup}"
        info "Restoring $ORIG_FILE from legacy backup..."
        mv "$BU_FILE" "$ORIG_FILE"
    done < <(find_module_paths "hp-wmi.ko*.backup" "$KVER")

    depmod -a

    info "Reloading stock hp-wmi module..."
    modprobe hp_wmi 2>/dev/null || warn "Could not reload stock hp-wmi — may need a reboot."

    ok "omen-space driver uninstalled. Stock hp-wmi restored."
}

# ── Helper (also used in uninstall) ──────────────────────────────────────────
find_module_paths() {
    local pattern="$1"
    local kver="${2:-$(uname -r)}"
    find \
        "/lib/modules/$kver" \
        "/usr/lib/modules/$kver" \
        -name "$pattern" 2>/dev/null | sort -u
}

# ── Main ──────────────────────────────────────────────────────────────────────

usage() {
    echo "Usage: sudo $0 [install|uninstall]"
    echo ""
    echo "  install      Build and install the module via DKMS"
    echo "  uninstall    Remove the module and restore the original"
    echo ""
    echo "If no argument is given, 'install' is assumed."
}

case "${1:-install}" in
    install)   do_install ;;
    uninstall) do_uninstall ;;
    -h|--help) usage ;;
    *)         usage; exit 1 ;;
esac
