"""Klient CLI. Nie dotyka urzadzenia — rozmawia z demonem po gniezdzie unixowym.

Dzieki temu 'omen-kbd all 00FF88' to jedna linijka JSON do gotowego procesu,
a nie otwieranie hidraw i czytanie mapy. Stad ponizej 100 ms razem ze startem
interpretera.
"""

import argparse
import json
import sys

from . import i18n
from .client import Client, DaemonError, NoDaemon
from .core import color as C


def send(req, args):
    """Wysyla surowe zadanie i konczy program czytelnym bledem, gdy demon odmowil."""
    req = dict(req)
    cmd = req.pop('cmd')
    try:
        return Client(path=args.socket).call(cmd, **req)
    except DaemonError as e:
        print(i18n.t('cli.error.generic'), e, file=sys.stderr)
        raise SystemExit(1) from None


# ---------- komendy ----------

def c_status(a):
    r = send({'cmd': 'status'}, a)
    if a.json:
        print(json.dumps(r, indent=2, ensure_ascii=False))
        return
    dev = r.get('device') or {}
    at = r.get('attrs') or {}
    print(f"{i18n.t('cli.status.device'):<11}: {dev.get('node', '—')}  {dev.get('name', '')}")
    state = i18n.t('cli.status.connected') if r['connected'] else i18n.t('cli.status.none')
    print(f"{i18n.t('cli.status.state'):<11}: {state}"
          f"{i18n.t('cli.status.released_suffix') if r['released'] else ''}")
    if at:
        print(f"{i18n.t('cli.status.lamps'):<11}: {r['lamp_count']}  "
              f"({at['width_um']/1000:.0f} x {at['height_um']/1000:.0f} mm)  "
              f"max {r['fps_cap']} kl./s")
    eff = r.get('effect') or {}
    name = eff.get('effect', '?')
    label = i18n.effect_label(name, name)
    extra = ' '.join(f'{k}={v}' for k, v in sorted(eff.items())
                     if k != 'effect' and k != 'colors')
    print(f"{i18n.t('cli.status.effect'):<11}: {label} {extra}".rstrip())
    print(f"{i18n.t('cli.status.brightness'):<11}: {r['brightness']}/255")
    if r.get('profile'):
        print(f"{i18n.t('cli.status.profile'):<11}: {r['profile']}")
    rx = r.get('reactive') or {}
    if rx.get('enabled'):
        rp = rx.get('params', {})
        avail = '' if rx.get('available') else i18n.t('cli.status.reactive_no_access')
        print(i18n.t('cli.status.reactive_on', color=rp.get('color'),
                     decay=rp.get('decay'), avail=avail))
    st = r.get('stats') or {}
    print(f"{i18n.t('cli.status.counters'):<11}: " + i18n.t(
        'cli.status.counters_line', frames=st.get('frames', 0),
        reports=st.get('reports', 0), reconnects=st.get('reconnects', 0)))


def c_all(a):
    send({'cmd': 'set', 'params': {'effect': 'static', 'color': a.color},
          'brightness': a.brightness}, a)


def c_off(a):
    send({'cmd': 'set', 'params': {'effect': 'off'}}, a)


def c_gradient(a):
    send({'cmd': 'set', 'params': {'effect': 'gradient', 'color': a.color,
                                   'color2': a.color2, 'axis': a.axis},
          'brightness': a.brightness}, a)


def c_wave(a):
    send({'cmd': 'set', 'params': {'effect': 'wave', 'speed': a.speed,
                                   'spread': a.spread, 'axis': a.axis},
          'brightness': a.brightness}, a)


def c_breathe(a):
    send({'cmd': 'set', 'params': {'effect': 'breathe', 'color': a.color,
                                   'period': a.period},
          'brightness': a.brightness}, a)


def c_preset(a):
    send({'cmd': 'preset', 'name': a.name}, a)
    if a.brightness is not None:
        send({'cmd': 'brightness', 'value': a.brightness}, a)


