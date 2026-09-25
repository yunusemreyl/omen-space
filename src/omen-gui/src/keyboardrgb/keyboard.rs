use gtk::prelude::*;
use std::rc::Rc;
use std::cell::RefCell;
use std::collections::HashMap;
use crate::keyboardrgb::helpers::*;
pub fn build_interactive_keyboard(
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
        .label(&*crate::i18n::t("rgb_paint_hint"))
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
    #[allow(deprecated)]
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
