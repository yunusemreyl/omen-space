"""Ikona aplikacji — klawiaturka: ciemny kafelek z siatka kolorowych klawiszy.

Kafelek ma margines i jest wysrodkowany w kwadracie 64x64. To nie jest kosmetyka:
w trayu sasiaduje z kwadratowymi glifami, a ikona wypelniajaca caly kafelek ma
wieksza wage wizualna niz reszta i odstaje. Siatka klawiszy jest wysrodkowana
takze w pionie — trzy rzedy nie wypelniaja kwadratu, wiec bez tego caly rysunek
wisial przy gornej krawedzi.

Geometrie liczy _geometry(); korzystaja z niej i to_svg(), i sciezka zapasowa
rysowana QPainterem, wiec nie ma szans na rozjechanie sie obu.
"""

MARGIN = 9.0          # margines kafelka w jednostkach viewBoxa 64x64
INNER = 0.13          # wciecie siatki wewnatrz kafelka
GAP_FACTOR = 0.14     # odstep miedzy klawiszami w szerokosciach klawisza
KEY_HEIGHT = 0.86     # wysokosc klawisza w wysokosciach komorki
CORNER = 0.20         # zaokraglenie kafelka
COLS, ROWS = 4, 3

BG_FROM, BG_TO = '#1b1b22', '#30303c'

# (kolumna, rzad, szerokosc w jednostkach klawisza) — ostatni to spacja
KEYS = [(0, 0, 1), (1, 0, 1), (2, 0, 1), (3, 0, 1),
        (0, 1, 1), (1, 1, 1), (2, 1, 1), (3, 1, 1),
        (0, 2, 1), (1, 2, 3)]
COLORS = ['#FF2000', '#FF8000', '#FFD000', '#00FF88',
          '#00D0FF', '#00A8FF', '#5060FF', '#A000FF',
          '#FF0080', '#FFFFFF']


def _geometry(size=64.0):
    """(kafelek, promien_kafelka, [(x, y, w, h, kolor)], promien_klawisza)."""
    k = size / 64.0
    m = MARGIN * k
    s = size - 2 * m
    pad = s * INNER
    avail = s - 2 * pad

    unit_w = avail / (COLS + GAP_FACTOR * (COLS - 1))
    gap = unit_w * GAP_FACTOR
    unit_h = (avail - gap * (ROWS - 1)) / ROWS
    key_h = unit_h * KEY_HEIGHT

    grid_w = COLS * unit_w + (COLS - 1) * gap
    grid_h = ROWS * key_h + (ROWS - 1) * gap
    ox = m + (s - grid_w) / 2.0
    oy = m + (s - grid_h) / 2.0        # wysrodkowanie takze w pionie

    keys = []
    for (col, row, span), colour in zip(KEYS, COLORS):
        keys.append((ox + col * (unit_w + gap),
                     oy + row * (key_h + gap),
                     unit_w * span + gap * (span - 1),
                     key_h, colour))
    return (m, m, s, s), s * CORNER, keys, unit_w * 0.20


def to_svg():
    """SVG ikony. Bez Qt — instalator wola to, zeby zapisac plik do menu,
    i musi dzialac zanim PySide6 w ogole jest w systemie."""
    (tx, ty, tw, th), trad, keys, krad = _geometry()
    parts = [
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">',
        '<defs><linearGradient id="b" x1="0" y1="0" x2="1" y2="1">'
        f'<stop offset="0" stop-color="{BG_FROM}"/>'
        f'<stop offset="1" stop-color="{BG_TO}"/></linearGradient></defs>',
        f'<rect x="{tx:.2f}" y="{ty:.2f}" width="{tw:.2f}" height="{th:.2f}"'
        f' rx="{trad:.2f}" fill="url(#b)"/>',
    ]
    for x, y, w, h, colour in keys:
        parts.append(f'<rect x="{x:.2f}" y="{y:.2f}" width="{w:.2f}"'
                     f' height="{h:.2f}" rx="{krad:.2f}" fill="{colour}"/>')
    parts.append('</svg>')
    return '\n'.join(parts)


def app_icon():
    from PySide6.QtCore import QByteArray, Qt
    from PySide6.QtGui import QIcon, QPainter, QPixmap

    try:
        from PySide6.QtSvg import QSvgRenderer
    except ImportError:
        return _painted_icon()

    renderer = QSvgRenderer(QByteArray(to_svg().encode()))
    icon = QIcon()
    for size in (16, 22, 24, 32, 48, 64, 128, 256):
        pm = QPixmap(size, size)
        pm.fill(Qt.transparent)
        p = QPainter(pm)
        p.setRenderHint(QPainter.Antialiasing, True)
        renderer.render(p)
        p.end()
        icon.addPixmap(pm)
    return icon


def _painted_icon():
    """Sciezka zapasowa, gdyby w systemie nie bylo modulu QtSvg. Rysuje z tej
    samej geometrii co to_svg(), wiec nie trzeba jej pilnowac osobno."""
    from PySide6.QtCore import QRectF, Qt
    from PySide6.QtGui import (QBrush, QColor, QIcon, QLinearGradient, QPainter,
                               QPixmap)

    icon = QIcon()
    for size in (16, 22, 24, 32, 48, 64, 128, 256):
        (tx, ty, tw, th), trad, keys, krad = _geometry(float(size))
        pm = QPixmap(size, size)
        pm.fill(Qt.transparent)
        p = QPainter(pm)
        p.setRenderHint(QPainter.Antialiasing, True)
        p.setPen(Qt.NoPen)
        g = QLinearGradient(tx, ty, tx + tw, ty + th)
        g.setColorAt(0.0, QColor(BG_FROM))
        g.setColorAt(1.0, QColor(BG_TO))
        p.setBrush(QBrush(g))
        p.drawRoundedRect(QRectF(tx, ty, tw, th), trad, trad)
        for x, y, w, h, colour in keys:
            p.setBrush(QColor(colour))
            p.drawRoundedRect(QRectF(x, y, w, h), krad, krad)
        p.end()
        icon.addPixmap(pm)
    return icon


if __name__ == '__main__':
    print(to_svg())
