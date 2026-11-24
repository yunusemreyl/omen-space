use gtk::prelude::*;
use libadwaita as adw;
use adw::prelude::*;
use std::fs;
use crate::i18n;

/* ─────────────────────────────────────────────────────────────
   mux.rs — GPU MUX Switch Page
   ───────────────────────────────────────────────────────────── */

fn get_nvidia_driver_version() -> String {
    if let Ok(content) = fs::read_to_string("/proc/driver/nvidia/version") {
        for line in content.lines() {
            if line.contains("NVRM version:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                for part in parts {
                    if part.contains('.') && part.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                        return format!("{} (NVIDIA Open Kernel)", part);
                    }
                }
            }
        }
    }
    "Unknown".to_string()
}

fn detect_active_display_gpu() -> (String, bool, String, String) {
    let specs = crate::daemon_client::get_hardware_specs_sync();
    let gpu_name = specs.gpu_spec.split("  ·  ").next().unwrap_or("NVIDIA GPU").to_string();
    let is_amd = specs.cpu_spec.to_lowercase().contains("ryzen") || specs.cpu_spec.to_lowercase().contains("amd");
    let igpu_name = if is_amd { "AMD Radeon Graphics".to_string() } else { "Intel Integrated Graphics".to_string() };

    if let Ok(entries) = glob::glob("/sys/class/drm/card[0-9]*-*eDP-1*/device/vendor") {
        for entry in entries.filter_map(Result::ok) {
            if let Ok(vendor) = fs::read_to_string(entry) {
                if vendor.trim().to_lowercase() == "0x10de" {
                    return (format!("eDP-1 → {} (Discrete)", gpu_name), true, gpu_name, igpu_name);
                }
            }
        }
    }
    if let Ok(entries) = glob::glob("/sys/class/drm/card[0-9]*-*eDP-1*/device/device/vendor") {
        for entry in entries.filter_map(Result::ok) {
            if let Ok(vendor) = fs::read_to_string(entry) {
                if vendor.trim().to_lowercase() == "0x10de" {
                    return (format!("eDP-1 → {} (Discrete)", gpu_name), true, gpu_name, igpu_name);
                }
            }
        }
    }
    (format!("eDP-1 → {} (Hybrid)", igpu_name), false, gpu_name, igpu_name)
}

fn read_firmware_mux_mode_sync() -> Option<String> {
    for path in [
        "/sys/devices/platform/hp-wmi/gpu_mux_mode",
        "/sys/devices/platform/hp-wmi/graphics_mode",
    ] {
        if let Ok(raw) = fs::read_to_string(path) {
            match raw.trim() {
                "0" | "hybrid" => return Some("hybrid".to_string()),
                "1" | "discrete" => return Some("discrete".to_string()),
                "2" | "advanced" | "optimus" => return Some("advanced".to_string()),
                "3" | "integrated" => return Some("integrated".to_string()),
                _ => {}
            }
        }
    }
    None
}

fn format_mux_mode_label(mode: &str) -> String {
    match mode {
        "discrete" => "Discrete (dGPU)".to_string(),
        "advanced" => "Advanced Optimus (Dynamic)".to_string(),
        "integrated" => "Integrated (iGPU)".to_string(),
        _ => "Hybrid (MSHybrid)".to_string(),
    }
}

