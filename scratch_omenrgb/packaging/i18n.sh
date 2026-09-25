# packaging/i18n.sh — bilingual (pl/en) output for install.sh and uninstall.sh.
#
# Mirrors the precedence used by omenkbd/i18n.py: explicit OMEN_KBD_LANG,
# the SAME saved preference file the app itself reads, then the system
# locale; English by default unless the locale explicitly says Polish.
#
# Each message is ONE full sentence per key (not label+status glued
# together), so Polish grammatical gender/case agreement stays correct
# instead of being reconstructed from generic fragments.
#
# The root phase cannot reliably detect language on its own: sudo resets the
# environment and $HOME by default, so it would read root's (nonexistent)
# preference file instead of the real user's. The user phase therefore
# resolves LANG_CODE once and passes it explicitly as an argument to the
# --root-phase invocation; this file only auto-detects when LANG_CODE isn't
# already set.

detect_lang() {
    case "${OMEN_KBD_LANG:-}" in
        pl|en) echo "$OMEN_KBD_LANG"; return ;;
    esac
    local pref="${XDG_CONFIG_HOME:-$HOME/.config}/omen-kbd/language"
    if [ -f "$pref" ]; then
        case "$(cat "$pref" 2>/dev/null)" in
            pl|en) cat "$pref"; return ;;
        esac
    fi
    case "${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}" in
        pl*) echo pl ;;
        *) echo en ;;
    esac
}

: "${LANG_CODE:=$(detect_lang)}"

declare -A MSG_PL
declare -A MSG_EN

# msg KEY [ARGS...] — resolves and printf-formats a message, no trailing
# newline (mirrors plain printf). msgln adds the newline; emsgln sends to
# stderr. Unknown key prints the key itself, so a missing translation is
# visible instead of crashing the installer.
msg() {
    local key="$1"; shift
    local template
    if [ "$LANG_CODE" = en ]; then
        template="${MSG_EN[$key]-${MSG_PL[$key]-$key}}"
    else
        template="${MSG_PL[$key]-$key}"
    fi
    if [ "$#" -gt 0 ]; then
        # shellcheck disable=SC2059
        printf -- "$template" "$@"
    else
        printf -- '%s' "$template"
    fi
}

msgln() { msg "$@"; printf '\n'; }
emsgln() { msgln "$@" >&2; }

# ---------------------------------------------------------------------------
# install.sh — user phase
# ---------------------------------------------------------------------------

MSG_PL[install.help]='Uzycie: bash packaging/install.sh [--no-gui] [--with-reactive]

  --no-gui          tylko demon i CLI, bez GUI i bez PySide6
  --with-reactive   dodatkowo pozwala demonowi czytać NACIŚNIĘCIA KLAWISZY tej
                    klawiatury — potrzebne do efektu reagującego na pisanie.
                    Dostęp dostaje WYŁĄCZNIE użytkownik systemowy '"'"'omenkbd'"'"',
                    pod którym chodzi demon; twoje aplikacje nie mają jak
                    dosięgnąć klawiszy. Bez tej opcji podświetlenie działa
                    w pełni, tylko bez reakcji na pisanie.

Demon jest usługą systemową działającą jako osobny, nieuprzywilejowany
użytkownik. Stan i profile: /var/lib/omen-kbd. Gniazdo: /run/omen-kbd.'
MSG_EN[install.help]='Usage: bash packaging/install.sh [--no-gui] [--with-reactive]

  --no-gui          daemon and CLI only, no GUI and no PySide6
  --with-reactive   additionally lets the daemon read this keyboard'"'"'s
                    KEYSTROKES — needed for the effect that reacts to typing.
                    Access is granted ONLY to the '"'"'omenkbd'"'"' system user the
                    daemon runs as; your applications have no way to reach
                    the keys. Without this option the backlight works fully,
                    just without reacting to typing.

The daemon is a system service running as a separate, unprivileged user.
State and profiles: /var/lib/omen-kbd. Socket: /run/omen-kbd.'

