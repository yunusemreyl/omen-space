use gtk::prelude::*;
use libadwaita as adw;
use adw::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::collections::HashMap;

/* ─────────────────────────────────────────────────────────────
   keyboardrgb.rs — Aydınlatma (Klavye & Lightbar RGB Kontrolü)
   OMEN 4-Zone / Victus 1-Zone + 4-Segment Lightbar Desteği (Ref: OmenCore)
   ───────────────────────────────────────────────────────────── */

#[derive(Clone, Copy, PartialEq, Debug)]
enum KeyboardMode {
    Victus1Zone,
    Omen4Zone,
    #[allow(dead_code)]
    PerKey,
    DesktopRgb,
}

fn get_active_keyboard_mode(detected: KeyboardMode) -> KeyboardMode {
    if let Ok(home) = std::env::var("HOME") {
        let path = format!("{}/.config/omenspace/settings.json", home);
        if let Ok(json_str) = std::fs::read_to_string(&path) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&json_str) {
                if let Some(zo) = json.get("zone_override").and_then(|v| v.as_u64()) { 
                    return match zo {
                        1 => KeyboardMode::Omen4Zone,
                        2 => KeyboardMode::Victus1Zone,
                        3 => KeyboardMode::PerKey,
                        4 => KeyboardMode::DesktopRgb,
                        _ => detected,
                    };
                }
            }
        }
    }
    detected
}


fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (f64, f64, f64) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let mut h_prime = h / 60.0;
    while h_prime < 0.0 { h_prime += 6.0; }
    h_prime = h_prime % 6.0;
    let x = c * (1.0 - (h_prime % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    
    let (r1, g1, b1) = if h_prime >= 0.0 && h_prime < 1.0 { (c, x, 0.0) }
    else if h_prime >= 1.0 && h_prime < 2.0 { (x, c, 0.0) }
    else if h_prime >= 2.0 && h_prime < 3.0 { (0.0, c, x) }
    else if h_prime >= 3.0 && h_prime < 4.0 { (0.0, x, c) }
    else if h_prime >= 4.0 && h_prime < 5.0 { (x, 0.0, c) }
    else { (c, 0.0, x) };
    
    (r1 + m, g1 + m, b1 + m)
}

fn get_zone_for_key(name: &str) -> i32 {
    if name == "zone_0" || name == "c1_btn" { return 1; }
    if name == "global_color_btn" { return 8; }
    if name == "zone_1" || name == "c2_btn" { return 2; }
    if name == "zone_2" || name == "c3_btn" { return 3; }
    if name == "zone_3" || name == "c4_btn" { return 4; }
    match name {
        "W" | "A" | "S" | "D" => 4,
        "Esc" | "F1" | "F2" | "F3" | "F4" | "~" | "1" | "2" | "3" | "4" | 
        "Tab" | "Q" | "E" | "R" | "Caps" | "F" | "Shift" | "Z" | "X" | "C" | "V" | 
        "Ctrl" | "Win" => 1,
        "F5" | "F6" | "F7" | "F8" | "5" | "6" | "7" | "8" | "T" | "Y" | "U" | "I" |
        "G" | "H" | "J" | "K" | "B" | "N" | "M" | "," | "Alt" | "Space" => 2,
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
    for hex in palette {
        let btn = gtk::Button::builder().width_request(24).height_request(24).build();
        btn.add_css_class("circular");
        let provider = gtk::CssProvider::new();
        provider.load_from_string(&format!("button {{ background: {}; min-width: 24px; min-height: 24px; padding: 0; }}", hex));
        btn.style_context().add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
        
        let pop_clone = popover.clone();
        let hex_clone = hex.to_string();
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
enum SelectionMode {
    Off,
    Key,
    Row,
    Zone,
    All,
}

#[derive(Clone, Debug)]
struct KeyGeom {
    id: usize,
    name: String,
    display: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

fn fast_hex_val(b: u8) -> f64 {
    match b {
        b'0'..=b'9' => (b - b'0') as f64,
        b'a'..=b'f' => (b - b'a' + 10) as f64,
        b'A'..=b'F' => (b - b'A' + 10) as f64,
        _ => 0.0,
    }
}

fn parse_hex(hex: &str) -> (f64, f64, f64) {
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

fn draw_rounded_rect(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    cr.arc(x + r, y + r, r, std::f64::consts::PI, 3.0 * std::f64::consts::FRAC_PI_2);
    cr.close_path();
}

fn build_interactive_keyboard(
    detected_mode: KeyboardMode,
    zone_colors: Rc<RefCell<Vec<String>>>,
    per_key_colors: Rc<RefCell<Vec<String>>>,
    state_json_opt: &Option<serde_json::Value>
) -> (gtk::Box, Rc<dyn Fn(&str, f64)>, gtk::Box) {
    let kb_card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["os-card"])
        .spacing(12)
        .margin_top(16).margin_bottom(16).margin_start(16).margin_end(16)
        .halign(gtk::Align::Center)
        .build();

    // 1. Top Effects Tabs
    let (effect_labels, mode_str_list) = match detected_mode {
        KeyboardMode::Victus1Zone => (
            vec![crate::i18n::t("effect_static"), crate::i18n::t("effect_breathing"), crate::i18n::t("effect_cycle")],
            vec!["static", "breathing", "cycle"]
        ),
        KeyboardMode::Omen4Zone => (
            vec![
                crate::i18n::t("effect_static"), 
                crate::i18n::t("effect_breathing"), 
                crate::i18n::t("effect_blinking"), 
                crate::i18n::t("effect_cycle"), 
                crate::i18n::t("effect_wave_custom"), 
                crate::i18n::t("effect_wave_rainbow")
            ],
            vec!["static", "breathing", "blinking", "cycle", "wave", "wave_rainbow"]
        ),
        KeyboardMode::PerKey => (
            vec![
                crate::i18n::t("effect_static"),
                "Per-Key Custom",
                crate::i18n::t("effect_breathing"),
                crate::i18n::t("effect_blinking"),
                crate::i18n::t("effect_cycle"),
                crate::i18n::t("effect_wave_custom"),
                crate::i18n::t("effect_wave_rainbow"),
                crate::i18n::t("effect_starlight"),
                crate::i18n::t("effect_marquee"),
                crate::i18n::t("effect_reactive"),
                crate::i18n::t("effect_ripple"),
                crate::i18n::t("effect_raindrop")
            ],
            vec!["static", "per_key_custom", "breathing", "blinking", "cycle", "wave", "wave_rainbow", "starlight", "marquee", "reactive", "ripple", "raindrop"]
        ),
        KeyboardMode::DesktopRgb => (
            vec![
                crate::i18n::t("effect_static"),
                crate::i18n::t("effect_breathing"),
                crate::i18n::t("effect_cycle"),
                crate::i18n::t("effect_blinking"),
                crate::i18n::t("effect_wave"),
            ],
            vec!["static", "breathing", "cycle", "blinking", "wave"]
        ),
    };

    let top_bar = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(12).build();

    // -- Selection Tools (Left-aligned) --
    let sel_box = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(6).build();
    let sel_label = gtk::Label::builder().label("Select:").css_classes(["dim-label"]).build();
    sel_box.append(&sel_label);
    
    let sel_btns_box = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(0).build();
    sel_btns_box.add_css_class("linked");
    
    let btn_off = gtk::ToggleButton::builder().label("Off").build();
    let btn_key = gtk::ToggleButton::builder().label("Key").active(detected_mode == KeyboardMode::PerKey).sensitive(detected_mode == KeyboardMode::PerKey).build();
    let btn_zone = gtk::ToggleButton::builder().label("Zone").active(detected_mode != KeyboardMode::PerKey).build();
    let btn_all = gtk::ToggleButton::builder().label("All").build();
    
    // Set initial sel_mode based on supported mode
    let sel_mode = Rc::new(RefCell::new(if detected_mode == KeyboardMode::PerKey { SelectionMode::Key } else { SelectionMode::Zone }));

    btn_key.set_group(Some(&btn_off));
    btn_zone.set_group(Some(&btn_off));
    btn_all.set_group(Some(&btn_off));
    
    let sm_o = sel_mode.clone(); btn_off.connect_toggled(move |b| if b.is_active() { *sm_o.borrow_mut() = SelectionMode::Off; });
    let sm_k = sel_mode.clone(); btn_key.connect_toggled(move |b| if b.is_active() { *sm_k.borrow_mut() = SelectionMode::Key; });
    let sm_z = sel_mode.clone(); btn_zone.connect_toggled(move |b| if b.is_active() { *sm_z.borrow_mut() = SelectionMode::Zone; });
    let sm_a = sel_mode.clone(); btn_all.connect_toggled(move |b| if b.is_active() { *sm_a.borrow_mut() = SelectionMode::All; });
    
    sel_btns_box.append(&btn_off);
    sel_btns_box.append(&btn_key);
    sel_btns_box.append(&btn_zone);
    sel_btns_box.append(&btn_all);
    sel_box.append(&sel_btns_box);
    top_bar.append(&sel_box);

    // Spacer to push effects dropdown to the right
    let spacer = gtk::Box::builder().hexpand(true).build();
    top_bar.append(&spacer);

    // -- Effects Dropdown (Right-aligned) --
    let active_mode_idx = Rc::new(RefCell::new(0));

    let mut current_mode_str = "static".to_string();
    if let Some(state_json) = state_json_opt {
        if let Some(ms) = state_json["mode"].as_str() {
            current_mode_str = ms.to_string();
        }
    }
    
    let mut speed_val = 50.0;
    if let Some(state_json) = state_json_opt {
        if let Some(speed) = state_json["speed"].as_f64() {
            speed_val = speed;
        }
    }
    let speed_rc = Rc::new(RefCell::new(speed_val));

    let apply_anim_rc: Rc<RefCell<Option<Rc<dyn Fn(&str, f64)>>>> = Rc::new(RefCell::new(None));
    let aa_hook = apply_anim_rc.clone();
    let msl_c: Vec<String> = mode_str_list.iter().map(|s| s.to_string()).collect();

    let effect_model = gtk::StringList::new(&effect_labels);
    let effects_dropdown = gtk::DropDown::builder()
        .model(&effect_model)
        .valign(gtk::Align::Center)
        .build();

    if let Some(pos) = msl_c.iter().position(|s| s == &current_mode_str) {
        effects_dropdown.set_selected(pos as u32);
        *active_mode_idx.borrow_mut() = pos;
    }

    let am_idx = active_mode_idx.clone();
    let msl_local = msl_c.clone();
    let aa_local = aa_hook.clone();
    let speed_ref = speed_rc.clone();
    effects_dropdown.connect_selected_notify(move |dd| {
        let i = dd.selected() as usize;
        if i < msl_local.len() {
            *am_idx.borrow_mut() = i;
            let m = &msl_local[i];
            let speed = *speed_ref.borrow();
            if m == "wave_ltr" {
                crate::daemon_client::set_mode_sync("wave", speed as i32);
                crate::daemon_client::set_global_sync(true, 100, "ltr");
            } else if m == "wave_rtl" {
                crate::daemon_client::set_mode_sync("wave", speed as i32);
                crate::daemon_client::set_global_sync(true, 100, "rtl");
            } else {
                crate::daemon_client::set_mode_sync(m, speed as i32);
            }
            if let Some(anim_func) = &*aa_local.borrow() {
                let anim_m = if m.starts_with("wave_") { "wave" } else { m.as_str() };
                anim_func(anim_m, speed);
            }
        }
    });

    top_bar.append(&gtk::Label::builder().label("Effect:").css_classes(["dim-label"]).build());
    top_bar.append(&effects_dropdown);

    kb_card.append(&top_bar);

    // 3. Canvas
    let layout_def = vec![
        vec![("Esc", 1.0), ("", 0.5), ("F1", 1.0), ("F2", 1.0), ("F3", 1.0), ("F4", 1.0), ("", 0.5), ("F5", 1.0), ("F6", 1.0), ("F7", 1.0), ("F8", 1.0), ("", 0.5), ("F9", 1.0), ("F10", 1.0), ("F11", 1.0), ("F12", 1.0), ("", 0.5), ("Del", 1.0), ("Omen", 1.0), ("Calc", 1.0), ("Power", 1.0)],
        vec![("~", 1.0), ("1", 1.0), ("2", 1.0), ("3", 1.0), ("4", 1.0), ("5", 1.0), ("6", 1.0), ("7", 1.0), ("8", 1.0), ("9", 1.0), ("0", 1.0), ("-", 1.0), ("=", 1.0), ("Backspace", 2.0), ("Num", 1.0), ("/", 1.0), ("*", 1.0), ("-_num", 1.0)],
        vec![("Tab", 1.5), ("Q", 1.0), ("W", 1.0), ("E", 1.0), ("R", 1.0), ("T", 1.0), ("Y", 1.0), ("U", 1.0), ("I", 1.0), ("O", 1.0), ("P", 1.0), ("[", 1.0), ("]", 1.0), ("\\", 1.5), ("7_num", 1.0), ("8_num", 1.0), ("9_num", 1.0), ("+", 1.0)],
        vec![("Caps", 1.75), ("A", 1.0), ("S", 1.0), ("D", 1.0), ("F", 1.0), ("G", 1.0), ("H", 1.0), ("J", 1.0), ("K", 1.0), ("L", 1.0), (";", 1.0), ("'", 1.0), ("Enter", 2.25), ("4_num", 1.0), ("5_num", 1.0), ("6_num", 1.0), ("", 1.0)],
        vec![("Shift", 2.25), ("Z", 1.0), ("X", 1.0), ("C", 1.0), ("V", 1.0), ("B", 1.0), ("N", 1.0), ("M", 1.0), (",", 1.0), (".", 1.0), ("/", 1.0), ("Shift_R", 1.75), ("Up", 1.0), ("1_num", 1.0), ("2_num", 1.0), ("3_num", 1.0), ("Ent", 1.0)],
        vec![("Ctrl", 1.25), ("Fn", 1.25), ("Win", 1.25), ("Alt", 1.25), ("Space", 5.5), ("Alt_R", 1.25), ("Menu", 1.25), ("Left", 1.0), ("Down", 1.0), ("Right", 1.0), ("0_num", 1.0), ("._num", 1.0), ("", 1.0)]
    ];
    let unit_size = 38.0;
    let margin = 3.5;
    let height = 34.0;

    let mut keys = Vec::new();
    let mut global_idx = 0;
    let mut current_y = 0.0;
    let key_colors = Rc::new(RefCell::new(HashMap::<String, String>::new()));

    for row_keys in &layout_def {
        let mut current_x = 0.0;
        for (name, size_mult) in row_keys {
            let width = unit_size * *size_mult;
            if !name.is_empty() {
                let display_name = if let Some(stripped) = name.strip_suffix("_R") { stripped }
                    else if let Some(stripped) = name.strip_suffix("_num") { stripped }
                    else { name };
                
                let def_color = if detected_mode == KeyboardMode::Omen4Zone {
                    zone_colors.borrow()[(get_zone_for_key(name) - 1) as usize].clone()
                } else if detected_mode == KeyboardMode::Victus1Zone {
                    zone_colors.borrow()[0].clone()
                } else if detected_mode == KeyboardMode::PerKey {
                    if global_idx < per_key_colors.borrow().len() { per_key_colors.borrow()[global_idx].clone() } else { "#0099ED".to_string() }
                } else {
                    "#0099ED".to_string()
                };
                key_colors.borrow_mut().insert(name.to_string(), def_color);

                let k_height = if *name == "+" || *name == "Ent" { height * 2.0 + margin } else { height };
                keys.push(KeyGeom {
                    id: global_idx,
                    name: name.to_string(),
                    display: display_name.to_string(),
                    x: current_x,
                    y: current_y,
                    w: width - margin,
                    h: k_height,
                });
                global_idx += 1;
            }
            current_x += width;
        }
        current_y += height + margin;
    }

    let drawing_area = gtk::DrawingArea::builder()
        .width_request(760)
        .height_request(260)
        .halign(gtk::Align::Center)
        .build();

    let keys_rc = Rc::new(keys);
    let selected_keys = Rc::new(RefCell::new(Vec::<String>::new()));
    let anim_state = Rc::new(RefCell::new(("static".to_string(), 50.0, 0.0f64)));
    let anim_draw = anim_state.clone();

    let hovered_key = Rc::new(RefCell::new(None::<String>));

    let keys_draw = keys_rc.clone();
    let selected_draw = selected_keys.clone();
    let hovered_draw = hovered_key.clone();
    let colors_draw = key_colors.clone();
    let sel_mode_draw = sel_mode.clone();
    
    drawing_area.set_draw_func(move |_, cr, _, _| {
        cr.set_source_rgba(0.08, 0.08, 0.09, 1.0);
        let _ = cr.paint();
        let sel = selected_draw.borrow();
        let hov = hovered_draw.borrow();
        let cols = colors_draw.borrow();
        cr.translate(10.0, 10.0);
        
        let (anim_mode, anim_speed, anim_time) = anim_draw.borrow().clone();
        for k in keys_draw.iter() {
            let hex = cols.get(&k.name).map(|s| s.as_str()).unwrap_or("#141418");
            let (mut r, mut g, mut b) = parse_hex(hex);

            if anim_mode == "breathing" {
                let factor = (anim_time * anim_speed * 0.05).sin().abs() * 0.8 + 0.2;
                r *= factor; g *= factor; b *= factor;
            } else if anim_mode == "cycle" {
                let h = (anim_time * anim_speed * 2.0) % 360.0;
                let (nr, ng, nb) = hsl_to_rgb(h, 1.0, 0.5);
                r = nr; g = ng; b = nb;
            } else if anim_mode == "wave" {
                let offset = k.x / 10.0;
                let h = ((anim_time * anim_speed * 4.0) + offset * 8.0) % 360.0;
                let (nr, ng, nb) = hsl_to_rgb(h, 1.0, 0.5);
                r = nr; g = ng; b = nb;
            } else if anim_mode == "wave_rainbow" {
                let offset = k.x / 10.0;
                let h = ((anim_time * anim_speed * 4.0) - offset * 8.0) % 360.0;
                let (nr, ng, nb) = hsl_to_rgb(h, 1.0, 0.5);
                r = nr; g = ng; b = nb;
            }

            let active_sel = *sel_mode_draw.borrow();
            let is_sel = sel.contains(&k.name);
            let is_hov = if active_sel == SelectionMode::Zone {
                if let Some(h_name) = hov.as_ref() {
                    get_zone_for_key(h_name) == get_zone_for_key(&k.name)
                } else {
                    false
                }
            } else {
                hov.as_ref() == Some(&k.name)
            };
            
            cr.set_line_width(1.0);
            cr.set_source_rgb(r, g, b);
            draw_rounded_rect(cr, k.x, k.y, k.w, k.h, 4.0);
            let _ = cr.fill_preserve();
            
            if is_sel {
                cr.set_source_rgb(1.0, 1.0, 1.0);
                cr.set_line_width(2.0);
                let _ = cr.stroke();
            } else if is_hov {
                cr.set_source_rgb(0.7, 0.7, 0.75);
                cr.set_line_width(1.5);
                let _ = cr.stroke();
            } else {
                cr.set_source_rgba(0.3, 0.3, 0.35, 1.0);
                let _ = cr.stroke();
            }
            
            let lum = r * 0.299 + g * 0.587 + b * 0.114;
            if lum > 0.5 { cr.set_source_rgb(0.0, 0.0, 0.0); } else { cr.set_source_rgb(0.9, 0.9, 0.9); }
            
            cr.set_font_size(11.0);
            if let Ok(extents) = cr.text_extents(&k.display) {
                cr.move_to(
                    k.x + (k.w - extents.width()) / 2.0 - extents.x_bearing(),
                    k.y + (k.h - extents.height()) / 2.0 - extents.y_bearing()
                );
                let _ = cr.show_text(&k.display);
            }
        }
    });

    let motion = gtk::EventControllerMotion::new();
    let keys_hov = keys_rc.clone();
    let hov_state = hovered_key.clone();
    let da_hov = drawing_area.clone();
    motion.connect_motion(move |_, x, y| {
        let lx = x - 10.0;
        let ly = y - 10.0;
        let mut hit = None;
        for k in keys_hov.iter() {
            if lx >= k.x && lx <= k.x + k.w && ly >= k.y && ly <= k.y + k.h {
                hit = Some(k.name.clone());
                break;
            }
        }
        if *hov_state.borrow() != hit {
            *hov_state.borrow_mut() = hit;
            da_hov.queue_draw();
        }
    });
    let hov_state_l = hovered_key.clone();
    let da_hov_l = drawing_area.clone();
    motion.connect_leave(move |_| {
        *hov_state_l.borrow_mut() = None;
        da_hov_l.queue_draw();
    });
    drawing_area.add_controller(motion);

    let paint_color = Rc::new(RefCell::new("#0099ED".to_string()));
    
    let click = gtk::GestureClick::new();
    click.set_button(0);
    let keys_click = keys_rc.clone();
    let da_click = drawing_area.clone();
    let sm_click = sel_mode.clone();
    
    let p_color_c = paint_color.clone();
    let kc_c = key_colors.clone();
    let pk_c = per_key_colors.clone();
    let zc_c = zone_colors.clone();
    
    click.connect_pressed(move |_g, _n_press, x, y| {
        let mode = *sm_click.borrow();
        let hex = p_color_c.borrow().clone();
        
        let mut hit_names = Vec::new();
        
        if mode == SelectionMode::All {
            for k in keys_click.iter() { hit_names.push(k.name.clone()); }
        } else {
            let lx = x - 10.0;
            let ly = y - 10.0;
            let mut hit = None;
            for k in keys_click.iter() {
                if lx >= k.x && lx <= k.x + k.w && ly >= k.y && ly <= k.y + k.h {
                    hit = Some(k.clone());
                    break;
                }
            }
            if let Some(k) = hit {
                match mode {
                    SelectionMode::Key => hit_names.push(k.name.clone()),
                    SelectionMode::Row => {
                        let y_target = k.y;
                        for other in keys_click.iter() {
                            if (other.y - y_target).abs() < 5.0 { hit_names.push(other.name.clone()); }
                        }
                    }
                    SelectionMode::Zone => {
                        let z_target = get_zone_for_key(&k.name);
                        for other in keys_click.iter() {
                            if get_zone_for_key(&other.name) == z_target { hit_names.push(other.name.clone()); }
                        }
                    }
                    _ => {}
                }
            }
        }
        
        if !hit_names.is_empty() {
            let mut kc_mut = kc_c.borrow_mut();
            let mut pk_mut = pk_c.borrow_mut();
            let mut zc_mut = zc_c.borrow_mut();
            let mut zones_changed = Vec::new();
            
            for n in &hit_names {
                kc_mut.insert(n.clone(), hex.clone());
                if detected_mode == KeyboardMode::Omen4Zone {
                    let z = get_zone_for_key(n);
                    zc_mut[(z-1) as usize] = hex.clone();
                    if !zones_changed.contains(&z) { zones_changed.push(z); }
                } else if detected_mode == KeyboardMode::PerKey {
                    if let Some(k) = keys_click.iter().find(|x| &x.name == n) {
                        if k.id < pk_mut.len() { pk_mut[k.id] = hex.clone(); }
                    }
                } else if detected_mode == KeyboardMode::Victus1Zone {
                    zc_mut[0] = hex.clone();
                    if !zones_changed.contains(&1) { zones_changed.push(1); }
                }
            }
            
            if detected_mode == KeyboardMode::PerKey {
                crate::daemon_client::set_per_key_colors_sync(pk_mut.clone());
            } else if detected_mode == KeyboardMode::Omen4Zone {
                for z in zones_changed { crate::daemon_client::set_color_sync(z-1, hex.clone()); }
            } else if detected_mode == KeyboardMode::Victus1Zone {
                if !zones_changed.is_empty() { crate::daemon_client::set_color_sync(8, hex.clone()); }
            }
            da_click.queue_draw();
        }
    });
    drawing_area.add_controller(click);

    let drag = gtk::GestureDrag::new();
    let keys_drag = keys_rc.clone();
    let da_drag = drawing_area.clone();
    let drag_start = Rc::new(RefCell::new((0.0, 0.0)));
    let sm_drag = sel_mode.clone();

    let ds_start = drag_start.clone();
    drag.connect_drag_begin(move |_g, x, y| {
        *ds_start.borrow_mut() = (x - 10.0, y - 10.0);
    });

    // We store painted keys during the drag to avoid redundant daemon calls
    let painted_during_drag = Rc::new(RefCell::new(Vec::new()));
    
    let ds_update = drag_start.clone();
    let keys_drag_u = keys_drag.clone();
    let da_drag_u = da_drag.clone();
    let sm_d2 = sm_drag.clone();
    let p_color_u = paint_color.clone();
    let kc_u = key_colors.clone();
    let painted_u = painted_during_drag.clone();
    
    drag.connect_drag_update(move |_, dx, dy| {
        if *sm_d2.borrow() == SelectionMode::All { return; }
        let (sx, sy) = *ds_update.borrow();
        let ex = sx + dx;
        let ey = sy + dy;
        let rx = sx.min(ex);
        let ry = sy.min(ey);
        let rw = (sx - ex).abs();
        let rh = (sy - ey).abs();

        let mode = *sm_d2.borrow();
        let hex = p_color_u.borrow().clone();
        let mut hit_names = Vec::new();

        for k in keys_drag_u.iter() {
            let overlaps = !(k.x + k.w < rx || k.x > rx + rw || k.y + k.h < ry || k.y > ry + rh);
            if overlaps { hit_names.push(k.name.clone()); }
        }
        
        let mut final_hits = Vec::new();
        match mode {
            SelectionMode::Key => { final_hits = hit_names; }
            SelectionMode::Row => {
                let mut hit_y = Vec::new();
                for n in &hit_names {
                    if let Some(k) = keys_drag_u.iter().find(|x| &x.name == n) {
                        if !hit_y.contains(&k.y) { hit_y.push(k.y); }
                    }
                }
                for k in keys_drag_u.iter() {
                    if hit_y.iter().any(|y| (k.y - y).abs() < 5.0) { final_hits.push(k.name.clone()); }
                }
            }
            SelectionMode::Zone => {
                let mut hit_z = Vec::new();
                for n in &hit_names {
                    let z = get_zone_for_key(n);
                    if !hit_z.contains(&z) { hit_z.push(z); }
                }
                for k in keys_drag_u.iter() {
                    if hit_z.contains(&get_zone_for_key(&k.name)) { final_hits.push(k.name.clone()); }
                }
            }
            _ => {}
        }
        
        let mut changed = false;
        let mut kc_mut = kc_u.borrow_mut();
        let mut painted = painted_u.borrow_mut();
        
        for n in final_hits {
            if !painted.contains(&n) {
                kc_mut.insert(n.clone(), hex.clone());
                painted.push(n);
                changed = true;
            }
        }
        
        if changed { da_drag_u.queue_draw(); }
    });
    
    let pk_d = per_key_colors.clone();
    let zc_d = zone_colors.clone();
    let keys_d = keys_rc.clone();
    let painted_d = painted_during_drag.clone();
    let hex_d = paint_color.clone();
    
    drag.connect_drag_end(move |_, _, _| {
        let painted = painted_d.borrow().clone();
        if painted.is_empty() { return; }
        
        let hex = hex_d.borrow().clone();
        let mut pk_mut = pk_d.borrow_mut();
        let mut zc_mut = zc_d.borrow_mut();
        let mut zones_changed = Vec::new();
        
        for n in &painted {
            if detected_mode == KeyboardMode::Omen4Zone {
                let z = get_zone_for_key(n);
                zc_mut[(z-1) as usize] = hex.clone();
                if !zones_changed.contains(&z) { zones_changed.push(z); }
            } else if detected_mode == KeyboardMode::PerKey {
                if let Some(k) = keys_d.iter().find(|x| &x.name == n) {
                    if k.id < pk_mut.len() { pk_mut[k.id] = hex.clone(); }
                }
            } else if detected_mode == KeyboardMode::Victus1Zone {
                zc_mut[0] = hex.clone();
                if !zones_changed.contains(&1) { zones_changed.push(1); }
            }
        }
        
        if detected_mode == KeyboardMode::PerKey {
            crate::daemon_client::set_per_key_colors_sync(pk_mut.clone());
        } else if detected_mode == KeyboardMode::Omen4Zone {
            for z in zones_changed { crate::daemon_client::set_color_sync(z-1, hex.clone()); }
        } else if detected_mode == KeyboardMode::Victus1Zone {
            if !zones_changed.is_empty() { crate::daemon_client::set_color_sync(8, hex.clone()); }
        }
        
        painted_d.borrow_mut().clear();
    });

    drawing_area.add_controller(drag);

    kb_card.append(&drawing_area);

    // 4. Color Pickers and Controls at the Bottom
    let bottom_box = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(12).margin_top(12).halign(gtk::Align::Center).build();

    let instructions = gtk::Label::builder()
        .label("İlk renk seçiminizi yapıp boyamak istediğiniz tuşlara/bölgelere tıklayın.")
        .css_classes(["dim-label"])
        .build();
    bottom_box.append(&instructions);
    
    let controls_row = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(16).halign(gtk::Align::Center).build();

    let color_container = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(6).valign(gtk::Align::Center).build();
    color_container.append(&gtk::Label::builder().label("Color:").css_classes(["dim-label"]).build());
    
    let global_color_btn = gtk::Button::builder().width_request(32).height_request(32).build();
    global_color_btn.add_css_class("circular");
    global_color_btn.set_widget_name("global_color_btn");
    
    let dyn_prov_g = gtk::CssProvider::new();
    dyn_prov_g.load_from_string(&format!("#global_color_btn {{ background: {}; background-image: none; border: 1px solid rgba(255,255,255,0.4); }}", "#0099ED"));
    global_color_btn.style_context().add_provider(&dyn_prov_g, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    
    let dyn_prov_clone = dyn_prov_g.clone();
    let paint_color_btn = paint_color.clone();
    global_color_btn.connect_clicked(move |btn| {
        let dyn_local = dyn_prov_clone.clone();
        let pc_local = paint_color_btn.clone();
        show_color_picker_popover(btn, Rc::new(move |hex| {
            *pc_local.borrow_mut() = hex.clone();
            dyn_local.load_from_string(&format!("#global_color_btn {{ background: {}; background-image: none; border: 1px solid rgba(255,255,255,0.4); }}", hex));
        }));
    });
    color_container.append(&global_color_btn);
    controls_row.append(&color_container);

    controls_row.append(&gtk::Separator::new(gtk::Orientation::Vertical));

    let label_sg = gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal);

    let speed_box = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(6).build();
    let speed_label = gtk::Label::builder().label(crate::i18n::t("kb_effect_speed")).xalign(0.0).build();
    label_sg.add_widget(&speed_label);
    speed_box.append(&speed_label);
    let speed_scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    speed_scale.set_value(*speed_rc.borrow());
    speed_scale.set_size_request(150, -1);
    speed_scale.set_valign(gtk::Align::Center);
    
    let ae_hook = apply_anim_rc.clone();
    let speed_rc2 = speed_rc.clone();
    let msl_local = msl_c.clone();
    let am_idx = active_mode_idx.clone();
    speed_scale.connect_value_changed(move |sc| {
        let speed = sc.value();
        *speed_rc2.borrow_mut() = speed;
        let idx = *am_idx.borrow();
        if idx < msl_local.len() {
            let m = &msl_local[idx];
            if m == "wave_ltr" {
                crate::daemon_client::set_mode_sync("wave", speed as i32);
            } else if m == "wave_rtl" {
                crate::daemon_client::set_mode_sync("wave", speed as i32);
            } else {
                crate::daemon_client::set_mode_sync(m, speed as i32);
            }
            if let Some(anim_func) = &*ae_hook.borrow() {
                let anim_m = if m.starts_with("wave_") { "wave" } else { m.as_str() };
                anim_func(anim_m, speed);
            }
        }
    });
    speed_box.append(&speed_scale);
    
    let bright_box = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(6).build();
    let bright_label = gtk::Label::builder().label(crate::i18n::t("kb_brightness")).xalign(0.0).build();
    label_sg.add_widget(&bright_label);
    bright_box.append(&bright_label);
    let bright_scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    let mut bval = 100.0;
    if let Some(state_json) = state_json_opt {
        if let Some(b) = state_json["brightness"].as_f64() { bval = b; }
    }
    bright_scale.set_value(bval);
    bright_scale.set_size_request(150, -1);
    bright_scale.set_valign(gtk::Align::Center);
    bright_scale.connect_value_changed(move |sc| {
        let val = sc.value() as i32;
        crate::daemon_client::set_global_sync(val > 0, val, "ltr");
    });
    bright_box.append(&bright_scale);
    
    let sliders_container = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(6).build();
    sliders_container.append(&speed_box);
    sliders_container.append(&bright_box);
    controls_row.append(&sliders_container);
    
    bottom_box.append(&controls_row);

    kb_card.append(&bottom_box);

    let tick_id: Rc<RefCell<Option<gtk::TickCallbackId>>> = Rc::new(RefCell::new(None));
    let da_anim = drawing_area.clone();
    let anim_state_apply = anim_state.clone();
    
    let apply_anim: Rc<dyn Fn(&str, f64)> = Rc::new(move |mode: &str, speed: f64| {
        let mut ts = anim_state_apply.borrow_mut();
        ts.0 = mode.to_string();
        ts.1 = speed;
        drop(ts);
        
        let needs_anim = mode != "static" && mode != "per_key_custom";
        
        let mut curr_id = tick_id.borrow_mut();
        if !needs_anim && curr_id.is_some() {
            curr_id.take().unwrap().remove();
        } else if needs_anim && curr_id.is_none() {
            let anim_state_tick = anim_state_apply.clone();
            let da_tick = da_anim.clone();
            let id = da_anim.add_tick_callback(move |_, clock| {
                let frame_time = clock.frame_time() as f64 / 1_000_000.0;
                let mut ts = anim_state_tick.borrow_mut();
                ts.2 = frame_time;
                drop(ts);
                da_tick.queue_draw();
                glib::ControlFlow::Continue
            });
            *curr_id = Some(id);
        }
        da_anim.queue_draw();
    });
    *apply_anim_rc.borrow_mut() = Some(apply_anim.clone());

    (kb_card, apply_anim, gtk::Box::new(gtk::Orientation::Horizontal, 0))
}


// ── OmenCore 4-Segment Lightbar Widget Builder ───────────────
fn build_interactive_lightbar(state_json_opt: &Option<serde_json::Value>) -> gtk::Box {
    let bar_card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["os-card"])
        .spacing(12)
        .build();

    let title_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    title_row.append(&gtk::Label::builder()
        .label(i18n::t("lightbar_title"))
        .css_classes(["chip-title"])
        .hexpand(true)
        .halign(gtk::Align::Start)
        .build());

    let sync_btn = gtk::Button::builder()
        .label(i18n::t("lightbar_sync_btn"))
        .css_classes(["ec-btn"])
        .build();
    title_row.append(&sync_btn);
    bar_card.append(&title_row);

    // 4 Segment Light Bar Strip
    let strip_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .homogeneous(true)
        .margin_top(4)
        .margin_bottom(4)
        .build();

    let segment_names = [i18n::t("lb_seg_1"), i18n::t("lb_seg_2"), i18n::t("lb_seg_3"), i18n::t("lb_seg_4")];
    let default_colors = ["#E03454", "#0099EE", "#0099EE", "#E03454"]; // OMEN gradient
    let mut segment_buttons = Vec::new();

    let dyn_provider = Rc::new(gtk::CssProvider::new());
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(&display, &*dyn_provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }
    let mut initial_colors = default_colors.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    if let Some(state) = state_json_opt {
        if let Some(zones) = state.get("zones").and_then(|z| z.as_object()) {
            for i in 0..4 {
                let key = format!("{}", i + 4); // Lightbar zones are 4, 5, 6, 7
                if let Some(hex) = zones.get(&key).and_then(|h| h.as_str()) {
                    initial_colors[i] = hex.to_string();
                }
            }
        }
    }
    let colors_map = Rc::new(RefCell::new(initial_colors));

    for (i, name) in segment_names.iter().enumerate() {
        let seg_btn = gtk::Button::builder()
            .label(*name)
            .height_request(46)
            .build();
        
        seg_btn.set_widget_name(&format!("lightbar_seg_{}", i));
        
        let dyn_prov_inner = dyn_provider.clone();
        let colors_inner = colors_map.clone();
        
        seg_btn.connect_clicked(move |btn_ref| {
            let colors_local = colors_inner.clone();
            let dyn_local = dyn_prov_inner.clone();
            
            show_color_picker_popover(btn_ref, Rc::new(move |hex| {
                colors_local.borrow_mut()[i] = hex.clone();
                
                let mut css_str = String::new();
                for (idx, c) in colors_local.borrow().iter().enumerate() {
                    css_str.push_str(&format!(
                        "#lightbar_seg_{} {{ background: {}; background-image: none; color: #fff; border-radius: 6px; font-size: 11px; font-weight: bold; border: 1px solid rgba(255,255,255,0.25); box-shadow: 0 0 14px {}60; transition: all 0.25s ease; }}\n",
                        idx, c, c
                    ));
                }
                dyn_local.load_from_string(&css_str);
                
                // Assuming lightbar uses zones 4, 5, 6, 7
                let zone_id = 4 + i as i32;
                crate::daemon_client::set_color_sync(zone_id, hex.clone());
            }));
        });

        strip_box.append(&seg_btn);
        segment_buttons.push(seg_btn);
    }
    bar_card.append(&strip_box);

    let dyn_prov_inner_sync = dyn_provider.clone();
    let colors_inner_sync = colors_map.clone();
    
    sync_btn.connect_clicked(move |btn_ref| {
        let colors_local = colors_inner_sync.clone();
        let dyn_local = dyn_prov_inner_sync.clone();
        
        show_color_picker_popover(btn_ref, Rc::new(move |hex| {
            let mut css_str = String::new();
            for i in 0..4 {
                colors_local.borrow_mut()[i] = hex.clone();
                css_str.push_str(&format!(
                    "#lightbar_seg_{} {{ background: {}; background-image: none; color: #fff; border-radius: 6px; font-size: 11px; font-weight: bold; border: 1px solid rgba(255,255,255,0.25); box-shadow: 0 0 14px {}60; transition: all 0.25s ease; }}\n",
                    i, hex, hex
                ));
                crate::daemon_client::set_color_sync((4 + i) as i32, hex.clone());
            }
            dyn_local.load_from_string(&css_str);
        }));
    });

    // Initialize initial colors
    let mut init_css = String::new();
    for (idx, c) in colors_map.borrow().iter().enumerate() {
        init_css.push_str(&format!(
            "#lightbar_seg_{} {{ background: {}; background-image: none; color: #fff; border-radius: 6px; font-size: 11px; font-weight: bold; border: 1px solid rgba(255,255,255,0.15); box-shadow: 0 0 10px {}40; transition: all 0.25s ease; }}\n",
            idx, c, c
        ));
    }
    dyn_provider.load_from_string(&init_css);

    bar_card
}

use crate::i18n;


pub fn build_page() -> (adw::PreferencesPage, Option<adw::PreferencesGroup>, Option<adw::PreferencesGroup>) {
    let page = adw::PreferencesPage::builder().build();
    let specs = crate::daemon_client::get_hardware_specs_sync();

    let prod_lower = specs.product_name.to_lowercase();
    let is_omen = prod_lower.contains("omen");

    let current_state_str = crate::daemon_client::get_rgb_state_sync();
    let mut is_per_key = false;
    let mut state_json_opt: Option<serde_json::Value> = None;
    if let Ok(state_json) = serde_json::from_str::<serde_json::Value>(&current_state_str) {
        if state_json["per_key_available"].as_bool().unwrap_or(false) {
            is_per_key = true;
        }
        state_json_opt = Some(state_json);
    }

    let mut zone_override = 0;
    if let Ok(home) = std::env::var("HOME") {
        let path = format!("{}/.config/omenspace/settings.json", home);
        if let Ok(json_str) = std::fs::read_to_string(&path) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&json_str) {
                if let Some(zo) = json.get("zone_override").and_then(|v| v.as_u64()) {
                    zone_override = zo;
                }
            }
        }
    }

    let detected_mode = match zone_override {
        4 => KeyboardMode::DesktopRgb,
        3 => KeyboardMode::PerKey,
        2 => KeyboardMode::Victus1Zone,
        1 => KeyboardMode::Omen4Zone,
        _ => {
            if specs.product_name.to_lowercase().contains("desktop") || specs.product_name.to_lowercase().contains("tower") {
                KeyboardMode::DesktopRgb
            } else if is_per_key {
                KeyboardMode::PerKey
            } else if is_omen {
                KeyboardMode::Omen4Zone
            } else {
                KeyboardMode::Victus1Zone
            }
        }
    };

    let std_kb_group = adw::PreferencesGroup::builder().build();

    let zone_colors = Rc::new(RefCell::new(vec!["#0099ED".to_string(); 7]));
    let per_key_colors = Rc::new(RefCell::new(vec!["#0099ED".to_string(); 104]));

    let (std_kb_grid, apply_anim, _) = if detected_mode == KeyboardMode::DesktopRgb {
        crate::desktop_rgb_gui::build_desktop_rgb_card(zone_colors.clone())
    } else {
        build_interactive_keyboard(detected_mode, zone_colors.clone(), per_key_colors.clone(), &state_json_opt)
    };
    std_kb_group.add(&std_kb_grid);
    page.add(&std_kb_group);

    if let Some(state_json) = &state_json_opt {
        if let Some(mode_str) = state_json["mode"].as_str() {
            let speed = state_json["speed"].as_f64().unwrap_or(50.0);
            apply_anim(mode_str, speed);
        } else {
            apply_anim("static", 50.0);
        }
    } else {
        apply_anim("static", 50.0);
    }

    let zc_load = zone_colors.clone();
    glib::spawn_future_local(async move {
        if let Ok(json) = crate::daemon_client::get_rgb_state_async().await {
            if let Ok(state) = serde_json::from_str::<serde_json::Value>(&json) {
                if let Some(zones) = state.get("zones").and_then(|z| z.as_object()) {
                    let mut loaded_colors = zc_load.borrow_mut();
                    for (i, v) in zones.iter() {
                        if let Ok(idx) = i.parse::<usize>() {
                            if let Some(hex) = v.as_str() {
                                if idx < loaded_colors.len() {
                                    loaded_colors[idx] = hex.to_string();
                                }
                            }
                        }
                    }
                }
            }
        }
    });

    let mut lb_group_ret = None;
    let mut lb_preview_group_ret = None;

    let is_omen_brand = is_omen || detected_mode == KeyboardMode::DesktopRgb;
    if is_omen_brand {
        let lb_group = adw::PreferencesGroup::builder()
            .title(crate::i18n::t("lightbar_group"))
            .description(crate::i18n::t("lightbar_desc"))
            .build();

        let lb_enable_row = adw::SwitchRow::builder()
            .title(crate::i18n::t("lightbar_enable"))
            .subtitle(crate::i18n::t("lightbar_enable_sub"))
            .build();
        lb_enable_row.set_active(true);
        lb_group.add(&lb_enable_row);

        let lb_effect_model = gtk::StringList::new(&[
            crate::i18n::t("effect_static"),
            crate::i18n::t("effect_wave"),
            crate::i18n::t("effect_breathing"),
            crate::i18n::t("effect_cycle"),
        ]);
        let lb_speed_rc = Rc::new(RefCell::new(50.0));
        let lb_speed_rc_c1 = lb_speed_rc.clone();
        
        let lb_effect_row = adw::ComboRow::builder()
            .title(crate::i18n::t("lightbar_effect"))
            .model(&lb_effect_model)
            .build();
            
        let lb_effect_row_rc = Rc::new(lb_effect_row.clone());
        let lber_c = lb_effect_row_rc.clone();
        let lbs_c2 = lb_speed_rc.clone();
        
        lb_effect_row.connect_selected_notify(move |row| {
            let mode = match row.selected() {
                1 => "wave",
                2 => "breathing",
                3 => "cycle",
                _ => "static",
            };
            crate::daemon_client::set_mode_sync(mode, *lbs_c2.borrow() as i32);
        });
        lb_group.add(&lb_effect_row);

        let lb_speed_row = adw::ActionRow::builder().title(crate::i18n::t("kb_effect_speed")).build();
        let lb_speed_scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
        lb_speed_scale.set_value(50.0);
        lb_speed_scale.set_draw_value(true);
        lb_speed_scale.set_hexpand(false);
        lb_speed_scale.set_size_request(250, -1);
        lb_speed_scale.set_margin_start(12);
        lb_speed_scale.set_margin_end(12);
        lb_speed_scale.set_valign(gtk::Align::Center);
        lb_speed_row.add_suffix(&lb_speed_scale);
        lb_group.add(&lb_speed_row);
        
        lb_speed_scale.connect_value_changed(move |scale| {
            let speed = scale.value();
            *lb_speed_rc_c1.borrow_mut() = speed;
            let row = &*lber_c;
            let mode = match row.selected() {
                1 => "wave",
                2 => "breathing",
                3 => "cycle",
                _ => "static",
            };
            crate::daemon_client::set_mode_sync(mode, speed as i32);
        });

        let lb_bright_row = adw::ActionRow::builder().title(crate::i18n::t("lightbar_brightness")).build();
        let lb_bright_scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
        lb_bright_scale.set_value(80.0);
        lb_bright_scale.set_draw_value(true);
        lb_bright_scale.set_hexpand(false);
        lb_bright_scale.set_size_request(250, -1);
        lb_bright_scale.set_margin_start(12);
        lb_bright_scale.set_margin_end(12);
        lb_bright_scale.set_valign(gtk::Align::Center);
        lb_bright_row.add_suffix(&lb_bright_scale);
        lb_group.add(&lb_bright_row);
        
        lb_bright_scale.connect_value_changed(move |scale| {
            let val = scale.value() as i32;
            crate::daemon_client::set_global_sync(val > 0, val, "ltr");
        });

        page.add(&lb_group);

        let lb_preview_group = adw::PreferencesGroup::builder()
            .title(crate::i18n::t("lightbar_segments"))
            .build();
        let lb_widget = build_interactive_lightbar(&state_json_opt);
        lb_preview_group.add(&lb_widget);
        page.add(&lb_preview_group);

        let lb_effect_row_c = lb_effect_row.clone();
        let lb_bright_row_c = lb_bright_row.clone();
        let lb_speed_row_c = lb_speed_row.clone();
        let lb_preview_group_c = lb_preview_group.clone();

        lb_enable_row.connect_active_notify(move |row| {
            let is_active = row.is_active();
            lb_effect_row_c.set_visible(is_active);
            lb_speed_row_c.set_visible(is_active);
            lb_bright_row_c.set_visible(is_active);
            lb_preview_group_c.set_visible(is_active);
        });
        let lb_prod_lower = specs.product_name.to_lowercase();
        let mut show_lightbar = detected_mode == KeyboardMode::DesktopRgb || lb_prod_lower.contains("desktop") || lb_prod_lower.contains("transcend") || lb_prod_lower.contains("max");
        if let Ok(home) = std::env::var("HOME") {
            let path = format!("{}/.config/omenspace/settings.json", home);
            if let Ok(json_str) = std::fs::read_to_string(&path) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&json_str) {
                    if let Some(lb) = json.get("lightbar_enabled").and_then(|v| v.as_bool()) {
                        show_lightbar = lb;
                    }
                }
            }
        }
        lb_group.set_visible(show_lightbar);
        lb_preview_group.set_visible(show_lightbar);

        lb_group_ret = Some(lb_group);
        lb_preview_group_ret = Some(lb_preview_group);
    }

    (page, lb_group_ret, lb_preview_group_ret)
}

