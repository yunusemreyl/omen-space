"""Efekty. Kazdy maluje do bufora klatki na podstawie czasu i geometrii.

Kontrakt:
  name      — identyfikator w protokole i w zapisanym stanie
  label     — nazwa dla czlowieka
  animated  — czy efekt zmienia sie w czasie. Statyczny pozwala demonowi zasnac
              calkowicie (zero CPU na baterii)
  PARAMS    — deklaracja parametrow; GUI buduje z niej kontrolki, wiec dodanie
              efektu nie wymaga pisania ani jednej linii interfejsu
  render(t, layout, frame) — t w sekundach od wlaczenia efektu

Wspolrzedne biora sie z firmware'u (layout.nx/ny, znormalizowane 0..1), wiec
wszystko dziala na kazdym wariancie klawiatury bez przerabiania kodu.

Efekty animowane sa BEZSTANOWE — klatka zalezy wylacznie od t. Deszcz nie trzyma
listy kropli, tylko wylicza, ktore z nich zylyby w danej chwili. Dzieki temu nic
sie nie rozjezdza po wybudzeniu, restart demona nie gubi fazy, a przewiniecie
czasu w podgladzie GUI daje dokladnie to samo, co widac na klawiaturze.
"""

import collections
import math

from ..core import color as C

REGISTRY = {}

# Deklaracja jednego parametru. kind: color | float | axis | choice
P = collections.namedtuple(
    'P', 'name label kind lo hi default step choices',
    defaults=('float', 0.0, 1.0, 0.0, 0.01, ()))


def color_p(name, label, default):
    return P(name, label, 'color', default=default)


def axis_p(default='x'):
    return P('axis', 'Kierunek', 'axis', default=default)


def register(cls):
    REGISTRY[cls.name] = cls
    return cls


# ---------------------------------------------------------------- pomocnicze --

def _fnv(*ints):
    """Hash calkowitoliczbowy -> 0..1. Deterministyczny szum bez modulu random:
    ta sama klatka wychodzi tak samo w demonie i w podgladzie GUI, w kazdym
    uruchomieniu.

    Akumulacja FNV-1a plus finalizer w stylu MurmurHash3 (fmix32). Sam FNV-1a
    bez finalizera ma slabe lawinowanie dla KOLEJNYCH i (i, i+1, i+2, ...) —
    kolejne wyniki dryfowaly niemal liniowo zamiast byc rozrzucone. W praktyce
    ujawnilo sie to w Deszczu: numer kropli jest kolejna liczba calkowita,
    wiec kolumny x kolejnych kropli wychodzily niemal identyczne i deszcz
    wygladal jak jedna blyskajaca linia zamiast kropli rozrzuconych po calej
    szerokosci. Finalizer daje pelne lawinowanie niezaleznie od wzorca wejscia.
    """
    h = 2166136261
    for v in ints:
        h ^= v & 0xFFFFFFFF
        h = (h * 16777619) & 0xFFFFFFFF
    h ^= h >> 16
    h = (h * 0x85EBCA6B) & 0xFFFFFFFF
    h ^= h >> 13
    h = (h * 0xC2B2AE35) & 0xFFFFFFFF
    h ^= h >> 16
    return h / 4294967295.0


def _smoothstep(t):
    return t * t * (3.0 - 2.0 * t)


def _noise(seed, x):
    """Gladki szum 1D — interpolacja miedzy wartosciami w calkowitych punktach."""
    i = math.floor(x)
    return (lambda a, b, f: a + (b - a) * _smoothstep(f))(
        _fnv(seed, int(i)), _fnv(seed, int(i) + 1), x - i)


def _ramp(stops, n=256):
    """Buduje tablice n kolorow z listy (pozycja, (r,g,b))."""
    out = []
    for i in range(n):
        t = i / (n - 1)
        for (p0, c0), (p1, c1) in zip(stops, stops[1:]):
            if t <= p1 or p1 == stops[-1][0]:
                span = (p1 - p0) or 1.0
                out.append(C.lerp(c0, c1, (t - p0) / span))
                break
        else:
            out.append(stops[-1][1])
    return out


