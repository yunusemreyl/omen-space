#!/usr/bin/env bash
# install.sh — instaluje omen-kbd: demon systemowy, CLI, GUI z trayem,
# wpis w menu, hook wybudzenia.
#
#   bash packaging/install.sh                     (BEZ sudo z przodu)
#   bash packaging/install.sh --with-reactive     + dostep do nacisniec klawiszy
#   bash packaging/install.sh --no-gui            bez GUI i bez PySide6
#   bash packaging/install.sh --lang en           wymus jezyk komunikatow
#
# MODEL BEZPIECZENSTWA
# Demon dziala jako osobny uzytkownik systemowy 'omenkbd'. To celowe: przy
# efekcie reagujacym na pisanie strumien nacisniec klawiszy jest czytelny tylko
# dla tego uid. Twoje aplikacje rozmawiaja z demonem po gniezdzie — moga zmieniac
# kolory, nie majac wgladu w klawisze. Nie uzywamy tu TAG+="uaccess", bo ten
# nadaje ACL UZYTKOWNIKOWI, czyli kazdemu procesowi dzialajacemu jako ty, co
# przekreslaloby caly sens separacji.
set -euo pipefail

SRC="$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)"
LIBDIR=/usr/local/lib/omen-kbd
BINDIR=/usr/local/bin
SHAREDIR=/usr/local/share
DOCDIR="$SHAREDIR/doc/omen-kbd"
PY=/usr/bin/python3
SVC_USER=omenkbd
SVC_GROUP=omenkbd
INPUT_GROUP=omenkbd-input

