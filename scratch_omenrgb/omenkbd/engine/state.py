"""Trwalosc: ostatni stan i nazwane profile w ~/.config/omen-kbd/.

Stan zapisujemy przy kazdej zmianie, atomowo (zapis do .tmp + rename), zeby
przerwanie zasilania nie zostawilo obcietego JSON-a, ktory zablokowalby start.
"""

import json
import os

def _config_dir():
    """Katalog stanu. Pod systemd bierzemy StateDirectory (/var/lib/omen-kbd),
    bo demon dziala jako uzytkownik systemowy i nie ma dostepu do ~/.
    Fallback na XDG sluzy uruchomieniu z reki przy pracy nad kodem."""
    sd = os.environ.get('STATE_DIRECTORY')
    if sd:
        return sd.split(':')[0]
    return os.path.join(
        os.environ.get('XDG_CONFIG_HOME') or os.path.expanduser('~/.config'),
        'omen-kbd')


CONFIG_DIR = _config_dir()
STATE_PATH = os.path.join(CONFIG_DIR, 'state.json')
REACTIVE_PATH = os.path.join(CONFIG_DIR, 'reactive.json')
PROFILE_DIR = os.path.join(CONFIG_DIR, 'profiles')

DEFAULT = {'effect': 'static', 'color': '#FFFFFF'}
DEFAULT_BRIGHTNESS = 200
DEFAULT_REACTIVE = {'color': '#FFFFFF', 'decay': 0.6, 'curve': 'soft',
                    'intensity': 1.0}


def _write_atomic(path, obj):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    tmp = path + '.tmp'
    with open(tmp, 'w') as f:
        json.dump(obj, f, indent=1)
        f.flush()
        os.fsync(f.fileno())
    os.replace(tmp, path)


def load_state():
    try:
        with open(STATE_PATH) as f:
            d = json.load(f)
    except (OSError, ValueError):
        return dict(DEFAULT), DEFAULT_BRIGHTNESS, None
    eff = d.get('effect_params') or dict(DEFAULT)
    br = int(d.get('brightness', DEFAULT_BRIGHTNESS))
    return eff, max(0, min(255, br)), d.get('profile')


def save_state(effect_params, brightness, profile=None):
    _write_atomic(STATE_PATH, {'effect_params': effect_params,
                               'brightness': brightness,
                               'profile': profile})


# --- reactive typing: osobny plik, bo dotyczy zupelnie innego uprawnienia
# (dostep do klawiszy) i chcemy, zeby dalo sie go skasowac niezaleznie od
# reszty stanu, np. przy odbieraniu dostepu bez odinstalowania calosci ---

def load_reactive():
    try:
        with open(REACTIVE_PATH) as f:
            d = json.load(f)
    except (OSError, ValueError):
        return dict(DEFAULT_REACTIVE), False
    params = dict(DEFAULT_REACTIVE)
    params.update(d.get('params') or {})
    return params, bool(d.get('enabled', False))


def save_reactive(params, enabled):
    _write_atomic(REACTIVE_PATH, {'params': params, 'enabled': enabled})


# --- profile ---

def _safe(name):
    if not name or any(c in name for c in '/\\\0') or name in ('.', '..'):
        raise ValueError(f'niedozwolona nazwa profilu: {name!r}')
    return name


def profile_path(name):
    return os.path.join(PROFILE_DIR, _safe(name) + '.json')


def list_profiles():
    try:
        return sorted(f[:-5] for f in os.listdir(PROFILE_DIR) if f.endswith('.json'))
    except OSError:
        return []


def save_profile(name, effect_params, brightness):
    _write_atomic(profile_path(name),
                  {'effect_params': effect_params, 'brightness': brightness})


def load_profile(name):
    path = profile_path(name)
    try:
        with open(path) as f:
            d = json.load(f)
    except OSError:
        raise ValueError(f'nie ma profilu {name!r}') from None
    except ValueError:
        raise ValueError(f'profil {name!r} jest uszkodzony') from None
    return d.get('effect_params') or dict(DEFAULT), \
        int(d.get('brightness', DEFAULT_BRIGHTNESS))


def delete_profile(name):
    try:
        os.remove(profile_path(name))
    except OSError:
        raise ValueError(f'nie ma profilu {name!r}') from None
