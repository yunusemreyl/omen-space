mod charts;
mod drives_ui;

use gtk::prelude::*;
use libadwaita as adw;
use omen_types::{GpuSampleState, HardwareSpecs, SystemStats};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use crate::daemon_client;
use crate::i18n;
use charts::{
    attach_chart_draw_callback, build_metric_switcher, push_history_sample, ChartMetric,
    HistorySample,
};
use drives_ui::{build_storage_row, drive_blocks, update_drive_rows, DriveRowUI};

/* ─────────────────────────────────────────────────────────
monitoring/mod.rs
1. `build_telemetry_dock()` — Boxless pinned bottom live
   telemetry strip for the Performance landing page.
2. `build_monitor_page()`   — Full System Monitor console
   with unified CPU/GPU live 60s charts, Memory & Storage,
   Battery & Power Health, and Hardware metadata.
───────────────────────────────────────────────────────── */

// ── Shared helpers ──────────────────────────────────────────

fn estimate_max_watts(specs: &HardwareSpecs) -> (f64, f64) {
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
    let gpu_max_watt = if gpu_lower.contains("4090") || gpu_lower.contains("4080") {
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

    (cpu_max_watt, gpu_max_watt)
}

fn format_gpu_power(stats: &SystemStats) -> String {
    if stats.gpu_sample_state == GpuSampleState::Unavailable && stats.gpu_pwr < 0.0 {
        // Real sampling failure (e.g. NVML timeout), not an idle GPU.
        "──".to_string()
    } else if stats.gpu_sample_state == GpuSampleState::Deferred || stats.gpu_pwr <= -1.5 {
        i18n::t("mon_gpu_not_in_use").to_string()
    } else if stats.gpu_sample_state == GpuSampleState::Suspended || stats.gpu_pwr < 0.0 {
        i18n::t("mon_gpu_suspended").to_string()
    } else {
        format!("{:.1} W", stats.gpu_pwr)
    }
}

fn format_battery_status_short(status: omen_types::BatteryStatus) -> &'static str {
    match status {
        omen_types::BatteryStatus::Charging => i18n::t("bat_charging"),
        omen_types::BatteryStatus::Discharging => i18n::t("bat_discharging"),
        omen_types::BatteryStatus::NotCharging => i18n::t("bat_not_charging"),
        omen_types::BatteryStatus::Full => i18n::t("bat_full"),
        omen_types::BatteryStatus::Unknown => "──",
    }
}

fn format_battery_tooltip(bat: &omen_types::BatteryTelemetry) -> Option<String> {
    let mut parts = Vec::new();
    if let (Some(now), Some(full)) = (bat.energy_now_wh, bat.energy_full_wh) {
        parts.push(format!("{:.1} / {:.1} Wh", now, full));
    }
    if let Some(des) = bat.energy_design_wh {
        parts.push(format!("Design: {:.1} Wh", des));
    }
    if let Some(h) = bat.health_pct {
        parts.push(format!("{}: {:.0}%", i18n::t("battery_health"), h));
    }
    if let Some(cycles) = bat.cycle_count {
        parts.push(format!("{}: {}", i18n::t("bat_cycles"), cycles));
    }
    if let Some(v) = bat.voltage_v {
        parts.push(format!("{:.2} V", v));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("  ·  "))
    }
}

fn update_temp_sma(
    cpu_temp_hist: &Rc<RefCell<VecDeque<i32>>>,
    gpu_temp_hist: &Rc<RefCell<VecDeque<i32>>>,
    cpu_temp: i32,
    gpu_temp: i32,
) -> (i32, i32) {
    let mut c_th = cpu_temp_hist.borrow_mut();
    c_th.push_back(cpu_temp);
    if c_th.len() > 5 {
        c_th.pop_front();
    }
    let c_avg = c_th.iter().sum::<i32>() / c_th.len() as i32;

    let mut g_th = gpu_temp_hist.borrow_mut();
    if gpu_temp > 0 {
        g_th.push_back(gpu_temp);
        if g_th.len() > 5 {
            g_th.pop_front();
        }
    } else {
        g_th.clear();
    }
    let g_avg = if g_th.is_empty() {
        0
    } else {
        g_th.iter().sum::<i32>() / g_th.len() as i32
    };

    (c_avg, g_avg)
}

// ═══════════════════════════════════════════════════════════
// 1. PINNED BOXLESS TELEMETRY DOCK (Performance Landing Page)
// ═══════════════════════════════════════════════════════════

struct DockProcCol {
    container: gtk::Box,
    temp_label: gtk::Label,
    load_bar: gtk::ProgressBar,
    load_val: gtk::Label,
    pwr_val: gtk::Label,
    fan_val: gtk::Label,
}

