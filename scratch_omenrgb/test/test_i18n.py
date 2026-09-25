#!/usr/bin/env python3
"""Testy i18n — kompletnosc tlumaczen i neutralnosc jezykowa zapisywanego stanu.

Najwazniejszy test w tym pliku sprawdza katalog i18n.EFFECTS PRZECIW
prawdziwej deklaracji PARAMS w silniku (effects.py, reactive.py) — jesli ktos
doda nowy tryb albo parametr i zapomni dopisac tlumaczenia, ten test to
wychwyci od razu, zamiast zostawic angielskiego uzytkownika z polskim
tekstem (albo odwrotnie) gdzies w GUI.

    python3 test/test_i18n.py
"""

import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..'))

from omenkbd import i18n
from omenkbd.engine import effects as E
from omenkbd.engine import reactive as R


class TestLanguageSwitching(unittest.TestCase):
    def setUp(self):
        self._saved = i18n._current
        self._saved_pref = i18n._PREF_PATH

    def tearDown(self):
        i18n._current = self._saved
        i18n._PREF_PATH = self._saved_pref

    def test_set_and_get(self):
        i18n.set_language('en')
        self.assertEqual(i18n.get_language(), 'en')
        i18n.set_language('pl')
        self.assertEqual(i18n.get_language(), 'pl')

    def test_unknown_language_rejected(self):
        with self.assertRaises(ValueError):
            i18n.set_language('de')

    def test_persist_and_detect_roundtrip(self):
        with tempfile.TemporaryDirectory() as d:
            i18n._PREF_PATH = os.path.join(d, 'sub', 'language')
            i18n.set_language('en', persist=True)
            i18n._current = None                 # symuluj nowy proces
            os.environ.pop('OMEN_KBD_LANG', None)
            self.assertEqual(i18n._detect_language(), 'en')

    def test_env_var_overrides_saved_preference(self):
        with tempfile.TemporaryDirectory() as d:
            i18n._PREF_PATH = os.path.join(d, 'language')
            i18n.set_language('pl', persist=True)
            os.environ['OMEN_KBD_LANG'] = 'en'
            try:
                i18n._current = None
                self.assertEqual(i18n._detect_language(), 'en')
            finally:
                del os.environ['OMEN_KBD_LANG']

    def test_missing_pref_file_falls_back_gracefully(self):
        i18n._PREF_PATH = '/nie/ma/takiego/pliku/language'
        os.environ.pop('OMEN_KBD_LANG', None)
        # nie ma wyjatku, tylko dalsze przejscie do LANG/domyslnego
        self.assertIn(i18n._detect_language(), i18n.LANGUAGES)


class TestGenericStrings(unittest.TestCase):
    def test_every_string_has_both_languages(self):
        missing = []
        for key, entry in i18n.STRINGS.items():
            for lang in i18n.LANGUAGES:
                if not entry.get(lang):
                    missing.append((key, lang))
        self.assertEqual(missing, [], f'brakujace tlumaczenia: {missing}')

    def test_unknown_key_returns_key_not_crash(self):
        self.assertEqual(i18n.t('nie.ma.takiego.klucza'), 'nie.ma.takiego.klucza')

    def test_t_switches_with_language(self):
        i18n.set_language('pl')
        pl = i18n.t('window.brightness')
        i18n.set_language('en')
        en = i18n.t('window.brightness')
        self.assertNotEqual(pl, en)
        self.assertEqual(pl, 'Jasność')
        self.assertEqual(en, 'Brightness')

    def test_format_kwargs_applied(self):
        i18n.set_language('en')
        self.assertIn('42', i18n.t('window.lamps_at', n=42, dev='/dev/hidraw7'))


class TestEffectCatalogueCompleteness(unittest.TestCase):
    """To jest test regresyjny na dokladnie ten rodzaj bledu, ktory latwo
    popelnic przy dodawaniu efektu: dopisac PARAMS w silniku i zapomniec o
    i18n.py. Idzie PRZEZ PRAWDZIWA deklaracje w engine/effects.py, nie przez
    kopie listy nazw w tym pliku — nowy tryb jest wiec pokryty automatycznie."""

    def test_every_registered_effect_has_i18n_entry(self):
        missing = [name for name in E.REGISTRY if name not in i18n.EFFECTS]
        self.assertEqual(missing, [], f'brak wpisu i18n dla trybow: {missing}')

    def test_every_effect_label_has_both_languages(self):
        bad = []
        for name in E.REGISTRY:
            label = i18n.EFFECTS.get(name, {}).get('label', {})
            for lang in i18n.LANGUAGES:
                if not label.get(lang):
                    bad.append((name, lang))
        self.assertEqual(bad, [])

    def test_every_param_of_every_effect_has_i18n_label(self):
        missing = []
        for name, cls in E.REGISTRY.items():
            entry = i18n.EFFECTS.get(name, {})
            params = entry.get('params', {})
            for p in cls.PARAMS:
                labels = params.get(p.name)
                if labels is None:
                    missing.append((name, p.name, 'brak wpisu'))
                    continue
                for lang in i18n.LANGUAGES:
                    if not labels.get(lang):
                        missing.append((name, p.name, lang))
        self.assertEqual(missing, [], f'brakujace etykiety parametrow: {missing}')

    def test_every_choice_value_of_every_effect_is_translated(self):
        """Parametry typu 'choice' (np. Scanner.bounce) — kazda WARTOSC z
        deklaracji PARAMS musi miec etykiete w obu jezykach, inaczej combo w
        GUI pokaze surowy kod zamiast czytelnego tekstu."""
        missing = []
        for name, cls in E.REGISTRY.items():
            choices_map = i18n.EFFECTS.get(name, {}).get('choices', {})
            for p in cls.PARAMS:
                if p.kind != 'choice':
                    continue
                entry_choices = choices_map.get(p.name, {})
                for value in p.choices:
                    labels = entry_choices.get(value)
                    if labels is None:
                        missing.append((name, p.name, value, 'brak wpisu'))
                        continue
                    for lang in i18n.LANGUAGES:
                        if not labels.get(lang):
                            missing.append((name, p.name, value, lang))
        self.assertEqual(missing, [], f'brakujace etykiety opcji: {missing}')

    def test_axis_choices_cover_all_three_directions(self):
        for value in ('x', 'y', 'd'):
            for lang in i18n.LANGUAGES:
                self.assertTrue(i18n.AXIS_CHOICES[value].get(lang))


