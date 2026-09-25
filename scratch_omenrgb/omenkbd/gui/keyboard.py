"""Klikalny uklad klawiatury rysowany z RZECZYWISTYCH wspolrzednych z firmware'u.

To jest sedno calego GUI: nie ma tu zadnej grafiki ani hardkodowanego ukladu.
Pozycje, rozmiary i podpisy biora sie z Reportu 3, wiec widok dziala tak samo na
wariancie ISO, ANSI i bez bloku numerycznego — bez dorabiania obrazkow.

Lampki tego samego klawisza (Spacja ma 5, LShift 3, Enter 2) laczymy w jeden
prostokat, zeby klikalo sie w klawisze, a nie w diody. Tryb "per dioda" zostaje
dostepny dla tych, ktorzy chca malowac pojedyncze punkty.
"""

import statistics

from PySide6.QtCore import QRectF, Qt, Signal
from PySide6.QtGui import (QBrush, QColor, QFont, QFontMetrics, QPainter,
                           QPen)
from PySide6.QtWidgets import QGraphicsItem, QGraphicsScene, QGraphicsView

# Odstep miedzy klawiszami w mikrometrach — dobrany tak, zeby siatka wygladala
# jak klawiatura, a nie jak plansza do gry.
GAP_UM = 1600.0

# Skroty dla nazw, ktore nie mieszcza sie na waskim klawiszu. Lepiej pokazac
# czytelne „Caps" niz ucietego „CapsLoc". Strzalki jako znaki to najwiekszy zysk:
# jeden glif zamiast pieciu liter.
SHORT_LABELS = {
    'Backspace': 'Bksp', 'CapsLock': 'Caps', 'NumLock': 'Num', 'Delete': 'Del',
    'Insert': 'Ins', 'PrtSc': 'Prt', 'ScrLk': 'Scr', 'Pause': 'Pau',
    'LShift': 'Shift', 'RShift': 'Shift', 'LCtrl': 'Ctrl', 'RCtrl': 'Ctrl',
    'LAlt': 'Alt', 'RAlt': 'Alt', 'LMeta': 'Meta', 'RMeta': 'Meta',
    'Left': '\u2190', 'Up': '\u2191', 'Right': '\u2192', 'Down': '\u2193',
    'Enter': '\u21b5', 'KPEnter': '\u21b5', 'Menu': '\u2261', 'Omen': 'OM',
}


# Kolejnosc w pionie, gdy kilka lampek dzieli jedna pozycje. Firmware nie mowi,
# ktora jest wyzej, ale dla par gora/dol konwencja jest jednoznaczna.
_STACK_TOP = {'Up': 0, 'PgUp': 0, 'Home': 0, 'Insert': 0}
_STACK_BOTTOM = {'Down': 2, 'PgDn': 2, 'End': 2, 'Delete': 2}


def _stack_key(lamp):
    key = lamp.get('key')
    return (_STACK_TOP.get(key, _STACK_BOTTOM.get(key, 1)), lamp['id'])


