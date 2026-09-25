"""Demon: jedyny wlasciciel deskryptora urzadzenia.

Dlaczego jeden proces pisze: klatka to kilkanascie raportow LampMultiUpdate
z LampUpdateComplete=0 we wszystkich poza ostatnia. Dwa procesy piszace naraz
przeplataja sie i rozwalaja framing. Demon serializuje wszystko.

Petla jest sterowana zdarzeniami, nie sleepem: gdy efekt jest statyczny, nie ma
zadnego deadline'u i selektor blokuje sie bezterminowo — zero CPU na baterii.
Gdy efekt animowany, harmonogram jest ABSOLUTNY (next += interval), nie
sleep(1/30), bo tamto kumuluje dryf.
"""

import errno
import json
import logging
import os
import selectors
import signal
import socket
import time

from ..client import socket_path
from ..core import layout as layout_mod
from ..core.device import DeviceGone, LampArray, PermissionProblem, discover
from ..core.inputwatch import KeyWatcher, codes_to_hid, filter_copilot_burst
from . import effects as effects_mod
from . import reactive as reactive_mod
from . import state as state_mod
from .frame import Frame, Sender

log = logging.getLogger('omen-kbd')


def _whoami():
    try:
        import pwd
        return pwd.getpwuid(os.geteuid()).pw_name
    except Exception:
        return f'uid {os.geteuid()}'

RECONNECT_MIN = 0.5
RECONNECT_MAX = 5.0
MAX_CLIENTS = 16


