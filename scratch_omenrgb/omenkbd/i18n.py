"""Dwujezyczne (pl/en) teksty interfejsu — jedno zrodlo prawdy dla CLI, GUI i traya.

Zasada projektowa: KLUCZE tlumaczen sa stabilne i jezykowo neutralne (angielskie
identyfikatory albo "effect.<name>" / "param.<effect>.<param>" / "choice.<...>"),
a WARTOSCI PARAMETROW zapisywane w stanie/profilach (np. 'axis', 'curve',
'bounce') sa rowniez jezykowo neutralne — nigdy nie uzywamy przetlumaczonego
tekstu jako klucza stanu. Dzieki temu zmiana jezyka nie psuje zapisanych
profili, a stary profil zapisany w jednym jezyku dziala identycznie w drugim.

Jezyk wybiera sie raz, per proces (CLI/GUI), z pierwszenstwem:
  1. jawny --lang / set_language()
  2. zmienna srodowiskowa OMEN_KBD_LANG
  3. zapisana preferencja w ~/.config/omen-kbd/language
  4. systemowe LC_ALL/LC_MESSAGES/LANG
  5. domyslnie angielski (chyba ze LANG/LC_* wprost mowi pl)

Demon NIE ma pojecia o jezyku — wszystkie jego komunikaty diagnostyczne
(bledy uprawnien, ACL, udev) zostaja po polsku/angielsku tak jak napisane w
kodzie demona; ten modul thumaczy WYLACZNIE statyczne etykiety interfejsu
(nazwy trybow, parametrow, przyciskow, menu), nie dynamiczne teksty z demona.
"""

import os

LANGUAGES = ('pl', 'en')
DEFAULT_LANGUAGE = 'en'

_PREF_PATH = os.path.join(
    os.environ.get('XDG_CONFIG_HOME') or os.path.expanduser('~/.config'),
    'omen-kbd', 'language')

_current = None   # ustalane leniwie, patrz get_language()


def set_language(lang, persist=False):
    global _current
    if lang not in LANGUAGES:
        raise ValueError(f'nieznany jezyk {lang!r}; dostepne: {LANGUAGES}')
    _current = lang
    if persist:
        try:
            os.makedirs(os.path.dirname(_PREF_PATH), exist_ok=True)
            with open(_PREF_PATH, 'w') as f:
                f.write(lang)
        except OSError:
            pass   # brak mozliwosci zapisu nie powinien wywalac programu


def get_language():
    global _current
    if _current is not None:
        return _current
    _current = _detect_language()
    return _current


def _detect_language():
    env = os.environ.get('OMEN_KBD_LANG')
    if env in LANGUAGES:
        return env
    try:
        with open(_PREF_PATH) as f:
            saved = f.read().strip()
        if saved in LANGUAGES:
            return saved
    except OSError:
        pass
    for var in ('LC_ALL', 'LC_MESSAGES', 'LANG'):
        v = os.environ.get(var, '').lower()
        if v.startswith('pl'):
            return 'pl'
        if v.startswith('en'):
            return 'en'
    return DEFAULT_LANGUAGE


def t(key, **kwargs):
    """Tlumaczy klucz UI. Nieznany klucz wraca jako jest (nie wywala programu —
    brakujace tlumaczenie ma byc widoczne, nie fatalne)."""
    entry = STRINGS.get(key)
    if entry is None:
        return key
    lang = get_language()
    text = entry.get(lang) or entry.get(DEFAULT_LANGUAGE) or key
    return text.format(**kwargs) if kwargs else text


def effect_label(name, fallback=None):
    entry = EFFECTS.get(name)
    if entry is None:
        return fallback or name
    lang = get_language()
    return entry['label'].get(lang) or entry['label'].get(DEFAULT_LANGUAGE) or (fallback or name)


def param_label(effect_name, param_name, fallback=None):
    entry = EFFECTS.get(effect_name)
    if entry is not None:
        p = entry.get('params', {}).get(param_name)
        if p is not None:
            lang = get_language()
            return p.get(lang) or p.get(DEFAULT_LANGUAGE) or (fallback or param_name)
    return fallback or param_name


