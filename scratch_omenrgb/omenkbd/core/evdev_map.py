"""Mapowanie kodu klawisza z Linuksowego evdev (KEY_* z input-event-codes.h)
na HID Usage z Keyboard/Keypad Page (0x07).

Zrodlem prawdy jest tablica z jadra Linuksa (drivers/hid/hid-input.c,
hid_keyboard[256]), ktora wiaze HID Usage z kodem evdev — bo to jadro
tlumaczy raporty HID na zdarzenia evdev dla kazdej klawiatury USB w systemie,
w tym dla naszej. Piszemy ja w kierunku HID -> evdev (bo tak jest w zrodle)
i odwracamy programowo, zeby nie popelnic bledu przy recznym odwracaniu.

Nieliczne rzadkie klawisze (dodatkowe klawisze F13-F24, jezykowe klawisze
niektorych ukladow) sa swiadomie pominiete — brak wpisu oznacza po prostu,
ze nacisniecie tego klawisza nie wywola blysku, nie ze cos jest zle.
"""

# HID Usage (Keyboard/Keypad Page 0x07) -> kod evdev (linux/input-event-codes.h)
HID_TO_EVDEV = {
    0x04: 30, 0x05: 48, 0x06: 46, 0x07: 32, 0x08: 18, 0x09: 33, 0x0A: 34,
    0x0B: 35, 0x0C: 23, 0x0D: 36, 0x0E: 37, 0x0F: 38, 0x10: 50, 0x11: 49,
    0x12: 24, 0x13: 25, 0x14: 16, 0x15: 19, 0x16: 31, 0x17: 20, 0x18: 22,
    0x19: 47, 0x1A: 17, 0x1B: 45, 0x1C: 21, 0x1D: 44,               # A..Z
    0x1E: 2, 0x1F: 3, 0x20: 4, 0x21: 5, 0x22: 6, 0x23: 7, 0x24: 8,
    0x25: 9, 0x26: 10, 0x27: 11,                                    # 1..9,0
    0x28: 28,   # Enter
    0x29: 1,    # Esc
    0x2A: 14,   # Backspace
    0x2B: 15,   # Tab
    0x2C: 57,   # Space
    0x2D: 12,   # -_
    0x2E: 13,   # =+
    0x2F: 26,   # [{
    0x30: 27,   # ]}
    0x31: 43,   # \|
    0x33: 39,   # ;:
    0x34: 40,   # '"
    0x35: 41,   # `~
    0x36: 51,   # ,<
    0x37: 52,   # .>
    0x38: 53,   # /?
    0x39: 58,   # CapsLock
    0x3A: 59, 0x3B: 60, 0x3C: 61, 0x3D: 62, 0x3E: 63, 0x3F: 64,
    0x40: 65, 0x41: 66, 0x42: 67, 0x43: 68,                          # F1..F10
    0x44: 87, 0x45: 88,                                              # F11, F12
    0x46: 99,   # PrtSc
    0x47: 70,   # ScrLk
    0x48: 119,  # Pause
    0x49: 110,  # Insert
    0x4A: 102,  # Home
    0x4B: 104,  # PgUp
    0x4C: 111,  # Delete
    0x4D: 107,  # End
    0x4E: 109,  # PgDn
    0x4F: 106,  # Right
    0x50: 105,  # Left
    0x51: 108,  # Down
    0x52: 103,  # Up
    0x53: 69,   # NumLock
    0x54: 98,   # KP/
    0x55: 55,   # KP*
    0x56: 74,   # KP-
    0x57: 78,   # KP+
    0x58: 96,   # KPEnter
    0x59: 79, 0x5A: 80, 0x5B: 81, 0x5C: 75, 0x5D: 76, 0x5E: 77,
    0x5F: 71, 0x60: 72, 0x61: 73,                                    # KP1..KP9
    0x62: 82,   # KP0
    0x63: 83,   # KP.
    0x65: 127,  # Menu (Compose)
    0x66: 116,  # Power
    0x67: 117,  # KP=
    0x85: 121,  # KP,
    0xE0: 29,   # LCtrl
    0xE1: 42,   # LShift
    0xE2: 56,   # LAlt
    0xE3: 125,  # LMeta
    0xE4: 97,   # RCtrl
    0xE5: 54,   # RShift
    0xE6: 100,  # RAlt
    0xE7: 126,  # RMeta
}

# Kod evdev -> HID Usage. Odwrocenie automatyczne, zeby nie przepisywac
# 90 wpisow drugi raz recznie i nie wprowadzic literowki, ktorej nikt by nie
# zauwazyl (klawisz po prostu by nie swiecil).
EVDEV_TO_HID = {code: usage for usage, code in HID_TO_EVDEV.items()}

assert len(EVDEV_TO_HID) == len(HID_TO_EVDEV), \
    'dwa HID usages dzielace jeden kod evdev — sprawdz tablice'


def hid_usage_for(evdev_code):
    return EVDEV_TO_HID.get(evdev_code)
