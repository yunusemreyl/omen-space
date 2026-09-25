"""Ikona w trayu: szybkie przelaczanie profili, presetow i jasnosci.

Menu budujemy na nowo przy kazdym otwarciu, bo profile moga sie zmienic z CLI
albo z drugiego okna. To tanie — kilkanascie pozycji.
"""

from PySide6.QtGui import QAction, QActionGroup
from PySide6.QtWidgets import QMenu, QSystemTrayIcon

from .. import i18n
from ..client import DaemonError, NoDaemon

QUICK_COLORS = [
    ('colors.white', '#FFFFFF'), ('colors.cyan', '#00A8FF'),
    ('colors.green', '#00FF88'), ('colors.amber', '#FF8000'),
    ('colors.omen_red', '#FF2000'), ('colors.violet', '#A000FF'),
]
BRIGHTNESS_STEPS = [('25%', 64), ('50%', 128), ('75%', 191), ('100%', 255)]
LANGUAGE_NAMES = [('pl', 'Polski'), ('en', 'English')]


class Tray(QSystemTrayIcon):
    def __init__(self, icon, window, client, app):
        super().__init__(icon)
        self.window = window
        self.client = client
        self.app = app
        self.setToolTip(i18n.t('window.title'))
        self.menu = QMenu()
        self.setContextMenu(self.menu)
        self.menu.aboutToShow.connect(self._rebuild)
        self.activated.connect(self._on_activated)
        window.stateChanged.connect(self._refresh_tooltip)
        self._rebuild()
        self._refresh_tooltip()

    # ---------- menu ----------

    def _rebuild(self):
        self.menu.clear()
        st = self.client.try_call('status')

        if st is None:
            self.menu.addAction(i18n.t('tray.daemon_down')).setEnabled(False)
        elif not st.get('connected'):
            self.menu.addAction(i18n.t('tray.not_connected')).setEnabled(False)

        act = self.menu.addAction(i18n.t('tray.open_window'))
        act.triggered.connect(self._show_window)
        self.menu.addSeparator()

        colors = self.menu.addMenu(i18n.t('tray.color'))
        for key, hexv in QUICK_COLORS:
            a = colors.addAction(i18n.t(key))
            a.triggered.connect(lambda _c=False, h=hexv: self._set(
                {'effect': 'static', 'color': h}))

        # Lista trybow prosto z katalogu demona — nowy efekt w silniku pojawia
        # sie tu sam, bez dopisywania czegokolwiek w trayu.
        cat = self.client.try_call('effects') or {}
        current = (st or {}).get('effect', {}).get('effect')
        modes = self.menu.addMenu(i18n.t('tray.mode'))
        group = QActionGroup(modes)
        group.setExclusive(True)
        for spec in cat.get('catalogue', []):
            if spec['name'] == 'perkey':
                continue          # per klawisz ma sens tylko w oknie
            a = modes.addAction(i18n.effect_label(spec['name'], spec['label']))
            a.setCheckable(True)
            a.setChecked(spec['name'] == current)
            a.setActionGroup(group)
            a.triggered.connect(
                lambda _c=False, n=spec['name']: self._set({'effect': n}))

        presets = self.menu.addMenu(i18n.t('tray.preset'))
        for name in cat.get('presets', []):
            a = presets.addAction(name)
            a.triggered.connect(lambda _c=False, n=name: self._preset(n))

        bright = self.menu.addMenu(i18n.t('tray.brightness'))
        cur = (st or {}).get('brightness')
        group = QActionGroup(bright)
        group.setExclusive(True)
        for label, value in BRIGHTNESS_STEPS:
            a = bright.addAction(label)
            a.setCheckable(True)
            a.setActionGroup(group)
            if cur is not None and abs(cur - value) <= 12:
                a.setChecked(True)
            a.triggered.connect(lambda _c=False, v=value: self._brightness(v))

        profiles = self.menu.addMenu(i18n.t('tray.profile'))
        r = self.client.try_call('profile.list')
        names = (r or {}).get('profiles', [])
        current = (r or {}).get('current')
        if not names:
            profiles.addAction(i18n.t('tray.profile_none')).setEnabled(False)
        for name in names:
            a = profiles.addAction(name)
            a.setCheckable(True)
            a.setChecked(name == current)
            a.triggered.connect(lambda _c=False, n=name: self._profile(n))

        self.menu.addSeparator()
        rx = (st or {}).get('reactive') or {}
        a = self.menu.addAction(i18n.t('tray.reactive'))
        a.setCheckable(True)
        a.setChecked(bool(rx.get('enabled')))
        if rx.get('enabled') and not rx.get('available'):
            a.setText(i18n.t('tray.reactive_no_access'))
        a.triggered.connect(lambda _c=False, v=not rx.get('enabled'):
                            self._reactive(v))

        self.menu.addSeparator()
        ctrl = self.menu.addMenu(i18n.t('tray.control'))
        cgroup = QActionGroup(ctrl)
        cgroup.setExclusive(True)
        owner = (st or {}).get('control', 'app')
        for label_key, value, tip_key in (
                ('window.control_app', 'app', 'tray.control_app_tip'),
                ('window.control_bios', 'bios', 'tray.control_bios_tip')):
            a = ctrl.addAction(i18n.t(label_key))
            a.setCheckable(True)
            a.setChecked(owner == value)
            a.setActionGroup(cgroup)
            a.setToolTip(i18n.t(tip_key))
            a.triggered.connect(lambda _c=False, v=value: self._control(v))

        self.menu.addSeparator()
        lang_menu = self.menu.addMenu(i18n.t('tray.language'))
        lgroup = QActionGroup(lang_menu)
        lgroup.setExclusive(True)
        for code, name in LANGUAGE_NAMES:
            a = lang_menu.addAction(name)
            a.setCheckable(True)
            a.setChecked(i18n.get_language() == code)
            a.setActionGroup(lgroup)
            a.triggered.connect(lambda _c=False, cc=code: self._set_language(cc))

        self.menu.addSeparator()
        a = self.menu.addAction(i18n.t('tray.quit'))
        a.triggered.connect(self._quit)

    # ---------- akcje ----------

    def _guard(self, fn):
        try:
            fn()
        except (NoDaemon, DaemonError) as e:
            self.showMessage('OMEN Keyboard', str(e),
                             QSystemTrayIcon.Warning, 4000)
            return
        self._refresh_tooltip()
        if self.window.isVisible():
            self.window.refresh()

    def _set(self, params):
        self._guard(lambda: self.client.call('set', params=params))

    def _preset(self, name):
        self._guard(lambda: self.client.call('preset', name=name))

    def _brightness(self, value):
        self._guard(lambda: self.client.call('brightness', value=value))

    def _profile(self, name):
        self._guard(lambda: self.client.call('profile.load', name=name))

    def _control(self, owner):
        self._guard(lambda: self.client.call('control', owner=owner))

    def _reactive(self, enable):
        self._guard(lambda: self.client.call('reactive', enable=enable))

    def _set_language(self, code):
        i18n.set_language(code, persist=True)
        self.window.retranslate()
        self._refresh_tooltip()

    def _show_window(self):
        self.window.show()
        self.window.raise_()
        self.window.activateWindow()

    def _on_activated(self, reason):
        if reason == QSystemTrayIcon.Trigger:
            if self.window.isVisible():
                self.window.hide()
            else:
                self._show_window()

    def _quit(self):
        # Zamykamy TYLKO GUI. Demon zyje dalej i klawiatura swieci — od gaszenia
        # jest "Zgaszone", a od oddania sprzetu "Oddaj kontrole firmware".
        self.window.quitting = True
        self.hide()
        self.app.quit()

    def _refresh_tooltip(self):
        st = self.client.try_call('status')
        if st is None:
            self.setToolTip(i18n.t('tray.tooltip_daemon_down'))
            return
        name = (st.get('effect') or {}).get('effect', '?')
        label = i18n.effect_label(name, name)
        parts = [f"{i18n.t('window.title')} — {label}"]
        if st.get('control') == 'bios':
            parts.append(i18n.t('tray.tooltip_bios'))
        else:
            parts.append(i18n.t('tray.tooltip_brightness', v=st.get('brightness', '?')))
        if st.get('profile'):
            parts.append(i18n.t('tray.tooltip_profile', name=st['profile']))
        self.setToolTip('\n'.join(parts))