def choice_label(effect_name, param_name, value, fallback=None):
    """Etykieta dla jednej opcji parametru typu 'choice'/'axis'. `value` jest
    zawsze jezykowo neutralnym kodem zapisywanym w stanie (np. 'x', 'loop',
    'soft'), nigdy tlumaczonym tekstem."""
    entry = EFFECTS.get(effect_name)
    if entry is not None:
        choices = entry.get('choices', {}).get(param_name)
        if choices is not None:
            c = choices.get(value)
            if c is not None:
                lang = get_language()
                return c.get(lang) or c.get(DEFAULT_LANGUAGE) or (fallback or value)
    return fallback or value


# ---------------------------------------------------------------------------
# Ogolne teksty interfejsu (okno, tray, CLI)
# ---------------------------------------------------------------------------

STRINGS = {
    # --- okno glowne ---
    'window.title': {'pl': 'OMEN Keyboard', 'en': 'OMEN Keyboard'},
    'window.mode': {'pl': 'Tryb', 'en': 'Mode'},
    'window.control': {'pl': 'Sterowanie', 'en': 'Control'},
    'window.brightness': {'pl': 'Jasność', 'en': 'Brightness'},
    'window.profile': {'pl': 'Profil', 'en': 'Profile'},
    'window.save_as': {'pl': 'Zapisz jako…', 'en': 'Save as…'},
    'window.delete': {'pl': 'Usuń', 'en': 'Delete'},
    'window.profile_none': {'pl': '(brak)', 'en': '(none)'},
    'window.save_profile_title': {'pl': 'Zapisz profil', 'en': 'Save profile'},
    'window.save_profile_prompt': {'pl': 'Nazwa profilu:', 'en': 'Profile name:'},
    'window.save_profile_failed_title': {'pl': 'Nie zapisano', 'en': 'Not saved'},
    'window.delete_profile_title': {'pl': 'Usunąć profil?', 'en': 'Delete profile?'},
    'window.delete_profile_confirm':
        {'pl': 'Usunąć profil „{name}"?', 'en': 'Delete profile "{name}"?'},
    'window.profile_saved': {'pl': 'zapisano profil „{name}"',
                             'en': 'saved profile "{name}"'},

    'window.reactive_box': {'pl': 'Reakcja na klawisze', 'en': 'React to keystrokes'},
    'window.reactive_no_access':
        {'pl': 'Brak dostępu do klawiszy tej klawiatury. Zainstaluj z: '
               'bash packaging/install.sh --with-reactive',
         'en': 'No access to this keyboard\'s keys. Install with: '
               'bash packaging/install.sh --with-reactive'},

    'window.control_bios': {'pl': 'BIOS', 'en': 'BIOS'},
    'window.control_app': {'pl': 'Aplikacja', 'en': 'App'},
    'window.control_bios_tip':
        {'pl': 'Podświetleniem steruje firmware klawiatury — wraca fabryczny\n'
               'efekt ustawiony w BIOS-ie (żółto-pomarańczowe pulsowanie).\n'
               'Aplikacja niczego nie wysyła.',
         'en': "The keyboard's own firmware controls the backlight — the "
               'factory\neffect from the BIOS comes back (yellow-orange '
               'pulsing).\nThe app sends nothing.'},
    'window.control_app_tip':
        {'pl': 'Podświetleniem steruje ta aplikacja — wybrany tryb, kolory\n'
               'i jasność. Firmware oddaje kontrolę hostowi.',
         'en': 'This app controls the backlight — the selected mode, colors\n'
               'and brightness. The firmware hands control to the host.'},
    'window.control_bios_status': {'pl': 'steruje firmware klawiatury (BIOS)',
                                   'en': "keyboard firmware is in control (BIOS)"},
    'window.control_app_status': {'pl': 'steruje aplikacja', 'en': 'app is in control'},

    'window.no_settings': {'pl': 'Ten tryb nie ma ustawień.',
                           'en': 'This mode has no settings.'},

    'window.paint_selected': {'pl': 'Pomaluj zaznaczone', 'en': 'Paint selection'},
    'window.paint_selected_n': {'pl': 'Pomaluj zaznaczone ({n})',
                                'en': 'Paint selection ({n})'},
    'window.clear_all': {'pl': 'Wyczyść wszystkie', 'en': 'Clear all'},
    'window.brush_color': {'pl': 'Kolor pędzla', 'en': 'Brush color'},
    'window.perkey_hint':
        {'pl': 'Klikaj klawisze na podglądzie. Ctrl = wiele, '
               'przeciąganie = malowanie zaznaczenia.',
         'en': 'Click keys on the preview. Ctrl = multiple, '
               'drag = paint the selection.'},

    'window.disconnected': {'pl': 'klawiatura niepodłączona',
                            'en': 'keyboard not connected'},
    'window.daemon_unreachable': {'pl': 'demon niedostępny: {err}',
                                  'en': 'daemon unreachable: {err}'},
    'window.lamps_at': {'pl': '{n} lampek · {dev}', 'en': '{n} lamps · {dev}'},
    'window.pick_color': {'pl': 'Wybierz kolor', 'en': 'Pick a color'},

    # --- tray ---
    'tray.tooltip_daemon_down': {'pl': 'OMEN Keyboard — demon nie odpowiada',
                                 'en': 'OMEN Keyboard — daemon not responding'},
    'tray.tooltip_bios': {'pl': 'steruje BIOS', 'en': 'BIOS in control'},
    'tray.tooltip_brightness': {'pl': 'jasność {v}/255', 'en': 'brightness {v}/255'},
    'tray.tooltip_profile': {'pl': 'profil: {name}', 'en': 'profile: {name}'},
    'tray.daemon_down': {'pl': 'Demon nie odpowiada', 'en': 'Daemon not responding'},
    'tray.not_connected': {'pl': 'Klawiatura niepodłączona',
                           'en': 'Keyboard not connected'},
    'tray.open_window': {'pl': 'Otwórz okno…', 'en': 'Open window…'},
    'tray.color': {'pl': 'Kolor', 'en': 'Color'},
    'tray.mode': {'pl': 'Tryb', 'en': 'Mode'},
    'tray.preset': {'pl': 'Preset', 'en': 'Preset'},
    'tray.brightness': {'pl': 'Jasność', 'en': 'Brightness'},
    'tray.profile': {'pl': 'Profil', 'en': 'Profile'},
    'tray.profile_none': {'pl': '(brak zapisanych)', 'en': '(none saved)'},
    'tray.reactive': {'pl': 'Reakcja na klawisze', 'en': 'React to keystrokes'},
    'tray.reactive_no_access': {'pl': 'Reakcja na klawisze (brak dostępu)',
                                'en': 'React to keystrokes (no access)'},
    'tray.control': {'pl': 'Sterowanie', 'en': 'Control'},
    'tray.control_app_tip': {'pl': 'Kolory i tryby wysyła ta aplikacja',
                             'en': 'This app sends colors and modes'},
    'tray.control_bios_tip': {'pl': 'Firmware klawiatury odgrywa fabryczny efekt',
                              'en': "The keyboard's firmware plays the factory effect"},
    'tray.language': {'pl': 'Język', 'en': 'Language'},
    'tray.quit': {'pl': 'Zakończ', 'en': 'Quit'},

    'colors.white': {'pl': 'Biel', 'en': 'White'},
    'colors.cyan': {'pl': 'Cyjan', 'en': 'Cyan'},
    'colors.green': {'pl': 'Zieleń', 'en': 'Green'},
    'colors.amber': {'pl': 'Bursztyn', 'en': 'Amber'},
    'colors.omen_red': {'pl': 'Czerwień OMEN', 'en': 'OMEN Red'},
    'colors.violet': {'pl': 'Fiolet', 'en': 'Violet'},

    # --- CLI: opisy podkomend i pomoc ---
    'cli.prog_desc': {'pl': 'Sterowanie podświetleniem klawiatury HP OMEN (HID LampArray)',
                      'en': 'HP OMEN keyboard backlight control (HID LampArray)'},
    'cli.help.socket': {'pl': 'ścieżka gniazda demona', 'en': "daemon socket path"},
    'cli.help.json': {'pl': 'surowa odpowiedź JSON', 'en': 'raw JSON response'},
    'cli.help.brightness_top': {'pl': 'jasność 0-255 przy okazji zmiany efektu',
                                'en': 'brightness 0-255 along with a mode change'},
    'cli.help.brightness': {'pl': 'jasność 0-255', 'en': 'brightness 0-255'},
    'cli.help.lang': {'pl': 'język interfejsu: pl albo en',
                      'en': 'interface language: pl or en'},

    'cli.help.status': {'pl': 'co teraz świeci', 'en': 'what is lit right now'},
    'cli.help.map': {'pl': 'mapa lampek', 'en': 'lamp map'},
    'cli.help.keylist': {'pl': 'nazwy klawiszy -> lampki', 'en': 'key names -> lamps'},
    'cli.help.effects': {'pl': 'dostępne efekty i presety', 'en': 'available effects and presets'},
    'cli.help.off': {'pl': 'zgaś', 'en': 'turn off'},
    'cli.help.release': {'pl': 'skrót na "control bios"', 'en': 'shorthand for "control bios"'},
    'cli.help.control': {'pl': 'kto steruje: bios albo app', 'en': 'who is in control: bios or app'},
    'cli.help.effect': {'pl': 'dowolny tryb, np. effect fire speed=3',
                        'en': 'any mode, e.g. effect fire speed=3'},
    'cli.help.resume': {'pl': 'przejmij kontrolę i odtwórz stan',
                        'en': 'take control back and restore state'},
    'cli.help.reactive': {'pl': 'klawisz świeci pod palcem (wymaga --with-reactive)',
                          'en': 'key lights up under your finger (needs --with-reactive)'},
    'cli.help.reactive_settings':
        {'pl': 'dla "set": np. color=FF0000 decay=0.4 curve=linear',
         'en': 'for "set": e.g. color=FF0000 decay=0.4 curve=linear'},
    'cli.help.all': {'pl': 'jednolity kolor', 'en': 'a single color'},
    'cli.help.gradient': {'pl': 'gradient wzdłuż osi', 'en': 'gradient along an axis'},
    'cli.help.wave': {'pl': 'przesuwająca się tęcza', 'en': 'a moving rainbow'},
    'cli.help.breathe': {'pl': 'pulsowanie', 'en': 'pulsing'},
    'cli.help.preset': {'pl': 'gotowiec', 'en': 'a preset'},
    'cli.help.keys': {'pl': 'podświetl wybrane klawisze', 'en': 'light up selected keys'},
    'cli.help.keys_names': {'pl': 'np. W,A,S,D,Space', 'en': 'e.g. W,A,S,D,Space'},
    'cli.help.keys_base': {'pl': 'kolor pozostałych', 'en': 'color of the rest'},
    'cli.help.brightness_cmd': {'pl': 'jasność 0-255', 'en': 'brightness 0-255'},
    'cli.help.profile': {'pl': 'profile', 'en': 'profiles'},

    'cli.error.bad_param': {'pl': 'błąd: parametr ma mieć postać klucz=wartość, dostałem: {v!r}',
                            'en': 'error: a parameter must be key=value, got: {v!r}'},
    'cli.error.profile_needs_name': {'pl': 'błąd: "profile {action}" wymaga nazwy',
                                     'en': 'error: "profile {action}" needs a name'},
    'cli.error.generic': {'pl': 'błąd:', 'en': 'error:'},
    'cli.error.bad_lang': {'pl': 'błąd: nieznany język {lang!r}; dostępne: pl, en',
                           'en': 'error: unknown language {lang!r}; available: pl, en'},

    'cli.status.device': {'pl': 'urządzenie', 'en': 'device'},
    'cli.status.state': {'pl': 'stan', 'en': 'state'},
    'cli.status.connected': {'pl': 'podłączone', 'en': 'connected'},
    'cli.status.none': {'pl': 'BRAK', 'en': 'NONE'},
    'cli.status.released_suffix': {'pl': '  (kontrola oddana firmware)',
                                   'en': '  (control released to firmware)'},
    'cli.status.lamps': {'pl': 'lampki', 'en': 'lamps'},
    'cli.status.effect': {'pl': 'efekt', 'en': 'effect'},
    'cli.status.brightness': {'pl': 'jasność', 'en': 'brightness'},
    'cli.status.profile': {'pl': 'profil', 'en': 'profile'},
    'cli.status.reactive_on': {'pl': 'reactive   : włączony, {color}, zanik {decay} s{avail}',
                               'en': 'reactive   : on, {color}, decay {decay} s{avail}'},
    'cli.status.reactive_no_access': {'pl': '  (BRAK DOSTĘPU — patrz omen-kbd reactive status)',
                                      'en': '  (NO ACCESS — see omen-kbd reactive status)'},
    'cli.status.counters': {'pl': 'licznik', 'en': 'counters'},
    'cli.status.counters_line':
        {'pl': '{frames} klatek, {reports} raportów, {reconnects} połączeń',
         'en': '{frames} frames, {reports} reports, {reconnects} reconnects'},

    'cli.brightness_result': {'pl': 'jasność: {v}/255', 'en': 'brightness: {v}/255'},
    'cli.control_bios_msg': {'pl': 'steruje firmware klawiatury (BIOS) — wraca fabryczne pulsowanie',
                             'en': "keyboard firmware is in control (BIOS) — factory pulsing is back"},
    'cli.control_result_bios': {'pl': 'steruje firmware klawiatury (BIOS)',
                                'en': 'keyboard firmware is in control (BIOS)'},
    'cli.control_result_app': {'pl': 'steruje aplikacja', 'en': 'app is in control'},

    'cli.reactive.enabled': {'pl': 'włączony', 'en': 'enabled'},
    'cli.reactive.yes': {'pl': 'tak', 'en': 'yes'},
    'cli.reactive.no': {'pl': 'nie', 'en': 'no'},
    'cli.reactive.available': {'pl': 'dostępny', 'en': 'available'},
    'cli.reactive.no_access_hint': {'pl': '  (brakuje --with-reactive przy instalacji)',
                                    'en': '  (missing --with-reactive at install)'},
    'cli.reactive.color': {'pl': 'kolor', 'en': 'color'},
    'cli.reactive.decay': {'pl': 'zanik', 'en': 'decay'},
    'cli.reactive.on_ok': {'pl': 'reactive: włączony', 'en': 'reactive: enabled'},
    'cli.reactive.on_no_access':
        {'pl': 'uwaga: reactive włączony, ale brak dostępu do klawiszy — '
               'zainstaluj z: bash packaging/install.sh --with-reactive',
         'en': 'note: reactive is enabled, but there is no access to the keys — '
               'install with: bash packaging/install.sh --with-reactive'},
    'cli.reactive.off_ok': {'pl': 'reactive: wyłączony', 'en': 'reactive: disabled'},
    'cli.reactive.set_ok': {'pl': 'reactive: parametry zapisane',
                            'en': 'reactive: settings saved'},

    'cli.effects.animated': {'pl': 'animowany', 'en': 'animated'},
    'cli.effects.static': {'pl': 'statyczny', 'en': 'static'},
    'cli.effects.color_kind': {'pl': 'kolor', 'en': 'color'},
    'cli.effects.default': {'pl': 'domyślnie', 'en': 'default'},
    'cli.effects.presets': {'pl': 'presety', 'en': 'presets'},

    'cli.profile.empty':
        {'pl': '(brak profili — zapisz bieżący: omen-kbd profile save nazwa)',
         'en': '(no profiles — save the current one: omen-kbd profile save name)'},
    'cli.profile.saved': {'pl': 'zapisany: {name}', 'en': 'saved: {name}'},
    'cli.profile.deleted': {'pl': 'usunięty: {name}', 'en': 'deleted: {name}'},

    'cli.lang.set': {'pl': 'język: {lang}', 'en': 'language: {lang}'},

    'app.needs_pyside6':
        {'pl': 'GUI wymaga PySide6, którego nie ma w systemie.\n'
               '  Fedora:  sudo dnf install python3-pyside6\n'
               'Samo sterowanie działa bez GUI — spróbuj: omen-kbd status\n',
         'en': 'The GUI needs PySide6, which is not installed.\n'
               '  Fedora:  sudo dnf install python3-pyside6\n'
               'Backlight control still works without the GUI — try: omen-kbd status\n'},
    'app.cannot_reach_daemon':
        {'pl': 'Nie mogę połączyć się z demonem.\n\n{err}',
         'en': 'Cannot connect to the daemon.\n\n{err}'},
}