# ---------------------------------------------------------------- faza root ---
# Wywolywana przez sam skrypt pod sudo. Cala powierzchnia dotykajaca roota jest
# w tym jednym bloku — mozna go przeczytac przed wpisaniem hasla.
if [ "${1:-}" = --root-phase ]; then
    PKG="$2"; TARGET_USER="$3"; WANT_GUI="$4"; WANT_REACTIVE="$5"; LANG_CODE="$6"
    # shellcheck source=/dev/null
    . "$PKG/i18n.sh"

    say() { printf '    %s\n' "$1"; }

    # --- konta i grupy ---
    if getent group "$SVC_GROUP" >/dev/null; then
        say "$(msg group_exists "$SVC_GROUP")"
    else
        groupadd --system "$SVC_GROUP"; say "$(msg group_created "$SVC_GROUP")"
    fi
    if getent passwd "$SVC_USER" >/dev/null; then
        say "$(msg sysuser_exists "$SVC_USER")"
    else
        useradd --system --gid "$SVC_GROUP" --no-create-home \
                --home-dir /nonexistent --shell /sbin/nologin \
                --comment "omen-kbd keyboard backlight daemon" "$SVC_USER"
        say "$(msg sysuser_created "$SVC_USER")"
    fi
    # Ty w grupie demona — to daje dostep do GNIAZDA (sterowanie kolorami).
    if id -nG "$TARGET_USER" | tr ' ' '\n' | grep -qx "$SVC_GROUP"; then
        say "$(msg user_in_group_already "$TARGET_USER" "$SVC_GROUP")"
    else
        usermod -aG "$SVC_GROUP" "$TARGET_USER"
        say "$(msg user_in_group_added "$TARGET_USER" "$SVC_GROUP")"
    fi

    if [ "$WANT_REACTIVE" = 1 ]; then
        getent group "$INPUT_GROUP" >/dev/null || groupadd --system "$INPUT_GROUP"
        # Do grupy klawiszowej nalezy WYLACZNIE demon. Ciebie tam nie ma i to
        # jest istota zabezpieczenia.
        usermod -aG "$INPUT_GROUP" "$SVC_USER"
        say "$(msg input_group_daemon_only "$INPUT_GROUP" "$TARGET_USER")"
        if id -nG "$TARGET_USER" | tr ' ' '\n' | grep -qx "$INPUT_GROUP"; then
            emsgln input_group_warning_1 "$TARGET_USER" "$INPUT_GROUP"
            emsgln input_group_warning_2 "$TARGET_USER" "$INPUT_GROUP"
        fi
    fi

    # --- kod i polecenia ---
    rm -rf "$LIBDIR"; install -d "$LIBDIR" "$BINDIR" "$DOCDIR"
    cp -r "$PKG/../omenkbd" "$LIBDIR/"
    find "$LIBDIR" -name __pycache__ -type d -prune -exec rm -rf {} + 2>/dev/null || true
    chmod -R a+rX "$LIBDIR"
    say "$(msg code_installed "$LIBDIR")"

    wrapper() {
        cat > "$BINDIR/$1" <<WRAP
#!/bin/sh
exec $PY -c 'import sys; sys.path.insert(0, "$LIBDIR"); from ${2%%:*} import ${2##*:}; sys.exit(${2##*:}())' "\$@"
WRAP
        chmod 0755 "$BINDIR/$1"
    }
    wrapper omen-kbd        omenkbd.cli:main
    wrapper omen-kbd-daemon omenkbd.engine.daemon:main
    if [ "$WANT_GUI" = 1 ]; then wrapper omen-kbd-gui omenkbd.gui.app:main; fi
    say "$(msg commands_installed "$BINDIR/omen-kbd*")"

    for f in README.md BRIEF-omen-rgb-app.md omen-max-8d41-keyboard-rgb.md; do
        [ -f "$PKG/../$f" ] && install -m 0644 "$PKG/../$f" "$DOCDIR/"
    done

    # --- reguly udev ---
    install -m 0644 "$PKG/99-hp-lamparray.rules" \
        /etc/udev/rules.d/99-hp-lamparray.rules
    say "$(msg udev_leds_installed "$SVC_GROUP")"
    if [ "$WANT_REACTIVE" = 1 ]; then
        install -m 0644 "$PKG/99-hp-lamparray-input.rules" \
            /etc/udev/rules.d/99-hp-lamparray-input.rules
        say "$(msg udev_keys_installed "$INPUT_GROUP")"
    else
        rm -f /etc/udev/rules.d/99-hp-lamparray-input.rules
        say "$(msg udev_keys_not_installed)"
    fi
    udevadm control --reload-rules
    # samo "trigger" bez --action=add nie odswieza uprawnien
    udevadm trigger --action=add --subsystem-match=hidraw
    udevadm trigger --action=add --subsystem-match=input
    udevadm settle --timeout=5 2>/dev/null || true

    # WERYFIKACJA, ze regula naprawde zadzialala. Regula udev moze byc poprawna
    # skladniowo i nigdy nie trafiac (np. gdy klucze ATTRS leza na roznych
    # urzadzeniach nadrzednych — udev wymaga jednego). Bez tego sprawdzenia
    # objawem jest cicho ciemna klawiatura i demon petlacy sie na EACCES.
    # Weryfikacja przez FAKTYCZNE OTWARCIE urzadzenia jako uzytkownik demona.
    # Sprawdzanie samej nazwy grupy (stat -c %G) NIE WYSTARCZA: gdy na wezle wisi
    # POSIX ACL, "ls -l" pokazuje maske ACL w miejscu uprawnien grupy, wiec
    # grupa moze byc poprawna, a wpis group:: pusty — i proces dostaje EACCES.
    # Taki wlasnie blad przeszedl przez sprawdzenie oparte na nazwie grupy.
    check_open() {  # $1 = wezel, $2 = tryb (rw|r), $3 = klucz i18n opisu
        [ -e "$1" ] || { say "$(msg verify_missing_node "$(msg "$3")" "$1")"; return 0; }
        if runuser -u "$SVC_USER" -- "$PY" -c "
import os, sys
flag = os.O_RDWR if sys.argv[2] == 'rw' else os.O_RDONLY
os.close(os.open(sys.argv[1], flag | os.O_NONBLOCK))
" "$1" "$2" 2>/dev/null; then
            say "$(msg verify_open_ok "$(msg "$3")" "$1" "$SVC_USER")"
            return 0
        fi
        echo >&2
        emsgln verify_open_fail_1 "$SVC_USER" "$1" "$2"
        emsgln verify_open_fail_group_mode \
            "$(stat -c %G "$1" 2>/dev/null)" "$(stat -c %A "$1" 2>/dev/null)"
        if getfacl -p "$1" 2>/dev/null | grep -q "^group::---"; then
            emsgln verify_acl_cause_1
            emsgln verify_acl_cause_2
            getfacl -p "$1" 2>/dev/null | grep -v "^#" | sed "s/^/      /" >&2
        else
            emsgln verify_check_udev "$1"
            emsgln verify_check_selinux
        fi
        return 1
    }

    NODE="$("$PY" -c "
