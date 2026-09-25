use gtk::prelude::*;
use libadwaita as adw;
use adw::prelude::*;
use std::rc::Rc;
use crate::i18n::{self, Language};

/* ─────────────────────────────────────────────────────────────
   settings.rs — Application & daemon settings
   ───────────────────────────────────────────────────────────── */

pub fn build_page(window: &adw::ApplicationWindow, on_lang_changed: Option<Rc<dyn Fn()>>, lb_group_opt: Option<adw::PreferencesGroup>, lb_preview_group_opt: Option<adw::PreferencesGroup>) -> gtk::Box {
    let mut init_hb = 30.0;
    let mut init_auto = true;
    let mut init_startup_profile = 0u32;
    let mut init_battery_care = false;
    let mut init_thermal_alerts = true;
    let mut init_zone_override = 0u32;
    let mut init_appearance_mode = 0u32;
    
    let specs = crate::daemon_client::get_hardware_specs_sync();
    let _prod_lower = specs.product_name.to_lowercase();
    let mut init_lightbar = true; // Always show by default so users see "Unsupported" explicitly

    if let Ok(home) = std::env::var("HOME") {
        let path = format!("{}/.config/omenspace/settings.json", home);
        if let Ok(json_str) = std::fs::read_to_string(&path) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&json_str) {
                if let Some(hb) = json.get("heartbeat_interval").and_then(|v| v.as_f64()) { init_hb = hb; }
                if let Some(auto) = json.get("autostart").and_then(|v| v.as_bool()) { init_auto = auto; }
                if let Some(sp) = json.get("startup_profile").and_then(|v| v.as_u64()) { init_startup_profile = sp as u32; }
                if let Some(bc) = json.get("battery_care").and_then(|v| v.as_bool()) { init_battery_care = bc; }
                if let Some(ta) = json.get("thermal_alerts").and_then(|v| v.as_bool()) { init_thermal_alerts = ta; }
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

    // Header
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

    // ── Hardware Config group ─────────────────────────────────
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

    // ── Appearance & Language group ───────────────────────────
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
            let path = format!("{}/.config/omenspace/settings.json", home);
            let mut json = serde_json::json!({});
            if let Ok(js) = std::fs::read_to_string(&path) {
                if let Ok(j) = serde_json::from_str::<serde_json::Value>(&js) { json = j; }
            }
            json["appearance_mode"] = serde_json::json!(idx);
            let _ = std::fs::write(&path, serde_json::to_string_pretty(&json).unwrap_or_default());
        }
    });
    app_lang_group.add(&appearance_row);

    let lang_row = adw::ComboRow::builder()
        .title(i18n::t("language_row_title"))
        .subtitle(i18n::t("language_row_sub"))
        .build();
    let lang_model = gtk::StringList::new(&[
        Language::Auto.display_name(),
        Language::Tr.display_name(),
        Language::En.display_name(),
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

    // ── Daemon group ──────────────────────────────────────────
    // Perform a real D-Bus ping to verify daemon is alive
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

    let autostart_row = adw::SwitchRow::builder()
        .title(i18n::t("autostart"))
        .subtitle(i18n::t("autostart_sub"))
        .build();
    autostart_row.set_active(init_auto);
    daemon_group.add(&autostart_row);

    page.append(&daemon_group);

    // ── Performance group ─────────────────────────────────────
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

    let battery_row = adw::SwitchRow::builder()
        .title(i18n::t("battery_care"))
        .subtitle(i18n::t("battery_care_sub"))
        .build();
    battery_row.set_active(init_battery_care);
    perf_group.add(&battery_row);

    let thermal_row = adw::SwitchRow::builder()
        .title(i18n::t("thermal_alerts"))
        .subtitle(i18n::t("thermal_alerts_sub"))
        .build();
    thermal_row.set_active(init_thermal_alerts);
    perf_group.add(&thermal_row);

    page.append(&perf_group);


    // Save logic for all
    let hb_spin_clone = hb_spin.clone();
    let as_row_clone = autostart_row.clone();
    let sm_row_clone = startup_mode_row.clone();
    let bat_row_clone = battery_row.clone();
    let thm_row_clone = thermal_row.clone();
    let zone_row_clone = zone_override_row.clone();
    let lb_row_clone = lightbar_row.clone();

    let save_settings = move || {
        let hb = hb_spin_clone.value();
        let auto = as_row_clone.is_active();
        let sp = sm_row_clone.selected();
        let bc = bat_row_clone.is_active();
        let ta = thm_row_clone.is_active();
        let zo = zone_row_clone.selected();
        let lb = lb_row_clone.is_active();
        if let Ok(home) = std::env::var("HOME") {
            let dir = format!("{}/.config/omenspace", home);
            let _ = std::fs::create_dir_all(&dir);
            let path = format!("{}/settings.json", dir);
            let json = serde_json::json!({
                "heartbeat_interval": hb,
                "autostart": auto,
                "startup_profile": sp,
                "battery_care": bc,
                "thermal_alerts": ta,
                "zone_override": zo,
                "lightbar_enabled": lb
            });
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
        // apply to daemon immediately
        let limit = if r.is_active() { 80 } else { 100 };
        crate::daemon_client::set_battery_care_sync(limit);
    });
    
    let s5 = save_settings_rc.clone();
    thermal_row.connect_active_notify(move |r| {
        s5();
        crate::daemon_client::set_thermal_protection_sync(r.is_active());
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

    // ── Fan Control group ─────────────────────────────────────
    let fan_control_group = adw::PreferencesGroup::builder()
        .title(i18n::t("fan_control_group"))
        .build();

    let fan_clean_row = adw::ActionRow::builder()
        .title(i18n::t("fan_cleaning_title"))
        .subtitle(i18n::t("fan_cleaning_sub"))
        .activatable(true)
        .build();
    fan_clean_row.add_suffix(&gtk::Image::builder().icon_name("weather-storm-symbolic").build());
    
    let win_clone_clean = window.clone();
    fan_clean_row.connect_activated(move |_| {
        let dialog = adw::MessageDialog::builder()
            .heading(i18n::t("fan_cleaning_title"))
            .body(i18n::t("fan_cleaning_msg"))
            .transient_for(&win_clone_clean)
            .build();
        let spinner = gtk::Spinner::builder().spinning(true).halign(gtk::Align::Center).margin_top(12).margin_bottom(12).build();
        dialog.set_extra_child(Some(&spinner));
        dialog.present();
        
        let dialog_clone = dialog.clone();
        glib::spawn_future_local(async move {
            let res = crate::daemon_client::run_fan_cleaning_async().await;
            spinner.set_spinning(false);
            dialog_clone.set_heading(Some("Complete"));
            dialog_clone.set_body(&res.unwrap_or_else(|e| format!("Error: {}", e)));
            dialog_clone.add_response("ok", i18n::t("btn_close"));
            dialog_clone.connect_response(None, |d: &adw::MessageDialog, _| d.close());
        });
    });
    
    fan_control_group.add(&fan_clean_row);
    page.append(&fan_control_group);


    // ── Troubleshooting & Diagnostics group ─────────────────────
    let trouble_group = adw::PreferencesGroup::builder()
        .title(i18n::t("troubleshooting_group"))
        .description(i18n::t("troubleshooting_desc"))
        .build();

    let rgb_issue_row = adw::ActionRow::builder()
        .title(i18n::t("rgb_issue_title"))
        .subtitle(i18n::t("rgb_issue_sub"))
        .activatable(true)
        .build();
    rgb_issue_row.add_suffix(&gtk::Image::builder().icon_name("go-next-symbolic").build());
    
    let dsdt_row = adw::ActionRow::builder()
        .title(i18n::t("diag_report_title"))
        .subtitle(i18n::t("diag_report_sub"))
        .activatable(true)
        .build();
    dsdt_row.add_suffix(&gtk::Image::builder().icon_name("go-next-symbolic").build());

    let triage_row = adw::ActionRow::builder()
        .title(i18n::t("triage_title"))
        .subtitle(i18n::t("triage_sub"))
        .activatable(true)
        .build();
    triage_row.add_suffix(&gtk::Image::builder().icon_name("go-next-symbolic").build());

    // ── Callbacks for Diagnostics ──
    let win_clone1 = window.clone();
    rgb_issue_row.connect_activated(move |_| {
        let dialog = adw::MessageDialog::builder()
            .heading(i18n::t("gen_rgb_issue"))
            .body(i18n::t("wait_hw_footprint"))
            .transient_for(&win_clone1)
            .build();
        let spinner = gtk::Spinner::builder().spinning(true).halign(gtk::Align::Center).margin_top(12).margin_bottom(12).build();
        dialog.set_extra_child(Some(&spinner));
        dialog.present();
        
        let dialog_clone = dialog.clone();
        glib::spawn_future_local(async move {
            let res = crate::daemon_client::generate_rgb_issue_async().await;
            spinner.set_spinning(false);
            
            match res {
                Ok(report) => {
                    dialog_clone.set_heading(Some(i18n::t("rgb_issue_ready")));
                    dialog_clone.set_body(i18n::t("copy_report_gh"));
                    let tv = gtk::TextView::builder().editable(false).wrap_mode(gtk::WrapMode::WordChar).hexpand(true).vexpand(true).build();
                    tv.buffer().set_text(&report);
                    let sw = gtk::ScrolledWindow::builder().child(&tv).min_content_height(300).min_content_width(500).build();
                    dialog_clone.set_extra_child(Some(&sw));
                },
                Err(e) => {
                    dialog_clone.set_heading(Some(i18n::t("error_generic")));
                    dialog_clone.set_body(&format!("{}: {}", i18n::t("error_generic"), e));
                }
            }
            dialog_clone.add_response("ok", i18n::t("btn_close"));
            dialog_clone.set_default_response(Some("ok"));
            dialog_clone.set_close_response("ok");
        });
    });

    let win_clone2 = window.clone();
    dsdt_row.connect_activated(move |_| {
        let dialog = adw::MessageDialog::builder()
            .heading(i18n::t("diag_scan_running"))
            .body(i18n::t("scan_wmi_endpoints"))
            .transient_for(&win_clone2)
            .build();
        let spinner = gtk::Spinner::builder().spinning(true).halign(gtk::Align::Center).margin_top(12).margin_bottom(12).build();
        dialog.set_extra_child(Some(&spinner));
        dialog.present();
        
        let dialog_clone = dialog.clone();
        glib::spawn_future_local(async move {
            let res = crate::daemon_client::generate_diagnostic_report_async().await;
            spinner.set_spinning(false);
            
            match res {
                Ok(report) => {
                    dialog_clone.set_heading(Some(i18n::t("diag_report_ready")));
                    dialog_clone.set_body(i18n::t("diag_report_body"));
                    let tv = gtk::TextView::builder()
                        .editable(false)
                        .wrap_mode(gtk::WrapMode::WordChar)
                        .monospace(true)
                        .hexpand(true)
                        .vexpand(true)
                        .build();
                    tv.buffer().set_text(&report);
                    let sw = gtk::ScrolledWindow::builder().child(&tv).min_content_height(400).min_content_width(600).build();
                    dialog_clone.set_extra_child(Some(&sw));

                    dialog_clone.add_response("issue", i18n::t("btn_create_gh_issue"));
                    dialog_clone.set_response_appearance("issue", adw::ResponseAppearance::Suggested);
                    let report_clone = report.clone();
                    dialog_clone.connect_response(None, move |d: &adw::MessageDialog, response| {
                        if response == "issue" {
                            use urlencoding::encode;
                            let url = format!("https://github.com/yunusemreyl/omen-space/issues/new?title=Diagnostic+Report&body={}", encode(&report_clone));
                            let _ = gtk::gio::AppInfo::launch_default_for_uri(&url, None::<&gtk::gio::AppLaunchContext>);
                        }
                        d.close();
                    });
                },
                Err(e) => {
                    dialog_clone.set_heading(Some(i18n::t("error_generic")));
                    dialog_clone.set_body(&format!("{}: {}", i18n::t("error_generic"), e));
                    dialog_clone.connect_response(None, |d: &adw::MessageDialog, _| d.close());
                }
            }
            dialog_clone.add_response("ok", i18n::t("btn_close"));
        });
    });
    let win_clone3 = window.clone();
    triage_row.connect_activated(move |_| {
        let dialog = adw::MessageDialog::builder()
            .heading(i18n::t("triage_running"))
            .body(i18n::t("triage_running_body"))
            .transient_for(&win_clone3)
            .build();
        let spinner = gtk::Spinner::builder().spinning(true).halign(gtk::Align::Center).margin_top(12).margin_bottom(12).build();
        dialog.set_extra_child(Some(&spinner));
        dialog.present();
        
        let dialog_clone = dialog.clone();
        glib::spawn_future_local(async move {
            let res = crate::daemon_client::generate_triage_bundle_async().await;
            spinner.set_spinning(false);
            
            match res {
                Ok(report) => {
                    dialog_clone.set_heading(Some(i18n::t("diag_report_ready")));
                    dialog_clone.set_body(i18n::t("diag_report_body"));
                    let tv = gtk::TextView::builder()
                        .editable(false)
                        .wrap_mode(gtk::WrapMode::WordChar)
                        .monospace(true)
                        .hexpand(true)
                        .vexpand(true)
                        .build();
                    tv.buffer().set_text(&report);
                    let sw = gtk::ScrolledWindow::builder().child(&tv).min_content_height(400).min_content_width(600).build();
                    dialog_clone.set_extra_child(Some(&sw));

                    dialog_clone.add_response("issue", i18n::t("btn_create_gh_issue"));
                    dialog_clone.set_response_appearance("issue", adw::ResponseAppearance::Suggested);
                    let report_clone = report.clone();
                    dialog_clone.connect_response(None, move |d: &adw::MessageDialog, response| {
                        if response == "issue" {
                            use urlencoding::encode;
                            let url = format!("https://github.com/yunusemreyl/omen-space/issues/new?title=Hardware+Triage+Report&body={}", encode(&report_clone));
                            let _ = gtk::gio::AppInfo::launch_default_for_uri(&url, None::<&gtk::gio::AppLaunchContext>);
                        }
                        d.close();
                    });
                },
                Err(e) => {
                    dialog_clone.set_heading(Some(i18n::t("error_generic")));
                    dialog_clone.set_body(&format!("{}: {}", i18n::t("error_generic"), e));
                    dialog_clone.connect_response(None, |d: &adw::MessageDialog, _| d.close());
                }
            }
            dialog_clone.add_response("ok", i18n::t("btn_close"));
        });
    });


    trouble_group.add(&rgb_issue_row);
    trouble_group.add(&dsdt_row);
    trouble_group.add(&triage_row);
    page.append(&trouble_group);

    let specs = crate::daemon_client::get_hardware_specs_sync();
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
