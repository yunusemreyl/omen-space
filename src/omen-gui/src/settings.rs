use gtk::prelude::*;
use libadwaita as adw;
use adw::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use gtk::gdk;
use omen_types::hotkey::{HotkeyModifiers, HotkeySpec};
use crate::i18n::{self, Language};

/* ─────────────────────────────────────────────────────────────
   settings.rs — Clean application & daemon configuration
   (All diagnostics, triage, and hardware testing live in Debug Center)
   ───────────────────────────────────────────────────────────── */

pub fn build_page(
    window: &adw::ApplicationWindow,
    on_lang_changed: Option<Rc<dyn Fn()>>,
    lb_group_opt: Option<adw::PreferencesGroup>,
    lb_preview_group_opt: Option<adw::PreferencesGroup>,
) -> gtk::Box {
    let mut init_hb = 30.0;
    let mut init_auto = true;
    let mut init_startup_profile = 0u32;
    let mut init_battery_care = false;
    let mut init_battery_stop = 80.0;
    let mut init_battery_start = 75.0;
    let mut init_battery_off_hold = true;
    let mut init_thermal_alerts = true;
    let mut init_thermal_prot = true;
    let mut init_zone_override = 0u32;
    let mut init_appearance_mode = 0u32;
    let mut init_lightbar = true;
    let mut init_hotkey = HotkeySpec::default();

    let specs = crate::daemon_client::get_hardware_specs_sync();

    if let Ok(home) = std::env::var("HOME") {
        let path = format!("{}/.config/omenspace/settings.json", home);
        if let Ok(json_str) = std::fs::read_to_string(&path) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&json_str) {
                if let Some(hb) = json.get("heartbeat_interval").and_then(|v| v.as_f64()) { init_hb = hb; }
                if let Some(auto) = json.get("autostart").and_then(|v| v.as_bool()) { init_auto = auto; }
                if let Some(sp) = json.get("startup_profile").and_then(|v| v.as_u64()) { init_startup_profile = sp as u32; }
                if let Some(bc) = json.get("battery_care").and_then(|v| v.as_bool()) { init_battery_care = bc; }
                if let Some(bs) = json.get("battery_stop").and_then(|v| v.as_f64()) { init_battery_stop = bs.clamp(50.0, 95.0); }
                if let Some(br) = json.get("battery_start").and_then(|v| v.as_f64()) { init_battery_start = br.clamp(20.0, 90.0); }
                if let Some(bh) = json.get("battery_off_hold").and_then(|v| v.as_bool()) { init_battery_off_hold = bh; }
                if let Some(ta) = json.get("thermal_alerts").and_then(|v| v.as_bool()) { init_thermal_alerts = ta; }
                if let Some(tp) = json.get("thermal_protection").and_then(|v| v.as_bool()) { init_thermal_prot = tp; }
                if let Some(zo) = json.get("zone_override").and_then(|v| v.as_u64()) { init_zone_override = zo as u32; }
                if let Some(am) = json.get("appearance_mode").and_then(|v| v.as_u64()) { init_appearance_mode = am as u32; }
                if let Some(lb) = json.get("lightbar_enabled").and_then(|v| v.as_bool()) { init_lightbar = lb; }
                init_hotkey = HotkeySpec::from_settings(&json);
            }
        }
    }

    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .build();

    // ── Header ────────────────────────────────────────────────
    let hdr = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_bottom(4)
        .build();
    hdr.append(&gtk::Label::builder()
        .label(i18n::t("title_settings"))
        .css_classes(["page-title"])
        .halign(gtk::Align::Start)
        .build());
    hdr.append(&gtk::Label::builder()
        .label(i18n::t("settings_desc"))
        .css_classes(["os-section-desc"])
        .halign(gtk::Align::Start)
        .build());
    page.append(&hdr);

    // ── 1. Appearance & Language ──────────────────────────────
    let app_lang_group = adw::PreferencesGroup::builder()
        .title(i18n::t("appearance_and_lang"))
        .build();

    let appearance_model = gtk::StringList::new(&[
        i18n::t("theme_auto"),
        i18n::t("theme_light"),
        i18n::t("theme_dark"),
    ]);
    let appearance_row = adw::ComboRow::builder()
        .title(i18n::t("appearance_mode"))
        .subtitle(i18n::t("appearance_mode_sub"))
        .model(&appearance_model)
        .selected(init_appearance_mode)
        .build();

    appearance_row.connect_selected_notify(|row| {
        let idx = row.selected();
        let scheme = match idx {
            1 => adw::ColorScheme::ForceLight,
            2 => adw::ColorScheme::ForceDark,
            _ => adw::ColorScheme::Default,
        };
        adw::StyleManager::default().set_color_scheme(scheme);

        if let Ok(home) = std::env::var("HOME") {
            let dir = format!("{}/.config/omenspace", home);
            let _ = std::fs::create_dir_all(&dir);
            let path = format!("{}/settings.json", dir);
            let mut json = serde_json::json!({});
            if let Ok(js) = std::fs::read_to_string(&path) {
                if let Ok(j) = serde_json::from_str::<serde_json::Value>(&js) { json = j; }
            }
            json["appearance_mode"] = serde_json::json!(idx);
            let _ = std::fs::write(&path, serde_json::to_string_pretty(&json).unwrap_or_default());
        }
    });
    app_lang_group.add(&appearance_row);

    let scale_row = adw::ActionRow::builder()
        .title(i18n::t("ui_scale"))
        .subtitle(i18n::t("ui_scale_sub"))
        .build();
    let scale_spin = gtk::SpinButton::with_range(
        crate::ui_scale::MIN_SCALE * 100.0,
        crate::ui_scale::MAX_SCALE * 100.0,
        crate::ui_scale::SCALE_STEP * 100.0,
    );
    scale_spin.set_valign(gtk::Align::Center);
    crate::ui_scale::bind_spin(&scale_spin);
    scale_spin.connect_value_changed(|spin| {
        if crate::ui_scale::is_syncing_spin() {
            return;
        }
        crate::ui_scale::apply(spin.value() / 100.0);
    });
    scale_row.add_suffix(&scale_spin);
    app_lang_group.add(&scale_row);

    let lang_row = adw::ComboRow::builder()
        .title(i18n::t("language_row_title"))
        .subtitle(i18n::t("language_row_sub"))
        .build();
    let lang_model = gtk::StringList::new(&[
        Language::Auto.display_name(),
        Language::Tr.display_name(),
        Language::En.display_name(),
        Language::Es.display_name(),
    ]);
    lang_row.set_model(Some(&lang_model));
    lang_row.set_selected(i18n::get_selected_language().to_index());

    let on_lang_changed_clone = on_lang_changed.clone();
    let on_lang_changed_clone_zone = on_lang_changed.clone();
    lang_row.connect_selected_notify(move |row| {
        let idx = row.selected();
        let selected_lang = Language::from_index(idx);
        if selected_lang != i18n::get_selected_language() {
            i18n::set_language(selected_lang);
            if let Some(cb) = &on_lang_changed_clone {
                cb();
            }
        }
    });
    app_lang_group.add(&lang_row);
    page.append(&app_lang_group);

    // ── 2. Hardware Configuration ─────────────────────────────
    let hw_group = adw::PreferencesGroup::builder()
        .title(i18n::t("hw_config_group"))
        .build();

    let zone_override_model = gtk::StringList::new(&[
        i18n::t("auto_detect_recommended"),
        i18n::t("zone_4zone"),
        i18n::t("zone_single"),
        i18n::t("zone_perkey"),
        i18n::t("zone_desktop"),
    ]);
    let zone_override_row = adw::ComboRow::builder()
        .title(i18n::t("zone_override"))
        .subtitle(i18n::t("zone_override_sub"))
        .model(&zone_override_model)
        .selected(init_zone_override)
        .build();
    hw_group.add(&zone_override_row);

    let lightbar_row = adw::SwitchRow::builder()
        .title(i18n::t("lightbar_wmi_toggle"))
        .subtitle(i18n::t("lightbar_wmi_toggle_sub"))
        .build();
    lightbar_row.set_active(init_lightbar);
    hw_group.add(&lightbar_row);

    page.append(&hw_group);

    // ── 3. Performance & Thermal Behavior ─────────────────────
    let perf_group = adw::PreferencesGroup::builder()
        .title(i18n::t("perf_behavior"))
        .build();

    let startup_mode_row = adw::ComboRow::builder()
        .title(i18n::t("startup_profile"))
        .subtitle(i18n::t("startup_profile_sub"))
        .build();
    let mode_model = gtk::StringList::new(&[
        i18n::t("last_used"),
        i18n::t("mode_eco"),
        i18n::t("mode_balanced"),
        i18n::t("mode_performance"),
    ]);
    startup_mode_row.set_model(Some(&mode_model));
    startup_mode_row.set_selected(init_startup_profile);
    perf_group.add(&startup_mode_row);

    let thermal_row = adw::SwitchRow::builder()
        .title(i18n::t("thermal_alerts"))
        .subtitle(i18n::t("thermal_alerts_sub"))
        .build();
    thermal_row.set_active(init_thermal_alerts);
    perf_group.add(&thermal_row);

    let thermal_prot_row = adw::SwitchRow::builder()
        .title(i18n::t("thermal_prot_title"))
        .subtitle(i18n::t("thermal_prot_sub"))
        .build();
    thermal_prot_row.set_active(init_thermal_prot);
    perf_group.add(&thermal_prot_row);

    page.append(&perf_group);

    // ── 3b. Battery Charge Limiter ────────────────────────────
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

    // ── 4. Quick HUD Hotkey ───────────────────────────────────
    let hotkey_spec = Rc::new(RefCell::new(init_hotkey));

    let hotkey_group = adw::PreferencesGroup::builder()
        .title(i18n::t("overlay_hotkey_group"))
        .build();

    let hotkey_row = adw::ActionRow::builder()
        .title(i18n::t("overlay_hotkey_title"))
        .subtitle(i18n::t("overlay_hotkey_sub"))
        .build();
    let hotkey_value = gtk::Label::builder()
        .label(hotkey_spec.borrow().display())
        .css_classes(["os-section-desc"])
        .valign(gtk::Align::Center)
        .build();
    hotkey_row.add_suffix(&hotkey_value);
    let record_btn = gtk::Button::builder()
        .label(i18n::t("overlay_hotkey_record_btn"))
        .valign(gtk::Align::Center)
        .build();
    hotkey_row.add_suffix(&record_btn);
    hotkey_group.add(&hotkey_row);

    let reset_row = adw::ActionRow::builder()
        .title(i18n::t("overlay_hotkey_reset"))
        .build();
    let reset_btn = gtk::Button::builder()
        .icon_name("edit-undo-symbolic")
        .tooltip_text(i18n::t("overlay_hotkey_reset"))
        .valign(gtk::Align::Center)
        .build();
    reset_row.add_suffix(&reset_btn);
    hotkey_group.add(&reset_row);
    page.append(&hotkey_group);

    // ── 5. Daemon & Service ───────────────────────────────────
    let daemon_alive = crate::daemon_client::ping_daemon_sync();

    let daemon_group = adw::PreferencesGroup::builder()
        .title(i18n::t("daemon_group"))
        .build();

    let daemon_row = adw::ActionRow::builder()
        .title(i18n::t("daemon_status"))
        .subtitle(i18n::t("daemon_status_sub"))
        .build();
    let status_badge = gtk::Label::builder()
        .label(if daemon_alive { i18n::t("connected") } else { i18n::t("disconnected") })
        .css_classes(if daemon_alive { ["badge-ok"] } else { ["badge-warn"] })
        .valign(gtk::Align::Center)
        .build();
    daemon_row.add_suffix(&status_badge);
    daemon_group.add(&daemon_row);

    let heartbeat_row = adw::ActionRow::builder()
        .title(i18n::t("heartbeat_interval"))
        .subtitle(i18n::t("heartbeat_sub"))
        .build();
    let hb_spin = gtk::SpinButton::with_range(5.0, 120.0, 5.0);
    hb_spin.set_value(init_hb);
    hb_spin.set_valign(gtk::Align::Center);
    heartbeat_row.add_suffix(&hb_spin);
    daemon_group.add(&heartbeat_row);

    {
        let badge_sub = status_badge.clone();
        crate::daemon_client::subscribe_daemon_status(move |status| {
            use crate::daemon_client::DaemonStatus;
            let alive = status == DaemonStatus::Online;
            badge_sub.set_label(if alive { i18n::t("connected") } else { i18n::t("disconnected") });
            badge_sub.set_css_classes(if alive { &["badge-ok"] } else { &["badge-warn"] });
        });

        let badge_hb = status_badge.clone();
        let hb_spin_poll = hb_spin.clone();
        glib::spawn_future_local(async move {
            loop {
                let secs = hb_spin_poll.value().clamp(5.0, 120.0) as u64;
                glib::timeout_future_seconds(secs as u32).await;
                let (tx, rx) = tokio::sync::oneshot::channel();
                crate::daemon_client::get_runtime().spawn(async move {
                    let alive = crate::daemon_client::ping_daemon_sync();
                    let _ = tx.send(alive);
                });
                if let Ok(alive) = rx.await {
                    badge_hb.set_label(if alive { i18n::t("connected") } else { i18n::t("disconnected") });
                    badge_hb.set_css_classes(if alive { &["badge-ok"] } else { &["badge-warn"] });
                }
            }
        });
    }

    let autostart_row = adw::SwitchRow::builder()
        .title(i18n::t("autostart"))
        .subtitle(i18n::t("autostart_sub"))
        .build();
    autostart_row.set_active(init_auto);
    daemon_group.add(&autostart_row);

    page.append(&daemon_group);

    // ── Settings Persistence Wiring ───────────────────────────
    let hb_spin_clone = hb_spin.clone();
    let as_row_clone = autostart_row.clone();
    let sm_row_clone = startup_mode_row.clone();
    let bat_row_clone = battery_row.clone();
    let bat_stop_clone = stop_spin.clone();
    let bat_start_clone = start_spin.clone();
    let bat_hold_clone = off_hold_row.clone();
    let thm_row_clone = thermal_row.clone();
    let thm_prot_clone = thermal_prot_row.clone();
    let zone_row_clone = zone_override_row.clone();
    let lb_row_clone = lightbar_row.clone();
    let hk_spec_clone = hotkey_spec.clone();

    let save_settings = move || {
        let hb = hb_spin_clone.value();
        let auto = as_row_clone.is_active();
        let sp = sm_row_clone.selected();
        let bc = bat_row_clone.is_active();
        let ta = thm_row_clone.is_active();
        let tp = thm_prot_clone.is_active();
        let zo = zone_row_clone.selected();
        let lb = lb_row_clone.is_active();
        if let Ok(home) = std::env::var("HOME") {
            let dir = format!("{}/.config/omenspace", home);
            let _ = std::fs::create_dir_all(&dir);
            let path = format!("{}/settings.json", dir);
            let mut json = serde_json::json!({});
            if let Ok(js) = std::fs::read_to_string(&path) {
                if let Ok(j) = serde_json::from_str::<serde_json::Value>(&js) { json = j; }
            }
            json["heartbeat_interval"] = serde_json::json!(hb);
            json["autostart"] = serde_json::json!(auto);
            json["startup_profile"] = serde_json::json!(sp);
            json["battery_care"] = serde_json::json!(bc);
            json["battery_stop"] = serde_json::json!(bat_stop_clone.value());
            json["battery_start"] = serde_json::json!(bat_start_clone.value());
            json["battery_off_hold"] = serde_json::json!(bat_hold_clone.is_active());
            json["thermal_alerts"] = serde_json::json!(ta);
            json["thermal_protection"] = serde_json::json!(tp);
            json["zone_override"] = serde_json::json!(zo);
            json["lightbar_enabled"] = serde_json::json!(lb);
            hk_spec_clone.borrow().write_settings(&mut json);
            let _ = std::fs::write(path, serde_json::to_string_pretty(&json).unwrap_or_default());
        }
    };

    let save_settings_rc = Rc::new(save_settings);

    // Hotkey recording / reset. The daemon picks the new value up from
    // settings.json within about a second (no restart needed).
    {
        let spec = hotkey_spec.clone();
        let value_label = hotkey_value.clone();
        let save = save_settings_rc.clone();
        let parent = window.clone();
        record_btn.connect_clicked(move |_| {
            let current = spec.borrow().display();
            let spec_cb = spec.clone();
            let value_cb = value_label.clone();
            let save_cb = save.clone();
            open_hotkey_dialog(&parent, &current, Rc::new(move |new_spec| {
                *spec_cb.borrow_mut() = new_spec;
                value_cb.set_label(&spec_cb.borrow().display());
                save_cb();
            }));
        });
    }
    {
        let spec = hotkey_spec.clone();
        let value_label = hotkey_value.clone();
        let save = save_settings_rc.clone();
        reset_btn.connect_clicked(move |_| {
            *spec.borrow_mut() = HotkeySpec::default();
            value_label.set_label(&spec.borrow().display());
            save();
        });
    }

    let s1 = save_settings_rc.clone();
    hb_spin.connect_value_changed(move |_| s1());
    let s2 = save_settings_rc.clone();
    autostart_row.connect_active_notify(move |_| s2());
    let s3 = save_settings_rc.clone();
    startup_mode_row.connect_selected_notify(move |_| s3());

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
        let save = save_settings_rc.clone();
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
            save();
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

    let s5 = save_settings_rc.clone();
    thermal_row.connect_active_notify(move |_| {
        s5();
    });

    let syncing_thermal_prot = Rc::new(std::cell::Cell::new(false));
    let s5_prot = save_settings_rc.clone();
    let syncing_tp_notify = syncing_thermal_prot.clone();
    thermal_prot_row.connect_active_notify(move |r| {
        s5_prot();
        if !syncing_tp_notify.get() {
            crate::daemon_client::set_thermal_protection_sync(r.is_active());
        }
    });

    let tp_row_sync = thermal_prot_row.clone();
    let syncing_tp_load = syncing_thermal_prot.clone();
    glib::spawn_future_local(async move {
        if let Ok(fan_json) = crate::daemon_client::get_fan_info_async().await {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&fan_json) {
                if let Some(enabled) = val.get("thermal_protection_enabled").and_then(|v| v.as_bool()) {
                    if tp_row_sync.is_active() != enabled {
                        syncing_tp_load.set(true);
                        tp_row_sync.set_active(enabled);
                        syncing_tp_load.set(false);
                    }
                }
            }
        }
    });

    let s6 = save_settings_rc.clone();
    zone_override_row.connect_selected_notify(move |_| {
        s6();
        if let Some(cb) = &on_lang_changed_clone_zone {
            cb();
        }
    });

    let s7 = save_settings_rc.clone();
    lightbar_row.connect_active_notify(move |r| {
        s7();
        let is_active = r.is_active();
        if let Some(grp) = &lb_group_opt {
            grp.set_visible(is_active);
        }
        if let Some(pgrp) = &lb_preview_group_opt {
            pgrp.set_visible(is_active);
        }
    });

    // ── 5. About OmenSpace ────────────────────────────────────
    let about_group = adw::PreferencesGroup::builder()
        .title(i18n::t("about_group"))
        .build();

    let version_str = format!("v{}", env!("CARGO_PKG_VERSION"));
    let kernel_str = format!("Linux {}", specs.kernel_version);

    let about_items = [
        (i18n::t("version"), version_str.as_str()),
        (i18n::t("device"), specs.product_name.as_str()),
        (i18n::t("kernel"), kernel_str.as_str()),
        (i18n::t("daemon_socket"), "D-Bus: org.hp.omen (System Bus)"),
    ];

    for (title, val) in about_items {
        let row = adw::ActionRow::builder().title(title).build();
        row.add_suffix(&gtk::Label::builder()
            .label(val)
            .css_classes(["os-section-desc"])
            .valign(gtk::Align::Center)
            .build());
        about_group.add(&row);
    }
    page.append(&about_group);

    page
}

