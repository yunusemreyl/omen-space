use gtk::prelude::*;
use libadwaita as adw;
use adw::prelude::*;
use crate::i18n;
use crate::daemon_client;

// ── Config helpers ────────────────────────────────────────────────────────────

fn settings_path() -> Option<String> {
    std::env::var("HOME").ok().map(|h| format!("{}/.config/omenspace/settings.json", h))
}

fn load_settings() -> serde_json::Value {
    if let Some(path) = settings_path() {
        if let Ok(s) = std::fs::read_to_string(&path) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                return v;
            }
        }
    }
    serde_json::json!({})
}

fn save_settings(json: &serde_json::Value) {
    if let Some(path) = settings_path() {
        if let Some(parent) = std::path::Path::new(&path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, serde_json::to_string_pretty(json).unwrap_or_default());
    }
}

fn patch_settings<F: Fn(&mut serde_json::Value)>(f: F) {
    let mut v = load_settings();
    f(&mut v);
    save_settings(&v);
}

/// Map u32 position index → (halign, valign) as strings saved in JSON
fn index_to_pos_str(idx: u32) -> (&'static str, &'static str) {
    match idx {
        0 => ("start",  "start"),   // top-left
        1 => ("center", "start"),   // top-center
        2 => ("end",    "start"),   // top-right
        3 => ("start",  "center"),  // center-left
        4 => ("center", "center"),  // center
        5 => ("end",    "center"),  // center-right
        6 => ("start",  "end"),     // bottom-left
        7 => ("center", "end"),     // bottom-center
        _ => ("end",    "end"),     // bottom-right (default)
    }
}

fn pos_str_to_index(h: &str, v: &str) -> u32 {
    match (h, v) {
        ("start",  "start")  => 0,
        ("center", "start")  => 1,
        ("end",    "start")  => 2,
        ("start",  "center") => 3,
        ("center", "center") => 4,
        ("end",    "center") => 5,
        ("start",  "end")    => 6,
        ("center", "end")    => 7,
        _                    => 8,
    }
}

fn pos_index_key(idx: u32) -> &'static str {
    match idx {
        0 => "overlay_pos_top_left",
        1 => "overlay_pos_top_center",
        2 => "overlay_pos_top_right",
        3 => "overlay_pos_center_left",
        4 => "overlay_pos_center",
        5 => "overlay_pos_center_right",
        6 => "overlay_pos_bottom_left",
        7 => "overlay_pos_bottom_center",
        _ => "overlay_pos_bottom_right",
    }
}

// ── Build page ────────────────────────────────────────────────────────────────