import sys; sys.path.insert(0, '$LIBDIR')
from omenkbd.core.device import discover
d = discover(); print(d[0]['node'] if d else '')" 2>/dev/null || true)"
    check_open "$NODE" rw verify_leds || exit 1

    if [ "$WANT_REACTIVE" = 1 ]; then
        EVDEVS="$(
            for e in /dev/input/event*; do
                p=$(udevadm info -q property -n "$e" 2>/dev/null) || continue
                echo "$p" | grep -qx 'ID_INPUT_KEYBOARD=1' || continue
                echo "$p" | grep -qx 'ID_USB_VENDOR_ID=0d62' || continue
                echo "$p" | grep -qx 'ID_USB_MODEL_ID=54bf' || continue
                echo "$e"
            done)"
        if [ -z "$EVDEVS" ]; then
            say "$(msg verify_no_evdev_found)"
        else
            for e in $EVDEVS; do
                check_open "$e" r verify_keys || exit 1
            done
        fi
    fi

    # --- hook wybudzenia ---
    install -d /usr/lib/systemd/system-sleep
    install -m 0755 "$PKG/omen-kbd-sleep" /usr/lib/systemd/system-sleep/omen-kbd
    say "$(msg wake_hook_installed)"

    # --- GUI: wpis w menu, ikona, zaleznosc ---
    if [ "$WANT_GUI" = 1 ]; then
        install -d "$SHAREDIR/applications" \
                  "$SHAREDIR/icons/hicolor/scalable/apps"
        install -m 0644 "$PKG/omen-kbd.desktop" \
            "$SHAREDIR/applications/omen-kbd.desktop"
        "$PY" -c "
import sys; sys.path.insert(0, '$LIBDIR')
from omenkbd.gui.icon import to_svg
open('$SHAREDIR/icons/hicolor/scalable/apps/omen-kbd.svg', 'w').write(to_svg())"
        command -v update-desktop-database >/dev/null && \
            update-desktop-database "$SHAREDIR/applications" 2>/dev/null || true
        command -v gtk-update-icon-cache >/dev/null && \
            gtk-update-icon-cache -qtf "$SHAREDIR/icons/hicolor" 2>/dev/null || true
        say "$(msg menu_icon_installed)"
        if "$PY" -c 'import PySide6' 2>/dev/null; then
            say "$(msg pyside6_already)"
        elif command -v dnf >/dev/null; then
            dnf install -y python3-pyside6 >/dev/null 2>&1 \
                && say "$(msg pyside6_installed)" \
                || say "$(msg pyside6_failed)"
        else
            say "$(msg pyside6_no_dnf)"
        fi
    fi

    # --- usluga systemowa ---
    # Sprzatanie po poprzednim modelu (demon jako usluga uzytkownika).
    USER_UNIT="$(getent passwd "$TARGET_USER" | cut -d: -f6)/.config/systemd/user/omen-kbd.service"
    if [ -f "$USER_UNIT" ]; then
        rm -f "$USER_UNIT"
        say "$(msg old_user_unit_removed)"
    fi
    install -m 0644 "$PKG/omen-kbd.service" /etc/systemd/system/omen-kbd.service
    systemctl daemon-reload
    systemctl enable omen-kbd.service >/dev/null 2>&1
    systemctl restart omen-kbd.service
    say "$(msg service_started)"

    # Weryfikacja demona JAKO ROOT. Sesja wywolujacego moze jeszcze nie miec
    # grupy omenkbd (grupy przypisuja sie przy logowaniu), a to nie jest awaria
    # demona — poprzednia wersja instalatora meldowala z tego powodu falszywy
    # alarm i wypisywala dziennik, w ktorym nic zlego nie bylo.
    SOCK=/run/omen-kbd/omen-kbd.sock
    for _ in $(seq 60); do
        [ -S "$SOCK" ] && break
        sleep 0.25
    done
    if "$BINDIR/omen-kbd" status >/dev/null 2>&1; then
        say "$(msg daemon_ok)"
        "$BINDIR/omen-kbd" status | sed 's/^/      /'
    else
        echo >&2
        emsgln daemon_fail_root_1
        emsgln daemon_fail_root_2
        journalctl -u omen-kbd.service -n 15 --no-pager 2>/dev/null \
            | sed 's/^/      /' >&2
        echo >&2
        emsgln check_selinux
        exit 1
    fi
    exit 0
