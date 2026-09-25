#!/usr/bin/env python3
"""Testy geometrii canvasu klawiatury.

Wymagaja PySide6 i pomijaja sie, gdy go nie ma (np. w kontenerze bez Qt).
Sprawdzaja niezmienniki, ktore latwo zepsuc przy zmianie ukladu:
kazda lampka widoczna, klikalna i nie zaslonieta przez sasiada.

    python3 test/test_gui_geometry.py
"""

import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..'))
os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')

try:
    from PySide6.QtWidgets import QApplication
    HAVE_QT = True
except ImportError:
    HAVE_QT = False

# Prawdziwe dane z HP OMEN MAX 16: 6 rzedow po 20 lampek, rzedy poprzesuwane
# wzgledem siebie, Up i Down pod ta sama wspolrzedna.
ATTRS = {'lamp_count': 120, 'width_um': 342000, 'height_um': 125000,
         'depth_um': 1000, 'kind': 1, 'kind_name': 'Keyboard',
         'min_update_interval_us': 33000}
ROW_Y = [9000, 25000, 44000, 62000, 81000, 99000]


def sample_lamps():
    """Uklad odwzorowujacy istotne cechy prawdziwego: nierowne odstepy,
    rzedy nierownoodlegle, klawisze wielolampkowe, kolizja Up/Down."""
    lamps = []
    lid = 0
    for r, y in enumerate(ROW_Y):
        x = 12000 + r * 2000          # kazdy rzad przesuniety — to psulo mediane
        for c in range(20):
            key = f'K{r}_{c}'
            if r == 5 and c in (5, 6, 7):
                key = 'Space'         # klawisz z trzech lampek
            if r == 5 and c == 12:
                key = 'Down'
            if r == 5 and c == 13:
                key = 'Up'
                x = lamps[-1]['x_um']  # KOLIZJA: ta sama pozycja co Down
            if r == 0 and c == 1:
                key = None            # gola dioda bez klawisza
            lamps.append({'id': lid, 'x_um': x, 'y_um': y, 'z_um': 0,
                          'input_binding': 4, 'key': key,
                          'programmable': True, 'purposes': ['Control']})
            lid += 1
            x += 18000 if c % 3 else 17000
    return lamps


@unittest.skipUnless(HAVE_QT, 'brak PySide6')
class TestCanvasGeometry(unittest.TestCase):
    app = None

    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        from omenkbd.gui.keyboard import KeyboardView
        self.view = KeyboardView()
        self.lamps = sample_lamps()
        self.view.set_layout(self.lamps, ATTRS)

    def test_every_lamp_has_an_item(self):
        self.assertEqual(len(self.view.items_by_lamp), len(self.lamps))

    def test_multi_lamp_key_is_one_item(self):
        space = {id(self.view.items_by_lamp[l['id']])
                 for l in self.lamps if l['key'] == 'Space'}
        self.assertEqual(len(space), 1, 'Spacja musi byc jednym klawiszem')

    def test_colliding_lamps_get_separate_items(self):
        """Up i Down pod ta sama wspolrzedna — bez podzialu komorki jedna
        rysowalaby sie na drugiej i stawala nieklikalna."""
        up = next(l for l in self.lamps if l['key'] == 'Up')
        down = next(l for l in self.lamps if l['key'] == 'Down')
        a = self.view.items_by_lamp[up['id']]
        b = self.view.items_by_lamp[down['id']]
        self.assertIsNot(a, b)
        overlap = a._rect.intersected(b._rect)
        self.assertTrue(overlap.width() <= 0.5 or overlap.height() <= 0.5,
                        'podzielone komorki nie moga na siebie zachodzic')
        self.assertLess(a._rect.top(), b._rect.top(), 'Up ma byc nad Down')

    def test_no_two_items_overlap(self):
        ks = self.view.keys
        bad = []
        for i in range(len(ks)):
            for j in range(i + 1, len(ks)):
                r = ks[i]._rect.intersected(ks[j]._rect)
                if r.width() > 0.5 and r.height() > 0.5:
                    bad.append((ks[i].label, ks[j].label))
        self.assertEqual(bad, [], f'prostokaty zachodza na siebie: {bad[:5]}')

    def test_keys_are_not_slivers(self):
        """Regresja: naiwne liczenie rozstawu z unii wszystkich X dawalo
        1 mm zamiast 18 mm i cala klawiatura wychodzila jako paski."""
        widths = [k._rect.width() for k in self.view.keys if k.is_key]
        self.assertGreater(min(widths), 5.0, 'klawisze wyszly jako paski')

    def test_unbound_lamps_render_smaller(self):
        led = next(l for l in self.lamps if l['key'] is None)
        item = self.view.items_by_lamp[led['id']]
        self.assertFalse(item.is_key)
        key_h = max(k._rect.height() for k in self.view.keys if k.is_key)
        self.assertLess(item._rect.height(), key_h)

    def test_labels_never_come_out_empty(self):
        for it in self.view.keys:
            if not it.is_key:
                continue
            text, size = it._fit(it._rect.width() - 3, it._rect.height())
            self.assertTrue(text, f'pusty podpis dla {it.label!r}')
            self.assertGreaterEqual(size, 4)

    def test_colors_reach_every_key(self):
        colors = {l['id']: (10, 200, 30) for l in self.lamps}
        self.view.set_colors(colors)
        for it in self.view.keys:
            self.assertEqual((it.color.red(), it.color.green(), it.color.blue()),
                             (10, 200, 30))

    def test_selection_returns_all_lamps_of_a_key(self):
        space_ids = [l['id'] for l in self.lamps if l['key'] == 'Space']
        item = self.view.items_by_lamp[space_ids[0]]
        item.selected = True
        self.assertEqual(sorted(self.view.selected_lamps()), sorted(space_ids))

    def test_empty_layout_does_not_crash(self):
        from omenkbd.gui.keyboard import KeyboardView
        v = KeyboardView()
        v.set_layout([], ATTRS)
        self.assertEqual(v.keys, [])