class TestReactiveCatalogueCompleteness(unittest.TestCase):
    def test_reactive_entry_exists(self):
        self.assertIn('reactive', i18n.EFFECTS)

    def test_every_reactive_param_translated(self):
        missing = []
        params = i18n.EFFECTS['reactive'].get('params', {})
        for p in R.PARAMS:
            labels = params.get(p.name)
            if labels is None:
                missing.append((p.name, 'brak wpisu'))
                continue
            for lang in i18n.LANGUAGES:
                if not labels.get(lang):
                    missing.append((p.name, lang))
        self.assertEqual(missing, [])

    def test_every_reactive_choice_translated(self):
        missing = []
        choices_map = i18n.EFFECTS['reactive'].get('choices', {})
        for p in R.PARAMS:
            if p.kind != 'choice':
                continue
            entry_choices = choices_map.get(p.name, {})
            for value in p.choices:
                labels = entry_choices.get(value)
                if labels is None:
                    missing.append((p.name, value, 'brak wpisu'))
                    continue
                for lang in i18n.LANGUAGES:
                    if not labels.get(lang):
                        missing.append((p.name, value, lang))
        self.assertEqual(missing, [])


class TestStateValuesAreLanguageNeutral(unittest.TestCase):
    """Rdzen calej sprawy: wartosci PARAMS typu 'choice' zapisywane w stanie
    NIE MOGA byc tlumaczonym tekstem, inaczej zmiana jezyka psuje stare
    profile. Sprawdzamy to wprost — kazda wartosc ma byc krotkim, ascii,
    jednowyrazowym (bez spacji) kodem."""

    def _assert_neutral(self, value):
        self.assertTrue(value.isascii(), f'{value!r} nie jest ascii')
        self.assertNotIn(' ', value, f'{value!r} zawiera spacje — to tekst, nie kod')
        self.assertEqual(value, value.lower(), f'{value!r} nie jest lowercase')

    def test_scanner_bounce_values_are_neutral(self):
        for v in E.REGISTRY['scanner'].PARAMS:
            if v.name == 'bounce':
                for choice in v.choices:
                    self._assert_neutral(choice)

    def test_reactive_curve_values_are_neutral(self):
        for p in R.PARAMS:
            if p.name == 'curve':
                for choice in p.choices:
                    self._assert_neutral(choice)

    def test_axis_values_are_neutral(self):
        for v in ('x', 'y', 'd'):
            self._assert_neutral(v)

    def test_old_polish_values_no_longer_appear_as_defaults(self):
        """Regresja wprost: to byl realny blad projektowy w tej samej sesji —
        'miekki'/'liniowy'/'tam i z powrotem' byly kiedys jednoczesnie stanem
        I tekstem interfejsu."""
        scanner_default = E.make('scanner').params['bounce']
        reactive_default = R.ReactiveOverlay().params['curve']
        for bad in ('miekki', 'liniowy', 'tam i z powrotem', 'w kolko'):
            self.assertNotEqual(scanner_default, bad)
            self.assertNotEqual(reactive_default, bad)


class TestChoiceLabelLookup(unittest.TestCase):
    def test_scanner_bounce_label_pl_en(self):
        i18n.set_language('pl')
        self.assertEqual(i18n.choice_label('scanner', 'bounce', 'bounce'),
                         'tam i z powrotem')
        i18n.set_language('en')
        self.assertEqual(i18n.choice_label('scanner', 'bounce', 'bounce'),
                         'back and forth')

    def test_unknown_choice_falls_back_to_value(self):
        self.assertEqual(i18n.choice_label('scanner', 'bounce', 'nieznana'),
                         'nieznana')

    def test_unknown_effect_falls_back_gracefully(self):
        self.assertEqual(i18n.effect_label('nie-ma-takiego', 'Domyslna'),
                         'Domyslna')
        self.assertEqual(i18n.param_label('nie-ma', 'x', 'Domyslna'), 'Domyslna')


if __name__ == '__main__':
    unittest.main(verbosity=2)
