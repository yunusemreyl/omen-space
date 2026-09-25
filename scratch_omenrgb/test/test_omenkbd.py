#!/usr/bin/env python3
"""Testy jednostkowe omen-kbd — BEZ urzadzenia.

Sprawdzaja logike, ktora latwo zepsuc przy refaktorze: framing raportow,
wysylke roznicowa, obsluge znikniecia urzadzenia i parsowanie kolorow.
Urzadzenie jest zaslonione atrapa nagrywajaca kazdy zapis.

    python3 test/test_omenkbd.py
"""

import json
import os
import statistics
import struct
import sys
import time
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..'))

from omenkbd.core import color as C
from omenkbd.core.device import DeviceGone
from omenkbd.core.hidkeys import key_name
from omenkbd.core.layout import Lamp, Layout
from omenkbd.engine import effects as E
from omenkbd.engine.frame import Frame, Sender


def fake_layout(n=120, cols=20):
    """Siatka n lampek jak w prawdziwej klawiaturze: rzedy po `cols`."""
    lamps = []
    for i in range(n):
        col, row = i % cols, i // cols
        lamps.append(Lamp({'id': i, 'x_um': col * 18000, 'y_um': row * 18000,
                           'z_um': 0, 'input_binding': 0x04 + (i % 26),
                           'programmable': True, 'purposes': ['Control']}))
    return Layout({'lamp_count': n, 'width_um': (cols - 1) * 18000,
                   'height_um': ((n - 1) // cols) * 18000, 'depth_um': 1000,
                   'kind': 1, 'kind_name': 'Keyboard',
                   'min_update_interval_us': 33000}, lamps)


class FakeDevice:
    """Atrapa LampArray. Nagrywa raporty, umie zniknac na zawolanie."""

    def __init__(self):
        self.writes = []
        self.autonomous_calls = []
        self.gone = False

    def _guard(self):
        if self.gone:
            raise DeviceGone('atrapa: urzadzenie odlaczone')

    def set_autonomous(self, on):
        self._guard()
        self.autonomous_calls.append(on)

    def range_update(self, start, end, rgbi, complete=True):
        self._guard()
        self.writes.append(('range', start, end, tuple(rgbi), complete))

    def multi_update(self, items, complete=True):
        self._guard()
        assert len(items) <= 8, 'LampMultiUpdate przyjmuje max 8 lampek'
        self.writes.append(('multi', list(items), complete))


class TestColor(unittest.TestCase):
    def test_formats(self):
        self.assertEqual(C.parse('#FF8800'), (255, 136, 0))
        self.assertEqual(C.parse('ff8800'), (255, 136, 0))
        self.assertEqual(C.parse('f80'), (255, 136, 0))
        self.assertEqual(C.parse('red'), (255, 0, 0))
        self.assertEqual(C.parse((1, 2, 3)), (1, 2, 3))

    def test_bad(self):
        for bad in ('zielonkawy', '#12345', 'GGGGGG', ''):
            with self.assertRaises(C.ColorError):
                C.parse(bad)

    def test_roundtrip(self):
        self.assertEqual(C.to_hex(C.parse('#00FFC0')), '#00FFC0')

    def test_rainbow_table(self):
        self.assertEqual(len(C.RAINBOW), 256)
        self.assertTrue(all(len(c) == 3 for c in C.RAINBOW))


class TestKeys(unittest.TestCase):
    def test_usages(self):
        self.assertEqual(key_name(0x1a), 'W')
        self.assertEqual(key_name(0x2c), 'Space')
        self.assertEqual(key_name(0x3a), 'F1')
        self.assertEqual(key_name(0xe8), 'Omen')

    def test_unbound(self):
        # 0x00 i 0x03 to lampki bez klawisza — musza dac None, nie "0x00"
        self.assertIsNone(key_name(0x00))
        self.assertIsNone(key_name(0x03))


class TestLayout(unittest.TestCase):
    def setUp(self):
        self.lay = fake_layout()

    def test_shape(self):
        self.assertEqual(self.lay.count, 120)
        self.assertTrue(self.lay.contiguous)
        self.assertEqual(len(self.lay.rows()), 6)

    def test_normalised_coords(self):
        self.assertAlmostEqual(min(self.lay.nx), 0.0)
        self.assertAlmostEqual(max(self.lay.nx), 1.0)

    def test_multi_lamp_key(self):
        # Jednemu klawiszowi odpowiada czasem kilka lampek — resolve musi
        # zwrocic WSZYSTKIE, inaczej spacja swieci sie w jednej piatej.
        lids = self.lay.resolve('A')
        self.assertGreater(len(lids), 1)

    def test_serialisation_roundtrip(self):
        back = Layout.from_dict(self.lay.as_dict())
        self.assertEqual([l.id for l in back.lamps], [l.id for l in self.lay.lamps])
        self.assertEqual(back.by_key, self.lay.by_key)


class TestSender(unittest.TestCase):
    def setUp(self):
        self.dev = FakeDevice()
        self.lay = fake_layout()
        self.snd = Sender(self.dev, self.lay)
        self.frame = Frame(self.lay.count)

    def test_uniform_uses_single_range_report(self):
        self.frame.fill((0, 255, 128))
        n = self.snd.send(self.frame, 200)
        self.assertEqual(n, 1)
        kind, start, end, rgbi, complete = self.dev.writes[0]
        self.assertEqual((kind, start, end), ('range', 0, 119))
        self.assertEqual(rgbi, (0, 255, 128, 200))
        self.assertTrue(complete)

    def test_unchanged_frame_sends_nothing(self):
        self.frame.fill((10, 20, 30))
        self.snd.send(self.frame, 200)
        self.dev.writes.clear()
        self.assertEqual(self.snd.send(self.frame, 200), 0)
        self.assertEqual(self.dev.writes, [])

    def test_brightness_change_forces_resend(self):
        self.frame.fill((10, 20, 30))
        self.snd.send(self.frame, 200)
        self.dev.writes.clear()
        self.assertEqual(self.snd.send(self.frame, 100), 1)

    def test_diff_sends_only_changed_lamps(self):
        self.frame.fill((0, 0, 0))
        self.snd.send(self.frame, 255)
        self.dev.writes.clear()
        self.frame.rgb[7] = (255, 0, 0)
        self.frame.rgb[42] = (0, 255, 0)
        self.snd.send(self.frame, 255)
        multi = [w for w in self.dev.writes if w[0] == 'multi']
        self.assertEqual(len(multi), 1, 'dwie zmiany musza zmiescic sie w jednej paczce')
        ids = [lid for lid, _ in multi[0][1]]
        self.assertEqual(ids, [7, 42])

    def test_update_complete_framing(self):
        """LampUpdateComplete=0 we wszystkich paczkach poza ostatnia — inaczej
        kontroler zatrzaskuje polklatki i kolory rozjezdzaja sie w poprzek."""
        self.frame.fill((0, 0, 0))
        self.snd.send(self.frame, 255)
        self.dev.writes.clear()
        for i in range(0, 40):          # 40 zmian -> 5 paczek po 8
            self.frame.rgb[i] = (i, i, i)
        self.snd.send(self.frame, 255)
        multi = [w for w in self.dev.writes if w[0] == 'multi']
        self.assertEqual(len(multi), 5)
        self.assertEqual([w[2] for w in multi], [False, False, False, False, True])

    def test_batch_size_never_exceeds_eight(self):
        self.frame.fill((0, 0, 0))
        self.snd.send(self.frame, 255)
        self.dev.writes.clear()
        for i in range(self.lay.count):
            self.frame.rgb[i] = (i % 256, 0, 0)
        self.snd.send(self.frame, 255)
        for w in self.dev.writes:
            if w[0] == 'multi':
                self.assertLessEqual(len(w[1]), 8)

    def test_autonomous_reasserted_every_frame(self):
        """Firmware wraca do trybu wlasnego po S3 i po cichu zjada kolory.
        Re-asercja co klatke usuwa cala te klase bledow."""
        self.frame.fill((1, 1, 1))
        self.snd.send(self.frame, 255)
        self.snd.send(self.frame, 255)
        self.assertEqual(self.dev.autonomous_calls, [False, False])

    def test_invalidate_forces_full_resend(self):
        self.frame.fill((5, 5, 5))
        self.snd.send(self.frame, 200)
        self.dev.writes.clear()
        self.snd.invalidate()           # tak jak po wybudzeniu
        self.assertEqual(self.snd.send(self.frame, 200), 1)

    def test_device_gone_propagates(self):
        self.dev.gone = True
        self.frame.fill((1, 2, 3))
        with self.assertRaises(DeviceGone):
            self.snd.send(self.frame, 200)


class TestEffects(unittest.TestCase):
    def setUp(self):
        self.lay = fake_layout()
        self.frame = Frame(self.lay.count)

    def test_all_registered_effects_render(self):
        for name in E.REGISTRY:
            eff = E.make(name)
            eff.render(1.23, self.lay, self.frame)
            self.assertEqual(len(self.frame.rgb), self.lay.count, name)
            for c in self.frame.rgb:
                self.assertEqual(len(c), 3, name)
                self.assertTrue(all(0 <= v <= 255 for v in c), f'{name}: {c}')

    def test_animated_flags(self):
        self.assertFalse(E.make('static').animated)
        self.assertFalse(E.make('gradient').animated)
        self.assertFalse(E.make('perkey').animated)
        self.assertTrue(E.make('wave').animated)
        self.assertTrue(E.make('breathe').animated)

    def test_static_is_actually_static(self):
        eff = E.make('static', {'color': 'red'})
        eff.render(0.0, self.lay, self.frame)
        a = list(self.frame.rgb)
        eff.render(999.0, self.lay, self.frame)
        self.assertEqual(a, self.frame.rgb)

    def test_wave_moves(self):
        eff = E.make('wave', {'speed': 0.5})
        eff.render(0.0, self.lay, self.frame)
        a = list(self.frame.rgb)
        eff.render(1.0, self.lay, self.frame)
        self.assertNotEqual(a, self.frame.rgb)

    def test_gradient_endpoints(self):
        eff = E.make('gradient', {'color': '#FF0000', 'color2': '#0000FF',
                                  'axis': 'x'})
        eff.render(0, self.lay, self.frame)
        self.assertEqual(self.frame.rgb[0], (255, 0, 0))
        self.assertEqual(self.frame.rgb[19], (0, 0, 255))

    def test_describe_roundtrip(self):
        """Stan zapisujemy jako describe() i odtwarzamy from_dict() — jesli to
        sie rozjedzie, ustawienia nie przezyja restartu."""
        for name in E.REGISTRY:
            d = E.make(name).describe()
            again = E.from_dict(d)
            self.assertEqual(again.describe(), d, name)

    def test_unknown_effect_message_lists_options(self):
        with self.assertRaises(ValueError) as cm:
            E.make('dyskoteka')
        self.assertIn('wave', str(cm.exception))

    def test_preset_covers_all_lamps_of_a_key(self):
        eff = E.preset('wasd', self.lay)
        eff.render(0, self.lay, self.frame)
        for lid in self.lay.resolve('W'):
            self.assertEqual(self.frame.rgb[lid], C.parse('#FFFFFF'))

    def test_keys_effect_rejects_unknown_key(self):
        with self.assertRaises(ValueError) as cm:
            E.keys_effect(['W', 'NIEMA'], '#FFFFFF', '#000000', self.lay)
        self.assertIn('NIEMA', str(cm.exception))

    def test_breathe_stays_within_floor(self):
        eff = E.make('breathe', {'color': '#FFFFFF', 'period': 4.0, 'floor': 0.1})
        seen = []
        for i in range(200):
            eff.render(i * 0.02, self.lay, self.frame)
            seen.append(self.frame.rgb[0][0])
        self.assertGreaterEqual(min(seen), 25)   # 0.1 * 255 z zaokragleniem
        self.assertGreaterEqual(max(seen), 250)


class TestEffectSpecs(unittest.TestCase):
    """Deklaracje PARAMS sa kontraktem miedzy silnikiem a GUI — GUI buduje z nich
    suwaki i listy, wiec bledna deklaracja to zepsute okno, nie tylko brzydki kod."""

    def test_order_covers_registry_exactly(self):
        self.assertEqual(sorted(c.name for c in E.ORDER), sorted(E.REGISTRY))

    def test_every_effect_has_label(self):
        for cls in E.ORDER:
            self.assertTrue(cls.label and cls.label != '?', cls.name)

    def test_param_declarations_are_sane(self):
        for cls in E.ORDER:
            for p in cls.PARAMS:
                with self.subTest(effect=cls.name, param=p.name):
                    self.assertIn(p.kind, ('color', 'float', 'axis', 'choice'))
                    if p.kind == 'float':
                        self.assertLess(p.lo, p.hi)
                        self.assertGreaterEqual(p.default, p.lo)
                        self.assertLessEqual(p.default, p.hi)
                        self.assertGreater(p.step, 0)
                    elif p.kind == 'color':
                        C.parse(p.default)
                    elif p.kind == 'choice':
                        self.assertIn(p.default, p.choices)

    def test_out_of_range_values_are_clamped(self):
        e = E.make('fire', {'speed': 1e9, 'height': -5})
        self.assertLessEqual(e.params['speed'], 6.0)
        self.assertGreaterEqual(e.params['height'], 0.1)

    def test_bad_choice_falls_back_to_default(self):
        e = E.make('scanner', {'bounce': 'na ukos'})
        self.assertEqual(e.params['bounce'], 'bounce')

    def test_catalogue_survives_json(self):
        """Katalog idzie po gniezdzie do GUI — musi byc serializowalny."""
        cat = json.loads(json.dumps(E.catalogue()))
        self.assertEqual(len(cat), len(E.ORDER))
        self.assertEqual(cat[0]['name'], 'static')

    def test_describe_roundtrip_with_custom_values(self):
        """Stan zapisujemy jako describe() i odtwarzamy from_dict() — jesli to
        sie rozjedzie, ustawienia nie przezyja restartu demona."""
        for cls in E.ORDER:
            with self.subTest(effect=cls.name):
                d = E.make(cls.name).describe()
                self.assertEqual(E.from_dict(d).describe(), d)

    def test_effects_are_stateless(self):
        """Klatka zalezy WYLACZNIE od t. Na tym stoi brak dryfu po wybudzeniu
        i zgodnosc podgladu w GUI z tym, co widac na klawiaturze."""
        lay = fake_layout()
        a, b = Frame(lay.count), Frame(lay.count)
        for cls in E.ORDER:
            with self.subTest(effect=cls.name):
                e1, e2 = E.make(cls.name), E.make(cls.name)
                e1.render(7.25, lay, a)
                for t in (0.0, 3.5, 99.0, 7.25):   # e2 idzie inna droga do 7.25
                    e2.render(t, lay, b)
                self.assertEqual(a.rgb, b.rgb)

    def test_animated_effects_actually_move(self):
        lay = fake_layout()
        a, b = Frame(lay.count), Frame(lay.count)
        for cls in E.ORDER:
            if not cls.animated:
                continue
            with self.subTest(effect=cls.name):
                e = E.make(cls.name)
                e.render(0.5, lay, a)
                e.render(3.7, lay, b)
                self.assertNotEqual(a.rgb, b.rgb, f'{cls.name} stoi w miejscu')

    def test_static_effects_do_not_move(self):
        lay = fake_layout()
        a, b = Frame(lay.count), Frame(lay.count)
        for cls in E.ORDER:
            if cls.animated:
                continue
            with self.subTest(effect=cls.name):
                e = E.make(cls.name)
                e.render(0.0, lay, a)
                e.render(500.0, lay, b)
                self.assertEqual(a.rgb, b.rgb)

    def test_frame_budget(self):
        """Kazdy efekt musi zmiescic sie w klatce 33 ms z duzym zapasem —
        demon ma jeszcze wyslac 15 raportow po USB."""
        lay = fake_layout()
        f = Frame(lay.count)
        for cls in E.ORDER:
            e = E.make(cls.name)
            t0 = time.perf_counter()
            for i in range(200):
                e.render(i / 30.0, lay, f)
            ms = (time.perf_counter() - t0) / 200 * 1000
            with self.subTest(effect=cls.name):
                self.assertLess(ms, 5.0, f'{cls.name}: {ms:.2f} ms na klatke')


class TestNoiseDistribution(unittest.TestCase):
    """_fnv() jest silnikiem 'losowosci' dla Deszczu, Gwiazd i Ognia. Wolany na
    KOLEJNYCH liczbach calkowitych (numer kropli, id lampki) — realna regresja:
    sam FNV-1a bez finalizera mial slabe lawinowanie dla takiej sekwencji i
    kolejne wyniki dryfowaly niemal liniowo. W Deszczu wygladalo to jak jedna
    blyskajaca linia zjezdzajaca w dol, zamiast kropli rozrzuconych po calej
    szerokosci klawiatury."""

    def test_consecutive_integers_do_not_drift_linearly(self):
        vals = [E._fnv(31, i) for i in range(30)]
        diffs = [b - a for a, b in zip(vals, vals[1:])]
        # przy dryfie (bledna wersja) prawie wszystkie roznice maja ten sam
        # znak i zblizona wartosc bezwzglednā; przy dobrym rozrzucie znaki
        # powinny sie mieszac
        same_sign = sum(1 for d in diffs if d > 0) 
        self.assertGreater(same_sign, 5, 'wszystkie roznice ujemne — dryf')
        self.assertLess(same_sign, len(diffs) - 5, 'wszystkie roznice dodatnie — dryf')

    def test_consecutive_integers_spread_across_full_range(self):
        vals = [E._fnv(31, i) for i in range(200)]
        self.assertGreater(max(vals) - min(vals), 0.9)
        self.assertLess(abs(statistics.pstdev(vals) - 0.2887), 0.05,
                        'odchylenie odbiega od rozkladu jednostajnego')

    def test_rain_drops_scatter_across_width_not_one_column(self):
        """Regresja wprost na zglaszony objaw: krople maja byc rozrzucone po
        szerokosci, nie skupione w jednej kolumnie ('linia jak piorun')."""
        r = E.Rain(density=8.0, speed=1.1, tail=0.45)
        cols = set()
        for step in range(300):
            for dx, _head in r._drops(step * 0.033):
                cols.add(round(dx, 2))
        self.assertGreater(len(cols), 20, f'tylko {len(cols)} unikalnych kolumn')
        self.assertGreater(max(cols) - min(cols), 0.8,
                           'krople nie pokrywaja calej szerokosci')


class TestNewRainbowEffects(unittest.TestCase):
    """Trzy nowe tryby tęczowe: Plazma, Kolo teczy, Konfetti."""

    def setUp(self):
        self.lay = fake_layout()
        self.frame = Frame(self.lay.count)

    def test_registered_and_ordered(self):
        for name in ('plasma', 'wheel', 'confetti'):
            self.assertIn(name, E.REGISTRY)
            self.assertIn(name, [c.name for c in E.ORDER])

    def test_all_three_are_animated(self):
        for name in ('plasma', 'wheel', 'confetti'):
            self.assertTrue(E.REGISTRY[name].animated)

    def test_wheel_covers_full_hue_over_one_rotation(self):
        e = E.make('wheel', {'speed': 1.0})
        seen = set()
        for step in range(60):
            e.render(step / 60.0, self.lay, self.frame)
            seen.add(self.frame.rgb[0])
        self.assertGreater(len(seen), 20, 'kolo ma przechodzic przez wiele odcieni')

    def test_confetti_sparks_have_varied_hues(self):
        """Rozny odcien od Gwiazd: kazda iskra losuje wlasny kolor z widma,
        nie jeden staly kolor."""
        e = E.make('confetti', {'density': 1.0})
        colors = set()
        for step in range(40):
            e.render(step * 0.5, self.lay, self.frame)
            colors.update(c for c in self.frame.rgb if c != (0, 0, 0))
        self.assertGreater(len(colors), 100,
                           f'zbyt malo roznorodnosci: {len(colors)} kolorow')

    def test_plasma_is_smooth_not_flickering_per_frame(self):
        """Sasiednie klatki animacji nie moga wygladac losowo — plazma ma
        plynac, a nie migotac szumem."""
        e = E.make('plasma', {'speed': 0.1})
        e.render(10.0, self.lay, self.frame)
        a = list(self.frame.rgb)
        e.render(10.033, self.lay, self.frame)   # jedna klatka pozniej (33 ms)
        b = self.frame.rgb
        # roznica miedzy sasiednimi klatkami ma byc mala, nie skokowa
        diffs = [sum(abs(x - y) for x, y in zip(ca, cb)) for ca, cb in zip(a, b)]
        self.assertLess(max(diffs), 60, f'zbyt duzy skok miedzy klatkami: {max(diffs)}')


class TestReportFraming(unittest.TestCase):
    """Rozmiary raportow wg specyfikacji z dokumentacji sprzetowej."""

    def test_sizes(self):
        self.assertEqual(len(struct.pack('<BH', 2, 0)), 3)            # Report 2
        self.assertEqual(len(struct.pack('<BB', 6, 0)), 2)            # Report 6
        self.assertEqual(len(struct.pack('<BBHH', 5, 1, 0, 119) + bytes(4)), 10)
        self.assertEqual(len(struct.pack('<BBB', 4, 8, 1)
                             + struct.pack('<8H', *([0] * 8)) + bytes(32)), 51)


if __name__ == '__main__':
    unittest.main(verbosity=2)
