"""Model mapy lampek: budowa z firmware'u, cache na dysku, adresowanie po nazwie.

Mapa jest wlasciwoscia egzemplarza klawiatury, nie modelu laptopa — warianty
ISO/ANSI i wersje bez bloku numerycznego maja inna. Zawsze czytamy z urzadzenia,
nigdy nie hardkodujemy.
"""

import json
import os

from .hidkeys import key_name

def _cache_dir():
    """Pod systemd CacheDirectory (/var/cache/omen-kbd) — demon dziala jako
    uzytkownik systemowy. Fallback na XDG dla uruchomienia z reki."""
    cd = os.environ.get('CACHE_DIRECTORY')
    if cd:
        return cd.split(':')[0]
    return os.path.join(
        os.environ.get('XDG_CACHE_HOME') or os.path.expanduser('~/.cache'),
        'omen-kbd')


CACHE_DIR = _cache_dir()

CACHE_VERSION = 1


class Lamp:
    __slots__ = ('id', 'x_um', 'y_um', 'z_um', 'binding', 'key', 'programmable',
                 'purposes')

    def __init__(self, d):
        self.id = d['id']
        self.x_um = d['x_um']
        self.y_um = d['y_um']
        self.z_um = d['z_um']
        self.binding = d['input_binding']
        self.key = key_name(self.binding)
        self.programmable = d.get('programmable', True)
        self.purposes = d.get('purposes', [])

    def as_dict(self):
        return {'id': self.id, 'x_um': self.x_um, 'y_um': self.y_um,
                'z_um': self.z_um, 'input_binding': self.binding, 'key': self.key,
                'programmable': self.programmable, 'purposes': self.purposes}


class Layout:
    """Lampki posortowane po id, plus indeksy: nazwa klawisza -> lampki, oraz
    znormalizowane wspolrzedne 0..1 dla gradientow i fal."""

    def __init__(self, attrs, lamps):
        self.attrs = attrs
        self.lamps = sorted(lamps, key=lambda l: l.id)
        self.count = len(self.lamps)
        self.by_id = {l.id: l for l in self.lamps}

        self.by_key = {}
        for l in self.lamps:
            if l.key:
                self.by_key.setdefault(l.key.upper(), []).append(l.id)

        # HID Usage (surowy numer z InputBinding) -> lista lamp id. Uzywane przez
        # reactive typing: kod klawisza z evdev tlumaczy sie na HID Usage
        # (core/evdev_map.py), a to jest dokladnie to, co niesie InputBinding.
        # Indeksowanie po numerze, nie po nazwie, bo nazwa to tylko etykieta do
        # wyswietlenia — usage jest tym, co faktycznie przyszlo z firmware'u.
        self.by_binding = {}
        for l in self.lamps:
            if l.binding:
                self.by_binding.setdefault(l.binding, []).append(l.id)

        w = attrs['width_um'] or 1
        h = attrs['height_um'] or 1
        # Indeksowane pozycja w self.lamps, nie id — silnik efektow chodzi po liscie.
        self.nx = [l.x_um / w for l in self.lamps]
        self.ny = [l.y_um / h for l in self.lamps]
        self.ids = [l.id for l in self.lamps]

        # Czy id sa ciagle 0..count-1 — wtedy wolno uzyc taniego LampRangeUpdate.
        self.contiguous = self.ids == list(range(self.count))

    def resolve(self, name):
        """Nazwa klawisza -> lista lamp id. Jednemu klawiszowi odpowiada czasem
        kilka lampek (Spacja: 5, LShift: 3, Enter: 2)."""
        return list(self.by_key.get(name.upper(), ()))

    def key_names(self):
        return sorted(self.by_key)

    def rows(self):
        """Lampki pogrupowane po wspolrzednej Y — do rysowania i debugowania."""
        out = {}
        for l in self.lamps:
            out.setdefault(l.y_um, []).append(l)
        return [out[y] for y in sorted(out)]

    # --- serializacja ---

    def as_dict(self):
        return {'version': CACHE_VERSION, 'attrs': self.attrs,
                'lamps': [l.as_dict() for l in self.lamps]}

    @classmethod
    def from_dict(cls, d):
        return cls(d['attrs'], [Lamp(x) for x in d['lamps']])


def cache_key(attrs, dev):
    """Klucz cache: tozsamosc USB + ksztalt tablicy. Zmiana ktoregokolwiek
    z tych pol oznacza inna klawiature i wymusza przebudowe."""
    return 'layout-{:04x}{:04x}-{}-{}x{}.json'.format(
        dev.get('vid') or 0, dev.get('pid') or 0,
        attrs['lamp_count'], attrs['width_um'], attrs['height_um'])


def build(la, progress=None):
    """Odczytuje mape z firmware'u. ~120 ioctli, robic raz i cache'owac.

    Kursor Reportu 3 startuje w nieprzewidywalnym miejscu i sie zawija, wiec
    zbieramy do slownika az uzbiera sie lamp_count unikatow. Limit prob chroni
    przed nieskonczona petla, gdyby firmware zaczal powtarzac ten sam wpis.
    """
    n = la.attrs['lamp_count']
    seen = {}
    attempts = 0
    limit = n * 2 + 16
    while len(seen) < n and attempts < limit:
        d = la.next_lamp_attributes()
        attempts += 1
        seen.setdefault(d['id'], d)
        if progress:
            progress(len(seen), n)
    if len(seen) < n:
        raise RuntimeError(
            f'firmware zwrocil {len(seen)} z {n} lampek w {attempts} probach')
    return Layout(dict(la.attrs), [Lamp(d) for d in seen.values()])


def load(la, dev, refresh=False):
    """Layout z cache albo z urzadzenia. Zwraca (layout, from_cache)."""
    path = os.path.join(CACHE_DIR, cache_key(la.attrs, dev))
    if not refresh:
        try:
            with open(path) as f:
                d = json.load(f)
            if d.get('version') == CACHE_VERSION \
                    and len(d.get('lamps', ())) == la.attrs['lamp_count']:
                return Layout.from_dict(d), True
        except (OSError, ValueError, KeyError):
            pass
    layout = build(la)
    save(layout, path)
    return layout, False


def save(layout, path):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    tmp = path + '.tmp'
    with open(tmp, 'w') as f:
        json.dump(layout.as_dict(), f, indent=1)
    os.replace(tmp, path)
