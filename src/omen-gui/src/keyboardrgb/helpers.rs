use gtk::prelude::*;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum KeyboardMode {
    Victus1Zone,
    Omen4Zone,
    PerKey,
    DesktopRgb,
}

pub fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (f64, f64, f64) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let mut h_prime = h / 60.0;
    while h_prime < 0.0 {
        h_prime += 6.0;
    }
    h_prime %= 6.0;
    let x = c * (1.0 - (h_prime % 2.0 - 1.0).abs());
    let m = l - c / 2.0;

    let (r1, g1, b1) = if (0.0..1.0).contains(&h_prime) {
        (c, x, 0.0)
    } else if (1.0..2.0).contains(&h_prime) {
        (x, c, 0.0)
    } else if (2.0..3.0).contains(&h_prime) {
        (0.0, c, x)
    } else if (3.0..4.0).contains(&h_prime) {
        (0.0, x, c)
    } else if (4.0..5.0).contains(&h_prime) {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    (r1 + m, g1 + m, b1 + m)
}

pub fn get_zone_for_key(name: &str) -> i32 {
    if name == "zone_0" || name == "c1_btn" {
        return 1;
    }
    if name == "global_color_btn" {
        return 8;
    }
    if name == "zone_1" || name == "c2_btn" {
        return 2;
    }
    if name == "zone_2" || name == "c3_btn" {
        return 3;
    }
    if name == "zone_3" || name == "c4_btn" {
        return 4;
    }
    match name {
        // Zone 4 (WASD) -> hardware zone 3
        "W" | "A" | "S" | "D" => 4,
        // Zone 1 (Left) -> hardware zone 2 on classic OMEN, zone 1 on HyperX 8EEF/8EDD/8EEC
        "P1" | "P2" | "P3" | "P4" | "P5" | "P6" | "Esc" | "F1" | "F2" | "F3" | "F4" | "~" | "1"
        | "2" | "3" | "4" | "5" | "Tab" | "Q" | "E" | "R" | "Caps" | "F" | "Shift" | "Z" | "X"
        | "C" | "V" | "Ctrl" | "Fn" | "Win" | "Alt" => 1,
        // Zone 2 (Middle) -> hardware zone 1 on classic OMEN, zone 2 on HyperX 8EEF/8EDD/8EEC
        "F5" | "F6" | "F7" | "F8" | "F9" | "F10" | "F11" | "F12" | "6" | "7" | "8" | "9" | "0"
        | "-" | "=" | "T" | "Y" | "U" | "I" | "O" | "P" | "[" | "]" | "G" | "H" | "J" | "K"
        | "L" | ";" | "'" | "B" | "N" | "M" | "," | "." | "/" | "Space" | "Alt_R" | "Menu" => 2,
        // Zone 3 (Right: navigation/arrows/numpad/right-hand keys)
        _ => 3,
    }
}

// ── Color Popover Helper ───────────────────────────────────────
#[allow(deprecated)]
pub fn show_color_picker_popover(parent: &gtk::Button, on_color_selected: Rc<dyn Fn(String)>) {
    let popover = gtk::Popover::builder()
        .position(gtk::PositionType::Bottom)
        .autohide(true)
        .build();
    popover.set_parent(parent);

    let container = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .build();

    // Palette: Red, Green, Blue, Purple, Turquoise, Pink, White
    let palette = [
        "#FF0000", "#00FF00", "#0000FF", "#800080", "#40E0D0", "#FFC0CB", "#FFFFFF",
    ];
    let palette_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(4)
        .build();
    for (idx, hex) in palette.iter().enumerate() {
        let btn = gtk::Button::builder()
            .width_request(24)
            .height_request(24)
            .build();
        btn.add_css_class("circular");
        let wname = format!("rgb_swatch_{}", idx);
        btn.set_widget_name(&wname);
        let provider = gtk::CssProvider::new();
        provider.load_from_string(&format!(
            "#{} {{ background: {}; background-color: {}; background-image: none; border: 1px solid rgba(255,255,255,0.35); min-width: 24px; min-height: 24px; padding: 0; }}",
            wname, hex, hex
        ));
        btn.style_context()
            .add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);

        let pop_clone = popover.clone();
        let hex_clone = (*hex).to_string();
        let cb = on_color_selected.clone();
        btn.connect_clicked(move |_| {
            cb(hex_clone.clone());
            pop_clone.popdown();
        });
        palette_box.append(&btn);
    }

    let custom_btn = gtk::Button::builder()
        .icon_name("applications-graphics-symbolic")
        .width_request(24)
        .height_request(24)
        .build();
    custom_btn.add_css_class("circular");
    let cb2 = on_color_selected.clone();

    let parent_win_ref = parent.root().and_downcast::<gtk::Window>();
    let pop_clone2 = popover.clone();

    custom_btn.connect_clicked(move |_| {
        pop_clone2.popdown(); // Hide popover first
        let dialog = gtk::ColorDialog::builder().build();
        let cb_inner = cb2.clone();
        dialog.choose_rgba(
            parent_win_ref.as_ref(),
            None::<&gtk::gdk::RGBA>,
            None::<&gtk::gio::Cancellable>,
            move |res: Result<gtk::gdk::RGBA, glib::Error>| {
                if let Ok(rgba) = res {
                    let hex = format!(
                        "#{:02X}{:02X}{:02X}",
                        (rgba.red() * 255.) as u8,
                        (rgba.green() * 255.) as u8,
                        (rgba.blue() * 255.) as u8
                    );
                    cb_inner(hex);
                }
            },
        );
    });

    container.append(&palette_box);
    container.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    container.append(&custom_btn);

    popover.set_child(Some(&container));
    popover.popup();
}