# ---------------------------------------------------------------------------
# Tryby swiecenia: nazwa + etykiety parametrow + opcje wyboru
# ---------------------------------------------------------------------------

EFFECTS = {
    'static': {
        'label': {'pl': 'Jednolity kolor', 'en': 'Solid color'},
        'params': {'color': {'pl': 'Kolor', 'en': 'Color'}},
    },
    'gradient': {
        'label': {'pl': 'Gradient', 'en': 'Gradient'},
        'params': {
            'color': {'pl': 'Kolor początkowy', 'en': 'Start color'},
            'color2': {'pl': 'Kolor końcowy', 'en': 'End color'},
            'axis': {'pl': 'Kierunek', 'en': 'Direction'},
        },
    },
    'spectrum': {
        'label': {'pl': 'Cykl widma', 'en': 'Spectrum cycle'},
        'params': {
            'speed': {'pl': 'Prędkość', 'en': 'Speed'},
            'saturation': {'pl': 'Nasycenie', 'en': 'Saturation'},
        },
    },
    'wave': {
        'label': {'pl': 'Fala tęczy', 'en': 'Rainbow wave'},
        'params': {
            'speed': {'pl': 'Prędkość', 'en': 'Speed'},
            'spread': {'pl': 'Rozciągnięcie', 'en': 'Spread'},
            'axis': {'pl': 'Kierunek', 'en': 'Direction'},
            'saturation': {'pl': 'Nasycenie', 'en': 'Saturation'},
            'value': {'pl': 'Moc', 'en': 'Brightness'},
        },
    },
    'aurora': {
        'label': {'pl': 'Zorza', 'en': 'Aurora'},
        'params': {
            'color': {'pl': 'Kolor pierwszy', 'en': 'First color'},
            'color2': {'pl': 'Kolor drugi', 'en': 'Second color'},
            'speed': {'pl': 'Prędkość', 'en': 'Speed'},
            'scale': {'pl': 'Skala', 'en': 'Scale'},
        },
    },
    'plasma': {
        'label': {'pl': 'Plazma', 'en': 'Plasma'},
        'params': {
            'speed': {'pl': 'Prędkość', 'en': 'Speed'},
            'scale': {'pl': 'Skala', 'en': 'Scale'},
            'saturation': {'pl': 'Nasycenie', 'en': 'Saturation'},
            'value': {'pl': 'Moc', 'en': 'Brightness'},
        },
    },
    'wheel': {
        'label': {'pl': 'Koło tęczy', 'en': 'Rainbow wheel'},
        'params': {
            'speed': {'pl': 'Prędkość obrotu', 'en': 'Rotation speed'},
            'cx': {'pl': 'Środek w poziomie', 'en': 'Center X'},
            'cy': {'pl': 'Środek w pionie', 'en': 'Center Y'},
            'saturation': {'pl': 'Nasycenie', 'en': 'Saturation'},
            'value': {'pl': 'Moc', 'en': 'Brightness'},
        },
    },
    'breathe': {
        'label': {'pl': 'Oddech', 'en': 'Breathe'},
        'params': {
            'color': {'pl': 'Kolor', 'en': 'Color'},
            'period': {'pl': 'Okres', 'en': 'Period'},
            'floor': {'pl': 'Minimum', 'en': 'Floor'},
        },
    },
    'ripple': {
        'label': {'pl': 'Kręgi', 'en': 'Ripple'},
        'params': {
            'color': {'pl': 'Kolor grzbietu', 'en': 'Crest color'},
            'base': {'pl': 'Kolor doliny', 'en': 'Trough color'},
            'speed': {'pl': 'Prędkość', 'en': 'Speed'},
            'wavelength': {'pl': 'Odstęp kręgów', 'en': 'Ring spacing'},
            'cx': {'pl': 'Środek w poziomie', 'en': 'Center X'},
            'cy': {'pl': 'Środek w pionie', 'en': 'Center Y'},
        },
    },
    'scanner': {
        'label': {'pl': 'Skaner', 'en': 'Scanner'},
        'params': {
            'color': {'pl': 'Kolor smugi', 'en': 'Streak color'},
            'base': {'pl': 'Tło', 'en': 'Background'},
            'speed': {'pl': 'Prędkość', 'en': 'Speed'},
            'width': {'pl': 'Szerokość smugi', 'en': 'Streak width'},
            'axis': {'pl': 'Kierunek', 'en': 'Direction'},
            'bounce': {'pl': 'Ruch', 'en': 'Motion'},
        },
        'choices': {
            'bounce': {
                'bounce': {'pl': 'tam i z powrotem', 'en': 'back and forth'},
                'loop': {'pl': 'w kółko', 'en': 'looping'},
            },
        },
    },
    'twinkle': {
        'label': {'pl': 'Gwiazdy', 'en': 'Twinkle'},
        'params': {
            'color': {'pl': 'Kolor błysku', 'en': 'Sparkle color'},
            'base': {'pl': 'Tło', 'en': 'Background'},
            'speed': {'pl': 'Częstotliwość', 'en': 'Frequency'},
            'density': {'pl': 'Gęstość', 'en': 'Density'},
        },
    },
    'confetti': {
        'label': {'pl': 'Konfetti', 'en': 'Confetti'},
        'params': {
            'base': {'pl': 'Tło', 'en': 'Background'},
            'speed': {'pl': 'Częstotliwość', 'en': 'Frequency'},
            'density': {'pl': 'Gęstość', 'en': 'Density'},
        },
    },
    'fire': {
        'label': {'pl': 'Ogień', 'en': 'Fire'},
        'params': {
            'speed': {'pl': 'Żywość', 'en': 'Liveliness'},
            'height': {'pl': 'Wysokość płomienia', 'en': 'Flame height'},
            'cool': {'pl': 'Wychłodzenie', 'en': 'Cooling'},
        },
    },
    'rain': {
        'label': {'pl': 'Deszcz', 'en': 'Rain'},
        'params': {
            'color': {'pl': 'Kolor kropli', 'en': 'Drop color'},
            'base': {'pl': 'Tło', 'en': 'Background'},
            'speed': {'pl': 'Prędkość opadania', 'en': 'Fall speed'},
            'density': {'pl': 'Gęstość', 'en': 'Density'},
            'tail': {'pl': 'Długość smugi', 'en': 'Trail length'},
        },
    },
    'perkey': {
        'label': {'pl': 'Per klawisz', 'en': 'Per-key'},
        'params': {'base': {'pl': 'Tło', 'en': 'Background'}},
    },
    'off': {
        'label': {'pl': 'Zgaszone', 'en': 'Off'},
        'params': {},
    },

    # Nakladka reaktywna — nie jest trybem swiecenia, ale korzysta z tego
    # samego mechanizmu deklaracji parametrow (patrz engine/reactive.py).
    'reactive': {
        'label': {'pl': 'Reakcja na klawisze', 'en': 'React to keystrokes'},
        'params': {
            'color': {'pl': 'Kolor błysku', 'en': 'Flash color'},
            'decay': {'pl': 'Czas zaniku', 'en': 'Decay time'},
            'curve': {'pl': 'Kształt zaniku', 'en': 'Decay shape'},
            'intensity': {'pl': 'Moc błysku', 'en': 'Flash strength'},
        },
        'choices': {
            'curve': {
                'soft': {'pl': 'miękki', 'en': 'soft'},
                'linear': {'pl': 'liniowy', 'en': 'linear'},
            },
        },
    },
}

# Etykiety osi — wspolne dla wszystkich efektow z parametrem typu 'axis'
# (Gradient, Fala, Skaner). Wartosci ('x'/'y'/'d') sa neutralne od zawsze.
AXIS_CHOICES = {
    'x': {'pl': 'poziomo', 'en': 'horizontal'},
    'y': {'pl': 'pionowo', 'en': 'vertical'},
    'd': {'pl': 'po przekątnej', 'en': 'diagonal'},
}


def axis_label(value):
    lang = get_language()
    c = AXIS_CHOICES.get(value)
    if c is None:
        return value
    return c.get(lang) or c.get(DEFAULT_LANGUAGE) or value
