#!/usr/bin/env python3
"""Testy odpornosci demona — znikniecie urzadzenia, zmiana numeru hidraw,
powrot z uspienia. Bez fizycznego sprzetu: warstwa urzadzenia jest podmieniona.

Odpowiada kryteriom akceptacji nr 4 (wybudzenie) i nr 5 (przepiecie doku).

    python3 test/test_resilience.py
"""

import json
import os
import sys
import time
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..'))

from omenkbd.core.device import DeviceGone, PermissionProblem
from omenkbd.engine import daemon as daemon_mod
from omenkbd.engine import reactive as reactive_mod
from omenkbd.engine import state as state_mod
from test_omenkbd import fake_layout

ATTRS = {'lamp_count': 120, 'width_um': 342000, 'height_um': 125000,
         'depth_um': 1000, 'kind': 1, 'kind_name': 'Keyboard',
         'min_update_interval_us': 33000}


class StubLampArray:
    """Podmiana LampArray. `alive` sterowane z testu."""
    instances = []

    def __init__(self, node):
        self.node = node
        self.attrs = dict(ATTRS)
        self.alive = True
        self.closed = False
        self.writes = 0
        self.autonomous = []
        StubLampArray.instances.append(self)

    def _guard(self):
        if not self.alive:
            raise DeviceGone('stub: odlaczone')

    def close(self):
        self.closed = True

    def set_autonomous(self, on):
        self._guard()
        self.autonomous.append(on)

    def range_update(self, *a, **kw):
        self._guard()
        self.writes += 1

    def multi_update(self, *a, **kw):
        self._guard()
        self.writes += 1


class StubKeyWatcher:
    """Atrapa KeyWatcher. Uzywa prawdziwego pipe() jako fd — demon rejestruje
    go na selektorze (epoll_ctl wymaga faktycznie otwartego deskryptora), ale
    nigdy z niego nie czyta naprawde: read_presses() jest przechwycone i zwraca
    wstrzykniete przez test kody, bez dotykania jadra."""
    instances = []
    default_permitted = True   # testy przestawiaja to na False, zeby symulowac
                               # brak reguly udev (--with-reactive nie zainstalowany)

    def __init__(self, vid, pid, permitted=None):
        self.vid, self.pid = vid, pid
        self.permitted = StubKeyWatcher.default_permitted if permitted is None else permitted
        self.closed = False
        self._pending = []
        self._r = self._w = None
        if self.permitted:
            self._r, self._w = os.pipe()
        StubKeyWatcher.instances.append(self)

    def available(self):
        return self.permitted

    def fileno_list(self):
        return [self._r] if self.permitted else []

    @property
    def opened(self):
        return ['/dev/input/stub'] if self.permitted else []

    def inject(self, evdev_codes):
        self._pending.extend(evdev_codes)

    def read_presses(self, fd):
        out, self._pending = self._pending, []
        return out

    def close(self):
        self.closed = True
        for fd in (self._r, self._w):
            if fd is not None:
                try:
                    os.close(fd)
                except OSError:
                    pass
        self._r = self._w = None


