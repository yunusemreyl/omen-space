use adw::prelude::*;
use gtk::prelude::*;
use libadwaita as adw;
use omen_types::hotkey::HotkeySpec;
use omen_types::GpuSampleState;
use std::cell::RefCell;
use std::rc::Rc;

use crate::daemon_client::{self, SystemStats};

#[derive(Clone)]
struct OverlayConfig {
    hotkey: String,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            hotkey: HotkeySpec::default().display(),
        }
    }
}

fn load_overlay_config() -> OverlayConfig {
    if let Ok(home) = std::env::var("HOME") {
        let path = format!("{}/.config/omenspace/settings.json", home);
        if let Ok(s) = std::fs::read_to_string(&path) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                return OverlayConfig {
                    hotkey: HotkeySpec::from_settings(&v).display(),
                };
            }
        }
    }
    OverlayConfig::default()
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
    battery_sep: gtk::Separator,
    battery_chip: gtk::Box,
    battery_val_label: gtk::Label,
    tag_label: gtk::Label,
}

impl OverlayWindow {
    pub fn new(app: &adw::Application) -> Rc<Self> {
        let cfg = load_overlay_config();

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title(crate::i18n::t("title"))
            .decorated(false)
            .resizable(false)
            .default_width(560)
            .css_classes(["omen-overlay-window"])
            .build();

        let active_power = Rc::new(RefCell::new("balanced".to_string()));
        let active_fan = Rc::new(RefCell::new("auto".to_string()));
        let power_btns = Rc::new(RefCell::new(Vec::new()));
        let fan_btns = Rc::new(RefCell::new(Vec::new()));

        // Opaque GNOME / Libadwaita surface container
        let surface = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["omen-overlay-surface"])
            .spacing(0)
            .build();

        // ── 1. Native GNOME-style Header Bar (draggable via WindowHandle) ────
        let header_inner = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .css_classes(["overlay-header"])
            .spacing(8)
            .build();

        let brand_icon = gtk::Image::builder()
            .icon_name("omenspace")
            .pixel_size(18)
            .build();
        header_inner.append(&brand_icon);

        let title_label = gtk::Label::builder()
            .label(crate::i18n::t("title"))
            .css_classes(["overlay-title"])
            .hexpand(true)
            .halign(gtk::Align::Start)
            .build();
        header_inner.append(&title_label);

        let tag_label = gtk::Label::builder()
            .label(&cfg.hotkey)
            .css_classes(["overlay-hotkey-cap"])
            .valign(gtk::Align::Center)
            .build();
        header_inner.append(&tag_label);

        let close_btn = gtk::Button::builder()
            .icon_name("window-close-symbolic")
            .tooltip_text(crate::i18n::t("close"))
            .css_classes(["circular", "flat", "overlay-close-btn"])
            .valign(gtk::Align::Center)
            .build();
        let app_c = app.clone();
        close_btn.connect_clicked(move |_| {
            app_c.quit();
        });
        header_inner.append(&close_btn);

        let header_handle = gtk::WindowHandle::new();
        header_handle.set_child(Some(&header_inner));
        surface.append(&header_handle);

        surface.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

