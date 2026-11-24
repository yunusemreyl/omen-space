use gtk::prelude::*;
use libadwaita as adw;
use adw::prelude::*;
use std::rc::Rc;
use std::cell::RefCell;
use crate::i18n;

/* ─────────────────────────────────────────────────────────────
   undervolt.rs — Intel 13th Gen Undervolt & Power Limits
   ───────────────────────────────────────────────────────────── */

fn is_cpu_locked(cpu: &str) -> bool {
    let name_part = cpu.split('·').next().unwrap_or(cpu).to_uppercase();
    if name_part.contains("AMD") { return false; } // AMD handled by ryzenadj in daemon
    if !name_part.contains("INTEL") { return false; }
    
    // Check for 12, 13, 14th gen
    if name_part.contains("-12") || name_part.contains("-13") || name_part.contains("-14") 
        || name_part.contains(" 12") || name_part.contains(" 13") || name_part.contains(" 14") {
        if name_part.contains("HK") || name_part.contains("HX") {
            return false;
        }
        if name_part.contains("H") || name_part.contains("P") || name_part.contains("U") {
            return true;
        }
    }
    // Check for Core Ultra
    if name_part.contains("ULTRA") {
        return true;
    }
    false
}

fn save_battery_settings(enabled: bool, stop: f64, start: f64, off_hold: bool) {
    if let Ok(home) = std::env::var("HOME") {
        let dir = format!("{}/.config/omenspace", home);
        let _ = std::fs::create_dir_all(&dir);
        let path = format!("{}/settings.json", dir);
        let mut json = serde_json::json!({});
        if let Ok(js) = std::fs::read_to_string(&path) {
            if let Ok(j) = serde_json::from_str::<serde_json::Value>(&js) {
                json = j;
            }
        }
        json["battery_care"] = serde_json::json!(enabled);
        json["battery_stop"] = serde_json::json!(stop);
        json["battery_start"] = serde_json::json!(start);
        json["battery_off_hold"] = serde_json::json!(off_hold);
        let _ = std::fs::write(path, serde_json::to_string_pretty(&json).unwrap_or_default());
    }
}