pub fn build_page() -> gtk::Box {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .build();

    let drv_version = get_nvidia_driver_version();
    let (disp_info, is_discrete_active, gpu_name, igpu_name) = detect_active_display_gpu();
    let initial_fw_mode = read_firmware_mux_mode_sync()
        .unwrap_or_else(|| if is_discrete_active { "discrete".to_string() } else { "hybrid".to_string() });

    // ── Header ───────────────────────────────────────────────
    let hdr = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_bottom(4)
        .build();
    hdr.append(&gtk::Label::builder()
        .label(i18n::t("title_mux"))
        .css_classes(["page-title"])
        .halign(gtk::Align::Start)
        .build());
    hdr.append(&gtk::Label::builder()
        .label(i18n::t("mux_desc"))
        .css_classes(["os-section-desc"])
        .halign(gtk::Align::Start)
        .build());
    page.append(&hdr);

    // ── 3 Main Selection Cards ────────────────────────────────
    let modes_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(16)
        .homogeneous(true)
        .build();

    // 1. Hybrid Card
    let (btn_hybrid, wrap_hybrid) = build_simple_mux_card(
        &crate::asset_resolver::get_asset_path("balanced.svg"),
        i18n::t("hybrid_title"),
        i18n::t("hybrid_mode"),
        i18n::t("hybrid_desc"),
    );

    // 2. Discrete Card
    let (btn_discrete, wrap_discrete) = build_simple_mux_card(
        &crate::asset_resolver::get_asset_path("performance.svg"),
        i18n::t("discrete_title"),
        i18n::t("discrete_mode"),
        i18n::t("discrete_desc"),
    );

    // 3. Advanced Card
    let (btn_advanced, wrap_advanced) = build_simple_mux_card(
        &crate::asset_resolver::get_asset_path("omen-gpu-symbolic.svg"),
        i18n::t("advanced_title"),
        i18n::t("advanced_mode"),
        i18n::t("advanced_desc"),
    );

    btn_discrete.set_group(Some(&btn_hybrid));
    btn_advanced.set_group(Some(&btn_hybrid));
    match initial_fw_mode.as_str() {
        "discrete" => btn_discrete.set_active(true),
        "advanced" => btn_advanced.set_active(true),
        _ => btn_hybrid.set_active(true),
    }

    modes_box.append(&wrap_hybrid);
    modes_box.append(&wrap_discrete);
    modes_box.append(&wrap_advanced);
    page.append(&modes_box);

    // ── Reboot Warning Card ───────────────────────────────────
    let warn_card = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .css_classes(["os-card"])
        .margin_top(4)
        .visible(false)
        .build();

    let warn_icon = gtk::Image::builder()
        .icon_name("dialog-warning-symbolic")
        .pixel_size(20)
        .valign(gtk::Align::Center)
        .build();
    warn_card.append(&warn_icon);

    let warn_txt = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .build();
    warn_txt.append(&gtk::Label::builder()
        .label(i18n::t("restart_required"))
        .css_classes(["chip-title"])
        .halign(gtk::Align::Start)
        .build());
    warn_txt.append(&gtk::Label::builder()
        .label(i18n::t("restart_desc"))
        .css_classes(["os-section-desc"])
        .halign(gtk::Align::Start)
        .build());
    warn_card.append(&warn_txt);

    let reboot_btn = gtk::Button::builder()
        .label(i18n::t("restart_now"))
        .css_classes(["suggested-action"])
        .valign(gtk::Align::Center)
        .build();
    
    reboot_btn.connect_clicked(|_| {
        if std::path::Path::new("/run/systemd/system").exists() {
            let _ = std::process::Command::new("systemctl").arg("reboot").spawn();
        } else if std::process::Command::new("loginctl").arg("reboot").spawn().is_err() {
            let _ = std::process::Command::new("pkexec").arg("reboot").spawn();
        }
    });

    warn_card.append(&reboot_btn);
    page.append(&warn_card);

    // ── GPU & Display Details ─────────────────────────────────
    let info_group = adw::PreferencesGroup::builder()
        .title(i18n::t("gpu_details_group"))
        .build();

    let fw_mode_row = adw::ActionRow::builder()
        .title(i18n::t("mux_fw_mode_row"))
        .subtitle(i18n::t("mux_fw_mode_sub"))
        .build();
    let fw_mode_val = gtk::Label::builder()
        .label(format_mux_mode_label(&initial_fw_mode))
        .css_classes(if initial_fw_mode == "discrete" { ["badge-warn"] } else { ["badge-ok"] })
        .valign(gtk::Align::Center)
        .build();
    fw_mode_row.add_suffix(&fw_mode_val);
    info_group.add(&fw_mode_row);

    let disp_row = adw::ActionRow::builder()
        .title(i18n::t("disp_out_row"))
        .subtitle(i18n::t("disp_out_sub"))
        .build();
    let disp_val = gtk::Label::builder()
        .label(&disp_info)
        .css_classes(if is_discrete_active { ["badge-warn"] } else { ["badge-ok"] })
        .valign(gtk::Align::Center)
        .build();
    disp_row.add_suffix(&disp_val);
    info_group.add(&disp_row);

    let specs = crate::daemon_client::get_hardware_specs_sync();

    let gpu_row = adw::ActionRow::builder()
        .title(i18n::t("active_gpu"))
        .subtitle(i18n::t("dgpu_sub"))
        .build();
    let gpu_val = gtk::Label::builder()
        .label(&specs.gpu_spec)
        .css_classes(["os-section-desc"])
        .valign(gtk::Align::Center)
        .build();
    gpu_row.add_suffix(&gpu_val);
    info_group.add(&gpu_row);

    let drv_row = adw::ActionRow::builder()
        .title(i18n::t("driver_ver"))
        .subtitle(i18n::t("driver_sub"))
        .build();
    let drv_val = gtk::Label::builder()
        .label(&drv_version)
        .css_classes(["os-section-desc"])
        .valign(gtk::Align::Center)
        .build();
    drv_row.add_suffix(&drv_val);
    info_group.add(&drv_row);

    page.append(&info_group);

    let updating_ui = std::rc::Rc::new(std::cell::Cell::new(false));
    let boot_fw_mode = std::rc::Rc::new(std::cell::RefCell::new(initial_fw_mode));

    // Wire MUX toggles to daemon and verify firmware write result (#369)
    let wire_mux_toggle = |btn: &gtk::ToggleButton,
                           mode_key: &'static str,
                           pending_lbl: &'static str,
                           confirmed_lbl: &'static str,
                           css_class: &'static str| {
        let w_c = warn_card.clone();
        let fw_val = fw_mode_val.clone();
        let upd = updating_ui.clone();
        let boot_m = boot_fw_mode.clone();
        let bh = btn_hybrid.clone();
        let bd = btn_discrete.clone();
        let ba = btn_advanced.clone();
        btn.connect_toggled(move |b| {
            if !b.is_active() || upd.get() {
                return;
            }
            let w_c = w_c.clone();
            let fw_val = fw_val.clone();
            let upd = upd.clone();
            let boot_m = boot_m.clone();
            let bh = bh.clone();
            let bd = bd.clone();
            let ba = ba.clone();
            glib::spawn_future_local(async move {
                let res = crate::daemon_client::set_gpu_mode_async(mode_key.to_string()).await;
                if matches!(res.as_deref(), Ok("OK_REBOOT_REQUIRED")) {
                    let pending = *boot_m.borrow() != mode_key;
                    w_c.set_visible(pending);
                    fw_val.set_label(if pending { pending_lbl } else { confirmed_lbl });
                    fw_val.set_css_classes(&[css_class]);
                } else {
                    // Firmware/WMI rejected the switch: revert toggle and show clear status (#369)
                    let prev = boot_m.borrow().clone();
                    upd.set(true);
                    match prev.as_str() {
                        "discrete" => bd.set_active(true),
                        "advanced" => ba.set_active(true),
                        _ => bh.set_active(true),
                    }
                    upd.set(false);
                    w_c.set_visible(false);
                    fw_val.set_label(i18n::t("mux_switch_failed"));
                    fw_val.set_css_classes(&["badge-warn"]);
                }
            });
        });
    };

    wire_mux_toggle(
        &btn_discrete,
        "discrete",
        "Discrete (dGPU · Pending Reboot)",
        "Discrete (dGPU)",
        "badge-warn",
    );
    wire_mux_toggle(
        &btn_hybrid,
        "hybrid",
        "Hybrid (MSHybrid · Pending Reboot)",
        "Hybrid (MSHybrid)",
        "badge-ok",
    );
    wire_mux_toggle(
        &btn_advanced,
        "advanced",
        "Advanced Optimus (Pending Reboot)",
        "Advanced Optimus (Dynamic)",
        "badge-ok",
    );

    let bd_load = btn_discrete.clone();
    let bh_load = btn_hybrid.clone();
    let ba_load = btn_advanced.clone();
    let fw_val_load = fw_mode_val.clone();
    let disp_val_load = disp_val.clone();
    let upd_load = updating_ui.clone();
    let boot_m_load = boot_fw_mode.clone();
    glib::spawn_future_local(async move {
        if let Ok(json) = crate::daemon_client::get_gpu_info_async().await {
            if let Ok(info) = serde_json::from_str::<serde_json::Value>(&json) {
                let confirmed_mode = info
                    .get("mode")
                    .and_then(|v| v.as_str())
                    .unwrap_or("hybrid");
                let pending_mode = info
                    .get("pending_mode")
                    .and_then(|v| v.as_str());
                let effective_sel = pending_mode.unwrap_or(confirmed_mode);

                upd_load.set(true);
                *boot_m_load.borrow_mut() = confirmed_mode.to_string();
                if effective_sel == "discrete" {
                    bd_load.set_active(true);
                } else if effective_sel == "advanced" {
                    ba_load.set_active(true);
                } else {
                    bh_load.set_active(true);
                }
                let is_pending = pending_mode.is_some_and(|p| p != confirmed_mode);
                warn_card.set_visible(is_pending);
                if is_pending {
                    let pending_label = match effective_sel {
                        "discrete" => "Discrete (dGPU · Pending Reboot)",
                        "advanced" => "Advanced Optimus (Pending Reboot)",
                        _ => "Hybrid (MSHybrid · Pending Reboot)",
                    };
                    fw_val_load.set_label(pending_label);
                    fw_val_load.set_css_classes(&["badge-warn"]);
                } else {
                    fw_val_load.set_label(&format_mux_mode_label(confirmed_mode));
                    fw_val_load.set_css_classes(if confirmed_mode == "discrete" {
                        &["badge-warn"]
                    } else {
                        &["badge-ok"]
                    });
                }
                upd_load.set(false);
                if let Some(routing) = info.get("routing_mode").and_then(|v| v.as_str()) {
                    if routing == "discrete" {
                        disp_val_load.set_label(&format!("eDP-1 → {} (Discrete)", gpu_name));
                        disp_val_load.set_css_classes(&["badge-warn"]);
                    } else if routing == "hybrid" || routing == "integrated" {
                        disp_val_load.set_label(&format!("eDP-1 → {} (Hybrid)", igpu_name));
                        disp_val_load.set_css_classes(&["badge-ok"]);
                    }
                }
            }
        }
    });

    page
}