FIRE_RAMP = _ramp([(0.00, (0, 0, 0)), (0.18, (48, 0, 0)), (0.40, (200, 24, 0)),
                   (0.66, (255, 112, 0)), (0.86, (255, 200, 40)),
                   (1.00, (255, 246, 200))])


# -------------------------------------------------------------------- bazowe --

class Effect:
    name = '?'
    label = '?'
    animated = False
    PARAMS = ()

    def __init__(self, **kw):
        self.params = {}
        for p in self.PARAMS:
            v = kw.get(p.name, p.default)
            if p.kind == 'color':
                v = C.to_hex(C.parse(v))
            elif p.kind == 'float':
                v = max(p.lo, min(p.hi, float(v)))
            elif p.kind in ('axis', 'choice'):
                allowed = p.choices or ('x', 'y', 'd')
                v = v if v in allowed else p.default
            self.params[p.name] = v
        self.setup()

    def setup(self):
        """Rozpakowanie parametrow do pol. Wolane raz, po walidacji."""

    def render(self, t, layout, frame):
        raise NotImplementedError

    def describe(self):
        return dict(self.params, effect=self.name)

    # --- pomocnicze dla podklas ---

    def col(self, name):
        return C.parse(self.params[name])

    def num(self, name):
        return self.params[name]

    @staticmethod
    def axis_pos(layout, axis):
        if axis == 'y':
            return layout.ny
        if axis == 'd':
            return [(x + y) * 0.5 for x, y in zip(layout.nx, layout.ny)]
        return layout.nx

    @staticmethod
    def aspect(layout):
        """Stosunek szerokosci do wysokosci — potrzebny tam, gdzie ksztalt ma
        byc okragly w rzeczywistosci, a nie w znormalizowanych wspolrzednych."""
        a = getattr(layout, 'attrs', None) or {}
        w, h = a.get('width_um') or 1, a.get('height_um') or 1
        return w / h

    def cached(self, layout, build):
        """Wynik zalezny tylko od geometrii liczymy raz na uklad."""
        if getattr(self, '_cache_for', None) is not layout:
            self._cache = build()
            self._cache_for = layout
        return self._cache


# ------------------------------------------------------------------ statyczne --

@register
class Static(Effect):
    name, label, animated = 'static', 'Jednolity kolor', False
    PARAMS = (color_p('color', 'Kolor', '#FFFFFF'),)

    def setup(self):
        self.c = self.col('color')

    def render(self, t, layout, frame):
        frame.fill(self.c)


@register
class Gradient(Effect):
    name, label, animated = 'gradient', 'Gradient', False
    PARAMS = (color_p('color', 'Kolor poczatkowy', '#FF0000'),
              color_p('color2', 'Kolor koncowy', '#0000FF'),
              axis_p('x'))

    def setup(self):
        self.a, self.b = self.col('color'), self.col('color2')

    def render(self, t, layout, frame):
        frame.rgb = self.cached(layout, lambda: [
            C.lerp(self.a, self.b, p)
            for p in self.axis_pos(layout, self.params['axis'])])


@register
class PerKey(Effect):
    name, label, animated = 'perkey', 'Per klawisz', False
    PARAMS = (color_p('base', 'Tlo', '#000000'),)

    def __init__(self, colors=None, **kw):
        colors = colors or {}
        self._colors = {int(k): C.to_hex(C.parse(v)) for k, v in colors.items()}
        super().__init__(**kw)

    def setup(self):
        self.map = {k: C.parse(v) for k, v in self._colors.items()}
        self.base = self.col('base')

    def describe(self):
        return dict(self.params, effect=self.name, colors=dict(self._colors))

    def render(self, t, layout, frame):
        m, base = self.map, self.base
        frame.rgb = [m.get(i, base) for i in layout.ids]


