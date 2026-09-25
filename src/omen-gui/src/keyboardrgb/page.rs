use gtk::prelude::*;
use libadwaita as adw;
use adw::prelude::*;
use std::rc::Rc;
use std::cell::RefCell;
use crate::keyboardrgb::helpers::*;
use crate::keyboardrgb::keyboard::*;
use crate::keyboardrgb::lightbar::*;
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

    let is_omen_brand = true; // Make UI visible for all, gray out if unsupported
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
        let has_lightbar_hardware = detected_mode == KeyboardMode::DesktopRgb || lb_prod_lower.contains("desktop") || lb_prod_lower.contains("transcend") || lb_prod_lower.contains("max");
        let mut show_lightbar = true;
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
        
        // Force show if hardware is not supported so the user can explicitly see the "Unsupported" message.
        if !has_lightbar_hardware {
            show_lightbar = true;
        }

        lb_group.set_visible(show_lightbar);
        lb_preview_group.set_visible(show_lightbar);

        if !has_lightbar_hardware {
            lb_group.set_sensitive(false);
            lb_preview_group.set_sensitive(false);
            lb_group.set_description(Some(crate::i18n::t("lightbar_unsupported")));
        }

        lb_group_ret = Some(lb_group);
        lb_preview_group_ret = Some(lb_preview_group);
    }

    (page, lb_group_ret, lb_preview_group_ret)
}

