use gtk::prelude::*;
use libadwaita as adw;
use adw::prelude::*;
use crate::daemon_client::{self, DaemonStatus, SystemStats};
use crate::i18n;

// GLib futures do not enter a Tokio runtime. Run process I/O on the shared
// runtime and await only its join handle on the GTK main context.
async fn command_output(program: &'static str, args: &[&str]) -> std::io::Result<std::process::Output> {
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
    daemon_client::get_runtime().spawn(async move {
        tokio::process::Command::new(program).args(args).output().await
    }).await.map_err(std::io::Error::other)?
}

fn is_systemd_running() -> bool {
    std::path::Path::new("/run/systemd/system").exists()
}

fn is_openrc_running() -> bool {
    std::path::Path::new("/run/openrc").exists()
}

fn show_toast(window: &adw::ApplicationWindow, msg: &str) {
    let toast = adw::Toast::new(msg);
    toast.set_timeout(3);
    if let Some(content) = window.content() {
        if let Ok(overlay) = content.downcast::<adw::ToastOverlay>() {
            overlay.add_toast(toast);
        }
    }
}

/// Detect conflicting power/thermal services across systemd, OpenRC, and generic init (#361, #373).
fn detect_conflicting_services() -> Vec<&'static str> {
    let candidates = ["thermald", "power-profiles-daemon", "tlp", "throttled"];
    let mut active = Vec::new();
    for svc in candidates {
        let running = if is_systemd_running() {
            std::process::Command::new("systemctl")
                .args(["is-active", svc])
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "active")
                .unwrap_or(false)
        } else if is_openrc_running() {
            std::process::Command::new("rc-service")
                .args([svc, "status"])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        } else {
            std::process::Command::new("pgrep")
                .args(["-x", svc])
                .output()
                .map(|o| o.status.success() && !o.stdout.is_empty())
                .unwrap_or(false)
        };
        if running {
            active.push(svc);
        }
    }
    active
}

/// Build the exact terminal command to disable conflicting power/thermal services (#373).
fn disable_conflicts_command(conflicts: &[&str]) -> String {
    let list = conflicts.join(" ");
    if is_systemd_running() {
        format!("sudo systemctl disable --now {list} && sudo systemctl mask {list}")
    } else if is_openrc_running() {
        conflicts
            .iter()
            .map(|s| format!("sudo rc-service {s} stop && sudo rc-update del {s} default"))
            .collect::<Vec<_>>()
            .join(" && ")
    } else {
        format!("sudo pkill -x {list}  # and disable them in your init supervisor")
    }
}

const GITHUB_NEW_ISSUE_BASE: &str = "https://github.com/yunusemreyl/omen-space/issues/new";
const GITHUB_URL_MAX_BYTES: usize = 7000;
const TRUNCATION_NOTICE: &str = "\n\n> _(Truncated for URL length — the complete untruncated dump was copied to your clipboard; press Ctrl+V to paste it in full.)_";

/// Builds a GitHub issue URL whose total percent-encoded length stays within `max_url_bytes` (#364).
pub(crate) fn build_github_issue_url(title: &str, report: &str, max_url_bytes: usize) -> String {
    use urlencoding::encode;
    let prefix = format!("{}?title={}&body=", GITHUB_NEW_ISSUE_BASE, encode(title));
    let full = format!("{}{}", prefix, encode(report));
    if full.len() <= max_url_bytes {
        return full;
    }

    let encoded_notice = encode(TRUNCATION_NOTICE);
    let budget = max_url_bytes.saturating_sub(prefix.len() + encoded_notice.len());

    let mut lo = 0usize;
    let mut hi = report.len();
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        let mut cut = mid;
        while cut > lo && !report.is_char_boundary(cut) {
            cut -= 1;
        }
        if cut == lo {
            let next = report[lo..]
                .chars()
                .next()
                .map(|c| lo + c.len_utf8())
                .unwrap_or(lo);
            if next > lo && encode(&report[..next]).len() <= budget {
                lo = next;
            } else {
                break;
            }
        } else if encode(&report[..cut]).len() <= budget {
            lo = cut;
        } else {
            hi = cut - 1;
        }
    }

    let slice_end = match report[..lo].rfind('\n') {
        Some(nl) if lo - nl <= 120 => nl,
        _ => lo,
    };

    format!("{}{}{}", prefix, encode(&report[..slice_end]), encoded_notice)
}

