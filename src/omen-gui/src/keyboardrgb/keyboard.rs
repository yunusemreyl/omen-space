use crate::keyboardrgb::helpers::*;
use gtk::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

fn load_keyboard_layout() -> KeyboardLayout {
    let Ok(home) = std::env::var("HOME") else {
        return KeyboardLayout::En;
    };
    let path = format!("{}/.config/omenspace/settings.json", home);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| {
            v.get("keyboard_layout")
                .and_then(|x| x.as_str())
                .map(KeyboardLayout::from_saved)
        })
        .unwrap_or(KeyboardLayout::En)
}

fn save_keyboard_layout(layout: KeyboardLayout) {
    let Ok(home) = std::env::var("HOME") else {
        return;
    };
    let path = format!("{}/.config/omenspace/settings.json", home);
    if let Some(dir) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    value["keyboard_layout"] = serde_json::json!(layout.code());
    let _ = std::fs::write(
        &path,
        serde_json::to_string_pretty(&value).unwrap_or_default(),
    );
}

pub type AnimCallback = Rc<dyn Fn(&str, f64)>;

pub fn build_interactive_keyboard(
    detected_mode: KeyboardMode,
    zone_colors: Rc<RefCell<Vec<String>>>,
    per_key_colors: Rc<RefCell<Vec<String>>>,
    state_json_opt: &Option<serde_json::Value>,
) -> (gtk::Box, AnimCallback, gtk::Box) {
    // Restore saved 104-key frame from daemon so undrawn placeholder slots (98..103)
    // do not stay #0099ED blue on restart (#355).
    if let Some(state) = state_json_opt {
        if let Some(arr) = state.get("per_key_colors").and_then(|v| v.as_array()) {
            let mut pk = per_key_colors.borrow_mut();
            for (i, v) in arr.iter().enumerate().take(pk.len()) {
                if let Some(hex) = v.as_str() {
                    pk[i] = if hex.starts_with('#') {
                        hex.to_string()
                    } else {
                        format!("#{}", hex)
                    };
                }
            }
        }
    }

    let kb_card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["os-card"])
        .spacing(12)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .halign(gtk::Align::Center)
        .build();

    // 1. Top Effects Tabs
    let (effect_labels, mode_str_list) = match detected_mode {
        KeyboardMode::Victus1Zone => (
            vec![
                crate::i18n::t("effect_static"),
                crate::i18n::t("effect_breathing"),
                crate::i18n::t("effect_cycle"),
            ],
            vec!["static", "breathing", "cycle"],
        ),
        KeyboardMode::Omen4Zone => (
            vec![
                crate::i18n::t("effect_static"),
                crate::i18n::t("effect_breathing"),
                crate::i18n::t("effect_blinking"),
                crate::i18n::t("effect_cycle"),
                crate::i18n::t("effect_wave_custom"),
                crate::i18n::t("effect_wave_rainbow"),
            ],
            vec![
                "static",
                "breathing",
                "blinking",
                "cycle",
                "wave",
                "wave_rainbow",
            ],
        ),
        KeyboardMode::PerKey => (
            vec![
                crate::i18n::t("effect_static"),
                crate::i18n::t("kb_per_key_custom"),
                crate::i18n::t("effect_breathing"),
                crate::i18n::t("effect_blinking"),
                crate::i18n::t("effect_cycle"),
                crate::i18n::t("effect_wave_custom"),
                crate::i18n::t("effect_wave_rainbow"),
                crate::i18n::t("effect_starlight"),
                crate::i18n::t("effect_marquee"),
                crate::i18n::t("effect_reactive"),
                crate::i18n::t("effect_ripple"),
                crate::i18n::t("effect_raindrop"),
            ],
            vec![
                "static",
                "per_key_custom",
                "breathing",
                "blinking",
                "cycle",
                "wave",
                "wave_rainbow",
                "starlight",
                "marquee",
                "reactive",
                "ripple",
                "raindrop",
            ],
        ),
        KeyboardMode::DesktopRgb => (
            vec![
                crate::i18n::t("effect_static"),
                crate::i18n::t("effect_breathing"),
                crate::i18n::t("effect_cycle"),
                crate::i18n::t("effect_blinking"),
                crate::i18n::t("effect_wave"),
            ],
            vec!["static", "breathing", "cycle", "blinking", "wave"],
        ),
    };

    let globals = Rc::new(RefCell::new(KeyboardGlobals::from_snapshot(
        state_json_opt.as_ref(),
    )));
    // Key-label language. Saved in settings.json; display only.
    let current_layout = Rc::new(std::cell::Cell::new(load_keyboard_layout()));
    let layout_labels: Rc<RefCell<HashMap<String, String>>> =
        Rc::new(RefCell::new(current_layout.get().keycap_labels()));

    let top_bar = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();

    // -- Selection Tools (Left-aligned) --
    let sel_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();
    let sel_label = gtk::Label::builder()
        .label(crate::i18n::t("kb_select_label"))
        .css_classes(["dim-label"])
        .build();
    sel_box.append(&sel_label);

    let sel_btns_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .build();
    sel_btns_box.add_css_class("linked");

    let initial_power = globals.borrow().power;
    let initial_sel = if !initial_power {
        SelectionMode::Off
    } else if detected_mode == KeyboardMode::PerKey {
        SelectionMode::Key
    } else {
        SelectionMode::Zone
    };

    let btn_off = gtk::ToggleButton::builder()
        .label(crate::i18n::t("kb_sel_off"))
        .active(initial_sel == SelectionMode::Off)
        .build();
    let btn_key = gtk::ToggleButton::builder()
        .label(crate::i18n::t("kb_sel_key"))
        .active(initial_sel == SelectionMode::Key)
        .sensitive(detected_mode == KeyboardMode::PerKey)
        .build();
    let btn_zone = gtk::ToggleButton::builder()
        .label(crate::i18n::t("kb_sel_zone"))
        .active(initial_sel == SelectionMode::Zone)
        .build();
    let btn_all = gtk::ToggleButton::builder()
        .label(crate::i18n::t("kb_sel_all"))
        .build();

    // Set initial sel_mode based on supported mode and saved power state
    let sel_mode = Rc::new(RefCell::new(initial_sel));

    btn_key.set_group(Some(&btn_off));
    btn_zone.set_group(Some(&btn_off));
    btn_all.set_group(Some(&btn_off));

    let sm_o = sel_mode.clone();
    let globals_sm_o = globals.clone();
    btn_off.connect_toggled(move |b| {
        if b.is_active() {
            *sm_o.borrow_mut() = SelectionMode::Off;
            let mut state = globals_sm_o.borrow_mut();
            state.power = false;
            state.send();
        }
    });
    let sm_k = sel_mode.clone();
    let globals_sm_k = globals.clone();
    btn_key.connect_toggled(move |b| {
        if b.is_active() {
            *sm_k.borrow_mut() = SelectionMode::Key;
            let mut state = globals_sm_k.borrow_mut();
            state.power = true;
            state.send();
        }
    });
    let sm_z = sel_mode.clone();
    let globals_sm_z = globals.clone();
    btn_zone.connect_toggled(move |b| {
        if b.is_active() {
            *sm_z.borrow_mut() = SelectionMode::Zone;
            let mut state = globals_sm_z.borrow_mut();
            state.power = true;
            state.send();
        }
    });
    let sm_a = sel_mode.clone();
    let globals_sm_a = globals.clone();
    btn_all.connect_toggled(move |b| {
        if b.is_active() {
            *sm_a.borrow_mut() = SelectionMode::All;
            let mut state = globals_sm_a.borrow_mut();
            state.power = true;
            state.send();
        }
    });

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

    let apply_anim_rc: Rc<RefCell<Option<AnimCallback>>> = Rc::new(RefCell::new(None));
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

    top_bar.append(
        &gtk::Label::builder()
            .label(crate::i18n::t("kb_effect_label"))
            .css_classes(["dim-label"])
            .build(),
    );
    top_bar.append(&effects_dropdown);

    kb_card.append(&top_bar);

    // 3. Canvas
    let layout_def = vec![
        vec![
            ("Esc", 1.0),
            ("", 0.5),
            ("F1", 1.0),
            ("F2", 1.0),
            ("F3", 1.0),
            ("F4", 1.0),
            ("", 0.5),
            ("F5", 1.0),
            ("F6", 1.0),
            ("F7", 1.0),
            ("F8", 1.0),
            ("", 0.5),
            ("F9", 1.0),
            ("F10", 1.0),
            ("F11", 1.0),
            ("F12", 1.0),
            ("", 0.5),
            ("Del", 1.0),
            ("Omen", 1.0),
            ("Calc", 1.0),
            ("Power", 1.0),
        ],
        vec![
            ("~", 1.0),
            ("1", 1.0),
            ("2", 1.0),
            ("3", 1.0),
            ("4", 1.0),
            ("5", 1.0),
            ("6", 1.0),
            ("7", 1.0),
            ("8", 1.0),
            ("9", 1.0),
            ("0", 1.0),
            ("-", 1.0),
            ("=", 1.0),
            ("Backspace", 2.0),
            ("Num", 1.0),
            ("/_num", 1.0),
            ("*_num", 1.0),
            ("-_num", 1.0),
        ],
        vec![
            ("Tab", 1.5),
            ("Q", 1.0),
            ("W", 1.0),
            ("E", 1.0),
            ("R", 1.0),
            ("T", 1.0),
            ("Y", 1.0),
            ("U", 1.0),
            ("I", 1.0),
            ("O", 1.0),
            ("P", 1.0),
            ("[", 1.0),
            ("]", 1.0),
            ("\\", 1.5),
            ("7_num", 1.0),
            ("8_num", 1.0),
            ("9_num", 1.0),
            ("+_num", 1.0),
        ],
        vec![
            ("Caps", 1.75),
            ("A", 1.0),
            ("S", 1.0),
            ("D", 1.0),
            ("F", 1.0),
            ("G", 1.0),
            ("H", 1.0),
            ("J", 1.0),
            ("K", 1.0),
            ("L", 1.0),
            (";", 1.0),
            ("'", 1.0),
            ("Enter", 2.25),
            ("4_num", 1.0),
            ("5_num", 1.0),
            ("6_num", 1.0),
            ("", 1.0),
        ],
        vec![
            ("Shift", 2.25),
            ("Z", 1.0),
            ("X", 1.0),
            ("C", 1.0),
            ("V", 1.0),
            ("B", 1.0),
            ("N", 1.0),
            ("M", 1.0),
            (",", 1.0),
            (".", 1.0),
            ("/", 1.0),
            ("Shift_R", 1.75),
            ("Up", 1.0),
            ("1_num", 1.0),
            ("2_num", 1.0),
            ("3_num", 1.0),
            ("Enter_num", 1.0),
        ],
        vec![
            ("Ctrl", 1.25),
            ("Fn", 1.25),
            ("Win", 1.25),
            ("Alt", 1.25),
            ("Space", 5.5),
            ("Alt_R", 1.25),
            ("Menu", 1.25),
            ("Left", 1.0),
            ("Down", 1.0),
            ("Right", 1.0),
            ("0_num", 1.0),
            ("._num", 1.0),
            ("", 1.0),
        ],
    ];
    let unit_size = 38.0;
    let margin = 3.5;
    let height = 34.0;

    let mut keys = Vec::new();
    let mut global_idx = 0;
    let mut current_y = 0.0;
    let key_colors = Rc::new(RefCell::new(HashMap::<String, String>::new()));

    // Set by every user edit to the colouring (painting keys, All, per-key
    // picker). The preset dropdown watches it and drops back to the neutral
    // entry, because the board no longer matches any saved preset.
    let colors_dirty = Rc::new(std::cell::Cell::new(false));

    for row_keys in &layout_def {
        let mut current_x = 0.0;
        for (name, size_mult) in row_keys {
            let width = unit_size * *size_mult;
            if !name.is_empty() {
                let display_name = if *name == "Enter_num" {
                    "Ent"
                } else if let Some(stripped) = name.strip_suffix("_R") {
                    stripped
                } else if let Some(stripped) = name.strip_suffix("_num") {
                    stripped
                } else {
                    name
                };

                let def_color = if detected_mode == KeyboardMode::Omen4Zone {
                    zone_colors.borrow()[(get_zone_for_key(name) - 1) as usize].clone()
                } else if detected_mode == KeyboardMode::Victus1Zone {
                    zone_colors.borrow()[0].clone()
                } else if detected_mode == KeyboardMode::PerKey {
                    if global_idx < per_key_colors.borrow().len() {
                        per_key_colors.borrow()[global_idx].clone()
                    } else {
                        "#0099ED".to_string()
                    }
                } else {
                    "#0099ED".to_string()
                };
                key_colors.borrow_mut().insert(name.to_string(), def_color);

                let k_height = if *name == "+_num" || *name == "Enter_num" {
                    height * 2.0 + margin
                } else {
                    height
                };
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

    // Layout selector: same DropDown control as Effect, so new layouts are just
    // new model entries. Left of Effect.
    let layout_model = gtk::StringList::new(&KeyboardLayout::LABELS);
    let layout_dropdown = gtk::DropDown::builder()
        .model(&layout_model)
        .selected(current_layout.get().index())
        .valign(gtk::Align::Center)
        .tooltip_text(crate::i18n::t("kb_layout_tooltip"))
        .build();
    top_bar.prepend(&layout_dropdown);
    top_bar.prepend(
        &gtk::Label::builder()
            .label(crate::i18n::t("kb_layout_label"))
            .css_classes(["dim-label"])
            .build(),
    );

    let da_layout = drawing_area.clone();
    let layout_labels_c = layout_labels.clone();
    let layout_c = current_layout.clone();
    layout_dropdown.connect_selected_notify(move |dd| {
        let chosen = KeyboardLayout::from_index(dd.selected());
        layout_c.set(chosen);
        *layout_labels_c.borrow_mut() = chosen.keycap_labels();
        da_layout.queue_draw();
        save_keyboard_layout(chosen);
    });

    let keys_rc = Rc::new(keys);
    let selected_keys = Rc::new(RefCell::new(Vec::<String>::new()));
    let anim_state = Rc::new(RefCell::new(("static".to_string(), 50.0, 0.0f64)));
    let anim_draw = anim_state.clone();

    let hovered_key = Rc::new(RefCell::new(None::<String>));

    let keys_draw = keys_rc.clone();
    let layout_labels_draw = layout_labels.clone();
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
                r *= factor;
                g *= factor;
                b *= factor;
            } else if anim_mode == "cycle" {
                let h = (anim_time * anim_speed * 2.0) % 360.0;
                let (nr, ng, nb) = hsl_to_rgb(h, 1.0, 0.5);
                r = nr;
                g = ng;
                b = nb;
            } else if anim_mode == "wave" {
                let phase = (anim_time * anim_speed * 0.08) + (k.x / 120.0);
                let factor = (phase.sin() * 0.5) + 0.5;
                let blend = 0.08 + 0.92 * factor;
                r *= blend;
                g *= blend;
                b *= blend;
            } else if anim_mode == "wave_rainbow" {
                let offset = k.x / 10.0;
                let h = ((anim_time * anim_speed * 4.0) - offset * 8.0) % 360.0;
                let (nr, ng, nb) = hsl_to_rgb(h, 1.0, 0.5);
                r = nr;
                g = ng;
                b = nb;
            } else if anim_mode == "blinking" {
                let factor = if (anim_time * anim_speed * 0.12).sin() >= 0.0 {
                    1.0
                } else {
                    0.08
                };
                r *= factor;
                g *= factor;
                b *= factor;
            } else if anim_mode == "marquee" {
                let norm_x = (k.x / 720.0).clamp(0.0, 1.0);
                let head = (anim_time * anim_speed * 0.015).fract();
                let d = (norm_x - head).abs();
                let dist = d.min(1.0 - d);
                let factor = (1.0 - (dist / 0.25)).clamp(0.06, 1.0);
                r *= factor;
                g *= factor;
                b *= factor;
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
            if lum > 0.5 {
                cr.set_source_rgb(0.0, 0.0, 0.0);
            } else {
                cr.set_source_rgb(0.9, 0.9, 0.9);
            }

            cr.set_font_size(11.0);
            let labels_guard = layout_labels_draw.borrow();
            let label = labels_guard
                .get(&k.name)
                .map(|s| s.as_str())
                .unwrap_or(k.display.as_str());
            if let Ok(extents) = cr.text_extents(label) {
                cr.move_to(
                    k.x + (k.w - extents.width()) / 2.0 - extents.x_bearing(),
                    k.y + (k.h - extents.height()) / 2.0 - extents.y_bearing(),
                );
                let _ = cr.show_text(label);
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

    let initial_paint_color = if detected_mode == KeyboardMode::PerKey {
        per_key_colors
            .borrow()
            .first()
            .cloned()
            .unwrap_or_else(|| "#0099ED".to_string())
    } else {
        zone_colors
            .borrow()
            .first()
            .cloned()
            .unwrap_or_else(|| "#0099ED".to_string())
    };
    let paint_color = Rc::new(RefCell::new(initial_paint_color.clone()));

    let am_idx = active_mode_idx.clone();
    let msl_local = msl_c.clone();
    let dirty_effect = colors_dirty.clone();
    let aa_local = aa_hook.clone();
    let speed_ref = speed_rc.clone();
    let direction_globals = globals.clone();
    let sm_dd = sel_mode.clone();
    let btn_key_dd = btn_key.clone();
    let btn_zone_dd = btn_zone.clone();
    let kc_dd = key_colors.clone();
    let pk_dd = per_key_colors.clone();
    let keys_dd = keys_rc.clone();
    let p_color_dd = paint_color.clone();
    effects_dropdown.connect_selected_notify(move |dd| {
        let i = dd.selected() as usize;
        if i < msl_local.len() {
            *am_idx.borrow_mut() = i;
            let m = &msl_local[i];
            if *sm_dd.borrow() == SelectionMode::Off {
                match restored_selection_mode(*sm_dd.borrow(), detected_mode) {
                    SelectionMode::Key => btn_key_dd.set_active(true),
                    SelectionMode::Zone => btn_zone_dd.set_active(true),
                    _ => {}
                }
            }
            if detected_mode == KeyboardMode::PerKey && m == "static" {
                let hex = p_color_dd.borrow().clone();
                let mut kc_mut = kc_dd.borrow_mut();
                for k in keys_dd.iter() {
                    kc_mut.insert(k.name.clone(), hex.clone());
                }
            } else if detected_mode == KeyboardMode::PerKey && m == "per_key_custom" {
                let pk = pk_dd.borrow();
                let mut kc_mut = kc_dd.borrow_mut();
                for k in keys_dd.iter() {
                    if k.id < pk.len() {
                        kc_mut.insert(k.name.clone(), pk[k.id].clone());
                    }
                }
            }
            // Only "static" and the per-key layout render exactly what a
            // preset saved. Every other effect animates its own palette, so
            // the saved layout is no longer what the keyboard shows.
            if detected_mode == KeyboardMode::PerKey && m != "static" && m != "per_key_custom" {
                dirty_effect.set(true);
            }
            let speed = *speed_ref.borrow();
            if m == "wave_ltr" {
                crate::daemon_client::set_mode_sync("wave", speed as i32);
                let mut state = direction_globals.borrow_mut();
                state.direction = "ltr".into();
                state.send();
            } else if m == "wave_rtl" {
                crate::daemon_client::set_mode_sync("wave", speed as i32);
                let mut state = direction_globals.borrow_mut();
                state.direction = "rtl".into();
                state.send();
            } else {
                crate::daemon_client::set_mode_sync(m, speed as i32);
            }
            if let Some(anim_func) = &*aa_local.borrow() {
                let anim_m = if m.starts_with("wave_") {
                    "wave"
                } else {
                    m.as_str()
                };
                anim_func(anim_m, speed);
            }
        }
    });

    let click = gtk::GestureClick::new();
    click.set_button(0);
    let keys_click = keys_rc.clone();
    let da_click = drawing_area.clone();
    let sm_click = sel_mode.clone();

    let p_color_c = paint_color.clone();
    let kc_c = key_colors.clone();
    let pk_c = per_key_colors.clone();
    let dirty_paint = colors_dirty.clone();
    let zc_c = zone_colors.clone();

    let msl_click = msl_c.clone();
    let am_click = active_mode_idx.clone();

    click.connect_pressed(move |_g, _n_press, x, y| {
        let mode = *sm_click.borrow();
        let hex = p_color_c.borrow().clone();

        let mut hit_names = Vec::new();

        if mode == SelectionMode::All {
            for k in keys_click.iter() {
                hit_names.push(k.name.clone());
            }
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
                    SelectionMode::Zone => {
                        let z_target = get_zone_for_key(&k.name);
                        for other in keys_click.iter() {
                            if get_zone_for_key(&other.name) == z_target {
                                hit_names.push(other.name.clone());
                            }
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
                    zc_mut[(z - 1) as usize] = hex.clone();
                    if !zones_changed.contains(&z) {
                        zones_changed.push(z);
                    }
                } else if detected_mode == KeyboardMode::PerKey {
                    if let Some(k) = keys_click.iter().find(|x| &x.name == n) {
                        if k.id < pk_mut.len() {
                            pk_mut[k.id] = hex.clone();
                        }
                    }
                } else if detected_mode == KeyboardMode::Victus1Zone {
                    zc_mut[0] = hex.clone();
                    if !zones_changed.contains(&1) {
                        zones_changed.push(1);
                    }
                }
            }

            if detected_mode == KeyboardMode::PerKey {
                dirty_paint.set(true);
                if mode == SelectionMode::All {
                    fill_all_key_slots(&mut pk_mut, &hex);
                    zc_mut.iter_mut().take(4).for_each(|z| *z = hex.clone());
                    let cur_m = msl_click
                        .get(*am_click.borrow())
                        .map(|s| s.as_str())
                        .unwrap_or("static");
                    if cur_m == "per_key_custom" {
                        crate::daemon_client::set_per_key_colors_sync(pk_mut.clone());
                    } else {
                        crate::daemon_client::set_color_sync(8, hex.clone());
                    }
                } else {
                    crate::daemon_client::set_per_key_colors_sync(pk_mut.clone());
                }
            } else if detected_mode == KeyboardMode::Omen4Zone {
                for z in zones_changed {
                    crate::daemon_client::set_color_sync(z - 1, hex.clone());
                }
            } else if detected_mode == KeyboardMode::Victus1Zone && !zones_changed.is_empty() {
                crate::daemon_client::set_color_sync(8, hex.clone());
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
        if *sm_d2.borrow() == SelectionMode::All {
            return;
        }
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
            if overlaps {
                hit_names.push(k.name.clone());
            }
        }

        let mut final_hits = Vec::new();
        match mode {
            SelectionMode::Key => {
                final_hits = hit_names;
            }
            SelectionMode::Zone => {
                let mut hit_z = Vec::new();
                for n in &hit_names {
                    let z = get_zone_for_key(n);
                    if !hit_z.contains(&z) {
                        hit_z.push(z);
                    }
                }
                for k in keys_drag_u.iter() {
                    if hit_z.contains(&get_zone_for_key(&k.name)) {
                        final_hits.push(k.name.clone());
                    }
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

        if changed {
            da_drag_u.queue_draw();
        }
    });

    let pk_d = per_key_colors.clone();
    let dirty_drag = colors_dirty.clone();
    let zc_d = zone_colors.clone();
    let keys_d = keys_rc.clone();
    let painted_d = painted_during_drag.clone();
    let hex_d = paint_color.clone();

    drag.connect_drag_end(move |_, _, _| {
        let painted = painted_d.borrow().clone();
        if painted.is_empty() {
            return;
        }

        let hex = hex_d.borrow().clone();
        if detected_mode == KeyboardMode::PerKey {
            dirty_drag.set(true);
        }
        let mut pk_mut = pk_d.borrow_mut();
        let mut zc_mut = zc_d.borrow_mut();
        let mut zones_changed = Vec::new();

        for n in &painted {
            if detected_mode == KeyboardMode::Omen4Zone {
                let z = get_zone_for_key(n);
                zc_mut[(z - 1) as usize] = hex.clone();
                if !zones_changed.contains(&z) {
                    zones_changed.push(z);
                }
            } else if detected_mode == KeyboardMode::PerKey {
                if let Some(k) = keys_d.iter().find(|x| &x.name == n) {
                    if k.id < pk_mut.len() {
                        pk_mut[k.id] = hex.clone();
                    }
                }
            } else if detected_mode == KeyboardMode::Victus1Zone {
                zc_mut[0] = hex.clone();
                if !zones_changed.contains(&1) {
                    zones_changed.push(1);
                }
            }
        }

        if detected_mode == KeyboardMode::PerKey {
            crate::daemon_client::set_per_key_colors_sync(pk_mut.clone());
        } else if detected_mode == KeyboardMode::Omen4Zone {
            for z in zones_changed {
                crate::daemon_client::set_color_sync(z - 1, hex.clone());
            }
        } else if detected_mode == KeyboardMode::Victus1Zone && !zones_changed.is_empty() {
            crate::daemon_client::set_color_sync(8, hex.clone());
        }

        painted_d.borrow_mut().clear();
    });

    drawing_area.add_controller(drag);

    kb_card.append(&drawing_area);

    // 4. Color Pickers and Controls at the Bottom
    let bottom_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .halign(gtk::Align::Center)
        .build();

    let instructions = gtk::Label::builder()
        .label(crate::i18n::t("rgb_paint_hint"))
        .css_classes(["dim-label"])
        .build();
    bottom_box.append(&instructions);

    let controls_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(16)
        .halign(gtk::Align::Center)
        .build();

    let color_container = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .valign(gtk::Align::Center)
        .build();
    color_container.append(
        &gtk::Label::builder()
            .label(crate::i18n::t("kb_color_label"))
            .css_classes(["dim-label"])
            .build(),
    );

    let global_color_btn = gtk::Button::builder()
        .width_request(32)
        .height_request(32)
        .build();
    global_color_btn.add_css_class("circular");
    global_color_btn.set_widget_name("global_color_btn");

    let dyn_prov_g = gtk::CssProvider::new();
    dyn_prov_g.load_from_string(&format!("#global_color_btn {{ background: {}; background-image: none; border: 1px solid rgba(255,255,255,0.4); }}", initial_paint_color));
    #[allow(deprecated)]
    global_color_btn
        .style_context()
        .add_provider(&dyn_prov_g, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);

    let dyn_prov_clone = dyn_prov_g.clone();
    let paint_color_btn = paint_color.clone();
    let msl_color = msl_c.clone();
    let am_color = active_mode_idx.clone();
    let zc_color = zone_colors.clone();
    let pk_color = per_key_colors.clone();
    let dirty_color = colors_dirty.clone();
    let kc_color = key_colors.clone();
    let keys_color = keys_rc.clone();
    let da_color = drawing_area.clone();
    let sm_color = sel_mode.clone();
    let btn_key_color = btn_key.clone();
    let btn_zone_color = btn_zone.clone();
    global_color_btn.connect_clicked(move |btn| {
        let dyn_local = dyn_prov_clone.clone();
        let pc_local = paint_color_btn.clone();
        let msl_l = msl_color.clone();
        let am_l = am_color.clone();
        let zc_l = zc_color.clone();
        let pk_l = pk_color.clone();
        let dirty_l = dirty_color.clone();
        let kc_l = kc_color.clone();
        let keys_l = keys_color.clone();
        let da_l = da_color.clone();
        let sm_l = sm_color.clone();
        let btn_key_l = btn_key_color.clone();
        let btn_zone_l = btn_zone_color.clone();
        show_color_picker_popover(btn, Rc::new(move |hex| {
            if *sm_l.borrow() == SelectionMode::Off {
                match restored_selection_mode(*sm_l.borrow(), detected_mode) {
                    SelectionMode::Key => btn_key_l.set_active(true),
                    SelectionMode::Zone => btn_zone_l.set_active(true),
                    _ => {}
                }
            }
            *pc_local.borrow_mut() = hex.clone();
            dyn_local.load_from_string(&format!("#global_color_btn {{ background: {}; background-image: none; border: 1px solid rgba(255,255,255,0.4); }}", hex));
            // When in static or an animated effect mode on PerKey/1-zone (or using the global color button),
            // apply the chosen color uniformly to config.colors so Static/Marquee/Reactive/Ripple use it
            // immediately instead of keeping a stale per-zone color (#385).
            let cur_m = msl_l.get(*am_l.borrow()).map(|s| s.as_str()).unwrap_or("static");
            if cur_m != "per_key_custom" && detected_mode == KeyboardMode::PerKey {
                dirty_l.set(true);
                zc_l.borrow_mut().iter_mut().take(4).for_each(|z| *z = hex.clone());
                fill_all_key_slots(&mut pk_l.borrow_mut(), &hex);
                {
                    let mut kc_mut = kc_l.borrow_mut();
                    for k in keys_l.iter() {
                        kc_mut.insert(k.name.clone(), hex.clone());
                    }
                }
                crate::daemon_client::set_color_sync(8, hex);
                da_l.queue_draw();
            } else if detected_mode == KeyboardMode::Victus1Zone {
                zc_l.borrow_mut()[0] = hex.clone();
                {
                    let mut kc_mut = kc_l.borrow_mut();
                    for k in keys_l.iter() {
                        kc_mut.insert(k.name.clone(), hex.clone());
                    }
                }
                crate::daemon_client::set_color_sync(8, hex);
                da_l.queue_draw();
            }
        }));
    });
    color_container.append(&global_color_btn);

    let reset_rgb_btn = gtk::Button::builder()
        .label(crate::i18n::t("kb_reset_default"))
        .valign(gtk::Align::Center)
        .margin_start(6)
        .build();
    let zc_reset = zone_colors.clone();
    let pk_reset = per_key_colors.clone();
    let kc_reset = key_colors.clone();
    let keys_reset = keys_rc.clone();
    let da_reset = drawing_area.clone();
    let dirty_reset = colors_dirty.clone();
    let dd_reset = effects_dropdown.clone();
    let speed_reset = speed_rc.clone();
    reset_rgb_btn.connect_clicked(move |_| {
        dd_reset.set_selected(0);
        let default_zones = if detected_mode == KeyboardMode::Victus1Zone {
            ["#FFFFFF", "#FFFFFF", "#FFFFFF", "#FFFFFF"]
        } else {
            ["#FF0055", "#9900FF", "#0066FF", "#00FFAA"]
        };
        {
            let mut zc = zc_reset.borrow_mut();
            for (i, hex) in default_zones.iter().enumerate() {
                if i < zc.len() {
                    zc[i] = (*hex).to_string();
                }
            }
        }
        {
            let mut pk = pk_reset.borrow_mut();
            for c in pk.iter_mut() {
                *c = "#0099ED".to_string();
            }
        }
        {
            let zc = zc_reset.borrow();
            let mut kc = kc_reset.borrow_mut();
            for k in keys_reset.iter() {
                let c = if detected_mode == KeyboardMode::Omen4Zone {
                    let z = (get_zone_for_key(&k.name) - 1).clamp(0, 3) as usize;
                    zc[z].clone()
                } else if detected_mode == KeyboardMode::Victus1Zone {
                    zc[0].clone()
                } else {
                    "#0099ED".to_string()
                };
                kc.insert(k.name.clone(), c);
            }
        }
        if detected_mode == KeyboardMode::PerKey {
            crate::daemon_client::set_per_key_colors_sync(pk_reset.borrow().clone());
        } else if detected_mode == KeyboardMode::Victus1Zone {
            crate::daemon_client::set_color_sync(8, default_zones[0].to_string());
        } else {
            for (i, hex) in default_zones.iter().enumerate() {
                crate::daemon_client::set_color_sync(i as i32, (*hex).to_string());
            }
        }
        crate::daemon_client::set_mode_sync("static", *speed_reset.borrow() as i32);
        da_reset.queue_draw();
        dirty_reset.set(true);
    });
    color_container.append(&reset_rgb_btn);

    controls_row.append(&color_container);

    controls_row.append(&gtk::Separator::new(gtk::Orientation::Vertical));

    let label_sg = gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal);

    let speed_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();
    let speed_label = gtk::Label::builder()
        .label(crate::i18n::t("kb_effect_speed"))
        .xalign(0.0)
        .build();
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
            if m == "wave_ltr" || m == "wave_rtl" {
                crate::daemon_client::set_mode_sync("wave", speed as i32);
            } else {
                crate::daemon_client::set_mode_sync(m, speed as i32);
            }
            if let Some(anim_func) = &*ae_hook.borrow() {
                let anim_m = if m.starts_with("wave_") {
                    "wave"
                } else {
                    m.as_str()
                };
                anim_func(anim_m, speed);
            }
        }
    });
    speed_box.append(&speed_scale);

    let bright_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();
    let bright_label = gtk::Label::builder()
        .label(crate::i18n::t("kb_brightness"))
        .xalign(0.0)
        .build();
    label_sg.add_widget(&bright_label);
    bright_box.append(&bright_label);
    let bright_scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    let mut bval = 100.0;
    if let Some(state_json) = state_json_opt {
        if let Some(b) = state_json["brightness"].as_f64() {
            bval = b;
        }
    }
    bright_scale.set_value(bval);
    bright_scale.set_size_request(150, -1);
    bright_scale.set_valign(gtk::Align::Center);
    let brightness_globals = globals.clone();
    bright_scale.connect_value_changed(move |sc| {
        let mut state = brightness_globals.borrow_mut();
        state.brightness = sc.value() as i32;
        state.send();
    });
    bright_box.append(&bright_scale);

    // Experimental (#322): run supported effects on the keyboard MCU (Darfon 0d62:54bf & Primax 0461:*).
    // The MCU ignores brightness on Darfon (and accepts it via 0x0C on Primax).
    let hw_on = Rc::new(std::cell::Cell::new(false));
    let supports_hw_effects = detected_mode == KeyboardMode::PerKey
        && state_json_opt
            .as_ref()
            .and_then(|j| j["hid_device_pid"].as_str())
            .map(|p| {
                matches!(
                    p.to_ascii_uppercase().as_str(),
                    "54BF" | "4E9A" | "4E9B" | "4F03" | "4F11" | "4F1E" | "4ECB"
                )
            })
            .unwrap_or(false);
    let hw_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();
    if supports_hw_effects {
        hw_on.set(
            state_json_opt
                .as_ref()
                .and_then(|j| j["hw_effects"].as_bool())
                .unwrap_or(false),
        );
        let hw_label = gtk::Label::builder()
            .label(crate::i18n::t("kb_hw_effects"))
            .xalign(0.0)
            .build();
        label_sg.add_widget(&hw_label);
        hw_box.append(&hw_label);
        let hw_switch = gtk::Switch::builder()
            .valign(gtk::Align::Center)
            .active(hw_on.get())
            .build();
        hw_switch.set_tooltip_text(Some(crate::i18n::t("kb_hw_effects_tooltip")));
        hw_box.append(&hw_switch);
        bright_scale.set_sensitive(!hw_on.get());
        let hw_on_c = hw_on.clone();
        let bright_c = bright_scale.clone();
        hw_switch.connect_active_notify(move |sw| {
            hw_on_c.set(sw.is_active());
            crate::daemon_client::set_hw_effects_sync(sw.is_active());
            bright_c.set_sensitive(!sw.is_active());
        });
    }

    let sliders_container = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    sliders_container.append(&speed_box);
    sliders_container.append(&bright_box);
    if supports_hw_effects {
        sliders_container.append(&hw_box);
    }
    controls_row.append(&sliders_container);

    bottom_box.append(&controls_row);

    /// Replace a preset dropdown's model from disk.
    ///
    /// Kept as a free function so it can be deferred with idle_add_local_once:
    /// mutating the model while the DropDown is emitting selected-notify frees
    /// the StringList GTK is still walking (seen as a segfault in
    /// g_object_notify_by_pspec).
    /// Rebuild the preset list, optionally keeping a named preset selected.
    ///
    /// A DropDown whose model is replaced forgets the old selection, so the
    /// caller says what should be highlighted afterwards: a preset name, or
    /// None for the leading "Custom" entry.
    fn refresh_dropdown(dd: &gtk::DropDown, select: Option<&str>) {
        let mut labels: Vec<String> = vec![crate::i18n::t("kp_custom").to_string()];
        labels.extend(
            crate::keyboardrgb::presets::load_presets()
                .into_iter()
                .map(|p| p.name),
        );
        let refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
        let model = gtk::StringList::new(&refs);
        dd.set_model(Some(&model));

        let wanted = select
            .and_then(|name| labels.iter().position(|l| l == name))
            .filter(|idx| *idx > 0)
            .unwrap_or(0);
        dd.set_selected(wanted as u32);
    }

    // -- Presets (Save as preset + saved list) --
    // Only meaningful for per-key boards: a preset is a 104-slot frame, and
    // zone boards colour by zone instead.
    if detected_mode == KeyboardMode::PerKey {
        let preset_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .valign(gtk::Align::Center)
            .build();

        preset_box.append(&gtk::Separator::new(gtk::Orientation::Vertical));

        // A plain button, not a dropdown entry. Making "Save as preset..." an
        // item meant every ordinary selection had to rebuild the model from
        // inside the selection handler, which frees the StringList GTK is
        // still reading — that segfaulted in g_object_notify.
        let save_btn = gtk::Button::builder()
            .label(crate::i18n::t("kp_save"))
            .valign(gtk::Align::Center)
            .build();
        preset_box.append(&save_btn);

        preset_box.append(
            &gtk::Label::builder()
                .label(crate::i18n::t("kp_label"))
                .css_classes(["dim-label"])
                .build(),
        );

        // Build the dropdown with a model already attached: a DropDown created
        // without one does not reliably pick up a later set_model() before it
        // is realised, which showed up as an empty selector.
        // Index 0 is a neutral "no preset selected" entry. Without it a board
        // with a single preset would sit on that preset, and picking it again
        // emits no selected-notify (GTK only fires on a real change) — which
        // looked like "choosing my preset does nothing".
        let initial_labels: Vec<String> = {
            let mut v = vec![crate::i18n::t("kp_custom").to_string()];
            v.extend(
                crate::keyboardrgb::presets::load_presets()
                    .into_iter()
                    .map(|p| p.name),
            );
            v
        };
        let initial_refs: Vec<&str> = initial_labels.iter().map(|s| s.as_str()).collect();
        let preset_dd = gtk::DropDown::builder()
            .model(&gtk::StringList::new(&initial_refs))
            .valign(gtk::Align::Center)
            .tooltip_text(crate::i18n::t("kp_load_tooltip"))
            .build();

        // Persist the current frame under a name chosen by the user.
        let do_save: Rc<dyn Fn()> = {
            let pk = per_key_colors.clone();
            let dd = preset_dd.clone();
            let card = kb_card.clone();
            Rc::new(move || {
                let dialog = gtk::Window::builder()
                    .title(crate::i18n::t("kp_name_title"))
                    .transient_for(
                        &card
                            .root()
                            .and_downcast::<gtk::Window>()
                            .unwrap_or_default(),
                    )
                    .modal(true)
                    .default_width(360)
                    .build();
                let vbox = gtk::Box::builder()
                    .orientation(gtk::Orientation::Vertical)
                    .spacing(12)
                    .margin_top(18)
                    .margin_bottom(18)
                    .margin_start(18)
                    .margin_end(18)
                    .build();
                vbox.append(
                    &gtk::Label::builder()
                        .label(crate::i18n::t("kp_name_prompt"))
                        .xalign(0.0)
                        .wrap(true)
                        .build(),
                );
                let entry = gtk::Entry::builder()
                    .placeholder_text(crate::i18n::t("kp_name_placeholder"))
                    .activates_default(true)
                    .build();
                vbox.append(&entry);

                let btns = gtk::Box::builder()
                    .orientation(gtk::Orientation::Horizontal)
                    .spacing(6)
                    .halign(gtk::Align::End)
                    .build();
                let cancel = gtk::Button::with_label(crate::i18n::t("cancel"));
                let ok = gtk::Button::with_label(crate::i18n::t("save_preset"));
                ok.add_css_class("suggested-action");
                btns.append(&cancel);
                btns.append(&ok);
                vbox.append(&btns);
                dialog.set_child(Some(&vbox));

                {
                    let d = dialog.clone();
                    cancel.connect_clicked(move |_| d.close());
                }

                // Save on a later main-loop turn so the dropdown is rebuilt
                // after the dialog is closed, never during its own signal.
                let commit = {
                    let pk = pk.clone();
                    let dd = dd.clone();
                    let d = dialog.clone();
                    let e = entry.clone();
                    move || {
                        let name = crate::keyboardrgb::presets::normalize_name(&e.text());
                        if name.is_empty() {
                            e.add_css_class("error");
                            return;
                        }
                        let preset = crate::keyboardrgb::presets::KeyPreset {
                            name,
                            colors: pk.borrow().clone(),
                        };
                        let saved_name = preset.name.clone();
                        crate::keyboardrgb::presets::upsert_preset(preset);
                        d.close();
                        let dd = dd.clone();
                        gtk::glib::idle_add_local_once(move || {
                            refresh_dropdown(&dd, Some(&saved_name))
                        });
                    }
                };
                let commit = Rc::new(commit);
                {
                    let s = commit.clone();
                    ok.connect_clicked(move |_| s());
                }
                {
                    let s = commit.clone();
                    entry.connect_activate(move |_| s());
                }

                dialog.present();
                entry.grab_focus();
            })
        };
        {
            let s = do_save.clone();
            save_btn.connect_clicked(move |_| s());
        }

        // Apply a stored preset to every key, switch out of Off or animated
        // effects so the loaded layout renders cleanly, then push to the daemon.
        let apply_preset: Rc<dyn Fn(&str)> = {
            let dirty_apply = colors_dirty.clone();
            let pk = per_key_colors.clone();
            let kc = key_colors.clone();
            let keys = keys_rc.clone();
            let da = drawing_area.clone();
            let sm_apply = sel_mode.clone();
            let btn_key_apply = btn_key.clone();
            let dd_eff_apply = effects_dropdown.clone();
            let msl_apply = msl_c.clone();
            let globals_apply = globals.clone();
            Rc::new(move |name: &str| {
                let Some(preset) = crate::keyboardrgb::presets::load_presets()
                    .into_iter()
                    .find(|p| p.name == name)
                else {
                    return;
                };
                {
                    let mut frame = pk.borrow_mut();
                    for (i, slot) in frame.iter_mut().enumerate() {
                        // Pad short presets with the default rather than
                        // leaving stale colours on the tail slots.
                        *slot = preset
                            .colors
                            .get(i)
                            .cloned()
                            .unwrap_or_else(|| "#0099ED".to_string());
                    }
                }
                {
                    let frame = pk.borrow();
                    let mut kc = kc.borrow_mut();
                    for k in keys.iter() {
                        if let Some(c) = frame.get(k.id) {
                            kc.insert(k.name.clone(), c.clone());
                        }
                    }
                }
                if *sm_apply.borrow() == SelectionMode::Off {
                    btn_key_apply.set_active(true);
                }
                if let Some(pk_idx) = msl_apply.iter().position(|m| m == "per_key_custom") {
                    if dd_eff_apply.selected() as usize != pk_idx {
                        dd_eff_apply.set_selected(pk_idx as u32);
                    }
                }
                {
                    let mut st = globals_apply.borrow_mut();
                    st.power = true;
                }
                crate::daemon_client::set_per_key_colors_sync(pk.borrow().clone());
                da.queue_draw();
                // The frame now equals this preset, so a later manual edit is
                // what makes it "custom" again.
                dirty_apply.set(false);
            })
        };

        {
            let apply = apply_preset.clone();
            preset_dd.connect_selected_notify(move |dd| {
                let idx = dd.selected();
                if idx == 0 {
                    return; // neutral entry, nothing to apply
                }
                let names = crate::keyboardrgb::presets::load_presets();
                if let Some(p) = names.get((idx - 1) as usize) {
                    apply(&p.name);
                }
                // Stay on the chosen preset: that is what the board now shows.
                // apply() clears the dirty flag, so the watcher will not pull
                // us back to "Custom" straight after.
                let _ = dd;
            });
        }

        // Deleting acts on the current selection.
        let delete_btn = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text(crate::i18n::t("kp_delete"))
            .valign(gtk::Align::Center)
            .build();
        {
            let dd = preset_dd.clone();
            delete_btn.connect_clicked(move |_| {
                let idx = dd.selected();
                if idx == 0 {
                    return;
                }
                let names = crate::keyboardrgb::presets::load_presets();
                if let Some(p) = names.get((idx - 1) as usize) {
                    crate::keyboardrgb::presets::delete_preset(&p.name);
                    // The frame no longer matches anything that is left.
                    let dd = dd.clone();
                    gtk::glib::idle_add_local_once(move || refresh_dropdown(&dd, None));
                }
            });
        }
        preset_box.append(&preset_dd);
        preset_box.append(&delete_btn);

        // Any manual edit (painting keys, All, the colour picker) means the
        // board no longer matches a saved preset, so fall back to the neutral
        // entry instead of claiming one is active.
        {
            let dd_weak = preset_dd.downgrade();
            let dirty = colors_dirty.clone();
            gtk::glib::timeout_add_local(std::time::Duration::from_millis(300), move || {
                let Some(dd) = dd_weak.upgrade() else {
                    return gtk::glib::ControlFlow::Break;
                };
                if dirty.take() {
                    refresh_dropdown(&dd, None);
                }
                gtk::glib::ControlFlow::Continue
            });
        }

        bottom_box.append(&preset_box);
    }

    kb_card.append(&bottom_box);

    let tick_id: Rc<RefCell<Option<gtk::TickCallbackId>>> = Rc::new(RefCell::new(None));
    let da_anim = drawing_area.clone();
    let anim_state_apply = anim_state.clone();

    let bright_scale_apply = bright_scale.clone();
    let hw_on_apply = hw_on.clone();
    let apply_anim: AnimCallback = Rc::new(move |mode: &str, speed: f64| {
        let mut ts = anim_state_apply.borrow_mut();
        ts.0 = mode.to_string();
        ts.1 = speed;
        drop(ts);
        bright_scale_apply.set_sensitive(!hw_on_apply.get());

        let needs_anim = mode != "static" && mode != "per_key_custom";

        let mut curr_id = tick_id.borrow_mut();
        if !needs_anim && curr_id.is_some() {
            curr_id.take().unwrap().remove();
        } else if needs_anim && curr_id.is_none() {
            let anim_state_tick = anim_state_apply.clone();
            let da_tick = da_anim.clone();
            let id = da_anim.add_tick_callback(move |_, clock| {
                if !da_tick.is_mapped() {
                    return glib::ControlFlow::Continue;
                }
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

    (
        kb_card,
        apply_anim,
        gtk::Box::new(gtk::Orientation::Horizontal, 0),
    )
}
