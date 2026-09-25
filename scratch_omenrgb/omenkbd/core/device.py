"""Warstwa urzadzenia: wykrywanie i protokol HID LampArray (Usage Page 0x59).

Wszystko idzie przez Feature reports na /dev/hidraw*, little-endian, bez wyrownania,
pierwszy bajt = Report ID. Zadnych zaleznosci poza stdlib.
"""

import ctypes
import errno
import fcntl
import glob
import os
import struct

# 05 59 = Usage Page (Lighting And Illumination), 09 01 = Usage (LampArray),
# a1 01 = Collection (Application). Numeracja hidraw zmienia sie miedzy bootami
# i przy przepieciu stacji dokujacej, wiec identyfikujemy po deskryptorze.
LAMPARRAY_PREFIX = bytes([0x05, 0x59, 0x09, 0x01, 0xA1, 0x01])


def _hidioc(nr, size):
    """_IOC(_IOC_WRITE|_IOC_READ, 'H', nr, size)"""
    return (3 << 30) | (size << 16) | (ord('H') << 8) | nr


def HIDIOCSFEATURE(size):
    return _hidioc(0x06, size)


def HIDIOCGFEATURE(size):
    return _hidioc(0x07, size)


KINDS = {1: 'Keyboard', 2: 'Mouse', 3: 'GameController', 4: 'Peripheral',
         5: 'Scene', 6: 'Notification', 7: 'Chassis', 8: 'Wearable',
         9: 'Furniture', 10: 'Art'}

PURPOSES = {1: 'Control', 2: 'Accent', 4: 'Branding', 8: 'Status',
            16: 'Illumination', 32: 'Presentation'}


class DeviceGone(Exception):
    """Urzadzenie zniknelo w trakcie pracy — przepiecie doku, uspienie, reset USB."""


class PermissionProblem(Exception):
    """Brak praw do /dev/hidraw* — regula udev nie dziala albo blokuje ACL."""


def permission_hint(node):
    """Co dokladnie blokuje dostep do wezla urzadzenia.

    Rozroznienie jest istotne, bo objaw jest ten sam, a przyczyny dwie:
      * grupa/tryb sa zle    -> regula udev nie trafila
      * na wezle wisi ACL    -> grupa i tryb sa BEZ ZNACZENIA, decyduje wpis
                                group:: w ACL, ktory moze byc pusty
    W drugim przypadku "ls -l" klamie: pokazuje maske ACL w miejscu uprawnien
    grupy, wiec wyglada, jakby grupa miala dostep.
    """
    import grp
    import stat as statmod

    try:
        st = os.stat(node)
    except OSError as e:
        return f'{node}: {e.strerror}'
    try:
        group = grp.getgrgid(st.st_gid).gr_name
    except KeyError:
        group = str(st.st_gid)
    mode = statmod.filemode(st.st_mode)
    parts = [f'{node} ma grupe {group!r}, tryb {mode}']

    has_acl = False
    try:
        has_acl = 'system.posix_acl_access' in os.listxattr(node)
    except OSError:
        pass
    if has_acl:
        parts.append(
            'na wezle wisi POSIX ACL — grupa i tryb moga byc bez znaczenia, '
            'bo o dostepie decyduje wpis group:: w ACL. Sprawdz: '
            f'getfacl {node}   (jesli widzisz "group::---", to jest przyczyna)')
    else:
        parts.append('brak ACL, wiec decyduje grupa i tryb — '
                     'sprawdz, czy regula udev trafila: '
                     f'udevadm info -q property -n {node} | grep ID_USB')
    return '; '.join(parts)


def _read_sysfs(path):
    # stat() na report_descriptor klamie (zwraca 4096 — rozmiar strony sysfs),
    # wiec czytamy do konca i liczymy faktycznie odczytane bajty.
    with open(path, 'rb') as f:
        return f.read()


def _uevent(dirpath):
    out = {}
    try:
        with open(os.path.join(dirpath, 'uevent')) as f:
            for line in f:
                if '=' in line:
                    k, v = line.strip().split('=', 1)
                    out[k] = v
    except OSError:
        pass
    return out


def discover():
    """Zwraca liste dictow opisujacych kazde urzadzenie LampArray w systemie."""
    found = []
    for syspath in sorted(glob.glob('/sys/class/hidraw/hidraw*')):
        devdir = os.path.join(syspath, 'device')
        try:
            desc = _read_sysfs(os.path.join(devdir, 'report_descriptor'))
        except OSError:
            continue
        if not desc.startswith(LAMPARRAY_PREFIX):
            continue
        ue = _uevent(devdir)
        # HID_ID ma postac "0003:00000D62:000054BF"
        vid = pid = None
        if 'HID_ID' in ue:
            parts = ue['HID_ID'].split(':')
            if len(parts) == 3:
                vid, pid = int(parts[1], 16), int(parts[2], 16)
        found.append({
            'node': '/dev/' + os.path.basename(syspath),
            'name': ue.get('HID_NAME', ''),
            'phys': ue.get('HID_PHYS', ''),
            'vid': vid,
            'pid': pid,
        })
    return found