/// Opens the unified Full System Dump dialog and lets the user submit it to GitHub in one click.
/// Used by both the Debug Center button and the Unverified Board banner (#364, #370, #371, #372).
pub fn open_full_dump_dialog(parent: &adw::ApplicationWindow, issue_title: &str) {
    let dialog = adw::MessageDialog::builder()
        .heading(i18n::t("debug_dump_generating_title"))
        .body(i18n::t("debug_dump_generating_body"))
        .transient_for(parent)
        .build();

    let spinner = gtk::Spinner::builder()
        .spinning(true)
        .halign(gtk::Align::Center)
        .margin_top(14)
        .margin_bottom(14)
        .build();
    dialog.set_extra_child(Some(&spinner));
    dialog.present();

    let dialog_c = dialog.clone();
    let parent_c = parent.clone();
    let issue_title_s = issue_title.to_string();

    glib::spawn_future_local(async move {
        let res = daemon_client::generate_diagnostic_report_async().await;
        spinner.set_spinning(false);

        match res {
            Ok(report) => {
                dialog_c.set_heading(Some(i18n::t("debug_dump_ready_title")));
                dialog_c.set_body(i18n::t("debug_dump_ready_body"));

                let tv = gtk::TextView::builder()
                    .editable(false)
                    .wrap_mode(gtk::WrapMode::WordChar)
                    .monospace(true)
                    .hexpand(true)
                    .vexpand(true)
                    .build();
                tv.buffer().set_text(&report);
                let sw = gtk::ScrolledWindow::builder()
                    .child(&tv)
                    .min_content_height(420)
                    .min_content_width(640)
                    .build();
                dialog_c.set_extra_child(Some(&sw));

                dialog_c.add_response("ok", i18n::t("btn_close"));
                dialog_c.add_response("copy", i18n::t("btn_copy_dump"));
                dialog_c.add_response("issue", i18n::t("btn_submit_github"));
                dialog_c.set_response_appearance("issue", adw::ResponseAppearance::Suggested);
                dialog_c.set_default_response(Some("issue"));

                let report_copy = report.clone();
                let parent_toast = parent_c.clone();
                dialog_c.connect_response(None, move |d: &adw::MessageDialog, response| {
                    if response == "copy" {
                        if let Some(display) = gtk::gdk::Display::default() {
                            display.clipboard().set_text(&report_copy);
                        }
                        show_toast(&parent_toast, i18n::t("dump_copied_toast"));
                    } else if response == "issue" {
                        // Always copy full untruncated report to clipboard so nothing is ever lost
                        if let Some(display) = gtk::gdk::Display::default() {
                            display.clipboard().set_text(&report_copy);
                        }
                        let url = build_github_issue_url(
                            &issue_title_s,
                            &report_copy,
                            GITHUB_URL_MAX_BYTES,
                        );
                        let _ = gtk::gio::AppInfo::launch_default_for_uri(&url, None::<&gtk::gio::AppLaunchContext>);
                        d.close();
                    } else {
                        d.close();
                    }
                });
            }
            Err(e) => {
                dialog_c.set_heading(Some(i18n::t("error_generic")));
                dialog_c.set_body(&format!("{}: {}", i18n::t("error_generic"), e));
                dialog_c.add_response("ok", i18n::t("btn_close"));
                dialog_c.connect_response(None, |d: &adw::MessageDialog, _| d.close());
            }
        }
    });
}

