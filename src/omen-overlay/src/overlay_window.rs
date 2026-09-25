use gtk::prelude::*;
use libadwaita as adw;
use adw::prelude::*;
use std::rc::Rc;
use std::cell::RefCell;
use crate::daemon_client::{self, SystemStats};
// ── Overlay config (position, hotkey, margin) ─────────────────────────────────

#[derive(Clone)]
struct OverlayConfig {
    halign: String,  // "start" | "center" | "end"
    valign: String,  // "start" | "center" | "end"
    margin: i32,
    hotkey: String,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            halign: "end".into(),
            valign: "start".into(),
            margin: 24,
            hotkey: "Shift+F2".into(),
        }
    }
}

fn load_overlay_config() -> OverlayConfig {
    let mut cfg = OverlayConfig::default();
    if let Ok(home) = std::env::var("HOME") {
        let path = format!("{}/.config/omenspace/settings.json", home);
        if let Ok(s) = std::fs::read_to_string(&path) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                if let Some(h) = v.get("overlay_halign").and_then(|x| x.as_str()) { cfg.halign = h.to_string(); }
                if let Some(v2) = v.get("overlay_valign").and_then(|x| x.as_str()) { cfg.valign = v2.to_string(); }
                if let Some(m) = v.get("overlay_margin").and_then(|x| x.as_i64()) { cfg.margin = m as i32; }
                if let Some(hk) = v.get("overlay_hotkey").and_then(|x| x.as_str()) { cfg.hotkey = hk.to_string(); }
            }
        }
    }
    cfg
}

pub struct OverlayWindow {
    pub window: adw::ApplicationWindow,
    active_power: Rc<RefCell<String>>,
    active_fan: Rc<RefCell<String>>,
    power_btns: Rc<RefCell<Vec<(String, gtk::Button)>>>,
    fan_btns: Rc<RefCell<Vec<(String, gtk::Button)>>>,
    cpu_val_label: gtk::Label,
    gpu_val_label: gtk::Label,
    fan_val_label: gtk::Label,
    ram_val_label: gtk::Label,
    tag_label: gtk::Label,
}

impl OverlayWindow {
    pub fn new(app: &adw::Application) -> Rc<Self> {
        let cfg = load_overlay_config();

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title(&crate::i18n::t("title"))
            .decorated(false)
            .resizable(false)
            .default_width(680)
            .default_height(420)
            .css_classes(["omen-overlay-window"])
            .build();

        let active_power = Rc::new(RefCell::new("Default".to_string()));
        let active_fan = Rc::new(RefCell::new("auto".to_string()));
        let power_btns = Rc::new(RefCell::new(Vec::new()));
        let fan_btns = Rc::new(RefCell::new(Vec::new()));

        // Outer container
        let root_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["omen-overlay-card"])
            .spacing(14)
            .build();

        // ── 1. Header ────────────────────────────────────────────────────────
        let header = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .css_classes(["overlay-header"])
            .spacing(10)
            .build();

        let brand_icon = gtk::Image::builder()
            .icon_name("preferences-desktop-display-symbolic")
            .pixel_size(20)
            .build();
        header.append(&brand_icon);

        let title_label = gtk::Label::builder()
            .label(&crate::i18n::t("title"))
            .css_classes(["overlay-title"])
            .hexpand(true)
            .halign(gtk::Align::Start)
            .build();
        header.append(&title_label);

        // Show configured hotkey in header tag
        let tag_label = gtk::Label::builder()
            .label(&cfg.hotkey.to_uppercase())
            .css_classes(["overlay-brand-tag"])
            .build();
        header.append(&tag_label);

        let close_btn = gtk::Button::builder()
            .icon_name("window-close-symbolic")
            .css_classes(["overlay-close-btn"])
            .build();
        let app_c = app.clone();
        close_btn.connect_clicked(move |_| {
            app_c.quit();
        });
        header.append(&close_btn);

        root_box.append(&header);

        // ── 2. Performance Profiles ──────────────────────────────────────────
        let perf_section = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .build();

        let perf_label = gtk::Label::builder()
            .label(&crate::i18n::t("perf_mode"))
            .css_classes(["section-label"])
            .halign(gtk::Align::Start)
            .build();
        perf_section.append(&perf_label);

        let perf_grid = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(10)
            .homogeneous(true)
            .build();

        let power_modes = [
            (crate::i18n::t("quiet"), "1", crate::i18n::t("eco_silent"), "active-eco", "power-profile-power-saver-symbolic"),
            (crate::i18n::t("default"), "2", crate::i18n::t("balanced"), "active", "power-profile-balanced-symbolic"),
            (crate::i18n::t("performance"), "3", crate::i18n::t("max_power"), "active-perf", "power-profile-performance-symbolic"),
        ];