def c_keys(a):
    names = [n.strip() for n in a.names.split(',') if n.strip()]
    send({'cmd': 'keys.set', 'names': names, 'color': a.color, 'base': a.base}, a)
    if a.brightness is not None:
        send({'cmd': 'brightness', 'value': a.brightness}, a)


def c_brightness(a):
    r = send({'cmd': 'brightness', 'value': a.value}, a)
    print(i18n.t('cli.brightness_result', v=r['brightness']))


def c_release(a):
    send({'cmd': 'control', 'owner': 'bios'}, a)
    print(i18n.t('cli.control_bios_msg'))


def c_control(a):
    r = send({'cmd': 'control', 'owner': a.owner}, a)
    print(i18n.t('cli.control_result_bios') if r['control'] == 'bios'
          else i18n.t('cli.control_result_app'))


def c_effect(a):
    """Generyczne wlaczenie dowolnego trybu: omen-kbd effect fire speed=3"""
    params = {'effect': a.name}
    for item in a.settings:
        if '=' not in item:
            print(i18n.t('cli.error.bad_param', v=item), file=sys.stderr)
            raise SystemExit(1)
        k, v = item.split('=', 1)
        try:
            params[k] = float(v) if v.replace('.', '', 1).replace(
                '-', '', 1).isdigit() else v
        except ValueError:
            params[k] = v
    send({'cmd': 'set', 'params': params, 'brightness': a.brightness}, a)


def c_resume(a):
    send({'cmd': 'resume'}, a)


def c_reactive(a):
    if a.action == 'status':
        r = send({'cmd': 'status'}, a)
        rx = r.get('reactive', {})
        if a.json:
            print(json.dumps(rx, indent=2, ensure_ascii=False))
            return
        yn = lambda v: i18n.t('cli.reactive.yes') if v else i18n.t('cli.reactive.no')
        print(f"{i18n.t('cli.reactive.enabled'):<10}: {yn(rx.get('enabled'))}")
        hint = '' if rx.get('available') or not rx.get('enabled') \
            else i18n.t('cli.reactive.no_access_hint')
        print(f"{i18n.t('cli.reactive.available'):<10}: {yn(rx.get('available'))}{hint}")
        p = rx.get('params', {})
        print(f"{i18n.t('cli.reactive.color'):<10}: {p.get('color')}")
        print(f"{i18n.t('cli.reactive.decay'):<10}: {p.get('decay')} s, "
             f"{i18n.choice_label('reactive', 'curve', p.get('curve'))}")
        return
    if a.action == 'on':
        r = send({'cmd': 'reactive', 'enable': True}, a)
        if not r.get('available'):
            print(i18n.t('cli.reactive.on_no_access'), file=sys.stderr)
        else:
            print(i18n.t('cli.reactive.on_ok'))
        return
    if a.action == 'off':
        send({'cmd': 'reactive', 'enable': False}, a)
        print(i18n.t('cli.reactive.off_ok'))
        return
    if a.action == 'set':
        params = {}
        for item in a.settings:
            if '=' not in item:
                print(i18n.t('cli.error.bad_param', v=item), file=sys.stderr)
                raise SystemExit(1)
            k, v = item.split('=', 1)
            params[k] = v
        send({'cmd': 'reactive', 'params': params}, a)
        print(i18n.t('cli.reactive.set_ok'))


def c_map(a):
    r = send({'cmd': 'layout'}, a)
    if a.json:
        print(json.dumps(r, indent=1, ensure_ascii=False))
        return
    rows = {}
    for l in r['lamps']:
        rows.setdefault(l['y_um'], []).append(l)
    for y in sorted(rows):
        cells = ' '.join(f"{l['id']}:{l['key'] or '—'}"
                         for l in sorted(rows[y], key=lambda x: x['id']))
        print(f'y={y:>6} ({len(rows[y])})  {cells}')


def c_keylist(a):
    r = send({'cmd': 'keys'}, a)
    for k in sorted(r['keys']):
        print(f"{k:<12} {r['keys'][k]}")


