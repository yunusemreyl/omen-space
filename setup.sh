#!/usr/bin/env bash
set -e

if [[ $EUID -ne 0 ]]; then
   echo "This script must be run as root (use sudo ./setup.sh [install|update|uninstall])" 
   exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

install_dependencies() {
    echo "====================================="
    echo " Installing system dependencies..."
    echo "====================================="
    if command -v dnf &> /dev/null; then
        echo "Detected Fedora/RHEL. Installing dependencies via dnf..."
        dnf install -y gcc pkgconf-pkg-config gtk4-devel libadwaita-devel systemd-devel make kernel-devel kernel-headers dbus-devel dkms hidapi-devel
    elif command -v apt-get &> /dev/null; then
        echo "Detected Debian/Ubuntu. Installing dependencies via apt..."
        apt-get update
        apt-get install -y build-essential pkg-config libgtk-4-dev libadwaita-1-dev libsystemd-dev libdbus-1-dev dkms linux-headers-$(uname -r) libhidapi-dev
    elif command -v pacman &> /dev/null; then
        echo "Detected Arch Linux. Installing dependencies via pacman..."
        local ARCH_PKGS=(gcc pkgconf gtk4 libadwaita systemd dbus base-devel dkms hidapi)

        # Only install headers if not already available for the running kernel
        if [ ! -d "/lib/modules/$(uname -r)/build" ] && [ ! -d "/usr/lib/modules/$(uname -r)/build" ]; then
            local RUNNING_KVER
            RUNNING_KVER=$(uname -r)
            if [[ $RUNNING_KVER == *"-cachyos"* ]]; then
                local SUFFIX
                SUFFIX=$(echo "$RUNNING_KVER" | sed 's/^[0-9.]*-[0-9]*-\(.*\)/\1/')
                if [[ -n "$SUFFIX" ]] && pacman -Si "linux-$SUFFIX-headers" &>/dev/null 2>&1; then
                    ARCH_PKGS+=("linux-$SUFFIX-headers")
                elif pacman -Si linux-cachyos-headers &>/dev/null 2>&1; then
                    ARCH_PKGS+=("linux-cachyos-headers")
                else
                    ARCH_PKGS+=("linux-headers")
                fi
            elif [[ $RUNNING_KVER == *"-zen"* ]]; then
                ARCH_PKGS+=("linux-zen-headers")
            elif [[ $RUNNING_KVER == *"-lts"* ]]; then
                ARCH_PKGS+=("linux-lts-headers")
            elif [[ $RUNNING_KVER == *"-hardened"* ]]; then
                ARCH_PKGS+=("linux-hardened-headers")
            elif [[ $RUNNING_KVER == *"-rt"* ]]; then
                ARCH_PKGS+=("linux-rt-headers")
            else
                local KVER_MAJOR KVER_MINOR VERSIONED_PKG
                KVER_MAJOR=$(echo "$RUNNING_KVER" | cut -d. -f1)
                KVER_MINOR=$(echo "$RUNNING_KVER" | cut -d. -f2)
                VERSIONED_PKG="linux${KVER_MAJOR}${KVER_MINOR}-headers"
                if pacman -Si "$VERSIONED_PKG" &>/dev/null 2>&1; then
                    ARCH_PKGS+=("$VERSIONED_PKG")
                else
                    ARCH_PKGS+=("linux-headers")
                fi
            fi
        fi
        # FIX #5: Hard guard against partial systemd upgrade (#211 adjacent).
        # pacman -S --needed systemd dbus with stale sync DB + old installed = partial
        # upgrade = next boot → systemd crash → emergency mode. Require user to run
        # a FULL pacman -Syu first.
        if command -v pacman >/dev/null 2>&1; then
            echo "=> Arch-based system detected: verifying packages are fully up-to-date to avoid partial systemd upgrade risk"
            if pacman -Qu 2>/dev/null | grep -qE '^(systemd|dbus|glibc|linux-cachyos|linux) '; then
                echo "ERROR: Your system has pending core updates that would cause a partial upgrade. Run:"
                echo " sudo pacman -Syu"
                echo "then reboot (to confirm updated systemd/kernel boots fine) and re-run setup.sh."
                exit 1
            fi
            # Now safe to install packages
            pacman -S --needed --noconfirm "${ARCH_PKGS[@]}" || true
        fi
    elif command -v zypper &> /dev/null; then
        echo "Detected openSUSE. Installing dependencies via zypper..."
        zypper install -y gcc make pkgconfig gtk4-devel libadwaita-devel systemd-devel dbus-1-devel kernel-devel dkms libhidapi-devel
    else
        echo "Warning: Unsupported package manager. Please ensure gcc, make, pkgconfig, gtk4, libadwaita, and hidapi dev packages are installed."
    fi
}