        let mut p_btns = Vec::new();

        for (name, key_shortcut, desc, _active_class, icon_name) in power_modes.iter() {
            let btn = gtk::Button::builder()
                .css_classes(["mode-btn"])
                .build();

            let inner = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(4)
                .build();

            let top_row = gtk::Box::builder()
                .orientation(gtk::Orientation::Horizontal)
                .spacing(6)
                .build();

            let icon = gtk::Image::builder()
                .icon_name(*icon_name)
                .pixel_size(18)
                .build();
            top_row.append(&icon);

            let lbl = gtk::Label::builder()
                .label(name)
                .css_classes(["mode-name"])
                .hexpand(true)
                .halign(gtk::Align::Start)
                .build();
            top_row.append(&lbl);

            let badge = gtk::Label::builder()
                .label(*key_shortcut)
                .css_classes(["mode-badge"])
                .build();
            top_row.append(&badge);

            inner.append(&top_row);

            let sub = gtk::Label::builder()
                .label(desc)
                .css_classes(["mode-desc"])
                .halign(gtk::Align::Start)
                .build();
            inner.append(&sub);

            btn.set_child(Some(&inner));

            let p_name = name.to_string();
            p_btns.push((p_name, btn.clone()));
            perf_grid.append(&btn);
        }

        perf_section.append(&perf_grid);
        root_box.append(&perf_section);

        // ── 3. Fan Profiles / Modes ──────────────────────────────────────────
        let fan_section = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .margin_top(4)
            .build();

        let fan_label = gtk::Label::builder()
            .label(&crate::i18n::t("fan_mode"))
            .css_classes(["section-label"])
            .halign(gtk::Align::Start)
            .build();
        fan_section.append(&fan_label);

        let fan_grid = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(10)
            .homogeneous(true)
            .build();

        let fan_modes = [
            ("auto", "Q", crate::i18n::t("auto"), "active", "weather-clear-symbolic"),
            ("max", "W", crate::i18n::t("max"), "active-turbo", "weather-storm-symbolic"),
            ("custom", "E", crate::i18n::t("custom"), "active", "emblem-system-symbolic"),
        ];

        let mut f_btns = Vec::new();

        for (mode_key, key_shortcut, title, _active_class, icon_name) in fan_modes.iter() {
            let btn = gtk::Button::builder()
                .css_classes(["mode-btn"])
                .build();

            let inner = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(4)
                .build();

            let top_row = gtk::Box::builder()
                .orientation(gtk::Orientation::Horizontal)
                .spacing(6)
                .build();

            let icon = gtk::Image::builder()
                .icon_name(*icon_name)
                .pixel_size(18)
                .build();
            top_row.append(&icon);

            let lbl = gtk::Label::builder()
                .label(title)
                .css_classes(["mode-name"])
                .hexpand(true)
                .halign(gtk::Align::Start)
                .build();
            top_row.append(&lbl);

            let badge = gtk::Label::builder()
                .label(*key_shortcut)
                .css_classes(["mode-badge"])
                .build();
            top_row.append(&badge);

            inner.append(&top_row);

            let sub = gtk::Label::builder()
                .label(&match *mode_key {
                    "auto" => crate::i18n::t("auto_desc"),
                    "max" => crate::i18n::t("max_desc"),
                    _ => crate::i18n::t("custom_desc"),
                })
                .css_classes(["mode-desc"])
                .halign(gtk::Align::Start)
                .build();
            inner.append(&sub);

            btn.set_child(Some(&inner));

            let m_key = mode_key.to_string();
            f_btns.push((m_key, btn.clone()));
            fan_grid.append(&btn);
        }

        fan_section.append(&fan_grid);
        root_box.append(&fan_section);

        // ── 4. Live Telemetry Strip ──────────────────────────────────────────
        let telemetry_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .css_classes(["telemetry-box"])
            .spacing(12)
            .homogeneous(true)
            .build();

        // CPU chip
        let cpu_chip = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(2).build();
        cpu_chip.append(&gtk::Label::builder().label("CPU").css_classes(["telem-chip-title"]).halign(gtk::Align::Start).build());
        let cpu_val = gtk::Label::builder().label("──°C | ──W").css_classes(["telem-chip-val"]).halign(gtk::Align::Start).build();
        cpu_chip.append(&cpu_val);
        telemetry_box.append(&cpu_chip);