fn build_dock_proc_column(icon_path: &str, title: &str, bar_class: &str) -> DockProcCol {
    let col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(14)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .css_classes(["os-dock-col"])
        .build();

    // Row 1: Icon + Name + Live Temperature
    let head = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    head.append(&gtk::Image::builder().file(icon_path).pixel_size(20).build());
    head.append(
        &gtk::Label::builder()
            .label(title)
            .css_classes(["os-dock-title"])
            .halign(gtk::Align::Start)
            .hexpand(true)
            .build(),
    );
    let temp_label = gtk::Label::builder()
        .label("──°C")
        .css_classes(["os-dock-temp"])
        .halign(gtk::Align::End)
        .build();
    head.append(&temp_label);
    col.append(&head);

    // Row 2: Slim utilization bar
    let load_bar = gtk::ProgressBar::builder()
        .fraction(0.0)
        .css_classes([bar_class, "os-dock-bar"])
        .build();
    col.append(&load_bar);

    // Row 3: Inline tabular telemetry readouts (Load · Power · Fan)
    let stats_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .homogeneous(true)
        .build();

    let make_stat = |lbl: &str, initial: &str| -> (gtk::Box, gtk::Label) {
        let b = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(1)
            .hexpand(true)
            .build();
        b.append(
            &gtk::Label::builder()
                .label(lbl)
                .css_classes(["os-dock-key"])
                .halign(gtk::Align::Start)
                .build(),
        );
        let val = gtk::Label::builder()
            .label(initial)
            .css_classes(["os-dock-val"])
            .halign(gtk::Align::Start)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        b.append(&val);
        (b, val)
    };

    let (b_load, load_val) = make_stat(i18n::t("mon_label_load"), "0%");
    let (b_pwr, pwr_val) = make_stat(i18n::t("mon_label_power"), "── W");
    let (b_fan, fan_val) = make_stat(i18n::t("mon_label_fan"), "── RPM");

    stats_row.append(&b_load);
    stats_row.append(&b_pwr);
    stats_row.append(&b_fan);
    col.append(&stats_row);

    DockProcCol {
        container: col,
        temp_label,
        load_bar,
        load_val,
        pwr_val,
        fan_val,
    }
}

fn build_dock_meter_row(
    title: &str,
    tooltip: Option<&str>,
    bar_class: &str,
    initial_val: &str,
) -> (gtk::Box, gtk::ProgressBar, gtk::Label) {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();

    let head = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    let title_lbl = gtk::Label::builder()
        .label(title)
        .css_classes(["os-dock-key"])
        .halign(gtk::Align::Start)
        .build();
    if let Some(tip) = tooltip {
        title_lbl.set_tooltip_text(Some(tip));
    }
    let val_lbl = gtk::Label::builder()
        .label(initial_val)
        .css_classes(["os-dock-val-sm"])
        .halign(gtk::Align::End)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    head.append(&title_lbl);
    head.append(&val_lbl);
    row.append(&head);

    let bar = gtk::ProgressBar::builder()
        .fraction(0.0)
        .css_classes([bar_class, "os-dock-bar-thin"])
        .build();
    row.append(&bar);

    (row, bar, val_lbl)
}

