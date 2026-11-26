use evdev::Key;

/// Authoritative 104-slot physical keyboard coordinate table matching the GUI layout
/// in `omen-gui/src/keyboardrgb/keyboard.rs` (slots `0..98` visible keys, `98..104` extra).
///
/// Coordinates are expressed in keycap width units (`x` ~ `0.0..=18.0`, `y` ~ `0.0..=5.0`),
/// measured at the visual center of each key.
pub const KEY_COORDS: [(f64, f64); 104] = [
    // Row 0 (y = 0.0): Esc, F1..F4, F5..F8, F9..F12, Del, Omen, Calc, Power (17 keys: 0..=16)
    (0.5, 0.0),  // 0: Esc
    (2.0, 0.0),  // 1: F1
    (3.0, 0.0),  // 2: F2
    (4.0, 0.0),  // 3: F3
    (5.0, 0.0),  // 4: F4
    (6.5, 0.0),  // 5: F5
    (7.5, 0.0),  // 6: F6
    (8.5, 0.0),  // 7: F7
    (9.5, 0.0),  // 8: F8
    (11.0, 0.0), // 9: F9
    (12.0, 0.0), // 10: F10
    (13.0, 0.0), // 11: F11
    (14.0, 0.0), // 12: F12
    (15.5, 0.0), // 13: Del
    (16.5, 0.0), // 14: Omen
    (17.5, 0.0), // 15: Calc
    (18.5, 0.0), // 16: Power
    // Row 1 (y = 1.0): ~, 1..0, -, =, Backspace, Num, /_num, *_num, -_num (18 keys: 17..=34)
    (0.5, 1.0),  // 17: ~
    (1.5, 1.0),  // 18: 1
    (2.5, 1.0),  // 19: 2
    (3.5, 1.0),  // 20: 3
    (4.5, 1.0),  // 21: 4
    (5.5, 1.0),  // 22: 5
    (6.5, 1.0),  // 23: 6
    (7.5, 1.0),  // 24: 7
    (8.5, 1.0),  // 25: 8
    (9.5, 1.0),  // 26: 9
    (10.5, 1.0), // 27: 0
    (11.5, 1.0), // 28: -
    (12.5, 1.0), // 29: =
    (14.0, 1.0), // 30: Backspace (w=2.0)
    (15.5, 1.0), // 31: NumLock
    (16.5, 1.0), // 32: Numpad /
    (17.5, 1.0), // 33: Numpad *
    (18.5, 1.0), // 34: Numpad -
    // Row 2 (y = 2.0): Tab, Q..P, [, ], \, 7..9_num, +_num (18 keys: 35..=52)
    (0.75, 2.0), // 35: Tab (w=1.5)
    (2.0, 2.0),  // 36: Q
    (3.0, 2.0),  // 37: W
    (4.0, 2.0),  // 38: E
    (5.0, 2.0),  // 39: R
    (6.0, 2.0),  // 40: T
    (7.0, 2.0),  // 41: Y
    (8.0, 2.0),  // 42: U
    (9.0, 2.0),  // 43: I
    (10.0, 2.0), // 44: O
    (11.0, 2.0), // 45: P
    (12.0, 2.0), // 46: [
    (13.0, 2.0), // 47: ]
    (14.25, 2.0),// 48: \ (w=1.5)
    (15.5, 2.0), // 49: Numpad 7
    (16.5, 2.0), // 50: Numpad 8
    (17.5, 2.0), // 51: Numpad 9
    (18.5, 2.5), // 52: Numpad + (2 rows tall)
    // Row 3 (y = 3.0): Caps, A..', Enter, 4..6_num (16 keys: 53..=68)
    (0.87, 3.0), // 53: Caps (w=1.75)
    (2.25, 3.0), // 54: A
    (3.25, 3.0), // 55: S
    (4.25, 3.0), // 56: D
    (5.25, 3.0), // 57: F
    (6.25, 3.0), // 58: G
    (7.25, 3.0), // 59: H
    (8.25, 3.0), // 60: J
    (9.25, 3.0), // 61: K
    (10.25, 3.0),// 62: L
    (11.25, 3.0),// 63: ;
    (12.25, 3.0),// 64: '
    (13.87, 3.0),// 65: Enter (w=2.25)
    (15.5, 3.0), // 66: Numpad 4
    (16.5, 3.0), // 67: Numpad 5
    (17.5, 3.0), // 68: Numpad 6
    // Row 4 (y = 4.0): Shift, Z../, Shift_R, Up, 1..3_num, Enter_num (17 keys: 69..=85)
    (1.12, 4.0), // 69: Left Shift (w=2.25)
    (2.75, 4.0), // 70: Z
    (3.75, 4.0), // 71: X
    (4.75, 4.0), // 72: C
    (5.75, 4.0), // 73: V
    (6.75, 4.0), // 74: B
    (7.75, 4.0), // 75: N
    (8.75, 4.0), // 76: M
    (9.75, 4.0), // 77: ,
    (10.75, 4.0),// 78: .
    (11.75, 4.0),// 79: /
    (13.12, 4.0),// 80: Right Shift (w=1.75)
    (14.5, 4.0), // 81: Up
    (15.5, 4.0), // 82: Numpad 1
    (16.5, 4.0), // 83: Numpad 2
    (17.5, 4.0), // 84: Numpad 3
    (18.5, 4.5), // 85: Numpad Enter (2 rows tall)
    // Row 5 (y = 5.0): Ctrl, Fn, Win, Alt, Space, Alt_R, Menu, Left, Down, Right, 0_num, ._num (12 keys: 86..=97)
    (0.62, 5.0), // 86: Left Ctrl (w=1.25)
    (1.87, 5.0), // 87: Fn
    (3.12, 5.0), // 88: Win/Meta
    (4.37, 5.0), // 89: Left Alt
    (7.75, 5.0), // 90: Space (w=5.5)
    (11.12, 5.0),// 91: Right Alt / AltGr
    (12.37, 5.0),// 92: Menu / Right Ctrl
    (13.5, 5.0), // 93: Left
    (14.5, 5.0), // 94: Down
    (15.5, 5.0), // 95: Right
    (16.5, 5.0), // 96: Numpad 0
    (17.5, 5.0), // 97: Numpad .
    // Auxiliary / Macro / Settings slots (98..=103)
    (18.0, 0.0), // 98: Settings / Prog1
    (0.0, 1.0),  // 99: Macro P1
    (0.0, 2.0),  // 100: Macro P2
    (0.0, 3.0),  // 101: Macro P3
    (0.0, 4.0),  // 102: Macro P4
    (0.0, 5.0),  // 103: Macro P5 / Extra
];