@register
class Off(Effect):
    name, label, animated = 'off', 'Zgaszone', False

    def render(self, t, layout, frame):
        frame.fill((0, 0, 0))


# ----------------------------------------------------------------- animowane --

@register
class Wave(Effect):
    name, label, animated = 'wave', 'Fala teczy', True
    PARAMS = (P('speed', 'Predkosc', lo=0.01, hi=1.0, default=0.15),
              P('spread', 'Rozciagniecie', lo=0.1, hi=3.0, default=1.0),
              axis_p('x'),
              P('saturation', 'Nasycenie', lo=0.0, hi=1.0, default=1.0),
              P('value', 'Moc', lo=0.1, hi=1.0, default=1.0))

    def setup(self):
        self.speed, self.spread = self.num('speed'), self.num('spread')
        self.sat, self.val = self.num('saturation'), self.num('value')

    def render(self, t, layout, frame):
        pos = self.cached(layout,
                          lambda: self.axis_pos(layout, self.params['axis']))
        phase = t * self.speed
        if self.sat >= 0.999 and self.val >= 0.999:
            tab = C.RAINBOW          # pelna tecza z gotowej tablicy
            frame.rgb = [tab[int((p * self.spread + phase) * 256) & 255]
                         for p in pos]
        else:
            frame.rgb = [C.hsv((p * self.spread + phase) * 360.0,
                               self.sat, self.val) for p in pos]


@register
class Spectrum(Effect):
    name, label, animated = 'spectrum', 'Cykl widma', True
    PARAMS = (P('speed', 'Predkosc', lo=0.01, hi=1.0, default=0.08),
              P('saturation', 'Nasycenie', lo=0.0, hi=1.0, default=1.0))

    def setup(self):
        self.speed, self.sat = self.num('speed'), self.num('saturation')

    def render(self, t, layout, frame):
        frame.fill(C.hsv(t * self.speed * 360.0, self.sat, 1.0))


@register
class Breathe(Effect):
    name, label, animated = 'breathe', 'Oddech', True
    PARAMS = (color_p('color', 'Kolor', '#00FFC0'),
              P('period', 'Okres', lo=0.5, hi=20.0, default=4.0, step=0.1),
              P('floor', 'Minimum', lo=0.0, hi=0.9, default=0.05))

    def setup(self):
        self.c = self.col('color')
        self.period = max(0.2, self.num('period'))
        self.floor = self.num('floor')

    def render(self, t, layout, frame):
        # cos daje lagodne wytlumienie na obu koncach, inaczej niz piloksztaltny sin
        k = (1.0 - math.cos(2.0 * math.pi * t / self.period)) * 0.5
        frame.fill(C.scale(self.c, self.floor + (1.0 - self.floor) * k))


@register
class Scanner(Effect):
    name, label, animated = 'scanner', 'Skaner', True
    # Wartosci 'bounce'/'loop' sa jezykowo neutralne — to jest STAN zapisywany
    # w profilach, nie tekst do wyswietlenia. Etykiety dla obu jezykow sa w
    # omenkbd/i18n.py (EFFECTS['scanner']['choices']['bounce']); GUI/CLI
    # pobieraja je stamtad, nie z tej deklaracji.
    PARAMS = (color_p('color', 'Kolor smugi', '#FF2000'),
              color_p('base', 'Tlo', '#000000'),
              P('speed', 'Predkosc', lo=0.05, hi=3.0, default=0.6),
              P('width', 'Szerokosc smugi', lo=0.02, hi=0.6, default=0.18),
              axis_p('x'),
              P('bounce', 'Ruch', 'choice', default='bounce',
                choices=('bounce', 'loop')))

    def setup(self):
        self.c, self.base = self.col('color'), self.col('base')
        self.speed, self.width = self.num('speed'), self.num('width')
        self.bounce = self.params['bounce'] == 'bounce'

    def render(self, t, layout, frame):
        pos = self.cached(layout,
                          lambda: self.axis_pos(layout, self.params['axis']))
        u = (t * self.speed) % 1.0
        head = 1.0 - abs(2.0 * u - 1.0) if self.bounce else u
        w, c, base = self.width, self.c, self.base
        out = []
        for p in pos:
            d = abs(p - head)
            if not self.bounce:                 # loop: krawedzie sie sklejaja
                d = min(d, 1.0 - d)
            k = 0.0 if d >= w else (1.0 - d / w) ** 2
            out.append(base if k <= 0.0 else C.lerp(base, c, k))
        frame.rgb = out