pub fn build_page() -> gtk::Box {
    let mut init_battery_care = false;
    let mut init_battery_stop = 80.0_f64;
    let mut init_battery_start = 75.0_f64;
    let mut init_battery_off_hold = true;

    if let Ok(home) = std::env::var("HOME") {
        let path = format!("{}/.config/omenspace/settings.json", home);
        if let Ok(json_str) = std::fs::read_to_string(&path) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&json_str) {
                if let Some(bc) = json.get("battery_care").and_then(|v| v.as_bool()) {
                    init_battery_care = bc;
                }
                if let Some(bs) = json.get("battery_stop").and_then(|v| v.as_f64()) {
                    init_battery_stop = bs.clamp(50.0, 95.0);
                }
                if let Some(br) = json.get("battery_start").and_then(|v| v.as_f64()) {
                    init_battery_start = br.clamp(20.0, 90.0);
                }
                if let Some(bh) = json.get("battery_off_hold").and_then(|v| v.as_bool()) {
                    init_battery_off_hold = bh;
                }
            }
        }
    }

    let specs = crate::daemon_client::get_hardware_specs_sync();

    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .build();

    // ── Header ───────────────────────────────────────────────
    let hdr = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_bottom(4)
        .build();
    hdr.append(&gtk::Label::builder()
        .label(i18n::t("title_undervolt"))
        .css_classes(["page-title"])
        .halign(gtk::Align::Start)
        .build());
    hdr.append(&gtk::Label::builder()
        .label(i18n::t("uv_desc"))
        .css_classes(["os-section-desc"])
        .halign(gtk::Align::Start)
        .build());
    page.append(&hdr);

    // ── Battery Charge Limiter (Top) ─────────────────────────
    let bat_status_badge = gtk::Label::builder()
        .label(if init_battery_care {
            i18n::t("battery_limit_status_limiting")
        } else {
            i18n::t("battery_limit_status_off")
        })
        .css_classes([if init_battery_care { "badge-ok" } else { "badge-neutral" }])
        .valign(gtk::Align::Center)
        .build();

    let bat_group = adw::PreferencesGroup::builder()
        .title(i18n::t("battery_limit_group"))
        .header_suffix(&bat_status_badge)
        .build();

    let bat_sub = if !specs.battery_spec.is_empty() {
        format!("{} ({})", i18n::t("battery_care_sub"), specs.battery_spec)
    } else {
        i18n::t("battery_care_sub").to_string()
    };
    let battery_row = adw::SwitchRow::builder()
        .title(i18n::t("battery_care"))
        .subtitle(&bat_sub)
        .build();
    battery_row.set_active(init_battery_care);
    bat_group.add(&battery_row);

    let stop_row = adw::ActionRow::builder()
        .title(i18n::t("battery_limit_stop"))
        .subtitle(i18n::t("battery_limit_stop_sub"))
        .build();
    let stop_spin = gtk::SpinButton::with_range(50.0, 95.0, 5.0);
    stop_spin.set_value(init_battery_stop);
    stop_spin.set_valign(gtk::Align::Center);
    stop_row.add_suffix(&stop_spin);
    stop_row.set_sensitive(init_battery_care);
    bat_group.add(&stop_row);

    let start_row = adw::ActionRow::builder()
        .title(i18n::t("battery_limit_start"))
        .subtitle(i18n::t("battery_limit_start_sub"))
        .build();
    let start_spin = gtk::SpinButton::with_range(20.0, 90.0, 5.0);
    start_spin.set_value(init_battery_start.min((init_battery_stop - 5.0).max(20.0)));
    start_spin.set_valign(gtk::Align::Center);
    start_row.add_suffix(&start_spin);
    start_row.set_sensitive(init_battery_care);
    bat_group.add(&start_row);

    let off_hold_row = adw::SwitchRow::builder()
        .title(i18n::t("battery_limit_off_hold"))
        .subtitle(i18n::t("battery_limit_off_hold_sub"))
        .build();
    off_hold_row.set_active(init_battery_off_hold);
    off_hold_row.set_sensitive(init_battery_care);
    bat_group.add(&off_hold_row);

    let bypass_row = adw::SwitchRow::builder()
        .title(i18n::t("battery_limit_bypass"))
        .subtitle(i18n::t("battery_limit_bypass_sub"))
        .build();
    bypass_row.set_active(false);
    bypass_row.set_sensitive(init_battery_care);
    bat_group.add(&bypass_row);

    page.append(&bat_group);

    let syncing_battery = Rc::new(std::cell::Cell::new(false));
    let update_bat_ui = {
        let stop_r = stop_row.clone();
        let start_r = start_row.clone();
        let hold_r = off_hold_row.clone();
        let byp_r = bypass_row.clone();
        let badge = bat_status_badge.clone();
        let bat_sw = battery_row.clone();
        Rc::new(move || {
            let active = bat_sw.is_active();
            let byp = byp_r.is_active();
            stop_r.set_sensitive(active);
            start_r.set_sensitive(active);
            hold_r.set_sensitive(active);
            byp_r.set_sensitive(active);
            if !active {
                badge.set_label(i18n::t("battery_limit_status_off"));
                badge.set_css_classes(&["badge-neutral"]);
            } else if byp {
                badge.set_label(i18n::t("battery_limit_status_bypassed"));
                badge.set_css_classes(&["badge-accent"]);
            } else {
                badge.set_label(i18n::t("battery_limit_status_limiting"));
                badge.set_css_classes(&["badge-ok"]);
            }
        })
    };

    let send_bat_cfg = {
        let bat_sw = battery_row.clone();
        let stop_sp = stop_spin.clone();
        let start_sp = start_spin.clone();
        let hold_sw = off_hold_row.clone();
        let ui = update_bat_ui.clone();
        let guard = syncing_battery.clone();
        Rc::new(move || {
            if guard.get() {
                return;
            }
            let stop_v = stop_sp.value() as u32;
            let start_v = (start_sp.value() as u32).min(stop_v.saturating_sub(1).max(20));
            if start_sp.value() as u32 != start_v {
                guard.set(true);
                start_sp.set_value(start_v as f64);
                guard.set(false);
            }
            save_battery_settings(
                bat_sw.is_active(),
                stop_sp.value(),
                start_sp.value(),
                hold_sw.is_active(),
            );
            ui();
            crate::daemon_client::set_battery_limit_config_sync(
                bat_sw.is_active(),
                stop_v,
                start_v,
                hold_sw.is_active(),
            );
        })
    };

    {
        let f = send_bat_cfg.clone();
        battery_row.connect_active_notify(move |_| f());
    }
    {
        let f = send_bat_cfg.clone();
        stop_spin.connect_value_changed(move |_| f());
    }
    {
        let f = send_bat_cfg.clone();
        let guard = syncing_battery.clone();
        let stop_sp = stop_spin.clone();
        start_spin.connect_value_changed(move |sp| {
            if guard.get() {
                return;
            }
            let max_s = (stop_sp.value() - 1.0).max(20.0);
            if sp.value() > max_s {
                guard.set(true);
                sp.set_value(max_s);
                guard.set(false);
            }
            f();
        });
    }
    {
        let f = send_bat_cfg.clone();
        off_hold_row.connect_active_notify(move |_| f());
    }
    {
        let guard = syncing_battery.clone();
        let ui = update_bat_ui.clone();
        bypass_row.connect_active_notify(move |r| {
            if guard.get() {
                return;
            }
            ui();
            crate::daemon_client::set_battery_bypass_sync(r.is_active());
        });
    }
    {
        let guard = syncing_battery.clone();
        let bat_sw = battery_row.clone();
        let stop_sp = stop_spin.clone();
        let start_sp = start_spin.clone();
        let hold_sw = off_hold_row.clone();
        let byp_sw = bypass_row.clone();
        let stop_r = stop_row.clone();
        let start_r = start_row.clone();
        let hold_r = off_hold_row.clone();
        let byp_r = bypass_row.clone();
        let badge = bat_status_badge.clone();
        glib::spawn_future_local(async move {
            if let Ok(state_json) = crate::daemon_client::get_battery_limit_state_async().await {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&state_json) {
                    let supported = val.get("supported").and_then(|v| v.as_bool()).unwrap_or(false);
                    let enabled = val.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
                    let stop = val.get("stop").and_then(|v| v.as_f64()).unwrap_or(80.0);
                    let start = val.get("start").and_then(|v| v.as_f64()).unwrap_or(75.0);
                    let off_hold = val.get("off_hold").and_then(|v| v.as_bool()).unwrap_or(true);
                    let bypass = val.get("bypass").and_then(|v| v.as_bool()).unwrap_or(false);
                    let lim_state = val.get("limiter_state").and_then(|v| v.as_str()).unwrap_or("off");
                    let backend = val.get("backend").and_then(|v| v.as_str()).unwrap_or("none");

                    guard.set(true);
                    bat_sw.set_active(enabled);
                    stop_sp.set_value(stop.clamp(50.0, 95.0));
                    start_sp.set_value(start.clamp(20.0, 90.0));
                    hold_sw.set_active(off_hold);
                    byp_sw.set_active(bypass);
                    guard.set(false);

                    let native_only = backend == "native_threshold";
                    stop_r.set_sensitive(enabled);
                    start_r.set_sensitive(enabled && !native_only);
                    hold_r.set_sensitive(enabled && !native_only);
                    byp_r.set_sensitive(enabled);

                    if !supported {
                        badge.set_label(i18n::t("battery_limit_status_unsupported"));
                        badge.set_css_classes(&["badge-warn"]);
                    } else if !enabled {
                        badge.set_label(i18n::t("battery_limit_status_off"));
                        badge.set_css_classes(&["badge-neutral"]);
                    } else if bypass {
                        badge.set_label(i18n::t("battery_limit_status_bypassed"));
                        badge.set_css_classes(&["badge-accent"]);
                    } else if lim_state == "charging" {
                        badge.set_label(i18n::t("battery_limit_status_charging"));
                        badge.set_css_classes(&["badge-accent"]);
                    } else {
                        badge.set_label(i18n::t("battery_limit_status_limiting"));
                        badge.set_css_classes(&["badge-ok"]);
                    }
                }
            }
        });
    }

    // ── Protection / Warning Banner ───────────────────────────
    let warn_card = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .css_classes(["os-card"])
        .build();

    let warn_icon = gtk::Image::builder()
        .icon_name("dialog-warning-symbolic")
        .pixel_size(24)
        .valign(gtk::Align::Center)
        .build();
    warn_card.append(&warn_icon);

    let warn_text_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .build();
    let warn_title = gtk::Label::builder()
        .label(i18n::t("uv_lock_title"))
        .css_classes(["chip-title"])
        .halign(gtk::Align::Start)
        .build();
    warn_text_box.append(&warn_title);
    
    let warn_desc = gtk::Label::builder()
        .label(i18n::t("uv_lock_desc"))
        .css_classes(["os-section-desc"])
        .halign(gtk::Align::Start)
        .wrap(true)
        .build();
    warn_text_box.append(&warn_desc);
    
    warn_card.append(&warn_text_box);

    let status_badge = gtk::Label::builder()
        .label(i18n::t("msr_protected"))
        .css_classes(["badge-warn"])
        .valign(gtk::Align::Center)
        .build();
    warn_card.append(&status_badge);

    page.append(&warn_card);

    // ── Voltage Offsets Section ───────────────────────────────
    let volt_group = adw::PreferencesGroup::builder()
        .title(i18n::t("voltage_offsets_group"))
        .description(i18n::t("voltage_offsets_desc"))
        .build();

    // 1. Core Voltage Offset — initialize then load from daemon
    let core_val = Rc::new(RefCell::new(0i32));
    let core_row = adw::ActionRow::builder()
        .title(i18n::t("core_offset_title"))
        .subtitle(i18n::t("core_offset_sub"))
        .build();
    let core_scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, -150.0, 0.0, 5.0);
    core_scale.set_value(0.0);
    core_scale.set_hexpand(true);
    core_scale.set_size_request(200, -1);
    core_scale.set_valign(gtk::Align::Center);
    let core_lbl = gtk::Label::builder()
        .label("0 mV")
        .css_classes(["os-monitor-val-sm"])
        .valign(gtk::Align::Center)
        .margin_start(8)
        .build();
    let core_lbl_clone = core_lbl.clone();
    let core_val_clone = core_val.clone();
    core_scale.connect_value_changed(move |s| {
        let val = s.value() as i32;
        *core_val_clone.borrow_mut() = val;
        core_lbl_clone.set_label(&format!("{} mV", val));
    });
    core_row.add_suffix(&core_scale);
    core_row.add_suffix(&core_lbl);
    volt_group.add(&core_row);

    // 2. CPU Cache (Ring) Offset
    let cache_row = adw::ActionRow::builder()
        .title(i18n::t("cache_offset_title"))
        .subtitle(i18n::t("cache_offset_sub"))
        .build();
    let cache_scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, -150.0, 0.0, 5.0);
    cache_scale.set_value(0.0);
    cache_scale.set_hexpand(true);
    cache_scale.set_size_request(200, -1);
    cache_scale.set_valign(gtk::Align::Center);
    let cache_lbl = gtk::Label::builder()
        .label("0 mV")
        .css_classes(["os-monitor-val-sm"])
        .valign(gtk::Align::Center)
        .margin_start(8)
        .build();
    let cache_lbl_clone = cache_lbl.clone();
    cache_scale.connect_value_changed(move |s| {
        let val = s.value() as i32;
        cache_lbl_clone.set_label(&format!("{} mV", val));
    });
    cache_row.add_suffix(&cache_scale);
    cache_row.add_suffix(&cache_lbl);
    volt_group.add(&cache_row);

    page.append(&volt_group);

    // ── Power Limits (PL1 / PL2 / Tau / TCC) ───────────────────
    let pwr_group = adw::PreferencesGroup::builder()
        .title(i18n::t("power_limits_title"))
        .description(i18n::t("power_limits_desc"))
        .build();

    // PL1 (Sürekli Güç Limiti)
    let pl1_row = adw::ActionRow::builder()
        .title(i18n::t("pl1_label"))
        .subtitle(i18n::t("pl1_sub"))
        .build();
    let pl1_scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 25.0, 220.0, 1.0);
    pl1_scale.set_value(45.0);
    pl1_scale.set_hexpand(true);
    pl1_scale.set_size_request(200, -1);
    pl1_scale.set_valign(gtk::Align::Center);
    let pl1_lbl = gtk::Label::builder()
        .label("45 W")
        .css_classes(["os-monitor-val-sm"])
        .valign(gtk::Align::Center)
        .margin_start(8)
        .build();
    let pl1_lbl_clone = pl1_lbl.clone();
    pl1_scale.connect_value_changed(move |s| {
        let val = s.value() as i32;
        pl1_lbl_clone.set_label(&format!("{} W", val));
    });
    pl1_row.add_suffix(&pl1_scale);
    pl1_row.add_suffix(&pl1_lbl);
    pwr_group.add(&pl1_row);

    // PL2 (Kısa Süreli Turbo Güç Limiti)
    let pl2_row = adw::ActionRow::builder()
        .title(i18n::t("pl2_label"))
        .subtitle(i18n::t("pl2_sub"))
        .build();
    let pl2_scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 45.0, 260.0, 1.0);
    pl2_scale.set_value(95.0);
    pl2_scale.set_hexpand(true);
    pl2_scale.set_size_request(200, -1);
    pl2_scale.set_valign(gtk::Align::Center);
    let pl2_lbl = gtk::Label::builder()
        .label("95 W")
        .css_classes(["os-monitor-val-sm"])
        .valign(gtk::Align::Center)
        .margin_start(8)
        .build();
    let pl2_lbl_clone = pl2_lbl.clone();
    pl2_scale.connect_value_changed(move |s| {
        let val = s.value() as i32;
        pl2_lbl_clone.set_label(&format!("{} W", val));
    });
    pl2_row.add_suffix(&pl2_scale);
    pl2_row.add_suffix(&pl2_lbl);
    pwr_group.add(&pl2_row);

    // TCC Offset (Termal Tetikleme Sıcaklığı)
    let tj_max_state = Rc::new(RefCell::new(100i32));
    let is_voltage_locked = Rc::new(RefCell::new(false));
    let is_tcc_locked = Rc::new(RefCell::new(false));
    let loaded_core_mv = Rc::new(RefCell::new(0i32));
    let loaded_cache_mv = Rc::new(RefCell::new(0i32));
    let loaded_tcc = Rc::new(RefCell::new(0i32));

    let tcc_row = adw::ActionRow::builder()
        .title(i18n::t("tcc_label"))
        .subtitle(i18n::t("tcc_sub"))
        .build();
    let tcc_scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 15.0, 1.0);
    tcc_scale.set_value(0.0);
    tcc_scale.set_hexpand(true);
    tcc_scale.set_size_request(200, -1);
    tcc_scale.set_valign(gtk::Align::Center);
    let tcc_lbl = gtk::Label::builder()
        .label(format!("100°C ({}: 0°C)", i18n::t("tcc_offset_str")))
        .css_classes(["os-monitor-val-sm"])
        .valign(gtk::Align::Center)
        .margin_start(8)
        .build();
    let tcc_lbl_clone = tcc_lbl.clone();
    let tj_max_for_scale = tj_max_state.clone();
    let tcc_locked_for_scale = is_tcc_locked.clone();
    tcc_scale.connect_value_changed(move |s| {
        let val = s.value() as i32;
        let max_temp = *tj_max_for_scale.borrow() - val;
        if *tcc_locked_for_scale.borrow() {
            tcc_lbl_clone.set_label(&format!("{}°C ({}: {}°C · Locked)", max_temp, i18n::t("tcc_offset_str"), val));
        } else {
            tcc_lbl_clone.set_label(&format!("{}°C ({}: {}°C)", max_temp, i18n::t("tcc_offset_str"), val));
        }
    });
    tcc_row.add_suffix(&tcc_scale);
    tcc_row.add_suffix(&tcc_lbl);
    pwr_group.add(&tcc_row);

    page.append(&pwr_group);

    // ── Action Buttons Row ────────────────────────────────────
    let actions_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .halign(gtk::Align::End)
        .margin_top(4)
        .margin_bottom(12)
        .build();

    let default_pl1 = Rc::new(RefCell::new(45.0_f64));
    let default_pl2 = Rc::new(RefCell::new(95.0_f64));

    let reset_btn = gtk::Button::builder()
        .label("Restore initial power limits")
        .sensitive(false)
        .tooltip_text("Restore the first observed limits, which may include earlier tuning; these are not verified firmware defaults.")
        .css_classes(["ec-btn"])
        .build();
    let pl1_s_c = pl1_scale.clone();
    let pl2_s_c = pl2_scale.clone();
    let def_pl1_reset = default_pl1.clone();
    let def_pl2_reset = default_pl2.clone();
    reset_btn.connect_clicked(move |_| {
        let def1 = *def_pl1_reset.borrow(); let def2 = *def_pl2_reset.borrow();
        pl1_s_c.set_value(def1); pl2_s_c.set_value(def2);
        crate::daemon_client::reset_power_limits_sync(def1 as i32,def2 as i32);
    });

    let apply_btn = gtk::Button::builder()
        .label(i18n::t("apply_save"))
        .css_classes(["suggested-action"])
        .build();

    let core_val_c = core_val.clone();
    let pl1_s_apply = pl1_scale.clone();
    let pl2_s_apply = pl2_scale.clone();
    let cache_s_apply = cache_scale.clone();
    let tcc_s_apply = tcc_scale.clone();
    let v_locked_apply = is_voltage_locked.clone();
    let tcc_locked_apply = is_tcc_locked.clone();
    let loaded_core_apply = loaded_core_mv.clone();
    let loaded_cache_apply = loaded_cache_mv.clone();
    let loaded_tcc_apply = loaded_tcc.clone();
    let warn_title_apply = warn_title.clone();
    let warn_desc_apply = warn_desc.clone();
    let badge_apply = status_badge.clone();
    apply_btn.connect_clicked(move |_| {
        let core_mv = *core_val_c.borrow();
        let cache_mv = cache_s_apply.value() as i32;
        let pl1 = pl1_s_apply.value() as i32;
        let pl2 = pl2_s_apply.value() as i32;
        let tcc = tcc_s_apply.value() as i32;

        // Only call SetOffset when voltage controls are supported and changed (#313)
        if !*v_locked_apply.borrow()
            && (core_mv != *loaded_core_apply.borrow() || cache_mv != *loaded_cache_apply.borrow())
        {
            let loaded_c = loaded_core_apply.clone();
            let loaded_ca = loaded_cache_apply.clone();
            let wt = warn_title_apply.clone();
            let wd = warn_desc_apply.clone();
            let bd = badge_apply.clone();
            glib::spawn_future_local(async move {
                let r1 = crate::daemon_client::set_offset_async("core".to_string(), core_mv).await;
                let r2 = crate::daemon_client::set_offset_async("cache".to_string(), cache_mv).await;
                let ok1 = r1.as_deref().is_ok_and(|s| s == "OK");
                let ok2 = r2.as_deref().is_ok_and(|s| s == "OK");
                if ok1 && ok2 {
                    *loaded_c.borrow_mut() = core_mv;
                    *loaded_ca.borrow_mut() = cache_mv;
                } else {
                    wt.set_label(i18n::t("uv_lock_title"));
                    wd.set_label(i18n::t("uv_lock_desc"));
                    bd.set_label(i18n::t("msr_protected"));
                    bd.set_css_classes(&["badge-warn"]);
                }
            });
        }

        crate::daemon_client::set_power_limits_sync(pl1, pl2);

        // Only call SetTccOffset when TCC is unlocked and changed (#313)
        if !*tcc_locked_apply.borrow() && tcc != *loaded_tcc_apply.borrow() {
            crate::daemon_client::set_tcc_offset_sync(tcc);
            *loaded_tcc_apply.borrow_mut() = tcc;
        }
    });

    actions_box.append(&reset_btn);
    actions_box.append(&apply_btn);

    page.append(&actions_box);

    // ── Load current state from daemon (async) ────────────────
    let core_s_load = core_scale.clone();
    let cache_s_load = cache_scale.clone();
    let pl1_s_load = pl1_scale.clone();
    let pl2_s_load = pl2_scale.clone();
    let tcc_s_load = tcc_scale.clone();
    let tcc_lbl_load = tcc_lbl.clone();
    let tcc_row_load = tcc_row.clone();
    let pl1_row_load = pl1_row.clone();
    let pl2_row_load = pl2_row.clone();
    let badge_load = status_badge.clone();
    let reset_btn_load = reset_btn.clone();
    let def_pl1_load = default_pl1.clone();
    let def_pl2_load = default_pl2.clone();
    
    let cache_row_clone = cache_row.clone();
    let core_row_clone = core_row.clone();
    let core_scale_range = core_scale.clone();
    
    let warn_title_clone = warn_title.clone();
    let warn_desc_clone = warn_desc.clone();
    let volt_group_clone = volt_group.clone();

    let tj_max_load = tj_max_state.clone();
    let v_locked_load = is_voltage_locked.clone();
    let tcc_locked_load = is_tcc_locked.clone();
    let loaded_core_load = loaded_core_mv.clone();
    let loaded_cache_load = loaded_cache_mv.clone();
    let loaded_tcc_load = loaded_tcc.clone();

    glib::spawn_future_local(async move {
        // First check CPU lock
        let mut locked = false;
        let mut is_amd = false;
        let mut cpu_spec_str = String::new();
        if let Ok(hw_json) = crate::daemon_client::get_hardware_specs_async().await {
            if let Ok(hw) = serde_json::from_str::<crate::daemon_client::HardwareSpecs>(&hw_json) {
                cpu_spec_str = hw.cpu_spec.clone();
                let cpu_upper = hw.cpu_spec.to_uppercase();
                is_amd = cpu_upper.contains("AMD");
                // High-TDP HX / i9 / Core Ultra 7/9 / Ryzen 9 CPUs default to higher PL1/PL2 (#302)
                if cpu_upper.contains("HX") || cpu_upper.contains("I9") || cpu_upper.contains("ULTRA 9") || cpu_upper.contains("ULTRA 7") || cpu_upper.contains("RYZEN 9") {
                    *def_pl1_load.borrow_mut() = 80.0;
                    *def_pl2_load.borrow_mut() = 140.0;
                    pl1_s_load.set_value(80.0);
                    pl2_s_load.set_value(140.0);
                }
                if is_cpu_locked(&hw.cpu_spec) {
                    locked = true;
                }
            }
        }

        if let Ok(json) = crate::daemon_client::get_undervolt_state_async().await {
            if let Ok(state) = serde_json::from_str::<serde_json::Value>(&json) {
                if let Some(amd_flag) = state.get("is_amd").and_then(|v| v.as_bool()) {
                    is_amd = amd_flag;
                }
                if let Some(v_locked) = state.get("voltage_locked").and_then(|v| v.as_bool()) {
                    locked = locked || v_locked;
                }
                *v_locked_load.borrow_mut() = locked;

                if locked && !is_amd {
                    warn_title_clone.set_label(i18n::t("uv_unsupported_title"));
                    warn_desc_clone.set_label(&i18n::t("uv_unsupported_desc").replace("{}", &cpu_spec_str));
                    badge_load.set_label("Locked");
                    badge_load.set_css_classes(&["badge-err"]);
                    volt_group_clone.set_sensitive(false);
                } else if is_amd {
                    warn_title_clone.set_label("AMD Curve Optimizer & PBO");
                    if let Some(w) = state.get("warning").and_then(|v| v.as_str()) {
                        warn_desc_clone.set_label(w);
                        badge_load.set_label("RyzenAdj Missing");
                        badge_load.set_css_classes(&["badge-err"]);
                    } else {
                        warn_desc_clone.set_label("RyzenAdj is managing Curve Optimizer and Power Limits.");
                        badge_load.set_label("RyzenAdj OK");
                        badge_load.set_css_classes(&["badge-ok"]);
                    }
                    core_scale_range.set_range(-30.0, 30.0);
                    cache_row_clone.set_visible(false);
                    core_row_clone.set_title("Curve Optimizer (All Core)");
                    core_row_clone.set_subtitle("Negative values = Undervolt. Range: -30 to +30.");
                } else {
                    badge_load.set_label("MSR OK");
                    badge_load.set_css_classes(&["badge-ok"]);
                }

                if let Some(tj) = state.get("tj_max").and_then(|v| v.as_i64()) {
                    *tj_max_load.borrow_mut() = tj as i32;
                }
                let tcc_locked = state.get("tcc_locked").and_then(|v| v.as_bool()).unwrap_or(false);
                *tcc_locked_load.borrow_mut() = tcc_locked;

                if let Some(core) = state.get("offsets").and_then(|v| v.get("core")).and_then(|v| v.as_f64())
                    .or_else(|| state.get("core").and_then(|v| v.as_f64()))
                {
                    core_s_load.set_value(core);
                    *loaded_core_load.borrow_mut() = core as i32;
                }
                
                if let Some(cache) = state.get("offsets").and_then(|v| v.get("cache")).and_then(|v| v.as_f64())
                    .or_else(|| state.get("cache").and_then(|v| v.as_f64()))
                {
                    cache_s_load.set_value(cache);
                    *loaded_cache_load.borrow_mut() = cache as i32;
                }

                let effective_tcc = state
                    .get("hw_tcc_offset")
                    .and_then(|v| v.as_f64())
                    .or_else(|| state.get("tcc_offset").and_then(|v| v.as_f64()))
                    .unwrap_or(0.0);
                let saved_tcc = state.get("saved_tcc_offset").and_then(|v| v.as_i64());

                tcc_s_load.set_value(effective_tcc);
                *loaded_tcc_load.borrow_mut() = effective_tcc as i32;

                let tj = *tj_max_load.borrow();
                let target_temp = tj - (effective_tcc as i32);
                if tcc_locked {
                    tcc_s_load.set_sensitive(false);
                    tcc_lbl_load.set_label(&format!(
                        "{}°C ({}: {}°C · Locked)",
                        target_temp,
                        i18n::t("tcc_offset_str"),
                        effective_tcc as i32
                    ));
                    let lock_note = if let Some(saved) = saved_tcc {
                        if saved != effective_tcc as i64 {
                            format!(
                                "TjMax: {}°C · Hardware TCC offset is locked at {}°C by firmware (effective target: {}°C; saved config: {}°C).",
                                tj, effective_tcc as i32, target_temp, saved
                            )
                        } else {
                            format!(
                                "TjMax: {}°C · Hardware TCC offset is locked at {}°C by firmware (effective target: {}°C).",
                                tj, effective_tcc as i32, target_temp
                            )
                        }
                    } else {
                        format!(
                            "TjMax: {}°C · Hardware TCC offset is locked at {}°C by firmware (effective target: {}°C).",
                            tj, effective_tcc as i32, target_temp
                        )
                    };
                    tcc_row_load.set_subtitle(&lock_note);
                } else {
                    tcc_lbl_load.set_label(&format!(
                        "{}°C ({}: {}°C)",
                        target_temp,
                        i18n::t("tcc_offset_str"),
                        effective_tcc as i32
                    ));
                    if tj != 100 {
                        tcc_row_load.set_subtitle(&format!(
                            "{} (TjMax: {}°C)",
                            i18n::t("tcc_sub"),
                            tj
                        ));
                    }
                }
            }
        }
        
        // Load Power Limits separately
        if let Ok(json) = crate::daemon_client::get_power_profile_raw_async().await {
            if let Ok(state) = serde_json::from_str::<serde_json::Value>(&json) {
                reset_btn_load.set_sensitive(state.get("baseline_available").and_then(|v|v.as_bool()).unwrap_or(false));
                if let Some(boot_pl1) = state.get("boot_pl1_w").and_then(|v| v.as_f64()) {
                    *def_pl1_load.borrow_mut() = boot_pl1;
                    pl1_row_load.set_subtitle(&format!("{} (Initially observed: {:.0} W)", i18n::t("pl1_sub"), boot_pl1));
                }
                if let Some(boot_pl2) = state.get("boot_pl2_w").and_then(|v| v.as_f64()) {
                    *def_pl2_load.borrow_mut() = boot_pl2;
                    pl2_row_load.set_subtitle(&format!("{} (Initially observed: {:.0} W)", i18n::t("pl2_sub"), boot_pl2));
                }
                if let Some(pl1) = state.get("pl1_w").and_then(|v| v.as_f64()) {
                    pl1_s_load.set_value(pl1);
                }
                if let Some(pl2) = state.get("pl2_w").and_then(|v| v.as_f64()) {
                    pl2_s_load.set_value(pl2);
                }
            }
        }
    });

    page
}
