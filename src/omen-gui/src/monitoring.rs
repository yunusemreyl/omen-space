use gtk::prelude::*;
use std::rc::Rc;
use std::cell::RefCell;

/* ─────────────────────────────────────────────────────────
   monitoring.rs
   Shows live CPU / GPU / RAM / Disk stats for:
   Victus by HP 16  |  i5-13500H  |  RTX 4050 Mobile  |  16 GB
   ───────────────────────────────────────────────────────── */

struct MonitorUI {
    container: gtk::Box,
    temp_label: gtk::Label,
    load_pct_label: gtk::Label,
    load_bar: gtk::ProgressBar,
    pwr_label: gtk::Label,
    fan_label: gtk::Label,
}

struct DeviceUI {
    container: gtk::Box,
    ram_bar: gtk::ProgressBar,
    ram_val_label: gtk::Label,
    disk_bar: gtk::ProgressBar,
    disk_val_label: gtk::Label,
}

struct WattGraphUI {
    container: gtk::Box,
    drawing_area: gtk::DrawingArea,
    warning_label: gtk::Label,
}

struct TotalWattUI {
    container: gtk::Box,
    val_label: gtk::Label,
}

struct ModesUI {
    container: gtk::Box,
}

// ── Helpers ────────────────────────────────────────────────


fn val_label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .css_classes(["os-monitor-val"])
        .build()
}

fn sub_label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .css_classes(["os-monitor-label"])
        .build()
}

use crate::i18n;

// ── CPU / GPU monitor card ──────────────────────────────────
fn build_monitor_card(icon_path: &str, title: &str, bar_class: &str, pwr_title: &str) -> MonitorUI {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["os-monitor-card"])
        .spacing(0)
        .build();

    // Row 1: icon + title + temp
    let r1 = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    r1.append(&gtk::Image::builder().file(icon_path).pixel_size(20).build());
    r1.append(&gtk::Label::builder()
        .label(title)
        .css_classes(["monitor-icon-label"])
        .hexpand(true)
        .halign(gtk::Align::Start)
        .build());
    let l_temp = val_label("──°C");
    r1.append(&l_temp);
    card.append(&r1);

    // Row 2: TEMP / LOAD labels
    let r2 = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .margin_top(14)
        .build();
    r2.append(&gtk::Label::builder()
        .label(i18n::t("mon_temp"))
        .css_classes(["os-monitor-label"])
        .hexpand(true)
        .halign(gtk::Align::Start)
        .build());
    r2.append(&sub_label(i18n::t("mon_load")));
    card.append(&r2);

    // Progress bar
    let pbar = gtk::ProgressBar::builder()
        .fraction(0.0)
        .css_classes([bar_class])
        .margin_top(6)
        .margin_bottom(4)
        .build();
    card.append(&pbar);

    let l_load_pct = gtk::Label::builder()
        .label("0%")
        .css_classes(["os-monitor-label"])
        .halign(gtk::Align::End)
        .build();
    card.append(&l_load_pct);

    // Separator
    let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
    sep.set_margin_top(10);
    sep.set_margin_bottom(10);
    card.append(&sep);

    // Row 3: power + fan
    let r3 = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .homogeneous(true)
        .build();

    let b_pwr = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(3).build();
    b_pwr.append(&sub_label(pwr_title));
    let l_p_val = gtk::Label::builder().label("──W").css_classes(["os-monitor-val-sm"]).build();
    b_pwr.append(&l_p_val);

    let b_fan = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(3).build();
    b_fan.append(&sub_label(i18n::t("mon_fan")));
    let l_f_val = gtk::Label::builder().label(format!("── {}", i18n::t("mon_rpm"))).css_classes(["os-monitor-val-sm"]).build();
    b_fan.append(&l_f_val);

    r3.append(&b_pwr);
    r3.append(&b_fan);
    card.append(&r3);

    MonitorUI {
        container: card,
        temp_label: l_temp,
        load_pct_label: l_load_pct,
        load_bar: pbar,
        pwr_label: l_p_val,
        fan_label: l_f_val,
    }
}