/// Modal recorder: the next non-modifier key press becomes the new hotkey.
fn open_hotkey_dialog(
    parent: &adw::ApplicationWindow,
    current: &str,
    on_save: Rc<dyn Fn(HotkeySpec)>,
) {
    let dialog = adw::Window::builder()
        .title(i18n::t("overlay_hotkey_record_btn"))
        .transient_for(parent)
        .modal(true)
        .default_width(420)
        .build();

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();

    content.append(&gtk::Label::builder()
        .label(i18n::t("overlay_hotkey_recording"))
        .css_classes(["title-3"])
        .build());

    let combo = gtk::Label::builder()
        .label(current)
        .css_classes(["title-1"])
        .build();
    content.append(&combo);

    let unsupported = gtk::Label::builder()
        .label(i18n::t("overlay_hotkey_unsupported"))
        .css_classes(["error"])
        .wrap(true)
        .visible(false)
        .build();
    content.append(&unsupported);

    let buttons = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::Center)
        .build();
    let reset_btn = gtk::Button::with_label(i18n::t("overlay_hotkey_reset"));
    let cancel_btn = gtk::Button::with_label(i18n::t("cancel"));
    buttons.append(&reset_btn);
    buttons.append(&cancel_btn);
    content.append(&buttons);

    dialog.set_content(Some(&content));
    content.set_focusable(true);

    let controller = gtk::EventControllerKey::new();
    controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let dialog_c = dialog.clone();
        let combo_c = combo.clone();
        let unsupported_c = unsupported.clone();
        let on_save_c = on_save.clone();
        controller.connect_key_pressed(move |_, keyval, keycode, state| {
            if keyval == gdk::Key::Escape {
                dialog_c.close();
                return glib::Propagation::Stop;
            }

            unsupported_c.set_visible(false);

            // Ignore bare modifier presses but show what is being held.
            if is_modifier_key(keyval) {
                combo_c.set_label(&modifier_preview(state));
                return glib::Propagation::Stop;
            }

            // GDK reports X11/xkb key codes; evdev codes are offset by 8.
            let Some(code) = keycode.checked_sub(8).and_then(|c| u16::try_from(c).ok()) else {
                unsupported_c.set_label(i18n::t("overlay_hotkey_unsupported"));
                unsupported_c.set_visible(true);
                return glib::Propagation::Stop;
            };
            if code == 0 || omen_types::hotkey::is_modifier_code(code) {
                unsupported_c.set_label(i18n::t("overlay_hotkey_unsupported"));
                unsupported_c.set_visible(true);
                return glib::Propagation::Stop;
            }

            let modifiers = HotkeyModifiers {
                ctrl: state.contains(gdk::ModifierType::CONTROL_MASK),
                alt: state.contains(gdk::ModifierType::ALT_MASK),
                shift: state.contains(gdk::ModifierType::SHIFT_MASK),
                // Not META_MASK: GTK often reports it alongside ALT_MASK for
                // plain Alt, which would record Alt+X as Alt+Super+X.
                meta: state.contains(gdk::ModifierType::SUPER_MASK),
            };
            // Prefer the unshifted physical key name ("1", not "exclam").
            let label = omen_types::hotkey::evdev_key_name(code)
                .map(str::to_string)
                .unwrap_or_else(|| prettify_key_label(keyval));
            if modifiers.to_bits() == 0 && !omen_types::hotkey::allows_no_modifier(code) {
                unsupported_c.set_label(i18n::t("overlay_hotkey_needs_modifier"));
                unsupported_c.set_visible(true);
                return glib::Propagation::Stop;
            }
            on_save_c(HotkeySpec::with_label(code, modifiers, label));
            dialog_c.close();
            glib::Propagation::Stop
        });
    }
    dialog.add_controller(controller);

    {
        let dialog_c = dialog.clone();
        let on_save_c = on_save.clone();
        reset_btn.connect_clicked(move |_| {
            on_save_c(HotkeySpec::default());
            dialog_c.close();
        });
    }
    {
        let dialog_c = dialog.clone();
        cancel_btn.connect_clicked(move |_| dialog_c.close());
    }

    dialog.present();
    content.grab_focus();
}