@register
class Ripple(Effect):
    name, label, animated = 'ripple', 'Kregi', True
    PARAMS = (color_p('color', 'Kolor grzbietu', '#00C2FF'),
              color_p('base', 'Kolor doliny', '#100030'),
              P('speed', 'Predkosc', lo=0.05, hi=3.0, default=0.5),
              P('wavelength', 'Odstep kregow', lo=0.05, hi=1.0, default=0.28),
              P('cx', 'Srodek w poziomie', lo=0.0, hi=1.0, default=0.5),
              P('cy', 'Srodek w pionie', lo=0.0, hi=1.0, default=0.5))

    def setup(self):
        self.c, self.base = self.col('color'), self.col('base')
        self.speed = self.num('speed')
        self.wl = max(0.01, self.num('wavelength'))

    def render(self, t, layout, frame):
        # Odleglosc liczymy w proporcjach FIZYCZNYCH, inaczej "kregi" wychodza
        # elipsami rozciagnietymi na szerokosc klawiatury.
        def build():
            a = self.aspect(layout)
            cx, cy = self.num('cx'), self.num('cy')
            return [math.hypot((x - cx) * a, y - cy)
                    for x, y in zip(layout.nx, layout.ny)]
        dist = self.cached(layout, build)
        k = 2.0 * math.pi / self.wl
        phase = t * self.speed * 2.0 * math.pi
        c, base = self.c, self.base
        frame.rgb = [C.lerp(base, c, 0.5 + 0.5 * math.sin(d * k - phase))
                     for d in dist]


@register
class Aurora(Effect):
    name, label, animated = 'aurora', 'Zorza', True
    PARAMS = (color_p('color', 'Kolor pierwszy', '#00FF9C'),
              color_p('color2', 'Kolor drugi', '#7A00FF'),
              P('speed', 'Predkosc', lo=0.01, hi=1.5, default=0.18),
              P('scale', 'Skala', lo=0.3, hi=4.0, default=1.2))

    def setup(self):
        self.a, self.b = self.col('color'), self.col('color2')
        self.speed, self.scale = self.num('speed'), self.num('scale')

    def render(self, t, layout, frame):
        s = self.scale
        p1 = t * self.speed * 2.2
        p2 = t * self.speed * -1.4
        a, b = self.a, self.b
        out = []
        # Trzy fale o niewspolmiernych okresach — suma nie ma widocznej petli.
        for x, y in zip(layout.nx, layout.ny):
            v = (0.5
                 + 0.28 * math.sin((x * 4.1 * s) + p1)
                 + 0.16 * math.sin((y * 6.7 * s) + p2)
                 + 0.12 * math.sin(((x + y) * 3.3 * s) + p1 * 0.61))
            out.append(C.lerp(a, b, 0.0 if v < 0.0 else (1.0 if v > 1.0 else v)))
        frame.rgb = out


