"""Odczyt nacisniec klawiszy z /dev/input/event* — wylacznie dla reactive typing.

Bezpieczenstwo jest tu warstwowe, ale WLASCIWA granica jest w regule udev
(99-hp-lamparray-input.rules), nie w tym module: ten kod otwiera to, na co
pozwolily uprawnienia systemowe, i nic wiecej. Odkrywanie urzadzen dziala
identyczna zasada co regula — dopasowanie po VID:PID tej samej klawiatury, ktora
ma LampArray — wiec kod i regula opisuja to samo urzadzenie z dwoch stron.

Kazdy odczytany bajt zamienia sie NATYCHMIAST na (lamp_id, czas) i nic wiecej
nie zostaje: modul nie loguje, nie buforuje surowych zdarzen, nie ma stanu
dluzszego niz jedna klatka przetwarzania. To jest niezmiennik pilnowany
testami w test_resilience.py (brak kanalu bocznego).
"""

import glob
import os
import struct

from .evdev_map import hid_usage_for

# struct input_event z linux/input.h na x86_64: dwa 'long' (sekundy, mikrosekundy)
# + typ (u16) + kod (u16) + wartosc (s32). '<qqHHi' pasuje na wiekszosci
# nowoczesnych jader x86_64/arm64 (64-bit time_t). Rozmiar: 24 bajty.
_FMT = '<qqHHi'
EVENT_SIZE = struct.calcsize(_FMT)

EV_KEY = 0x01
KEY_PRESS = 1
KEY_REPEAT = 2
# KEY_RELEASE = 0 — swiadomie ignorowane: puszczenie klawisza nie ma odswiezac
# zaniku, przeciwnie by zniweczylo caly efekt "blysk pod palcem".


def parse_events(buf):
    """Bajty z read() -> lista (typ, kod, wartosc). Czysta funkcja — do testow
    bez prawdziwego urzadzenia. Ignoruje niepelny ogon (czesciowo doczytany
    rekord), ktory dojdzie przy nastepnym read()."""
    out = []
    n = (len(buf) // EVENT_SIZE) * EVENT_SIZE
    for off in range(0, n, EVENT_SIZE):
        _, _, typ, code, val = struct.unpack_from(_FMT, buf, off)
        out.append((typ, code, val))
    return out


def find_devices(vid, pid):
    """Wezly /dev/input/eventN nalezace do klawiatury o danym VID:PID.

    Nie replikujemy tu heurystyki ID_INPUT_KEYBOARD z udev (ktora decyduje
    faktyczny zakres reguly) — probujemy otworzyc KAZDY wezel danego VID:PID,
    a te, do ktorych nie mamy uprawnien (bo regula ich nie objela — np. myszka
    wskaznikowa tej samej klawiatury), po prostu odpadaja przy proba otwarcia.
    Uprawnienia systemowe SA tu filtrem, nie ten kod.
    """
    out = []
    for path in sorted(glob.glob('/dev/input/event*')):
        sysdev = f'/sys/class/input/{os.path.basename(path)}/device'
        try:
            v = int(open(os.path.join(sysdev, 'id', 'vendor')).read().strip(), 16)
            p = int(open(os.path.join(sysdev, 'id', 'product')).read().strip(), 16)
        except (OSError, ValueError):
            continue
        if v == vid and p == pid:
            out.append(path)
    return out


class KeyWatcher:
    """Otwarte deskryptory klawiatury. Nieblokujace odczyty, wolane z petli
    demona po zdarzeniu na selektorze."""

    def __init__(self, vid, pid):
        self.paths = find_devices(vid, pid)
        self.fds = {}          # fd -> path, do komunikatow diagnostycznych
        self._buf = {}         # fd -> bytearray ogona po niepelnym rekordzie
        opened = []
        denied = []
        for path in self.paths:
            try:
                fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
            except PermissionError:
                denied.append(path)
                continue
            except OSError:
                continue
            self.fds[fd] = path
            self._buf[fd] = bytearray()
            opened.append(path)
        self.opened = opened
        self.denied = denied

    def available(self):
        return bool(self.fds)

    def fileno_list(self):
        return list(self.fds)

    def read_presses(self, fd):
        """Czyta co jest dostepne na fd, zwraca liste kodow evdev NACISNIETYCH
        (press albo repeat) klawiszy. Rzuca OSError przy zniknieciu urzadzenia —
        wolajacy ma go zlapac tak jak DeviceGone dla LampArray."""
        try:
            chunk = os.read(fd, EVENT_SIZE * 64)
        except BlockingIOError:
            return []
        if not chunk:
            raise OSError('urzadzenie wejsciowe zamkniete (EOF)')
        buf = self._buf[fd]
        buf.extend(chunk)
        events = parse_events(bytes(buf))
        consumed = (len(buf) // EVENT_SIZE) * EVENT_SIZE
        del buf[:consumed]
        return [code for typ, code, val in events
                if typ == EV_KEY and val in (KEY_PRESS, KEY_REPEAT)]

    def close(self):
        for fd in list(self.fds):
            try:
                os.close(fd)
            except OSError:
                pass
        self.fds.clear()
        self._buf.clear()


def codes_to_hid(codes):
    """Kody evdev -> HID usages, pomijajac nieznane (klawisze spoza tablicy)."""
    out = []
    for c in codes:
        u = hid_usage_for(c)
        if u is not None:
            out.append(u)
    return out


# Klawisz Copilot (i podobne "AI key" na nowszych laptopach) nie ma wlasnego
# kodu HID Keyboard/Keypad Page — firmware, dla zgodnosci ze starszymi
# systemami, emuluje go skrotem KLAWISZY REALNYCH: Lewy Windows + Lewy Shift +
# F23. Zweryfikowane empirycznie na HP OMEN MAX 16 (8D41): nacisniecie samego
# Copilota wysyla evdev 125 (KEY_LEFTMETA), 42 (KEY_LEFTSHIFT) i 193 (KEY_F23)
# w jednej paczce zdarzen. Bez filtrowania reactive typing zapalaloby lampki
# Lewego Shifta i Lewego Windows — klawisze, ktorych uzytkownik nie nacisnal.
#
# F23 jest tu bezpiecznym sygnalem: ta klawiatura nie ma fizycznego klawisza
# F23, wiec kod 193 pojawia sie WYLACZNIE jako czesc tego skrotu. Gdy widzimy
# go w paczce, traktujemy LMeta/LShift z tej samej paczki jako fantomowe i je
# odfiltrowujemy — nie gasimy jednak calej paczki, na wypadek, gdyby zawierala
# tez inne, prawdziwe nacisniecia.
_COPILOT_SENTINEL = 193          # KEY_F23 — nie ma go fizycznie na klawiaturze
_COPILOT_PHANTOM = {42, 125}     # KEY_LEFTSHIFT, KEY_LEFTMETA


def filter_copilot_burst(codes):
    """Usuwa fantomowe LShift/LMeta ze skrotu klawisza Copilot. Bezpieczne
    do wolania zawsze — bez sentinela w paczce nic sie nie zmienia."""
    if _COPILOT_SENTINEL not in codes:
        return codes
    return [c for c in codes if c not in _COPILOT_PHANTOM]
