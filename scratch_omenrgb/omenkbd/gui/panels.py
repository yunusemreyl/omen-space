"""Kontrolki parametrow efektu — budowane z deklaracji PARAMS w silniku.

Zamiast klasy panelu na kazdy efekt jest jeden panel czytajacy specyfikacje.
Dzieki temu silnik jest jedynym zrodlem prawdy o parametrach: dodanie efektu
w effects.py automatycznie daje mu komplet kontrolek, bez ryzyka, ze GUI i demon
rozjada sie co do zakresow albo wartosci domyslnych.
"""

from PySide6.QtCore import Qt, Signal
from PySide6.QtGui import QColor
from PySide6.QtWidgets import (QColorDialog, QComboBox, QFormLayout, QHBoxLayout,
                               QLabel, QPushButton, QSlider, QWidget)

from .. import i18n

AXES = ['x', 'y', 'd']


class ColorButton(QPushButton):
    """Przycisk pokazujacy kolor; klikniecie otwiera systemowy dialog wyboru."""

    colorChanged = Signal(str)

    def __init__(self, hex_color='#FFFFFF', parent=None):
        super().__init__(parent)
        self.setFixedSize(70, 26)
        self._color = QColor(hex_color)
        self._refresh()
        self.clicked.connect(self._pick)

    def color_hex(self):
        return self._color.name().upper()

    def set_color_hex(self, hex_color):
        c = QColor(hex_color)
        if c.isValid() and c != self._color:
            self._color = c
            self._refresh()

    def _refresh(self):
        lum = (self._color.red() * 299 + self._color.green() * 587
               + self._color.blue() * 114) / 1000.0
        fg = '#000000' if lum > 140 else '#FFFFFF'
        self.setStyleSheet(
            f'QPushButton {{ background: {self._color.name()}; color: {fg};'
            f' border: 1px solid palette(mid); border-radius: 4px; }}')
        self.setText(self._color.name().upper()[1:])

    def _pick(self):
        c = QColorDialog.getColor(self._color, self, i18n.t('window.pick_color'))
        if c.isValid():
            self._color = c
            self._refresh()
            self.colorChanged.emit(self.color_hex())


class _FloatSlider(QWidget):
    """Suwak na wartosci ulamkowe. Qt zna tylko liczby calkowite, wiec skalujemy
    przez krok zadeklarowany w PARAMS."""

    valueChanged = Signal()

    def __init__(self, spec):
        super().__init__()
        self.spec = spec
        self.step = spec['step'] or 0.01
        self.slider = QSlider(Qt.Horizontal)
        self.slider.setRange(round(spec['lo'] / self.step),
                             round(spec['hi'] / self.step))
        self.label = QLabel()
        self.label.setMinimumWidth(44)
        self.label.setAlignment(Qt.AlignRight | Qt.AlignVCenter)
        h = QHBoxLayout(self)
        h.setContentsMargins(0, 0, 0, 0)
        h.addWidget(self.slider, 1)
        h.addWidget(self.label)
        self.slider.valueChanged.connect(self._sync)
        self.slider.valueChanged.connect(self.valueChanged)
        self.set_value(spec['default'])

    def _sync(self):
        self.label.setText(f'{self.value():g}')

    def value(self):
        return round(self.slider.value() * self.step, 4)

    def set_value(self, v):
        self.slider.setValue(round(float(v) / self.step))
        self._sync()


class EffectPanel(QWidget):
    """Panel jednego efektu, zbudowany ze specyfikacji z effects.catalogue()."""

    changed = Signal()

    def __init__(self, spec):
        super().__init__()
        self.effect_name = spec['name']
        self.spec = spec
        self.widgets = {}
        self.form = QFormLayout(self)
        self.form.setContentsMargins(0, 0, 0, 0)
        for p in spec['params']:
            w = self._make(p)
            self.widgets[p['name']] = w
            label = i18n.param_label(self.effect_name, p['name'], p['label'])
            self.form.addRow(label, w)
        if not spec['params']:
            hint = QLabel(i18n.t('window.no_settings'))
            hint.setStyleSheet('color: palette(mid);')
            self.form.addRow(hint)

    def _make(self, p):
        kind = p['kind']
        if kind == 'color':
            w = ColorButton(p['default'])
            w.colorChanged.connect(self.changed)
            return w
        if kind in ('axis', 'choice'):
            w = QComboBox()
            if kind == 'axis':
                values = AXES
                items = [(i18n.axis_label(v), v) for v in values]
            else:
                items = [(i18n.choice_label(self.effect_name, p['name'], v), v)
                         for v in p['choices']]
            for label, data in items:
                w.addItem(label, data)
            i = w.findData(p['default'])
            w.setCurrentIndex(max(0, i))
            w.currentIndexChanged.connect(self.changed)
            return w
        w = _FloatSlider(p)
        w.valueChanged.connect(self.changed)
        return w

    def params(self):
        out = {'effect': self.effect_name}
        for name, w in self.widgets.items():
            if isinstance(w, ColorButton):
                out[name] = w.color_hex()
            elif isinstance(w, QComboBox):
                out[name] = w.currentData()
            else:
                out[name] = w.value()
        return out

    def load(self, params):
        """Ustawia kontrolki wg zapisanego stanu. Wolane przy zablokowanych
        sygnalach przez okno, wiec nie emituje changed."""
        for name, w in self.widgets.items():
            if name not in params:
                continue
            v = params[name]
            if isinstance(w, ColorButton):
                w.set_color_hex(v)
            elif isinstance(w, QComboBox):
                i = w.findData(v)
                if i >= 0:
                    w.setCurrentIndex(i)
            else:
                try:
                    w.set_value(v)
                except (TypeError, ValueError):
                    pass


class PerKeyPanel(EffectPanel):
    """Malowanie pojedynczych klawiszy. Kolory trzyma panel, bo zaznaczenie zyje
    na canvasie, a nie w modelu efektu."""

    applyRequested = Signal(str)

    def __init__(self, spec):
        super().__init__(spec)
        self.colors = {}
        self.brush = ColorButton('#FF2000')
        self.apply_btn = QPushButton(i18n.t('window.paint_selected'))
        self.apply_btn.setEnabled(False)
        self.clear_btn = QPushButton(i18n.t('window.clear_all'))
        self.apply_btn.clicked.connect(
            lambda: self.applyRequested.emit(self.brush.color_hex()))
        self.clear_btn.clicked.connect(self._clear)

        btns = QWidget()
        h = QHBoxLayout(btns)
        h.setContentsMargins(0, 0, 0, 0)
        h.addWidget(self.apply_btn)
        h.addWidget(self.clear_btn)
        h.addStretch(1)

        hint = QLabel(i18n.t('window.perkey_hint'))
        hint.setWordWrap(True)
        hint.setStyleSheet('color: palette(mid);')

        self.form.insertRow(0, i18n.t('window.brush_color'), self.brush)
        self.form.addRow('', btns)
        self.form.addRow('', hint)

    def paint(self, lamp_ids, hex_color):
        for i in lamp_ids:
            self.colors[int(i)] = hex_color
        self.changed.emit()

    def _clear(self):
        self.colors.clear()
        self.changed.emit()

    def params(self):
        p = super().params()
        p['colors'] = {str(k): v for k, v in self.colors.items()}
        return p

    def load(self, params):
        super().load(params)
        self.colors = {int(k): v for k, v in (params.get('colors') or {}).items()}


def make_panel(spec):
    cls = PerKeyPanel if spec['name'] == 'perkey' else EffectPanel
    return cls(spec)
