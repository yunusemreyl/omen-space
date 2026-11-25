//! Global Quick HUD hotkey model, shared by the daemon and the GUI.
//!
//! The hotkey is persisted in `~/.config/omenspace/settings.json` as three
//! fields:
//!
//! * `overlay_hotkey` — human readable label, e.g. `"Shift+F2"` (used by the
//!   overlay header and the settings row).
//! * `overlay_hotkey_code` — the physical Linux evdev key code (layout
//!   independent, e.g. `60` for `KEY_F2`). This is what the daemon listens to.
//! * `overlay_hotkey_mods` — bitmask: `1` Ctrl, `2` Alt, `4` Shift, `8` Super.
//!
//! Older installs only have the string field. [`HotkeySpec::from_settings`]
//! still understands the common `"Ctrl+Shift+F5"` spelling as a fallback.

use serde_json::Value;

/// Default hotkey: `Shift+F2` (`KEY_F2` is evdev code 60).
pub const DEFAULT_HOTKEY: &str = "Shift+F2";

const MOD_CTRL: u8 = 1;
const MOD_ALT: u8 = 2;
const MOD_SHIFT: u8 = 4;
const MOD_META: u8 = 8;

/// Modifier keys that must be held for the hotkey to fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HotkeyModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// Super / Windows / Command.
    pub meta: bool,
}

impl HotkeyModifiers {
    pub fn from_bits(bits: u8) -> Self {
        Self {
            ctrl: bits & MOD_CTRL != 0,
            alt: bits & MOD_ALT != 0,
            shift: bits & MOD_SHIFT != 0,
            meta: bits & MOD_META != 0,
        }
    }

    pub fn to_bits(self) -> u8 {
        (if self.ctrl { MOD_CTRL } else { 0 })
            | (if self.alt { MOD_ALT } else { 0 })
            | (if self.shift { MOD_SHIFT } else { 0 })
            | (if self.meta { MOD_META } else { 0 })
    }

    /// `"Ctrl+Alt+Shift+Super+"` (empty when no modifier is set).
    fn prefix(self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.meta {
            parts.push("Super");
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("{}+", parts.join("+"))
        }
    }
}

/// A complete hotkey: modifiers plus a physical evdev key code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeySpec {
    pub key_code: u16,
    pub modifiers: HotkeyModifiers,
    /// Display label for the key itself, e.g. `"F2"`, `"A"`, `"PageUp"`.
    pub label: String,
}

impl Default for HotkeySpec {
    fn default() -> Self {
        Self {
            key_code: 60, // KEY_F2
            modifiers: HotkeyModifiers { shift: true, ..Default::default() },
            label: "F2".to_string(),
        }
    }
}

impl HotkeySpec {
    pub fn new(key_code: u16, modifiers: HotkeyModifiers) -> Self {
        let label = evdev_key_name(key_code)
            .map(str::to_string)
            .unwrap_or_else(|| format!("Key{key_code}"));
        Self { key_code, modifiers, label }
    }

    /// Build from an explicit display label (what the GUI records from GTK).
    pub fn with_label(key_code: u16, modifiers: HotkeyModifiers, label: impl Into<String>) -> Self {
        let label = sanitize_label(label.into(), key_code);
        Self { key_code, modifiers, label }
    }

    /// Human readable form, e.g. `"Ctrl+Shift+F5"`.
    pub fn display(&self) -> String {
        format!("{}{}", self.modifiers.prefix(), self.label)
    }

    /// Read the hotkey from a parsed `settings.json` value, falling back to
    /// [`DEFAULT_HOTKEY`] when the file has no usable value.
    pub fn from_settings(json: &Value) -> Self {
        let code = json
            .get("overlay_hotkey_code")
            .and_then(Value::as_u64)
            .and_then(|n| u16::try_from(n).ok());
        if let Some(code) = code.filter(|&code| code != 0 && !is_modifier_code(code)) {
            let bits = json
                .get("overlay_hotkey_mods")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u8;
            let label = json
                .get("overlay_hotkey_label")
                .and_then(Value::as_str)
                .unwrap_or("");
            return Self::with_label(code, HotkeyModifiers::from_bits(bits), label);
        }

        // Legacy/hand-written string such as "Ctrl+Alt+F5".
        if let Some(spec) = json
            .get("overlay_hotkey")
            .and_then(Value::as_str)
            .and_then(Self::parse)
        {
            return spec;
        }

        Self::default()
    }