        // GPU chip
        let gpu_chip = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(2).build();
        gpu_chip.append(&gtk::Label::builder().label("GPU").css_classes(["telem-chip-title"]).halign(gtk::Align::Start).build());
        let gpu_val = gtk::Label::builder().label("──°C | ──W").css_classes(["telem-chip-val"]).halign(gtk::Align::Start).build();
        gpu_chip.append(&gpu_val);
        telemetry_box.append(&gpu_chip);

        // Fan chip
        let fan_chip = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(2).build();
        fan_chip.append(&gtk::Label::builder().label(&crate::i18n::t("fans")).css_classes(["telem-chip-title"]).halign(gtk::Align::Start).build());
        let fan_val = gtk::Label::builder().label("── RPM").css_classes(["telem-chip-val"]).halign(gtk::Align::Start).build();
        fan_chip.append(&fan_val);
        telemetry_box.append(&fan_chip);

        // RAM chip
        let ram_chip = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(2).build();
        ram_chip.append(&gtk::Label::builder().label("RAM").css_classes(["telem-chip-title"]).halign(gtk::Align::Start).build());
        let ram_val = gtk::Label::builder().label("── / ── GB").css_classes(["telem-chip-val"]).halign(gtk::Align::Start).build();
        ram_chip.append(&ram_val);
        telemetry_box.append(&ram_chip);

        root_box.append(&telemetry_box);

        // ── 5. Footer Shortcut Hints ─────────────────────────────────────────
        let footer = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .css_classes(["footer-shortcut-box"])
            .spacing(14)
            .halign(gtk::Align::Center)
            .build();

        let hint1 = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(4).build();
        hint1.append(&gtk::Label::builder().label("1-3").css_classes(["shortcut-key"]).build());
        hint1.append(&gtk::Label::builder().label(&crate::i18n::t("perf_mode")).css_classes(["shortcut-hint"]).build());
        footer.append(&hint1);

        let hint2 = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(4).build();
        hint2.append(&gtk::Label::builder().label("Q/W/E").css_classes(["shortcut-key"]).build());
        hint2.append(&gtk::Label::builder().label(&crate::i18n::t("fan_mode")).css_classes(["shortcut-hint"]).build());
        footer.append(&hint2);

        let hint3 = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(4).build();
        hint3.append(&gtk::Label::builder().label("ESC").css_classes(["shortcut-key"]).build());
        hint3.append(&gtk::Label::builder().label(&crate::i18n::t("close")).css_classes(["shortcut-hint"]).build());
        footer.append(&hint3);

        root_box.append(&footer);

        window.set_content(Some(&root_box));

        *power_btns.borrow_mut() = p_btns;
        *fan_btns.borrow_mut() = f_btns;

        let overlay = Rc::new(Self {
            window,
            active_power,
            active_fan,
            power_btns,
            fan_btns,
            cpu_val_label: cpu_val,
            gpu_val_label: gpu_val,
            fan_val_label: fan_val,
            ram_val_label: ram_val,
            tag_label,
        });