#[inline]
pub fn slot_coords(slot: usize) -> (f64, f64) {
    KEY_COORDS.get(slot).copied().unwrap_or((9.0, 2.5))
}

/// Map a Linux `evdev` key code (`EV_KEY`) to the physical `(x, y)` coordinates
/// of the matching keycap on an OMEN keyboard. Returns `None` for non-keyboard
/// events (mouse buttons, switches, etc.) so composite USB KBM nodes stay clean.
pub fn evdev_code_to_coords(code: u16) -> Option<(f64, f64)> {
    let slot: usize = match Key::new(code) {
        Key::KEY_ESC => 0,
        Key::KEY_F1 => 1,
        Key::KEY_F2 => 2,
        Key::KEY_F3 => 3,
        Key::KEY_F4 => 4,
        Key::KEY_F5 => 5,
        Key::KEY_F6 => 6,
        Key::KEY_F7 => 7,
        Key::KEY_F8 => 8,
        Key::KEY_F9 => 9,
        Key::KEY_F10 => 10,
        Key::KEY_F11 => 11,
        Key::KEY_F12 => 12,
        Key::KEY_DELETE | Key::KEY_INSERT | Key::KEY_PRINT => 13,
        Key::KEY_PROG1 | Key::KEY_PROG2 => 14,
        Key::KEY_CALC => 15,
        Key::KEY_POWER => 16,

        Key::KEY_GRAVE => 17,
        Key::KEY_1 => 18,
        Key::KEY_2 => 19,
        Key::KEY_3 => 20,
        Key::KEY_4 => 21,
        Key::KEY_5 => 22,
        Key::KEY_6 => 23,
        Key::KEY_7 => 24,
        Key::KEY_8 => 25,
        Key::KEY_9 => 26,
        Key::KEY_0 => 27,
        Key::KEY_MINUS => 28,
        Key::KEY_EQUAL => 29,
        Key::KEY_BACKSPACE => 30,
        Key::KEY_NUMLOCK => 31,
        Key::KEY_KPSLASH => 32,
        Key::KEY_KPASTERISK => 33,
        Key::KEY_KPMINUS => 34,

        Key::KEY_TAB => 35,
        Key::KEY_Q => 36,
        Key::KEY_W => 37,
        Key::KEY_E => 38,
        Key::KEY_R => 39,
        Key::KEY_T => 40,
        Key::KEY_Y => 41,
        Key::KEY_U => 42,
        Key::KEY_I => 43,
        Key::KEY_O => 44,
        Key::KEY_P => 45,
        Key::KEY_LEFTBRACE => 46,
        Key::KEY_RIGHTBRACE => 47,
        Key::KEY_BACKSLASH => 48,
        Key::KEY_KP7 => 49,
        Key::KEY_KP8 => 50,
        Key::KEY_KP9 => 51,
        Key::KEY_KPPLUS => 52,

        Key::KEY_CAPSLOCK => 53,
        Key::KEY_A => 54,
        Key::KEY_S => 55,
        Key::KEY_D => 56,
        Key::KEY_F => 57,
        Key::KEY_G => 58,
        Key::KEY_H => 59,
        Key::KEY_J => 60,
        Key::KEY_K => 61,
        Key::KEY_L => 62,
        Key::KEY_SEMICOLON => 63,
        Key::KEY_APOSTROPHE => 64,
        Key::KEY_ENTER => 65,
        Key::KEY_KP4 => 66,
        Key::KEY_KP5 => 67,
        Key::KEY_KP6 => 68,

        Key::KEY_LEFTSHIFT | Key::KEY_102ND => 69,
        Key::KEY_Z => 70,
        Key::KEY_X => 71,
        Key::KEY_C => 72,
        Key::KEY_V => 73,
        Key::KEY_B => 74,
        Key::KEY_N => 75,
        Key::KEY_M => 76,
        Key::KEY_COMMA => 77,
        Key::KEY_DOT => 78,
        Key::KEY_SLASH => 79,
        Key::KEY_RIGHTSHIFT => 80,
        Key::KEY_UP | Key::KEY_HOME | Key::KEY_PAGEUP => 81,
        Key::KEY_KP1 => 82,
        Key::KEY_KP2 => 83,
        Key::KEY_KP3 => 84,
        Key::KEY_KPENTER => 85,

        Key::KEY_LEFTCTRL => 86,
        Key::KEY_FN => 87,
        Key::KEY_LEFTMETA | Key::KEY_RIGHTMETA => 88,
        Key::KEY_LEFTALT => 89,
        Key::KEY_SPACE => 90,
        Key::KEY_RIGHTALT => 91,
        Key::KEY_COMPOSE | Key::KEY_RIGHTCTRL => 92,
        Key::KEY_LEFT | Key::KEY_END => 93,
        Key::KEY_DOWN | Key::KEY_PAGEDOWN => 94,
        Key::KEY_RIGHT => 95,
        Key::KEY_KP0 => 96,
        Key::KEY_KPDOT => 97,
        _ => return None,
    };
    Some(slot_coords(slot))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evdev_mapping_matches_physical_key_slots() {
        assert_eq!(evdev_code_to_coords(Key::KEY_ESC.code()), Some(slot_coords(0)));
        assert_eq!(evdev_code_to_coords(Key::KEY_W.code()), Some(slot_coords(37)));
        assert_eq!(evdev_code_to_coords(Key::KEY_Z.code()), Some(slot_coords(70)));
        assert_eq!(evdev_code_to_coords(Key::KEY_SPACE.code()), Some(slot_coords(90)));
        assert_eq!(evdev_code_to_coords(Key::KEY_KPENTER.code()), Some(slot_coords(85)));
        assert!(evdev_code_to_coords(Key::BTN_LEFT.code()).is_none());

        // KEY_W and KEY_Z are both on the left side of the keyboard (unlike code % 15!).
        let (wx, _) = evdev_code_to_coords(Key::KEY_W.code()).unwrap();
        let (zx, _) = evdev_code_to_coords(Key::KEY_Z.code()).unwrap();
        let (entx, _) = evdev_code_to_coords(Key::KEY_ENTER.code()).unwrap();
        assert!(wx < 5.0 && zx < 5.0);
        assert!(entx > 12.0);
    }
}
