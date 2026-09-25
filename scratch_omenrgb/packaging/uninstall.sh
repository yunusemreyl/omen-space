#!/usr/bin/env bash
# uninstall.sh — wycofuje wszystko, co zalozyl install.sh, i oddaje klawiature
# firmware'owi (wraca fabryczne zolto-pomaranczowe pulsowanie).
#
#   bash packaging/uninstall.sh                 (BEZ sudo z przodu)
#   bash packaging/uninstall.sh --keep-config   zostaw profile i cache
#   bash packaging/uninstall.sh --legacy        usun tez slady starego prototypu
#   bash packaging/uninstall.sh --lang en       wymus jezyk komunikatow
set -uo pipefail

SRC="$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)"

# Prescan dla --lang — patrz install.sh, ten sam powod (i18n.sh definiuje
# detect_lang() dopiero po zrodlowaniu, wiec nie mozna go wolac wczesniej).
ARGS=("$@")
LANG_OVERRIDE=""
i=0
while [ $i -lt ${#ARGS[@]} ]; do
    case "${ARGS[$i]}" in
        --lang) i=$((i + 1)); LANG_OVERRIDE="${ARGS[$i]:-}" ;;
        --lang=*) LANG_OVERRIDE="${ARGS[$i]#--lang=}" ;;
    esac
    i=$((i + 1))
done
LANG_CODE="$LANG_OVERRIDE"
# shellcheck source=/dev/null
. "$SRC/packaging/i18n.sh"

KEEP_CONFIG=0; LEGACY=0
i=0
while [ $i -lt ${#ARGS[@]} ]; do
    a="${ARGS[$i]}"
    case "$a" in
        --keep-config) KEEP_CONFIG=1 ;;
        --legacy) LEGACY=1 ;;
        --lang) i=$((i + 1)) ;;      # juz obsluzone w prescanie powyzej
        --lang=*) ;;                  # jw.
        -h|--help) msgln uninstall.help; exit 0 ;;
        *) emsgln unknown_option "$a"; exit 1 ;;
    esac
    i=$((i + 1))
done

if [ -f /run/.containerenv ] || [ -f /run/.toolboxenv ]; then
    NAME="$(sed -n 's/^name="\(.*\)"/\1/p' /run/.containerenv 2>/dev/null)"
    emsgln in_container_uninstall "${NAME:+ ($NAME)}"
    echo >&2
    if command -v distrobox-host-exec >/dev/null; then
        emsgln container_run_with "$SRC/packaging/$(basename "$0")" "$*"
    else
        emsgln container_run_host
        emsgln container_run_host_cmd "$SRC/packaging/$(basename "$0")" "$*"
    fi
    exit 1
fi

# ---------------------------------------------------------------- faza root ---
if [ "${1:-}" = --root-phase ]; then
    KEEP_CONFIG="$2"; LEGACY="$3"; TARGET_USER="$4"; LANG_CODE="$5"
    # shellcheck source=/dev/null
    . "$SRC/packaging/i18n.sh"
    say() { printf '    %s\n' "$1"; }

    if [ -x /usr/local/bin/omen-kbd ] \
       && /usr/local/bin/omen-kbd control bios >/dev/null 2>&1; then
        say "$(msg control_back_to_firmware)"
    else
        say "$(msg control_back_later)"
    fi

    systemctl disable --now omen-kbd.service >/dev/null 2>&1
    rm -f /etc/systemd/system/omen-kbd.service
    systemctl daemon-reload
    systemctl reset-failed omen-kbd.service >/dev/null 2>&1
    rm -rf /run/omen-kbd
    say "$(msg service_removed)"

    removed=0
    for f in /etc/udev/rules.d/99-hp-lamparray.rules \
             /etc/udev/rules.d/99-hp-lamparray-input.rules; do
        [ -f "$f" ] && { rm -f "$f"; removed=1; }
    done
    if [ "$removed" = 1 ]; then
        udevadm control --reload-rules
        udevadm trigger --action=add --subsystem-match=hidraw
        udevadm trigger --action=add --subsystem-match=input
        say "$(msg udev_rules_removed)"
    else
        say "$(msg udev_rules_absent)"
    fi

    rm -f /usr/lib/systemd/system-sleep/omen-kbd
    say "$(msg wake_hook_removed)"

    rm -rf /usr/local/lib/omen-kbd /usr/local/share/doc/omen-kbd
    rm -f /usr/local/bin/omen-kbd /usr/local/bin/omen-kbd-daemon \
          /usr/local/bin/omen-kbd-gui
    rm -f /usr/local/share/applications/omen-kbd.desktop \
          /usr/local/share/icons/hicolor/scalable/apps/omen-kbd.svg
    command -v update-desktop-database >/dev/null && \
        update-desktop-database /usr/local/share/applications 2>/dev/null
    say "$(msg program_removed)"

    if [ "$KEEP_CONFIG" = 1 ]; then
        say "$(msg config_kept)"
    else
        rm -rf /var/lib/omen-kbd /var/cache/omen-kbd
        say "$(msg config_removed)"
    fi

    # Konto i grupy na koniec — po usunieciu regul nie daja dostepu do niczego.
    getent passwd omenkbd >/dev/null && userdel omenkbd 2>/dev/null \
        && say "$(msg sysuser_removed)"
    for g in omenkbd-input omenkbd; do
        getent group "$g" >/dev/null || continue
        gpasswd -d "$TARGET_USER" "$g" >/dev/null 2>&1
        groupdel "$g" 2>/dev/null && say "$(msg group_removed "$g")" \
            || say "$(msg group_still_used "$g")"
    done

    if [ "$LEGACY" = 1 ]; then
        rm -f /etc/omen-kbd.conf
        systemctl disable --now omen-kbd.service >/dev/null 2>&1
        say "$(msg legacy_removed)"
    fi
    exit 0
fi
# ------------------------------------------------------------ faza uzytkownika -

[ "$(id -u)" = 0 ] && { emsgln no_sudo_prefix; exit 1; }
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"

msgln step_tray_user_units
for unit in omen-kbd-tray.service omen-kbd.service; do
    systemctl --user disable --now "$unit" 2>/dev/null
    rm -f "$HOME/.config/systemd/user/$unit"
done
systemctl --user daemon-reload 2>/dev/null
loginctl disable-linger "$USER" 2>/dev/null
rm -f "$XDG_RUNTIME_DIR/omen-kbd.sock"
rm -f "$HOME/.local/share/applications/omen-kbd.desktop" \
      "$HOME/.local/share/icons/hicolor/scalable/apps/omen-kbd.svg"
rm -rf "$HOME/.local/lib/omen-kbd" "$HOME/.local/share/doc/omen-kbd"
rm -f "$HOME/.local/bin/omen-kbd" "$HOME/.local/bin/omen-kbd-daemon" \
      "$HOME/.local/bin/omen-kbd-gui"
msgln tray_removed

msgln step_system_part
sudo bash "$SRC/packaging/uninstall.sh" --root-phase \
    "$KEEP_CONFIG" "$LEGACY" "$USER" "$LANG_CODE"

if [ "$LEGACY" = 1 ] && [ "$KEEP_CONFIG" = 0 ]; then
    rm -rf "$HOME/.config/omen-kbd" "$HOME/.cache/omen-kbd"
    msgln step_old_user_config_removed
fi

echo
msgln uninstall_done_1
msgln uninstall_done_2
echo
msgln uninstall_keeps_pyside6
msgln uninstall_pyside6_cmd