MSG_PL[unknown_option]='nieznana opcja: %s (--help)'
MSG_EN[unknown_option]='unknown option: %s (--help)'

MSG_PL[in_container]='To jest kontener%s, a instalować trzeba na hoście.'
MSG_EN[in_container]='This is a container%s, and you need to install on the host.'
MSG_PL[in_container_uninstall]='To jest kontener%s, a odinstalować trzeba na hoście.'
MSG_EN[in_container_uninstall]='This is a container%s, and you need to uninstall on the host.'
MSG_PL[container_run_with]='  distrobox-host-exec bash %s %s'
MSG_EN[container_run_with]='  distrobox-host-exec bash %s %s'
MSG_PL[container_run_host]='  Otwórz terminal na hoście i uruchom tam:'
MSG_EN[container_run_host]='  Open a terminal on the host and run there:'
MSG_PL[container_run_host_cmd]='    bash %s %s'
MSG_EN[container_run_host_cmd]='    bash %s %s'

MSG_PL[no_sudo_prefix]='Nie uruchamiaj przez sudo — skrypt sam o nie poprosi.'
MSG_EN[no_sudo_prefix]='Do not run this with sudo — the script asks for it itself.'
MSG_PL[omenkbd_dir_missing]='Nie znalazłem katalogu omenkbd w %s'
MSG_EN[omenkbd_dir_missing]='Could not find the omenkbd directory in %s'

MSG_PL[step_checking_hw]='==> sprawdzam sprzęt'
MSG_EN[step_checking_hw]='==> checking hardware'
MSG_PL[hw_found]='    %s  —  interfejs LampArray znaleziony'
MSG_EN[hw_found]='    %s  —  LampArray interface found'
MSG_PL[hw_not_found]='    UWAGA: nie widzę urządzenia LampArray. Instaluję mimo to.'
MSG_EN[hw_not_found]='    NOTE: I do not see a LampArray device. Installing anyway.'

MSG_PL[step_stopping]='==> zatrzymuję to, co już działa'
MSG_EN[step_stopping]='==> stopping anything already running'
MSG_PL[stopped_processes]='    zatrzymane procesy: %s'
MSG_EN[stopped_processes]='    stopped processes: %s'
MSG_PL[nothing_was_running]='    nic nie działało'
MSG_EN[nothing_was_running]='    nothing was running'

MSG_PL[step_system_part]='==> część systemowa — poproszę o hasło raz'
MSG_EN[step_system_part]='==> system part — I will ask for your password once'
MSG_PL[reactive_warning_1]='    --with-reactive: demon dostanie dostęp do naciśnięć klawiszy.'
MSG_EN[reactive_warning_1]='    --with-reactive: the daemon will get access to your keystrokes.'
MSG_PL[reactive_warning_2]='    Dostęp dostaje tylko użytkownik systemowy omenkbd, nie twoje konto.'
MSG_EN[reactive_warning_2]='    Only the omenkbd system user gets access, not your own account.'

MSG_PL[step_tray]='==> tray (jednostka użytkownika, bo GUI jest per-użytkownik)'
MSG_EN[step_tray]='==> tray (a user unit, since the GUI is per-user)'
MSG_PL[tray_started]='    omen-kbd-tray.service   włączona i uruchomiona'
MSG_EN[tray_started]='    omen-kbd-tray.service   enabled and started'
MSG_PL[tray_enabled_pending]='    omen-kbd-tray.service   włączona (wystartuje po zalogowaniu)'
MSG_EN[tray_enabled_pending]='    omen-kbd-tray.service   enabled (will start after you log in)'

MSG_PL[step_legacy_cleanup]='==> sprzątanie po starym prototypie'
MSG_EN[step_legacy_cleanup]='==> checking for the old prototype'
MSG_PL[legacy_found]='    ślady poprzednich wersji (nie ruszam):'
MSG_EN[legacy_found]='    traces of previous versions (leaving them alone):'
MSG_PL[legacy_remove_hint]='    usunięcie:  bash packaging/uninstall.sh --legacy'
MSG_EN[legacy_remove_hint]='    to remove:  bash packaging/uninstall.sh --legacy'
MSG_PL[legacy_clean]='    czysto'
MSG_EN[legacy_clean]='    clean'