/// Boxless, unified bottom telemetry dock pinned flush to the bottom of the Performance page.
pub fn build_telemetry_dock() -> gtk::Box {
    let specs = daemon_client::get_hardware_specs_sync();

    let dock_outer = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .hexpand(true)
        .vexpand(false)
        .css_classes(["os-telemetry-dock"])
        .build();

    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .hexpand(true)
        .css_classes(["os-dock-inner"])
        .build();

    // Column 1: CPU
    let cpu_col = build_dock_proc_column(
        &crate::asset_resolver::get_asset_path("cpu.svg"),
        "CPU",
        "os-prog-cpu",
    );

    // Column 2: GPU
    let gpu_col = build_dock_proc_column(
        &crate::asset_resolver::get_asset_path("gpu.svg"),
        "GPU",
        "os-prog-gpu",
    );

    // Column 3: Device Status (RAM + Drives + Battery) — zero outer card, clean stacked meters
    let dev_col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(14)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .css_classes(["os-dock-col"])
        .build();

    let (ram_row, ram_bar, ram_val) = build_dock_meter_row(
        i18n::t("mon_ram"),
        Some(&specs.ram_spec),
        "os-prog-ram",
        "─.─ / ─.─ GB",
    );
    dev_col.append(&ram_row);

    let blocks = drive_blocks(&specs);
    let legacy = specs.drives.is_empty();
    let mut drive_rows = Vec::with_capacity(blocks.len());
    for block in &blocks {
        let (d_row, d_bar, d_val) = build_dock_meter_row(
            &block.title,
            Some(&block.spec),
            "os-prog-disk",
            "─.─ / ─.─ GB",
        );
        dev_col.append(&d_row);
        drive_rows.push(DriveRowUI {
            name: block.name.clone(),
            bar: d_bar,
            val_label: d_val,
        });
    }

    let has_battery = !specs.battery_spec.is_empty();
    let (bat_row, bat_bar, bat_val) = build_dock_meter_row(
        i18n::t("mon_label_battery"),
        if has_battery {
            Some(&specs.battery_spec)
        } else {
            None
        },
        "os-prog-bat",
        "──%",
    );
    bat_row.set_visible(has_battery);
    dev_col.append(&bat_row);

    let vsep1 = gtk::Separator::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["os-dock-vsep"])
        .build();
    let vsep2 = gtk::Separator::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["os-dock-vsep"])
        .build();

    row.append(&cpu_col.container);
    row.append(&vsep1);
    row.append(&gpu_col.container);
    row.append(&vsep2);
    row.append(&dev_col);

    let clamp = adw::Clamp::builder()
        .maximum_size(1040)
        .tightening_threshold(760)
        .child(&row)
        .build();
    dock_outer.append(&clamp);

    // Subscribe to live telemetry stream
    let cpu_temp_hist = Rc::new(RefCell::new(VecDeque::<i32>::new()));
    let gpu_temp_hist = Rc::new(RefCell::new(VecDeque::<i32>::new()));

    daemon_client::subscribe_telemetry(move |stats| {
        let (c_avg, g_avg) = update_temp_sma(
            &cpu_temp_hist,
            &gpu_temp_hist,
            stats.cpu_temp,
            stats.gpu_temp,
        );

        // CPU
        cpu_col.temp_label.set_label(&format!("{}°C", c_avg));
        cpu_col
            .load_bar
            .set_fraction(stats.cpu_load.clamp(0.0, 1.0));
        cpu_col
            .load_val
            .set_label(&format!("{}%", (stats.cpu_load * 100.0).round() as i32));
        cpu_col
            .pwr_val
            .set_label(&format!("{:.1} W", stats.cpu_pwr));
        let f1 = if stats.fan1_rpm > 0 {
            stats.fan1_rpm
        } else {
            stats.fan_rpm
        };
        cpu_col
            .fan_val
            .set_label(&format!("{} {}", f1, i18n::t("mon_rpm")));

        // GPU
        if stats.gpu_temp <= 0 || g_avg <= 0 {
            gpu_col.temp_label.set_label("──");
        } else {
            gpu_col.temp_label.set_label(&format!("{}°C", g_avg));
        }
        gpu_col
            .load_bar
            .set_fraction(stats.gpu_load.clamp(0.0, 1.0));
        gpu_col
            .load_val
            .set_label(&format!("{}%", (stats.gpu_load * 100.0).round() as i32));
        gpu_col.pwr_val.set_label(&format_gpu_power(&stats));
        let f2 = if stats.fan2_rpm > 0 {
            stats.fan2_rpm
        } else {
            stats.fan_rpm
        };
        gpu_col
            .fan_val
            .set_label(&format!("{} {}", f2, i18n::t("mon_rpm")));

        // RAM
        ram_bar.set_fraction(stats.ram_frac.clamp(0.0, 1.0));
        ram_val.set_label(&format!(
            "{:.1} / {:.1} GB",
            stats.ram_used_gb, stats.ram_total_gb
        ));

        // Drives
        update_drive_rows(&drive_rows, legacy, &stats);

        // Battery
        if let Some(ref bat) = stats.battery {
            bat_row.set_visible(true);
            bat_bar.set_fraction((bat.charge_pct as f64 / 100.0).clamp(0.0, 1.0));
            if bat.charge_pct < 20 {
                bat_bar.remove_css_class("os-prog-bat");
                bat_bar.add_css_class("os-prog-bat-low");
            } else {
                bat_bar.remove_css_class("os-prog-bat-low");
                bat_bar.add_css_class("os-prog-bat");
            }

            let status = match bat.status {
                omen_types::BatteryStatus::Charging if bat.power_w > 0.0 => {
                    format!("+{:.1} W", bat.power_w)
                }
                omen_types::BatteryStatus::Discharging if bat.power_w > 0.0 => {
                    format!("-{:.1} W", bat.power_w)
                }
                other => format_battery_status_short(other).to_string(),
            };
            let health_suffix = bat
                .health_pct
                .map(|h| format!(" · {:.0}%", h))
                .unwrap_or_default();
            bat_val.set_label(&format!(
                "{}% · {}{}",
                bat.charge_pct, status, health_suffix
            ));

            let tip = format_battery_tooltip(bat);
            bat_val.set_tooltip_text(tip.as_deref());
            bat_bar.set_tooltip_text(tip.as_deref());
        } else {
            bat_row.set_visible(false);
        }
    });

    dock_outer
}

// ═══════════════════════════════════════════════════════════
// 2. REDESIGNED SYSTEM MONITOR PAGE (Sistem İzleyici)
// ═══════════════════════════════════════════════════════════

struct ProcPanelUI {
    container: gtk::Box,
    temp_label: gtk::Label,
    status_badge: gtk::Label,
    drawing_area: gtk::DrawingArea,
    load_val: gtk::Label,
    load_bar: gtk::ProgressBar,
    pwr_val: gtk::Label,
    fan_val: gtk::Label,
}