/// Simple, modern rectangular MUX card.
fn build_simple_mux_card(
    icon_path: &str,
    title: &str,
    subtitle: &str,
    desc: &str,
) -> (gtk::ToggleButton, gtk::Box) {
    let btn = gtk::ToggleButton::builder()
        .css_classes(["chip-card"])
        .build();

    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .halign(gtk::Align::Fill)
        .valign(gtk::Align::Center)
        .build();

    let top_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();

    top_row.append(&gtk::Image::builder().file(icon_path).pixel_size(28).build());

    let text_col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    text_col.append(&gtk::Label::builder()
        .label(title)
        .css_classes(["chip-title"])
        .halign(gtk::Align::Start)
        .build());
    text_col.append(&gtk::Label::builder()
        .label(subtitle)
        .css_classes(["chip-sub"])
        .halign(gtk::Align::Start)
        .build());
    top_row.append(&text_col);
    inner.append(&top_row);

    inner.append(&gtk::Label::builder()
        .label(desc)
        .css_classes(["os-section-desc"])
        .halign(gtk::Align::Start)
        .wrap(true)
        .build());

    btn.set_child(Some(&inner));

    let wrap = gtk::Box::builder().orientation(gtk::Orientation::Vertical).build();
    wrap.append(&btn);
    (btn, wrap)
}
