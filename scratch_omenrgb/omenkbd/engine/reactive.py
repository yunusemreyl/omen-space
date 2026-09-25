"""Nakladka reaktywna: klawisz rozblyska pod palcem i gasnie.

Dziala NA KAZDYM z trybow z effects.py, bo renderuje sie PO warstwie bazowej —
mieszajac swoj kolor z tym, co juz jest w klatce — a nie zamiast niej. Zeby to
dodac do nowego efektu, nie trzeba zmieniac ani jednej linii w effects.py.

Bezstanowa w tym samym sensie co efekty animowane: intensywnosc blysku zalezy
tylko od czasu, ktory uplynal od ostatniego naciscniecia, wiec podglad w GUI
i to, co faktycznie widac na klawiaturze, nigdy sie nie rozjezdzaja.
"""

import collections

from ..core import color as C

# Deklaracja parametrow w tym samym stylu co Effect.PARAMS z effects.py — GUI
# buduje z tego kontrolki tym samym mechanizmem (gui/panels.make_panel).
from .effects import P, color_p

PARAMS = (
    color_p('color', 'Kolor blysku', '#FFFFFF'),
    P('decay', 'Czas zaniku', lo=0.05, hi=3.0, default=0.6, step=0.05),
    # Wartosci 'soft'/'linear' sa jezykowo neutralne — to STAN zapisywany na
    # dysku, nie tekst do wyswietlenia. Etykiety w omenkbd/i18n.py
    # (EFFECTS['reactive']['choices']['curve']).
    P('curve', 'Ksztalt zaniku', 'choice', default='soft',
      choices=('soft', 'linear')),
    # Osobna os regulacji od globalnego suwaka "Jasnosc": tamten skaluje CALA
    # klatke na koniec (tryb bazowy + blysk razem, w tej samej proporcji).
    # To tutaj kontroluje, jak mocno kolor blysku miesza sie z tlem w
    # szczytowym momencie — przy 0.4 blysk nigdy nie przebije w pelni koloru
    # bazowego, nawet zaraz po nacisnieciu.
    P('intensity', 'Moc blysku', lo=0.05, hi=1.0, default=1.0, step=0.05),
)

# Ile sekund po zgasnieciu trzymac wpis w pamieci, zanim go wyrzucimy —
# zapobiega nieograniczonemu rosnieciu slownika przy dlugim sesjach pisania.
_PURGE_AFTER = 2.0


def _curve(u, kind):
    """u: 0 (swiezy blysk) .. 1 (calkiem zgaslo). Zwraca sile koloru 1..0."""
    if u >= 1.0:
        return 0.0
    if kind == 'linear':
        return 1.0 - u
    # 'soft': gladkie wytlumienie na koncu (smoothstep odwrocony) —
    # blysk nie gasnie skokowo, tylko wyciska sie plynnie.
    t = 1.0 - u
    return t * t * (3.0 - 2.0 * t)


class ReactiveOverlay:
    def __init__(self, color='#FFFFFF', decay=0.6, curve='soft',
                 intensity=1.0):
        self.params = {}
        values = {'color': color, 'decay': decay, 'curve': curve,
                  'intensity': intensity}
        for p in PARAMS:
            v = values[p.name]
            if p.kind == 'color':
                v = C.to_hex(C.parse(v))
            elif p.kind == 'float':
                v = max(p.lo, min(p.hi, float(v)))
            elif p.kind == 'choice':
                v = v if v in p.choices else p.default
            self.params[p.name] = v
        self.color = C.parse(self.params['color'])
        self.decay = max(0.05, self.params['decay'])
        self.curve = self.params['curve']
        self.intensity = self.params['intensity']
        self.presses = {}      # lamp_id -> monotonic time nacisniecia

    def describe(self):
        return dict(self.params)

    def press(self, lamp_ids, now):
        for i in lamp_ids:
            self.presses[i] = now

    def has_pending(self):
        return bool(self.presses)

    def clear(self):
        self.presses.clear()

    def apply(self, frame, layout, now):
        """Miesza kolor blysku w bufor klatki. Wolane PO efekcie bazowym."""
        if not self.presses:
            return
        stale = []
        idx_by_id = {lid: i for i, lid in enumerate(layout.ids)}
        for lamp_id, t0 in self.presses.items():
            u = (now - t0) / self.decay
            if u >= 1.0 + _PURGE_AFTER / self.decay:
                stale.append(lamp_id)
                continue
            k = _curve(min(u, 1.0), self.curve) * self.intensity
            if k <= 0.0:
                continue
            i = idx_by_id.get(lamp_id)
            if i is None:
                continue
            frame.rgb[i] = C.lerp(frame.rgb[i], self.color, k)
        for lamp_id in stale:
            del self.presses[lamp_id]
