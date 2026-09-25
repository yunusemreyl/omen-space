import re

with open("src/omen-gui/src/keyboardrgb.rs", "r") as f:
    content = f.read()

# 1. Update the layout of bottom_box
old_bottom_layout = """    let bottom_box = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(16).margin_top(12).halign(gtk::Align::Center).build();

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

    let instructions = gtk::Label::builder()
        .label("İlk renk seçiminizi yapıp boyamak istediğiniz tuşlara/bölgelere tıklayın.")
        .css_classes(["dim-label"])
        .margin_end(16)
        .build();
    bottom_box.append(&instructions);
    
    bottom_box.append(&gtk::Label::builder().label("Color:").css_classes(["dim-label"]).build());
    bottom_box.append(&global_color_btn);

    // Speed and Brightness in bottom box
    let speed_box = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(6).build();
    speed_box.append(&gtk::Label::builder().label(crate::i18n::t("kb_effect_speed")).build());
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
    bright_box.append(&gtk::Label::builder().label(crate::i18n::t("kb_brightness")).build());
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
    
    bottom_box.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    bottom_box.append(&sliders_container);"""

new_bottom_layout = """    let bottom_box = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(12).margin_top(12).halign(gtk::Align::Center).build();

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

    let speed_box = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(6).build();
    speed_box.append(&gtk::Label::builder().label(crate::i18n::t("kb_effect_speed")).build());
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
    bright_box.append(&gtk::Label::builder().label(crate::i18n::t("kb_brightness")).build());
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
    
    bottom_box.append(&controls_row);"""

content = content.replace(old_bottom_layout, new_bottom_layout)

# 2. Fix the hover logic for Zone/Key and remove bounding box drawing
old_draw_logic = """            let is_sel = sel.contains(&k.name);
            let is_hov = hov.as_ref() == Some(&k.name);
            
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
        
        let active_sel = *sel_mode_draw.borrow();
        if active_sel == SelectionMode::Zone {
            // Draw zone bounding boxes
            for z in 1..=4 {
                let mut min_x = f64::MAX;
                let mut min_y = f64::MAX;
                let mut max_x = f64::MIN;
                let mut max_y = f64::MIN;
                let mut found = false;
                for k in keys_draw.iter() {
                    if get_zone_for_key(&k.name) == z {
                        if k.x < min_x { min_x = k.x; }
                        if k.y < min_y { min_y = k.y; }
                        if k.x + k.w > max_x { max_x = k.x + k.w; }
                        if k.y + k.h > max_y { max_y = k.y + k.h; }
                        found = true;
                    }
                }
                if found {
                    cr.set_source_rgba(1.0, 1.0, 1.0, 0.4);
                    cr.set_line_width(2.0);
                    // cr.set_dash(&[4.0, 4.0], 0.0);
                    draw_rounded_rect(cr, min_x - 2.0, min_y - 2.0, (max_x - min_x) + 4.0, (max_y - min_y) + 4.0, 6.0);
                    let _ = cr.stroke();
                }
            }
        } else if active_sel == SelectionMode::Key {
             // In Key mode, maybe draw a faint dash around all keys to signify they are individual?
             // Actually, the stroke on each key is enough, but we can make it slightly brighter if needed.
             // But let's leave it, the keys already have individual strokes.
        }
    });"""

new_draw_logic = """            let active_sel = *sel_mode_draw.borrow();
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
    });"""

content = content.replace(old_draw_logic, new_draw_logic)

with open("src/omen-gui/src/keyboardrgb.rs", "w") as f:
    f.write(content)