// ── RAM + Disk card ─────────────────────────────────────────
// ── RAM + Disk card ─────────────────────────────────────────
fn build_device_card() -> DeviceUI {
    let specs = crate::daemon_client::get_hardware_specs_sync();

    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["os-monitor-card"])
        .spacing(0)
        .build();

    let r1 = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    r1.append(&gtk::Image::builder()
        .icon_name("drive-harddisk-symbolic")
        .pixel_size(18)
        .build());
    r1.append(&gtk::Label::builder()
        .label(i18n::t("device_status"))
        .css_classes(["monitor-icon-label"])
        .hexpand(true)
        .halign(gtk::Align::Start)
        .build());
    card.append(&r1);

    // RAM
    let ram_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .margin_top(14)
        .build();
    ram_row.append(&sub_label(i18n::t("mon_ram")));
    ram_row.append(&gtk::Label::builder().label(&specs.ram_spec).css_classes(["os-spec-text"])
        .halign(gtk::Align::End).hexpand(true).build());
    card.append(&ram_row);

    let ram_bar = gtk::ProgressBar::builder()
        .fraction(0.0)
        .css_classes(["os-prog-ram"])
        .margin_top(5)
        .margin_bottom(3)
        .build();
    card.append(&ram_bar);
    let ram_val = gtk::Label::builder()
        .label("─.─ / ─.─ GB")
        .css_classes(["os-monitor-label"])
        .halign(gtk::Align::End)
        .build();
    card.append(&ram_val);

    let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
    sep.set_margin_top(10);
    sep.set_margin_bottom(10);
    card.append(&sep);

    // Disk
    let disk_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .build();
    disk_row.append(&sub_label(i18n::t("mon_disk")));
    disk_row.append(&gtk::Label::builder().label(&specs.ssd_spec).css_classes(["os-spec-text"])
        .halign(gtk::Align::End).hexpand(true).build());
    card.append(&disk_row);

    let disk_bar = gtk::ProgressBar::builder()
        .fraction(0.0)
        .css_classes(["os-prog-disk"])
        .margin_top(5)
        .margin_bottom(3)
        .build();
    card.append(&disk_bar);
    let disk_val = gtk::Label::builder()
        .label("─.─ / ─.─ GB")
        .css_classes(["os-monitor-label"])
        .halign(gtk::Align::End)
        .build();
    card.append(&disk_val);

    DeviceUI {
        container: card,
        ram_bar,
        ram_val_label: ram_val,
        disk_bar,
        disk_val_label: disk_val,
    }
}

// ── Total wattage card ──────────────────────────────────────
fn build_watt_card() -> TotalWattUI {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["os-monitor-card"])
        .build();
    let r1 = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    r1.append(&gtk::Image::builder()
        .icon_name("battery-symbolic")
        .pixel_size(18)
        .build());
    r1.append(&gtk::Label::builder()
        .label(i18n::t("total_system_power"))
        .css_classes(["monitor-icon-label"])
        .hexpand(true)
        .halign(gtk::Align::Start)
        .build());
    card.append(&r1);
    let val = gtk::Label::builder()
        .label("──.─ W")
        .css_classes(["os-monitor-val"])
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .vexpand(true)
        .margin_top(10)
        .margin_bottom(10)
        .build();
    card.append(&val);
    TotalWattUI { container: card, val_label: val }
}

// ── Watt history graph ──────────────────────────────────────
fn build_watt_graph_card(title: &str, _max_watt: f64) -> WattGraphUI {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["os-monitor-card"])
        .build();
    let r1 = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    let l_title = gtk::Label::builder()
        .label(title)
        .css_classes(["monitor-icon-label"])
        .hexpand(true)
        .halign(gtk::Align::Start)
        .build();
    let warn = gtk::Label::builder()
        .css_classes(["badge-warn"])
        .halign(gtk::Align::End)
        .build();
    warn.set_markup("<span weight='bold'>⚠ THROTTLE</span>");
    warn.set_visible(false);
    r1.append(&l_title);
    r1.append(&warn);
    card.append(&r1);
    let da = gtk::DrawingArea::builder()
        .hexpand(true)
        .vexpand(true)
        .height_request(90)
        .margin_top(12)
        .build();
    card.append(&da);
    WattGraphUI { container: card, drawing_area: da, warning_label: warn }
}