    /// Persist the hotkey into a `settings.json` object.
    pub fn write_settings(&self, json: &mut Value) {
        json["overlay_hotkey"] = Value::String(self.display());
        json["overlay_hotkey_label"] = Value::String(self.label.clone());
        json["overlay_hotkey_code"] = Value::from(self.key_code);
        json["overlay_hotkey_mods"] = Value::from(self.modifiers.to_bits());
    }

    /// Parse a legacy string spelling such as `"Shift+F2"` or `"Ctrl+Alt+F5"`.
    pub fn parse(input: &str) -> Option<Self> {
        let mut modifiers = HotkeyModifiers::default();
        let mut key_token: Option<&str> = None;

        for raw in input.split('+') {
            let part = raw.trim();
            if part.is_empty() {
                continue;
            }
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => modifiers.ctrl = true,
                "alt" | "option" => modifiers.alt = true,
                "shift" => modifiers.shift = true,
                "super" | "meta" | "win" | "windows" | "cmd" | "command" => modifiers.meta = true,
                _ => {
                    if key_token.is_some() {
                        return None;
                    }
                    key_token = Some(part);
                }
            }
        }

        let code = evdev_key_code(key_token?)?;
        if code == 0 || is_modifier_code(code) {
            return None;
        }
        Some(Self::new(code, modifiers))
    }
}

/// True for keys that only act as modifiers and therefore cannot be the
/// non-modifier half of a hotkey.
pub fn is_modifier_code(code: u16) -> bool {
    matches!(
        code,
        29 | 97 | 56 | 100 | 42 | 54 | 125 | 126 // Ctrl, Alt, Shift, Meta (left/right)
    )
}

/// True for keys that are safe to bind without any modifier: function keys,
/// dedicated media/launcher keys, and programmable macro keys (such as the HP
/// OMEN/Victus key). Ordinary typing keys (letters, digits, Space, Enter, ...)
/// need a modifier, otherwise every keystroke would also toggle the overlay
/// (the daemon only listens, it does not grab the key).
pub fn allows_no_modifier(code: u16) -> bool {
    matches!(
        code,
        59..=68 | 87 | 88 | 183..=194 // F1-F24
            | 99 | 119 | 70 | 110 | 139 // PrintScreen, Pause, ScrollLock, Insert, Menu
            | 113..=115 | 164..=167 // Mute, Volume, Play/Prev/Stop/Next
            | 224 | 225 | 140 | 142 | 143 // Brightness, Calculator, Sleep, WakeUp
            | 148 | 149 | 200 | 202..=205 | 256 // Prog1-4 / HP OMEN & Victus launch/macro keys
    )
}

fn sanitize_label(label: String, code: u16) -> String {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return evdev_key_name(code)
            .map(str::to_string)
            .unwrap_or_else(|| format!("Key{code}"));
    }
    // Keep the overlay header readable even if settings.json was hand-edited.
    trimmed.chars().take(32).collect()
}

const F_KEYS: [&str; 24] = [
    "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14",
    "F15", "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24",
];

const DIGITS: [&str; 10] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"];