def c_effects(a):
    r = send({'cmd': 'effects'}, a)
    if a.json:
        print(json.dumps(r, indent=2, ensure_ascii=False))
        return
    for c in r.get('catalogue', []):
        anim = i18n.t('cli.effects.animated') if c['animated'] else i18n.t('cli.effects.static')
        label = i18n.effect_label(c['name'], c['label'])
        print(f"{c['name']:<10} {label:<18} {anim}")
        for p in c['params']:
            plabel = i18n.param_label(c['name'], p['name'], p['label'])
            if p['kind'] == 'color':
                rng = i18n.t('cli.effects.color_kind')
            elif p['kind'] == 'axis':
                rng = '|'.join(i18n.axis_label(v) for v in ('x', 'y', 'd'))
            elif p['kind'] == 'choice':
                rng = '|'.join(i18n.choice_label(c['name'], p['name'], v)
                               for v in p['choices'])
            else:
                rng = f"{p['lo']:g}..{p['hi']:g}"
            print(f"    {p['name']:<12} ({plabel}) {rng:<26} "
                 f"{i18n.t('cli.effects.default')} {p['default']}")
    print(f"\n{i18n.t('cli.effects.presets')}:", ', '.join(r['presets']))


def c_profile(a):
    if a.action == 'list':
        r = send({'cmd': 'profile.list'}, a)
        for p in r['profiles']:
            print(('* ' if p == r.get('current') else '  ') + p)
        if not r['profiles']:
            print(i18n.t('cli.profile.empty'))
    elif a.action == 'save':
        send({'cmd': 'profile.save', 'name': a.name}, a)
        print(i18n.t('cli.profile.saved', name=a.name))
    elif a.action == 'load':
        send({'cmd': 'profile.load', 'name': a.name}, a)
    elif a.action == 'delete':
        send({'cmd': 'profile.delete', 'name': a.name}, a)
        print(i18n.t('cli.profile.deleted', name=a.name))


def c_lang(a):
    i18n.set_language(a.value, persist=True)
    print(i18n.t('cli.lang.set', lang=a.value))