fn build_proc_panel(
    icon_path: &str,
    title: &str,
    model_spec: &str,
    bar_class: &str,
) -> ProcPanelUI {
    let panel = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .css_classes(["os-panel"])
        .hexpand(true)
        .build();

    // Header: Icon + Name/Model + Badge + Large Tabular Temp
    let head = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .valign(gtk::Align::Center)
        .build();

    head.append(&gtk::Image::builder().file(icon_path).pixel_size(22).build());

    let name_col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(1)
        .hexpand(true)
        .build();
    name_col.append(
        &gtk::Label::builder()
            .label(title)
            .css_classes(["os-panel-title"])
            .halign(gtk::Align::Start)
            .build(),
    );
    let model_clean = model_spec.split('·').next().unwrap_or(model_spec).trim();
    name_col.append(
        &gtk::Label::builder()
            .label(model_clean)
            .css_classes(["os-panel-sub"])
            .halign(gtk::Align::Start)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .tooltip_text(model_spec)
            .build(),
    );
    head.append(&name_col);

    let status_badge = gtk::Label::builder()
        .css_classes(["badge-warn"])
        .valign(gtk::Align::Center)
        .visible(false)
        .build();
    head.append(&status_badge);

    let temp_label = gtk::Label::builder()
        .label("──°C")
        .css_classes(["os-mon-hero-temp"])
        .halign(gtk::Align::End)
        .valign(gtk::Align::Center)
        .build();
    head.append(&temp_label);

    panel.append(&head);

    // Live 60s Cairo telemetry chart
    let drawing_area = gtk::DrawingArea::builder()
        .hexpand(true)
        .height_request(116)
        .build();
    panel.append(&drawing_area);

    // Hairline divider
    panel.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

    // Inline 3-column metrics footer (Load | Power | Fan)
    let metrics = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(16)
        .homogeneous(true)
        .build();

    // Load column with slim bar
    let load_col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();
    let load_top = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();
    load_top.append(
        &gtk::Label::builder()
            .label(i18n::t("mon_label_load"))
            .css_classes(["os-mon-key"])
            .halign(gtk::Align::Start)
            .hexpand(true)
            .build(),
    );
    let load_val = gtk::Label::builder()
        .label("0%")
        .css_classes(["os-mon-stat"])
        .halign(gtk::Align::End)
        .build();
    load_top.append(&load_val);
    let load_bar = gtk::ProgressBar::builder()
        .fraction(0.0)
        .css_classes([bar_class])
        .margin_top(2)
        .build();
    load_col.append(&load_top);
    load_col.append(&load_bar);

    // Power column
    let pwr_col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .build();
    pwr_col.append(
        &gtk::Label::builder()
            .label(i18n::t("mon_label_power"))
            .css_classes(["os-mon-key"])
            .halign(gtk::Align::Start)
            .build(),
    );
    let pwr_val = gtk::Label::builder()
        .label("── W")
        .css_classes(["os-mon-stat"])
        .halign(gtk::Align::Start)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    pwr_col.append(&pwr_val);

    // Fan column
    let fan_col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .build();
    fan_col.append(
        &gtk::Label::builder()
            .label(i18n::t("mon_label_fan"))
            .css_classes(["os-mon-key"])
            .halign(gtk::Align::Start)
            .build(),
    );
    let fan_val = gtk::Label::builder()
        .label(format!("── {}", i18n::t("mon_rpm")))
        .css_classes(["os-mon-stat"])
        .halign(gtk::Align::Start)
        .build();
    fan_col.append(&fan_val);

    metrics.append(&load_col);
    metrics.append(&pwr_col);
    metrics.append(&fan_col);
    panel.append(&metrics);

    ProcPanelUI {
        container: panel,
        temp_label,
        status_badge,
        drawing_area,
        load_val,
        load_bar,
        pwr_val,
        fan_val,
    }
}

fn make_summary_cell(label: &str, initial: &str) -> (gtk::Box, gtk::Label) {
    let cell = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .css_classes(["os-summary-cell"])
        .build();
    cell.append(
        &gtk::Label::builder()
            .label(label)
            .css_classes(["os-mon-key"])
            .halign(gtk::Align::Start)
            .build(),
    );
    let val = gtk::Label::builder()
        .label(initial)
        .css_classes(["os-summary-val"])
        .halign(gtk::Align::Start)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    cell.append(&val);
    (cell, val)
}

fn make_kv_row(key: &str, initial: &str) -> (gtk::Box, gtk::Label) {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_top(4)
        .margin_bottom(4)
        .build();
    row.append(
        &gtk::Label::builder()
            .label(key)
            .css_classes(["os-mon-key"])
            .halign(gtk::Align::Start)
            .hexpand(true)
            .build(),
    );
    let val = gtk::Label::builder()
        .label(initial)
        .css_classes(["os-mon-kv-val"])
        .halign(gtk::Align::End)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    row.append(&val);
    (row, val)
}

