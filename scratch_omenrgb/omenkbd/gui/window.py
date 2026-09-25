"""Okno glowne.

Podglad na canvasie liczymy LOKALNIE tym samym modulem efektow, ktorego uzywa
demon — jest calkowicie niezalezny od sprzetu, wiec animacja w oknie odpowiada
temu, co widac na klawiaturze, bez odpytywania demona 30 razy na sekunde.
Timer podgladu chodzi tylko, gdy okno jest widoczne i efekt jest animowany.
"""

import time

from PySide6.QtCore import Qt, QTimer, Signal
from PySide6.QtWidgets import (QButtonGroup, QCheckBox, QComboBox, QGroupBox,
                               QHBoxLayout, QInputDialog, QLabel, QMainWindow,
                               QMessageBox, QPushButton, QSlider,
                               QStackedWidget, QVBoxLayout, QWidget)

from .. import i18n
from ..client import Client, DaemonError, NoDaemon
from ..engine import effects as effects_mod
from ..engine import reactive as reactive_mod
from ..engine.frame import Frame
from .keyboard import KeyboardView
from .panels import EffectPanel, PerKeyPanel, make_panel

PREVIEW_FPS = 25


class PreviewLayout:
    """Minimalny odpowiednik core.layout.Layout zbudowany z odpowiedzi demona.

    Silnik efektow potrzebuje tylko ids/nx/ny/count, wiec nie ciagniemy tu calej
    klasy — GUI nie ma dostepu do urzadzenia i nie powinno go potrzebowac.
    """

    def __init__(self, lamps, attrs):
        lamps = sorted(lamps, key=lambda l: l['id'])
        w = attrs.get('width_um') or 1
        h = attrs.get('height_um') or 1
        self.ids = [l['id'] for l in lamps]
        self.nx = [l['x_um'] / w for l in lamps]
        self.ny = [l['y_um'] / h for l in lamps]
        self.count = len(lamps)
        self.contiguous = self.ids == list(range(self.count))
        self.attrs = attrs