@register
class Plasma(Effect):
    name, label, animated = 'plasma', 'Plazma', True
    PARAMS = (P('speed', 'Predkosc', lo=0.01, hi=1.0, default=0.12),
              P('scale', 'Skala', lo=0.3, hi=4.0, default=1.4),
              P('saturation', 'Nasycenie', lo=0.0, hi=1.0, default=1.0),
              P('value', 'Moc', lo=0.1, hi=1.0, default=1.0))

    def setup(self):
        self.speed = self.num('speed')
        self.scale = self.num('scale')
        self.sat = self.num('saturation')
        self.val = self.num('value')

    def render(self, t, layout, frame):
        s = self.scale
        p1 = t * self.speed * 2.0
        p2 = t * self.speed * -1.3
        p3 = t * self.speed * 0.7
        sat, val = self.sat, self.val
        full = sat >= 0.999 and val >= 0.999
        tab = C.RAINBOW
        out = []
        # Cztery fale o niewspolmiernych czestotliwosciach (w tym jedna
        # promieniowa od srodka) — plynny, organiczny wzor bez widocznej petli,
        # w odroznieniu od Fali (jedna os) i Zorzy (dwa kolory zamiast pelnego
        # widma).
        for x, y in zip(layout.nx, layout.ny):
            h = (math.sin((x * 4.0 * s) + p1)
                 + math.sin((y * 4.0 * s) + p2)
                 + math.sin(((x + y) * 3.0 * s) + p3)
                 + math.sin((math.hypot(x - 0.5, y - 0.5) * 5.0 * s) - p1 * 0.6))
            hue = (h + 4.0) / 8.0
            if full:
                out.append(tab[int(hue * 256.0) & 255])
            else:
                out.append(C.hsv(hue * 360.0, sat, val))
        frame.rgb = out


@register
class Wheel(Effect):
    name, label, animated = 'wheel', 'Kolo teczy', True
    PARAMS = (P('speed', 'Predkosc obrotu', lo=0.01, hi=1.0, default=0.15),
              P('cx', 'Srodek w poziomie', lo=0.0, hi=1.0, default=0.5),
              P('cy', 'Srodek w pionie', lo=0.0, hi=1.0, default=0.5),
              P('saturation', 'Nasycenie', lo=0.0, hi=1.0, default=1.0),
              P('value', 'Moc', lo=0.1, hi=1.0, default=1.0))

    def setup(self):
        self.speed = self.num('speed')
        self.sat = self.num('saturation')
        self.val = self.num('value')

    def render(self, t, layout, frame):
        # Kat liczymy w proporcjach FIZYCZNYCH (przez aspect), inaczej kolo
        # wychodzi elipsa rozciagnieta na szerokosc klawiatury.
        def build():
            a = self.aspect(layout)
            cx, cy = self.num('cx'), self.num('cy')
            return [math.degrees(math.atan2(y - cy, (x - cx) * a))
                   for x, y in zip(layout.nx, layout.ny)]
        angles = self.cached(layout, build)
        phase = t * self.speed * 360.0
        sat, val = self.sat, self.val
        full = sat >= 0.999 and val >= 0.999
        tab = C.RAINBOW
        out = []
        for ang in angles:
            deg = (ang + phase) % 360.0
            if full:
                out.append(tab[int(deg / 360.0 * 256.0) & 255])
            else:
                out.append(C.hsv(deg, sat, val))
        frame.rgb = out


@register
class Confetti(Effect):
    name, label, animated = 'confetti', 'Konfetti', True
    PARAMS = (color_p('base', 'Tlo', '#000000'),
              P('speed', 'Czestotliwosc', lo=0.05, hi=3.0, default=0.5),
              P('density', 'Gestosc', lo=0.02, hi=1.0, default=0.25))

    def setup(self):
        self.base = self.col('base')
        self.speed = self.num('speed')
        self.density = self.num('density')

    def render(self, t, layout, frame):
        # Jak Gwiazdy (Twinkle), ale kazda iskra dostaje WLASNY, losowy odcien
        # z pelnego widma zamiast jednego stalego koloru — tecza migoczaca
        # punktowo, nie plynaca fala.
        phases = self.cached(layout, lambda: [_fnv(41, i) for i in layout.ids])
        base, dens = self.base, self.density
        out = []
        for i, ph in zip(layout.ids, phases):
            u = t * self.speed + ph
            cycle = int(u)
            if _fnv(53, i, cycle) >= dens:
                out.append(base)
                continue
            hue = _fnv(59, i, cycle) * 360.0
            k = math.sin(math.pi * (u - cycle))
            out.append(C.lerp(base, C.hsv(hue, 1.0, 1.0), k * k))
        frame.rgb = out