class DaemonHarness(unittest.TestCase):
    """Demon z podmieniona warstwa urzadzenia i konfiguracja w katalogu tymczasowym."""

    def setUp(self):
        import tempfile
        self.tmp = tempfile.mkdtemp(prefix='omen-kbd-test-')
        self._saved = (state_mod.CONFIG_DIR, state_mod.STATE_PATH,
                       state_mod.PROFILE_DIR, state_mod.REACTIVE_PATH)
        state_mod.CONFIG_DIR = self.tmp
        state_mod.STATE_PATH = os.path.join(self.tmp, 'state.json')
        state_mod.PROFILE_DIR = os.path.join(self.tmp, 'profiles')
        # Bez tego kazdy test cmd_reactive() pisze do PRAWDZIWEGO
        # ~/.config/omen-kbd/reactive.json — dokladnie tak zbudowal sie
        # falszywy alarm, ktory kosztowal godzine diagnozy: lezacy tam plik
        # z poprzedniej reki uruchomionej sesji wlaczal reactive w KAZDYM
        # kolejnym tescie, w tym w testach z niego niezwiazanych.
        state_mod.REACTIVE_PATH = os.path.join(self.tmp, 'reactive.json')

        StubLampArray.instances = []
        self.nodes = ['/dev/hidraw7']
        self._orig = (daemon_mod.discover, daemon_mod.LampArray,
                      daemon_mod.layout_mod.load)
        daemon_mod.discover = lambda: [
            {'node': n, 'name': 'stub', 'phys': '', 'vid': 0x0d62, 'pid': 0x54bf}
            for n in self.nodes]
        daemon_mod.LampArray = StubLampArray
        daemon_mod.layout_mod.load = lambda la, dev, refresh=False: (fake_layout(), True)

        StubKeyWatcher.instances = []
        StubKeyWatcher.default_permitted = True
        self._orig_watcher = daemon_mod.KeyWatcher
        daemon_mod.KeyWatcher = StubKeyWatcher

        self.d = daemon_mod.Daemon(sock_path=os.path.join(self.tmp, 'sock'))

    def tearDown(self):
        daemon_mod.KeyWatcher = self._orig_watcher
        (daemon_mod.discover, daemon_mod.LampArray,
         daemon_mod.layout_mod.load) = self._orig
        (state_mod.CONFIG_DIR, state_mod.STATE_PATH,
         state_mod.PROFILE_DIR, state_mod.REACTIVE_PATH) = self._saved
        import shutil
        shutil.rmtree(self.tmp, ignore_errors=True)


class TestDeviceLoss(DaemonHarness):
    def test_connects_and_renders(self):
        self.assertTrue(self.d.ensure_device(0.0))
        self.assertTrue(self.d.render_now(force=True))
        self.assertGreater(StubLampArray.instances[0].writes, 0)

    def test_loss_during_render_is_caught_not_raised(self):
        """Kryterium 7: znikniecie w trakcie pracy ma dac czytelny stan,
        nie traceback z petli glownej."""
        self.d.ensure_device(0.0)
        self.d.render_now(force=True)
        StubLampArray.instances[0].alive = False
        self.assertFalse(self.d.render_now())          # bez wyjatku
        self.assertIsNone(self.d.la)
        self.assertTrue(StubLampArray.instances[0].closed)

    def test_reconnects_on_new_hidraw_number(self):
        """Kryterium 5: przepiecie doku przenumerowuje /dev/hidraw*.
        Demon musi znalezc urzadzenie po deskryptorze, nie po nazwie wezla."""
        self.d.ensure_device(0.0)
        self.d.render_now(force=True)
        StubLampArray.instances[0].alive = False
        self.d.render_now()
        self.assertIsNone(self.d.la)

        self.nodes = ['/dev/hidraw12']                 # wrocilo pod innym numerem
        self.d.reconnect_at = 0.0
        self.assertTrue(self.d.ensure_device(1.0))
        self.assertEqual(self.d.dev_info['node'], '/dev/hidraw12')
        self.d.sender.invalidate()
        self.assertTrue(self.d.render_now(force=True))
        self.assertGreater(StubLampArray.instances[-1].writes, 0)

    def test_backoff_grows_when_device_absent(self):
        """Bez urzadzenia demon nie moze krecic petla na pelnych obrotach."""
        self.nodes = []
        delays = []
        now = 0.0
        for _ in range(5):
            self.assertFalse(self.d.ensure_device(now))
            delays.append(self.d.reconnect_at - now)
            now = self.d.reconnect_at
        self.assertEqual(delays, sorted(delays))
        self.assertLessEqual(max(delays), daemon_mod.RECONNECT_MAX)
        self.assertGreater(max(delays), min(delays))

    def test_permission_problem_is_reported_not_crashed(self):
        """Kryterium 7: brak reguly udev ma dac komunikat, nie wyjatek."""
        def boom(node):
            raise PermissionProblem(node)
        daemon_mod.LampArray = boom
        self.assertFalse(self.d.ensure_device(0.0))
        self.assertIsNone(self.d.la)

    def test_permission_problem_logged_once_not_every_retry(self):
        """Petla ponawiania nie moze zasypywac dziennika jedna linia. Realny
        objaw z instalacji: ten sam ERROR co 5 sekund przez cala dobe, w ktorym
        gina wszystkie inne komunikaty."""
        import io
        import logging

        from omenkbd.core.device import permission_hint

        # Zaslepka idzie ta sama droga co LampArray.__init__: opakowuje wynik
        # permission_hint(). Bez tego test sprawdzalby komunikat, ktory w kodzie
        # produkcyjnym nigdy nie powstaje.
        self.nodes = ['/dev/null']          # wezel istniejacy wszedzie

        def boom(node):
            raise PermissionProblem(permission_hint(node))
        daemon_mod.LampArray = boom

        buf = io.StringIO()
        handler = logging.StreamHandler(buf)
        daemon_mod.log.addHandler(handler)
        daemon_mod.log.setLevel(logging.ERROR)
        try:
            now = 0.0
            for _ in range(15):
                self.d.ensure_device(now)
                now = self.d.reconnect_at
        finally:
            daemon_mod.log.removeHandler(handler)
        lines = [l for l in buf.getvalue().splitlines() if 'brak dostepu' in l]
        self.assertEqual(len(lines), 1, f'15 prob dalo {len(lines)} linii')
        # komunikat musi mowic, JAK to zdiagnozowac, nie tylko ze jest zle:
        # pod jakim uzytkownikiem chodzi demon i co sprawdzic na wezle
        self.assertIn('uzytkownik', lines[0])
        self.assertTrue('getfacl' in lines[0] or 'udevadm' in lines[0],
                        f'komunikat nie podpowiada, co sprawdzic: {lines[0]}')

    def test_problem_message_repeats_after_recovery(self):
        """Wyciszenie nie moze byc trwale: po odzyskaniu i ponownej awarii
        komunikat ma sie pojawic znowu, inaczej ukrylibysmy nowa awarie."""
        def boom(node):
            raise PermissionProblem(node)
        daemon_mod.LampArray = boom
        self.d.ensure_device(0.0)
        first = self.d._last_problem
        self.assertIsNotNone(first)

        daemon_mod.LampArray = StubLampArray      # naprawa
        self.d.reconnect_at = 0.0
        self.assertTrue(self.d.ensure_device(1.0))
        self.assertIsNone(self.d._last_problem, 'pamiec bledu ma sie czyscic')