class KeyItem(QGraphicsItem):
    """Jeden klawisz — prostokat obejmujacy wszystkie jego lampki — albo gola
    dioda bez przypisanego klawisza (InputBinding 0x00/0x03), rysowana mniejsza,
    zeby bylo widac, ze to lampka w przerwie miedzy blokami, a nie klawisz."""

    def __init__(self, lamp_ids, label, rect_um, scale, is_key=True):
        super().__init__()
        self.lamp_ids = list(lamp_ids)
        self.label = label or ''
        self.is_key = is_key
        r = QRectF(rect_um.x() * scale, rect_um.y() * scale,
                   rect_um.width() * scale, rect_um.height() * scale)
        if not is_key:
            # gola dioda: maly kwadrat wysrodkowany w komorce
            side = min(r.width(), r.height()) * 0.42
            r = QRectF(r.center().x() - side / 2, r.center().y() - side / 2,
                       side, side)
        self._rect = r
        self.color = QColor(20, 20, 24)
        self.selected = False
        self.hovered = False
        self.setAcceptHoverEvents(True)
        self.setFlag(QGraphicsItem.ItemIsSelectable, False)

    def boundingRect(self):
        return self._rect.adjusted(-2, -2, 2, 2)

    def hoverEnterEvent(self, e):
        self.hovered = True
        self.update()

    def hoverLeaveEvent(self, e):
        self.hovered = False
        self.update()

    def paint(self, p, option, widget=None):
        p.setRenderHint(QPainter.Antialiasing, True)
        r = self._rect
        radius = min(5.0, r.height() * (0.5 if not self.is_key else 0.18))

        p.setBrush(QBrush(self.color))
        if self.selected:
            p.setPen(QPen(QColor(255, 255, 255), 2.0))
        elif self.hovered:
            p.setPen(QPen(QColor(180, 180, 190), 1.5))
        else:
            p.setPen(QPen(QColor(70, 70, 80), 0.8))
        p.drawRoundedRect(r, radius, radius)

        if not self.label or not self.is_key or r.width() < 12:
            return
        # Podpis w kolorze kontrastujacym z wypelnieniem — inaczej bialy tekst
        # znika na bialym klawiszu.
        lum = (self.color.red() * 299 + self.color.green() * 587
               + self.color.blue() * 114) / 1000.0
        p.setPen(QColor(0, 0, 0) if lum > 140 else QColor(235, 235, 240))

        text, size = self._fit(r.width() - 3, r.height())
        f = QFont()
        f.setPixelSize(size)
        p.setFont(f)
        p.drawText(r, Qt.AlignCenter, text)

    def _fit(self, avail, height):
        """Dobiera napis i rozmiar czcionki do szerokosci klawisza.

        Kolejnosc prob na kazdym rozmiarze: pelna nazwa, potem skrot. Dzieki temu
        skrot w czytelnym rozmiarze wygrywa z pelna nazwa w nieczytelnym, a do
        obcinania tekstu dochodzi dopiero, gdy nie pomoglo ani jedno, ani drugie.
        """
        base = max(5, min(10, int(height * 0.44)))
        candidates = [self.label]
        short = SHORT_LABELS.get(self.label)
        if short and short != self.label:
            candidates.append(short)
        f = QFont()
        for size in range(base, 3, -1):
            f.setPixelSize(size)
            fm = QFontMetrics(f)
            for cand in candidates:
                if fm.horizontalAdvance(cand) <= avail:
                    return cand, size
        # nic nie weszlo — obcinamy najkrotszego kandydata w najmniejszym rozmiarze
        f.setPixelSize(4)
        fm = QFontMetrics(f)
        text = min(candidates, key=len)
        while text and fm.horizontalAdvance(text) > avail:
            text = text[:-1]
        return text, 4