/// Complete, redesigned System Monitor page.
pub fn build_monitor_page() -> gtk::Box {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(13)
        .build();

    let specs = daemon_client::get_hardware_specs_sync();
    let (cpu_max_watt, gpu_max_watt) = estimate_max_watts(&specs);

    // ── Header Row + Chart Metric Switcher ────────────────────────────────────
    let header_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();

    let title_col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(3)
        .hexpand(true)
        .build();
    title_col.append(
        &gtk::Label::builder()
            .label(i18n::t("title_monitoring"))
            .css_classes(["page-title"])
            .halign(gtk::Align::Start)
            .build(),
    );
    let sub_text = if !specs.product_name.is_empty() && specs.product_name != "HP Device" {
        specs.product_name.clone()
    } else {
        i18n::t("monitoring_desc").to_string()
    };
    title_col.append(
        &gtk::Label::builder()
            .label(&sub_text)
            .css_classes(["os-section-desc"])
            .halign(gtk::Align::Start)
            .build(),
    );
    header_row.append(&title_col);

    // ── Live System Summary Strip (Total Power · Profile · Fan · Chassis) ─────
    let summary_strip = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(0)
        .css_classes(["os-summary-strip"])
        .build();

    let (c_tot, total_watt_val) = make_summary_cell(i18n::t("mon_system_total"), "──.─ W");
    let (c_prof, power_mode_val) = make_summary_cell(i18n::t("mon_power_mode"), "──");
    let (c_fan, fan_mode_val) = make_summary_cell(i18n::t("mon_fan_mode"), "──");
    let (c_chassis, chassis_val) = make_summary_cell(i18n::t("mon_chassis"), "──°C");
    let chassis_sep = gtk::Separator::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["os-dock-vsep"])
        .visible(false)
        .build();
    c_chassis.set_visible(false);

    summary_strip.append(&c_tot);
    summary_strip.append(
        &gtk::Separator::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["os-dock-vsep"])
            .build(),
    );
    summary_strip.append(&c_prof);
    summary_strip.append(
        &gtk::Separator::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["os-dock-vsep"])
            .build(),
    );
    summary_strip.append(&c_fan);
    summary_strip.append(&chassis_sep);
    summary_strip.append(&c_chassis);

    // Poll active power profile & fan mode for the summary strip
    {
        let pl = power_mode_val.clone();
        let fl = fan_mode_val.clone();
        let refresh = move || {
            let pl = pl.clone();
            let fl = fl.clone();
            glib::spawn_future_local(async move {
                let power = daemon_client::get_power_profile_async()
                    .await
                    .unwrap_or_else(|_| "balanced".to_string());
                pl.set_label(match power.as_str() {
                    "power-saver" => i18n::t("mode_eco"),
                    "performance" => i18n::t("mode_performance"),
                    _ => i18n::t("mode_balanced"),
                });
                let fan = daemon_client::get_fan_mode_async()
                    .await
                    .unwrap_or_else(|_| "auto".to_string());
                fl.set_label(match fan.as_str() {
                    "max" => i18n::t("fan_max"),
                    "manual" | "custom" => i18n::t("fan_custom"),
                    "ec" => "EC",
                    _ => i18n::t("fan_auto"),
                });
            });
        };
        refresh();
        let anchor = summary_strip.clone();
        glib::timeout_add_local(std::time::Duration::from_secs(3), move || {
            if anchor.is_mapped() {
                refresh();
            }
            glib::ControlFlow::Continue
        });
    }

    // ── Hero Row: Unified CPU & GPU Telemetry Panels ──────────────────────────
    let proc_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(14)
        .homogeneous(true)
        .build();

    let cpu_panel = build_proc_panel(
        &crate::asset_resolver::get_asset_path("cpu.svg"),
        "CPU",
        &specs.cpu_spec,
        "os-prog-cpu",
    );
    let gpu_panel = build_proc_panel(
        &crate::asset_resolver::get_asset_path("gpu.svg"),
        "GPU",
        &specs.gpu_spec,
        "os-prog-gpu",
    );

    proc_row.append(&cpu_panel.container);
    proc_row.append(&gpu_panel.container);

    // Chart metric segmented pill (Power / Temp / Load)
    let active_metric = Rc::new(RefCell::new(ChartMetric::Power));
    let metric_pill = build_metric_switcher(
        &cpu_panel.drawing_area,
        &gpu_panel.drawing_area,
        &active_metric,
    );
    header_row.append(&metric_pill);

    page.append(&header_row);
    page.append(&summary_strip);
    page.append(&proc_row);

    // ── Lower Split: Memory & Storage (Left) + Battery & Power (Right) ────────
    let lower_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(14)
        .build();

    // Left: Memory & Storage panel
    let mem_panel = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .css_classes(["os-panel"])
        .hexpand(true)
        .build();

    let mem_hdr = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    mem_hdr.append(
        &gtk::Image::builder()
            .icon_name("drive-harddisk-symbolic")
            .pixel_size(17)
            .build(),
    );
    mem_hdr.append(
        &gtk::Label::builder()
            .label(i18n::t("mon_memory_storage"))
            .css_classes(["os-panel-title"])
            .halign(gtk::Align::Start)
            .hexpand(true)
            .build(),
    );
    mem_panel.append(&mem_hdr);

    let (ram_row, ram_bar, ram_val) = build_storage_row(
        i18n::t("mon_ram"),
        &specs.ram_spec,
        &specs.ram_spec,
        "os-prog-ram",
    );
    mem_panel.append(&ram_row);

    let blocks = drive_blocks(&specs);
    let legacy = specs.drives.is_empty();
    let mut drive_rows = Vec::with_capacity(blocks.len());
    for block in &blocks {
        mem_panel.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let (d_row, d_bar, d_val) =
            build_storage_row(&block.title, &block.card_spec, &block.spec, "os-prog-disk");
        mem_panel.append(&d_row);
        drive_rows.push(DriveRowUI {
            name: block.name.clone(),
            bar: d_bar,
            val_label: d_val,
        });
    }
    lower_row.append(&mem_panel);

    // Right: Battery & Power Health panel (auto-hidden on desktops)
    let has_battery = !specs.battery_spec.is_empty();
    let bat_panel = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .css_classes(["os-panel"])
        .width_request(330)
        .visible(has_battery)
        .build();

    let bat_hdr = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    bat_hdr.append(
        &gtk::Image::builder()
            .icon_name("battery-symbolic")
            .pixel_size(17)
            .build(),
    );
    bat_hdr.append(
        &gtk::Label::builder()
            .label(i18n::t("mon_battery_power"))
            .css_classes(["os-panel-title"])
            .halign(gtk::Align::Start)
            .hexpand(true)
            .build(),
    );
    let bat_status_pill = gtk::Label::builder()
        .label("──")
        .css_classes(["badge-neutral"])
        .valign(gtk::Align::Center)
        .build();
    bat_hdr.append(&bat_status_pill);
    bat_panel.append(&bat_hdr);

    let bat_hero = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    let bat_charge_lbl = gtk::Label::builder()
        .label("──%")
        .css_classes(["os-mon-hero-temp"])
        .halign(gtk::Align::Start)
        .build();
    let bat_wh_lbl = gtk::Label::builder()
        .label("")
        .css_classes(["os-panel-sub"])
        .halign(gtk::Align::End)
        .valign(gtk::Align::End)
        .hexpand(true)
        .margin_bottom(4)
        .build();
    bat_hero.append(&bat_charge_lbl);
    bat_hero.append(&bat_wh_lbl);
    bat_panel.append(&bat_hero);

    let bat_bar = gtk::ProgressBar::builder()
        .fraction(0.0)
        .css_classes(["os-prog-bat", "os-storage-bar"])
        .build();
    bat_panel.append(&bat_bar);

    let bat_details = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .margin_top(2)
        .build();
    let (r_health, bat_health_val) = make_kv_row(i18n::t("battery_health"), "──");
    let (r_cycles, bat_cycles_val) = make_kv_row(i18n::t("bat_cycles"), "──");
    let (r_power, bat_power_val) = make_kv_row(i18n::t("bat_power"), "──");
    let (r_src, bat_src_val) = make_kv_row(i18n::t("mon_power_source"), "──");

    bat_details.append(&r_health);
    bat_details.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    bat_details.append(&r_cycles);
    bat_details.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    bat_details.append(&r_power);
    bat_details.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    bat_details.append(&r_src);
    bat_panel.append(&bat_details);

    lower_row.append(&bat_panel);
    page.append(&lower_row);

    // ── Quiet Hardware & Firmware Metadata Strip ──────────────────────────────
    let mut meta_items: Vec<(&str, String)> = Vec::new();
    if !specs.os_spec.is_empty() && specs.os_spec != "Linux" {
        meta_items.push(("OS", specs.os_spec.clone()));
    }
    if !specs.bios_version.is_empty() {
        meta_items.push(("BIOS", specs.bios_version.clone()));
    }
    if !specs.ec_version.is_empty() {
        meta_items.push(("EC", specs.ec_version.clone()));
    }
    if !specs.nvidia_driver.is_empty() {
        meta_items.push(("NVIDIA", specs.nvidia_driver.clone()));
    }
    if !meta_items.is_empty() {
        let meta_strip = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .max_children_per_line(6)
            .column_spacing(18)
            .row_spacing(6)
            .css_classes(["os-hw-meta-strip"])
            .build();
        for (k, v) in meta_items {
            let item = gtk::Box::builder()
                .orientation(gtk::Orientation::Horizontal)
                .spacing(6)
                .build();
            item.append(
                &gtk::Label::builder()
                    .label(k)
                    .css_classes(["os-hw-meta-key"])
                    .build(),
            );
            item.append(
                &gtk::Label::builder()
                    .label(&v)
                    .css_classes(["os-hw-meta-val"])
                    .ellipsize(gtk::pango::EllipsizeMode::End)
                    .tooltip_text(&v)
                    .build(),
            );
            meta_strip.append(&item);
        }
        page.append(&meta_strip);
    }

    // ── Live History Buffers + Chart Callbacks ────────────────────────────────
    let cpu_history = Rc::new(RefCell::new(vec![HistorySample::default(); 60]));
    let gpu_history = Rc::new(RefCell::new(vec![HistorySample::default(); 60]));
    let cpu_temp_hist = Rc::new(RefCell::new(VecDeque::<i32>::new()));
    let gpu_temp_hist = Rc::new(RefCell::new(VecDeque::<i32>::new()));
    let last_throttle = Rc::new(RefCell::new(0u32));

    attach_chart_draw_callback(
        &cpu_panel.drawing_area,
        cpu_history.clone(),
        active_metric.clone(),
        cpu_max_watt,
        (0.88, 0.27, 0.38),
    );
    attach_chart_draw_callback(
        &gpu_panel.drawing_area,
        gpu_history.clone(),
        active_metric,
        gpu_max_watt,
        (0.17, 0.60, 0.95),
    );

    // ── Subscribe to Telemetry Stream ─────────────────────────────────────────
    daemon_client::subscribe_telemetry(move |stats| {
        let (c_avg, g_avg) = update_temp_sma(
            &cpu_temp_hist,
            &gpu_temp_hist,
            stats.cpu_temp,
            stats.gpu_temp,
        );

        // Summary strip
        let prefix = if stats.total_power_estimated == Some(true) {
            "~"
        } else {
            ""
        };
        total_watt_val.set_label(&format!("{}{:.1} W", prefix, stats.total_pwr));
        total_watt_val.set_tooltip_text(Some(if stats.total_power_partial == Some(true) {
            "Estimated total; one or more power components are unavailable."
        } else if stats.total_power_estimated == Some(true) {
            "Estimated from sampled CPU/GPU power plus a fixed overhead."
        } else {
            "System power"
        }));

        if (15..=120).contains(&stats.chassis_temp) {
            chassis_sep.set_visible(true);
            c_chassis.set_visible(true);
            chassis_val.set_label(&format!("{}°C", stats.chassis_temp));
        } else {
            chassis_sep.set_visible(false);
            c_chassis.set_visible(false);
        }

        // Throttle detection
        let mut last = last_throttle.borrow_mut();
        let cpu_throttled = (stats.cpu_throttle_count > *last && *last > 0) || stats.cpu_temp >= 95;
        let gpu_throttled = stats.gpu_temp >= 87
            || (stats.gpu_load > 0.9 && stats.gpu_pwr >= 0.0 && stats.gpu_pwr < 20.0);
        *last = stats.cpu_throttle_count;

        // CPU panel update
        cpu_panel.temp_label.set_label(&format!("{}°C", c_avg));
        cpu_panel
            .load_bar
            .set_fraction(stats.cpu_load.clamp(0.0, 1.0));
        cpu_panel
            .load_val
            .set_label(&format!("{}%", (stats.cpu_load * 100.0).round() as i32));
        cpu_panel
            .pwr_val
            .set_label(&format!("{:.1} W", stats.cpu_pwr));
        let f1 = if stats.fan1_rpm > 0 {
            stats.fan1_rpm
        } else {
            stats.fan_rpm
        };
        cpu_panel
            .fan_val
            .set_label(&format!("{} {}", f1, i18n::t("mon_rpm")));
        if cpu_throttled {
            cpu_panel
                .status_badge
                .set_label(i18n::t("throttle_warning"));
            cpu_panel.status_badge.set_css_classes(&["badge-warn"]);
            cpu_panel.status_badge.set_visible(true);
        } else {
            cpu_panel.status_badge.set_visible(false);
        }

        // GPU panel update
        // The daemon deliberately stops polling an idle dGPU so it can sleep.
        // Say so instead of showing placeholder dashes that look like an error.
        let gpu_idle = matches!(
            stats.gpu_sample_state,
            GpuSampleState::Deferred | GpuSampleState::Suspended
        ) || (stats.gpu_sample_state == GpuSampleState::Unknown
            && stats.gpu_pwr < 0.0);
        if gpu_idle {
            gpu_panel.temp_label.set_label(i18n::t("mon_gpu_idle"));
        } else if stats.gpu_temp <= 0 || g_avg <= 0 {
            gpu_panel.temp_label.set_label("──");
        } else {
            gpu_panel.temp_label.set_label(&format!("{}°C", g_avg));
        }
        gpu_panel
            .load_bar
            .set_fraction(stats.gpu_load.clamp(0.0, 1.0));
        gpu_panel
            .load_val
            .set_label(&format!("{}%", (stats.gpu_load * 100.0).round() as i32));
        gpu_panel.pwr_val.set_label(&format_gpu_power(&stats));
        let f2 = if stats.fan2_rpm > 0 {
            stats.fan2_rpm
        } else {
            stats.fan_rpm
        };
        gpu_panel
            .fan_val
            .set_label(&format!("{} {}", f2, i18n::t("mon_rpm")));
        if gpu_throttled {
            gpu_panel
                .status_badge
                .set_label(i18n::t("throttle_warning"));
            gpu_panel.status_badge.set_css_classes(&["badge-warn"]);
            gpu_panel.status_badge.set_visible(true);
        } else if stats.gpu_sample_state == GpuSampleState::Suspended
            || (stats.gpu_pwr < 0.0 && stats.gpu_pwr > -1.5)
        {
            gpu_panel
                .status_badge
                .set_label(i18n::t("mon_gpu_suspended"));
            gpu_panel.status_badge.set_css_classes(&["badge-neutral"]);
            gpu_panel.status_badge.set_visible(true);
        } else {
            gpu_panel.status_badge.set_visible(false);
        }

        // Update 60s ring buffers
        push_history_sample(
            &cpu_history,
            HistorySample {
                power: stats.cpu_pwr.max(0.0),
                temp: c_avg.max(0) as f64,
                load_pct: (stats.cpu_load * 100.0).clamp(0.0, 100.0),
                throttled: cpu_throttled,
            },
            &cpu_panel.drawing_area,
        );
        push_history_sample(
            &gpu_history,
            HistorySample {
                power: if stats.gpu_pwr < 0.0 {
                    0.0
                } else {
                    stats.gpu_pwr
                },
                temp: g_avg.max(0) as f64,
                load_pct: (stats.gpu_load * 100.0).clamp(0.0, 100.0),
                throttled: gpu_throttled,
            },
            &gpu_panel.drawing_area,
        );

        // Memory
        let ram_pct = (stats.ram_frac * 100.0).round() as i32;
        ram_bar.set_fraction(stats.ram_frac.clamp(0.0, 1.0));
        ram_val.set_label(&format!(
            "{:.1} / {:.1} GB  ({}%)",
            stats.ram_used_gb, stats.ram_total_gb, ram_pct
        ));

        // Drives
        update_drive_rows(&drive_rows, legacy, &stats);

        // Battery panel
        if let Some(ref bat) = stats.battery {
            bat_panel.set_visible(true);
            bat_charge_lbl.set_label(&format!("{}%", bat.charge_pct));
            bat_bar.set_fraction((bat.charge_pct as f64 / 100.0).clamp(0.0, 1.0));
            if bat.charge_pct < 20 {
                bat_bar.remove_css_class("os-prog-bat");
                bat_bar.add_css_class("os-prog-bat-low");
            } else {
                bat_bar.remove_css_class("os-prog-bat-low");
                bat_bar.add_css_class("os-prog-bat");
            }

            bat_status_pill.set_label(format_battery_status_short(bat.status));
            let badge_cls = match bat.status {
                omen_types::BatteryStatus::Charging | omen_types::BatteryStatus::Full => "badge-ok",
                omen_types::BatteryStatus::Discharging if bat.charge_pct < 20 => "badge-err",
                omen_types::BatteryStatus::Discharging => "badge-warn",
                _ => "badge-neutral",
            };
            bat_status_pill.set_css_classes(&[badge_cls]);

            match (bat.energy_now_wh, bat.energy_full_wh) {
                (Some(now), Some(full)) => {
                    bat_wh_lbl.set_label(&format!("{:.1} / {:.1} Wh", now, full))
                }
                _ => bat_wh_lbl.set_label(""),
            }

            if let Some(h) = bat.health_pct {
                let cap = match (bat.energy_full_wh, bat.energy_design_wh) {
                    (Some(f), Some(d)) => format!(" ({:.1} / {:.1} Wh)", f, d),
                    _ => String::new(),
                };
                bat_health_val.set_label(&format!("{:.1}%{}", h, cap));
            } else {
                bat_health_val.set_label("──");
            }

            if let Some(cycles) = bat.cycle_count {
                bat_cycles_val.set_label(&cycles.to_string());
            } else {
                bat_cycles_val.set_label("──");
            }

            let pwr_str = if bat.power_w > 0.0 {
                format!("{:.1} W", bat.power_w)
            } else {
                "0.0 W".to_string()
            };
            let volt_str = bat
                .voltage_v
                .map(|v| format!(" · {:.2} V", v))
                .unwrap_or_default();
            bat_power_val.set_label(&format!("{}{}", pwr_str, volt_str));

            let src_base = if bat.ac_online {
                i18n::t("mon_ac_adapter")
            } else {
                i18n::t("mon_on_battery")
            };
            if let Some(lim) = bat.charge_limit_pct {
                bat_src_val.set_label(&format!(
                    "{} ({}: {}%)",
                    src_base,
                    i18n::t("mon_charge_limit"),
                    lim
                ));
            } else {
                bat_src_val.set_label(src_base);
            }
        } else {
            bat_panel.set_visible(false);
        }
    });

    page
}