        overlay.apply_position(&cfg);
        overlay.refresh_initial_state();
        overlay.setup_interactions();
        overlay
    }

    fn setup_interactions(self: &Rc<Self>) {
        // Connect Power Profile buttons
        for (name, btn) in self.power_btns.borrow().iter() {
            let this = self.clone();
            let p_name = name.clone();
            btn.connect_clicked(move |_| {
                this.select_power_profile(&p_name);
            });
        }

        // Connect Fan Mode buttons
        for (mode, btn) in self.fan_btns.borrow().iter() {
            let this = self.clone();
            let m_name = mode.clone();
            btn.connect_clicked(move |_| {
                this.select_fan_mode(&m_name);
            });
        }

        // Keyboard Controller
        let key_controller = gtk::EventControllerKey::new();
        let this = self.clone();
        key_controller.connect_key_pressed(move |_, key, _keycode, state| {
            let key_val = key.name().unwrap_or_default().to_lowercase();
            let _has_shift = state.contains(gtk::gdk::ModifierType::SHIFT_MASK);

            // Escape/F2 triggers app quit, handled in main.rs key controller

            match key_val.as_str() {
                "1" | "kp_1" => {
                    this.select_power_profile("Quiet");
                    glib::Propagation::Stop
                }
                "2" | "kp_2" => {
                    this.select_power_profile("Default");
                    glib::Propagation::Stop
                }
                "3" | "kp_3" => {
                    this.select_power_profile("Performance");
                    glib::Propagation::Stop
                }
                "q" => {
                    this.select_fan_mode("auto");
                    glib::Propagation::Stop
                }
                "w" => {
                    this.select_fan_mode("max");
                    glib::Propagation::Stop
                }
                "e" => {
                    this.select_fan_mode("custom");
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });

        self.window.add_controller(key_controller);
    }

    pub fn select_power_profile(self: &Rc<Self>, profile: &str) {
        *self.active_power.borrow_mut() = profile.to_string();
        self.update_button_states();

        let prof = profile.to_string();
        let rt = daemon_client::get_runtime();
        rt.spawn(async move {
            let _ = daemon_client::set_power_profile(&prof).await;
        });
    }

    pub fn select_fan_mode(self: &Rc<Self>, mode: &str) {
        *self.active_fan.borrow_mut() = mode.to_string();
        self.update_button_states();

        let m = mode.to_string();
        let rt = daemon_client::get_runtime();
        rt.spawn(async move {
            let _ = daemon_client::set_fan_mode(&m).await;
        });
    }

    pub fn update_button_states(&self) {
        let cur_power = self.active_power.borrow().clone().to_lowercase();
        for (name, btn) in self.power_btns.borrow().iter() {
            btn.remove_css_class("active");
            btn.remove_css_class("active-perf");
            btn.remove_css_class("active-eco");

            let n_lower = name.to_lowercase();
            let is_match = n_lower == cur_power ||
                (n_lower == "default" && (cur_power == "balanced" || cur_power == "default")) ||
                (n_lower == "quiet" && (cur_power == "quiet" || cur_power == "eco" || cur_power == "power-saver" || cur_power == "low-power")) ||
                (n_lower == "performance" && (cur_power == "performance" || cur_power == "perf" || cur_power == "max"));

            if is_match {
                if n_lower == "performance" {
                    btn.add_css_class("active-perf");
                } else if n_lower == "quiet" {
                    btn.add_css_class("active-eco");
                } else {
                    btn.add_css_class("active");
                }
            }
        }

        let cur_fan = self.active_fan.borrow().clone().to_lowercase();
        for (mode, btn) in self.fan_btns.borrow().iter() {
            btn.remove_css_class("active");
            btn.remove_css_class("active-turbo");

            let m_lower = mode.to_lowercase();
            let is_match = m_lower == cur_fan ||
                (m_lower == "custom" && (cur_fan == "manual" || cur_fan == "custom" || cur_fan == "curve")) ||
                (m_lower == "auto" && (cur_fan == "auto" || cur_fan == "ec" || cur_fan == "default"));

            if is_match {
                if m_lower == "max" {
                    btn.add_css_class("active-turbo");
                } else {
                    btn.add_css_class("active");
                }
            }
        }
    }

    pub fn update_telemetry(&self, stats: &SystemStats) {
        self.cpu_val_label.set_label(&format!("{}°C | {:.0}W", stats.cpu_temp, stats.cpu_pwr));
        self.gpu_val_label.set_label(&format!("{}°C | {:.0}W", stats.gpu_temp, stats.gpu_pwr));

        if stats.fan1_rpm > 0 || stats.fan2_rpm > 0 {
            self.fan_val_label.set_label(&format!("{} / {} RPM", stats.fan1_rpm, stats.fan2_rpm));
        } else if stats.fan_rpm > 0 {
            self.fan_val_label.set_label(&format!("{} RPM", stats.fan_rpm));
        } else {
            self.fan_val_label.set_label("Auto RPM");
        }

        if stats.ram_total_gb > 0.0 {
            self.ram_val_label.set_label(&format!("{:.1} / {:.0} GB", stats.ram_used_gb, stats.ram_total_gb));
        }
    }

    #[allow(deprecated)]
    pub fn refresh_initial_state(self: &Rc<Self>) {
        let (tx, rx) = glib::MainContext::channel::<(String, String)>(glib::Priority::default());
        let this = self.clone();
        rx.attach(None, move |(p, f)| {
            *this.active_power.borrow_mut() = p;
            *this.active_fan.borrow_mut() = f;
            this.update_button_states();
            glib::ControlFlow::Break
        });

        let rt = daemon_client::get_runtime();
        rt.spawn(async move {
            let p = daemon_client::get_power_profile().await;
            let f = daemon_client::get_fan_mode().await;
            let _ = tx.send((p, f));
        });
    }

    fn apply_position(&self, cfg: &OverlayConfig) {
        self.tag_label.set_label(&cfg.hotkey.to_uppercase());
        // Positioning is blocked by KDE Wayland focus stealing / layer shell rules.
        // Thus, the window will appear in the center by default as a standard undecorated window.
    }
}