class KeyboardView(QGraphicsView):
    """Widok calej klawiatury. Emituje selectionChanged z lista lamp id."""

    selectionChanged = Signal(list)

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setScene(QGraphicsScene(self))
        self.setRenderHint(QPainter.Antialiasing, True)
        self.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.setVerticalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.setFrameShape(QGraphicsView.NoFrame)
        self.setBackgroundBrush(QColor(28, 28, 32))
        self.setMinimumHeight(150)
        self.items_by_lamp = {}
        self.keys = []
        self._lamps = []
        self._scene_rect = QRectF()

    # ---------- budowa ----------

    def set_layout(self, lamps, attrs, group_by_key=True):
        """lamps: lista dictow z komendy 'layout' demona."""
        self._lamps = lamps
        scene = self.scene()
        scene.clear()
        self.items_by_lamp.clear()
        self.keys = []
        if not lamps:
            return

        cells = self._cells(lamps)
        groups = self._group(lamps, group_by_key)
        scale = 1.0 / 1000.0          # mikrometry -> jednostki sceny (1 = 1 mm)

        for label, ids in groups:
            rects = [cells[i] for i in ids]
            x0 = min(r.left() for r in rects)
            x1 = max(r.right() for r in rects)
            y0 = min(r.top() for r in rects)
            y1 = max(r.bottom() for r in rects)
            rect = QRectF(x0 + GAP_UM / 2, y0 + GAP_UM / 2,
                          max(1000.0, x1 - x0 - GAP_UM),
                          max(1000.0, y1 - y0 - GAP_UM))
            item = KeyItem(ids, label, rect, scale, is_key=label is not None)
            scene.addItem(item)
            self.keys.append(item)
            for i in ids:
                self.items_by_lamp[i] = item

        self._scene_rect = scene.itemsBoundingRect().adjusted(-4, -4, 4, 4)
        scene.setSceneRect(self._scene_rect)
        self.fit()

    @staticmethod
    def _cells(lamps):
        """Prostokat przypadajacy na kazda lampke, w mikrometrach.

        Rozstaw MIERZYMY, nie zakladamy — ale mierzymy go w obrebie rzedu.
        Naiwne "najmniejszy odstep w calej tablicy" nie dziala: rzedy sa wzgledem
        siebie poprzesuwane, wiec unia wszystkich X ma przypadkowe odstepy rzedu
        1000 um przy prawdziwym rozstawie 18000 um, i wszystkie klawisze wychodza
        jako paski. Bierzemy MEDIANE odstepow wewnatrz rzedow — odporna zarowno
        na te przypadkowe zblizenia, jak i na szerokie przerwy miedzy blokami.

        Komorka siega w polowie drogi do sasiada, ale nie dalej niz pol rozstawu.
        Dzieki temu klawisze sie nie nakladaja, a prawdziwe przerwy miedzy
        blokami (np. przed strzalkami) zostaja widoczne jako przerwy.
        """
        rows = {}
        for l in lamps:
            rows.setdefault(l['y_um'], []).append(l)
        ys = sorted(rows)

        # Rozstaw liczymy miedzy UNIKALNYMI pozycjami x — inaczej lampki lezace
        # pod tym samym x daja odstep 0 i psuja mediane.
        gaps = []
        for y in ys:
            xs = sorted({l['x_um'] for l in rows[y]})
            gaps += [b - a for a, b in zip(xs, xs[1:])]
        pitch = statistics.median(gaps) if gaps else 18000.0

        row_gaps = [b - a for a, b in zip(ys, ys[1:]) if b > a]
        row_pitch = statistics.median(row_gaps) if row_gaps else pitch

        half = pitch / 2.0
        half_row = row_pitch / 2.0
        cells = {}
        for ri, y in enumerate(ys):
            # Wysokosc komorki tak samo jak szerokosc: w polowie drogi do
            # sasiedniego rzedu, nie dalej niz pol medianowego rozstawu. Sama
            # mediana nie wystarczy — rzedy nie sa rownoodlegle (miedzy pierwszym
            # a drugim jest 16000 um przy medianie 18000) i komorki by na siebie
            # zachodzily.
            gt = y - ys[ri - 1] if ri else 0
            gb = ys[ri + 1] - y if ri + 1 < len(ys) else 0
            top = half_row if gt <= 0 else min(gt / 2.0, half_row)
            bot = half_row if gb <= 0 else min(gb / 2.0, half_row)
            by_x = {}
            for l in rows[y]:
                by_x.setdefault(l['x_um'], []).append(l)
            xs = sorted(by_x)
            for i, x in enumerate(xs):
                gl = x - xs[i - 1] if i else 0
                gr = xs[i + 1] - x if i + 1 < len(xs) else 0
                left = half if gl <= 0 else min(gl / 2.0, half)
                right = half if gr <= 0 else min(gr / 2.0, half)
                cell = QRectF(x - left, y - top, left + right, top + bot)

                # Firmware potrafi zwrocic kilka lampek pod TA SAMA wspolrzedna:
                # na tej klawiaturze Up i Down (id 114 i 113) siedza oba
                # w (237000, 99000), bo fizycznie sa polwysokosciowym stosem,
                # a LampArray zna tylko jeden punkt. Bez podzialu komorki jedna
                # rysowalaby sie na drugiej i stawala sie nieklikalna.
                stack = sorted(by_x[x], key=_stack_key)
                h = cell.height() / len(stack)
                for j, l in enumerate(stack):
                    cells[l['id']] = QRectF(cell.x(), cell.y() + j * h,
                                            cell.width(), h)
        return cells

    def _lamp(self, lamp_id):
        return next(l for l in self._lamps if l['id'] == lamp_id)

    @staticmethod
    def _group(lamps, group_by_key):
        """[(podpis, [lamp_id, ...])]. Laczymy tylko lampki tego samego klawisza
        lezace obok siebie — inaczej lewy i prawy Shift zlalyby sie w jeden
        prostokat przez cala klawiature."""
        if not group_by_key:
            return [(str(l['id']), [l['id']]) for l in lamps]
        out = []
        run_key, run = None, []
        for l in sorted(lamps, key=lambda x: (x['y_um'], x['x_um'], x['id'])):
            key = (l['key'], l['y_um'])
            if key != run_key or l['key'] is None:
                if run:
                    out.append((run_key[0], run))
                run_key, run = key, [l['id']]
            else:
                run.append(l['id'])
        if run:
            out.append((run_key[0], run))
        return out

    # ---------- kolory ----------

    def set_colors(self, colors_by_lamp, default=(0, 0, 0)):
        """colors_by_lamp: {lamp_id: (r, g, b)}. Klawisz z wielu lampek dostaje
        kolor pierwszej — w praktyce sa identyczne."""
        for item in self.keys:
            c = colors_by_lamp.get(item.lamp_ids[0], default)
            item.color = QColor(*c)
            item.update()

    # ---------- zaznaczanie ----------

    def selected_lamps(self):
        out = []
        for item in self.keys:
            if item.selected:
                out.extend(item.lamp_ids)
        return out

    def clear_selection(self):
        for item in self.keys:
            if item.selected:
                item.selected = False
                item.update()
        self.selectionChanged.emit([])

    def select_all(self):
        for item in self.keys:
            item.selected = True
            item.update()
        self.selectionChanged.emit(self.selected_lamps())

    def mousePressEvent(self, e):
        item = self._key_at(e.position())
        if item is None:
            if not (e.modifiers() & Qt.ControlModifier):
                self.clear_selection()
            return
        if e.modifiers() & Qt.ControlModifier:
            item.selected = not item.selected
        else:
            for other in self.keys:
                if other is not item and other.selected:
                    other.selected = False
                    other.update()
            item.selected = True
        item.update()
        self.selectionChanged.emit(self.selected_lamps())

    def mouseMoveEvent(self, e):
        # Przeciaganie z wcisnietym lewym = malowanie zaznaczenia po kilku klawiszach
        if not (e.buttons() & Qt.LeftButton):
            return super().mouseMoveEvent(e)
        item = self._key_at(e.position())
        if item is not None and not item.selected:
            item.selected = True
            item.update()
            self.selectionChanged.emit(self.selected_lamps())

    def _key_at(self, pos):
        for it in self.items(pos.toPoint()):
            if isinstance(it, KeyItem):
                return it
        return None

    # ---------- skalowanie ----------

    def fit(self):
        if not self._scene_rect.isEmpty():
            self.fitInView(self._scene_rect, Qt.KeepAspectRatio)

    def resizeEvent(self, e):
        super().resizeEvent(e)
        self.fit()

    def sizeHint(self):
        from PySide6.QtCore import QSize
        return QSize(760, 260)