class TestResume(DaemonHarness):
    def test_resume_resends_full_frame(self):
        """Kryterium 4: po wybudzeniu nie wiemy, co jest na urzadzeniu.
        Nie wolno polegac na pamieci ostatnio wyslanej klatki."""
        self.d.ensure_device(0.0)
        self.d.apply_effect({'effect': 'static', 'color': '#00FF88'})
        before = StubLampArray.instances[0].writes
        self.d.render_now()                             # nic sie nie zmienilo
        self.assertEqual(StubLampArray.instances[0].writes, before)

        self.d.cmd_resume({})                           # hook wybudzenia
        self.assertGreater(StubLampArray.instances[0].writes, before)

    def test_resume_clears_backoff_when_disconnected(self):
        self.d.ensure_device(0.0)
        StubLampArray.instances[0].alive = False
        self.d.render_now()
        self.d.reconnect_at = 9999.0
        self.d.cmd_resume({})
        self.assertEqual(self.d.reconnect_at, 0.0)

    def test_autonomous_disabled_on_every_send(self):
        self.d.ensure_device(0.0)
        self.d.apply_effect({'effect': 'static', 'color': 'red'})
        self.d.cmd_resume({})
        self.assertTrue(StubLampArray.instances[0].autonomous)
        self.assertTrue(all(v is False
                            for v in StubLampArray.instances[0].autonomous))