fn is_modifier_key(key: gdk::Key) -> bool {
    matches!(
        key,
        gdk::Key::Shift_L
            | gdk::Key::Shift_R
            | gdk::Key::Control_L
            | gdk::Key::Control_R
            | gdk::Key::Alt_L
            | gdk::Key::Alt_R
            | gdk::Key::Super_L
            | gdk::Key::Super_R
            | gdk::Key::Meta_L
            | gdk::Key::Meta_R
            | gdk::Key::Hyper_L
            | gdk::Key::Hyper_R
            | gdk::Key::Caps_Lock
            | gdk::Key::Num_Lock
            | gdk::Key::ISO_Level3_Shift
            | gdk::Key::ISO_Level5_Shift
            | gdk::Key::Mode_switch
    )
}

fn modifier_preview(state: gdk::ModifierType) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if state.contains(gdk::ModifierType::CONTROL_MASK) {
        parts.push("Ctrl");
    }
    if state.contains(gdk::ModifierType::ALT_MASK) {
        parts.push("Alt");
    }
    if state.contains(gdk::ModifierType::SHIFT_MASK) {
        parts.push("Shift");
    }
    if state.contains(gdk::ModifierType::SUPER_MASK) {
        parts.push("Super");
    }
    if parts.is_empty() {
        "...".to_string()
    } else {
        format!("{}+...", parts.join("+"))
    }
}

