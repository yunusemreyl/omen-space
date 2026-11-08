#!/usr/bin/env bash
set -e

if [[ $EUID -ne 0 ]]; then
   echo "This script must be run as root (use sudo ./setup.sh [install|update|uninstall])" 
   exit 1
fi

# Resolve calling user when launched via pkexec (where SUDO_USER is unset but PKEXEC_UID is set)
if [[ -z "${SUDO_USER:-}" && -n "${PKEXEC_UID:-}" && "${PKEXEC_UID}" != "0" ]]; then
    SUDO_USER="$(id -un "$PKEXEC_UID" 2>/dev/null || true)"
    export SUDO_USER
fi

for arg in "$@"; do
    case "$arg" in
        --non-interactive|-y)
            export OMEN_NONINTERACTIVE=1
            ;;
        --from-gui)
            export OMEN_UPDATER_UI=1
            export OMEN_NONINTERACTIVE=1
            ;;
    esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

emit_step() {
    local step_num="$1"
    local pct="$2"
    local msg="$3"
    echo "::OMEN_STEP::${step_num}::${pct}::${msg}"
}

install_dependencies() {
    echo "====================================="
    echo " Installing system dependencies..."
    echo "====================================="
    if command -v dnf &> /dev/null; then
        echo "Detected Fedora/RHEL. Installing dependencies via dnf..."
        dnf install -y gcc pkgconf-pkg-config gtk4-devel libadwaita-devel gtk4-layer-shell-devel systemd-devel make kernel-devel kernel-headers dbus-devel dkms hidapi-devel
    elif command -v apt-get &> /dev/null; then
        echo "Detected Debian/Ubuntu. Installing dependencies via apt..."
        export DEBIAN_FRONTEND=noninteractive
        apt-get update -qq || apt-get update
        # FIX #252: libgtk4-layer-shell-dev is unavailable on some Linux Mint / older Ubuntu
        # versions where the package hasn't been backported yet. Install mandatory packages
        # first, then try the layer-shell package gracefully so the build doesn't abort.
        apt-get install -y build-essential pkg-config libgtk-4-dev libadwaita-1-dev libsystemd-dev libdbus-1-dev dkms linux-headers-$(uname -r) libhidapi-dev
        if apt-get install -y libgtk4-layer-shell-dev 2>/dev/null; then
            echo "[✓] libgtk4-layer-shell-dev installed."
        else
            echo "[!] Warning: libgtk4-layer-shell-dev not found in apt repos (common on Linux Mint / Ubuntu < 24.04)."
            echo "    The overlay/tray components may not build. You can try installing it manually:"
            echo "    https://github.com/wmww/gtk4-layer-shell"
            echo "    Continuing build without it..."
        fi
    elif command -v pacman &> /dev/null; then
        echo "Detected Arch Linux. Installing dependencies via pacman..."
        local ARCH_PKGS=(gcc pkgconf gtk4 libadwaita gtk4-layer-shell systemd dbus base-devel dkms hidapi)

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
        zypper install -y gcc make pkgconfig gtk4-devel libadwaita-devel gtk4-layer-shell-devel systemd-devel dbus-1-devel kernel-devel dkms libhidapi-devel
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
    if [[ "${OMEN_NONINTERACTIVE:-0}" == "1" ]] || [[ ! -t 0 ]]; then
        echo "   Non-interactive mode active: proceeding automatically..."
        return 0
    fi
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
    rm -f /etc/modprobe.d/omen-space-hpwmi-override.conf

    systemctl daemon-reload
    systemctl reload dbus 2>/dev/null || true
    echo "Legacy cleanup complete."
}

rustc_meets_min_version() {
    local rustc_bin="$1"
    [[ -x "$rustc_bin" ]] || return 1
    local ver major minor
    ver=$("$rustc_bin" --version 2>/dev/null | awk '{print $2}')
    major=$(echo "$ver" | cut -d. -f1)
    minor=$(echo "$ver" | cut -d. -f2)
    [[ -n "$major" && -n "$minor" ]] || return 1
    if [[ "$major" -gt 1 ]] || { [[ "$major" -eq 1 ]] && [[ "$minor" -ge 85 ]]; }; then
        return 0
    fi
    return 1
}

