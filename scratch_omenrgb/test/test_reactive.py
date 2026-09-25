#!/usr/bin/env python3
"""Testy reactive typing — BEZ urzadzenia i BEZ prawdziwej klawiatury.

Sprawdzaja: parser surowych zdarzen evdev, poprawnosc tablicy evdev->HID,
matematyke zaniku blysku, oraz — co najwazniejsze — ze demon prawidlowo
wplata reactive w harmonogram klatek i nigdy nie wystawia surowych nacisniec
przez gniazdo (kanal boczny).

    python3 test/test_reactive.py
"""

import os
import struct
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..'))

from omenkbd.core import color as C
from omenkbd.core.evdev_map import EVDEV_TO_HID, HID_TO_EVDEV, hid_usage_for
from omenkbd.core.hidkeys import key_name
from omenkbd.core.inputwatch import (EVENT_SIZE, codes_to_hid,
                                     filter_copilot_burst, parse_events)
from omenkbd.engine.frame import Frame
from omenkbd.engine.reactive import PARAMS, ReactiveOverlay
from test_omenkbd import fake_layout


def pack_event(typ, code, val):
    return struct.pack('<qqHHi', 0, 0, typ, code, val)


class TestEvdevMap(unittest.TestCase):
    def test_table_is_a_true_bijection_where_defined(self):
        self.assertEqual(len(EVDEV_TO_HID), len(HID_TO_EVDEV),
                         'dwa HID usages nie moga dzielic jednego kodu evdev')

    def test_spot_checks_against_hidkeys_names(self):
        # (kod evdev, oczekiwana nazwa z hidkeys.key_name)
        pairs = [
            (17, 'W'), (30, 'A'), (57, 'Space'), (28, 'Enter'), (1, 'Esc'),
            (42, 'LShift'), (54, 'RShift'), (29, 'LCtrl'), (97, 'RCtrl'),
            (56, 'LAlt'), (100, 'RAlt'), (125, 'LMeta'), (126, 'RMeta'),
            (103, 'Up'), (108, 'Down'), (105, 'Left'), (106, 'Right'),
            (58, 'CapsLock'), (14, 'Backspace'), (15, 'Tab'),
            (59, 'F1'), (68, 'F10'), (87, 'F11'), (88, 'F12'),
        ]
        for code, expect in pairs:
            with self.subTest(code=code, expect=expect):
                usage = hid_usage_for(code)
                self.assertIsNotNone(usage, f'brak wpisu dla evdev {code}')
                self.assertEqual(key_name(usage), expect)

    def test_unknown_code_returns_none_not_raises(self):
        self.assertIsNone(hid_usage_for(99999))


class TestEventParsing(unittest.TestCase):
    def test_parses_press_and_release(self):
        raw = pack_event(1, 17, 1) + pack_event(1, 17, 0)
        ev = parse_events(raw)
        self.assertEqual(ev, [(1, 17, 1), (1, 17, 0)])

    def test_incomplete_tail_is_dropped_not_crashed(self):
        raw = pack_event(1, 17, 1) + b'\x01\x02\x03'
        ev = parse_events(raw)
        self.assertEqual(ev, [(1, 17, 1)])

    def test_empty_buffer(self):
        self.assertEqual(parse_events(b''), [])

    def test_codes_to_hid_skips_unmapped(self):
        self.assertEqual(codes_to_hid([17, 999999, 57]),
                         [hid_usage_for(17), hid_usage_for(57)])

    def test_event_size_matches_struct(self):
        self.assertEqual(EVENT_SIZE, 24, 'rozmiar struct input_event na x86_64')