class Daemon:
    def __init__(self, sock_path=None):
        self.sock_path = sock_path or socket_path()
        self.la = None
        self.layout = None
        self.sender = None
        self.frame = None
        self.dev_info = None

        self.effect_params, self.brightness, self.profile = state_mod.load_state()
        self.effect = self._build_effect(self.effect_params)
        self.t0 = time.monotonic()

        # Reaktywnosc jest ORTOGONALNA do trybu: dziala na kazdym z 13 efektow,
        # bo overlay renderuje sie PO warstwie bazowej. reactive_enabled to
        # zamiar uzytkownika (przetrwac restart), watcher to biezacy stan
        # deskryptorow — moga sie rozjechac, gdy klawiatura zniknie z systemu.
        self.reactive_params, self.reactive_enabled = state_mod.load_reactive()
        self.reactive = reactive_mod.ReactiveOverlay(**self.reactive_params)
        self.watcher = None
        self._reactive_denied = False   # regula udev nie zainstalowana

        self.running = True
        self.next_frame = None
        self.reconnect_at = 0.0
        self.reconnect_delay = RECONNECT_MIN
        self._last_problem = None      # zeby nie zasypywac dziennika powtorka
        self.interval = 0.033
        self.released = False      # kontrola oddana firmware'owi

        self.sel = selectors.DefaultSelector()
        # Self-pipe zakladamy dopiero w run(): samo skonstruowanie Daemona
        # (testy, narzedzia) nie ma alokowac deskryptorow, a open_socket() moze
        # zakonczyc proces, gdy demon juz dziala.
        self._wake_r = self._wake_w = None
        self.clients = {}
        self.stats = {'frames': 0, 'reports': 0, 'reconnects': 0}

    # ---------- urzadzenie ----------

    def _build_effect(self, params):
        try:
            return effects_mod.from_dict(params)
        except Exception as e:
            log.warning('zapisany efekt jest nieprawidlowy (%s), wracam do bieli', e)
            self.effect_params = dict(state_mod.DEFAULT)
            return effects_mod.from_dict(self.effect_params)

    def connect(self):
        devs = discover()
        if not devs:
            return False
        self.dev_info = devs[0]
        self.la = LampArray(self.dev_info['node'])
        self.layout, cached = layout_mod.load(self.la, self.dev_info)
        self.sender = Sender(self.la, self.layout)
        self.frame = Frame(self.layout.count)
        mui = self.la.attrs['min_update_interval_us']
        # Nie wolno wysylac czesciej niz MinUpdateInterval.
        self.interval = max(0.010, mui / 1_000_000.0)
        log.info('%s: %d lampek, %s, mapa %s, %.0f kl./s max',
                 self.dev_info['node'], self.layout.count,
                 self.dev_info['name'], 'z cache' if cached else 'zbudowana',
                 1.0 / self.interval)
        self.reconnect_delay = RECONNECT_MIN
        self._last_problem = None
        if self.reactive_enabled:
            self._open_watcher()
        return True

    def _problem(self, fmt, *args):
        """Loguje tylko przy ZMIANIE komunikatu — inaczej petla ponawiania
        wypelnia dziennik jedna linia powtorzona setki razy."""
        msg = fmt % args
        if msg == self._last_problem:
            return
        self._last_problem = msg
        log.error('%s', msg)

    def _open_watcher(self):
        """Otwiera urzadzenia klawiszowe TEJ SAMEJ klawiatury i rejestruje
        je na selektorze. Uprawnienia systemowe (regula udev + grupa
        omenkbd-input) sa jedynym filtrem — jesli reguly nie ma, KeyWatcher
        po prostu nie otworzy zadnego wezla i reactive dziala bez efektu,
        z czytelnym statusem, nie z awaria."""
        if self.watcher:
            self._close_watcher()
        vid, pid = self.dev_info.get('vid'), self.dev_info.get('pid')
        self.watcher = KeyWatcher(vid, pid) if vid and pid else None
        if not self.watcher or not self.watcher.available():
            self._reactive_denied = True
            log.warning(
                'reactive typing wlaczony, ale brak dostepu do klawiszy tej '
                'klawiatury — zainstaluj z --with-reactive: '
                'bash packaging/install.sh --with-reactive')
            return
        self._reactive_denied = False
        for fd in self.watcher.fileno_list():
            self.sel.register(fd, selectors.EVENT_READ, self._on_key_event)
        log.info('reactive typing: %d urzadzen (%s)', len(self.watcher.opened),
                 ', '.join(self.watcher.opened))

    def _close_watcher(self):
        if not self.watcher:
            return
        for fd in self.watcher.fileno_list():
            try:
                self.sel.unregister(fd)
            except (KeyError, ValueError):
                pass
        self.watcher.close()
        self.watcher = None

    def _on_key_event(self, fd):
        """Nacisniecie -> HID usage -> lamp id -> znacznik czasu, NATYCHMIAST.
        Zaden surowy kod klawisza nie przezywa poza ta funkcje — to jest
        niezmiennik pilnowany testem braku kanalu bocznego."""
        try:
            codes = self.watcher.read_presses(fd)
        except OSError as e:
            log.warning('urzadzenie klawiszowe zniklo (%s)', e)
            self._close_watcher()
            self._reactive_denied = False   # sprobujemy ponownie przy nastepnym polaczeniu
            return
        codes = filter_copilot_burst(codes)
        if not codes or not self.layout:
            return
        now = time.monotonic()
        for usage in codes_to_hid(codes):
            ids = self.layout.by_binding.get(usage)
            if ids:
                self.reactive.press(ids, now)
        self.render_now()
        self.reschedule()

    def disconnect(self, why=''):
        self._close_watcher()
        if self.la:
            self.la.close()
        self.la = self.sender = self.layout = self.frame = None
        if why:
            log.warning('urzadzenie zniknelo (%s), szukam ponownie', why)

    def ensure_device(self, now):
        """Zwraca True gdy urzadzenie jest gotowe. Ponawia z narastajaca zwloka,
        zeby przy braku sprzetu nie zajezdzac CPU."""
        if self.la:
            return True
        if now < self.reconnect_at:
            return False
        try:
            if self.connect():
                self.stats['reconnects'] += 1
                self.t0 = time.monotonic()   # animacja startuje od nowa
                return True
        except PermissionProblem as e:
            # Ten sam blad co 5 sekund zasmieca dziennik i utrudnia znalezienie
            # prawdziwej przyczyny. Logujemy przy zmianie stanu, nie przy kazdej
            # probie; ponawianie idzie dalej po cichu.
            self._problem(
                'brak dostepu do urzadzenia. Demon dziala jako uzytkownik %r. %s',
                _whoami(), e)
        except (DeviceGone, OSError) as e:
            log.debug('proba polaczenia nieudana: %s', e)
        self.reconnect_at = now + self.reconnect_delay
        self.reconnect_delay = min(RECONNECT_MAX, self.reconnect_delay * 2)
        return False

    # ---------- renderowanie ----------

    def render_now(self, force=False):
        """Maluje i wysyla jedna klatke. Zwraca True jesli poszlo na urzadzenie."""
        if not self.la or self.released:
            return False
        try:
            now = time.monotonic()
            self.effect.render(now - self.t0, self.layout, self.frame)
            if self.reactive_enabled and self.reactive.has_pending():
                self.reactive.apply(self.frame, self.layout, now)
            n = self.sender.send(self.frame, self.brightness, force=force)
            self.stats['frames'] += 1
            self.stats['reports'] += n
            return True
        except DeviceGone as e:
            self.disconnect(str(e))
            self.reconnect_at = time.monotonic() + RECONNECT_MIN
            return False

    def reschedule(self):
        """Ustawia (albo kasuje) deadline nastepnej klatki.

        Zanikajacy blysk musi animowac klatki, NAWET gdy wybrany tryb jest
        statyczny — inaczej blysk zapaliby sie i zostal na stale, bo nic by
        wiecej nie przerysowalo klatki po zgasnieciu naciskanego klawisza.
        """
        animating = self.effect.animated or (
            self.reactive_enabled and self.reactive.has_pending())
        if animating and self.la and not self.released:
            self.next_frame = time.monotonic() + self.interval
        else:
            self.next_frame = None

    def apply_effect(self, params, *, persist=True, profile=None):
        eff = effects_mod.from_dict(params)
        self.effect = eff
        self.effect_params = eff.describe()
        self.profile = profile
        self.released = False
        self.t0 = time.monotonic()
        if self.sender:
            self.sender.invalidate()
        self.render_now(force=True)
        self.reschedule()
        if persist:
            state_mod.save_state(self.effect_params, self.brightness, self.profile)

    # ---------- serwer ----------

    def open_socket(self):
        # Sprzatanie po ubitym poprzedniku: gniazdo w XDG_RUNTIME_DIR przezywa
        # SIGKILL. Probujemy sie polaczyc — jesli nikt nie odpowiada, jest martwe.
        if os.path.exists(self.sock_path):
            probe = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            try:
                probe.connect(self.sock_path)
            except OSError:
                os.unlink(self.sock_path)
            else:
                probe.close()
                raise SystemExit(f'demon juz dziala na {self.sock_path}')
            finally:
                probe.close()
        self.srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.srv.bind(self.sock_path)
        # 0660, nie 0600: granica uprawnien jest na KATALOGU (/run/omen-kbd ma
        # tryb 0750 i grupe demona), a nie na samym gniezdzie. Czlonek grupy ma
        # miec dostep do sterowania kolorami — dostepu do klawiszy to nie daje,
        # bo te ida bezposrednio do procesu demona i nigdy nie wychodza gniazdem.
        # Tryb 0600 odcinalby zalogowanego uzytkownika calkowicie.
        os.chmod(self.sock_path, 0o660)
        self.srv.listen(MAX_CLIENTS)
        self.srv.setblocking(False)
        self.sel.register(self.srv, selectors.EVENT_READ, self._accept)

    def _drain_wakeup(self, sock):
        try:
            sock.recv(4096)
        except OSError:
            pass

    def _accept(self, sock):
        try:
            conn, _ = sock.accept()
        except OSError:
            return
        conn.setblocking(False)
        self.clients[conn] = bytearray()
        self.sel.register(conn, selectors.EVENT_READ, self._read_client)

    def _drop(self, conn):
        try:
            self.sel.unregister(conn)
        except (KeyError, ValueError):
            pass
        self.clients.pop(conn, None)
        conn.close()

    def _read_client(self, conn):
        try:
            data = conn.recv(65536)
        except OSError:
            self._drop(conn)
            return
        if not data:
            self._drop(conn)
            return
        buf = self.clients.get(conn)
        if buf is None:
            return
        buf.extend(data)
        while b'\n' in buf:
            line, _, rest = bytes(buf).partition(b'\n')
            buf.clear()
            buf.extend(rest)
            self._handle_line(conn, line)
            if conn not in self.clients:
                return

    def _handle_line(self, conn, line):
        line = line.strip()
        if not line:
            return
        try:
            req = json.loads(line)
            if not isinstance(req, dict):
                raise ValueError('oczekiwano obiektu JSON')
            resp = self.dispatch(req)
        except Exception as e:
            resp = {'ok': False, 'error': str(e)}
        try:
            conn.sendall((json.dumps(resp) + '\n').encode())
        except OSError:
            self._drop(conn)

    # ---------- komendy ----------

    def dispatch(self, req):
        cmd = req.get('cmd')
        fn = getattr(self, 'cmd_' + str(cmd).replace('.', '_').replace('-', '_'), None)
        if fn is None or not cmd:
            raise ValueError(f'nieznana komenda: {cmd!r}')
        return fn(req)

    def cmd_ping(self, req):
        return {'ok': True, 'pong': True}

    def cmd_status(self, req):
        return {'ok': True,
                'connected': self.la is not None,
                'released': self.released,
                'control': self.control_owner(),
                'device': self.dev_info,
                'attrs': self.la.attrs if self.la else None,
                'lamp_count': self.layout.count if self.layout else 0,
                'effect': self.effect_params,
                'brightness': self.brightness,
                'profile': self.profile,
                'fps_cap': round(1.0 / self.interval, 1),
                'stats': dict(self.stats),
                # Tylko zamiar i parametry — nigdy stan nacisniec
                # (self.reactive.presses). To jest ten sam niezmiennik, ktory
                # pilnuje testu braku kanalu bocznego.
                'reactive': {'enabled': self.reactive_enabled,
                            'available': self.watcher is not None
                            and self.watcher.available(),
                            'params': self.reactive.params}}

    def cmd_layout(self, req):
        if not self.layout:
            raise ValueError('urzadzenie niedostepne')
        return {'ok': True, 'attrs': self.layout.attrs,
                'lamps': [l.as_dict() for l in self.layout.lamps]}

    def cmd_keys(self, req):
        if not self.layout:
            raise ValueError('urzadzenie niedostepne')
        return {'ok': True, 'keys': {k: v for k, v in self.layout.by_key.items()}}

    def cmd_set(self, req):
        params = dict(req.get('params') or {})
        if 'effect' not in params:
            raise ValueError('brakuje pola "effect"')
        if 'brightness' in req and req['brightness'] is not None:
            self.brightness = max(0, min(255, int(req['brightness'])))
        self.apply_effect(params)
        return {'ok': True, 'effect': self.effect_params,
                'brightness': self.brightness}

    def cmd_preset(self, req):
        if not self.layout:
            raise ValueError('urzadzenie niedostepne')
        eff = effects_mod.preset(req.get('name', ''), self.layout)
        self.apply_effect(eff.describe())
        return {'ok': True, 'effect': self.effect_params}

    def cmd_keys_set(self, req):
        if not self.layout:
            raise ValueError('urzadzenie niedostepne')
        eff = effects_mod.keys_effect(req.get('names') or [],
                                     req.get('color', '#FFFFFF'),
                                     req.get('base', '#000000'), self.layout)
        self.apply_effect(eff.describe())
        return {'ok': True, 'effect': self.effect_params}

    def cmd_brightness(self, req):
        self.brightness = max(0, min(255, int(req.get('value', 200))))
        self.released = False
        if self.sender:
            self.sender.invalidate()
        self.render_now(force=True)
        self.reschedule()
        state_mod.save_state(self.effect_params, self.brightness, self.profile)
        return {'ok': True, 'brightness': self.brightness}

    def cmd_control(self, req):
        """Kto steruje podswietleniem: 'bios' (firmware klawiatury odgrywa
        wlasny efekt) albo 'app' (host wysyla kolory). To jeden stan widziany
        z dwoch stron, wiec jedna komenda zamiast dwoch niesymetrycznych."""
        owner = req.get('owner')
        if owner == 'bios':
            self.cmd_release(req)
        elif owner == 'app':
            self.cmd_resume(req)
        else:
            raise ValueError("owner musi byc 'bios' albo 'app'")
        return {'ok': True, 'control': self.control_owner()}

    def control_owner(self):
        return 'bios' if self.released else 'app'

    def cmd_reactive(self, req):
        """Wlacza/wylacza reactive typing i/albo zmienia jego parametry.

        req: {enable?: bool, params?: {color, decay, curve}}
        Dziala ORTOGONALNIE do wybranego trybu — nie wywoluje apply_effect().
        """
        if 'params' in req and req['params'] is not None:
            merged = dict(self.reactive.params)
            merged.update(req['params'])
            # 'effect' nie jest parametrem ReactiveOverlay — moze sie tu
            # znalezc, bo GUI buduje ten panel ta sama klasa co panele trybow
            # (EffectPanel.params() zawsze go dokleja). Ignorujemy defensywnie,
            # zeby literowka klienta nie wywalala calego demona bledem.
            merged.pop('effect', None)
            try:
                self.reactive = reactive_mod.ReactiveOverlay(**merged)
            except Exception as e:
                raise ValueError(f'zle parametry reactive: {e}') from None
            self.reactive_params = self.reactive.params

        if 'enable' in req and req['enable'] is not None:
            want = bool(req['enable'])
            if want and not self.reactive_enabled:
                self.reactive_enabled = True
                if self.la:
                    self._open_watcher()
            elif not want and self.reactive_enabled:
                self.reactive_enabled = False
                self._close_watcher()
                self.reactive.clear()
                self.render_now(force=True)   # zgasic ewentualny reziduum blysku
            self.reschedule()

        state_mod.save_reactive(self.reactive_params, self.reactive_enabled)
        return {'ok': True,
                'enabled': self.reactive_enabled,
                'available': self.watcher is not None and self.watcher.available(),
                'params': self.reactive.params}

    def cmd_resume(self, req):
        """Po wybudzeniu firmware wraca do AutonomousMode=1 i zjada nasze kolory.
        Nie wiemy, co jest na urzadzeniu — kasujemy pamiec i wysylamy cala klatke."""
        if not self.la:
            self.reconnect_at = 0.0     # nie czekaj na backoff
        if self.sender:
            self.sender.invalidate()
        self.released = False
        self.t0 = time.monotonic()
        ok = self.render_now(force=True)
        self.reschedule()
        return {'ok': True, 'applied': ok}

    def cmd_release(self, req):
        """Oddaje kontrole firmware'owi — wraca fabryczne pulsowanie."""
        self.released = True
        self.next_frame = None
        if self.la:
            try:
                self.la.set_autonomous(True)
            except DeviceGone as e:
                self.disconnect(str(e))
        return {'ok': True, 'released': True}

    def cmd_profile_list(self, req):
        return {'ok': True, 'profiles': state_mod.list_profiles(),
                'current': self.profile}

    def cmd_profile_save(self, req):
        name = req.get('name', '')
        state_mod.save_profile(name, self.effect_params, self.brightness)
        self.profile = name
        state_mod.save_state(self.effect_params, self.brightness, self.profile)
        return {'ok': True, 'saved': name}

    def cmd_profile_load(self, req):
        name = req.get('name', '')
        params, br = state_mod.load_profile(name)
        self.brightness = br
        self.apply_effect(params, profile=name)
        return {'ok': True, 'effect': self.effect_params, 'brightness': br,
                'profile': name}

    def cmd_profile_delete(self, req):
        name = req.get('name', '')
        state_mod.delete_profile(name)
        if self.profile == name:
            self.profile = None
        return {'ok': True, 'deleted': name}

    def cmd_effects(self, req):
        """Katalog efektow razem z deklaracja parametrow — GUI buduje z tego
        kontrolki, wiec silnik zostaje jedynym zrodlem prawdy."""
        return {'ok': True,
                'effects': [c['name'] for c in effects_mod.catalogue()],
                'catalogue': effects_mod.catalogue(),
                'presets': sorted(effects_mod.KEY_PRESETS)}

    def cmd_quit(self, req):
        self.running = False
        return {'ok': True, 'bye': True}

    # ---------- petla ----------

    def _timeout(self, now):
        deadlines = []
        if self.next_frame is not None:
            deadlines.append(self.next_frame)
        if not self.la:
            deadlines.append(self.reconnect_at)
        if not deadlines:
            return None                  # nic sie nie dzieje — spij bezterminowo
        return max(0.0, min(deadlines) - now)

    def run(self):
        self.open_socket()
        # Bez self-pipe'a SIGTERM ustawia flage, ale select() wznawia sie
        # automatycznie (PEP 475) i petla nigdy nie sprawdza warunku — demon
        # wisi az do SIGKILL.
        self._wake_r, self._wake_w = socket.socketpair()
        self._wake_r.setblocking(False)
        self._wake_w.setblocking(False)
        self.sel.register(self._wake_r, selectors.EVENT_READ, self._drain_wakeup)
        signal.set_wakeup_fd(self._wake_w.fileno())
        for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
            signal.signal(sig, lambda *_: setattr(self, 'running', False))

        now = time.monotonic()
        if self.ensure_device(now):
            self.render_now(force=True)
        self.reschedule()
        log.info('gniazdo %s, efekt %s', self.sock_path,
                 self.effect_params.get('effect'))

        while self.running:
            now = time.monotonic()
            try:
                events = self.sel.select(self._timeout(now))
            except InterruptedError:
                continue
            except OSError as e:
                if e.errno == errno.EINTR:
                    continue
                raise
            for key, _ in events:
                key.data(key.fileobj)

            now = time.monotonic()
            if not self.la:
                if self.ensure_device(now):
                    self.sender.invalidate()
                    self.render_now(force=True)
                    self.reschedule()
                continue

            if self.next_frame is not None and now >= self.next_frame:
                self.render_now()
                if self.next_frame is None:
                    continue
                # Harmonogram absolutny: brak dryfu przez godziny animacji.
                self.next_frame += self.interval
                if self.next_frame < now:
                    # Zgubione klatki (zawieszenie, przeciazenie) — nie nadrabiamy
                    # ich seria, tylko przesuwamy sie na najblizszy przyszly slot.
                    self.next_frame = now + self.interval

        self.shutdown()

    def shutdown(self):
        log.info('koncze; klatek=%d raportow=%d',
                 self.stats['frames'], self.stats['reports'])
        self._close_watcher()
        for conn in list(self.clients):
            self._drop(conn)
        try:
            signal.set_wakeup_fd(-1)
        except (ValueError, OSError):
            pass
        for sock in (self.srv, self._wake_r, self._wake_w):
            if sock is None:
                continue
            try:
                self.sel.unregister(sock)
            except (KeyError, ValueError):
                pass
            sock.close()
        self._wake_r = self._wake_w = None
        try:
            os.unlink(self.sock_path)
        except OSError:
            pass
        # Stan zostaje na urzadzeniu. Kontroli NIE oddajemy — inaczej kazdy restart
        # demona mrugalby fabrycznym efektem. Od tego jest komenda 'release'.
        self.disconnect()


def main(argv=None):
    import argparse
    p = argparse.ArgumentParser(prog='omen-kbd-daemon',
                                description='Demon podswietlenia HID LampArray')
    p.add_argument('--socket', default=None)
    p.add_argument('-v', '--verbose', action='store_true')
    a = p.parse_args(argv)
    logging.basicConfig(
        level=logging.DEBUG if a.verbose else logging.INFO,
        format='%(levelname)s: %(message)s')
    Daemon(a.socket).run()
    return 0


if __name__ == '__main__':
    import sys
    sys.exit(main())