fi
# ------------------------------------------------------------ faza uzytkownika -

# Prescan dla --lang: musi byc znany PRZED zbudowaniem komunikatow (np. zeby
# --help tez byl w wybranym jezyku), a i18n.sh definiuje detect_lang() dopiero
# po zrodlowaniu — nie mozna go wiec wolac wczesniej.
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
# LANG_CODE="" wymusza autodetekcje w i18n.sh (":=" reaguje tez na pusty
# string), a LANG_CODE niepuste od --lang zostaje nietkniete.
LANG_CODE="$LANG_OVERRIDE"
# shellcheck source=/dev/null
. "$SRC/packaging/i18n.sh"

WANT_GUI=1
WANT_REACTIVE=0
i=0
while [ $i -lt ${#ARGS[@]} ]; do
    a="${ARGS[$i]}"
    case "$a" in
        --no-gui) WANT_GUI=0 ;;
        --with-reactive) WANT_REACTIVE=1 ;;
        --lang) i=$((i + 1)) ;;      # juz obsluzone w prescanie powyzej
        --lang=*) ;;                  # jw.
        -h|--help) msgln install.help; exit 0 ;;
        *) emsgln unknown_option "$a"; exit 1 ;;
    esac
    i=$((i + 1))
done

# Instalacja MUSI isc na hoscie, nie w kontenerze (distrobox/toolbox): dnf, udev,
# systemd i /usr/lib/systemd/system-sleep to byty hosta, a wrappery zaszywaja
# /usr/bin/python3, ktory w kontenerze jest innym Pythonem niz na hoscie.
if [ -f /run/.containerenv ] || [ -f /run/.toolboxenv ]; then
    NAME="$(sed -n 's/^name="\(.*\)"/\1/p' /run/.containerenv 2>/dev/null)"
    emsgln in_container "${NAME:+ ($NAME)}"
    echo >&2
    if command -v distrobox-host-exec >/dev/null; then
        emsgln container_run_with "$SRC/packaging/$(basename "$0")" "$*"
    else
        emsgln container_run_host
        emsgln container_run_host_cmd "$SRC/packaging/$(basename "$0")" "$*"
    fi
    exit 1
fi

[ "$(id -u)" = 0 ] && { emsgln no_sudo_prefix; exit 1; }
[ -d "$SRC/omenkbd" ] || { emsgln omenkbd_dir_missing "$SRC"; exit 1; }
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"

msgln step_checking_hw
DEVINFO="$("$PY" -c "
import sys; sys.path.insert(0, '$SRC')
from omenkbd.core.device import discover
d = discover()
print(f\"{d[0]['node']}  {d[0]['name']}\" if d else '')" 2>/dev/null || true)"
if [ -n "$DEVINFO" ]; then
    msgln hw_found "$DEVINFO"