# FIX #156: Check for conflicting power management tools before any package changes.
# The old OmenCtl installer removed system76-power without warning to satisfy the
# power-profiles-daemon dependency.  We detect this upfront and ask the user before
# proceeding so they can make an informed choice.
check_conflicting_power_managers() {
    local CONFLICTS=()

    # system76-power conflicts with power-profiles-daemon on Arch/Manjaro
    if command -v system76-power &>/dev/null || \
       (command -v pacman &>/dev/null && pacman -Qq system76-power &>/dev/null 2>&1); then
        CONFLICTS+=("system76-power")
    fi

    if [ ${#CONFLICTS[@]} -eq 0 ]; then
        return 0
    fi

    echo ""
    echo "⚠️  WARNING: Conflicting power management software detected:"
    for PKG in "${CONFLICTS[@]}"; do
        echo "   • $PKG"
    done
    echo ""
    echo "   OMENSpace relies on power-profiles-daemon for thermal profile integration."
    echo "   Your package manager may automatically remove the above package(s) when"
    echo "   power-profiles-daemon is installed as a dependency."
    echo ""
    echo "   If you want to keep ${CONFLICTS[*]}, cancel now and manage the conflict manually."
    echo ""
    read -r -p "   Continue installation anyway? [y/N]: " REPLY
    case "$REPLY" in
        [yY][eE][sS]|[yY]) echo "   Proceeding..." ;;
        *)
            echo "   Installation aborted. No changes were made."
            exit 0
            ;;
    esac
    echo ""
}


remove_legacy_omenctl() {
    echo "====================================="
    echo " Cleaning up legacy omenctl / OmenCtl microservices..."
    echo "====================================="
    # Stop & disable all legacy services
    systemctl stop omenctl.service hpm-fan.service hpm-power.service hpm-rgb.service hpm-mux.service hpm-platform.service 2>/dev/null || true
    systemctl disable omenctl.service hpm-fan.service hpm-power.service hpm-rgb.service hpm-mux.service hpm-platform.service 2>/dev/null || true
    pkill -f "python3.*(hp-manager|omen-cli|omenctl)" 2>/dev/null || true

    # Systemd service definitions
    rm -f /etc/systemd/system/omenctl.service
    rm -f /etc/systemd/system/hpm-fan.service
    rm -f /etc/systemd/system/hpm-power.service
    rm -f /etc/systemd/system/hpm-rgb.service
    rm -f /etc/systemd/system/hpm-mux.service
    rm -f /etc/systemd/system/hpm-platform.service

    # Legacy D-Bus policies
    rm -f /etc/dbus-1/system.d/com.yyl.hpmanager.*.conf

    # Legacy binaries and directories
    rm -f /usr/bin/omenctl
    rm -f /usr/bin/omenctl-gui
    rm -f /usr/bin/omenctl-tray
    rm -f /usr/bin/hp-manager
    rm -f /usr/bin/hp-manager-uninstall
    rm -rf /usr/libexec/hp-manager
    rm -rf /usr/share/hp-manager
    rm -rf /etc/hp-manager

    # Legacy desktop and autostart files
    rm -f /usr/share/applications/omenctl.desktop
    rm -f /usr/share/applications/com.yyl.hpmanager.desktop
    rm -f /etc/xdg/autostart/omenctl-bg.desktop
    rm -f /usr/share/icons/hicolor/48x48/apps/omenctl.png

    # Legacy state files
    rm -rf /var/lib/hp-manager

    # Legacy kernel module (hp-rgb-lighting DKMS — OmenCtl installs this instead of hp-omen-extra)
    if command -v dkms &>/dev/null; then
        for v in $(dkms status 2>/dev/null | grep -i 'hp-rgb-lighting' | grep -oP '(?<=hp-rgb-lighting[/, ])[^,:]+' | tr -d ' ' | sort -u); do
            dkms remove -m "hp-rgb-lighting" -v "$v" --all 2>/dev/null || true
        done
    fi
    modprobe -r hp_rgb_lighting 2>/dev/null || true
    rm -rf /usr/src/hp-rgb-lighting-*
    rm -f /etc/modules-load.d/hp-rgb-lighting.conf

    systemctl daemon-reload
    systemctl reload dbus 2>/dev/null || true
    echo "Legacy cleanup complete."
}