MSG_PL[step_verify_account]='==> weryfikacja z Twojego konta'
MSG_EN[step_verify_account]='==> verifying from your own account'
MSG_PL[daemon_running_no_group_1]='    Demon działa (sprawdzony w części systemowej), ale TA SESJA nie ma'
MSG_EN[daemon_running_no_group_1]='    The daemon is running (verified in the system part), but THIS SESSION'
MSG_PL[daemon_running_no_group_2]='    jeszcze grupy omenkbd, więc nie dosięga gniazda. Podświetlenie'
MSG_EN[daemon_running_no_group_2]='    does not have the omenkbd group yet, so it cannot reach the socket.'
MSG_PL[daemon_running_no_group_3]='    działa niezależnie od Twojej sesji.'
MSG_EN[daemon_running_no_group_3]='    The backlight works independently of your session.'
MSG_PL[try_now_no_logout]='    od razu, bez wylogowania:   sg omenkbd -c '"'"'omen-kbd status'"'"''
MSG_EN[try_now_no_logout]='    right away, without logging out:   sg omenkbd -c '"'"'omen-kbd status'"'"''
MSG_PL[try_permanently]='    na stałe:                   wyloguj się i zaloguj ponownie'
MSG_EN[try_permanently]='    permanently:                       log out and back in'
MSG_PL[daemon_not_responding_with_group]='    Demon nie odpowiada, mimo że masz grupę omenkbd.'
MSG_EN[daemon_not_responding_with_group]='    The daemon is not responding, even though you have the omenkbd group.'
MSG_PL[check_selinux]='    Sprawdź:  sudo ausearch -m avc -ts recent   (SELinux)'
MSG_EN[check_selinux]='    Check:  sudo ausearch -m avc -ts recent   (SELinux)'

MSG_PL[done]='GOTOWE.'
MSG_EN[done]='DONE.'
MSG_PL[tray_visible]='  Ikona w trayu widoczna, wpis w menu: „OMEN Keyboard”.'
MSG_EN[tray_visible]='  Tray icon visible, menu entry: "OMEN Keyboard".'
MSG_PL[try_these]='  omen-kbd effects · omen-kbd effect fire · omen-kbd --help'
MSG_EN[try_these]='  omen-kbd effects · omen-kbd effect fire · omen-kbd --help'

MSG_PL[group_after_relogin_1]='UWAGA: grupa omenkbd zaczyna działać po ponownym zalogowaniu. Do tego'
MSG_EN[group_after_relogin_1]='NOTE: the omenkbd group takes effect after you log back in. Until then,'
MSG_PL[group_after_relogin_2]='czasu ani omen-kbd, ani ikona w trayu nie dosięgną gniazda demona.'
MSG_EN[group_after_relogin_2]='neither omen-kbd nor the tray icon can reach the daemon'"'"'s socket.'
MSG_PL[group_after_relogin_3]='Samo podświetlenie działa — demon jest usługą systemową i nie zależy'
MSG_EN[group_after_relogin_3]='The backlight itself works — the daemon is a system service and does not'
MSG_PL[group_after_relogin_4]='od Twojej sesji. Żeby spróbować teraz:  sg omenkbd -c '"'"'omen-kbd status'"'"''
MSG_EN[group_after_relogin_4]='depend on your session. To try it now:  sg omenkbd -c '"'"'omen-kbd status'"'"''