class TestCopilotKeyFilter(unittest.TestCase):
    """Klawisz Copilot na HP OMEN MAX 16 (8D41) nie ma wlasnego kodu HID —
    firmware emuluje go skrotem LMeta+LShift+F23 (kompatybilnosciowy fallback
    sprzed natywnego wsparcia systemow dla tego klawisza). Zweryfikowane
    empirycznie: nacisniecie samego Copilota daje paczke evdev [125, 42, 193].
    Bez filtra reactive typing zapalaloby lampki Shift i Windows zamiast nic."""

    def test_real_captured_burst_strips_shift_and_meta(self):
        # dokladnie ta paczka, zaobserwowana na prawdziwym sprzecie
        self.assertEqual(filter_copilot_burst([125, 42, 193]), [193])

    def test_order_and_duplicates_do_not_matter(self):
        self.assertEqual(sorted(filter_copilot_burst([193, 42, 125, 42])), [193])

    def test_plain_shift_alone_is_untouched(self):
        self.assertEqual(filter_copilot_burst([42]), [42])

    def test_plain_meta_alone_is_untouched(self):
        self.assertEqual(filter_copilot_burst([125]), [125])

    def test_shift_and_meta_together_without_sentinel_untouched(self):
        """Prawdziwy Shift+Win (np. skrot Windows) nie ma byc filtrowany —
        sygnalem jest WYLACZNIE obecnosc F23, nie sama kombinacja klawiszy."""
        self.assertEqual(sorted(filter_copilot_burst([42, 125])), [42, 125])

    def test_normal_typing_is_never_touched(self):
        self.assertEqual(filter_copilot_burst([17, 30, 31]), [17, 30, 31])

    def test_sentinel_with_unrelated_keys_only_strips_phantom_pair(self):
        """Gdyby paczka z F23 zawierala tez cos innego, ma zostac tylko to
        inne — filtr nie ma gasic calej paczki, tylko fantomowa pare."""
        self.assertEqual(sorted(filter_copilot_burst([125, 42, 193, 17])),
                         [17, 193])