do_build() {
    echo "====================================="
    echo " Building OMENSpace (Daemon, CLI, GUI, Tray)"
    echo "====================================="
    export CARGO_HOME=/root/.cargo
    export RUSTUP_HOME=/root/.rustup
    
    # Check for cargo, instruct user to install rustup if missing
    if ! command -v cargo &> /dev/null; then
        if [[ -n "$SUDO_USER" ]] && su - "$SUDO_USER" -c "command -v cargo" &> /dev/null; then
            echo "cargo found for user $SUDO_USER, proceeding with build..."
        else
            echo "cargo is not installed. Installing Rust via rustup automatically..."
            if [[ -n "$SUDO_USER" ]]; then
                su - "$SUDO_USER" -c "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y"
            else
                curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
                export PATH="$HOME/.cargo/bin:$PATH"
            fi
        fi
    fi

    if ! command -v cargo &> /dev/null; then
        echo "cargo not found for root. Attempting to build as SUDO_USER if available..."
        if [[ -n "$SUDO_USER" ]]; then
            chown -R "$SUDO_USER":"$SUDO_USER" "$SCRIPT_DIR"
            su - "$SUDO_USER" -c "export PATH=\"\$HOME/.cargo/bin:\$PATH\"; cd '$SCRIPT_DIR' && cargo build --release"
        else
            echo "Error: cargo is not installed or not in PATH for root."
            exit 1
        fi
    else
        # FIX #234: Verify rustc >= 1.85 (required for edition 2024 transitive deps).
        # toml_datetime 1.1.1+spec-1.1.0 uses edition 2024 internally; older toolchains
        # fail with "feature edition2024 is required" — a confusing error for end-users.
        _RUSTC_VER=$(rustc --version 2>/dev/null | awk '{print $2}')
        _RUSTC_MAJOR=$(echo "$_RUSTC_VER" | cut -d. -f1)
        _RUSTC_MINOR=$(echo "$_RUSTC_VER" | cut -d. -f2)
        if [[ "$_RUSTC_MAJOR" -lt 1 ]] || { [[ "$_RUSTC_MAJOR" -eq 1 ]] && [[ "$_RUSTC_MINOR" -lt 85 ]]; }; then
            echo ""
            echo "❌ ERROR: Rust toolchain too old (found: $_RUSTC_VER, required: ≥ 1.85)"
            echo ""
            echo "   OMENSpace depends on packages that require Rust edition 2024."
            echo "   Please upgrade your Rust toolchain:"
            echo ""
            echo "     rustup update stable"
            echo ""
            echo "   If you installed Rust via your distro package manager (apt/dnf/pacman),"
            echo "   it may be out of date. Install the official toolchain via rustup instead:"
            echo ""
            echo "     curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
            echo ""
            exit 1
        fi
        cargo build --release
    fi
}

