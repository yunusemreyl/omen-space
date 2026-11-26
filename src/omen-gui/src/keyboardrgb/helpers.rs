use gtk::prelude::*;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum KeyboardMode {
    Victus1Zone,
    Omen4Zone,
    #[allow(dead_code)]
    PerKey,
    DesktopRgb,
}


pub fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (f64, f64, f64) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let mut h_prime = h / 60.0;
    while h_prime < 0.0 { h_prime += 6.0; }
    h_prime %= 6.0;
    let x = c * (1.0 - (h_prime % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    
    let (r1, g1, b1) = if (0.0..1.0).contains(&h_prime) { (c, x, 0.0) }
    else if (1.0..2.0).contains(&h_prime) { (x, c, 0.0) }
    else if (2.0..3.0).contains(&h_prime) { (0.0, c, x) }
    else if (3.0..4.0).contains(&h_prime) { (0.0, x, c) }
    else if (4.0..5.0).contains(&h_prime) { (x, 0.0, c) }
    else { (c, 0.0, x) };
    
    (r1 + m, g1 + m, b1 + m)
}

pub fn get_zone_for_key(name: &str) -> i32 {
    if name == "zone_0" || name == "c1_btn" { return 1; }
    if name == "global_color_btn" { return 8; }
    if name == "zone_1" || name == "c2_btn" { return 2; }
    if name == "zone_2" || name == "c3_btn" { return 3; }
    if name == "zone_3" || name == "c4_btn" { return 4; }
    match name {
        // Zone 4 (WASD) -> hardware zone 3
        "W" | "A" | "S" | "D" => 4,
        // Zone 1 (Left) -> hardware zone 2 on classic OMEN, zone 1 on HyperX 8EEF/8EDD/8EEC
        "P1" | "P2" | "P3" | "P4" | "P5" | "P6" | "Esc" | "F1" | "F2" | "F3" | "F4" | "~"
        | "1" | "2" | "3" | "4" | "5" | "Tab" | "Q" | "E" | "R" | "Caps" | "F" | "Shift"
        | "Z" | "X" | "C" | "V" | "Ctrl" | "Fn" | "Win" | "Alt" => 1,
        // Zone 2 (Middle) -> hardware zone 1 on classic OMEN, zone 2 on HyperX 8EEF/8EDD/8EEC
        "F5" | "F6" | "F7" | "F8" | "F9" | "F10" | "F11" | "F12" | "6" | "7" | "8" | "9"
        | "0" | "-" | "=" | "T" | "Y" | "U" | "I" | "O" | "P" | "[" | "]" | "G" | "H" | "J"
        | "K" | "L" | ";" | "'" | "B" | "N" | "M" | "," | "." | "/" | "Space" | "Alt_R"
        | "Menu" => 2,
        // Zone 3 (Right: navigation/arrows/numpad/right-hand keys)
        _ => 3,
    }
}

// ── Color Popover Helper ───────────────────────────────────────
#[allow(deprecated)]
pub fn show_color_picker_popover(
    parent: &gtk::Button,
    on_color_selected: Rc<dyn Fn(String)>
) {
    let popover = gtk::Popover::builder().position(gtk::PositionType::Bottom).autohide(true).build();
    popover.set_parent(parent);
    
    let container = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(6).margin_top(6).margin_bottom(6).margin_start(6).margin_end(6).build();
    
    // Palette: Red, Green, Blue, Purple, Turquoise, Pink, White
    let palette = ["#FF0000", "#00FF00", "#0000FF", "#800080", "#40E0D0", "#FFC0CB", "#FFFFFF"];
    let palette_box = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(4).build();
    for (idx, hex) in palette.iter().enumerate() {
        let btn = gtk::Button::builder().width_request(24).height_request(24).build();
        btn.add_css_class("circular");
        let wname = format!("rgb_swatch_{}", idx);
        btn.set_widget_name(&wname);
        let provider = gtk::CssProvider::new();
        provider.load_from_string(&format!(
            "#{} {{ background: {}; background-color: {}; background-image: none; border: 1px solid rgba(255,255,255,0.35); min-width: 24px; min-height: 24px; padding: 0; }}",
            wname, hex, hex
        ));
        btn.style_context().add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
        
        let pop_clone = popover.clone();
        let hex_clone = (*hex).to_string();
        let cb = on_color_selected.clone();
        btn.connect_clicked(move |_| {
            cb(hex_clone.clone());
            pop_clone.popdown();
        });
        palette_box.append(&btn);
    }
    
    let custom_btn = gtk::Button::builder().icon_name("applications-graphics-symbolic").width_request(24).height_request(24).build();
    custom_btn.add_css_class("circular");
    let cb2 = on_color_selected.clone();
    
    let parent_win_ref = parent.root().and_downcast::<gtk::Window>();
    let pop_clone2 = popover.clone();
    
    custom_btn.connect_clicked(move |_| {
        pop_clone2.popdown(); // Hide popover first
        let dialog = gtk::ColorDialog::builder().build();
        let cb_inner = cb2.clone();
        dialog.choose_rgba(parent_win_ref.as_ref(), None::<&gtk::gdk::RGBA>, None::<&gtk::gio::Cancellable>, move |res: Result<gtk::gdk::RGBA, glib::Error>| {
            if let Ok(rgba) = res {
                let hex = format!("#{:02X}{:02X}{:02X}", (rgba.red()*255.) as u8, (rgba.green()*255.) as u8, (rgba.blue()*255.) as u8);
                cb_inner(hex);
            }
        });
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
    #[allow(dead_code)]
    Row,
    Zone,
    All,
}

#[derive(Clone, Debug)]
pub struct KeyGeom {
    pub id: usize,
    pub name: String,
    pub display: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
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
    cr.arc(x + r, y + h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    cr.arc(x + r, y + r, r, std::f64::consts::PI, 3.0 * std::f64::consts::FRAC_PI_2);
    cr.close_path();
}

/// None means metadata is unknown; runtime detection can still be used.
pub fn board_per_key_rgb(board_id: &str) -> Option<bool> {
    let db: serde_json::Value = serde_json::from_str(include_str!("../../../omen-space-daemon/src/boards.json")).ok()?;
    db["models"].as_array()?.iter()
        .find(|m| m["product_id"].as_str().is_some_and(|id| id.eq_ignore_ascii_case(board_id.trim())))?
        .get("has_per_key_rgb")?.as_bool()
}

#[cfg(test)]
mod board_tests {
    #[test]
    fn capabilities_match_the_authoritative_board_database() {
        assert_eq!(super::board_per_key_rgb("8D41"),Some(true));
        for board in ["8901","8A17","8BD4","8EDF"] { assert_eq!(super::board_per_key_rgb(board),Some(false)); }
        assert_eq!(super::board_per_key_rgb("unknown"),None);
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
        for k in ["F5", "F9", "F12", "6", "0", "=", "O", "]", "L", "'", ".", "/", "Alt_R", "Menu", "Space"] {
            assert_eq!(get_zone_for_key(k), 2, "{}", k);
        }
        // Zone 3: Right / Numpad block (including distinct numpad /_num and *_num, #393)
        for k in ["Del", "Backspace", "\\", "Enter", "Shift_R", "Left", "/_num", "*_num", "+_num", "Enter_num"] {
            assert_eq!(get_zone_for_key(k), 3, "{}", k);
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LightbarSettings {
    pub power: bool, pub brightness: i32, pub mode: String, pub speed: i32,
}
impl LightbarSettings {
    pub fn from_snapshot(v: &serde_json::Value) -> Option<Self> {
        Some(Self {
            power: v.get("lightbar_power")?.as_bool()?,
            brightness: v.get("lightbar_brightness")?.as_i64()?.clamp(0,100) as i32,
            mode: v.get("lightbar_mode")?.as_str()?.to_string(),
            speed: v.get("lightbar_speed")?.as_i64()?.clamp(1,100) as i32,
        })
    }
    pub fn send(&self) {
        crate::daemon_client::set_lightbar_global_sync(self.power,self.brightness,&self.mode,self.speed);
    }
}
impl Default for LightbarSettings {
    fn default() -> Self { Self {power:true,brightness:100,mode:"static".into(),speed:50} }
}

#[cfg(test)]
mod lightbar_tests {
    use super::*;
    #[test]
    fn independent_control_changes_preserve_saved_fields() {
        let snapshot=serde_json::json!({"lightbar_power":false,"lightbar_brightness":5,"lightbar_mode":"breathing","lightbar_speed":17});
        let saved=LightbarSettings::from_snapshot(&snapshot).unwrap();
        let mut state=saved.clone();state.speed=23;
        assert_eq!(state.brightness,5);assert!(!state.power);assert_eq!(state.mode,"breathing");
        state.power=true;assert_eq!(state.brightness,5);assert_eq!(state.speed,23);
        assert!(LightbarSettings::from_snapshot(&serde_json::json!({})).is_none());
    }
}

pub fn fill_all_key_slots(colors: &mut [String], color: &str) { colors.fill(color.to_string()); }

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
pub fn apply_zone_colors_from_snapshot(snapshot: &serde_json::Value, zone_colors: &mut [String]) -> bool {
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
    #[test] fn all_includes_the_six_slots_absent_from_the_preview() {
        let mut frame=vec!["0099ED".to_string();104];
        super::fill_all_key_slots(&mut frame,"FF0000");
        assert_eq!(frame.len(),104);assert!(frame.iter().all(|c|c=="FF0000"));
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
pub struct KeyboardGlobals { pub power:bool, pub brightness:i32, pub direction:String }
impl KeyboardGlobals {
    pub fn from_snapshot(v:Option<&serde_json::Value>) -> Self {
        Self {
            power:v.and_then(|v|v["power"].as_bool()).unwrap_or(true),
            brightness:v.and_then(|v|v["brightness"].as_i64()).unwrap_or(100).clamp(0,100) as i32,
            direction:v.and_then(|v|v["direction"].as_str()).unwrap_or("ltr").to_string(),
        }
    }
    pub fn send(&self) {crate::daemon_client::set_global_sync(self.power,self.brightness,&self.direction);}
}

/// Runtime HID evidence takes precedence over an optional board capability hint.
pub fn detect_per_key_rgb(board_id: &str, runtime_available: bool) -> bool {
    runtime_available || board_per_key_rgb(board_id).unwrap_or(false)
}
#[cfg(test)]
mod capability_detection_tests {
    #[test] fn board_metadata_never_downgrades_detected_hardware() {
        assert!(!super::detect_per_key_rgb("8901",false));
        assert!(super::detect_per_key_rgb("8901",true));
        assert!(super::detect_per_key_rgb("8D41",false));
        assert!(super::detect_per_key_rgb("unknown",true));
        assert!(!super::detect_per_key_rgb("unknown",false));
    }
}
