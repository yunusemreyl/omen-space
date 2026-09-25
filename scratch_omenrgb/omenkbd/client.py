"""Klient demona — wspolny dla CLI i GUI.

Protokol: linie JSON po gniezdzie unixowym. Zadanie {"cmd": ...}, odpowiedz
{"ok": bool, ...}. Swiadomie trzymane bez zaleznosci od Qt, zeby CLI nie musialo
ciagnac PySide6, a testy dzialaly bez srodowiska graficznego.
"""

import json
import os
import socket
import time

DEFAULT_TIMEOUT = 5.0


SYSTEM_SOCKET = '/run/omen-kbd/omen-kbd.sock'
SOCKET_GROUP = 'omenkbd'      # grupa dajaca dostep do gniazda demona


def socket_path():
    """Sciezka gniazda demona.

    Demon jest usluga SYSTEMOWA dzialajaca jako uzytkownik 'omenkbd' — tylko on
    ma dostep do urzadzen wejsciowych, wiec strumien nacisniec nie jest czytelny
    dla procesow zalogowanego uzytkownika. Gniazdo lezy w /run/omen-kbd i ma
    grupe 'omenkbd', do ktorej nalezysz; stad klienci moga sterowac kolorami,
    nie majac wgladu w klawisze.

    Kolejnosc: jawna zmienna srodowiskowa, katalog od systemd (RUNTIME_DIRECTORY),
    gniazdo systemowe, na koncu XDG_RUNTIME_DIR — ta ostatnia sciezka sluzy
    uruchomieniu demona z reki przy pracy nad kodem.
    """
    env = os.environ.get('OMEN_KBD_SOCKET')
    if env:
        return env
    rt = os.environ.get('RUNTIME_DIRECTORY')
    if rt:
        return os.path.join(rt.split(':')[0], 'omen-kbd.sock')
    # Sprawdzamy istnienie KATALOGU, nie samego gniazda. Katalog ma tryb 0750
    # i grupe demona, wiec os.path.exists() na gniezdzie zwraca False dla kogos
    # bez tej grupy — nie dlatego, ze gniazda nie ma, tylko dlatego, ze nie da
    # sie go zobaczyc. Poprzednia wersja szukala wtedy dalej i konczyla
    # komunikatem "demon nie dziala", zamiast powiedziec o uprawnieniach.
    if os.path.isdir(os.path.dirname(SYSTEM_SOCKET)):
        return SYSTEM_SOCKET
    run = os.environ.get('XDG_RUNTIME_DIR')
    if run:
        return os.path.join(run, 'omen-kbd.sock')
    return f'/tmp/omen-kbd-{os.getuid()}.sock'


class NoDaemon(Exception):
    """Demon nie dziala i nie udalo sie go wystartowac."""


class SocketPermission(NoDaemon):
    """Demon dziala, ale gniazdo jest nieosiagalne z braku uprawnien.

    Osobny typ, bo reakcja jest inna: startowanie demona nic nie da, a diagnoza
    jest konkretna (grupa i przelogowanie). Bez tego rozdzielenia proba
    autostartu przeslania wlasciwy komunikat swoim ogolnym."""


class DaemonError(Exception):
    """Demon odpowiedzial bledem — komunikat jest z jego strony."""


class Client:
    def __init__(self, path=None, timeout=DEFAULT_TIMEOUT, autostart=True):
        self.path = path or socket_path()
        self.timeout = timeout
        self.autostart = autostart

    def call(self, cmd, **kw):
        """Wysyla komende, zwraca dict odpowiedzi. Rzuca DaemonError gdy ok=False."""
        req = dict(kw, cmd=cmd)
        try:
            resp = self._once(req)
        except SocketPermission:
            raise                      # autostart tego nie naprawi
        except NoDaemon:
            if not self.autostart:
                raise
            self._start_daemon()
            resp = self._once(req)
        if not resp.get('ok'):
            raise DaemonError(resp.get('error', 'nieznany blad'))
        return resp

    def try_call(self, cmd, **kw):
        """Jak call(), ale zwraca None zamiast rzucac — do odswiezania GUI,
        gdzie chwilowy brak demona nie jest zdarzeniem wyjatkowym."""
        try:
            return self.call(cmd, **kw)
        except (NoDaemon, DaemonError, OSError):
            return None

    def _once(self, req):
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.settimeout(self.timeout)
        try:
            s.connect(self.path)
        except PermissionError:
            # Najczestszy przypadek: konto jest w grupie demona w bazie, ale
            # BIEZACA SESJA jej jeszcze nie ma, bo grupy przypisuja sie przy
            # logowaniu. Bez tego komunikatu wychodzi goly traceback.
            s.close()
            raise SocketPermission(
                f'brak praw do gniazda {self.path}.\n'
                f'  Twoje konto musi byc w grupie {SOCKET_GROUP!r}, a grupy '
                'zaczynaja dzialac po ponownym zalogowaniu.\n'
                f'  sprawdz baze  :  id -nG {os.environ.get("USER", "")} | '
                f'tr " " "\\n" | grep -x {SOCKET_GROUP}\n'
                f'  sprawdz sesje :  id -nG | tr " " "\\n" | grep -x {SOCKET_GROUP}\n'
                f'  bez wylogowania:  sg {SOCKET_GROUP} -c "omen-kbd status"'
            ) from None
        except (FileNotFoundError, ConnectionRefusedError, NotADirectoryError) as e:
            s.close()
            raise NoDaemon(str(e)) from None
        try:
            s.sendall((json.dumps(req) + '\n').encode())
            buf = bytearray()
            while b'\n' not in buf:
                chunk = s.recv(65536)
                if not chunk:
                    raise NoDaemon('demon rozlaczyl sie bez odpowiedzi')
                buf.extend(chunk)
            return json.loads(bytes(buf).split(b'\n', 1)[0])
        finally:
            s.close()

    def _start_daemon(self):
        """Demon jest usluga systemowa, wiec klient nie moze go podniesc sam —
        to wymagaloby roota. Probujemy tylko jednostki uzytkownika (uzywanej przy
        pracy nad kodem), a poza tym mowimy wprost, co uruchomic."""
        if os.system('systemctl --user start omen-kbd.service >/dev/null 2>&1') == 0:
            for _ in range(60):
                if os.path.exists(self.path):
                    return
                time.sleep(0.05)
        raise NoDaemon(
            'demon nie odpowiada.\n'
            '  sprawdz:    systemctl status omen-kbd\n'
            '  uruchom:    sudo systemctl start omen-kbd\n'
            '  z reki:     omen-kbd-daemon -v\n'
            '  instalacja: bash packaging/install.sh')