do_install() {
    echo "====================================="
    echo " Stopping existing services"
    echo "====================================="
    systemctl stop omen-space-daemon.service 2>/dev/null || true
    killall omen-tray 2>/dev/null || true
    killall omen-gui 2>/dev/null || true

    echo "====================================="
    echo " Installing system files"
    echo "====================================="
    mkdir -p /usr/libexec/omen-space
    mkdir -p /etc/omen-space
    mkdir -p /etc/dbus-1/system.d
    mkdir -p /etc/systemd/system
    mkdir -p /usr/lib/sysusers.d
    mkdir -p /usr/lib/udev/rules.d
    mkdir -p /usr/bin
    mkdir -p /usr/share/omen-space/assets
    mkdir -p /usr/share/applications
    mkdir -p /usr/share/pixmaps
    mkdir -p /etc/xdg/autostart
    mkdir -p /usr/share/dbus-1/services

    find_bin() {
        local name="$1"
        if [[ -f "$SCRIPT_DIR/target/release/$name" ]]; then
            echo "$SCRIPT_DIR/target/release/$name"
        elif [[ -n "$(ls $SCRIPT_DIR/target/*/release/$name 2>/dev/null)" ]]; then
            ls $SCRIPT_DIR/target/*/release/$name | head -n 1
        else
            echo ""
        fi
    }

    local daemon_bin=$(find_bin "omen-space-daemon")
    local cli_bin=$(find_bin "omen-cli")
    local tray_bin=$(find_bin "omen-tray")
    local gui_bin=$(find_bin "omen-gui")
    local overlay_bin=$(find_bin "omen-overlay")

    rm -f /usr/libexec/omen-space/omen-space-daemon
    cp "${daemon_bin:-target/release/omen-space-daemon}" /usr/libexec/omen-space/
    rm -f /usr/bin/omen-cli
    cp "${cli_bin:-target/release/omen-cli}" /usr/bin/
    rm -f /usr/bin/omen-tray
    cp "${tray_bin:-target/release/omen-tray}" /usr/bin/
    rm -f /usr/bin/omen-gui
    cp "${gui_bin:-target/release/omen-gui}" /usr/bin/
    rm -f /usr/bin/omen-overlay
    cp "${overlay_bin:-target/release/omen-overlay}" /usr/bin/

    install -m 644 data/org.hp.omen.conf /etc/dbus-1/system.d/
    install -m 644 data/omen-space-daemon.service /etc/systemd/system/
    install -m 644 data/sysusers.d/omen-space.conf /usr/lib/sysusers.d/
    install -m 644 data/99-omen-space.rules /usr/lib/udev/rules.d/
    rm -f /usr/share/applications/omen-space.desktop /usr/share/applications/org.hp.OmenSpace.desktop
    cp data/org.hp.OmenSpace.desktop /usr/share/applications/
    cp data/org.hp.OmenSpace.service /usr/share/dbus-1/services/
    mkdir -p /usr/share/icons/hicolor/512x512/apps
    cp src/omen-gui/assets/omenspace.png /usr/share/icons/hicolor/512x512/apps/omenspace.png
    cp src/omen-gui/assets/omenspace.png /usr/share/pixmaps/omenspace.png
    gtk-update-icon-cache -f -t /usr/share/icons/hicolor || true
    cp -r src/omen-gui/assets/* /usr/share/omen-space/assets/

    cat <<EOF > /etc/xdg/autostart/omenspace-tray.desktop
[Desktop Entry]
Name=OMEN SPACE Tray
Comment=OMENSpace System Tray Icon
Exec=/usr/bin/omen-tray
Icon=omenspace
Terminal=false
Type=Application
Categories=Utility;
EOF

    echo "Creating system users and reloading udev rules..."
    systemd-sysusers || true
    udevadm control --reload-rules && udevadm trigger || true

    # Automatically add the invoking sudo user to the omen-hw group
    if [[ -n "$SUDO_USER" && "$SUDO_USER" != "root" ]]; then
        echo "Adding user '$SUDO_USER' to omen-hw group..."
        usermod -aG omen-hw "$SUDO_USER" || true
    fi

    echo "====================================="
    echo " Installing DKMS Kernel Driver"
    echo "====================================="
    cd driver
    chmod +x setup.sh
    ./setup.sh install || true
    cd ..

    echo "====================================="
    echo " Starting omen-space-daemon service"
    echo "====================================="
    systemctl daemon-reload
    systemctl reload dbus || true
    systemctl enable --now omen-space-daemon.service

    echo "====================================="
    echo " Installation Complete!"
    echo " Testing omen-cli connection to backend..."
    echo "====================================="
    sleep 2 # Give daemon a moment to initialize on dbus

    if /usr/bin/omen-cli system info; then
        echo -e "\n✅ SUCCESS: CLI successfully communicated with the daemon!"
    else
        echo -e "\n❌ ERROR: CLI failed to communicate with the daemon. Check 'systemctl status omen-space-daemon'."
    fi

    if [ -n "$SUDO_USER" ] && [ -x /usr/bin/omen-tray ]; then
        local user_id
        user_id=$(id -u "$SUDO_USER")
        if [ -d "/run/user/$user_id" ]; then
            echo "Starting omen-tray for $SUDO_USER..."
            su - "$SUDO_USER" -c "DISPLAY=${DISPLAY:-:0} WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-wayland-0} XDG_RUNTIME_DIR=/run/user/$user_id /usr/bin/omen-tray >/dev/null 2>&1 &" || true
        fi
    fi

    echo "====================================="
    echo " Cleaning build cache to free disk space..."
    echo "====================================="
    rm -rf target src/*/target
}

do_uninstall() {
    echo "====================================="
    echo " Uninstalling OMENSpace..."
    echo "====================================="
    systemctl stop omen-space-daemon.service 2>/dev/null || true
    killall omen-tray 2>/dev/null || true
    killall omen-gui 2>/dev/null || true
    killall omen-overlay 2>/dev/null || true
    systemctl disable omen-space-daemon.service 2>/dev/null || true

    rm -rf /usr/libexec/omen-space
    rm -rf /etc/omen-space
    rm -rf /var/lib/omen-space /var/lib/omen-space-daemon
    rm -f /etc/dbus-1/system.d/org.hp.omen.conf
    rm -f /etc/systemd/system/omen-space-daemon.service
    rm -f /usr/lib/sysusers.d/omen-space.conf
    rm -f /usr/lib/udev/rules.d/99-omen-space.rules

    rm -f /usr/bin/omen-cli
    rm -f /usr/bin/omen-tray
    rm -f /usr/bin/omen-gui
    rm -f /usr/bin/omen-overlay

    rm -rf /usr/share/omen-space
    rm -f /usr/share/applications/omen-space.desktop
    rm -f /usr/share/applications/org.hp.OmenSpace.desktop
    rm -f /usr/share/dbus-1/services/org.hp.OmenSpace.service
    rm -f /usr/share/pixmaps/omenspace.png
    rm -f /usr/share/icons/hicolor/512x512/apps/omenspace.png
    gtk-update-icon-cache -f -t /usr/share/icons/hicolor || true
    rm -f /etc/xdg/autostart/omenspace-tray.desktop

    systemctl daemon-reload
    systemctl reload dbus || true
    udevadm control --reload-rules || true

    echo "Uninstalling DKMS Kernel Driver..."
    if [ -d "driver" ]; then
        cd driver
        chmod +x setup.sh
        ./setup.sh uninstall || true
        cd ..
    fi

    echo "Uninstallation complete!"
}

do_update() {
    echo "====================================="
    echo " Updating OMENSpace..."
    echo "====================================="
    if [ -d ".git" ]; then
        git pull || echo "Warning: Failed to pull latest changes. Building current version..."
    else
        echo "Warning: Not a git repository. Building current version..."
    fi
    do_build
    do_install
}

# Subcommand routing
COMMAND=${1:-install}

case "$COMMAND" in
    install)
        check_conflicting_power_managers
        install_dependencies
        remove_legacy_omenctl
        do_build
        do_install
        ;;
    uninstall)
        do_uninstall
        ;;
    update)
        check_conflicting_power_managers
        install_dependencies
        remove_legacy_omenctl
        do_update
        ;;
    *)
        echo "Usage: sudo ./setup.sh [install|update|uninstall]"
        echo "  install   : Builds and installs OMENSpace (cleans legacy omenctl)"
        echo "  update    : Pulls latest git changes, builds, and reinstalls"
        echo "  uninstall : Completely removes OMENSpace from the system"
        exit 1
        ;;
esac