@unittest.skipUnless(HAVE_QT, 'brak PySide6')
class TestPanelsFromSpec(unittest.TestCase):
    """GUI buduje kontrolki z deklaracji PARAMS w silniku. Ten test pilnuje, ze
    dodanie efektu nie wymaga tkniecia interfejsu i ze wartosci wracaja z paneli
    dokladnie takie, jakie silnik przyjmuje."""

    app = None

    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def test_panel_builds_for_every_effect(self):
        from omenkbd.engine import effects as E
        from omenkbd.gui.panels import make_panel
        for spec in E.catalogue():
            with self.subTest(effect=spec['name']):
                panel = make_panel(spec)
                self.assertEqual(len(panel.widgets), len(spec['params']))

    def test_panel_defaults_match_engine_defaults(self):
        from omenkbd.engine import effects as E
        from omenkbd.gui.panels import make_panel
        for spec in E.catalogue():
            with self.subTest(effect=spec['name']):
                got = make_panel(spec).params()
                want = E.make(spec['name']).describe()
                for p in spec['params']:
                    a, b = got[p['name']], want[p['name']]
                    if p['kind'] == 'float':
                        self.assertAlmostEqual(a, b, places=3, msg=p['name'])
                    else:
                        self.assertEqual(a, b, p['name'])

    def test_panel_output_is_accepted_by_engine(self):
        from omenkbd.engine import effects as E
        from omenkbd.gui.panels import make_panel
        for spec in E.catalogue():
            with self.subTest(effect=spec['name']):
                E.from_dict(make_panel(spec).params())

    def test_load_then_params_roundtrip(self):
        """Okno wczytuje stan z demona do panelu i odsyla go z powrotem —
        po drodze nic nie moze sie zgubic ani zaokraglic."""
        from omenkbd.engine import effects as E
        from omenkbd.gui.panels import make_panel
        for spec in E.catalogue():
            with self.subTest(effect=spec['name']):
                want = E.make(spec['name']).describe()
                panel = make_panel(spec)
                panel.load(want)
                got = panel.params()
                for p in spec['params']:
                    a, b = got[p['name']], want[p['name']]
                    if p['kind'] == 'float':
                        self.assertAlmostEqual(a, b, places=3, msg=p['name'])
                    else:
                        self.assertEqual(a, b, p['name'])

    def test_perkey_panel_keeps_painted_colors(self):
        from omenkbd.engine import effects as E
        from omenkbd.gui.panels import make_panel
        spec = next(c for c in E.catalogue() if c['name'] == 'perkey')
        panel = make_panel(spec)
        panel.paint([3, 4, 5], '#FF0000')
        self.assertEqual(panel.params()['colors'],
                         {'3': '#FF0000', '4': '#FF0000', '5': '#FF0000'})
        E.from_dict(panel.params())


@unittest.skipUnless(HAVE_QT, 'brak PySide6')
class TestRetranslate(unittest.TestCase):
    """Regresja: retranslate() (wolane przy zmianie jezyka w trayu) buduje
    NOWY KeyboardView w _build(), ale refresh() odtwarza uklad tylko gdy
    self.layout_model is None. Bez skasowania go po _build() nowy widok
    zostawal pusty — przelaczenie jezyka gasilo caly podglad klawiatury."""

    app = None

    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def test_retranslate_repopulates_the_new_view(self):
        from omenkbd.client import Client
        from omenkbd.gui.window import MainWindow

        class FakeClient(Client):
            def __init__(self):
                pass

            def call(self, cmd, **kw):
                if cmd == 'status':
                    return {'connected': True, 'lamp_count': 3,
                            'device': {'node': '/dev/hidraw0'},
                            'effect': {'effect': 'static', 'color': '#FFFFFF'},
                            'brightness': 200, 'profile': None,
                            'control': 'app', 'reactive': {'enabled': False,
                                                          'available': False,
                                                          'params': {}}}
                if cmd == 'layout':
                    lamps = [{'id': i, 'x_um': i * 18000, 'y_um': 0, 'z_um': 0,
                             'input_binding': 4, 'key': None,
                             'programmable': True, 'purposes': []}
                            for i in range(3)]
                    return {'lamps': lamps,
                           'attrs': {'width_um': 36000, 'height_um': 18000}}
                if cmd == 'profile.list':
                    return {'profiles': [], 'current': None}
                return {}

            def try_call(self, cmd, **kw):
                return self.call(cmd, **kw)

        w = MainWindow(FakeClient())
        self.assertEqual(len(w.view.keys), 3, 'podglad nie zaladowal sie na starcie')

        old_view = w.view
        w.retranslate()

        self.assertIsNot(w.view, old_view, '_build() ma tworzyc nowy widok')
        self.assertEqual(len(w.view.keys), 3,
                         'PO retranslate() nowy widok zostal pusty — to jest ten blad')


if __name__ == '__main__':
    unittest.main(verbosity=2)