class TestReactiveOverlay(unittest.TestCase):
    def setUp(self):
        self.lay = fake_layout()
        self.frame = Frame(self.lay.count)

    def test_fresh_press_is_full_strength(self):
        ov = ReactiveOverlay(color='#FF0000', decay=1.0, curve='linear')
        self.frame.fill((0, 0, 0))
        ov.press([5], 10.0)
        ov.apply(self.frame, self.lay, 10.0)
        self.assertEqual(self.frame.rgb[5], (255, 0, 0))

    def test_linear_decay_at_midpoint(self):
        ov = ReactiveOverlay(color='#FF0000', decay=1.0, curve='linear')
        self.frame.fill((0, 0, 0))
        ov.press([5], 10.0)
        ov.apply(self.frame, self.lay, 10.5)
        r, g, b = self.frame.rgb[5]
        self.assertAlmostEqual(r, 127, delta=2)

    def test_fully_decayed_is_untouched(self):
        ov = ReactiveOverlay(color='#FF0000', decay=0.5, curve='linear')
        self.frame.fill((9, 9, 9))
        ov.press([5], 10.0)
        ov.apply(self.frame, self.lay, 10.5)
        self.assertEqual(self.frame.rgb[5], (9, 9, 9))

    def test_soft_curve_differs_from_linear(self):
        lin = ReactiveOverlay(color='#FFFFFF', decay=1.0, curve='linear')
        soft = ReactiveOverlay(color='#FFFFFF', decay=1.0, curve='soft')
        for ov in (lin, soft):
            self.frame.fill((0, 0, 0))
            ov.press([0], 10.0)
        f1, f2 = Frame(self.lay.count), Frame(self.lay.count)
        f1.fill((0, 0, 0)); f2.fill((0, 0, 0))
        lin.apply(f1, self.lay, 10.25)
        soft.apply(f2, self.lay, 10.25)
        self.assertNotEqual(f1.rgb[0], f2.rgb[0])

    def test_does_not_touch_other_lamps(self):
        ov = ReactiveOverlay(color='#FF0000', decay=1.0)
        self.frame.fill((3, 3, 3))
        ov.press([5], 10.0)
        ov.apply(self.frame, self.lay, 10.0)
        self.assertEqual(self.frame.rgb[6], (3, 3, 3))

    def test_multiple_keys_of_one_lamp_group(self):
        """Jednemu klawiszowi (np. Spacji) odpowiada czasem kilka lampek —
        jedno nacisniecie ma zapalic wszystkie naraz."""
        ov = ReactiveOverlay(color='#FFFFFF', decay=1.0)
        ids = self.lay.resolve('A')
        self.assertGreater(len(ids), 1)
        self.frame.fill((0, 0, 0))
        ov.press(ids, 10.0)
        ov.apply(self.frame, self.lay, 10.0)
        for i in ids:
            self.assertNotEqual(self.frame.rgb[i], (0, 0, 0))

    def test_repeated_press_refreshes_timer(self):
        ov = ReactiveOverlay(color='#FF0000', decay=1.0, curve='linear')
        ov.press([5], 10.0)
        ov.press([5], 10.9)             # powtorka tuz przed zgasnieciem
        self.frame.fill((0, 0, 0))
        ov.apply(self.frame, self.lay, 10.95)
        r, _, _ = self.frame.rgb[5]
        self.assertGreater(r, 200, 'powtorka mial odswiezyc jasnosc blysku')

    def test_stale_entries_are_purged(self):
        ov = ReactiveOverlay(decay=0.1)
        ov.press([5], 10.0)
        ov.apply(self.frame, self.lay, 20.0)   # dawno po zaniku
        self.assertFalse(ov.has_pending())

    def test_clear_wipes_everything(self):
        ov = ReactiveOverlay()
        ov.press([1, 2, 3], 10.0)
        ov.clear()
        self.assertFalse(ov.has_pending())

    def test_out_of_range_params_are_clamped(self):
        ov = ReactiveOverlay(decay=999.0)
        self.assertLessEqual(ov.decay, 3.0)

    def test_bad_curve_falls_back_to_default(self):
        ov = ReactiveOverlay(curve='na ukos')
        self.assertEqual(ov.params['curve'], 'soft')

    def test_describe_roundtrip(self):
        ov = ReactiveOverlay(color='#112233', decay=0.4, curve='linear')
        again = ReactiveOverlay(**ov.describe())
        self.assertEqual(again.describe(), ov.describe())

    def test_params_declaration_matches_constructor(self):
        names = {p.name for p in PARAMS}
        self.assertEqual(names, {'color', 'decay', 'curve', 'intensity'})

    def test_intensity_caps_peak_strength(self):
        """Moc blysku to OSOBNA os regulacji od globalnej Jasnosci — steruje,
        jak mocno kolor blysku miesza sie z tlem w szczycie, nie tylko jak
        szybko gasnie."""
        ov = ReactiveOverlay(color='#FFFFFF', decay=1.0, curve='linear',
                             intensity=0.4)
        self.frame.fill((0, 0, 0))
        ov.press([5], 10.0)
        ov.apply(self.frame, self.lay, 10.0)      # swiezy blysk, szczyt krzywej
        r, g, b = self.frame.rgb[5]
        self.assertAlmostEqual(r, 102, delta=2)   # 0.4 * 255 ≈ 102
        self.assertAlmostEqual(g, 102, delta=2)
        self.assertAlmostEqual(b, 102, delta=2)

    def test_full_intensity_reaches_full_color(self):
        ov = ReactiveOverlay(color='#FF0000', decay=1.0, intensity=1.0)
        self.frame.fill((0, 0, 0))
        ov.press([5], 10.0)
        ov.apply(self.frame, self.lay, 10.0)
        self.assertEqual(self.frame.rgb[5], (255, 0, 0))

    def test_intensity_out_of_range_is_clamped(self):
        ov = ReactiveOverlay(intensity=5.0)
        self.assertLessEqual(ov.intensity, 1.0)
        ov2 = ReactiveOverlay(intensity=-1.0)
        self.assertGreaterEqual(ov2.intensity, 0.05)


class TestFrameBudget(unittest.TestCase):
    def test_apply_is_cheap_with_many_pending(self):
        import time
        lay = fake_layout()
        frame = Frame(lay.count)
        ov = ReactiveOverlay()
        for i in range(lay.count):
            ov.press([lay.ids[i]], 0.0)
        t0 = time.perf_counter()
        for _ in range(200):
            ov.apply(frame, lay, 0.1)
        ms = (time.perf_counter() - t0) / 200 * 1000
        self.assertLess(ms, 2.0, f'{ms:.3f} ms na klatke z pelna klawiatura blyskow')


if __name__ == '__main__':
    unittest.main(verbosity=2)