class TestStatePersistence(DaemonHarness):
    def test_effect_survives_restart(self):
        """'Pamieta ostatnie ustawienie' — stan musi przezyc restart procesu."""
        self.d.ensure_device(0.0)
        self.d.apply_effect({'effect': 'wave', 'speed': 0.42, 'axis': 'y'})
        self.d.brightness = 123
        state_mod.save_state(self.d.effect_params, self.d.brightness, None)

        d2 = daemon_mod.Daemon(sock_path=os.path.join(self.tmp, 'sock2'))
        self.assertEqual(d2.effect_params['effect'], 'wave')
        self.assertEqual(d2.effect_params['speed'], 0.42)
        self.assertEqual(d2.effect_params['axis'], 'y')
        self.assertEqual(d2.brightness, 123)

    def test_corrupt_state_falls_back_to_white(self):
        """Obciety JSON nie moze zablokowac startu — inaczej jedno zaniknniecie
        zasilania w trakcie zapisu i klawiatura zostaje ciemna na zawsze."""
        with open(state_mod.STATE_PATH, 'w') as f:
            f.write('{"effect_params": {"effect": "nie-ma-takiego"}}')
        d2 = daemon_mod.Daemon(sock_path=os.path.join(self.tmp, 'sock3'))
        self.assertEqual(d2.effect_params['effect'], 'static')

    def test_profiles_roundtrip(self):
        self.d.ensure_device(0.0)
        self.d.apply_effect({'effect': 'breathe', 'color': '#112233',
                             'period': 7.0})
        self.d.cmd_profile_save({'name': 'nocny'})
        self.d.apply_effect({'effect': 'static', 'color': '#FFFFFF'})
        self.d.cmd_profile_load({'name': 'nocny'})
        self.assertEqual(self.d.effect_params['effect'], 'breathe')
        self.assertEqual(self.d.effect_params['period'], 7.0)
        self.assertEqual(self.d.profile, 'nocny')

    def test_profile_name_traversal_rejected(self):
        with self.assertRaises(ValueError):
            state_mod.save_profile('../../../etc/passwd', {}, 200)


class TestScheduling(DaemonHarness):
    def test_static_effect_has_no_deadline(self):
        """Statyczny efekt = selektor spi bezterminowo = zero CPU na baterii."""
        self.d.ensure_device(0.0)
        self.d.apply_effect({'effect': 'static', 'color': 'red'})
        self.assertIsNone(self.d.next_frame)
        self.assertIsNone(self.d._timeout(0.0))

    def test_animated_effect_schedules_frames(self):
        self.d.ensure_device(0.0)
        self.d.apply_effect({'effect': 'wave'})
        self.assertIsNotNone(self.d.next_frame)
        self.assertAlmostEqual(self.d._timeout(self.d.next_frame - 0.033), 0.033,
                               places=5)

    def test_release_stops_the_clock(self):
        self.d.ensure_device(0.0)
        self.d.apply_effect({'effect': 'wave'})
        self.d.cmd_release({})
        self.assertIsNone(self.d.next_frame)
        self.assertTrue(self.d.released)
        self.assertIn(True, StubLampArray.instances[0].autonomous)

    def test_frame_interval_respects_firmware_limit(self):
        """Nie wolno wysylac czesciej niz MinUpdateInterval (33 ms)."""
        self.d.ensure_device(0.0)
        self.assertAlmostEqual(self.d.interval, 0.033, places=4)