MSG_PL[reactive_on_summary]='Dostęp do naciśnięć klawiszy: WŁĄCZONY, tylko dla użytkownika omenkbd.'
MSG_EN[reactive_on_summary]='Access to keystrokes: ENABLED, only for the omenkbd user.'
MSG_PL[reactive_check_who]='Sprawdzenie, kto ma dostęp:  getent group omenkbd-input'
MSG_EN[reactive_check_who]='Checking who has access:  getent group omenkbd-input'
MSG_PL[reactive_revoke_hint]='Odebranie bez odinstalowania całości:'
MSG_EN[reactive_revoke_hint]='Revoking it without uninstalling everything:'

MSG_PL[uninstall_hint]='Odinstalowanie:  bash packaging/uninstall.sh'
MSG_EN[uninstall_hint]='Uninstalling:  bash packaging/uninstall.sh'

# --- root phase ---

MSG_PL[group_exists]='grupa %s już istnieje'
MSG_EN[group_exists]='group %s already exists'
MSG_PL[group_created]='grupa %s utworzona'
MSG_EN[group_created]='group %s created'
MSG_PL[sysuser_exists]='użytkownik systemowy %s już istnieje'
MSG_EN[sysuser_exists]='system user %s already exists'
MSG_PL[sysuser_created]='użytkownik systemowy %s utworzony'
MSG_EN[sysuser_created]='system user %s created'
MSG_PL[user_in_group_already]='%s jest już w grupie %s'
MSG_EN[user_in_group_already]='%s is already in group %s'
MSG_PL[user_in_group_added]='%s dopisany do grupy %s (zadziała po przelogowaniu)'
MSG_EN[user_in_group_added]='%s added to group %s (takes effect after you log back in)'
MSG_PL[input_group_daemon_only]='grupa %s: tylko demon (nie %s)'
MSG_EN[input_group_daemon_only]='group %s: daemon only (not %s)'
MSG_PL[input_group_warning_1]='    UWAGA: %s należy do %s — to znosi ochronę.'
MSG_EN[input_group_warning_1]='    WARNING: %s is a member of %s — this defeats the isolation.'
MSG_PL[input_group_warning_2]='    Usuń:  sudo gpasswd -d %s %s'
MSG_EN[input_group_warning_2]='    Remove with:  sudo gpasswd -d %s %s'

MSG_PL[code_installed]='%s: kod'
MSG_EN[code_installed]='%s: code'
MSG_PL[commands_installed]='%s: polecenia'
MSG_EN[commands_installed]='%s: commands'

MSG_PL[udev_leds_installed]='reguła udev (diody): zapisana (grupa %s)'
MSG_EN[udev_leds_installed]='udev rule (LEDs): written (group %s)'
MSG_PL[udev_keys_installed]='reguła udev (klawisze): zapisana (grupa %s)'
MSG_EN[udev_keys_installed]='udev rule (keys): written (group %s)'
MSG_PL[udev_keys_not_installed]='reguła udev (klawisze): NIE instalowana'
MSG_EN[udev_keys_not_installed]='udev rule (keys): NOT installed'

MSG_PL[verify_missing_node]='%s: nie ma węzła %s — pomijam'
MSG_EN[verify_missing_node]='%s: no device node %s — skipping'
MSG_PL[verify_leds]='weryfikacja dostępu do diod'
MSG_EN[verify_leds]='verifying access to the LEDs'
MSG_PL[verify_keys]='weryfikacja dostępu do klawiszy'
MSG_EN[verify_keys]='verifying access to the keys'
MSG_PL[verify_open_ok]='%s: %s otwarte jako %s, OK'
MSG_EN[verify_open_ok]='%s: %s opened as %s, OK'
MSG_PL[verify_open_fail_1]='    BŁĄD: użytkownik %s NIE MOŻE otworzyć %s (%s).'
MSG_EN[verify_open_fail_1]='    ERROR: user %s CANNOT open %s (%s).'
MSG_PL[verify_open_fail_group_mode]='    Grupa węzła: %s   tryb: %s'
MSG_EN[verify_open_fail_group_mode]='    Node group: %s   mode: %s'
MSG_PL[verify_acl_cause_1]='    PRZYCZYNA: na węźle wisi ACL z pustym wpisem group:: —'
MSG_EN[verify_acl_cause_1]='    CAUSE: the node has an ACL with an empty group:: entry —'
MSG_PL[verify_acl_cause_2]='    grupa i tryb są wtedy bez znaczenia. Pełny obraz:'
MSG_EN[verify_acl_cause_2]='    the group and mode are then irrelevant. Full picture:'
MSG_PL[verify_check_udev]='    Sprawdź:  udevadm info -q property -n %s | grep ID_USB'
MSG_EN[verify_check_udev]='    Check:  udevadm info -q property -n %s | grep ID_USB'
MSG_PL[verify_check_selinux]='              sudo ausearch -m avc -ts recent   (SELinux)'
MSG_EN[verify_check_selinux]='              sudo ausearch -m avc -ts recent   (SELinux)'
MSG_PL[verify_no_evdev_found]='weryfikacja dostępu do klawiszy: nie znalazłem urządzeń — pomijam'
MSG_EN[verify_no_evdev_found]='verifying access to the keys: no devices found — skipping'