/// Turn a GTK keysym into the label shown in the overlay/settings.
fn prettify_key_label(keyval: gdk::Key) -> String {
    let name = keyval.name().map(|s| s.to_string()).unwrap_or_default();
    match name.as_str() {
        "space" => "Space".to_string(),
        "Return" | "KP_Enter" => "Enter".to_string(),
        "BackSpace" => "Backspace".to_string(),
        "Escape" => "Esc".to_string(),
        "Page_Up" => "PageUp".to_string(),
        "Page_Down" => "PageDown".to_string(),
        "Caps_Lock" => "CapsLock".to_string(),
        "Num_Lock" => "NumLock".to_string(),
        "Scroll_Lock" => "ScrollLock".to_string(),
        "Print" => "PrintScreen".to_string(),
        "XF86AudioMute" => "Mute".to_string(),
        "XF86AudioLowerVolume" => "VolumeDown".to_string(),
        "XF86AudioRaiseVolume" => "VolumeUp".to_string(),
        "XF86MonBrightnessDown" => "BrightnessDown".to_string(),
        "XF86MonBrightnessUp" => "BrightnessUp".to_string(),
        _ if name.len() == 1 => name.to_uppercase(),
        _ if name.starts_with("XF86") => name.trim_start_matches("XF86").to_string(),
        _ => name,
    }
}
