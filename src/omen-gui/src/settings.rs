use gtk::prelude::*;
use libadwaita as adw;
use adw::prelude::*;
use std::rc::Rc;
use crate::i18n::{self, Language};

/* ─────────────────────────────────────────────────────────────
   settings.rs — Clean application & daemon configuration
   (All diagnostics, triage, and hardware testing live in Debug Center)
   ───────────────────────────────────────────────────────────── */

pub fn build_page(
    _window: &adw::ApplicationWindow,
    on_lang_changed: Option<Rc<dyn Fn()>>,
    lb_group_opt: Option<adw::PreferencesGroup>,
    lb_preview_group_opt: Option<adw::PreferencesGroup>,
) -> gtk::Box {
    let mut init_hb = 30.0;
    let mut init_auto = true;
    let mut init_startup_profile = 0u32;
    let mut init_battery_care = false;
    let mut init_thermal_alerts = true;
    let mut init_thermal_prot = true;
    let mut init_zone_override = 0u32;
    let mut init_appearance_mode = 0u32;
    let mut init_lightbar = true;

    let specs = crate::daemon_client::get_hardware_specs_sync();

    if let Ok(home) = std::env::var("HOME") {
        let path = format!("{}/.config/omenspace/settings.json", home);
        if let Ok(json_str) = std::fs::read_to_string(&path) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&json_str) {
                if let Some(hb) = json.get("heartbeat_interval").and_then(|v| v.as_f64()) { init_hb = hb; }
                if let Some(auto) = json.get("autostart").and_then(|v| v.as_bool()) { init_auto = auto; }
                if let Some(sp) = json.get("startup_profile").and_then(|v| v.as_u64()) { init_startup_profile = sp as u32; }
                if let Some(bc) = json.get("battery_care").and_then(|v| v.as_bool()) { init_battery_care = bc; }
                if let Some(ta) = json.get("thermal_alerts").and_then(|v| v.as_bool()) { init_thermal_alerts = ta; }
                if let Some(tp) = json.get("thermal_protection").and_then(|v| v.as_bool()) { init_thermal_prot = tp; }
                if let Some(zo) = json.get("zone_override").and_then(|v| v.as_u64()) { init_zone_override = zo as u32; }
                if let Some(am) = json.get("appearance_mode").and_then(|v| v.as_u64()) { init_appearance_mode = am as u32; }
                if let Some(lb) = json.get("lightbar_enabled").and_then(|v| v.as_bool()) { init_lightbar = lb; }
            }
        }
    }

    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(20)
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
    perf_group.add(&battery_row);

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

    // ── 4. Daemon & Service ───────────────────────────────────
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
    let thm_row_clone = thermal_row.clone();
    let thm_prot_clone = thermal_prot_row.clone();
    let zone_row_clone = zone_override_row.clone();
    let lb_row_clone = lightbar_row.clone();

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
            json["thermal_alerts"] = serde_json::json!(ta);
            json["thermal_protection"] = serde_json::json!(tp);
            json["zone_override"] = serde_json::json!(zo);
            json["lightbar_enabled"] = serde_json::json!(lb);
            let _ = std::fs::write(path, serde_json::to_string_pretty(&json).unwrap_or_default());
        }
    };

    let save_settings_rc = Rc::new(save_settings);
    let s1 = save_settings_rc.clone();
    hb_spin.connect_value_changed(move |_| s1());
    let s2 = save_settings_rc.clone();
    autostart_row.connect_active_notify(move |_| s2());
    let s3 = save_settings_rc.clone();
    startup_mode_row.connect_selected_notify(move |_| s3());

    let s4 = save_settings_rc.clone();
    battery_row.connect_active_notify(move |r| {
        s4();
        let limit = if r.is_active() { 80 } else { 100 };
        crate::daemon_client::set_battery_care_sync(limit);
    });

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