/// Canonical name, evdev code and extra spellings for named keys.
const NAMED_KEYS: &[(u16, &str, &[&str])] = &[
    (1, "Esc", &["Escape", "ESC"]),
    (14, "Backspace", &["BackSpace", "Backspace"]),
    (15, "Tab", &["Tab"]),
    (28, "Enter", &["Return", "Enter"]),
    (57, "Space", &["space", "Space"]),
    (58, "CapsLock", &["Caps_Lock", "CapsLock"]),
    (69, "NumLock", &["Num_Lock", "NumLock"]),
    (70, "ScrollLock", &["Scroll_Lock", "ScrollLock"]),
    (99, "PrintScreen", &["Print", "PrintScreen", "SysRq"]),
    (119, "Pause", &["Pause", "Break"]),
    (102, "Home", &["Home"]),
    (103, "Up", &["Up"]),
    (104, "PageUp", &["Page_Up", "Prior", "PageUp"]),
    (105, "Left", &["Left"]),
    (106, "Right", &["Right"]),
    (107, "End", &["End"]),
    (108, "Down", &["Down"]),
    (109, "PageDown", &["Page_Down", "Next", "PageDown"]),
    (110, "Insert", &["Insert"]),
    (111, "Delete", &["Delete"]),
    (139, "Menu", &["Menu", "Apps"]),
    (12, "Minus", &["minus"]),
    (13, "Equal", &["equal"]),
    (26, "LeftBracket", &["bracketleft"]),
    (27, "RightBracket", &["bracketright"]),
    (43, "Backslash", &["backslash"]),
    (39, "Semicolon", &["semicolon"]),
    (40, "Apostrophe", &["apostrophe", "Quote"]),
    (41, "Grave", &["grave", "Backtick"]),
    (51, "Comma", &["comma"]),
    (52, "Period", &["period"]),
    (53, "Slash", &["slash"]),
    (71, "KP7", &["KP_7"]),
    (72, "KP8", &["KP_8"]),
    (73, "KP9", &["KP_9"]),
    (74, "KPSubtract", &["KP_Subtract"]),
    (75, "KP4", &["KP_4"]),
    (76, "KP5", &["KP_5"]),
    (77, "KP6", &["KP_6"]),
    (78, "KPAdd", &["KP_Add"]),
    (79, "KP1", &["KP_1"]),
    (80, "KP2", &["KP_2"]),
    (81, "KP3", &["KP_3"]),
    (82, "KP0", &["KP_0"]),
    (83, "KPDecimal", &["KP_Decimal", "KP_Dot"]),
    (96, "KPEnter", &["KP_Enter"]),
    (98, "KPDivide", &["KP_Divide"]),
    (55, "KPMultiply", &["KP_Multiply"]),
    (117, "KPEqual", &["KP_Equal"]),
    (113, "Mute", &["XF86AudioMute", "Mute"]),
    (114, "VolumeDown", &["XF86AudioLowerVolume", "VolumeDown"]),
    (115, "VolumeUp", &["XF86AudioRaiseVolume", "VolumeUp"]),
    (164, "PlayPause", &["XF86AudioPlay", "XF86AudioPause", "PlayPause"]),
    (165, "Previous", &["XF86AudioPrev", "Previous"]),
    (167, "Next", &["XF86AudioNext", "Next"]),
    (166, "Stop", &["XF86AudioStop", "Stop"]),
    (224, "BrightnessDown", &["XF86MonBrightnessDown", "BrightnessDown"]),
    (225, "BrightnessUp", &["XF86MonBrightnessUp", "BrightnessUp"]),
    (142, "Sleep", &["XF86Sleep", "Sleep"]),
    (143, "WakeUp", &["XF86WakeUp", "WakeUp"]),
    (140, "Calculator", &["XF86Calculator", "Calculator"]),
    (148, "Launch1", &["XF86Launch1", "Prog1", "Launch1"]),
    (149, "Launch2", &["XF86Launch2", "Prog2", "Launch2"]),
    (200, "Launch2Raw", &["Launch2Raw"]),
    (202, "Launch3", &["XF86Launch3", "Prog3", "Launch3"]),
    (203, "Launch4", &["XF86Launch4", "Prog4", "Launch4"]),
    (204, "Launch5", &["XF86Launch5", "Launch5"]),
    (205, "Launch6", &["XF86Launch6", "Launch6"]),
    (256, "MacroBtn", &["MacroBtn", "BTN_0"]),
];