// ── Modes card (live from daemon) ───────────────────────────
fn build_modes_card() -> ModesUI {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["os-monitor-card"])
        .build();
    let r1 = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    r1.append(&gtk::Image::builder()
        .icon_name("preferences-system-symbolic")
        .pixel_size(18)
        .build());
    r1.append(&gtk::Label::builder()
        .label(i18n::t("system_modes"))
        .css_classes(["monitor-icon-label"])
        .hexpand(true)
        .halign(gtk::Align::Start)
        .build());
    card.append(&r1);

    let power_lbl = gtk::Label::builder().label("...").css_classes(["os-monitor-val-sub"]).build();
    let fan_lbl = gtk::Label::builder().label("...").css_classes(["os-monitor-val-sub"]).build();

    let make_row = |key: &str, val: &gtk::Label| {
        let row = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).margin_top(12).build();
        row.append(&gtk::Label::builder().label(key).css_classes(["os-monitor-label"]).hexpand(true).halign(gtk::Align::Start).build());
        row.append(val);
        row
    };

    card.append(&make_row(i18n::t("mon_mode"), &power_lbl));
    let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
    sep.set_margin_top(6);
    card.append(&sep);
    card.append(&make_row(i18n::t("mon_fan"), &fan_lbl));
    let sep2 = gtk::Separator::new(gtk::Orientation::Horizontal);
    sep2.set_margin_top(6);
    card.append(&sep2);

    let pl = power_lbl.clone();
    let fl = fan_lbl.clone();
    glib::spawn_future_local(async move {
        let power = crate::daemon_client::get_power_profile_async().await
            .unwrap_or_else(|_| "balanced".to_string());
        pl.set_label(match power.as_str() {
            "power-saver" => i18n::t("mode_eco"),
            "performance" => i18n::t("mode_performance"),
            _ => i18n::t("mode_balanced"),
        });
        let fan = crate::daemon_client::get_fan_mode_async().await
            .unwrap_or_else(|_| "auto".to_string());
        fl.set_label(match fan.as_str() {
            "max"    => i18n::t("fan_max"),
            "manual" => i18n::t("fan_custom"),
            _        => i18n::t("fan_auto"),
        });
    });

    ModesUI { container: card }
}

// ── Device spec header card ─────────────────────────────────
pub fn build_spec_header() -> gtk::Box {
    let specs = crate::daemon_client::get_hardware_specs_sync();

    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .css_classes(["spec-card"])
        .spacing(16)
        .build();

    let img_name = if specs.product_name.to_lowercase().contains("omen") {
        "omen_laptop.png"
    } else {
        "victus_laptop.png"
    };

    // Device image
    card.append(&gtk::Image::builder()
        .file(crate::asset_resolver::get_asset_path(img_name))
        .pixel_size(80)
        .valign(gtk::Align::Center)
        .build());

    let info = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .valign(gtk::Align::Center)
        .build();

    info.append(&gtk::Label::builder()
        .label(&specs.product_name)
        .css_classes(["spec-device-name"])
        .halign(gtk::Align::Start)
        .build());

    let grid = gtk::Grid::builder()
        .row_spacing(4)
        .column_spacing(16)
        .build();

    let spec_list = [
        ("CPU",  specs.cpu_spec.as_str()),
        ("GPU",  specs.gpu_spec.as_str()),
        ("RAM",  specs.ram_spec.as_str()),
        ("SSD",  specs.ssd_spec.as_str()),
        ("OS",   specs.os_spec.as_str()),
    ];

    for (i, (k, v)) in spec_list.iter().enumerate() {
        grid.attach(
            &gtk::Label::builder().label(*k).css_classes(["spec-label"])
                .halign(gtk::Align::Start).build(),
            0, i as i32, 1, 1
        );
        grid.attach(
            &gtk::Label::builder().label(*v).css_classes(["spec-value"])
                .halign(gtk::Align::Start).build(),
            1, i as i32, 1, 1
        );
    }
    info.append(&grid);
    card.append(&info);
    card
}