MSG_PL[wake_hook_installed]='hook wybudzenia: zapisany'
MSG_EN[wake_hook_installed]='wake hook: written'

MSG_PL[menu_icon_installed]='wpis w menu i ikona: zapisane'
MSG_EN[menu_icon_installed]='menu entry and icon: written'
MSG_PL[pyside6_already]='python3-pyside6: już jest'
MSG_EN[pyside6_already]='python3-pyside6: already installed'
MSG_PL[pyside6_installed]='python3-pyside6: zainstalowany'
MSG_EN[pyside6_installed]='python3-pyside6: installed'
MSG_PL[pyside6_failed]='python3-pyside6: NIE UDAŁO SIĘ — GUI nie ruszy'
MSG_EN[pyside6_failed]='python3-pyside6: FAILED — the GUI will not start'
MSG_PL[pyside6_no_dnf]='python3-pyside6: brak dnf, zainstaluj ręcznie'
MSG_EN[pyside6_no_dnf]='python3-pyside6: no dnf, install it manually'

MSG_PL[old_user_unit_removed]='stara jednostka użytkownika: usunięta (demon jest teraz systemowy)'
MSG_EN[old_user_unit_removed]='old user unit: removed (the daemon is now a system service)'
MSG_PL[service_started]='omen-kbd.service: włączona i uruchomiona'
MSG_EN[service_started]='omen-kbd.service: enabled and started'

MSG_PL[daemon_ok]='demon odpowiada: OK'
MSG_EN[daemon_ok]='daemon responding: OK'
MSG_PL[daemon_fail_root_1]='    BŁĄD: demon nie odpowiada (sprawdzone jako root).'
MSG_EN[daemon_fail_root_1]='    ERROR: the daemon is not responding (checked as root).'
MSG_PL[daemon_fail_root_2]='    Ostatnie wpisy dziennika:'
MSG_EN[daemon_fail_root_2]='    Latest journal entries:'

# ---------------------------------------------------------------------------
# uninstall.sh
# ---------------------------------------------------------------------------

MSG_PL[uninstall.help]='Użycie: bash packaging/uninstall.sh [--keep-config] [--legacy]

  --keep-config   zostaw stan i profile w /var/lib/omen-kbd
  --legacy        usuń też ślady starego prototypu (/etc/omen-kbd.conf,
                  jednostki użytkownika, ~/.config/omen-kbd)

Zawsze: oddaje klawiaturę firmware'"'"'owi, wyłącza i usuwa usługę, obie reguły
udev, hook wybudzenia, użytkownika i grupy systemowe oraz pliki programu.
Po tym /dev/hidraw* i /dev/input/event* wracają do stanu "tylko root".'
MSG_EN[uninstall.help]='Usage: bash packaging/uninstall.sh [--keep-config] [--legacy]

  --keep-config   leave state and profiles in /var/lib/omen-kbd
  --legacy        also remove traces of the old prototype (/etc/omen-kbd.conf,
                  the user unit, ~/.config/omen-kbd)

