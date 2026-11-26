use crate::i18n;
use adw::prelude::*;
use gtk::prelude::*;
use libadwaita as adw;
use std::fs;

/* ─────────────────────────────────────────────────────────────
mux.rs — Clean GPU MUX & PRIME Offload Page
───────────────────────────────────────────────────────────── */

fn get_nvidia_driver_version() -> String {
    if let Ok(content) = fs::read_to_string("/proc/driver/nvidia/version") {
        for line in content.lines() {
            if line.contains("NVRM version:") {
                for part in line.split_whitespace() {
                    if part.contains('.') && part.chars().next().is_some_and(|c| c.is_ascii_digit())
                    {
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
    let gpu_name = specs
        .gpu_spec
        .split("  ·  ")
        .next()
        .unwrap_or("NVIDIA GPU")
        .to_string();
    let is_amd = specs.cpu_spec.to_lowercase().contains("ryzen")
        || specs.cpu_spec.to_lowercase().contains("amd");
    let igpu_name = if is_amd {
        "AMD Radeon".to_string()
    } else {
        "Intel Graphics".to_string()
    };

    for pattern in [
        "/sys/class/drm/card[0-9]*-*eDP*/device/vendor",
        "/sys/class/drm/card[0-9]*-*eDP*/device/device/vendor",
    ] {
        if let Ok(entries) = glob::glob(pattern) {
            for entry in entries.filter_map(Result::ok) {
                if let Ok(vendor) = fs::read_to_string(entry) {
                    if vendor.trim().to_lowercase() == "0x10de" {
                        return ("eDP → NVIDIA".to_string(), true, gpu_name, igpu_name);
                    }
                }
            }
        }
    }
    (format!("eDP → {}", igpu_name), false, gpu_name, igpu_name)
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

fn read_supported_modes_sync() -> Option<Vec<String>> {
    let raw = fs::read_to_string("/sys/devices/platform/hp-wmi/gpu_mux_supported_modes").ok()?;
    let modes: Vec<String> = raw
        .split_whitespace()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    (!modes.is_empty()).then_some(modes)
}

fn format_mux_mode_label(mode: &str) -> String {
    match mode {
        "discrete" => "Discrete (dGPU)".to_string(),
        "advanced" => "Advanced Optimus".to_string(),
        "integrated" => "Integrated (iGPU)".to_string(),
        _ => "Hybrid (MSHybrid)".to_string(),
    }
}

fn format_pending_mux_label(mode: &str) -> &'static str {
    match mode {
        "discrete" => "Discrete · Pending Reboot",
        "advanced" => "Advanced Optimus · Pending Reboot",
        "integrated" => "Integrated · Pending Reboot",
        _ => "Hybrid · Pending Reboot",
    }
}

pub fn build_page() -> gtk::Box {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .build();

    let drv_version = get_nvidia_driver_version();
    let specs = crate::daemon_client::get_hardware_specs_sync();
    let (disp_badge_init, is_discrete_active, gpu_name, igpu_name) = detect_active_display_gpu();
    let initial_fw_mode = read_firmware_mux_mode_sync().unwrap_or_else(|| {
        if is_discrete_active {
            "discrete".to_string()
        } else {
            "hybrid".to_string()
        }
    });
    let initial_supported = read_supported_modes_sync();
    let initial_supports_advanced = initial_fw_mode == "advanced"
        || initial_supported
            .as_ref()
            .is_some_and(|m| m.iter().any(|x| x == "advanced"));
    let initial_supports_integrated = initial_fw_mode == "integrated"
        || initial_supported
            .as_ref()
            .is_some_and(|m| m.iter().any(|x| x == "integrated"));

    // ── Header + Active Mode Pill ────────────────────────────
    let hdr_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_bottom(4)
        .build();

    let hdr_txt = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .hexpand(true)
        .build();
    hdr_txt.append(
        &gtk::Label::builder()
            .label(i18n::t("title_mux"))
            .css_classes(["page-title"])
            .halign(gtk::Align::Start)
            .build(),
    );
    hdr_txt.append(
        &gtk::Label::builder()
            .label(i18n::t("mux_desc"))
            .css_classes(["os-section-desc"])
            .halign(gtk::Align::Start)
            .wrap(true)
            .build(),
    );
    hdr_row.append(&hdr_txt);

    let fw_mode_badge = gtk::Label::builder()
        .label(format_mux_mode_label(&initial_fw_mode))
        .css_classes(if initial_fw_mode == "discrete" {
            ["badge-warn"]
        } else {
            ["badge-ok"]
        })
        .valign(gtk::Align::Center)
        .build();
    hdr_row.append(&fw_mode_badge);
    page.append(&hdr_row);

    // ── No Hardware MUX Banner (only when available == false) ─
    let no_mux_card = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .css_classes(["os-card"])
        .visible(false)
        .build();
    no_mux_card.append(
        &gtk::Image::builder()
            .icon_name("dialog-information-symbolic")
            .pixel_size(20)
            .valign(gtk::Align::Center)
            .build(),
    );
    let no_mux_txt = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .build();
    no_mux_txt.append(
        &gtk::Label::builder()
            .label(i18n::t("mux_not_available_title"))
            .css_classes(["chip-title"])
            .halign(gtk::Align::Start)
            .build(),
    );
    no_mux_txt.append(
        &gtk::Label::builder()
            .label(i18n::t("mux_not_available_desc"))
            .css_classes(["os-section-desc"])
            .halign(gtk::Align::Start)
            .wrap(true)
            .build(),
    );
    no_mux_card.append(&no_mux_txt);
    page.append(&no_mux_card);

    // ── Compact MUX Mode Cards (gated by hardware support) ───
    let modes_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .homogeneous(true)
        .build();

    let (btn_hybrid, wrap_hybrid) = build_compact_mux_card(
        &crate::asset_resolver::get_asset_path("balanced.svg"),
        i18n::t("hybrid_title"),
        i18n::t("hybrid_mode"),
    );

    let (btn_discrete, wrap_discrete) = build_compact_mux_card(
        &crate::asset_resolver::get_asset_path("performance.svg"),
        i18n::t("discrete_title"),
        i18n::t("discrete_mode"),
    );

    let (btn_advanced, wrap_advanced) = build_compact_mux_card(
        &crate::asset_resolver::get_asset_path("omen-gpu-symbolic.svg"),
        i18n::t("advanced_title"),
        i18n::t("advanced_mode"),
    );
    wrap_advanced.set_visible(initial_supports_advanced);

    let (btn_integrated, wrap_integrated) = build_compact_mux_card(
        &crate::asset_resolver::get_asset_path("eco.svg"),
        i18n::t("integrated_title"),
        i18n::t("integrated_mode"),
    );
    wrap_integrated.set_visible(initial_supports_integrated);

    btn_discrete.set_group(Some(&btn_hybrid));
    btn_advanced.set_group(Some(&btn_hybrid));
    btn_integrated.set_group(Some(&btn_hybrid));
    match initial_fw_mode.as_str() {
        "discrete" => btn_discrete.set_active(true),
        "advanced" => btn_advanced.set_active(true),
        "integrated" => btn_integrated.set_active(true),
        _ => btn_hybrid.set_active(true),
    }

    modes_box.append(&wrap_hybrid);
    modes_box.append(&wrap_discrete);
    modes_box.append(&wrap_advanced);
    modes_box.append(&wrap_integrated);
    page.append(&modes_box);

    // ── Reboot Warning Banner (shown only on pending change) ─
    let warn_card = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .css_classes(["os-card"])
        .visible(false)
        .build();
    warn_card.append(
        &gtk::Image::builder()
            .icon_name("dialog-warning-symbolic")
            .pixel_size(20)
            .valign(gtk::Align::Center)
            .build(),
    );
    let warn_txt = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .build();
    warn_txt.append(
        &gtk::Label::builder()
            .label(i18n::t("restart_required"))
            .css_classes(["chip-title"])
            .halign(gtk::Align::Start)
            .build(),
    );
    warn_txt.append(
        &gtk::Label::builder()
            .label(i18n::t("restart_desc"))
            .css_classes(["os-section-desc"])
            .halign(gtk::Align::Start)
            .build(),
    );
    warn_card.append(&warn_txt);

    let reboot_btn = gtk::Button::builder()
        .label(i18n::t("restart_now"))
        .css_classes(["suggested-action"])
        .valign(gtk::Align::Center)
        .build();
    reboot_btn.connect_clicked(|_| {
        if std::path::Path::new("/run/systemd/system").exists() {
            let _ = std::process::Command::new("systemctl")
                .arg("reboot")
                .spawn();
        } else if std::process::Command::new("loginctl")
            .arg("reboot")
            .spawn()
            .is_err()
        {
            let _ = std::process::Command::new("pkexec").arg("reboot").spawn();
        }
    });
    warn_card.append(&reboot_btn);
    page.append(&warn_card);

    // ── Unified Hardware & Display Group (max 4 clean rows) ──
    let hw_group = adw::PreferencesGroup::builder()
        .title(i18n::t("mux_hardware_group"))
        .build();

    // 1. Graphics Hardware & Driver
    let gpu_row = adw::ActionRow::builder()
        .title(i18n::t("active_gpu"))
        .subtitle(&drv_version)
        .build();
    let gpu_val = gtk::Label::builder()
        .label(&specs.gpu_spec)
        .css_classes(["os-section-desc"])
        .valign(gtk::Align::Center)
        .build();
    gpu_row.add_suffix(&gpu_val);
    hw_group.add(&gpu_row);

    // 2. Display Outputs + eDP Routing Badge
    let displays_row = adw::ActionRow::builder()
        .title(i18n::t("mux_displays_row"))
        .subtitle(&disp_badge_init)
        .build();
    let disp_routing_badge = gtk::Label::builder()
        .label(&disp_badge_init)
        .css_classes(if is_discrete_active {
            ["badge-warn"]
        } else {
            ["badge-ok"]
        })
        .valign(gtk::Align::Center)
        .build();
    displays_row.add_suffix(&disp_routing_badge);
    hw_group.add(&displays_row);

    // 3. NVIDIA dGPU Power State (passive read; hidden if unavailable)
    let dgpu_power_row = adw::ActionRow::builder()
        .title(i18n::t("mux_dgpu_power_row"))
        .subtitle(i18n::t("mux_dgpu_power_sub"))
        .visible(false)
        .build();
    let dgpu_power_val = gtk::Label::builder().valign(gtk::Align::Center).build();
    dgpu_power_row.add_suffix(&dgpu_power_val);
    hw_group.add(&dgpu_power_row);

    // 4. Steam / App PRIME Launch Option (visible in Hybrid / Advanced Optimus)
    let prime_cmd = std::rc::Rc::new(std::cell::RefCell::new(
        "__NV_PRIME_RENDER_OFFLOAD=1 __GLX_VENDOR_LIBRARY_NAME=nvidia __VK_LAYER_NV_optimus=NVIDIA_only %command%".to_string(),
    ));
    let prime_row = adw::ActionRow::builder()
        .title(i18n::t("mux_prime_row"))
        .subtitle(prime_cmd.borrow().as_str())
        .subtitle_lines(1)
        .visible(false)
        .build();
    let copy_prime_btn = gtk::Button::builder()
        .label(i18n::t("mux_copy_btn"))
        .valign(gtk::Align::Center)
        .build();
    {
        let cmd_ref = prime_cmd.clone();
        let btn_ref = copy_prime_btn.clone();
        copy_prime_btn.connect_clicked(move |_| {
            if let Some(display) = gtk::gdk::Display::default() {
                display.clipboard().set_text(cmd_ref.borrow().as_str());
                btn_ref.set_label(i18n::t("mux_copied"));
                let btn_reset = btn_ref.clone();
                glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
                    btn_reset.set_label(i18n::t("mux_copy_btn"));
                });
            }
        });
    }
    prime_row.add_suffix(&copy_prime_btn);
    hw_group.add(&prime_row);

    page.append(&hw_group);

    let updating_ui = std::rc::Rc::new(std::cell::Cell::new(false));
    let boot_fw_mode = std::rc::Rc::new(std::cell::RefCell::new(initial_fw_mode));

    let wire_mux_toggle = |btn: &gtk::ToggleButton,
                           mode_key: &'static str,
                           pending_lbl: &'static str,
                           confirmed_lbl: &'static str,
                           css_class: &'static str| {
        let w_c = warn_card.clone();
        let badge = fw_mode_badge.clone();
        let upd = updating_ui.clone();
        let boot_m = boot_fw_mode.clone();
        let bh = btn_hybrid.clone();
        let bd = btn_discrete.clone();
        let ba = btn_advanced.clone();
        let bi = btn_integrated.clone();
        btn.connect_toggled(move |b| {
            if !b.is_active() || upd.get() {
                return;
            }
            let w_c = w_c.clone();
            let badge = badge.clone();
            let upd = upd.clone();
            let boot_m = boot_m.clone();
            let bh = bh.clone();
            let bd = bd.clone();
            let ba = ba.clone();
            let bi = bi.clone();
            glib::spawn_future_local(async move {
                let res = crate::daemon_client::set_gpu_mode_async(mode_key.to_string()).await;
                if matches!(res.as_deref(), Ok("OK_REBOOT_REQUIRED")) {
                    let pending = *boot_m.borrow() != mode_key;
                    w_c.set_visible(pending);
                    badge.set_label(if pending { pending_lbl } else { confirmed_lbl });
                    badge.set_css_classes(&[if pending { "badge-warn" } else { css_class }]);
                } else {
                    let unsupported = res.as_deref().is_ok_and(|s| s.contains("UNSUPPORTED_MODE"));
                    let prev = boot_m.borrow().clone();
                    upd.set(true);
                    match prev.as_str() {
                        "discrete" => bd.set_active(true),
                        "advanced" => ba.set_active(true),
                        "integrated" => bi.set_active(true),
                        _ => bh.set_active(true),
                    }
                    upd.set(false);
                    w_c.set_visible(false);
                    badge.set_label(i18n::t(if unsupported {
                        "mux_mode_unsupported"
                    } else {
                        "mux_switch_failed"
                    }));
                    badge.set_css_classes(&["badge-warn"]);
                }
            });
        });
    };

    wire_mux_toggle(
        &btn_discrete,
        "discrete",
        "Discrete · Pending Reboot",
        "Discrete (dGPU)",
        "badge-warn",
    );
    wire_mux_toggle(
        &btn_hybrid,
        "hybrid",
        "Hybrid · Pending Reboot",
        "Hybrid (MSHybrid)",
        "badge-ok",
    );
    wire_mux_toggle(
        &btn_advanced,
        "advanced",
        "Advanced Optimus · Pending Reboot",
        "Advanced Optimus",
        "badge-ok",
    );
    wire_mux_toggle(
        &btn_integrated,
        "integrated",
        "Integrated · Pending Reboot",
        "Integrated (iGPU)",
        "badge-ok",
    );

    let bd_load = btn_discrete.clone();
    let bh_load = btn_hybrid.clone();
    let ba_load = btn_advanced.clone();
    let bi_load = btn_integrated.clone();
    let wh_load = wrap_hybrid.clone();
    let wd_load = wrap_discrete.clone();
    let wa_load = wrap_advanced.clone();
    let wi_load = wrap_integrated.clone();
    let modes_box_load = modes_box.clone();
    let no_mux_load = no_mux_card.clone();
    let badge_load = fw_mode_badge.clone();
    let disp_badge_load = disp_routing_badge.clone();
    let displays_row_load = displays_row.clone();
    let dgpu_row_load = dgpu_power_row.clone();
    let dgpu_val_load = dgpu_power_val.clone();
    let prime_row_load = prime_row.clone();
    let prime_cmd_load = prime_cmd.clone();
    let upd_load = updating_ui.clone();
    let boot_m_load = boot_fw_mode.clone();

    glib::spawn_future_local(async move {
        if let Ok(json) = crate::daemon_client::get_gpu_info_async().await {
            if let Ok(info) = serde_json::from_str::<serde_json::Value>(&json) {
                let available = info
                    .get("available")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let confirmed_mode = info
                    .get("mode")
                    .and_then(|v| v.as_str())
                    .unwrap_or("hybrid");
                let boot_mode = info
                    .get("boot_mode")
                    .and_then(|v| v.as_str())
                    .unwrap_or(confirmed_mode);
                let pending_mode = info.get("pending_mode").and_then(|v| v.as_str());
                let effective_sel = pending_mode.unwrap_or(confirmed_mode);
                let routing = info
                    .get("routing_mode")
                    .and_then(|v| v.as_str())
                    .unwrap_or("hybrid");

                let supported: Vec<&str> = info
                    .get("supported_modes")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|x| x.as_str()).collect())
                    .unwrap_or_else(|| vec!["hybrid", "discrete"]);
                let supports_ao = info
                    .get("supports_advanced_optimus")
                    .and_then(|v| v.as_bool())
                    .unwrap_or_else(|| supported.contains(&"advanced"));

                modes_box_load.set_visible(available && !supported.is_empty());
                no_mux_load.set_visible(!available || supported.is_empty());
                badge_load.set_visible(available);

                if available {
                    wh_load.set_visible(supported.contains(&"hybrid"));
                    wd_load.set_visible(supported.contains(&"discrete"));
                    wa_load.set_visible(supports_ao || effective_sel == "advanced");
                    wi_load.set_visible(
                        supported.contains(&"integrated") || effective_sel == "integrated",
                    );
                }

                upd_load.set(true);
                *boot_m_load.borrow_mut() = boot_mode.to_string();
                match effective_sel {
                    "discrete" => bd_load.set_active(true),
                    "advanced" => ba_load.set_active(true),
                    "integrated" => bi_load.set_active(true),
                    _ => bh_load.set_active(true),
                }
                let is_pending = info
                    .get("reboot_pending")
                    .and_then(|v| v.as_bool())
                    .unwrap_or_else(|| {
                        pending_mode.is_some_and(|p| p != boot_mode) || boot_mode != confirmed_mode
                    });
                warn_card.set_visible(available && is_pending);

                if is_pending {
                    badge_load.set_label(format_pending_mux_label(effective_sel));
                    badge_load.set_css_classes(&["badge-warn"]);
                } else {
                    badge_load.set_label(&format_mux_mode_label(confirmed_mode));
                    badge_load.set_css_classes(if confirmed_mode == "discrete" {
                        &["badge-warn"]
                    } else {
                        &["badge-ok"]
                    });
                }
                upd_load.set(false);

                let prime_available = info
                    .get("prime_offload_available")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let has_prime_run = info
                    .get("has_prime_run")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let cmd_str = if has_prime_run {
                    "prime-run %command%"
                } else {
                    "__NV_PRIME_RENDER_OFFLOAD=1 __GLX_VENDOR_LIBRARY_NAME=nvidia __VK_LAYER_NV_optimus=NVIDIA_only %command%"
                };
                *prime_cmd_load.borrow_mut() = cmd_str.to_string();
                prime_row_load.set_subtitle(cmd_str);
                prime_row_load.set_visible(prime_available && routing != "discrete");

                if routing == "discrete" {
                    disp_badge_load.set_label(&format!("eDP → {}", gpu_name));
                    disp_badge_load.set_css_classes(&["badge-warn"]);
                } else if routing == "hybrid" || routing == "integrated" {
                    disp_badge_load.set_label(&format!("eDP → {}", igpu_name));
                    disp_badge_load.set_css_classes(&["badge-ok"]);
                }

                if let Some(displays) = info.get("displays").and_then(|v| v.as_array()) {
                    let formatted: Vec<String> = displays
                        .iter()
                        .filter_map(|d| {
                            let name = d.get("display")?.as_str()?;
                            let gpu = d.get("gpu").and_then(|g| g.as_str()).unwrap_or("GPU");
                            Some(format!("{} ({})", name, gpu))
                        })
                        .collect();
                    if !formatted.is_empty() {
                        displays_row_load.set_subtitle(&formatted.join(" · "));
                    }
                }

                if let Some(state) = info.get("dgpu_power_state").and_then(|v| v.as_str()) {
                    let is_suspended = state == "suspended";
                    dgpu_val_load.set_label(i18n::t(if is_suspended {
                        "mux_dgpu_suspended"
                    } else {
                        "mux_dgpu_active"
                    }));
                    dgpu_val_load.set_css_classes(if is_suspended {
                        &["badge-ok"]
                    } else {
                        &["badge-warn"]
                    });
                    dgpu_row_load.set_visible(true);
                }
            }
        }
    });

    page
}

/// Compact mode card matching `performance_control.rs` proportions.
fn build_compact_mux_card(
    icon_path: &str,
    title: &str,
    subtitle: &str,
) -> (gtk::ToggleButton, gtk::Box) {
    let btn = gtk::ToggleButton::builder()
        .css_classes(["chip-card"])
        .build();

    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .halign(gtk::Align::Fill)
        .valign(gtk::Align::Center)
        .build();

    row.append(&gtk::Image::builder().file(icon_path).pixel_size(26).build());

    let text_col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    text_col.append(
        &gtk::Label::builder()
            .label(title)
            .css_classes(["chip-title"])
            .halign(gtk::Align::Start)
            .build(),
    );
    text_col.append(
        &gtk::Label::builder()
            .label(subtitle)
            .css_classes(["chip-sub"])
            .halign(gtk::Align::Start)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build(),
    );
    row.append(&text_col);

    btn.set_child(Some(&row));

    let wrap = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    wrap.append(&btn);
    (btn, wrap)
}