// ── Public build_page ───────────────────────────────────────
pub fn build_page(is_general: bool) -> gtk::Box {
    let sec = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .build();

    let specs = crate::daemon_client::get_hardware_specs_sync();
    let cpu_lower = specs.cpu_spec.to_lowercase();
    let cpu_max_watt = if cpu_lower.contains("i9") || cpu_lower.contains("hx") {
        157.0_f64
    } else if cpu_lower.contains("i7") {
        115.0_f64
    } else if cpu_lower.contains("ryzen 9") || cpu_lower.contains("ryzen 7") {
        85.0_f64
    } else {
        65.0_f64
    };

    let gpu_lower = specs.gpu_spec.to_lowercase();
    let gpu_max_watt = if gpu_lower.contains("4090") {
        175.0_f64
    } else if gpu_lower.contains("4080") {
        175.0_f64
    } else if gpu_lower.contains("4070") {
        140.0_f64
    } else if gpu_lower.contains("4060") {
        120.0_f64
    } else if gpu_lower.contains("3080") || gpu_lower.contains("3070") {
        140.0_f64
    } else if gpu_lower.contains("3060") {
        115.0_f64
    } else if gpu_lower.contains("radeon") {
        120.0_f64
    } else {
        80.0_f64
    };

    let cpu_watt_ui = build_watt_graph_card(i18n::t("cpu_wattage"), cpu_max_watt);
    let gpu_watt_ui = build_watt_graph_card(i18n::t("gpu_wattage"), gpu_max_watt);
    let watt_ui     = build_watt_card();
    let modes_ui    = build_modes_card();

    let cpu_history = Rc::new(RefCell::new(vec![(0.0f64, false); 60]));
    let gpu_history = Rc::new(RefCell::new(vec![(0.0f64, false); 60]));
    let cpu_temp_hist = Rc::new(RefCell::new(std::collections::VecDeque::<i32>::new()));
    let gpu_temp_hist = Rc::new(RefCell::new(std::collections::VecDeque::<i32>::new()));

    if !is_general {
        // 1. CPU & GPU Wattage History Graphs Row (Monitoring page only)
        let graph_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(10)
            .homogeneous(true)
            .build();
        graph_row.append(&cpu_watt_ui.container);
        graph_row.append(&gpu_watt_ui.container);
        sec.append(&graph_row);
    }


    // ── Daemon Offline Dialog ────────────────────────────────────────────────
    use libadwaita as adw;
    use libadwaita::prelude::*;

    let sec_clone = sec.clone();
    crate::daemon_client::subscribe_daemon_status(move |status| {
        use crate::daemon_client::DaemonStatus;
        if status == DaemonStatus::Offline {
            if let Some(win) = sec_clone.root().and_downcast::<gtk::Window>() {
                let dialog = adw::MessageDialog::new(
                    Some(&win),
                    Some("Daemon Çalışmıyor"),
                    Some("Daemon şu an çalışmıyor. Fan RPM'leri alınamıyor.\nDaemonu yeniden başlatmak ister misiniz?")
                );
                dialog.add_response("cancel", "İptal");
                dialog.add_response("restart", "Yeniden Başlat");
                dialog.set_response_appearance("restart", adw::ResponseAppearance::Suggested);
                
                dialog.connect_response(None, |d, response| {
                    if response == "restart" {
                        let spawned = std::process::Command::new("pkexec")
                            .args(["systemctl", "restart", "omen-space-daemon"])
                            .spawn();
                        if spawned.is_err() {
                            let _ = std::process::Command::new("systemctl")
                                .args(["restart", "omen-space-daemon"])
                                .spawn();
                        }
                    }
                    d.close();
                });
                dialog.present();
            }
        }
    });

    // 2. Main monitor row: CPU + GPU + Device

    let mon_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .homogeneous(true)
        .build();
    let cpu_ui = build_monitor_card(&crate::asset_resolver::get_asset_path("cpu.svg"), "CPU",  "os-prog-cpu", i18n::t("mon_sys_pwr"));
    let gpu_ui = build_monitor_card(&crate::asset_resolver::get_asset_path("gpu.svg"), "GPU",  "os-prog-gpu", i18n::t("mon_sys_pwr"));
    let dev_ui = build_device_card();

    mon_row.append(&cpu_ui.container);
    mon_row.append(&gpu_ui.container);
    mon_row.append(&dev_ui.container);
    sec.append(&mon_row);

    if !is_general {
        let row2 = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(10)
            .homogeneous(true)
            .build();
        row2.append(&watt_ui.container);
        row2.append(&modes_ui.container);
        sec.append(&row2);
    }

    // ── Cairo draw functions ─────────────────────────────────
    let cpu_hist_c = cpu_history.clone();
    cpu_watt_ui.drawing_area.set_draw_func(move |_, cr, w, h| {
        draw_sparkline(cr, w, h, &cpu_hist_c.borrow(), cpu_max_watt, (0.88, 0.20, 0.33));
    });

    let gpu_hist_c = gpu_history.clone();
    gpu_watt_ui.drawing_area.set_draw_func(move |_, cr, w, h| {
        draw_sparkline(cr, w, h, &gpu_hist_c.borrow(), gpu_max_watt, (0.0, 0.60, 0.93));
    });

    // ── 1-second real-time refresh timer ─────────────────────
    let last_throttle = Rc::new(RefCell::new(0u32));

    crate::daemon_client::subscribe_telemetry(move |stats| {
        // 5-second SMA for temperatures
        let mut c_th = cpu_temp_hist.borrow_mut();
        c_th.push_back(stats.cpu_temp);
        if c_th.len() > 5 { c_th.pop_front(); }
        let c_avg = c_th.iter().sum::<i32>() / c_th.len() as i32;

        let mut g_th = gpu_temp_hist.borrow_mut();
        g_th.push_back(stats.gpu_temp);
        if g_th.len() > 5 { g_th.pop_front(); }
        let g_avg = g_th.iter().sum::<i32>() / g_th.len() as i32;

        cpu_ui.temp_label.set_label(&format!("{}°C", c_avg));
        cpu_ui.load_bar.set_fraction(stats.cpu_load);
        cpu_ui.load_pct_label.set_label(&format!("{}%", (stats.cpu_load * 100.0) as i32));
        cpu_ui.pwr_label.set_label(&format!("{:.1}W", stats.cpu_pwr));
        cpu_ui.fan_label.set_label(&format!("{} RPM", if stats.fan1_rpm > 0 { stats.fan1_rpm } else { stats.fan_rpm }));

        gpu_ui.temp_label.set_label(&format!("{}°C", g_avg));
        gpu_ui.load_bar.set_fraction(stats.gpu_load);
        gpu_ui.load_pct_label.set_label(&format!("{}%", (stats.gpu_load * 100.0) as i32));
        if stats.gpu_pwr < 0.0 {
            gpu_ui.pwr_label.set_label("D3Cold");
        } else {
            gpu_ui.pwr_label.set_label(&format!("{:.1}W", stats.gpu_pwr));
        }
        gpu_ui.fan_label.set_label(&format!("{} RPM", if stats.fan2_rpm > 0 { stats.fan2_rpm } else { stats.fan_rpm }));

        dev_ui.ram_bar.set_fraction(stats.ram_frac);
        dev_ui.ram_val_label.set_label(
            &format!("{:.1} / {:.1} GB", stats.ram_used_gb, stats.ram_total_gb));
        dev_ui.disk_bar.set_fraction(stats.disk_frac);
        dev_ui.disk_val_label.set_label(
            &format!("{:.0} / {:.0} GB", stats.disk_used_gb, stats.disk_total_gb));

        if !is_general {
            let mut last = last_throttle.borrow_mut();
            let cpu_throttled = stats.cpu_throttle_count > *last && *last > 0 || stats.cpu_temp >= 95;
            let gpu_throttled = stats.gpu_temp >= 87 || (stats.gpu_load > 0.9 && stats.gpu_pwr >= 0.0 && stats.gpu_pwr < 20.0);
            
            let mut ch = cpu_history.borrow_mut();
            ch.remove(0); ch.push((stats.cpu_pwr, cpu_throttled));
            cpu_watt_ui.drawing_area.queue_draw();

            let pwr = if stats.gpu_pwr < 0.0 { 0.0 } else { stats.gpu_pwr };
            let mut gh = gpu_history.borrow_mut();
            gh.remove(0); gh.push((pwr, gpu_throttled));
            gpu_watt_ui.drawing_area.queue_draw();

            cpu_watt_ui.warning_label.set_visible(cpu_throttled);
            *last = stats.cpu_throttle_count;

            gpu_watt_ui.warning_label.set_visible(gpu_throttled);

            watt_ui.val_label.set_label(&format!("{:.1} W", stats.total_pwr));
        }
    });

    sec
}