class TestSocketAddressing(unittest.TestCase):
    """Wyznaczanie sciezki gniazda i reakcja na brak uprawnien.

    Oba te miejsca daly realne, mylace awarie przy instalacji: klient szukal
    gniazda w zlym miejscu, bo nie mogl go zobaczyc, a potem meldowal "demon nie
    dziala" mimo dzialajacego demona."""

    def setUp(self):
        self._env = {k: os.environ.get(k) for k in
                     ('OMEN_KBD_SOCKET', 'RUNTIME_DIRECTORY', 'XDG_RUNTIME_DIR')}
        for k in self._env:
            os.environ.pop(k, None)

    def tearDown(self):
        for k, v in self._env.items():
            if v is None:
                os.environ.pop(k, None)
            else:
                os.environ[k] = v

    def test_explicit_env_wins(self):
        from omenkbd import client
        os.environ['OMEN_KBD_SOCKET'] = '/tmp/jawne.sock'
        self.assertEqual(client.socket_path(), '/tmp/jawne.sock')

    def test_systemd_runtime_directory_used_by_daemon(self):
        from omenkbd import client
        os.environ['RUNTIME_DIRECTORY'] = '/run/omen-kbd'
        self.assertEqual(client.socket_path(), '/run/omen-kbd/omen-kbd.sock')

    def test_system_socket_chosen_when_directory_exists_even_if_unreadable(self):
        """Sedno realnej awarii: katalog gniazda ma tryb 0750 i grupe demona,
        wiec os.path.exists() na SAMYM GNIEZDZIE zwraca False dla kogos bez tej
        grupy — nie dlatego, ze gniazda nie ma. Decyduje istnienie KATALOGU."""
        import tempfile
        from omenkbd import client
        with tempfile.TemporaryDirectory() as d:
            sockdir = os.path.join(d, 'omen-kbd')
            os.mkdir(sockdir, 0o750)
            orig = client.SYSTEM_SOCKET
            client.SYSTEM_SOCKET = os.path.join(sockdir, 'omen-kbd.sock')
            try:
                self.assertFalse(os.path.exists(client.SYSTEM_SOCKET),
                                 'gniazda faktycznie nie ma — i o to chodzi')
                self.assertEqual(client.socket_path(), client.SYSTEM_SOCKET)
            finally:
                client.SYSTEM_SOCKET = orig

    def test_falls_back_to_xdg_when_no_system_dir(self):
        from omenkbd import client
        orig = client.SYSTEM_SOCKET
        client.SYSTEM_SOCKET = '/tmp/nie-ma-takiego-katalogu/omen-kbd.sock'
        os.environ['XDG_RUNTIME_DIR'] = '/tmp/xdg-test'
        try:
            self.assertEqual(client.socket_path(),
                             '/tmp/xdg-test/omen-kbd.sock')
        finally:
            client.SYSTEM_SOCKET = orig

    def test_permission_denied_is_a_distinct_error_with_guidance(self):
        """Brak uprawnien do gniazda NIE MOZE byc raportowany jako "demon nie
        dziala", bo wysyla na zla sciezke diagnozy. Musi tez byc osobnym typem,
        zeby proba autostartu nie przeslonila komunikatu swoim ogolnym."""
        import socket as socketmod
        import tempfile
        from omenkbd import client

        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, 's.sock')
            srv = socketmod.socket(socketmod.AF_UNIX, socketmod.SOCK_STREAM)
            srv.bind(path)
            srv.listen(1)
            os.chmod(path, 0o000)        # istnieje, ale nie wolno sie polaczyc
            try:
                c = client.Client(path=path, autostart=True)
                with self.assertRaises(client.SocketPermission) as cm:
                    c.call('ping')
            finally:
                srv.close()
        msg = str(cm.exception)
        self.assertIn('omenkbd', msg)
        self.assertIn('sg ', msg, 'komunikat ma dac obejscie bez wylogowania')
        self.assertIsInstance(cm.exception, client.NoDaemon)

    def test_missing_socket_is_plain_nodaemon(self):
        from omenkbd import client
        c = client.Client(path='/tmp/nie-ma/takiego.sock', autostart=False)
        with self.assertRaises(client.NoDaemon) as cm:
            c.call('ping')
        self.assertNotIsInstance(cm.exception, client.SocketPermission)


class TestDaemonSocketMode(DaemonHarness):
    def test_socket_is_group_accessible(self):
        """Tryb 0660, nie 0600: granica uprawnien jest na katalogu, a gniazdo
        musi byc dostepne dla czlonkow grupy demona. Tryb 0600 odcinal
        zalogowanego uzytkownika calkowicie — realna awaria."""
        import stat as statmod
        self.d.open_socket()
        try:
            mode = statmod.S_IMODE(os.stat(self.d.sock_path).st_mode)
            self.assertEqual(mode, 0o660, f'tryb gniazda: {oct(mode)}')
        finally:
            self.d.shutdown()