/// Builds the comprehensive, unified Debug Center page.
pub fn build_page(window: &adw::ApplicationWindow) -> gtk::Box {
    let root_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .build();

    // ── Header Title & Subtitle ──────────────────────────────────────────────
    let title_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_bottom(4)
        .build();

    let header_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();

    let title_label = gtk::Label::builder()
        .label(i18n::t("title_debug"))
        .css_classes(["page-title"])
        .halign(gtk::Align::Start)
        .hexpand(true)
        .build();
    header_row.append(&title_label);

    // Daemon status pill matching Performance page
    let status_pill = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(7)
        .css_classes(["daemon-pill"])
        .valign(gtk::Align::Center)
        .build();
    let daemon_dot = gtk::Label::builder()
        .label("●")
        .css_classes(["daemon-dot-off"])
        .build();
    let daemon_text = gtk::Label::builder()
        .label(i18n::t("debug_daemon_connecting"))
        .css_classes(["daemon-pill-text"])
        .build();
    status_pill.append(&daemon_dot);
    status_pill.append(&daemon_text);
    header_row.append(&status_pill);

    // Restart daemon button (systemd + OpenRC compatible, #361)
    let restart_tooltip = if is_systemd_running() {
        format!("{} (pkexec systemctl restart omen-space-daemon)", i18n::t("debug_restart_daemon"))
    } else if is_openrc_running() {
        format!("{} (pkexec rc-service omen-space-daemon restart)", i18n::t("debug_restart_daemon"))
    } else {
        i18n::t("debug_restart_daemon").to_string()
    };
    let restart_btn = gtk::Button::builder()
        .icon_name("system-reboot-symbolic")
        .tooltip_text(&restart_tooltip)
        .valign(gtk::Align::Center)
        .build();
    let win_c_restart = window.clone();
    restart_btn.connect_clicked(move |_| {
        let win_clone = win_c_restart.clone();
        glib::spawn_future_local(async move {
            let res = if is_systemd_running() {
                command_output("pkexec", &["systemctl", "restart", "omen-space-daemon"]).await
            } else {
                command_output("pkexec", &["rc-service", "omen-space-daemon", "restart"]).await
            };
            match res {
                Ok(output) if output.status.success() => {
                    show_toast(&win_clone, i18n::t("debug_restart_ok"));
                }
                _ => {
                    show_toast(&win_clone, i18n::t("debug_restart_err"));
                }
            }
        });
    });
    header_row.append(&restart_btn);
    title_box.append(&header_row);

    let desc_label = gtk::Label::builder()
        .label(i18n::t("debug_desc"))
        .css_classes(["os-section-desc"])
        .halign(gtk::Align::Start)
        .build();
    title_box.append(&desc_label);
    root_box.append(&title_box);

    // Listen to daemon status transitions to update badge
    let daemon_dot_c = daemon_dot.clone();
    let daemon_text_c = daemon_text.clone();
    daemon_client::subscribe_daemon_status(move |status| {
        match status {
            DaemonStatus::Online => {
                daemon_dot_c.remove_css_class("daemon-dot-off");
                daemon_dot_c.add_css_class("daemon-dot-on");
                daemon_text_c.set_label(i18n::t("debug_daemon_online"));
            }
            DaemonStatus::Offline => {
                daemon_dot_c.remove_css_class("daemon-dot-on");
                daemon_dot_c.add_css_class("daemon-dot-off");
                daemon_text_c.set_label(i18n::t("debug_daemon_offline"));
            }
        }
    });

    let specs = daemon_client::get_hardware_specs_sync();
    let board_id = std::fs::read_to_string("/sys/class/dmi/id/board_name")
        .unwrap_or_default()
        .trim()
        .to_uppercase();

    // ─────────────────────────────────────────────────────────────────────────
    // 1. Single-Button Full System Dump to GitHub
    // ─────────────────────────────────────────────────────────────────────────
    let dump_group = adw::PreferencesGroup::builder()
        .title(i18n::t("debug_dump_group_title"))
        .description(i18n::t("debug_dump_group_desc"))
        .build();

    let dump_row = adw::ActionRow::builder()
        .title(i18n::t("debug_dump_row_title"))
        .subtitle(i18n::t("debug_dump_row_sub"))
        .activatable(true)
        .build();

    let dump_btn = gtk::Button::builder()
        .label(i18n::t("debug_dump_btn"))
        .css_classes(["suggested-action", "pill"])
        .valign(gtk::Align::Center)
        .build();

    let default_issue_title = if board_id.is_empty() {
        format!("System Dump: {}", specs.product_name)
    } else {
        format!("System Dump: {} (Board {})", specs.product_name, board_id)
    };

    let win_c_dump_btn = window.clone();
    let title_c_btn = default_issue_title.clone();
    dump_btn.connect_clicked(move |_| {
        open_full_dump_dialog(&win_c_dump_btn, &title_c_btn);
    });

    let win_c_dump_row = window.clone();
    let title_c_row = default_issue_title.clone();
    dump_row.connect_activated(move |_| {
        open_full_dump_dialog(&win_c_dump_row, &title_c_row);
    });

    dump_row.add_suffix(&dump_btn);
    dump_group.add(&dump_row);
    root_box.append(&dump_group);

    // ─────────────────────────────────────────────────────────────────────────
    // 2. Hardware & Driver Status Group
    // ─────────────────────────────────────────────────────────────────────────
    let hw_group = adw::PreferencesGroup::builder()
        .title(i18n::t("debug_group_hw"))
        .description(i18n::t("debug_group_hw_desc"))
        .build();

    let is_verified = daemon_client::is_board_verified_sync(&board_id);
    let board_row = adw::ActionRow::builder()
        .title(i18n::t("debug_board_id"))
        .subtitle(&format!("{} — {}", specs.product_name, if board_id.is_empty() { "Unknown" } else { &board_id }))
        .build();
    let badge_label = gtk::Label::builder()
        .label(i18n::t(if is_verified { "debug_board_verified" } else { "debug_board_unverified" }))
        .css_classes([if is_verified { "badge-ok" } else { "badge-warn" }])
        .valign(gtk::Align::Center)
        .margin_end(8)
        .build();
    board_row.add_suffix(&badge_label);
    hw_group.add(&board_row);

    let kernel_release = std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .unwrap_or_else(|_| "Unknown".to_string())
        .trim()
        .to_string();
    let init_label = if is_systemd_running() {
        "systemd"
    } else if is_openrc_running() {
        "OpenRC"
    } else {
        "Non-systemd"
    };
    let kernel_row = adw::ActionRow::builder()
        .title(i18n::t("debug_kernel_init"))
        .subtitle(&format!("{} ({})", kernel_release, init_label))
        .build();
    hw_group.add(&kernel_row);

    let proc_modules = std::fs::read_to_string("/proc/modules").unwrap_or_default();
    let has_hp_wmi = proc_modules.contains("hp_wmi");
    let has_hp_omen_extra = proc_modules.contains("hp_omen_extra");
    let has_ec_io = std::path::Path::new("/sys/kernel/debug/ec/ec0/io").exists();
    let ec_sys_status = if proc_modules.contains("ec_sys") {
        "Loaded"
    } else if has_ec_io {
        "Built-in / Active"
    } else {
        "Not Loaded (optional)"
    };

    let driver_row = adw::ActionRow::builder()
        .title(i18n::t("debug_kernel_modules"))
        .subtitle(&format!(
            "hp-wmi: {} | hp-omen-extra: {} | ec_sys: {}",
            if has_hp_wmi { "Loaded" } else { "Missing" },
            if has_hp_omen_extra { "Loaded" } else { "Not Loaded" },
            ec_sys_status,
        ))
        .build();
    hw_group.add(&driver_row);

    // Conflicting background services row (#373, #361)
    let conflicts = detect_conflicting_services();
    let conflict_sub = if conflicts.is_empty() {
        i18n::t("debug_no_conflicts").to_string()
    } else {
        i18n::t("debug_conflicts_warn").replace("{}", &conflicts.join(", "))
    };
    let conflict_row = adw::ActionRow::builder()
        .title(i18n::t("debug_conflicts"))
        .subtitle(&conflict_sub)
        .build();

    if !conflicts.is_empty() {
        let cmd = disable_conflicts_command(&conflicts);
        let help_btn = gtk::Button::builder()
            .label(i18n::t("conflict_how_to_disable"))
            .css_classes(["warning"])
            .valign(gtk::Align::Center)
            .build();
        let win_c_conf = window.clone();
        help_btn.connect_clicked(move |_| {
            let dialog = adw::MessageDialog::builder()
                .heading(i18n::t("conflict_dialog_title"))
                .body(i18n::t("conflict_dialog_body"))
                .transient_for(&win_c_conf)
                .build();
            let tv = gtk::TextView::builder()
                .editable(false)
                .monospace(true)
                .wrap_mode(gtk::WrapMode::WordChar)
                .margin_top(8)
                .margin_bottom(8)
                .margin_start(12)
                .margin_end(12)
                .build();
            tv.buffer().set_text(&cmd);
            dialog.set_extra_child(Some(&tv));
            dialog.add_response("close", i18n::t("btn_close"));
            dialog.add_response("copy", i18n::t("btn_copy_command"));
            dialog.set_response_appearance("copy", adw::ResponseAppearance::Suggested);
            let cmd_c = cmd.clone();
            let win_toast = win_c_conf.clone();
            dialog.connect_response(None, move |d: &adw::MessageDialog, resp| {
                if resp == "copy" {
                    if let Some(display) = gtk::gdk::Display::default() {
                        display.clipboard().set_text(&cmd_c);
                    }
                    show_toast(&win_toast, i18n::t("debug_cmd_copied"));
                }
                d.close();
            });
            dialog.present();
        });
        conflict_row.add_suffix(&help_btn);
    } else {
        let conflict_icon = gtk::Image::builder()
            .icon_name("emblem-ok-symbolic")
            .css_classes(["success"])
            .valign(gtk::Align::Center)
            .margin_end(8)
            .build();
        conflict_row.add_suffix(&conflict_icon);
    }
    hw_group.add(&conflict_row);

    root_box.append(&hw_group);

    // ─────────────────────────────────────────────────────────────────────────
    // 3. Live Sensor & Telemetry Inspector Group
    // ─────────────────────────────────────────────────────────────────────────
    let telem_group = adw::PreferencesGroup::builder()
        .title(i18n::t("debug_group_telemetry"))
        .description(i18n::t("debug_group_telemetry_desc"))
        .build();

    let cpu_telem_row = adw::ActionRow::builder()
        .title(i18n::t("debug_cpu_pkg"))
        .subtitle("Temperature: --°C | Power: -- W | Usage: --%")
        .build();
    telem_group.add(&cpu_telem_row);

    let gpu_telem_row = adw::ActionRow::builder()
        .title(i18n::t("debug_gpu_discrete"))
        .subtitle("Temperature: --°C | Power: -- W | Usage: --% | State: --")
        .build();
    telem_group.add(&gpu_telem_row);

    let fan_telem_row = adw::ActionRow::builder()
        .title(i18n::t("debug_cooling_fans"))
        .subtitle("Fan 1: -- RPM | Fan 2: -- RPM")
        .build();
    telem_group.add(&fan_telem_row);

    let battery_telem_row = adw::ActionRow::builder()
        .title(i18n::t("debug_battery_power"))
        .subtitle("Charge: --% | Care Limit: --% | Status: --")
        .build();
    telem_group.add(&battery_telem_row);

    let cpu_row_c = cpu_telem_row.clone();
    let gpu_row_c = gpu_telem_row.clone();
    let fan_row_c = fan_telem_row.clone();
    let bat_row_c = battery_telem_row.clone();

    daemon_client::subscribe_telemetry(move |stats: SystemStats| {
        cpu_row_c.set_subtitle(&format!(
            "Temperature: {}°C | Power: {:.1} W | Usage: {:.0}%",
            stats.cpu_temp, stats.cpu_pwr, stats.cpu_load
        ));

        let gpu_state = stats.gpu_sample_state.label();
        gpu_row_c.set_subtitle(&format!(
            "Temperature: {}°C | Power: {:.1} W | Usage: {:.0}% | State: {}",
            stats.gpu_temp, stats.gpu_pwr, stats.gpu_load, gpu_state
        ));

        fan_row_c.set_subtitle(&format!(
            "Fan 1: {} RPM | Fan 2: {} RPM (Max: {} RPM)",
            stats.fan1_rpm, stats.fan2_rpm, stats.fan_rpm
        ));

        if let Some(ref bat) = stats.battery {
            bat_row_c.set_subtitle(&format!(
                "Charge: {}% | Care Limit: {}% | Status: {:?} | Power: {:.1} W",
                bat.charge_pct, bat.charge_limit_pct.unwrap_or(100), bat.status, bat.power_w
            ));
        } else {
            bat_row_c.set_subtitle(i18n::t("debug_no_battery"));
        }
    });

    let sysfs_expander = adw::ExpanderRow::builder()
        .title(i18n::t("debug_sysfs_paths"))
        .subtitle(i18n::t("debug_sysfs_paths_sub"))
        .build();

    let mut paths_found = Vec::new();
    for p in [
        "/sys/devices/platform/hp-wmi",
        "/sys/devices/platform/hp-wmi/gpu_mux_mode",
        "/sys/devices/platform/hp-wmi/graphics_mode",
        "/sys/devices/platform/hp-omen-extra/all_zones",
        "/sys/devices/platform/hp-omen-extra/lightbar",
        "/sys/firmware/acpi/platform_profile",
        "/sys/kernel/debug/ec/ec0/io",
        "/var/lib/omen-space-daemon/rapl_baseline.json",
    ] {
        let exists = std::path::Path::new(p).exists();
        paths_found.push(format!("{}: {}", p, if exists { "EXISTS" } else { "NOT FOUND" }));
    }
    if let Ok(entries) = std::fs::read_dir("/sys/class/hwmon") {
        for entry in entries.flatten() {
            let name_path = entry.path().join("name");
            if let Ok(name) = std::fs::read_to_string(&name_path) {
                paths_found.push(format!("{}: {}", entry.path().display(), name.trim()));
            }
        }
    }
    for item in &paths_found {
        let row = adw::ActionRow::builder().title(item).build();
        sysfs_expander.add_row(&row);
    }
    telem_group.add(&sysfs_expander);

    root_box.append(&telem_group);

    // ─────────────────────────────────────────────────────────────────────────
    // 4. Hardware Maintenance & Interactive Test Bench
    // ─────────────────────────────────────────────────────────────────────────
    let test_group = adw::PreferencesGroup::builder()
        .title(i18n::t("debug_group_maint"))
        .description(i18n::t("debug_group_maint_desc"))
        .build();

    // Fan Dust Cleaning (moved from Settings into Debug Center)
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
        let spinner = gtk::Spinner::builder()
            .spinning(true)
            .halign(gtk::Align::Center)
            .margin_top(12)
            .margin_bottom(12)
            .build();
        dialog.set_extra_child(Some(&spinner));
        dialog.present();

        let dialog_clone = dialog.clone();
        glib::spawn_future_local(async move {
            let res = daemon_client::run_fan_cleaning_async().await;
            spinner.set_spinning(false);
            dialog_clone.set_heading(Some(i18n::t("debug_complete")));
            dialog_clone.set_body(&res.unwrap_or_else(|e| format!("Error: {}", e)));
            dialog_clone.add_response("ok", i18n::t("btn_close"));
            dialog_clone.connect_response(None, |d: &adw::MessageDialog, _| d.close());
        });
    });
    test_group.add(&fan_clean_row);

    // Fan Speed Override Tests
    let fan_test_row = adw::ActionRow::builder()
        .title(i18n::t("debug_fan_test"))
        .subtitle(i18n::t("debug_fan_test_sub"))
        .build();

    let fan_btn_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .valign(gtk::Align::Center)
        .build();

    let btn_fan_max = gtk::Button::builder().label(i18n::t("debug_fan_max_btn")).css_classes(["destructive-action"]).build();
    let btn_fan_auto = gtk::Button::builder().label(i18n::t("debug_fan_auto_btn")).css_classes(["suggested-action"]).build();

    btn_fan_max.connect_clicked(|_| {
        daemon_client::set_fan_mode_sync("max".to_string());
    });
    btn_fan_auto.connect_clicked(|_| {
        daemon_client::set_fan_mode_sync("auto".to_string());
    });

    fan_btn_box.append(&btn_fan_max);
    fan_btn_box.append(&btn_fan_auto);
    fan_test_row.add_suffix(&fan_btn_box);
    test_group.add(&fan_test_row);

    // RGB Lighting Test Row
    let rgb_test_row = adw::ActionRow::builder()
        .title(i18n::t("debug_rgb_diag"))
        .subtitle(i18n::t("debug_rgb_diag_sub"))
        .build();

    let rgb_btn_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .valign(gtk::Align::Center)
        .build();

    for (name, hex) in [
        ("Red", "FF0000"),
        ("Green", "00FF00"),
        ("Blue", "0000FF"),
        ("White", "FFFFFF"),
        ("Off", "000000"),
    ] {
        let btn = gtk::Button::builder().label(name).build();
        let hex_s = hex.to_string();
        btn.connect_clicked(move |_| {
            daemon_client::set_global_sync(true, 100, "ltr");
            daemon_client::set_color_sync(8, hex_s.clone());
            daemon_client::set_lightbar_color_sync(4, hex_s.clone());
        });
        rgb_btn_box.append(&btn);
    }

    rgb_test_row.add_suffix(&rgb_btn_box);
    test_group.add(&rgb_test_row);

    // Per-Key HID Direct Key Tester
    let per_key_test_row = adw::ActionRow::builder()
        .title(i18n::t("debug_hid_tester"))
        .subtitle(i18n::t("debug_hid_tester_sub"))
        .build();

    let pk_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .valign(gtk::Align::Center)
        .build();

    let spin_idx = gtk::SpinButton::with_range(0.0, 103.0, 1.0);
    spin_idx.set_value(0.0);
    let btn_light_key = gtk::Button::builder().label(i18n::t("debug_test_key_btn")).build();
    let spin_idx_c = spin_idx.clone();
    btn_light_key.connect_clicked(move |_| {
        let idx = spin_idx_c.value() as u32;
        daemon_client::light_key_index_sync(idx, "FF0000".to_string());
    });

    pk_box.append(&spin_idx);
    pk_box.append(&btn_light_key);
    per_key_test_row.add_suffix(&pk_box);
    test_group.add(&per_key_test_row);

    // Hotkey & Macro Key Realtime Event Monitor
    let macro_row = adw::ActionRow::builder()
        .title(i18n::t("debug_macro_listener"))
        .subtitle(i18n::t("debug_macro_listener_sub"))
        .build();

    let macro_badge = gtk::Label::builder()
        .label(i18n::t("debug_waiting_key"))
        .css_classes(["badge-neutral"])
        .valign(gtk::Align::Center)
        .margin_end(8)
        .build();
    macro_row.add_suffix(&macro_badge);

    let badge_c = macro_badge.clone();
    glib::spawn_future_local(async move {
        use omen_types::PlatformProxy;
        use zbus::export::futures_util::StreamExt;
        if let Ok(conn) = daemon_client::get_conn().await {
            if let Ok(proxy) = PlatformProxy::new(&conn).await {
                if let Ok(mut stream) = proxy.receive_macro_key_pressed().await {
                    while let Some(signal) = stream.next().await {
                        if let Ok(args) = signal.args() {
                            let key = args.key_name;
                            let time = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs() % 10000;
                            badge_c.set_label(&format!("EVENT: {} (T+{})", key, time));
                            badge_c.set_css_classes(&["badge-ok", "os-mono"]);
                        }
                    }
                }
            }
        }
    });

    test_group.add(&macro_row);
    root_box.append(&test_group);

    // ─────────────────────────────────────────────────────────────────────────
    // 5. EC Memory & System Logs Inspector
    // ─────────────────────────────────────────────────────────────────────────
    let logs_group = adw::PreferencesGroup::builder()
        .title(i18n::t("debug_group_ec_logs"))
        .description(i18n::t("debug_group_ec_logs_desc"))
        .build();

    let ec_row = adw::ExpanderRow::builder()
        .title(i18n::t("debug_ec_dump"))
        .subtitle(i18n::t("debug_ec_dump_sub"))
        .build();

    let ec_tv = gtk::TextView::builder()
        .editable(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::None)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(12)
        .margin_end(12)
        .build();

    let refresh_ec_btn = gtk::Button::builder()
        .label(i18n::t("debug_read_ec_btn"))
        .valign(gtk::Align::Center)
        .build();

    let ec_tv_c = ec_tv.clone();
    refresh_ec_btn.connect_clicked(move |_| {
        let tv = ec_tv_c.clone();
        glib::spawn_future_local(async move {
            let mut out = String::new();
            if let Ok(bytes) = std::fs::read("/sys/kernel/debug/ec/ec0/io") {
                out.push_str("       00 01 02 03 04 05 06 07  08 09 0A 0B 0C 0D 0E 0F  |ASCII|\n");
                out.push_str("----------------------------------------------------------------\n");
                for row in 0..16 {
                    out.push_str(&format!("{:02X}:   ", row * 16));
                    let mut ascii = String::new();
                    for col in 0..16 {
                        let idx = row * 16 + col;
                        let b = bytes.get(idx).copied().unwrap_or(0);
                        out.push_str(&format!("{:02X} ", b));
                        if col == 7 { out.push(' '); }
                        if (32..=126).contains(&b) {
                            ascii.push(b as char);
                        } else {
                            ascii.push('.');
                        }
                    }
                    out.push_str(&format!(" |{}|\n", ascii));
                }
            } else {
                out.push_str("EC sysfs node not directly accessible (/sys/kernel/debug/ec/ec0/io).\n");
                out.push_str("Querying daemon for EC snapshot...\n\n");
                if let Ok(rep) = daemon_client::generate_diagnostic_report_async().await {
                    let mut in_ec = false;
                    for line in rep.lines() {
                        if line.starts_with("## EC Registers") {
                            in_ec = true;
                        } else if in_ec && line.starts_with("## ") {
                            break;
                        }
                        if in_ec {
                            out.push_str(line);
                            out.push('\n');
                        }
                    }
                }
            }
            tv.buffer().set_text(&out);
        });
    });

    let ec_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    let sw_ec = gtk::ScrolledWindow::builder()
        .child(&ec_tv)
        .min_content_height(240)
        .build();
    ec_box.append(&refresh_ec_btn);
    ec_box.append(&sw_ec);
    ec_row.add_row(&adw::ActionRow::builder().child(&ec_box).build());
    logs_group.add(&ec_row);

    let logs_expander = adw::ExpanderRow::builder()
        .title(i18n::t("debug_sys_logs"))
        .subtitle(i18n::t("debug_sys_logs_sub"))
        .build();

    let logs_tv = gtk::TextView::builder()
        .editable(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(12)
        .margin_end(12)
        .build();

    let logs_sw = gtk::ScrolledWindow::builder()
        .child(&logs_tv)
        .min_content_height(260)
        .build();

    let refresh_logs_btn = gtk::Button::builder()
        .label(i18n::t("debug_fetch_logs_btn"))
        .valign(gtk::Align::Center)
        .build();

    let logs_tv_c = logs_tv.clone();
    refresh_logs_btn.connect_clicked(move |_| {
        let tv = logs_tv_c.clone();
        glib::spawn_future_local(async move {
            let mut out = String::new();
            out.push_str("=== omen-space-daemon logs (last 40 lines) ===\n");
            if is_systemd_running() {
                if let Ok(output) = command_output("journalctl", &["-u", "omen-space-daemon", "-n", "40", "--no-pager"]).await {
                    out.push_str(&String::from_utf8_lossy(&output.stdout));
                }
            } else if let Ok(text) = std::fs::read_to_string("/var/log/omen-space-daemon.log") {
                for line in text.lines().rev().take(40).collect::<Vec<_>>().into_iter().rev() {
                    out.push_str(line);
                    out.push('\n');
                }
            }
            out.push_str("\n=== Kernel dmesg (hp_wmi / hp-omen) ===\n");
            if let Ok(output) = command_output("dmesg", &[]).await {
                let text = String::from_utf8_lossy(&output.stdout);
                for line in text.lines().filter(|l| l.contains("hp_wmi") || l.contains("hp-wmi") || l.contains("omen")) {
                    out.push_str(line);
                    out.push('\n');
                }
            }
            tv.buffer().set_text(&out);
        });
    });

    let logs_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    logs_box.append(&refresh_logs_btn);
    logs_box.append(&logs_sw);
    logs_expander.add_row(&adw::ActionRow::builder().child(&logs_box).build());
    logs_group.add(&logs_expander);

    root_box.append(&logs_group);
    root_box
}

#[cfg(test)]
mod process_tests {
    #[test]
    fn process_io_can_be_awaited_from_glib_without_a_tokio_reactor() {
        let context = glib::MainContext::new();
        let output = context.block_on(super::command_output("true", &[])).unwrap();
        assert!(output.status.success());
    }

    #[test]
    fn github_issue_url_respects_max_bytes_and_utf8() {
        let short = super::build_github_issue_url("Test", "short body", 7000);
        assert!(short.contains("short%20body"));
        assert!(!short.contains("Truncated"));

        // Markdown tables and non-ASCII characters expand ~3x when percent-encoded.
        let long_report = "| Field | Value | Türkçe İÖÜ 🚀 |\n".repeat(400);
        let url = super::build_github_issue_url("Full Dump (8A4C)", &long_report, 7000);
        assert!(url.len() <= 7000, "URL len {} exceeded 7000", url.len());
        assert!(url.contains("Truncated"));
    }
}