do_build() {
    emit_step 3 50 "Compiling OmenSpace workspace (cargo build --release)..."
    echo "====================================="
    echo " Building OMENSpace (Daemon, CLI, GUI, Tray)"
    echo "====================================="

    # FIX #268 & #349: On openSUSE Tumbleweed, libhidapi-hidraw is not available as a
    # dynamic shared library. Switch to linux-static-hidraw before building if source exists.
    if command -v zypper &> /dev/null && [ -f "$SCRIPT_DIR/src/omen-space-daemon/Cargo.toml" ]; then
        echo "[fix #268] openSUSE detected: switching daemon hidapi backend to linux-static-hidraw..."
        sed -i 's/linux-shared-hidraw/linux-static-hidraw/g' \
            "$SCRIPT_DIR/src/omen-space-daemon/Cargo.toml" || true
    fi

    local user_home=""
    if [[ -n "${SUDO_USER:-}" && "$SUDO_USER" != "root" ]]; then
        user_home=$(getent passwd "$SUDO_USER" 2>/dev/null | cut -d: -f6 || true)
    fi

    # 1. Prefer SUDO_USER's rustup toolchain if present
    if [[ -n "$user_home" && -x "$user_home/.cargo/bin/cargo" ]]; then
        if ! rustc_meets_min_version "$user_home/.cargo/bin/rustc"; then
            if [[ -x "$user_home/.cargo/bin/rustup" ]]; then
                echo "Updating Rust toolchain for $SUDO_USER to stable (>= 1.85)..."
                su - "$SUDO_USER" -c "export PATH=\"\$HOME/.cargo/bin:\$PATH\"; rustup update stable" || true
            fi
        fi
        if rustc_meets_min_version "$user_home/.cargo/bin/rustc"; then
            echo "Using Rust toolchain from $user_home/.cargo/bin (user: $SUDO_USER)..."
            export CARGO_HOME="$user_home/.cargo"
            export RUSTUP_HOME="${user_home}/.rustup"
            export PATH="$user_home/.cargo/bin:$PATH"
            cargo build --release
            return 0
        fi
    fi

    # 2. Check root's ~/.cargo/bin or system PATH
    export CARGO_HOME=/root/.cargo
    export RUSTUP_HOME=/root/.rustup
    export PATH="/root/.cargo/bin:$PATH"

    if ! command -v cargo &> /dev/null; then
        echo "cargo is not installed. Installing Rust via rustup automatically..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        export PATH="/root/.cargo/bin:$PATH"
    fi

    local active_rustc
    active_rustc=$(command -v rustc 2>/dev/null || true)
    if ! rustc_meets_min_version "$active_rustc"; then
        if command -v rustup &> /dev/null; then
            echo "Updating Rust toolchain via rustup..."
            rustup update stable || true
        else
            echo "System Rust toolchain is older than 1.85; installing modern toolchain via rustup..."
            curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
            export PATH="/root/.cargo/bin:$PATH"
        fi
        active_rustc=$(command -v rustc 2>/dev/null || true)
    fi

    if ! rustc_meets_min_version "$active_rustc"; then
        local found_ver
        found_ver=$("$active_rustc" --version 2>/dev/null | awk '{print $2}' || echo "none")
        echo ""
        echo "❌ ERROR: Rust toolchain too old (found: $found_ver, required: ≥ 1.85)"
        echo "   Please upgrade your Rust toolchain: rustup update stable"
        exit 1
    fi

    cargo build --release
}