class LampArray:
    """Otwarte urzadzenie. Rzuca DeviceGone gdy znika w trakcie pracy."""

    def __init__(self, node):
        self.node = node
        try:
            self.fd = os.open(node, os.O_RDWR)
        except PermissionError as e:
            raise PermissionProblem(permission_hint(node)) from e
        try:
            self.attrs = self.read_attributes()
        except Exception:
            os.close(self.fd)
            raise

    def close(self):
        try:
            os.close(self.fd)
        except OSError:
            pass

    # --- niskopoziomowe ---

    def _get(self, report_id, size):
        buf = ctypes.create_string_buffer(size)
        buf[0] = bytes([report_id])
        try:
            fcntl.ioctl(self.fd, HIDIOCGFEATURE(size), buf, True)
        except OSError as e:
            raise self._translate(e) from e
        return bytes(buf.raw)

    def _set(self, payload):
        buf = ctypes.create_string_buffer(payload, len(payload))
        try:
            fcntl.ioctl(self.fd, HIDIOCSFEATURE(len(payload)), buf, True)
        except OSError as e:
            raise self._translate(e) from e

    @staticmethod
    def _translate(e):
        if e.errno in (errno.ENODEV, errno.EIO, errno.ENXIO, errno.ESHUTDOWN,
                       errno.EBADF, errno.EPIPE):
            return DeviceGone(e.strerror)
        return e

    # --- raporty ---

    def read_attributes(self):
        """Report 1 — LampArrayAttributes, 23 B."""
        d = self._get(1, 23)
        count, w, h, depth, kind, mui = struct.unpack_from('<HIIIII', d, 1)
        return {
            'lamp_count': count,
            'width_um': w, 'height_um': h, 'depth_um': depth,
            'kind': kind, 'kind_name': KINDS.get(kind, f'?{kind}'),
            'min_update_interval_us': mui,
        }

    def next_lamp_attributes(self):
        """Report 3 — LampAttributesResponse, 29 B.

        Firmware 8D41 IGNORUJE Report 2 (zadanie o konkretna lampke): trzyma wlasny
        kursor i inkrementuje go przy kazdym odczycie, niezaleznie od tego, o co pytano.
        Kursor zawija sie i przezywa miedzy procesami. Dlatego czytamy sekwencyjnie
        i ufamy polu LampId w odpowiedzi — start w dowolnym miejscu jest bezpieczny.
        """
        self._set(struct.pack('<BH', 2, 0))
        d = self._get(3, 29)
        lid, x, y, z, latency, purposes = struct.unpack_from('<HIIIII', d, 1)
        r, g, b, i, programmable, binding = struct.unpack_from('<BBBBBB', d, 23)
        return {
            'id': lid, 'x_um': x, 'y_um': y, 'z_um': z,
            'latency_us': latency,
            'purposes': [n for bit, n in PURPOSES.items() if purposes & bit],
            'levels': [r, g, b, i],
            'programmable': bool(programmable),
            'input_binding': binding,
        }

    def set_autonomous(self, on):
        """Report 6 — LampArrayControl, 2 B.

        Dopoki AutonomousMode=1, firmware odgrywa wlasny efekt i ignoruje kolory
        z hosta BEZ zglaszania bledu. Zapisy 'przechodza', tylko nic nie robia.
        """
        self._set(struct.pack('<BB', 6, 1 if on else 0))

    def range_update(self, start, end, rgbi, complete=True):
        """Report 5 — LampRangeUpdate, 10 B. Najtanszy jednolity kolor."""
        self._set(struct.pack('<BBHH', 5, 1 if complete else 0, start, end)
                  + bytes(rgbi))

    def multi_update(self, items, complete=True):
        """Report 4 — LampMultiUpdate, 51 B. items: [(lamp_id, (r,g,b,i))], max 8."""
        ids = [0] * 8
        rgbi = bytearray(32)
        n = 0
        for n, (lid, c) in enumerate(items[:8]):
            ids[n] = lid
            rgbi[n * 4:n * 4 + 4] = bytes(c)
        self._set(struct.pack('<BBB', 4, min(len(items), 8), 1 if complete else 0)
                  + struct.pack('<8H', *ids) + bytes(rgbi))