Always: hands the keyboard back to the firmware, disables and removes the
service, both udev rules, the wake hook, the system user and groups, and
the program files. Afterwards /dev/hidraw* and /dev/input/event* go back
to "root only".'

MSG_PL[step_tray_user_units]='==> tray i jednostki użytkownika'
MSG_EN[step_tray_user_units]='==> tray and user units'
MSG_PL[tray_removed]='    wyłączone i usunięte (także pozostałości z ~/.local)'
MSG_EN[tray_removed]='    disabled and removed (also leftovers from ~/.local)'

MSG_PL[uninstall_done_1]='GOTOWE. Uprawnienia są odebrane w całości: /dev/hidraw* i /dev/input/event*'
MSG_EN[uninstall_done_1]='DONE. Permissions have been fully revoked: /dev/hidraw* and /dev/input/event*'
MSG_PL[uninstall_done_2]='wracają do stanu '"'"'tylko root'"'"'.'
MSG_EN[uninstall_done_2]='are back to '"'"'root only'"'"'.'
MSG_PL[uninstall_keeps_pyside6]='Nie cofam jednej rzeczy, bo mogłeś jej chcieć niezależnie od tej apki:'
MSG_EN[uninstall_keeps_pyside6]='There is one thing I leave alone, since you may have wanted it independently of this app:'
MSG_PL[uninstall_pyside6_cmd]='  python3-pyside6    sudo dnf remove python3-pyside6'
MSG_EN[uninstall_pyside6_cmd]='  python3-pyside6    sudo dnf remove python3-pyside6'

# --- uninstall root phase ---

MSG_PL[control_back_to_firmware]='kontrola oddana firmware: wraca fabryczne pulsowanie'
MSG_EN[control_back_to_firmware]='control handed to firmware: factory pulsing is back'
MSG_PL[control_back_later]='kontrola oddana firmware: nie teraz; wróci po restarcie'
MSG_EN[control_back_later]='control handed to firmware: not right now; will apply after a restart'
MSG_PL[service_removed]='usługa omen-kbd: wyłączona i usunięta'
MSG_EN[service_removed]='omen-kbd service: disabled and removed'
MSG_PL[udev_rules_removed]='reguły udev (diody i klawisze): usunięte, dostęp odebrany'
MSG_EN[udev_rules_removed]='udev rules (LEDs and keys): removed, access revoked'
MSG_PL[udev_rules_absent]='reguły udev: nie było'
MSG_EN[udev_rules_absent]='udev rules: none were present'
MSG_PL[wake_hook_removed]='hook wybudzenia: usunięty'
MSG_EN[wake_hook_removed]='wake hook: removed'
MSG_PL[program_removed]='program, wpis w menu, ikona: usunięte'
MSG_EN[program_removed]='program, menu entry, icon: removed'
MSG_PL[config_kept]='stan i profile: zostawione (--keep-config)'
MSG_EN[config_kept]='state and profiles: left in place (--keep-config)'
MSG_PL[config_removed]='stan, profile i cache mapy: usunięte'
MSG_EN[config_removed]='state, profiles and lamp map cache: removed'
MSG_PL[sysuser_removed]='użytkownik systemowy omenkbd: usunięty'
MSG_EN[sysuser_removed]='system user omenkbd: removed'
MSG_PL[group_removed]='grupa %s: usunięta'
MSG_EN[group_removed]='group %s: removed'
MSG_PL[group_still_used]='grupa %s: została (ktoś jej jeszcze używa)'
MSG_EN[group_still_used]='group %s: still present (something else is still using it)'
MSG_PL[legacy_removed]='ślady starego prototypu: usunięte'
MSG_EN[legacy_removed]='traces of the old prototype: removed'
MSG_PL[step_old_user_config_removed]='==> stara konfiguracja użytkownika usunięta'
MSG_EN[step_old_user_config_removed]='==> old user configuration removed'