        // ── Main Content Body ────────────────────────────────────────────────
        let body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["overlay-body"])
            .spacing(16)
            .build();

        // ── 2. Performance Profiles ──────────────────────────────────────────
        let perf_section = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();

        perf_section.append(
            &gtk::Label::builder()
                .label(crate::i18n::t("perf_mode"))
                .css_classes(["overlay-section-label"])
                .halign(gtk::Align::Start)
                .build(),
        );

        let perf_grid = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .homogeneous(true)
            .build();

        let power_modes = [
            (
                "power-saver",
                crate::i18n::t("quiet"),
                "1",
                crate::i18n::t("eco_silent"),
                "power-profile-power-saver-symbolic",
            ),
            (
                "balanced",
                crate::i18n::t("default"),
                "2",
                crate::i18n::t("balanced"),
                "power-profile-balanced-symbolic",
            ),
            (
                "performance",
                crate::i18n::t("performance"),
                "3",
                crate::i18n::t("max_power"),
                "power-profile-performance-symbolic",
            ),
        ];

        let mut p_btns = Vec::new();
        for (mode_key, name, key_shortcut, desc, icon_name) in power_modes.iter() {
            let btn = build_mode_tile(name, desc, key_shortcut, icon_name);
            p_btns.push(((*mode_key).to_string(), btn.clone()));
            perf_grid.append(&btn);
        }

        perf_section.append(&perf_grid);
        body.append(&perf_section);

        // ── 3. Fan Profiles / Modes ──────────────────────────────────────────
        let fan_section = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();

        fan_section.append(
            &gtk::Label::builder()
                .label(crate::i18n::t("fan_mode"))
                .css_classes(["overlay-section-label"])
                .halign(gtk::Align::Start)
                .build(),
        );

        let fan_grid = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .homogeneous(true)
            .build();

        let fan_modes = [
            (
                "auto",
                "Q",
                crate::i18n::t("auto"),
                crate::i18n::t("auto_desc"),
                "weather-clear-symbolic",
            ),
            (
                "max",
                "W",
                crate::i18n::t("max"),
                crate::i18n::t("max_desc"),
                "weather-storm-symbolic",
            ),
            (
                "custom",
                "E",
                crate::i18n::t("custom"),
                crate::i18n::t("custom_desc"),
                "emblem-system-symbolic",
            ),
        ];

        let mut f_btns = Vec::new();
        for (mode_key, key_shortcut, title, desc, icon_name) in fan_modes.iter() {
            let btn = build_mode_tile(title, desc, key_shortcut, icon_name);
            f_btns.push(((*mode_key).to_string(), btn.clone()));
            fan_grid.append(&btn);
        }

        fan_section.append(&fan_grid);
        body.append(&fan_section);

        // ── 4. Live Telemetry Strip (Opaque Adwaita Card) ────────────────────
        let telemetry_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .css_classes(["overlay-telemetry-card"])
            .spacing(0)
            .build();

        let (cpu_chip, cpu_val) = build_telemetry_cell("CPU", "--°C · --W");
        telemetry_box.append(&cpu_chip);
        telemetry_box.append(&build_vertical_divider());

        let (gpu_chip, gpu_val) = build_telemetry_cell("GPU", "--°C · --W");
        telemetry_box.append(&gpu_chip);
        telemetry_box.append(&build_vertical_divider());

        let (fan_chip, fan_val) = build_telemetry_cell(&crate::i18n::t("fans"), "-- RPM");
        telemetry_box.append(&fan_chip);
        telemetry_box.append(&build_vertical_divider());

        let (ram_chip, ram_val) = build_telemetry_cell("RAM", "-- / -- GB");
        telemetry_box.append(&ram_chip);

        let battery_sep = build_vertical_divider();
        battery_sep.set_visible(false);
        telemetry_box.append(&battery_sep);

        let (battery_chip, battery_val) = build_telemetry_cell(&crate::i18n::t("battery"), "--%");
        battery_chip.set_visible(false);
        telemetry_box.append(&battery_chip);

        body.append(&telemetry_box);

        // ── 5. Footer Shortcut Hints ─────────────────────────────────────────
        let footer = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .css_classes(["overlay-footer"])
            .spacing(16)
            .halign(gtk::Align::Center)
            .build();

        footer.append(&build_shortcut_hint("1–3", &crate::i18n::t("perf_mode")));
        footer.append(&build_shortcut_hint(
            "Q / W / E",
            &crate::i18n::t("fan_mode"),
        ));
        footer.append(&build_shortcut_hint("Esc", &crate::i18n::t("close")));

        body.append(&footer);

        surface.append(&body);
        window.set_content(Some(&surface));

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
            battery_sep,
            battery_chip,
            battery_val_label: battery_val,
            tag_label,
        });

        overlay.apply_config(&cfg);
        overlay.update_button_states();
        overlay.refresh_initial_state();
        overlay.setup_interactions();

        // Periodically sync power profile and fan mode with daemon while visible (#320)
        let overlay_sync = overlay.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(1500), move || {
            if overlay_sync.window.is_visible() {
                overlay_sync.refresh_initial_state();
            }
            glib::ControlFlow::Continue
        });

        overlay
    }

    fn setup_interactions(self: &Rc<Self>) {
        for (name, btn) in self.power_btns.borrow().iter() {
            let this = self.clone();
            let p_name = name.clone();
            btn.connect_clicked(move |_| {
                this.select_power_profile(&p_name);
            });
        }

        for (mode, btn) in self.fan_btns.borrow().iter() {
            let this = self.clone();
            let m_name = mode.clone();
            btn.connect_clicked(move |_| {
                this.select_fan_mode(&m_name);
            });
        }

        let key_controller = gtk::EventControllerKey::new();
        let this = self.clone();
        key_controller.connect_key_pressed(move |_, key, _keycode, _state| {
            let key_val = key.name().unwrap_or_default().to_lowercase();
            match key_val.as_str() {
                "1" | "kp_1" => {
                    this.select_power_profile("power-saver");
                    glib::Propagation::Stop
                }
                "2" | "kp_2" => {
                    this.select_power_profile("balanced");
                    glib::Propagation::Stop
                }
                "3" | "kp_3" => {
                    this.select_power_profile("performance");
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

    #[allow(deprecated)]
    pub fn select_power_profile(self: &Rc<Self>, profile: &str) {
        let normalized = daemon_client::normalize_power_profile(profile);
        *self.active_power.borrow_mut() = normalized.to_string();
        self.update_button_states();

        let (tx, rx) = glib::MainContext::channel::<Option<String>>(glib::Priority::default());
        let this = self.clone();
        rx.attach(None, move |revert_to| {
            if let Some(actual) = revert_to {
                *this.active_power.borrow_mut() =
                    daemon_client::normalize_power_profile(&actual).to_string();
                this.update_button_states();
            }
            glib::ControlFlow::Break
        });

        let prof = normalized.to_string();
        let rt = daemon_client::get_runtime();
        rt.spawn(async move {
            let failed = !matches!(
                daemon_client::set_power_profile(&prof).await,
                Ok(resp) if !resp.starts_with("FAIL")
            );
            if failed {
                let actual = daemon_client::get_power_profile().await;
                let _ = tx.send(Some(actual));
            } else {
                let _ = tx.send(None);
            }
        });
    }

    pub fn select_fan_mode(self: &Rc<Self>, mode: &str) {
        let normalized = daemon_client::normalize_fan_mode(mode);
        *self.active_fan.borrow_mut() = normalized.to_string();
        self.update_button_states();

        let m = normalized.to_string();
        let rt = daemon_client::get_runtime();
        rt.spawn(async move {
            let _ = daemon_client::set_fan_mode(&m).await;
        });
    }

    pub fn update_button_states(&self) {
        let cur_power = daemon_client::normalize_power_profile(&self.active_power.borrow());
        for (mode_key, btn) in self.power_btns.borrow().iter() {
            let is_active = daemon_client::normalize_power_profile(mode_key) == cur_power;
            if is_active {
                btn.add_css_class("active");
            } else {
                btn.remove_css_class("active");
            }
        }

        let cur_fan = daemon_client::normalize_fan_mode(&self.active_fan.borrow());
        for (mode_key, btn) in self.fan_btns.borrow().iter() {
            let is_active = daemon_client::normalize_fan_mode(mode_key) == cur_fan;
            if is_active {
                btn.add_css_class("active");
            } else {
                btn.remove_css_class("active");
            }
        }
    }

    pub fn update_telemetry(&self, stats: &SystemStats) {
        self.cpu_val_label
            .set_label(&format!("{}°C · {:.0}W", stats.cpu_temp, stats.cpu_pwr));

        // The daemon stops polling an idle dGPU so it can sleep; say so
        // instead of showing placeholder dashes that look like an error.
        let gpu_idle = matches!(
            stats.gpu_sample_state,
            GpuSampleState::Deferred | GpuSampleState::Suspended
        ) || (stats.gpu_sample_state == GpuSampleState::Unknown
            && stats.gpu_pwr < 0.0);
        let gpu_t = if gpu_idle {
            crate::i18n::t("gpu_idle")
        } else if stats.gpu_temp > 0 {
            format!("{}°C", stats.gpu_temp)
        } else {
            "--".to_string()
        };
        if gpu_idle {
            self.gpu_val_label.set_label(&gpu_t);
        } else {
            let gpu_w = if stats.gpu_pwr >= 0.0 {
                format!("{:.0}W", stats.gpu_pwr)
            } else {
                "--".to_string()
            };
            self.gpu_val_label
                .set_label(&format!("{} · {}", gpu_t, gpu_w));
        }

        if stats.fan1_rpm > 0 || stats.fan2_rpm > 0 {
            self.fan_val_label
                .set_label(&format!("{} / {}", stats.fan1_rpm, stats.fan2_rpm));
        } else if stats.fan_rpm > 0 {
            self.fan_val_label
                .set_label(&format!("{} RPM", stats.fan_rpm));
        } else {
            self.fan_val_label.set_label("Auto");
        }

        if stats.ram_total_gb > 0.0 {
            self.ram_val_label.set_label(&format!(
                "{:.1} / {:.0} GB",
                stats.ram_used_gb, stats.ram_total_gb
            ));
        }

        if let Some(ref bat) = stats.battery {
            self.battery_sep.set_visible(true);
            self.battery_chip.set_visible(true);
            let pwr_str = if bat.power_w > 0.0 {
                format!(" · {:.0}W", bat.power_w)
            } else if bat.ac_online {
                " · AC".to_string()
            } else {
                String::new()
            };
            self.battery_val_label
                .set_label(&format!("{}%{}", bat.charge_pct, pwr_str));
        } else {
            self.battery_sep.set_visible(false);
            self.battery_chip.set_visible(false);
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

    fn apply_config(&self, cfg: &OverlayConfig) {
        self.tag_label.set_label(&cfg.hotkey);
    }
}

fn build_mode_tile(title: &str, desc: &str, key_shortcut: &str, icon_name: &str) -> gtk::Button {
    let btn = gtk::Button::builder().css_classes(["overlay-tile"]).build();

    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();

    let top_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();

    let icon = gtk::Image::builder()
        .icon_name(icon_name)
        .pixel_size(16)
        .css_classes(["tile-icon"])
        .build();
    top_row.append(&icon);

    let lbl = gtk::Label::builder()
        .label(title)
        .css_classes(["tile-title"])
        .hexpand(true)
        .halign(gtk::Align::Start)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    top_row.append(&lbl);

    let badge = gtk::Label::builder()
        .label(key_shortcut)
        .css_classes(["tile-keycap"])
        .valign(gtk::Align::Center)
        .build();
    top_row.append(&badge);

    inner.append(&top_row);

    let sub = gtk::Label::builder()
        .label(desc)
        .css_classes(["tile-subtitle"])
        .halign(gtk::Align::Start)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    inner.append(&sub);

    btn.set_child(Some(&inner));
    btn
}

fn build_telemetry_cell(title: &str, initial_val: &str) -> (gtk::Box, gtk::Label) {
    let cell = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .css_classes(["telemetry-cell"])
        .build();
    cell.append(
        &gtk::Label::builder()
            .label(title)
            .css_classes(["telemetry-label"])
            .halign(gtk::Align::Start)
            .build(),
    );
    let val = gtk::Label::builder()
        .label(initial_val)
        .css_classes(["telemetry-value"])
        .halign(gtk::Align::Start)
        .build();
    cell.append(&val);
    (cell, val)
}

fn build_vertical_divider() -> gtk::Separator {
    let sep = gtk::Separator::new(gtk::Orientation::Vertical);
    sep.add_css_class("telemetry-sep");
    sep
}

fn build_shortcut_hint(key: &str, label: &str) -> gtk::Box {
    let hint = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .valign(gtk::Align::Center)
        .build();
    hint.append(
        &gtk::Label::builder()
            .label(key)
            .css_classes(["shortcut-key"])
            .build(),
    );
    hint.append(
        &gtk::Label::builder()
            .label(label)
            .css_classes(["shortcut-hint"])
            .build(),
    );
    hint
}
