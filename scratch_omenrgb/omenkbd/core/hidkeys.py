"""Tablica HID Usage -> czytelna nazwa klawisza (Keyboard/Keypad Page 0x07).

Uzywana w dwie strony: do nazywania lampek (InputBinding z Reportu 3)
i do adresowania ich po nazwie ("W", "Space", "LShift").
"""

HID_KEYS = {
    0x28: 'Enter', 0x29: 'Esc', 0x2a: 'Backspace', 0x2b: 'Tab', 0x2c: 'Space',
    0x2d: '-_', 0x2e: '=+', 0x2f: '[{', 0x30: ']}', 0x31: '\\|', 0x33: ';:', 0x34: "'\"",
    0x35: '`~', 0x36: ',<', 0x37: '.>', 0x38: '/?', 0x39: 'CapsLock',
    0x46: 'PrtSc', 0x47: 'ScrLk', 0x48: 'Pause', 0x49: 'Insert', 0x4a: 'Home',
    0x4b: 'PgUp', 0x4c: 'Delete', 0x4d: 'End', 0x4e: 'PgDn',
    0x4f: 'Right', 0x50: 'Left', 0x51: 'Down', 0x52: 'Up', 0x53: 'NumLock',
    0x54: 'KP/', 0x55: 'KP*', 0x56: 'KP-', 0x57: 'KP+', 0x58: 'KPEnter', 0x59: 'KP1',
    0x5a: 'KP2', 0x5b: 'KP3', 0x5c: 'KP4', 0x5d: 'KP5', 0x5e: 'KP6', 0x5f: 'KP7',
    0x60: 'KP8', 0x61: 'KP9', 0x62: 'KP0', 0x63: 'KP.', 0x65: 'Menu',
    0x32: '#~', 0x64: '\\|', 0x66: 'Power', 0x67: 'KP=', 0x85: 'KP,',
    0xe0: 'LCtrl', 0xe1: 'LShift', 0xe2: 'LAlt', 0xe3: 'LMeta',
    0xe4: 'RCtrl', 0xe5: 'RShift', 0xe6: 'RAlt', 0xe7: 'RMeta', 0xe8: 'Omen',
}
for _i in range(4, 30):
    HID_KEYS[_i] = chr(ord('A') + _i - 4)
for _i in range(30, 39):
    HID_KEYS[_i] = str(_i - 29)
HID_KEYS[0x27] = '0'
for _i in range(0x3a, 0x46):
    HID_KEYS[_i] = f'F{_i - 0x39}'

# Lampki bez przypisanego klawisza — diody w przerwach miedzy blokami.
UNBOUND = (0x00, 0x03)


def key_name(usage):
    if usage in UNBOUND:
        return None
    return HID_KEYS.get(usage, f'0x{usage:02x}')