// ── Interactive Keyboard Builder ─────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SelectionMode {
    Off,
    Key,
    Zone,
    All,
}

#[derive(Clone, Debug)]
pub struct KeyGeom {
    pub id: usize,
    pub name: String,
    pub display: String,
    pub zone: i32,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Integrate animation phase smoothly over real frame delta so speed slider changes
/// never cause phase discontinuities (`anim_time * speed` jumps).
pub fn advance_anim_phase(phase: f64, dt_secs: f64, speed: f64) -> f64 {
    let dt = dt_secs.clamp(0.0, 0.25);
    let spd = speed.clamp(1.0, 100.0);
    (phase + dt * spd).rem_euclid(10_000.0)
}

pub fn fast_hex_val(b: u8) -> f64 {
    match b {
        b'0'..=b'9' => (b - b'0') as f64,
        b'a'..=b'f' => (b - b'a' + 10) as f64,
        b'A'..=b'F' => (b - b'A' + 10) as f64,
        _ => 0.0,
    }
}

pub fn parse_hex(hex: &str) -> (f64, f64, f64) {
    let bytes = hex.as_bytes();
    if bytes.len() >= 7 && bytes[0] == b'#' {
        let r = (fast_hex_val(bytes[1]) * 16.0 + fast_hex_val(bytes[2])) / 255.0;
        let g = (fast_hex_val(bytes[3]) * 16.0 + fast_hex_val(bytes[4])) / 255.0;
        let b = (fast_hex_val(bytes[5]) * 16.0 + fast_hex_val(bytes[6])) / 255.0;
        (r, g, b)
    } else {
        (0.1, 0.1, 0.12)
    }
}

pub fn draw_rounded_rect(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(
        x + r,
        y + h - r,
        r,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    cr.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        3.0 * std::f64::consts::FRAC_PI_2,
    );
    cr.close_path();
}

/// None means metadata is unknown; runtime detection can still be used.
pub fn board_per_key_rgb(board_id: &str) -> Option<bool> {
    let db: serde_json::Value =
        serde_json::from_str(include_str!("../../../omen-space-daemon/src/boards.json")).ok()?;
    db["models"]
        .as_array()?
        .iter()
        .find(|m| {
            m["product_id"]
                .as_str()
                .is_some_and(|id| id.eq_ignore_ascii_case(board_id.trim()))
        })?
        .get("has_per_key_rgb")?
        .as_bool()
}

#[cfg(test)]
mod board_tests {
    #[test]
    fn capabilities_match_the_authoritative_board_database() {
        assert_eq!(super::board_per_key_rgb("8D41"), Some(true));
        for board in ["8901", "8A17", "8BD4", "8EDF"] {
            assert_eq!(super::board_per_key_rgb(board), Some(false));
        }
        assert_eq!(super::board_per_key_rgb("unknown"), None);
    }

