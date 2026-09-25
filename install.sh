#!/usr/bin/env bash
# ==============================================================================
# OMENSpace Web Installer
# Automated installer for OMENSpace (Daemon, GUI, CLI, Tray, Kernel Driver)
# Repository: https://github.com/yunusemreyl/omen-space
# ==============================================================================

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
BLUE='\033[1;34m'
BOLD='\033[1m'
NC='\033[0m'

log() { echo -e "${GREEN}[✓]${NC} $1"; }
info() { echo -e "${CYAN}[i]${NC} $1"; }
warn() { echo -e "${YELLOW}[!]${NC} $1"; }
err() { echo -e "${RED}[✗]${NC} $1"; exit 1; }

REPO="yunusemreyl/omen-space"
TMP_DIR="/tmp/omen-space-install"

if [ "$EUID" -ne 0 ]; then
    err "This installer must be run as root. Please run:\n       curl -sSL https://raw.githubusercontent.com/$REPO/main/install.sh | sudo bash"
fi

echo -e "${CYAN}"
cat << "BANNER"
  ____  __  ________  _   __   _____ ____  ___   ____________
 / __ \/  |/  / ____// | / /  / ___// __ \/   | / ____/ ____/
/ / / / /|_/ / __/  /  |/ /   \__ \/ /_/ / /| |/ /   / __/   
/ /_/ / /  / / /___ / /|  /   ___/ / ____/ ___ / /___/ /___   
\____/_/  /_/_____//_/ |_/   /____/_/   /_/  |_\____/_____/   
BANNER
echo -e "${NC}"
echo -e "${BOLD}====================================================${NC}"
echo -e "${BOLD}       🚀 OMENSpace Automated Installer             ${NC}"
echo -e "${BOLD}====================================================${NC}"

# ------------------------------------------------------------------------------
# 1. Resolve Installation Channel (Stable vs Canary)
# ------------------------------------------------------------------------------
CHANNEL=""
ARG="${1:-}"

case "${ARG,,}" in
    --stable|-s|stable|1)
        CHANNEL="stable"
        ;;
    --canary|-c|canary|2)
        CHANNEL="canary"
        ;;
    "")
        # Interactive selection
        if [[ "${LANG:-}" == tr_* ]] || [[ "${LC_ALL:-}" == tr_* ]]; then
            echo -e "\nLütfen kurulum kanalını seçin:\n"
            echo -e "  ${BOLD}1) 🟢 Stable (Önerilen)${NC}"
            echo -e "     Doğrulanmış resmi sürüm. Maksimum kararlılık."
            echo -e ""
            echo -e "  ${BOLD}2) 🟡 Canary (Güncel Kod)${NC}"
            echo -e "     'main' dalındaki en güncel kodlar ve en yeni özellikler."
            echo -e ""
            PROMPT_TEXT="Seçiminiz [1/2] (Varsayılan: 1): "
        else
            echo -e "\nPlease choose your installation channel:\n"
            echo -e "  ${BOLD}1) 🟢 Stable (Recommended)${NC}"
            echo -e "     Official verified release. Maximum stability."
            echo -e ""
            echo -e "  ${BOLD}2) 🟡 Canary (Bleeding Edge)${NC}"
            echo -e "     Latest commits from 'main' branch. Newest features & fixes."
            echo -e ""
            PROMPT_TEXT="Select [1/2] (Default: 1): "
        fi

        CHOICE=""
        # Handle piping: if stdin is a pipe from curl, read from /dev/tty
        if [ -t 0 ]; then
            read -rp "$PROMPT_TEXT" CHOICE || true
        elif [ -e /dev/tty ]; then
            read -rp "$PROMPT_TEXT" CHOICE < /dev/tty || true
        else
            warn "No TTY detected. Defaulting to Stable channel."
            CHOICE="1"
        fi

        case "$CHOICE" in
            2|canary|Canary)
                CHANNEL="canary"
                ;;
            *)
                CHANNEL="stable"
                ;;
        esac
        ;;
    *)
        err "Invalid parameter: '$ARG'. Usage: sudo ./install.sh [--stable | --canary]"
        ;;
esac

# ------------------------------------------------------------------------------
# 2. Determine target Git Ref (Tag vs Branch)
# ------------------------------------------------------------------------------
TARGET_REF="main"
IS_TAG=false

if [ "$CHANNEL" == "stable" ]; then
    info "Resolving latest stable release tag..."
    
    # Try fetching latest release tag or top tag from GitHub
    LATEST_TAG=""
    if command -v curl &> /dev/null; then
        LATEST_TAG=$(curl -s "https://api.github.com/repos/$REPO/tags" 2>/dev/null | grep -m 1 '"name":' | sed -E 's/.*"([^"]+)".*/\1/' || true)
    fi

    # Fallback to v2.0.0 if API query failed or was empty
    if [ -z "$LATEST_TAG" ]; then
        LATEST_TAG="v2.1.2"
    fi

    TARGET_REF="$LATEST_TAG"
    IS_TAG=true
    log "Channel selected: ${GREEN}STABLE${NC} (${BOLD}$TARGET_REF${NC})"
else
    TARGET_REF="main"
    IS_TAG=false
    log "Channel selected: ${YELLOW}CANARY${NC} (${BOLD}main branch - latest commits${NC})"
fi

# ------------------------------------------------------------------------------
# 3. Fetch Source Code (Git clone or Tarball)
# ------------------------------------------------------------------------------
rm -rf "$TMP_DIR"
mkdir -p "$TMP_DIR"

if command -v git &> /dev/null; then
    info "Fetching OMENSpace source ($TARGET_REF)..."
    git clone --depth 1 -b "$TARGET_REF" "https://github.com/$REPO.git" "$TMP_DIR"
    cd "$TMP_DIR"
else
    info "Git not found. Downloading source archive..."
    cd "$TMP_DIR"
    if [ "$IS_TAG" = true ]; then
        TARBALL_URL="https://github.com/$REPO/archive/refs/tags/$TARGET_REF.tar.gz"
    else
        TARBALL_URL="https://github.com/$REPO/archive/refs/heads/$TARGET_REF.tar.gz"
    fi

    if command -v curl &> /dev/null; then
        curl -fsSL "$TARBALL_URL" -o omen-space.tar.gz
    elif command -v wget &> /dev/null; then
        wget -qO omen-space.tar.gz "$TARBALL_URL"
    else
        err "Neither git, curl, nor wget found. Please install one of them."
    fi

    tar -xzf omen-space.tar.gz --strip-components=1
    rm -f omen-space.tar.gz
fi

if [ ! -f "setup.sh" ]; then
    err "setup.sh not found in the downloaded archive. Installation aborted."
fi

chmod +x setup.sh

# ------------------------------------------------------------------------------
# 4. Run Setup
# ------------------------------------------------------------------------------
info "Starting installation (dependencies, kernel module, and build)..."
./setup.sh install

# Cleanup temporary files
cd /tmp
rm -rf "$TMP_DIR"

echo -e ""
echo -e "${GREEN}====================================================${NC}"
echo -e "${GREEN}  🎉 OMENSpace ($TARGET_REF) installed successfully!${NC}"
echo -e "${GREEN}  Launch GUI via app menu or terminal:              ${NC}"
echo -e "${BOLD}       omen-gui                                     ${NC}"
echo -e "${GREEN}  CLI control:                                      ${NC}"
echo -e "${BOLD}       omen-cli --help                              ${NC}"
echo -e "${GREEN}====================================================${NC}"