@register
class Twinkle(Effect):
    name, label, animated = 'twinkle', 'Gwiazdy', True
    PARAMS = (color_p('color', 'Kolor blysku', '#FFFFFF'),
              color_p('base', 'Tlo', '#000814'),
              P('speed', 'Czestotliwosc', lo=0.05, hi=3.0, default=0.5),
              P('density', 'Gestosc', lo=0.02, hi=1.0, default=0.25))

    def setup(self):
        self.c, self.base = self.col('color'), self.col('base')
        self.speed, self.density = self.num('speed'), self.num('density')

    def render(self, t, layout, frame):
        # Kazda lampka ma wlasna faze, wiec blyski nie ida rownym rytmem.
        phases = self.cached(layout,
                             lambda: [_fnv(11, i) for i in layout.ids])
        c, base, dens = self.c, self.base, self.density
        out = []
        for i, ph in zip(layout.ids, phases):
            u = t * self.speed + ph
            cycle = int(u)
            if _fnv(23, i, cycle) >= dens:
                out.append(base)
                continue
            k = math.sin(math.pi * (u - cycle))
            out.append(C.lerp(base, c, k * k))
        frame.rgb = out


@register
class Fire(Effect):
    name, label, animated = 'fire', 'Ogien', True
    PARAMS = (P('speed', 'Zywosc', lo=0.2, hi=6.0, default=2.2),
              P('height', 'Wysokosc plomienia', lo=0.1, hi=1.0, default=0.65),
              P('cool', 'Wychlodzenie', lo=0.0, hi=0.8, default=0.18))

    def setup(self):
        self.speed = self.num('speed')
        self.height = self.num('height')
        self.cool = self.num('cool')

    def render(self, t, layout, frame):
        # Kolumny bierzemy z rzeczywistego x, ale zaokraglone do kubelkow —
        # sasiednie klawisze maja migotac razem, inaczej wyglada to jak snieg.
        cols = self.cached(layout, lambda: [int(x * 28.0) for x in layout.nx])
        ny = layout.ny
        u = t * self.speed
        h, cool = self.height, self.cool
        ramp = FIRE_RAMP
        out = []
        for col, y in zip(cols, ny):
            # dwie oktawy szumu: wolna baza plomienia + szybkie migotanie
            n = 0.66 * _noise(col, u) + 0.34 * _noise(col + 977, u * 2.7)
            heat = n * (1.0 - h * (1.0 - y)) - cool
            out.append(ramp[0] if heat <= 0.0
                       else ramp[min(255, int(heat * 255.0))])
        frame.rgb = out


