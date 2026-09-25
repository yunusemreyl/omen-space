"""Kolory. Wewnetrznie (r, g, b) 0-255; jasnosc jest osobnym, globalnym kanalem
(pole Intensity w raportach), zeby zmiana jasnosci nie gubila odcienia."""

import colorsys

NAMED = {
    'black': (0, 0, 0), 'white': (255, 255, 255), 'red': (255, 0, 0),
    'green': (0, 255, 0), 'blue': (0, 0, 255), 'cyan': (0, 255, 255),
    'magenta': (255, 0, 255), 'yellow': (255, 255, 0), 'orange': (255, 96, 0),
    'purple': (160, 0, 255), 'pink': (255, 64, 160), 'teal': (0, 255, 192),
    'omen': (255, 32, 0),
}


class ColorError(ValueError):
    pass


def parse(s):
    """'#RRGGBB', 'RRGGBB', 'RGB' albo nazwa -> (r, g, b)."""
    if isinstance(s, (list, tuple)):
        if len(s) != 3:
            raise ColorError(f'kolor jako krotka musi miec 3 skladowe: {s!r}')
        return tuple(clamp8(int(v)) for v in s)
    t = str(s).strip().lower().lstrip('#')
    if t in NAMED:
        return NAMED[t]
    if len(t) == 3:
        t = ''.join(c * 2 for c in t)
    if len(t) != 6:
        raise ColorError(
            f'kolor w formacie RRGGBB albo nazwa ({", ".join(sorted(NAMED))}), '
            f'dostalem: {s!r}')
    try:
        return int(t[0:2], 16), int(t[2:4], 16), int(t[4:6], 16)
    except ValueError:
        raise ColorError(f'to nie jest szesnastkowy kolor: {s!r}') from None


def to_hex(c):
    return '#{:02X}{:02X}{:02X}'.format(*c)


def clamp8(v):
    return 0 if v < 0 else (255 if v > 255 else int(v))


def lerp(a, b, t):
    if t <= 0:
        return a
    if t >= 1:
        return b
    return (int(a[0] + (b[0] - a[0]) * t),
            int(a[1] + (b[1] - a[1]) * t),
            int(a[2] + (b[2] - a[2]) * t))


def hsv(h, s=1.0, v=1.0):
    r, g, b = colorsys.hsv_to_rgb((h % 360.0) / 360.0, s, v)
    return int(r * 255), int(g * 255), int(b * 255)


def scale(c, f):
    return clamp8(c[0] * f), clamp8(c[1] * f), clamp8(c[2] * f)


# 256-elementowa tablica teczy — fala odpytuje ja co klatke dla 120 lampek,
# a colorsys w takiej petli to najdrozsza rzecz w calym silniku.
RAINBOW = [hsv(i * 360.0 / 256.0) for i in range(256)]