/// Resolve a key name (or `"Key<code>"`) to an evdev key code.
pub fn evdev_key_code(name: &str) -> Option<u16> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }

    // "Key60" / "key60"
    if let Some(n) = name
        .strip_prefix("Key")
        .or_else(|| name.strip_prefix("key"))
        .and_then(|n| n.parse::<u16>().ok())
    {
        return Some(n);
    }

    // Single character: digit or letter. This must come before the bare-number
    // parse so that "1" means the 1 key (evdev 2), not evdev code 1.
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if let Some(code) = digit_code(c) {
            return Some(code);
        }
        if let Some(code) = letter_code(c) {
            return Some(code);
        }
    }

    // Bare numeric evdev code ("60").
    if let Ok(n) = name.parse::<u16>() {
        return Some(n);
    }

    // F1..F24 (evdev F1=59, F11=87, F13=183).
    if let Some(n) = name
        .strip_prefix('F')
        .or_else(|| name.strip_prefix('f'))
        .and_then(|rest| rest.parse::<u16>().ok())
    {
        match n {
            1..=10 => return Some(58 + n),
            11..=12 => return Some(76 + n),
            13..=24 => return Some(170 + n),
            _ => {}
        }
    }

    for &(code, canonical, aliases) in NAMED_KEYS {
        if name.eq_ignore_ascii_case(canonical) {
            return Some(code);
        }
        if aliases.iter().any(|alias| name.eq_ignore_ascii_case(alias)) {
            return Some(code);
        }
    }

    None
}

/// Friendly name for a physical evdev key code, if it is a common key.
pub fn evdev_key_name(code: u16) -> Option<&'static str> {
    if let Some(&(_, canonical, _)) = NAMED_KEYS.iter().find(|(c, _, _)| *c == code) {
        return Some(canonical);
    }

    match code {
        59..=68 => Some(F_KEYS[(code - 59) as usize]),
        87..=88 => Some(F_KEYS[(10 + (code - 87)) as usize]),
        183..=194 => Some(F_KEYS[(12 + (code - 183)) as usize]),
        2..=11 => Some(DIGITS[(code - 2) as usize]),
        _ => letter_name(code),
    }
}

fn digit_code(c: char) -> Option<u16> {
    match c {
        '1'..='9' => Some(c as u16 - '0' as u16 + 1),
        '0' => Some(11),
        _ => None,
    }
}

fn letter_code(c: char) -> Option<u16> {
    Some(match c.to_ascii_uppercase() {
        'A' => 30,
        'B' => 48,
        'C' => 46,
        'D' => 32,
        'E' => 18,
        'F' => 33,
        'G' => 34,
        'H' => 35,
        'I' => 23,
        'J' => 36,
        'K' => 37,
        'L' => 38,
        'M' => 50,
        'N' => 49,
        'O' => 24,
        'P' => 25,
        'Q' => 16,
        'R' => 19,
        'S' => 31,
        'T' => 20,
        'U' => 22,
        'V' => 47,
        'W' => 17,
        'X' => 45,
        'Y' => 21,
        'Z' => 44,
        _ => return None,
    })
}

