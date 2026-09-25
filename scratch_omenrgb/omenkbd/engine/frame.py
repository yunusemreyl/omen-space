"""Bufor klatki i wysylka roznicowa.

Silnik maluje do bufora, a Sender decyduje, ile z tego naprawde trafi na kabel.
Klatka bez zmian = zero ioctli. Klatka jednolita = jeden LampRangeUpdate zamiast
pietnastu LampMultiUpdate.
"""

from ..core.device import DeviceGone

BATCH = 8  # LampMultiUpdate przyjmuje maksymalnie 8 lampek na raport


class Frame:
    """Kolory RGB per pozycja w layout.lamps (nie per lamp id)."""

    __slots__ = ('n', 'rgb')

    def __init__(self, n):
        self.n = n
        self.rgb = [(0, 0, 0)] * n

    def fill(self, c):
        self.rgb = [c] * self.n

    def copy_from(self, other):
        self.rgb = list(other.rgb)

    def __eq__(self, other):
        return isinstance(other, Frame) and self.rgb == other.rgb


class Sender:
    """Wysyla klatki na urzadzenie, pomijajac to, co juz tam jest.

    LampUpdateComplete=0 we wszystkich paczkach poza ostatnia — wtedy kontroler
    zatrzaskuje cala klatke naraz i nie ma rozjezdzania sie kolorow w poprzek
    klawiatury. Ostatnia paczka ma 1.
    """

    def __init__(self, la, layout):
        self.la = la
        self.layout = layout
        self.last = None          # ostatnio WYSLANE (r,g,b) per pozycja
        self.last_intensity = None
        self.reasserts = 0

    def invalidate(self):
        """Po wybudzeniu / przepieciu firmware ma wlasny stan — nie wiemy jaki.
        Kasujemy pamiec, zeby nastepna klatka poszla w calosci."""
        self.last = None
        self.last_intensity = None

    def send(self, frame, intensity, force=False):
        """Zwraca liczbe wyslanych raportow. 0 = nic sie nie zmienilo."""
        # Firmware wraca do AutonomousMode=1 po S3 i wtedy po cichu ignoruje
        # kolory. Re-asercja kosztuje jeden ioctl na klatke i usuwa cala klase
        # bledow "zapisy przechodza, ale nic nie swieci".
        self.la.set_autonomous(False)
        self.reasserts += 1

        changed_all = force or self.last is None or intensity != self.last_intensity
        if not changed_all and frame.rgb == self.last:
            return 0

        sent = self._send_uniform(frame, intensity)
        if sent is None:
            sent = self._send_diff(frame, intensity, changed_all)

        self.last = list(frame.rgb)
        self.last_intensity = intensity
        return sent

    def _send_uniform(self, frame, intensity):
        """Jednolity kolor na ciaglym zakresie id -> jeden raport zamiast 15."""
        if not self.layout.contiguous:
            return None
        first = frame.rgb[0]
        for c in frame.rgb:
            if c != first:
                return None
        self.la.range_update(0, self.layout.count - 1, first + (intensity,))
        return 1

    def _send_diff(self, frame, intensity, changed_all):
        ids = self.layout.ids
        last = self.last
        items = []
        for i, c in enumerate(frame.rgb):
            if changed_all or c != last[i]:
                items.append((ids[i], c + (intensity,)))
        if not items:
            return 0
        reports = 0
        for n in range(0, len(items), BATCH):
            chunk = items[n:n + BATCH]
            self.la.multi_update(chunk, complete=(n + BATCH >= len(items)))
            reports += 1
        return reports


__all__ = ['Frame', 'Sender', 'DeviceGone', 'BATCH']