do_install() {
    emit_step 4 75 "Installing binaries & system configuration..."
    echo "====================================="
    echo " Stopping existing services"
    echo "====================================="
    systemctl stop omen-space-daemon.service 2>/dev/null || true
    killall omen-tray 2>/dev/null || true
    killall omen-overlay 2>/dev/null || true
    if [[ "${OMEN_UPDATER_UI:-0}" != "1" ]]; then
        killall omen-gui 2>/dev/null || true
    else
        echo "Updater UI active: keeping running omen-gui instance alive during file replacement."
    fi

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
    install -m 755 "$SCRIPT_DIR/setup.sh" /usr/share/omen-space/setup.sh
    echo "$SCRIPT_DIR" > /etc/omen-space/repo_path

    # Record installed build metadata (version + git commit + timestamp)
    local pkg_ver git_sha git_short now_iso
    pkg_ver=$(grep -m1 '^version[[:space:]]*=' "$SCRIPT_DIR/src/omen-gui/Cargo.toml" 2>/dev/null | sed -E 's/.*"([^"]+)".*/\1/' || echo "unknown")
    git_sha=$(git -c safe.directory="$SCRIPT_DIR" -C "$SCRIPT_DIR" rev-parse HEAD 2>/dev/null || echo "unknown")
    git_short=$(git -c safe.directory="$SCRIPT_DIR" -C "$SCRIPT_DIR" rev-parse --short=7 HEAD 2>/dev/null || echo "unknown")
    now_iso=$(date -u +"%Y-%m-%dT%H:%M:%SZ" 2>/dev/null || echo "")
    cat <<EOF > /etc/omen-space/build_info.json
{
  "version": "${pkg_ver}",
  "commit_sha": "${git_sha}",
  "commit_short": "${git_short}",
  "updated_at": "${now_iso}",
  "repo_path": "${SCRIPT_DIR}"
}
EOF
    chmod 644 /etc/omen-space/build_info.json 2>/dev/null || true

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

    emit_step 4 82 "Building & installing DKMS kernel drivers (hp-wmi & hp-omen-extra)..."
    echo "====================================="
    echo " Installing DKMS Kernel Driver"
    echo "====================================="
    cd driver
    chmod +x setup.sh
    ./setup.sh install || true
    cd ..

    emit_step 5 94 "Starting omen-space-daemon service & verifying connection..."
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
            killall -q -u "$SUDO_USER" omen-tray 2>/dev/null || true
            su - "$SUDO_USER" -c "XDG_RUNTIME_DIR=/run/user/$user_id /usr/bin/omen-tray >/dev/null 2>&1 &" || true
        fi
    fi

    echo "====================================="
    echo " Cleaning build cache to free disk space..."
    echo "====================================="
    rm -rf target src/*/target

    # Restore file ownership if building inside a user's home directory
    if [[ -n "$SUDO_USER" && "$SUDO_USER" != "root" && "$SCRIPT_DIR" == /home/* ]]; then
        chown -R "$SUDO_USER":"$SUDO_USER" "$SCRIPT_DIR" 2>/dev/null || true
    fi

    emit_step 6 100 "Update completed successfully!"
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
    rm -f /etc/modprobe.d/omen-space-hpwmi-override.conf

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

    # Fallback cleanup in case driver/setup.sh was not present or version mismatched (#302)
    modprobe -r hp_omen_extra 2>/dev/null || true
    modprobe -r hp_wmi 2>/dev/null || true
    if command -v dkms &>/dev/null; then
        for v in $(dkms status 2>/dev/null | grep -i 'hp-omen-extra' | grep -oP '(?<=hp-omen-extra[/, ])[^,:]+' | tr -d ' ' | sort -u); do
            [ -z "$v" ] && continue
            dkms remove -m "hp-omen-extra" -v "$v" --all 2>/dev/null || true
        done
    fi
    rm -rf /usr/src/hp-omen-extra-* 2>/dev/null || true
    rm -f /etc/modules-load.d/hp-omen-extra.conf
    KVER=$(uname -r)
    find /lib/modules/"$KVER" /usr/lib/modules/"$KVER" -name 'hp-omen-extra.ko*' -delete 2>/dev/null || true
    find /lib/modules/"$KVER" /usr/lib/modules/"$KVER" -path '*/updates/*' -name 'hp-wmi.ko*' -delete 2>/dev/null || true
    depmod -a 2>/dev/null || true
    modprobe hp_wmi 2>/dev/null || true

    echo "Uninstallation complete!"
}

prepare_update_repo() {
    emit_step 1 12 "Fetching latest OmenSpace source from GitHub..."
    echo "====================================="
    echo " Updating OMENSpace repository..."
    echo "====================================="

    if [ ! -f "Cargo.toml" ]; then
        local saved_repo=""
        if [ -f "/etc/omen-space/repo_path" ]; then
            saved_repo=$(tr -d '\r\n' < /etc/omen-space/repo_path || true)
        fi
        if [ -n "$saved_repo" ] && [ -f "$saved_repo/Cargo.toml" ] && [ -d "$saved_repo/.git" ]; then
            echo "Using existing repository at $saved_repo..."
            export OMEN_POST_PULL=0
            exec bash "$saved_repo/setup.sh" update "$@"
        fi

        CACHE_REPO="/var/cache/omen-space/repo"
        echo "Source tree not found in $(pwd); fetching latest repository into $CACHE_REPO..."
        mkdir -p "/var/cache/omen-space"
        if [ -d "$CACHE_REPO/.git" ]; then
            git -c safe.directory="$CACHE_REPO" -C "$CACHE_REPO" fetch --all --prune && \
            git -c safe.directory="$CACHE_REPO" -C "$CACHE_REPO" reset --hard origin/main
        else
            rm -rf "$CACHE_REPO"
            git clone --depth 1 https://github.com/yunusemreyl/omen-space.git "$CACHE_REPO"
        fi
        mkdir -p /etc/omen-space
        echo "$CACHE_REPO" > /etc/omen-space/repo_path
        export OMEN_POST_PULL=1
        exec bash "$CACHE_REPO/setup.sh" update "$@"
    fi

    if [ -d ".git" ] && [ "${OMEN_POST_PULL:-0}" != "1" ]; then
        echo "Pulling latest changes from origin/main in $SCRIPT_DIR..."
        # Revert automated build-time edits so git pull never aborts on Cargo.toml/Cargo.lock
        git -c safe.directory="$SCRIPT_DIR" checkout -- src/omen-space-daemon/Cargo.toml Cargo.lock 2>/dev/null || true

        if git -c safe.directory="$SCRIPT_DIR" fetch origin main --prune; then
            if ! git -c safe.directory="$SCRIPT_DIR" merge --ff-only FETCH_HEAD; then
                if [[ "$SCRIPT_DIR" == "/var/cache/omen-space/repo" ]] || [[ "${OMEN_UPDATER_UI:-0}" == "1" ]]; then
                    echo "Fast-forward merge not possible; resetting working tree to origin/main..."
                    git -c safe.directory="$SCRIPT_DIR" reset --hard FETCH_HEAD
                else
                    echo "Warning: Local repository has diverged from origin/main; building current checkout."
                fi
            fi
            export OMEN_POST_PULL=1
            exec bash "$SCRIPT_DIR/setup.sh" update "$@"
        else
            echo "Warning: Failed to fetch from origin/main. Building current checkout..."
        fi
    fi
}

do_update() {
    prepare_update_repo "$@"

    emit_step 2 28 "Checking system dependencies & cleaning old DKMS modules..."
    check_conflicting_power_managers
    install_dependencies
    remove_legacy_omenctl

    # FIX #254: Remove ALL previously installed DKMS versions for hp-omen-extra
    # before rebuilding so that old modules don't accumulate across updates.
    echo "====================================="
    echo " Cleaning up old DKMS modules..."
    echo "====================================="
    if command -v dkms &>/dev/null; then
        OLD_VERSIONS=$(dkms status 2>/dev/null | grep -i 'hp-omen-extra' | grep -oP '(?<=hp-omen-extra[/, ])[^,:]+' | tr -d ' ' | sort -u)
        if [ -n "$OLD_VERSIONS" ]; then
            for v in $OLD_VERSIONS; do
                echo "Removing old DKMS module: hp-omen-extra/$v"
                dkms remove -m "hp-omen-extra" -v "$v" --all 2>/dev/null || true
                rm -rf "/usr/src/hp-omen-extra-$v" 2>/dev/null || true
            done
        fi
    fi

    do_build
    do_install
}

# Subcommand routing
COMMAND=${1:-install}
shift || true

case "$COMMAND" in
    install)
        if [ ! -f "$SCRIPT_DIR/Cargo.toml" ]; then
            echo "Source tree not found in $SCRIPT_DIR; fetching repository..."
            prepare_update_repo install "$@"
        fi
        emit_step 1 15 "Checking system dependencies..."
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
        do_update "$@"
        ;;
    *)
        echo "Usage: sudo ./setup.sh [install|update|uninstall] [--non-interactive] [--from-gui]"
        echo "  install   : Builds and installs OMENSpace (cleans legacy omenctl)"
        echo "  update    : Pulls latest git changes, builds, and reinstalls"
        echo "  uninstall : Completely removes OMENSpace from the system"
        exit 1
        ;;
esac