fn letter_name(code: u16) -> Option<&'static str> {
    Some(match code {
        30 => "A",
        48 => "B",
        46 => "C",
        32 => "D",
        18 => "E",
        33 => "F",
        34 => "G",
        35 => "H",
        23 => "I",
        36 => "J",
        37 => "K",
        38 => "L",
        50 => "M",
        49 => "N",
        24 => "O",
        25 => "P",
        16 => "Q",
        19 => "R",
        31 => "S",
        20 => "T",
        22 => "U",
        47 => "V",
        17 => "W",
        45 => "X",
        21 => "Y",
        44 => "Z",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_is_shift_f2() {
        let spec = HotkeySpec::default();
        assert_eq!(spec.key_code, 60);
        assert!(spec.modifiers.shift);
        assert_eq!(spec.display(), "Shift+F2");
    }

    #[test]
    fn parses_legacy_strings() {
        let spec = HotkeySpec::parse("Ctrl+Alt+Shift+Super+F5").unwrap();
        assert_eq!(spec.key_code, 63);
        assert_eq!(
            spec.modifiers,
            HotkeyModifiers { ctrl: true, alt: true, shift: true, meta: true }
        );
        assert_eq!(spec.display(), "Ctrl+Alt+Shift+Super+F5");

        let spec = HotkeySpec::parse("shift+f2").unwrap();
        assert_eq!(spec.key_code, 60);
        assert!(spec.modifiers.shift);

        assert!(HotkeySpec::parse("Shift+Shift_L").is_none());
        assert!(HotkeySpec::parse("Shift+NotAKey").is_none());
    }

    #[test]
    fn f_key_ranges_are_correct() {
        assert_eq!(evdev_key_code("F1"), Some(59));
        assert_eq!(evdev_key_code("F10"), Some(68));
        assert_eq!(evdev_key_code("F11"), Some(87));
        assert_eq!(evdev_key_code("F12"), Some(88));
        assert_eq!(evdev_key_code("F13"), Some(183));
        assert_eq!(evdev_key_code("F24"), Some(194));
        assert_eq!(evdev_key_name(60), Some("F2"));
        assert_eq!(evdev_key_name(194), Some("F24"));
    }

    #[test]
    fn digits_and_letters_round_trip() {
        for (name, code) in [("1", 2u16), ("0", 11), ("A", 30), ("Q", 16), ("Z", 44)] {
            assert_eq!(evdev_key_code(name), Some(code));
            assert_eq!(evdev_key_name(code), Some(name));
        }
    }

    #[test]
    fn settings_round_trip_prefers_physical_code() {
        let spec = HotkeySpec::with_label(
            63,
            HotkeyModifiers { ctrl: true, shift: true, ..Default::default() },
            "F5",
        );
        let mut json = json!({});
        spec.write_settings(&mut json);
        assert_eq!(json["overlay_hotkey"], "Ctrl+Shift+F5");
        assert_eq!(json["overlay_hotkey_code"], 63);
        assert_eq!(json["overlay_hotkey_mods"], MOD_CTRL | MOD_SHIFT);

        assert_eq!(HotkeySpec::from_settings(&json), spec);
    }

    #[test]
    fn settings_fall_back_to_legacy_string() {
        let json = json!({ "overlay_hotkey": "Ctrl+F6" });
        let spec = HotkeySpec::from_settings(&json);
        assert_eq!(spec.key_code, 64);
        assert!(spec.modifiers.ctrl);
    }

    #[test]
    fn typing_keys_need_a_modifier() {
        for code in [59u16, 60, 87, 88, 183, 194, 110, 113, 139, 148, 149, 164, 200, 202, 224, 256] {
            assert!(allows_no_modifier(code), "code {code} should be allowed alone");
        }
        for code in [30u16, 2, 57, 28, 15, 1, 14, 103] {
            assert!(!allows_no_modifier(code), "code {code} must require a modifier");
        }
    }

    #[test]
    fn programmable_launch_keys_parse_and_round_trip() {
        let spec = HotkeySpec::parse("Alt+Launch2").unwrap();
        assert_eq!(spec.key_code, 149);
        assert!(spec.modifiers.alt);
        assert_eq!(spec.display(), "Alt+Launch2");

        let bare = HotkeySpec::parse("Launch2").unwrap();
        assert_eq!(bare.key_code, 149);
        assert_eq!(bare.modifiers.to_bits(), 0);
        assert!(allows_no_modifier(bare.key_code));
    }

    #[test]
    fn modifier_only_codes_are_rejected() {
        let json = json!({ "overlay_hotkey_code": 42, "overlay_hotkey_mods": 0 });
        assert_eq!(HotkeySpec::from_settings(&json), HotkeySpec::default());
    }
}