class MainWindow(QMainWindow):
    stateChanged = Signal()          # tray odswieza sie po tym sygnale

    def __init__(self, client=None):
        super().__init__()
        self.client = client or Client()
        self.layout_model = None
        self.frame = None
        self.effect = None
        self.t0 = time.monotonic()
        self._loading = False

        # Timery MUSZA powstac przed _build() i przed setWindowTitle():
        # Qt potrafi wywolac changeEvent() juz w trakcie konstrukcji okna,
        # a ten siega do preview_timer.
        self.preview_timer = QTimer(self)
        self.preview_timer.timeout.connect(self._tick)
        self.status_timer = QTimer(self)
        self.status_timer.timeout.connect(self._poll_status)

        self.setWindowTitle(i18n.t('window.title'))
        self._build()

        self.status_timer.start(4000)
        self.refresh(initial=True)

    # ---------- budowa ----------

    def _build(self):
        central = QWidget()
        root = QVBoxLayout(central)
        root.setContentsMargins(12, 12, 12, 12)
        root.setSpacing(10)

        self.view = KeyboardView()
        self.view.selectionChanged.connect(self._on_selection)
        root.addWidget(self.view, 1)

        # --- efekt + parametry ---
        row = QHBoxLayout()
        row.addWidget(QLabel(i18n.t('window.mode')))
        self.effect_combo = QComboBox()
        self.effect_combo.setMinimumWidth(160)
        catalogue = effects_mod.catalogue()
        for spec in catalogue:
            self.effect_combo.addItem(i18n.effect_label(spec['name'], spec['label']),
                                      spec['name'])
        self.effect_combo.currentIndexChanged.connect(self._on_effect_changed)
        row.addWidget(self.effect_combo)
        row.addStretch(1)
        row.addWidget(QLabel(i18n.t('window.control')))
        row.addLayout(self._build_control_switch())
        root.addLayout(row)

        root.addWidget(self._build_reactive_box())

        self.stack = QStackedWidget()
        self.panels = {}
        for spec in catalogue:
            panel = make_panel(spec)
            panel.changed.connect(self._on_params_changed)
            if isinstance(panel, PerKeyPanel):
                panel.applyRequested.connect(self._paint_selected)
            self.panels[spec['name']] = panel
            self.stack.addWidget(panel)
        root.addWidget(self.stack)

        # --- jasnosc ---
        row = QHBoxLayout()
        row.addWidget(QLabel(i18n.t('window.brightness')))
        self.bright = QSlider(Qt.Horizontal)
        self.bright.setRange(0, 255)
        self.bright.setValue(200)
        self.bright.valueChanged.connect(self._on_brightness)
        row.addWidget(self.bright, 1)
        self.bright_lbl = QLabel('200')
        self.bright_lbl.setMinimumWidth(30)
        self.bright_lbl.setAlignment(Qt.AlignRight | Qt.AlignVCenter)
        row.addWidget(self.bright_lbl)
        root.addLayout(row)

        # --- profile ---
        row = QHBoxLayout()
        row.addWidget(QLabel(i18n.t('window.profile')))
        self.profile_combo = QComboBox()
        self.profile_combo.setMinimumWidth(140)
        self.profile_combo.activated.connect(self._load_profile)
        row.addWidget(self.profile_combo)
        b = QPushButton(i18n.t('window.save_as'))
        b.clicked.connect(self._save_profile)
        row.addWidget(b)
        self.delete_btn = QPushButton(i18n.t('window.delete'))
        self.delete_btn.clicked.connect(self._delete_profile)
        row.addWidget(self.delete_btn)
        row.addStretch(1)
        self.status_lbl = QLabel()
        self.status_lbl.setStyleSheet('color: palette(mid);')
        row.addWidget(self.status_lbl)
        root.addLayout(row)

        self.setCentralWidget(central)
        self.resize(820, 520)

    def _build_reactive_box(self):
        """Nakladka reaktywna — ORTOGONALNA do wybranego trybu, wiec zyje
        poza QStackedWidget efektow, nie jako jedna z jego stron."""
        box = QGroupBox(i18n.t('window.reactive_box'))
        box.setCheckable(True)
        box.setChecked(False)
        box.toggled.connect(self._on_reactive_toggled)

        spec = {'name': 'reactive', 'params': [p._asdict() for p in reactive_mod.PARAMS]}
        self.reactive_panel = EffectPanel(spec)
        self.reactive_panel.changed.connect(self._on_reactive_params_changed)

        self.reactive_hint = QLabel()
        self.reactive_hint.setWordWrap(True)
        self.reactive_hint.setStyleSheet('color: palette(link-visited);')
        self.reactive_hint.hide()

        lay = QVBoxLayout(box)
        lay.addWidget(self.reactive_panel)
        lay.addWidget(self.reactive_hint)
        self.reactive_box = box
        return box

    def _on_reactive_toggled(self, on):
        if self._loading:
            return
        try:
            self.client.call('reactive', enable=on)
        except (NoDaemon, DaemonError) as e:
            self._set_status(str(e), error=True)
            return
        self.stateChanged.emit()
        self._refresh_reactive_hint()

    def _on_reactive_params_changed(self):
        if self._loading:
            return
        # EffectPanel.params() zawsze dokleja 'effect' (nazwe panelu) — sensowne
        # dla trybow swiecenia, ale ReactiveOverlay tego klucza nie zna.
        params = self.reactive_panel.params()
        params.pop('effect', None)
        try:
            self.client.call('reactive', params=params)
        except (NoDaemon, DaemonError) as e:
            self._set_status(str(e), error=True)
            return
        self.stateChanged.emit()

    def _refresh_reactive_hint(self):
        st = self.client.try_call('status')
        rx = (st or {}).get('reactive') or {}
        if rx.get('enabled') and not rx.get('available'):
            self.reactive_hint.setText(i18n.t('window.reactive_no_access'))
            self.reactive_hint.show()
        else:
            self.reactive_hint.hide()

    def _build_control_switch(self):
        """Kto steruje podswietleniem: firmware klawiatury czy ta aplikacja.

        Swiadomie przelacznik, a nie pojedynczy przycisk „oddaj": to jest stan,
        nie czynnosc, i uzytkownik ma widziec, w ktorym jest teraz.
        """
        box = QHBoxLayout()
        box.setSpacing(0)
        self.control_group = QButtonGroup(self)
        self.control_group.setExclusive(True)
        self.btn_bios = QPushButton(i18n.t('window.control_bios'))
        self.btn_bios.setToolTip(i18n.t('window.control_bios_tip'))
        self.btn_app = QPushButton(i18n.t('window.control_app'))
        self.btn_app.setToolTip(i18n.t('window.control_app_tip'))
        for b in (self.btn_bios, self.btn_app):
            b.setCheckable(True)
            b.setMinimumWidth(92)
            self.control_group.addButton(b)
            box.addWidget(b)
        self.btn_app.setChecked(True)
        self.btn_bios.clicked.connect(lambda: self._set_control('bios'))
        self.btn_app.clicked.connect(lambda: self._set_control('app'))
        return box

    def _show_control(self, owner):
        """Odzwierciedla stan z demona bez wywolywania akcji."""
        self.control_group.blockSignals(True)
        self.btn_bios.setChecked(owner == 'bios')
        self.btn_app.setChecked(owner != 'bios')
        self.control_group.blockSignals(False)
        # Przy sterowaniu z BIOS-u ustawienia trybu nic nie zmieniaja — lepiej
        # je wygasic, niz pozwolic klikac w kontrolki bez efektu.
        self.stack.setDisabled(owner == 'bios')
        self.effect_combo.setDisabled(owner == 'bios')

    def _set_control(self, owner):
        if self._loading:
            return
        try:
            self.client.call('control', owner=owner)
        except (NoDaemon, DaemonError) as e:
            self._set_status(str(e), error=True)
            return
        self._show_control(owner)
        self._set_status(i18n.t('window.control_bios_status') if owner == 'bios'
                         else i18n.t('window.control_app_status'))
        self.stateChanged.emit()

    # ---------- rozmowa z demonem ----------

    def refresh(self, initial=False):
        """Pobiera uklad i stan z demona i ustawia pod to cale GUI."""
        try:
            st = self.client.call('status')
        except (NoDaemon, DaemonError) as e:
            self._set_status(i18n.t('window.daemon_unreachable', err=e), error=True)
            if initial:
                self._disable_all(True)
            return

        self._disable_all(not st.get('connected'))
        if not st.get('connected'):
            self._set_status(i18n.t('window.disconnected'), error=True)
        else:
            dev = (st.get('device') or {}).get('node', '')
            self._set_status(i18n.t('window.lamps_at', n=st['lamp_count'], dev=dev))

        if self.layout_model is None and st.get('connected'):
            lay = self.client.try_call('layout')
            if lay:
                self.layout_model = PreviewLayout(lay['lamps'], lay['attrs'])
                self.frame = Frame(self.layout_model.count)
                self.view.set_layout(lay['lamps'], lay['attrs'])

        self._loading = True
        try:
            self.bright.setValue(int(st.get('brightness', 200)))
            params = st.get('effect') or {'effect': 'static'}
            name = params.get('effect', 'static')
            if name in self.panels:
                self.panels[name].load(params)
                idx = self.effect_combo.findData(name)
                if idx >= 0:
                    self.effect_combo.setCurrentIndex(idx)
                    self.stack.setCurrentWidget(self.panels[name])
            self._reload_profiles(st.get('profile'))
            self._show_control(st.get('control', 'app'))

            rx = st.get('reactive') or {}
            self.reactive_box.setChecked(bool(rx.get('enabled')))
            if rx.get('params'):
                self.reactive_panel.load(rx['params'])
        finally:
            self._loading = False
        self._refresh_reactive_hint()

        self._rebuild_effect()

    def _poll_status(self):
        """Lekkie odswiezanie: wykrywa odlaczenie/podlaczenie klawiatury
        i zmiany zrobione z CLI w innym oknie."""
        st = self.client.try_call('status')
        if st is None:
            self._set_status(i18n.t('tray.daemon_down'), error=True)
            self._disable_all(True)
            return
        if self.layout_model is None and st.get('connected'):
            self.refresh()
            return
        self._disable_all(not st.get('connected'))

    def _push(self, params=None, brightness=None):
        """Wysyla biezacy efekt do demona."""
        if self._loading:
            return
        params = params or self._current_panel().params()
        try:
            self.client.call('set', params=params, brightness=brightness)
        except (NoDaemon, DaemonError) as e:
            self._set_status(str(e), error=True)
            return
        # Ustawienie trybu z definicji oznacza sterowanie z aplikacji.
        self._show_control('app')
        self.stateChanged.emit()

    # ---------- reakcje na kontrolki ----------

    def _current_panel(self):
        return self.stack.currentWidget()

    def _on_effect_changed(self, _index):
        name = self.effect_combo.currentData()
        self.stack.setCurrentWidget(self.panels[name])
        if self._loading:
            return
        self._rebuild_effect()
        self._push()

    def _on_params_changed(self):
        if self._loading:
            return
        self._rebuild_effect()
        self._push()

    def _on_brightness(self, value):
        self.bright_lbl.setText(str(value))
        if self._loading:
            return
        try:
            self.client.call('brightness', value=value)
        except (NoDaemon, DaemonError) as e:
            self._set_status(str(e), error=True)
            return
        self.stateChanged.emit()

    def _on_selection(self, lamp_ids):
        panel = self._current_panel()
        if isinstance(panel, PerKeyPanel):
            n = len({self.view.items_by_lamp[i] for i in lamp_ids
                     if i in self.view.items_by_lamp})
            panel.apply_btn.setEnabled(bool(lamp_ids))
            panel.apply_btn.setText(
                i18n.t('window.paint_selected_n', n=n) if n
                else i18n.t('window.paint_selected'))

    def _paint_selected(self, hex_color):
        panel = self._current_panel()
        if isinstance(panel, PerKeyPanel):
            panel.paint(self.view.selected_lamps(), hex_color)

    # ---------- profile ----------

    def _reload_profiles(self, current=None):
        r = self.client.try_call('profile.list')
        names = (r or {}).get('profiles', [])
        cur = current if current is not None else (r or {}).get('current')
        self.profile_combo.blockSignals(True)
        self.profile_combo.clear()
        self.profile_combo.addItem(i18n.t('window.profile_none'), None)
        for n in names:
            self.profile_combo.addItem(n, n)
        idx = self.profile_combo.findData(cur)
        self.profile_combo.setCurrentIndex(idx if idx >= 0 else 0)
        self.profile_combo.blockSignals(False)
        self.delete_btn.setEnabled(bool(names))

    def _load_profile(self, _index):
        name = self.profile_combo.currentData()
        if not name:
            return
        try:
            self.client.call('profile.load', name=name)
        except (NoDaemon, DaemonError) as e:
            self._set_status(str(e), error=True)
            return
        self.refresh()
        self.stateChanged.emit()

    def _save_profile(self):
        name, ok = QInputDialog.getText(self, i18n.t('window.save_profile_title'),
                                        i18n.t('window.save_profile_prompt'))
        name = name.strip()
        if not ok or not name:
            return
        try:
            self.client.call('profile.save', name=name)
        except DaemonError as e:
            QMessageBox.warning(self, i18n.t('window.save_profile_failed_title'), str(e))
            return
        except NoDaemon as e:
            self._set_status(str(e), error=True)
            return
        self._reload_profiles(name)
        self._set_status(i18n.t('window.profile_saved', name=name))
        self.stateChanged.emit()

    def _delete_profile(self):
        name = self.profile_combo.currentData()
        if not name:
            return
        if QMessageBox.question(
                self, i18n.t('window.delete_profile_title'),
                i18n.t('window.delete_profile_confirm', name=name)) != QMessageBox.Yes:
            return
        try:
            self.client.call('profile.delete', name=name)
        except (NoDaemon, DaemonError) as e:
            self._set_status(str(e), error=True)
            return
        self._reload_profiles(None)
        self.stateChanged.emit()

    # ---------- podglad ----------

    def _rebuild_effect(self):
        if self.layout_model is None:
            return
        try:
            self.effect = effects_mod.from_dict(self._current_panel().params())
        except Exception:
            self.effect = None
            return
        self.t0 = time.monotonic()
        self._render_preview()
        self._sync_timer()

    def _sync_timer(self):
        want = (self.effect is not None and self.effect.animated
                and self.isVisible() and not self.isMinimized())
        if want and not self.preview_timer.isActive():
            self.preview_timer.start(int(1000 / PREVIEW_FPS))
        elif not want and self.preview_timer.isActive():
            self.preview_timer.stop()

    def _tick(self):
        self._render_preview()

    def _render_preview(self):
        if self.effect is None or self.frame is None:
            return
        self.effect.render(time.monotonic() - self.t0, self.layout_model,
                           self.frame)
        # Jasnosc w podgladzie jako zwykle przyciemnienie: na sprzecie to osobny
        # kanal Intensity, ale wizualnie efekt jest ten sam.
        f = max(0.06, self.bright.value() / 255.0)
        colors = {lid: (int(c[0] * f), int(c[1] * f), int(c[2] * f))
                  for lid, c in zip(self.layout_model.ids, self.frame.rgb)}
        self.view.set_colors(colors)

    # ---------- pomocnicze ----------

    def _set_status(self, text, error=False):
        self.status_lbl.setText(text)
        self.status_lbl.setStyleSheet(
            'color: palette(link-visited);' if error else 'color: palette(mid);')

    def _disable_all(self, disabled):
        for w in (self.effect_combo, self.stack, self.bright,
                  self.profile_combo, self.btn_bios, self.btn_app):
            w.setDisabled(disabled)

    # ---------- jezyk ----------

    def retranslate(self):
        """Przebudowuje UI po zmianie jezyka. _build() tworzy nowe widgety
        z aktualnymi tlumaczeniami (etykiety byly ustawione RAZ w i18n.t()
        podczas konstrukcji — nie sledza zmiany jezyka same z siebie), w tym
        NOWY, pusty KeyboardView (self.view). refresh() odtwarza uklad
        klawiatury tylko gdy self.layout_model is None — inaczej zaklada, ze
        biezacy self.view juz go ma. Po _build() to zalozenie jest falszywe:
        stary self.layout_model wskazuje na dane, ktorych nowy widok nigdy nie
        dostal, wiec podglad zostawal pusty. Kasujemy oba, zeby refresh()
        pobral geometrie z demona jeszcze raz i nalozyl ja na nowy widok."""
        self._build()
        self.layout_model = None
        self.frame = None
        self.refresh()

    # ---------- zdarzenia okna ----------

    def showEvent(self, e):
        super().showEvent(e)
        if hasattr(self, 'status_lbl'):
            self.refresh()
        self._sync_timer()

    def hideEvent(self, e):
        super().hideEvent(e)
        self.preview_timer.stop()      # niewidoczne okno nie liczy animacji

    def changeEvent(self, e):
        super().changeEvent(e)
        self._sync_timer()

    def closeEvent(self, e):
        """Zamkniecie chowa do traya — demon i tak swieci dalej."""
        if getattr(self, 'quitting', False):
            return super().closeEvent(e)
        e.ignore()
        self.hide()