    #[test]
    fn four_zone_boundaries_match_omen_hardware_zones() {
        use super::get_zone_for_key;
        // Zone 4: WASD
        for k in ["W", "A", "S", "D"] {
            assert_eq!(get_zone_for_key(k), 4, "{}", k);
        }
        // Zone 1: Left block (including 5 and Left Alt, #393)
        for k in ["Esc", "F4", "5", "R", "F", "V", "Alt"] {
            assert_eq!(get_zone_for_key(k), 1, "{}", k);
        }
        // Zone 2: Middle block (F5..F12, 6..=, T..], G..', B../, Space, Alt_R, Menu, #393)
        for k in [
            "F5", "F9", "F12", "6", "0", "=", "O", "]", "L", "'", ".", "/", "Alt_R", "Menu",
            "Space",
        ] {
            assert_eq!(get_zone_for_key(k), 2, "{}", k);
        }
        // Zone 3: Right / Numpad block (including distinct numpad /_num and *_num, #393)
        for k in [
            "Del",
            "Backspace",
            "\\",
            "Enter",
            "Shift_R",
            "Left",
            "/_num",
            "*_num",
            "+_num",
            "Enter_num",
        ] {
            assert_eq!(get_zone_for_key(k), 3, "{}", k);
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LightbarSettings {
    pub power: bool,
    pub brightness: i32,
    pub mode: String,
    pub speed: i32,
}
impl LightbarSettings {
    pub fn from_snapshot(v: &serde_json::Value) -> Option<Self> {
        Some(Self {
            power: v.get("lightbar_power")?.as_bool()?,
            brightness: v.get("lightbar_brightness")?.as_i64()?.clamp(0, 100) as i32,
            mode: v.get("lightbar_mode")?.as_str()?.to_string(),
            speed: v.get("lightbar_speed")?.as_i64()?.clamp(1, 100) as i32,
        })
    }
    pub fn send(&self) {
        crate::daemon_client::set_lightbar_global_sync(
            self.power,
            self.brightness,
            &self.mode,
            self.speed,
        );
    }
}
impl Default for LightbarSettings {
    fn default() -> Self {
        Self {
            power: true,
            brightness: 100,
            mode: "static".into(),
            speed: 50,
        }
    }
}

#[cfg(test)]
mod lightbar_tests {
    use super::*;
    #[test]
    fn independent_control_changes_preserve_saved_fields() {
        let snapshot = serde_json::json!({"lightbar_power":false,"lightbar_brightness":5,"lightbar_mode":"breathing","lightbar_speed":17});
        let saved = LightbarSettings::from_snapshot(&snapshot).unwrap();
        let mut state = saved.clone();
        state.speed = 23;
        assert_eq!(state.brightness, 5);
        assert!(!state.power);
        assert_eq!(state.mode, "breathing");
        state.power = true;
        assert_eq!(state.brightness, 5);
        assert_eq!(state.speed, 23);
        assert!(LightbarSettings::from_snapshot(&serde_json::json!({})).is_none());
    }
}

pub fn fill_all_key_slots(colors: &mut [String], color: &str) {
    colors.fill(color.to_string());
}

fn normalize_hex_color(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_start_matches('#');
    if trimmed.len() == 6 && trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(format!("#{}", trimmed.to_ascii_uppercase()))
    } else {
        None
    }
}

/// Populate `zone_colors` from a daemon `GetState()` JSON snapshot before building
/// the keyboard preview, so the UI shows the keyboard's current color immediately
/// without polling or extra D-Bus calls.
pub fn apply_zone_colors_from_snapshot(
    snapshot: &serde_json::Value,
    zone_colors: &mut [String],
) -> bool {
    let mut updated = false;
    if let Some(colors) = snapshot.get("colors").and_then(|c| c.as_array()) {
        for (idx, v) in colors.iter().enumerate() {
            if idx >= zone_colors.len() {
                break;
            }
            if let Some(formatted) = v.as_str().and_then(normalize_hex_color) {
                zone_colors[idx] = formatted;
                updated = true;
            }
        }
    } else if let Some(zones) = snapshot.get("zones").and_then(|z| z.as_object()) {
        for (k, v) in zones.iter() {
            if let Ok(idx) = k.parse::<usize>() {
                if idx < zone_colors.len() {
                    if let Some(formatted) = v.as_str().and_then(normalize_hex_color) {
                        zone_colors[idx] = formatted;
                        updated = true;
                    }
                }
            }
        }
    }
    updated
}

#[cfg(test)]
mod all_key_tests {
    #[test]
    fn all_includes_the_six_slots_absent_from_the_preview() {
        let mut frame = vec!["0099ED".to_string(); 104];
        super::fill_all_key_slots(&mut frame, "FF0000");
        assert_eq!(frame.len(), 104);
        assert!(frame.iter().all(|c| c == "FF0000"));
    }

    #[test]
    fn zone_colors_hydrate_from_daemon_snapshot_before_preview_build() {
        let mut zones = vec!["#0099ED".to_string(); 7];
        let snap = serde_json::json!({
            "colors": ["FFFFFF", "#ff0055", "00ffaa", "123456"]
        });
        assert!(super::apply_zone_colors_from_snapshot(&snap, &mut zones));
        assert_eq!(zones[0], "#FFFFFF");
        assert_eq!(zones[1], "#FF0055");
        assert_eq!(zones[2], "#00FFAA");
        assert_eq!(zones[3], "#123456");
        // Untouched slots keep their fallback
        assert_eq!(zones[4], "#0099ED");
    }

    #[test]
    fn zone_colors_ignore_invalid_or_missing_entries() {
        let mut zones = vec!["#0099ED".to_string(); 4];
        let snap = serde_json::json!({ "colors": ["nothex", ""] });
        assert!(!super::apply_zone_colors_from_snapshot(&snap, &mut zones));
        assert_eq!(zones[0], "#0099ED");
    }
}

#[derive(Clone, Debug)]
pub struct KeyboardGlobals {
    pub power: bool,
    pub brightness: i32,
    pub direction: String,
}
impl KeyboardGlobals {
    pub fn from_snapshot(v: Option<&serde_json::Value>) -> Self {
        Self {
            power: v.and_then(|v| v["power"].as_bool()).unwrap_or(true),
            brightness: v
                .and_then(|v| v["brightness"].as_i64())
                .unwrap_or(100)
                .clamp(0, 100) as i32,
            direction: v
                .and_then(|v| v["direction"].as_str())
                .unwrap_or("ltr")
                .to_string(),
        }
    }
    pub fn send(&self) {
        crate::daemon_client::set_global_sync(self.power, self.brightness, &self.direction);
    }
}

/// Runtime HID evidence takes precedence over an optional board capability hint.
pub fn detect_per_key_rgb(board_id: &str, runtime_available: bool) -> bool {
    runtime_available || board_per_key_rgb(board_id).unwrap_or(false)
}

pub fn restored_selection_mode(current: SelectionMode, kb_mode: KeyboardMode) -> SelectionMode {
    if current == SelectionMode::Off {
        if kb_mode == KeyboardMode::PerKey {
            SelectionMode::Key
        } else {
            SelectionMode::Zone
        }
    } else {
        current
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyboardLayout {
    En,
    De,
    TrQ,
}

impl KeyboardLayout {
    pub const LABELS: [&'static str; 3] = ["EN", "DE", "TR-Q"];

    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::De => "de",
            Self::TrQ => "trq",
        }
    }

    pub fn index(self) -> u32 {
        match self {
            Self::En => 0,
            Self::De => 1,
            Self::TrQ => 2,
        }
    }

    pub fn from_index(idx: u32) -> Self {
        match idx {
            1 => Self::De,
            2 => Self::TrQ,
            _ => Self::En,
        }
    }

    pub fn from_saved(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "de" | "german" => Self::De,
            "trq" | "tr-q" | "tr_q" | "tr" | "turkish" => Self::TrQ,
            _ => Self::En,
        }
    }

    /// Display-only keycap overrides for the US physical positions.
    /// HID usage mapping is unchanged, so this never affects which key lights up.
    pub fn keycap_labels(self) -> std::collections::HashMap<String, String> {
        let pairs: &[(&str, &str)] = match self {
            Self::En => &[],
            Self::De => &[
                ("~", "^"),
                ("-", "ß"),
                ("=", "´"),
                ("[", "Ü"),
                ("]", "+"),
                ("\\", "#"),
                (";", "Ö"),
                ("'", "Ä"),
                ("Z", "Y"),
                ("Y", "Z"),
                ("/", "-"),
                ("Del", "Entf"),
                ("Ctrl", "Strg"),
                ("Alt_R", "AltGr"),
            ],
            Self::TrQ => &[
                ("~", "\""),
                ("-", "*"),
                ("=", "-"),
                ("[", "Ğ"),
                ("]", "Ü"),
                ("\\", ","),
                (";", "Ş"),
                ("'", "İ"),
                (",", "Ö"),
                (".", "Ç"),
                ("/", "."),
                ("Alt_R", "AltGr"),
            ],
        };
        pairs
            .iter()
            .map(|(us, local)| ((*us).to_string(), (*local).to_string()))
            .collect()
    }
}

#[cfg(test)]
mod capability_detection_tests {
    #[test]
    fn board_metadata_never_downgrades_detected_hardware() {
        assert!(!super::detect_per_key_rgb("8901", false));
        assert!(super::detect_per_key_rgb("8901", true));
        assert!(super::detect_per_key_rgb("8D41", false));
        assert!(super::detect_per_key_rgb("unknown", true));
        assert!(!super::detect_per_key_rgb("unknown", false));
    }

    #[test]
    fn restored_selection_mode_leaves_off_for_active_mode() {
        use super::{restored_selection_mode, KeyboardMode, SelectionMode};
        assert_eq!(
            restored_selection_mode(SelectionMode::Off, KeyboardMode::PerKey),
            SelectionMode::Key
        );
        assert_eq!(
            restored_selection_mode(SelectionMode::Off, KeyboardMode::Omen4Zone),
            SelectionMode::Zone
        );
        assert_eq!(
            restored_selection_mode(SelectionMode::Off, KeyboardMode::Victus1Zone),
            SelectionMode::Zone
        );
        assert_eq!(
            restored_selection_mode(SelectionMode::All, KeyboardMode::PerKey),
            SelectionMode::All
        );
    }

    #[test]
    fn keyboard_layout_round_trips_and_maps_trq_keycaps() {
        use super::KeyboardLayout;

        assert_eq!(KeyboardLayout::LABELS, ["EN", "DE", "TR-Q"]);
        for (raw, expected) in [
            ("en", KeyboardLayout::En),
            ("DE", KeyboardLayout::De),
            ("german", KeyboardLayout::De),
            ("trq", KeyboardLayout::TrQ),
            ("TR-Q", KeyboardLayout::TrQ),
            ("tr_q", KeyboardLayout::TrQ),
            ("tr", KeyboardLayout::TrQ),
            ("turkish", KeyboardLayout::TrQ),
            ("unknown", KeyboardLayout::En),
        ] {
            assert_eq!(KeyboardLayout::from_saved(raw), expected, "raw={raw}");
        }

        for layout in [KeyboardLayout::En, KeyboardLayout::De, KeyboardLayout::TrQ] {
            assert_eq!(KeyboardLayout::from_saved(layout.code()), layout);
            assert_eq!(KeyboardLayout::from_index(layout.index()), layout);
        }
        assert_eq!(KeyboardLayout::from_index(99), KeyboardLayout::En);

        assert!(KeyboardLayout::En.keycap_labels().is_empty());

        let trq = KeyboardLayout::TrQ.keycap_labels();
        for (us, tr) in [
            ("~", "\""),
            ("-", "*"),
            ("=", "-"),
            ("[", "Ğ"),
            ("]", "Ü"),
            ("\\", ","),
            (";", "Ş"),
            ("'", "İ"),
            (",", "Ö"),
            (".", "Ç"),
            ("/", "."),
            ("Alt_R", "AltGr"),
        ] {
            assert_eq!(trq.get(us).map(String::as_str), Some(tr), "key={us}");
        }
    }

    #[test]
    fn anim_phase_advances_continuously_when_speed_changes() {
        let p0 = super::advance_anim_phase(0.0, 0.016, 20.0);
        let p1 = super::advance_anim_phase(p0, 0.016, 80.0);
        assert!(p1 > p0, "phase must remain monotonic across speed change");
        // Compare against naive `t * speed` jump: phase delta over one 16ms frame at speed=80
        // must be 0.016 * 80 = 1.28 regardless of accumulated prior phase.
        let p_late = super::advance_anim_phase(500.0, 0.016, 80.0);
        assert!((p_late - 501.28).abs() < 1e-6);
    }

    #[test]
    fn resource_preview_phase_and_color_math_stays_microsecond_fast() {
        let start = std::time::Instant::now();
        let mut phase = 0.0f64;
        let mut acc = 0.0f64;
        for frame in 0..1_000usize {
            phase = super::advance_anim_phase(phase, 0.025, 65.0);
            for key_id in 0..98usize {
                let x = (key_id % 18) as f64 * 38.0;
                let hue =
                    ((phase * 4.0) - (x / 10.0) * 8.0 + frame as f64 * 0.001).rem_euclid(360.0);
                let (r, g, b) = super::hsl_to_rgb(hue, 1.0, 0.5);
                acc += r + g + b;
            }
        }
        let elapsed_us = start.elapsed().as_secs_f64() * 1_000_000.0;
        let per_frame_us = elapsed_us / 1_000.0;
        println!(
            "GUI_PREVIEW_BENCH: total_us={:.1} per_frame_us={:.2} fps40_cpu_pct={:.4}% checksum={:.1}",
            elapsed_us,
            per_frame_us,
            (per_frame_us / 25_000.0) * 100.0,
            acc
        );
        assert!(
            per_frame_us < 500.0,
            "GUI preview math exceeded 500 us/frame budget: {:.2} us",
            per_frame_us
        );
    }
}