// ── Sparkline helper ─────────────────────────────────────────
fn draw_sparkline(
    cr: &gtk::cairo::Context,
    w: i32, h: i32,
    hist: &[(f64, bool)],
    max_val: f64,
    (r, g, b): (f64, f64, f64),
) {
    let n    = hist.len();
    let step = w as f64 / (n - 1).max(1) as f64;

    // Background transparent
    cr.set_operator(gtk::cairo::Operator::Clear);
    cr.paint().expect("Invalid cairo surface");
    cr.set_operator(gtk::cairo::Operator::Over);

    // Grid lines at 25%, 50%, 75%
    cr.set_line_width(0.5);
    cr.set_source_rgba(0.14, 0.14, 0.14, 1.0);
    for p in [0.25, 0.5, 0.75] {
        let y = h as f64 * (1.0 - p);
        cr.move_to(0.0, y); cr.line_to(w as f64, y); let _ = cr.stroke();
    }

    // Fill
    cr.move_to(0.0, h as f64);
    for i in 0..n {
        let yv = (hist[i].0 / max_val).clamp(0.0, 1.0);
        cr.line_to(i as f64 * step, h as f64 - yv * h as f64);
    }
    cr.line_to(w as f64, h as f64);
    cr.close_path();
    cr.set_source_rgba(r, g, b, 0.12);
    let _ = cr.fill();

    // Line (conditional color)
    cr.set_line_width(2.0);
    for i in 1..n {
        let prev_y = (hist[i-1].0 / max_val).clamp(0.0, 1.0);
        let curr_y = (hist[i].0 / max_val).clamp(0.0, 1.0);
        let is_throttled = hist[i].1;

        cr.move_to((i - 1) as f64 * step, h as f64 - prev_y * h as f64);
        cr.line_to(i as f64 * step, h as f64 - curr_y * h as f64);

        if is_throttled {
            cr.set_source_rgba(0.95, 0.77, 0.06, 1.0); // Yellow warning line
        } else {
            cr.set_source_rgba(r, g, b, 1.0); // Normal color
        }
        let _ = cr.stroke();
    }

    // Scale label (Max value)
    cr.select_font_face("Sans", gtk::cairo::FontSlant::Normal, gtk::cairo::FontWeight::Normal);
    cr.set_font_size(9.0);
    cr.set_source_rgba(0.32, 0.32, 0.32, 1.0);
    cr.move_to(4.0, 10.0);
    let _ = cr.show_text(&format!("{:.0}W", max_val));

    // Dynamic endpoint value label
    let last_val = hist[n - 1].0;
    let end_y = h as f64 - (last_val / max_val).clamp(0.0, 1.0) * h as f64;
    cr.set_font_size(11.0);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.9);
    
    // Draw text a bit to the left of the very right edge so it doesn't clip
    cr.move_to(w as f64 - 38.0, end_y - 6.0);
    let _ = cr.show_text(&format!("{:.1}W", last_val));

    // Draw endpoint dot
    cr.arc(w as f64, end_y, 3.0, 0.0, 2.0 * std::f64::consts::PI);
    if hist[n - 1].1 {
        cr.set_source_rgba(0.95, 0.77, 0.06, 1.0);
    } else {
        cr.set_source_rgba(r, g, b, 1.0);
    }
    let _ = cr.fill();
}