else
    msgln hw_not_found
fi

msgln step_stopping
systemctl --user stop omen-kbd.service 2>/dev/null || true
systemctl --user disable omen-kbd.service 2>/dev/null || true
loginctl disable-linger "$USER" 2>/dev/null || true
STRAY="$(pgrep -u "$USER" -f 'omen.kbd.daemon|omenkbd[.]engine[.]daemon' || true)"
if [ -n "$STRAY" ]; then
    # shellcheck disable=SC2086
    kill -TERM $STRAY 2>/dev/null || true
    sleep 1
    msgln stopped_processes "$(echo $STRAY | tr '\n' ' ')"
else
    msgln nothing_was_running
fi

msgln step_system_part
if [ "$WANT_REACTIVE" = 1 ]; then
    msgln reactive_warning_1
    msgln reactive_warning_2
fi
sudo bash "$SRC/packaging/install.sh" --root-phase \
    "$SRC/packaging" "$USER" "$WANT_GUI" "$WANT_REACTIVE" "$LANG_CODE"

if [ "$WANT_GUI" = 1 ]; then
    msgln step_tray
    UNITDIR="$HOME/.config/systemd/user"
    install -d "$UNITDIR"
    install -m 0644 "$SRC/packaging/omen-kbd-tray.service" \
        "$UNITDIR/omen-kbd-tray.service"
    systemctl --user daemon-reload
    systemctl --user enable omen-kbd-tray.service >/dev/null 2>&1
    systemctl --user restart omen-kbd-tray.service 2>/dev/null \
        && msgln tray_started \
        || msgln tray_enabled_pending
fi

msgln step_legacy_cleanup
LEFT=""
for f in /etc/omen-kbd.conf /usr/local/bin/omen-kbd.orig "$HOME/.config/omen-kbd"; do
    [ -e "$f" ] && LEFT="$LEFT $f"
done
if [ -n "$LEFT" ]; then
    msgln legacy_found
    for f in $LEFT; do echo "      $f"; done
    msgln legacy_remove_hint
else
    msgln legacy_clean
fi

echo
msgln step_verify_account
HAS_GROUP=0
id -nG | tr ' ' '\n' | grep -qx omenkbd && HAS_GROUP=1

if "$BINDIR/omen-kbd" status >/dev/null 2>&1; then
    "$BINDIR/omen-kbd" status | sed 's/^/    /'
elif [ "$HAS_GROUP" = 0 ]; then
    # Demon zostal sprawdzony jako root w czesci systemowej i odpowiada.
    # Ta sesja po prostu nie ma jeszcze grupy — swiatlo dziala, brakuje tylko
    # dostepu do gniazda z tego terminala.
    msgln daemon_running_no_group_1
    msgln daemon_running_no_group_2
    msgln daemon_running_no_group_3
    echo
    msgln try_now_no_logout
    msgln try_permanently
else
    emsgln daemon_not_responding_with_group
    sudo journalctl -u omen-kbd.service -n 20 --no-pager 2>/dev/null \
        | sed 's/^/    /' >&2 || true
    emsgln check_selinux
    exit 1
fi

echo
msgln done
[ "$WANT_GUI" = 1 ] && msgln tray_visible
msgln try_these
echo
if [ "$HAS_GROUP" = 0 ]; then
    msgln group_after_relogin_1
    msgln group_after_relogin_2
    msgln group_after_relogin_3
    msgln group_after_relogin_4
    echo
fi
if [ "$WANT_REACTIVE" = 1 ]; then
    msgln reactive_on_summary
    msgln reactive_check_who
    msgln reactive_revoke_hint
    echo "  sudo rm /etc/udev/rules.d/99-hp-lamparray-input.rules"
    echo "  sudo udevadm control --reload-rules"
    echo "  sudo udevadm trigger --action=add --subsystem-match=input"
    echo
fi
msgln uninstall_hint
