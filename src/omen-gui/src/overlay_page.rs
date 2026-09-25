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
        let rt = daemon_client::get_runtime();
        rt.spawn(async {
            let _ = daemon_client::send_toggle_overlay_signal().await;
        });
    });

    launch_row.add_suffix(&launch_btn);
    hero_group.add(&launch_row);
    page.append(&hero_group);

    // ── 3. Position Picker ────────────────────────────────────────────────────
    let pos_group = adw::PreferencesGroup::builder()
        .title(i18n::t("overlay_position_group"))
        .build();

    // Grid picker row: 3×3 toggle buttons
    let pos_row = adw::ActionRow::builder()
        .title(i18n::t("overlay_position_title"))
        .subtitle(i18n::t("overlay_position_sub"))
        .build();

    let grid_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .valign(gtk::Align::Center)
        .build();

    // Position labels in order
    let pos_labels = [
        "overlay_pos_top_left",    "overlay_pos_top_center",    "overlay_pos_top_right",
        "overlay_pos_center_left", "overlay_pos_center",        "overlay_pos_center_right",
        "overlay_pos_bottom_left", "overlay_pos_bottom_center", "overlay_pos_bottom_right",
    ];

    // Position emojis / icons for each cell
    let pos_icons = ["↖", "↑", "↗", "←", "✛", "→", "↙", "↓", "↘"];

    // Build 3 rows × 3 cols
    let mut pos_btns: Vec<gtk::ToggleButton> = Vec::new();

    for row_i in 0..3usize {
        let hbox = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(0)
            .build();
        for col_i in 0..3usize {
            let idx = (row_i * 3 + col_i) as u32;
            let icon = pos_icons[idx as usize];
            let lbl_key = pos_labels[idx as usize];

            let btn = gtk::ToggleButton::builder()
                .tooltip_text(i18n::t(lbl_key))
                .css_classes(["pos-grid-btn"])
                .build();

            let inner = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(1)
                .halign(gtk::Align::Center)
                .valign(gtk::Align::Center)
                .build();
            inner.append(&gtk::Label::builder().label(icon).css_classes(["pos-grid-icon"]).build());
            inner.append(&gtk::Label::builder()
                .label(i18n::t(lbl_key))
                .css_classes(["pos-grid-label"])
                .build());
            btn.set_child(Some(&inner));

            // On toggle: save position
            btn.connect_toggled(move |b| {
                if b.is_active() {
                    let (h, v) = index_to_pos_str(idx);
                    patch_settings(|s| {
                        s["overlay_halign"] = serde_json::json!(h);
                        s["overlay_valign"] = serde_json::json!(v);
                    });
                }
            });

            pos_btns.push(btn.clone());
            hbox.append(&btn);
        }
        grid_box.append(&hbox);
    }

    // Group all buttons to first, then set active
    if pos_btns.len() > 1 {
        for btn in pos_btns.iter().skip(1) {
            btn.set_group(Some(&pos_btns[0]));
        }
    }
    if let Some(btn) = pos_btns.get(init_pos_idx as usize) {
        btn.set_active(true);
    } else if let Some(btn) = pos_btns.first() {
        btn.set_active(true);
    }

    pos_row.add_suffix(&grid_box);
    pos_group.add(&pos_row);

    // ── Margin slider row ─────────────────────────────────────────────────────
    let margin_row = adw::ActionRow::builder()
        .title(i18n::t("overlay_margin_title"))
        .subtitle(i18n::t("overlay_margin_sub"))
        .build();

    let margin_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .valign(gtk::Align::Center)
        .build();

    let margin_scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 80.0, 4.0);
    margin_scale.set_value(init_margin);
    margin_scale.set_width_request(160);
    margin_scale.set_draw_value(true);
    margin_scale.set_value_pos(gtk::PositionType::Right);

    margin_scale.connect_value_changed(|s| {
        let val = s.value() as u64;
        patch_settings(|settings| {
            settings["overlay_margin"] = serde_json::json!(val);
        });
    });

    margin_box.append(&margin_scale);
    margin_row.add_suffix(&margin_box);
    pos_group.add(&margin_row);

    page.append(&pos_group);

    // ── 4. Hotkey Customization ───────────────────────────────────────────────
    let hk_group = adw::PreferencesGroup::builder()
        .title(i18n::t("overlay_hotkey_group"))
        .build();

    let hotkey_label = std::rc::Rc::new(std::cell::RefCell::new(init_hotkey.clone()));
    let recording = std::rc::Rc::new(std::cell::Cell::new(false));

    let hk_row = adw::ActionRow::builder()
        .title(i18n::t("overlay_hotkey_title"))
        .subtitle(i18n::t("overlay_hotkey_sub"))
        .build();

    // Current hotkey badge
    let hk_badge = gtk::Label::builder()
        .label(&init_hotkey)
        .css_classes(["badge-accent"])
        .valign(gtk::Align::Center)
        .build();

    // Record button
    let record_btn = gtk::Button::builder()
        .label(i18n::t("overlay_hotkey_record_btn"))
        .css_classes(["pill"])
        .valign(gtk::Align::Center)
        .build();

    // Reset button
    let reset_btn = gtk::Button::builder()
        .label(i18n::t("overlay_hotkey_reset"))
        .css_classes(["pill"])
        .valign(gtk::Align::Center)
        .build();

    // Key capture: we attach an EventControllerKey to the row when recording
    let hk_badge_c  = hk_badge.clone();
    let record_btn_c = record_btn.clone();
    let reset_btn_c  = reset_btn.clone();
    let hk_label_c   = hotkey_label.clone();
    let recording_c  = recording.clone();
    let hk_row_c     = hk_row.clone();

    record_btn.connect_clicked(move |btn| {
        if recording_c.get() { return; }
        recording_c.set(true);
        hk_badge_c.set_label(i18n::t("overlay_hotkey_recording"));
        hk_badge_c.remove_css_class("badge-accent");
        hk_badge_c.add_css_class("badge-warn");
        btn.set_sensitive(false);
        reset_btn_c.set_sensitive(false);

        // Attach key controller
        let controller = gtk::EventControllerKey::new();
        let hk_badge2   = hk_badge_c.clone();
        let rec2        = recording_c.clone();
        let rec_btn2    = btn.clone();
        let rst_btn2    = reset_btn_c.clone();
        let hk_label2   = hk_label_c.clone();

        controller.connect_key_pressed(move |ctrl, key, _code, state| {
            if !rec2.get() { return glib::Propagation::Proceed; }

            let key_name = key.name().unwrap_or_default().to_string();

            // ESC cancels recording
            if key_name.to_lowercase() == "escape" {
                hk_badge2.set_label(&hk_label2.borrow());
                hk_badge2.remove_css_class("badge-warn");
                hk_badge2.add_css_class("badge-accent");
                rec2.set(false);
                rec_btn2.set_sensitive(true);
                rst_btn2.set_sensitive(true);
                // Detach controller
                ctrl.widget().remove_controller(ctrl);
                return glib::Propagation::Stop;
            }

            // Skip modifier-only presses
            let is_modifier = matches!(key_name.to_lowercase().as_str(),
                "shift_l" | "shift_r" | "control_l" | "control_r" |
                "alt_l" | "alt_r" | "super_l" | "super_r" | "meta_l" | "meta_r" |
                "caps_lock" | "num_lock"
            );
            if is_modifier { return glib::Propagation::Stop; }

            // Build combo string
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

            // Save
            *hk_label2.borrow_mut() = combo.clone();
            patch_settings(|s| { s["overlay_hotkey"] = serde_json::json!(&combo); });

            hk_badge2.set_label(&combo);
            hk_badge2.remove_css_class("badge-warn");
            hk_badge2.add_css_class("badge-accent");
            rec2.set(false);
            rec_btn2.set_sensitive(true);
            rst_btn2.set_sensitive(true);
            ctrl.widget().remove_controller(ctrl);
            glib::Propagation::Stop
        });

        hk_row_c.add_controller(controller);
    });

    let hk_badge_r = hk_badge.clone();
    let hk_label_r = hotkey_label.clone();
    let recording_r = recording.clone();
    let record_btn_r = record_btn.clone();
    reset_btn.connect_clicked(move |_| {
        if recording_r.get() { return; }
        let default = "Shift+F2".to_string();
        *hk_label_r.borrow_mut() = default.clone();
        hk_badge_r.set_label(&default);
        hk_badge_r.remove_css_class("badge-warn");
        hk_badge_r.add_css_class("badge-accent");
        patch_settings(|s| { s["overlay_hotkey"] = serde_json::json!(&default); });
        record_btn_r.set_sensitive(true);
    });

    let suffix_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .valign(gtk::Align::Center)
        .build();
    suffix_box.append(&hk_badge);
    suffix_box.append(&record_btn);
    suffix_box.append(&reset_btn);

    hk_row.add_suffix(&suffix_box);
    hk_group.add(&hk_row);
    page.append(&hk_group);

    // ── 5. Keybindings Cheatsheet (overlay-internal) ──────────────────────────
    let shortcuts_group = adw::PreferencesGroup::builder()
        .title(i18n::t("overlay_shortcuts_title"))
        .description(i18n::t("overlay_shortcuts_sub"))
        .build();

    // Read current hotkey for global toggle row
    let current_hotkey = hotkey_label.borrow().clone();
    let sc_items = [
        (current_hotkey.as_str(), "Global Hotkey", "Toggles the on-screen overlay HUD anywhere, including in games", "badge-ok"),
        ("1 / 2 / 3", "Power Modes", "1: Quiet (Eco) • 2: Balanced (Default) • 3: Performance (Max Power)", "badge-accent"),
        ("Q / W / E", "Fan Modes", "Q: Auto (Smart Curve) • W: Max (100% Turbo) • E: Custom Preset", "badge-accent"),
        ("Esc", "Close HUD", "Dismisses overlay and returns immediate focus to game or app", "badge-warn"),
    ];

    for (key, title, desc, badge_style) in sc_items.iter() {
        let row = adw::ActionRow::builder()
            .title(*title)
            .subtitle(*desc)
            .build();
        let badge = gtk::Label::builder()
            .label(*key)
            .css_classes([*badge_style])
            .valign(gtk::Align::Center)
            .build();
        row.add_suffix(&badge);
        shortcuts_group.add(&row);
    }

    page.append(&shortcuts_group);

    // ── 6. Capabilities Card ──────────────────────────────────────────────────
    let feat_group = adw::PreferencesGroup::builder()
        .title(i18n::t("overlay_preview_title"))
        .description(i18n::t("overlay_preview_desc"))
        .build();

    let feat_telemetry = adw::ActionRow::builder()
        .title("Real-Time Cyber Telemetry")
        .subtitle("Displays live CPU & GPU temperatures (°C), wattage (W), dual fan RPMs, and memory usage without taking focus away from games.")
        .build();
    feat_group.add(&feat_telemetry);

    let feat_sync = adw::ActionRow::builder()
        .title("Bidirectional System Synchronization")
        .subtitle("Instant real-time sync with OMEN Space GUI, OMEN Tray, CLI, and Hardware Daemon.")
        .build();
    feat_group.add(&feat_sync);

    let feat_lock = adw::ActionRow::builder()
        .title("Zero-Lag Resident Architecture")
        .subtitle("Resident GTK4 / Libadwaita single-instance daemon guarantees instant (<10ms) overlay appearance.")
        .build();
    feat_group.add(&feat_lock);

    page.append(&feat_group);

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