@register
class Rain(Effect):
    name, label, animated = 'rain', 'Deszcz', True
    PARAMS = (color_p('color', 'Kolor kropli', '#00C2FF'),
              color_p('base', 'Tlo', '#000000'),
              P('speed', 'Predkosc opadania', lo=0.1, hi=4.0, default=1.1),
              P('density', 'Gestosc', lo=0.5, hi=30.0, default=8.0, step=0.5),
              P('tail', 'Dlugosc smugi', lo=0.05, hi=1.0, default=0.45))

    SPREAD = 0.035          # jak szeroko kropla oswietla sasiednie klawisze

    def setup(self):
        self.c, self.base = self.col('color'), self.col('base')
        self.speed = self.num('speed')
        self.rate = self.num('density')
        self.tail = max(0.02, self.num('tail'))

    def _drops(self, t):
        """Krople zyjace w chwili t. Bezstanowo: numer kropli wyznacza jej czas
        startu i kolumne, wiec nie trzymamy zadnej listy miedzy klatkami."""
        life = (1.0 + self.tail) / self.speed
        first = max(0, math.ceil((t - life) * self.rate))
        last = math.floor(t * self.rate)
        for i in range(first, last + 1):
            yield _fnv(31, i), (t - i / self.rate) * self.speed

    def render(self, t, layout, frame):
        drops = list(self._drops(t))
        c, base, tail, spread = self.c, self.base, self.tail, self.SPREAD
        out = []
        for x, y in zip(layout.nx, layout.ny):
            k = 0.0
            for dx, head in drops:
                if abs(x - dx) > spread:
                    continue
                dy = head - y
                if 0.0 <= dy <= tail:
                    v = 1.0 - dy / tail
                    if v > k:
                        k = v
            out.append(base if k <= 0.0 else C.lerp(base, c, k * k))
        frame.rgb = out


# ------------------------------------------------------------------ fabryki --

def make(name, params=None):
    params = dict(params or {})
    params.pop('effect', None)
    try:
        cls = REGISTRY[name]
    except KeyError:
        raise ValueError(
            f'nieznany efekt {name!r}; dostepne: {", ".join(sorted(REGISTRY))}'
        ) from None
    return cls(**params)


def from_dict(d):
    d = dict(d)
    return make(d.pop('effect', 'static'), d)


def catalogue():
    """Opis wszystkich efektow dla GUI — jedno zrodlo prawdy o parametrach."""
    return [{'name': cls.name, 'label': cls.label, 'animated': cls.animated,
             'params': [p._asdict() for p in cls.PARAMS]}
            for cls in ORDER]


# Kolejnosc na liscie w GUI: od najprostszych do najbardziej rozbudowanych.
ORDER = [REGISTRY[n] for n in (
    'static', 'gradient', 'spectrum', 'wave', 'aurora', 'plasma', 'wheel',
    'breathe', 'ripple', 'scanner', 'twinkle', 'confetti', 'fire', 'rain',
    'perkey', 'off')]


# Gotowce adresujace klawisze po nazwie — id zaleza od egzemplarza, wiec
# konkretny efekt da sie zbudowac dopiero przy znanym ukladzie.
KEY_PRESETS = {
    'gaming': {'keys': ['W', 'A', 'S', 'D', 'Up', 'Down', 'Left', 'Right',
                        'Space', 'LShift', 'LCtrl'],
               'color': '#FF2000', 'base': '#0A0A14'},
    'typing': {'keys': ['F', 'J'], 'color': '#00FFC0', 'base': '#141414'},
    'wasd':   {'keys': ['W', 'A', 'S', 'D'], 'color': '#FFFFFF',
               'base': '#000000'},
    'mods':   {'keys': ['LCtrl', 'LShift', 'LAlt', 'LMeta', 'RCtrl', 'RShift',
                        'RAlt', 'Tab', 'CapsLock', 'Esc'],
               'color': '#FF8000', 'base': '#101018'},
}


def preset(name, layout):
    try:
        p = KEY_PRESETS[name]
    except KeyError:
        raise ValueError(
            f'nieznany preset {name!r}; dostepne: {", ".join(sorted(KEY_PRESETS))}'
        ) from None
    colors = {}
    for key in p['keys']:
        for lid in layout.resolve(key):
            colors[lid] = p['color']
    return PerKey(colors=colors, base=p['base'])


def keys_effect(names, color, base, layout):
    """Podswietl wymienione klawisze. Jeden klawisz = czasem kilka lampek."""
    colors = {}
    unknown = []
    for name in names:
        lids = layout.resolve(name)
        if not lids:
            unknown.append(name)
        for lid in lids:
            colors[lid] = color
    if unknown:
        raise ValueError('nie ma takich klawiszy: ' + ', '.join(unknown))
    return PerKey(colors=colors, base=base)