class TestPermissionDiagnostics(unittest.TestCase):
    """Diagnostyka rozroznia dwie przyczyny EACCES, ktore daja ten sam objaw:
    zla grupa/tryb (regula udev nie trafila) i ACL z pustym wpisem group::
    (grupa i tryb sa wtedy bez znaczenia, a "ls -l" pokazuje maske ACL).
    Ten drugi przypadek zjadl godzine diagnozy przy pierwszej instalacji."""

    def test_reports_group_and_mode(self):
        from omenkbd.core.device import permission_hint
        msg = permission_hint('/dev/null')
        self.assertIn('/dev/null', msg)
        self.assertIn('grupe', msg)
        self.assertIn('tryb', msg)

    def test_mentions_acl_when_present(self):
        """Plik z ACL musi dostac wskazowke o group::, nie o regule udev."""
        import shutil
        import subprocess
        import tempfile
        if not shutil.which('setfacl'):
            self.skipTest('brak setfacl')
        from omenkbd.core.device import permission_hint
        with tempfile.NamedTemporaryFile() as f:
            subprocess.run(['setfacl', '-m', 'u:root:r', f.name],
                           check=False, capture_output=True)
            msg = permission_hint(f.name)
        if 'ACL' not in msg:
            self.skipTest('system plikow nie utrzymal ACL')
        self.assertIn('group::', msg)
        self.assertIn('getfacl', msg)

    def test_points_at_udev_when_no_acl(self):
        from omenkbd.core.device import permission_hint
        msg = permission_hint('/dev/null')
        if 'POSIX ACL' in msg:
            self.skipTest('/dev/null ma ACL na tym systemie')
        self.assertIn('udevadm', msg)

    def test_missing_node_does_not_raise(self):
        from omenkbd.core.device import permission_hint
        self.assertIsInstance(permission_hint('/dev/nie-ma-takiego'), str)


class TestReactiveTyping(DaemonHarness):
    def test_enable_opens_watcher_and_registers_selector(self):
        self.d.ensure_device(0.0)
        r = self.d.cmd_reactive({'enable': True})
        self.assertTrue(r['ok'] if 'ok' not in r else True)  # brak 'ok' na wewn. wywolaniu
        self.assertTrue(self.d.reactive_enabled)
        self.assertIsNotNone(self.d.watcher)
        w = StubKeyWatcher.instances[-1]
        for fd in w.fileno_list():
            self.assertIn(fd, self.d.sel.get_map(),
                          'fd watchera musi byc zarejestrowany na selektorze')

    def test_disable_closes_watcher_and_unregisters(self):
        self.d.ensure_device(0.0)
        self.d.cmd_reactive({'enable': True})
        w = StubKeyWatcher.instances[-1]
        fds = list(w.fileno_list())
        self.d.cmd_reactive({'enable': False})
        self.assertFalse(self.d.reactive_enabled)
        self.assertIsNone(self.d.watcher)
        self.assertTrue(w.closed)
        for fd in fds:
            self.assertNotIn(fd, self.d.sel.get_map())

    def test_key_press_lights_the_lamp(self):
        self.d.ensure_device(0.0)
        self.d.apply_effect({'effect': 'off'})
        self.d.cmd_reactive({'enable': True, 'params': {'color': '#FF0000',
                                                        'decay': 1.0}})
        w_evt = StubKeyWatcher.instances[-1].fileno_list()[0]
        StubKeyWatcher.instances[-1].inject([17])   # evdev W
        self.d._on_key_event(w_evt)

        usage = self.d.layout.by_binding
        w_ids = self.d.layout.resolve('W')
        self.assertTrue(w_ids)
        self.assertTrue(self.d.reactive.has_pending())
        for lid in w_ids:
            self.assertIn(lid, self.d.reactive.presses)

    def test_static_effect_still_animates_while_flash_is_fading(self):
        """Sedno funkcji: tryb statyczny normalnie nie planuje klatek, ale
        zanikajacy blysk MUSI je planowac, inaczej zostanie na stale."""
        self.d.ensure_device(0.0)
        self.d.apply_effect({'effect': 'static', 'color': 'red'})
        self.assertIsNone(self.d.next_frame)      # bez reactive: cisza

        self.d.cmd_reactive({'enable': True})
        self.d.reactive.press([0], time.monotonic())
        self.d.reschedule()
        self.assertIsNotNone(self.d.next_frame,
                             'zanikajacy blysk musi planowac kolejna klatke')

    def test_unavailable_watcher_reported_not_crashed(self):
        """Brak reguly udev (--with-reactive nie zainstalowany) ma dac czytelny
        status 'available': False, nie wyjatek ani zawieszenie."""
        StubKeyWatcher.default_permitted = False
        self.d.ensure_device(0.0)
        r = self.d.cmd_reactive({'enable': True})
        self.assertTrue(self.d.reactive_enabled)      # zamiar zapamietany
        self.assertFalse(r['available'])               # ale fizycznie niedostepny

    def test_settings_persist_across_restart(self):
        self.d.ensure_device(0.0)
        self.d.cmd_reactive({'enable': True,
                             'params': {'color': '#00FF00', 'decay': 0.3,
                                       'curve': 'linear'}})
        d2 = daemon_mod.Daemon(sock_path=os.path.join(self.tmp, 'sock2'))
        self.assertTrue(d2.reactive_enabled)
        self.assertEqual(d2.reactive.params['color'], '#00FF00')
        self.assertEqual(d2.reactive.params['decay'], 0.3)

    def test_device_loss_closes_watcher_too(self):
        self.d.ensure_device(0.0)
        self.d.cmd_reactive({'enable': True})
        w = StubKeyWatcher.instances[-1]
        StubLampArray.instances[0].alive = False
        self.d.render_now()                # wykrywa zniknięcie LampArray
        self.assertTrue(w.closed, 'watcher ma sie zamknac razem z urzadzeniem')
        self.assertIsNone(self.d.watcher)

    def test_reconnect_reopens_watcher_when_still_enabled(self):
        self.d.ensure_device(0.0)
        self.d.cmd_reactive({'enable': True})
        StubLampArray.instances[0].alive = False
        self.d.render_now()
        self.d.reconnect_at = 0.0
        self.assertTrue(self.d.ensure_device(1.0))
        self.assertIsNotNone(self.d.watcher, 'reactive mial sie sam odtworzyc')

    def test_input_device_gone_does_not_crash_daemon(self):
        self.d.ensure_device(0.0)
        self.d.cmd_reactive({'enable': True})
        w_fd = StubKeyWatcher.instances[-1].fileno_list()[0]

        def boom(fd):
            raise OSError('urzadzenie zniklo')
        StubKeyWatcher.instances[-1].read_presses = boom
        self.d._on_key_event(w_fd)             # nie ma wyjatku na zewnatrz
        self.assertIsNone(self.d.watcher)

    def test_gui_panel_effect_key_is_ignored_not_crashed(self):
        """Panel reactive w GUI dziedziczy z EffectPanel, ktory zawsze dokleja
        klucz 'effect' (nazwe panelu) — sensowne dla trybow swiecenia, ale
        ReactiveOverlay tego klucza nie zna. Realna awaria: kliknieta zmiana
        koloru w oknie wywalala demona traceoback'iem 'unexpected keyword
        argument effect'."""
        self.d.ensure_device(0.0)
        r = self.d.cmd_reactive({'params': {'effect': 'reactive',
                                            'color': '#00FF00'}})
        self.assertEqual(r['params']['color'], '#00FF00')

    def test_bad_param_is_a_value_error_not_a_crash(self):
        self.d.ensure_device(0.0)
        with self.assertRaises(ValueError):
            self.d.cmd_reactive({'params': {'decay': 'nie liczba'}})