pub fn build_page(_window: &adw::ApplicationWindow) -> gtk::Box {
    let settings = load_settings();

    // Read saved overlay position
    let saved_h = settings.get("overlay_halign").and_then(|v| v.as_str()).unwrap_or("end").to_string();
    let saved_v = settings.get("overlay_valign").and_then(|v| v.as_str()).unwrap_or("start").to_string();
    let init_pos_idx = pos_str_to_index(&saved_h, &saved_v);

    // Read saved hotkey
    let init_hotkey = settings
        .get("overlay_hotkey")
        .and_then(|v| v.as_str())
        .unwrap_or("Shift+F2")
        .to_string();

    // Read saved margin
    let init_margin = settings
        .get("overlay_margin")
        .and_then(|v| v.as_u64())
        .unwrap_or(24) as f64;

    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(20)
        .build();

    // ── 1. Page Header ────────────────────────────────────────────────────────
    let title_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .build();

    title_row.append(&gtk::Label::builder()
        .label(i18n::t("title_overlay"))
        .css_classes(["page-title"])
        .halign(gtk::Align::Start)
        .hexpand(true)
        .build());

    // Live overlay process status badge — green if omen-overlay is running
    let status_badge = gtk::Label::builder()
        .css_classes(["badge-ok"])
        .valign(gtk::Align::Center)
        .build();
    let status_badge_clone = status_badge.clone();

    // Check overlay process status every 3s
    let update_status_badge = move || {
        let is_running = std::process::Command::new("pgrep")
            .arg("-x")
            .arg("omen-overlay")
            .output()
            .map(|o| o.status.success() && !o.stdout.is_empty())
            .unwrap_or(false);
        if is_running {
            status_badge_clone.set_label(i18n::t("overlay_status_active"));
            status_badge_clone.remove_css_class("badge-warn");
            status_badge_clone.add_css_class("badge-ok");
        } else {
            status_badge_clone.set_label(i18n::t("overlay_status_inactive"));
            status_badge_clone.remove_css_class("badge-ok");
            status_badge_clone.add_css_class("badge-warn");
        }
    };
    update_status_badge();
    let update_status_clone = {
        let status_badge_clone2 = status_badge.clone();
        move || {
            let is_running = std::process::Command::new("pgrep")
                .arg("-x")
                .arg("omen-overlay")
                .output()
                .map(|o| o.status.success() && !o.stdout.is_empty())
                .unwrap_or(false);
            if is_running {
                status_badge_clone2.set_label(i18n::t("overlay_status_active"));
                status_badge_clone2.remove_css_class("badge-warn");
                status_badge_clone2.add_css_class("badge-ok");
            } else {
                status_badge_clone2.set_label(i18n::t("overlay_status_inactive"));
                status_badge_clone2.remove_css_class("badge-ok");
                status_badge_clone2.add_css_class("badge-warn");
            }
        }
    };
    glib::timeout_add_local(std::time::Duration::from_secs(3), move || {
        update_status_clone();
        glib::ControlFlow::Continue
    });

    title_row.append(&status_badge);

    let hdr = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_bottom(4)
        .build();
    hdr.append(&title_row);
    hdr.append(&gtk::Label::builder()
        .label(i18n::t("overlay_desc"))
        .css_classes(["os-section-desc"])
        .halign(gtk::Align::Start)
        .build());
    page.append(&hdr);

    // ── 2. Launch Card ────────────────────────────────────────────────────────
    let hero_group = adw::PreferencesGroup::builder()
        .title(i18n::t("overlay_group"))
        .build();

    let launch_row = adw::ActionRow::builder()
        .title(i18n::t("overlay_launch_now"))
        .subtitle(i18n::t("overlay_launch_now_sub"))
        .build();

    let launch_btn = gtk::Button::builder()
        .label(i18n::t("overlay_launch_btn"))
        .icon_name("preferences-desktop-display-symbolic")
        .css_classes(["suggested-action", "pill"])
        .valign(gtk::Align::Center)
        .build();

    launch_btn.connect_clicked(|_| {
        let is_running = std::process::Command::new("pgrep")
            .arg("-x")
            .arg("omen-overlay")
            .output()
            .map(|o| o.status.success() && !o.stdout.is_empty())
            .unwrap_or(false);

        if is_running {
            let _ = std::process::Command::new("pkill").arg("-TERM").arg("-x").arg("omen-overlay").output();
            if let Ok(mut child) = std::process::Command::new("omen-overlay")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn() {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
        }
    });

    launch_row.add_suffix(&launch_btn);
    hero_group.add(&launch_row);
    page.append(&hero_group);

    // (Position Picker and Margin Slider removed due to Wayland compositor black-screen restrictions.
    // Users must rely on Window Manager rules for positioning in Wayland.)

    // ── 4. Hotkey Customization ───────────────────────────────────────────────
    let hk_group = adw::PreferencesGroup::builder()
        .title(i18n::t("overlay_hotkey_group"))
        .build();

    let hotkey_state = std::rc::Rc::new(std::cell::RefCell::new(init_hotkey.clone()));
    let recording = std::rc::Rc::new(std::cell::Cell::new(false));

    let hk_row = adw::ActionRow::builder()
        .title(i18n::t("overlay_hotkey_title"))
        .subtitle(i18n::t("overlay_hotkey_sub"))
        .build();

    let hk_label = gtk::ShortcutLabel::builder()
        .accelerator(&init_hotkey)
        .valign(gtk::Align::Center)
        .build();

    let record_btn = gtk::Button::builder()
        .icon_name("document-edit-symbolic")
        .css_classes(["circular", "flat"])
        .tooltip_text(i18n::t("overlay_hotkey_record_btn"))
        .valign(gtk::Align::Center)
        .build();

    let reset_btn = gtk::Button::builder()
        .icon_name("edit-undo-symbolic")
        .css_classes(["circular", "flat"])
        .tooltip_text(i18n::t("overlay_hotkey_reset"))
        .valign(gtk::Align::Center)
        .build();

    let reset_btn_c  = reset_btn.clone();
    let hk_label_c   = hk_label.clone();
    let recording_c  = recording.clone();
    let hk_row_c     = hk_row.clone();
    let record_btn_c = record_btn.clone();
    let hk_state_c   = hotkey_state.clone();

    record_btn.connect_clicked(move |_| {
        if recording_c.get() { return; }
        recording_c.set(true);
        hk_label_c.set_disabled_text("...");
        hk_label_c.set_accelerator("");
        hk_row_c.set_subtitle(i18n::t("overlay_hotkey_recording"));
        record_btn_c.add_css_class("suggested-action");
        reset_btn_c.set_sensitive(false);

        let controller = gtk::EventControllerKey::new();
        let rec2        = recording_c.clone();
        let rst_btn2    = reset_btn_c.clone();
        let hk_lbl2     = hk_label_c.clone();
        let row2        = hk_row_c.clone();
        let rec_btn2    = record_btn_c.clone();
        let state2      = hk_state_c.clone();

        controller.connect_key_pressed(move |ctrl, key, _code, state| {
            if !rec2.get() { return glib::Propagation::Proceed; }

            let key_name = key.name().unwrap_or_default().to_string();

            if key_name.to_lowercase() == "escape" {
                hk_lbl2.set_accelerator(&state2.borrow());
                row2.set_subtitle(i18n::t("overlay_hotkey_sub"));
                rec_btn2.remove_css_class("suggested-action");
                rec2.set(false);
                rst_btn2.set_sensitive(true);
                ctrl.widget().remove_controller(ctrl);
                return glib::Propagation::Stop;
            }

            let is_modifier = matches!(key_name.to_lowercase().as_str(),
                "shift_l" | "shift_r" | "control_l" | "control_r" |
                "alt_l" | "alt_r" | "super_l" | "super_r" | "meta_l" | "meta_r" |
                "caps_lock" | "num_lock"
            );
            if is_modifier { return glib::Propagation::Stop; }

            let mut parts: Vec<&str> = Vec::new();
            if state.contains(gtk::gdk::ModifierType::CONTROL_MASK) { parts.push("Ctrl"); }
            if state.contains(gtk::gdk::ModifierType::SUPER_MASK)   { parts.push("Super"); }
            if state.contains(gtk::gdk::ModifierType::ALT_MASK)     { parts.push("Alt"); }
            if state.contains(gtk::gdk::ModifierType::SHIFT_MASK)   { parts.push("Shift"); }
            let formatted_key = format_key_name(&key_name);
            let combo = if parts.is_empty() {
                formatted_key.clone()
            } else {
                format!("{}+{}", parts.join("+"), formatted_key)
            };

            *state2.borrow_mut() = combo.clone();
            patch_settings(|s| { s["overlay_hotkey"] = serde_json::json!(&combo); });

            hk_lbl2.set_accelerator(&combo);
            row2.set_subtitle(i18n::t("overlay_hotkey_sub"));
            rec_btn2.remove_css_class("suggested-action");
            rec2.set(false);
            rst_btn2.set_sensitive(true);
            ctrl.widget().remove_controller(ctrl);
            glib::Propagation::Stop
        });

        hk_row_c.add_controller(controller);
    });

    let hk_state_r = hotkey_state.clone();
    let recording_r = recording.clone();
    let hk_label_r = hk_label.clone();
    
    reset_btn.connect_clicked(move |_| {
        if recording_r.get() { return; }
        let default = "Shift+F2".to_string();
        *hk_state_r.borrow_mut() = default.clone();
        hk_label_r.set_accelerator(&default);
        patch_settings(|s| { s["overlay_hotkey"] = serde_json::json!(&default); });
    });

    let suffix_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .valign(gtk::Align::Center)
        .build();
    suffix_box.append(&hk_label);
    suffix_box.append(&record_btn);
    suffix_box.append(&reset_btn);

    hk_row.add_suffix(&suffix_box);
    hk_group.add(&hk_row);
    page.append(&hk_group);

    // Sections 5 and 6 (Cheatsheet & Capabilities) have been removed for a cleaner UI

    page
}

fn format_key_name(raw: &str) -> String {
    match raw.to_lowercase().as_str() {
        "f1"  => "F1".into(),  "f2"  => "F2".into(),  "f3"  => "F3".into(),
        "f4"  => "F4".into(),  "f5"  => "F5".into(),  "f6"  => "F6".into(),
        "f7"  => "F7".into(),  "f8"  => "F8".into(),  "f9"  => "F9".into(),
        "f10" => "F10".into(), "f11" => "F11".into(), "f12" => "F12".into(),
        "space"  => "Space".into(),
        "return" => "Enter".into(),
        "tab"    => "Tab".into(),
        "home"   => "Home".into(),
        "end"    => "End".into(),
        "prior"  => "PgUp".into(),
        "next"   => "PgDn".into(),
        "delete" => "Del".into(),
        "insert" => "Ins".into(),
        "up"     => "↑".into(),
        "down"   => "↓".into(),
        "left"   => "←".into(),
        "right"  => "→".into(),
        other    => {
            let mut c = other.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        }
    }
}