def build_parser():
    p = argparse.ArgumentParser(
        prog='omen-kbd',
        description=i18n.t('cli.prog_desc'))
    p.add_argument('--socket', default=None, help=i18n.t('cli.help.socket'))
    p.add_argument('--json', action='store_true', help=i18n.t('cli.help.json'))
    p.add_argument('--lang', choices=list(i18n.LANGUAGES), default=None,
                   help=i18n.t('cli.help.lang'))
    p.add_argument('-b', '--brightness', type=int, default=None,
                   help=i18n.t('cli.help.brightness_top'))
    sub = p.add_subparsers(dest='cmd', required=True)

    # Ten sam -b doklejony do podkomend, zeby dzialal po OBU stronach podkomendy:
    # "omen-kbd -b 120 all teal" i "omen-kbd all teal -b 120". SUPPRESS jest
    # istotny — bez niego domyslne None z podparsera nadpisaloby wartosc podana
    # przed podkomenda.
    bright = argparse.ArgumentParser(add_help=False)
    bright.add_argument('-b', '--brightness', type=int, default=argparse.SUPPRESS,
                        help=i18n.t('cli.help.brightness'))

    sub.add_parser('status', help=i18n.t('cli.help.status')).set_defaults(fn=c_status)
    sub.add_parser('map', help=i18n.t('cli.help.map')).set_defaults(fn=c_map)
    sub.add_parser('keylist', help=i18n.t('cli.help.keylist')).set_defaults(fn=c_keylist)
    sub.add_parser('effects', help=i18n.t('cli.help.effects')).set_defaults(fn=c_effects)
    sub.add_parser('off', help=i18n.t('cli.help.off')).set_defaults(fn=c_off)
    sub.add_parser('release', help=i18n.t('cli.help.release')).set_defaults(fn=c_release)

    s = sub.add_parser('control', help=i18n.t('cli.help.control'))
    s.add_argument('owner', choices=['bios', 'app'])
    s.set_defaults(fn=c_control)

    s = sub.add_parser('effect', parents=[bright], help=i18n.t('cli.help.effect'))
    s.add_argument('name')
    s.add_argument('settings', nargs='*', metavar='klucz=wartosc')
    s.set_defaults(fn=c_effect)
    sub.add_parser('resume', help=i18n.t('cli.help.resume')).set_defaults(fn=c_resume)

    s = sub.add_parser('reactive', help=i18n.t('cli.help.reactive'))
    s.add_argument('action', choices=['on', 'off', 'set', 'status'])
    s.add_argument('settings', nargs='*', metavar='klucz=wartosc',
                   help=i18n.t('cli.help.reactive_settings'))
    s.set_defaults(fn=c_reactive)

    s = sub.add_parser('lang', help=i18n.t('cli.help.lang'))
    s.add_argument('value', choices=list(i18n.LANGUAGES))
    s.set_defaults(fn=c_lang)

    s = sub.add_parser('all', parents=[bright], help=i18n.t('cli.help.all'))
    s.add_argument('color')
    s.set_defaults(fn=c_all)

    s = sub.add_parser('gradient', parents=[bright], help=i18n.t('cli.help.gradient'))
    s.add_argument('color')
    s.add_argument('color2')
    s.add_argument('--axis', choices=['x', 'y', 'd'], default='x')
    s.set_defaults(fn=c_gradient)

    s = sub.add_parser('wave', parents=[bright], help=i18n.t('cli.help.wave'))
    s.add_argument('--speed', type=float, default=0.15)
    s.add_argument('--spread', type=float, default=1.0)
    s.add_argument('--axis', choices=['x', 'y', 'd'], default='x')
    s.set_defaults(fn=c_wave)

    s = sub.add_parser('breathe', parents=[bright], help=i18n.t('cli.help.breathe'))
    s.add_argument('color', nargs='?', default='#00FFC0')
    s.add_argument('--period', type=float, default=4.0)
    s.set_defaults(fn=c_breathe)

    s = sub.add_parser('preset', parents=[bright], help=i18n.t('cli.help.preset'))
    s.add_argument('name')
    s.set_defaults(fn=c_preset)

    s = sub.add_parser('keys', parents=[bright], help=i18n.t('cli.help.keys'))
    s.add_argument('names', help=i18n.t('cli.help.keys_names'))
    s.add_argument('color')
    s.add_argument('--base', default='#000000', help=i18n.t('cli.help.keys_base'))
    s.set_defaults(fn=c_keys)

    s = sub.add_parser('brightness', help=i18n.t('cli.help.brightness_cmd'))
    s.add_argument('value', type=int)
    s.set_defaults(fn=c_brightness)

    s = sub.add_parser('profile', help=i18n.t('cli.help.profile'))
    s.add_argument('action', choices=['list', 'save', 'load', 'delete'])
    s.add_argument('name', nargs='?')
    s.set_defaults(fn=c_profile)
    return p


def _prescan_lang(argv):
    """Ustala jezyk PRZED zbudowaniem parsera, zeby tez --help byl w tym
    jezyku, nie tylko wyjscie samej komendy."""
    for i, tok in enumerate(argv):
        if tok == '--lang' and i + 1 < len(argv):
            return argv[i + 1]
        if tok.startswith('--lang='):
            return tok.split('=', 1)[1]
    return None


def main(argv=None):
    raw = sys.argv[1:] if argv is None else list(argv)
    lang = _prescan_lang(raw)
    if lang in i18n.LANGUAGES:
        i18n.set_language(lang)

    a = build_parser().parse_args(argv)
    if not hasattr(a, 'brightness'):
        a.brightness = None
    if getattr(a, 'action', None) in ('save', 'load', 'delete') and not a.name:
        print(i18n.t('cli.error.profile_needs_name', action=a.action), file=sys.stderr)
        return 1
    for attr in ('color', 'color2', 'base'):
        v = getattr(a, attr, None)
        if v is not None:
            try:
                C.parse(v)
            except C.ColorError as e:
                print(i18n.t('cli.error.generic'), e, file=sys.stderr)
                return 1
    try:
        a.fn(a)
    except NoDaemon as e:
        print(i18n.t('cli.error.generic'), e, file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        return 130
    return 0


if __name__ == '__main__':
    sys.exit(main())