class TestNoKeystrokeSideChannel(DaemonHarness):
    """Demon dziala jako osobny uzytkownik wlasnie dlatego, ze przy efekcie
    reagujacym na pisanie zna nacisniecia klawiszy. Gniazdo jest dostepne dla
    zalogowanego uzytkownika, wiec ZADNA komenda nie moze zdradzac stanu lampek —
    z tego, ktora lampka swieci, da sie odczytac, co ktos wlasnie nacisnal.
    To niezmiennik bezpieczenstwa, nie szczegol implementacji."""

    READ_COMMANDS = ('ping', 'status', 'layout', 'keys', 'effects',
                     'profile.list')

    FORBIDDEN_KEYS = ('rgb', 'frame', 'colors_now', 'current_colors', 'pixels',
                      'lit', 'pressed', 'keystrokes', 'last_key')

    def _walk(self, obj, path='resp'):
        """Zwraca wszystkie sciezki i wartosci w odpowiedzi."""
        if isinstance(obj, dict):
            for k, v in obj.items():
                yield f'{path}.{k}', k, v
                yield from self._walk(v, f'{path}.{k}')
        elif isinstance(obj, (list, tuple)):
            for i, v in enumerate(obj):
                yield from self._walk(v, f'{path}[{i}]')

    def test_read_commands_never_return_lamp_state(self):
        self.d.ensure_device(0.0)
        self.d.apply_effect({'effect': 'wave'})
        self.d.render_now(force=True)
        for cmd in self.READ_COMMANDS:
            resp = self.d.dispatch({'cmd': cmd})
            for path, key, _ in self._walk(resp):
                with self.subTest(cmd=cmd, path=path):
                    self.assertNotIn(key.lower(), self.FORBIDDEN_KEYS,
                                     f'{cmd} zwraca {path}')

    def test_reactive_presses_never_appear_in_status(self):
        """cmd_status dostal nowe pole 'reactive' wraz z ta funkcja — musi
        wystawiac TYLKO zamiar (enabled/available/params), nigdy stan
        self.reactive.presses (lamp_id -> czas nacisniecia). To bylby dokladnie
        ten kanal boczny, przed ktorym ma chronic separacja na osobny uid."""
        self.d.ensure_device(0.0)
        self.d.apply_effect({'effect': 'wave'})
        self.d.cmd_reactive({'enable': True})
        self.d.reactive.press([3, 7, 11], time.monotonic())
        self.assertTrue(self.d.reactive.has_pending())

        resp = self.d.cmd_status({})
        rx = resp['reactive']
        self.assertEqual(set(rx), {'enabled', 'available', 'params'})
        for _, key, value in self._walk(resp):
            with self.subTest(key=key):
                self.assertNotIn(key.lower(),
                                 ('presses', 'press_times', 'lamp_times'))
        # twarde sprawdzenie: konkretne lamp id z nacisniecia nie wyciekaja
        dump = json.dumps(resp)
        for lamp_id in (3, 7, 11):
            self.assertNotIn(f'"{lamp_id}"', dump.split('"lamp_count"')[0])

    def test_status_does_not_leak_perkey_colours_of_other_effect(self):
        """Efekt perkey ma jawne kolory w opisie — to zapisane przez uzytkownika
        ustawienie, nie stan chwilowy, wiec jest w porzadku. Ale gdy aktywny jest
        inny efekt, zadnych kolorow per lampka byc nie moze."""
        self.d.ensure_device(0.0)
        self.d.apply_effect({'effect': 'fire'})
        self.d.render_now(force=True)
        resp = self.d.cmd_status({})
        self.assertNotIn('colors', resp.get('effect', {}))

    def test_no_command_returns_the_frame_buffer(self):
        """Twarde sprawdzenie: bierzemy faktyczna klatke i szukamy jej wartosci
        w odpowiedziach. Gdyby ktos kiedys dopisal 'podgladanie' po gniezdzie,
        ten test to wychwyci."""
        self.d.ensure_device(0.0)
        self.d.apply_effect({'effect': 'gradient', 'color': '#123456',
                             'color2': '#654321'})
        self.d.render_now(force=True)
        frame_values = {tuple(c) for c in self.d.frame.rgb}
        self.assertGreater(len(frame_values), 1, 'klatka ma byc zroznicowana')
        for cmd in self.READ_COMMANDS:
            resp = self.d.dispatch({'cmd': cmd})
            for path, _, v in self._walk(resp):
                if isinstance(v, (list, tuple)) and len(v) == 3 \
                        and all(isinstance(x, int) for x in v):
                    with self.subTest(cmd=cmd, path=path):
                        self.assertNotIn(tuple(v), frame_values,
                                         f'{cmd} zwraca kolor z klatki w {path}')


class TestProtocol(DaemonHarness):
    def test_unknown_command_is_an_error_not_a_crash(self):
        with self.assertRaises(ValueError):
            self.d.dispatch({'cmd': 'zrob_kawe'})

    def test_dotted_command_names_route(self):
        self.d.ensure_device(0.0)
        r = self.d.dispatch({'cmd': 'profile.list'})
        self.assertTrue(r['ok'])

    def test_status_works_without_device(self):
        r = self.d.cmd_status({})
        self.assertTrue(r['ok'])
        self.assertFalse(r['connected'])

    def test_set_requires_effect_field(self):
        with self.assertRaises(ValueError):
            self.d.cmd_set({'params': {'color': 'red'}})


if __name__ == '__main__':
    unittest.main(verbosity=2)
