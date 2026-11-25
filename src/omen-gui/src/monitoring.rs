use gtk::prelude::*;
use libadwaita as adw;
use omen_types::{DriveInfo, DriveUsage, GpuSampleState, HardwareSpecs, SystemStats};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use crate::daemon_client;
use crate::i18n;

/* ─────────────────────────────────────────────────────────
   monitoring.rs
   1. `build_telemetry_dock()` — Boxless pinned bottom live
      telemetry strip for the Performance landing page.
   2. `build_monitor_page()`   — Full System Monitor console
      with unified CPU/GPU live 60s charts, Memory & Storage,
      Battery & Power Health, and Hardware metadata.
   ───────────────────────────────────────────────────────── */

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChartMetric {
    Power,
    Temp,
    Load,
}

#[derive(Clone, Copy, Default)]
struct HistorySample {
    power: f64,
    temp: f64,
    load_pct: f64,
    throttled: bool,
}

struct DriveRowUI {
    name: String,
    bar: gtk::ProgressBar,
    val_label: gtk::Label,
}

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
    if stats.gpu_sample_state == GpuSampleState::Deferred || stats.gpu_pwr <= -1.5 {
        i18n::t("mon_gpu_not_sampled").to_string()
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

// ── Drive presentation helpers (pure, unit-tested below) ────

/// What one drive looks like in the UI: the kernel name that links it to telemetry, a short
/// title ("NVMe 1") and two renderings of its description.
#[derive(Debug, PartialEq, Eq)]
struct DriveBlock {
    name: String,
    title: String,
    /// Full line for the spec header and tooltips: "SAMSUNG MZVL81T0HFLB-00BH1  ·  1024 GB".
    spec: String,
    /// Short line for compact device status rows.
    card_spec: String,
}

/// Row titles: the drive kind, numbered per kind only when several drives share it, so an HDD
/// never reads "SSD" and two NVMe drives read "NVMe 1" / "NVMe 2".
fn drive_titles(drives: &[DriveInfo]) -> Vec<String> {
    drives
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let label = d.kind.label();
            if drives.iter().filter(|o| o.kind == d.kind).count() > 1 {
                let nth = drives[..=i].iter().filter(|o| o.kind == d.kind).count();
                format!("{label} {nth}")
            } else {
                label.to_string()
            }
        })
        .collect()
}

/// Decimal gigabytes, the unit drives are sold in; used for capacity and usage alike so the
/// two agree ("48 / 1024 GB").
fn gb(bytes: u64) -> f64 {
    bytes as f64 / 1e9
}

fn drive_capacity_text(d: &DriveInfo) -> String {
    format!("{:.0} GB", gb(d.size_bytes))
}

/// "MODEL  ·  1024 GB", or just the capacity when the model is unknown.
fn drive_spec_text(d: &DriveInfo) -> String {
    if d.model.is_empty() {
        drive_capacity_text(d)
    } else {
        format!("{}  ·  {}", d.model, drive_capacity_text(d))
    }
}

/// The model alone, or the capacity when the model is unknown.
fn drive_card_text(d: &DriveInfo) -> String {
    if d.model.is_empty() {
        drive_capacity_text(d)
    } else {
        d.model.clone()
    }
}

/// "47 / 1024 GB"
fn format_usage(used_bytes: u64, total_bytes: u64) -> String {
    format!("{:.0} / {:.0} GB", gb(used_bytes), gb(total_bytes))
}

fn usage_fraction(used_bytes: u64, total_bytes: u64) -> f64 {
    if total_bytes == 0 {
        0.0
    } else {
        (used_bytes as f64 / total_bytes as f64).clamp(0.0, 1.0)
    }
}

/// One block per drive. An older daemon sends no `drives`; its single `ssd_spec` line becomes
/// one block titled like the old "SSD" row.
fn drive_blocks(specs: &HardwareSpecs) -> Vec<DriveBlock> {
    if specs.drives.is_empty() {
        return vec![DriveBlock {
            name: String::new(),
            title: i18n::t("mon_disk").to_string(),
            spec: specs.ssd_spec.clone(),
            card_spec: specs.ssd_spec.clone(),
        }];
    }
    drive_titles(&specs.drives)
        .into_iter()
        .zip(&specs.drives)
        .map(|(title, d)| DriveBlock {
            name: d.name.clone(),
            title,
            spec: drive_spec_text(d),
            card_spec: drive_card_text(d),
        })
        .collect()
}

/// Caption and bar fraction for one drive's latest usage sample. A drive with nothing mounted
/// has a zero total and gets a text instead of a misleading "0 / 0 GB".
fn usage_display(u: Option<&DriveUsage>) -> (String, f64) {
    match u {
        Some(u) if u.usage_complete == Some(false) => (
            if u.total_bytes > 0 {
                format!("{} (partial)", format_usage(u.used_bytes, u.total_bytes))
            } else {
                "Usage unavailable".to_string()
            },
            0.0,
        ),
        Some(u) if u.total_bytes > 0 => (
            format_usage(u.used_bytes, u.total_bytes),
            usage_fraction(u.used_bytes, u.total_bytes),
        ),
        Some(u) if !u.mounts.is_empty() => (u.mounts.join(", "), 0.0),
        Some(_) => (i18n::t("mon_disk_unmounted").to_string(), 0.0),
        None => ("─.─ / ─.─ GB".to_string(), 0.0),
    }
}

/// Tooltip listing where a drive is mounted, or `None` when nothing is.
fn mounts_tooltip(u: Option<&DriveUsage>) -> Option<String> {
    let u = u.filter(|u| !u.mounts.is_empty())?;
    Some(i18n::t("mon_drive_mounts").replace("{}", &u.mounts.join(", ")))
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
        if has_battery { Some(&specs.battery_spec) } else { None },
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
        let mut c_th = cpu_temp_hist.borrow_mut();
        c_th.push_back(stats.cpu_temp);
        if c_th.len() > 5 {
            c_th.pop_front();
        }
        let c_avg = c_th.iter().sum::<i32>() / c_th.len() as i32;

        let mut g_th = gpu_temp_hist.borrow_mut();
        if stats.gpu_temp > 0 {
            g_th.push_back(stats.gpu_temp);
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

        // CPU
        cpu_col.temp_label.set_label(&format!("{}°C", c_avg));
        cpu_col.load_bar.set_fraction(stats.cpu_load.clamp(0.0, 1.0));
        cpu_col.load_val.set_label(&format!("{}%", (stats.cpu_load * 100.0).round() as i32));
        cpu_col.pwr_val.set_label(&format!("{:.1} W", stats.cpu_pwr));
        let f1 = if stats.fan1_rpm > 0 { stats.fan1_rpm } else { stats.fan_rpm };
        cpu_col.fan_val.set_label(&format!("{} {}", f1, i18n::t("mon_rpm")));

        // GPU
        if stats.gpu_temp <= 0 || g_avg <= 0 {
            gpu_col.temp_label.set_label("──");
        } else {
            gpu_col.temp_label.set_label(&format!("{}°C", g_avg));
        }
        gpu_col.load_bar.set_fraction(stats.gpu_load.clamp(0.0, 1.0));
        gpu_col.load_val.set_label(&format!("{}%", (stats.gpu_load * 100.0).round() as i32));
        gpu_col.pwr_val.set_label(&format_gpu_power(&stats));
        let f2 = if stats.fan2_rpm > 0 { stats.fan2_rpm } else { stats.fan_rpm };
        gpu_col.fan_val.set_label(&format!("{} {}", f2, i18n::t("mon_rpm")));

        // RAM
        ram_bar.set_fraction(stats.ram_frac.clamp(0.0, 1.0));
        ram_val.set_label(&format!("{:.1} / {:.1} GB", stats.ram_used_gb, stats.ram_total_gb));

        // Drives
        for row in &drive_rows {
            if legacy {
                row.bar.set_fraction(stats.disk_frac.clamp(0.0, 1.0));
                row.val_label.set_label(&format!(
                    "{:.0} / {:.0} GB",
                    stats.disk_used_gb, stats.disk_total_gb
                ));
                continue;
            }
            let usage = stats.drives.iter().find(|u| u.name == row.name);
            let (text, frac) = usage_display(usage);
            row.bar.set_fraction(frac);
            row.val_label.set_label(&text);
            let tip = mounts_tooltip(usage);
            if row.val_label.tooltip_text().map(|t| t.to_string()) != tip {
                row.val_label.set_tooltip_text(tip.as_deref());
                row.bar.set_tooltip_text(tip.as_deref());
            }
        }

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
            bat_val.set_label(&format!("{}% · {}{}", bat.charge_pct, status, health_suffix));

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

fn build_storage_row(
    title: &str,
    sub: &str,
    tooltip: &str,
    bar_class: &str,
) -> (gtk::Box, gtk::ProgressBar, gtk::Label) {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();

    let head = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .build();

    let left = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .hexpand(true)
        .build();
    left.append(
        &gtk::Label::builder()
            .label(title)
            .css_classes(["os-storage-title"])
            .halign(gtk::Align::Start)
            .build(),
    );
    left.append(
        &gtk::Label::builder()
            .label(sub)
            .css_classes(["os-panel-sub"])
            .halign(gtk::Align::Start)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .tooltip_text(tooltip)
            .build(),
    );
    head.append(&left);

    let val_label = gtk::Label::builder()
        .label("─.─ / ─.─ GB")
        .css_classes(["os-mon-kv-val"])
        .halign(gtk::Align::End)
        .build();
    head.append(&val_label);
    row.append(&head);

    let bar = gtk::ProgressBar::builder()
        .fraction(0.0)
        .css_classes([bar_class, "os-storage-bar"])
        .build();
    row.append(&bar);

    (row, bar, val_label)
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

    // Chart metric segmented pill (Power / Temp / Load)
    let metric_pill = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .css_classes(["curve-pill"])
        .valign(gtk::Align::Center)
        .build();
    let btn_pwr = gtk::ToggleButton::builder()
        .label(i18n::t("mon_metric_power"))
        .css_classes(["curve-pill-btn"])
        .active(true)
        .build();
    let btn_tmp = gtk::ToggleButton::builder()
        .label(i18n::t("mon_metric_temp"))
        .css_classes(["curve-pill-btn"])
        .group(&btn_pwr)
        .build();
    let btn_load = gtk::ToggleButton::builder()
        .label(i18n::t("mon_metric_load"))
        .css_classes(["curve-pill-btn"])
        .group(&btn_pwr)
        .build();
    metric_pill.append(&btn_pwr);
    metric_pill.append(&btn_tmp);
    metric_pill.append(&btn_load);
    header_row.append(&metric_pill);
    page.append(&header_row);

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
    page.append(&summary_strip);

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
        let (d_row, d_bar, d_val) = build_storage_row(
            &block.title,
            &block.card_spec,
            &block.spec,
            "os-prog-disk",
        );
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

    // ── Live History Buffers + Chart Metric State ─────────────────────────────
    let active_metric = Rc::new(RefCell::new(ChartMetric::Power));
    let cpu_history = Rc::new(RefCell::new(vec![HistorySample::default(); 60]));
    let gpu_history = Rc::new(RefCell::new(vec![HistorySample::default(); 60]));
    let cpu_temp_hist = Rc::new(RefCell::new(VecDeque::<i32>::new()));
    let gpu_temp_hist = Rc::new(RefCell::new(VecDeque::<i32>::new()));
    let last_throttle = Rc::new(RefCell::new(0u32));

    // Wire metric switcher buttons
    {
        let m = active_metric.clone();
        let c_da = cpu_panel.drawing_area.clone();
        let g_da = gpu_panel.drawing_area.clone();
        btn_pwr.connect_toggled(move |b| {
            if b.is_active() {
                *m.borrow_mut() = ChartMetric::Power;
                c_da.queue_draw();
                g_da.queue_draw();
            }
        });
    }
    {
        let m = active_metric.clone();
        let c_da = cpu_panel.drawing_area.clone();
        let g_da = gpu_panel.drawing_area.clone();
        btn_tmp.connect_toggled(move |b| {
            if b.is_active() {
                *m.borrow_mut() = ChartMetric::Temp;
                c_da.queue_draw();
                g_da.queue_draw();
            }
        });
    }
    {
        let m = active_metric.clone();
        let c_da = cpu_panel.drawing_area.clone();
        let g_da = gpu_panel.drawing_area.clone();
        btn_load.connect_toggled(move |b| {
            if b.is_active() {
                *m.borrow_mut() = ChartMetric::Load;
                c_da.queue_draw();
                g_da.queue_draw();
            }
        });
    }

    // Cairo draw callbacks (theme-adaptive!)
    {
        let hist = cpu_history.clone();
        let metric = active_metric.clone();
        cpu_panel.drawing_area.set_draw_func(move |widget, cr, w, h| {
            draw_telemetry_chart(
                widget,
                cr,
                w,
                h,
                &hist.borrow(),
                *metric.borrow(),
                cpu_max_watt,
                (0.88, 0.27, 0.38),
            );
        });
    }
    {
        let hist = gpu_history.clone();
        let metric = active_metric.clone();
        gpu_panel.drawing_area.set_draw_func(move |widget, cr, w, h| {
            draw_telemetry_chart(
                widget,
                cr,
                w,
                h,
                &hist.borrow(),
                *metric.borrow(),
                gpu_max_watt,
                (0.17, 0.60, 0.95),
            );
        });
    }

    // ── Subscribe to Telemetry Stream ─────────────────────────────────────────
    daemon_client::subscribe_telemetry(move |stats| {
        // 5-second SMA for temperatures
        let mut c_th = cpu_temp_hist.borrow_mut();
        c_th.push_back(stats.cpu_temp);
        if c_th.len() > 5 {
            c_th.pop_front();
        }
        let c_avg = c_th.iter().sum::<i32>() / c_th.len() as i32;

        let mut g_th = gpu_temp_hist.borrow_mut();
        if stats.gpu_temp > 0 {
            g_th.push_back(stats.gpu_temp);
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
        let cpu_throttled =
            (stats.cpu_throttle_count > *last && *last > 0) || stats.cpu_temp >= 95;
        let gpu_throttled = stats.gpu_temp >= 87
            || (stats.gpu_load > 0.9 && stats.gpu_pwr >= 0.0 && stats.gpu_pwr < 20.0);
        *last = stats.cpu_throttle_count;

        // CPU panel update
        cpu_panel.temp_label.set_label(&format!("{}°C", c_avg));
        cpu_panel.load_bar.set_fraction(stats.cpu_load.clamp(0.0, 1.0));
        cpu_panel
            .load_val
            .set_label(&format!("{}%", (stats.cpu_load * 100.0).round() as i32));
        cpu_panel.pwr_val.set_label(&format!("{:.1} W", stats.cpu_pwr));
        let f1 = if stats.fan1_rpm > 0 {
            stats.fan1_rpm
        } else {
            stats.fan_rpm
        };
        cpu_panel
            .fan_val
            .set_label(&format!("{} {}", f1, i18n::t("mon_rpm")));
        if cpu_throttled {
            cpu_panel.status_badge.set_label(i18n::t("throttle_warning"));
            cpu_panel.status_badge.set_css_classes(&["badge-warn"]);
            cpu_panel.status_badge.set_visible(true);
        } else {
            cpu_panel.status_badge.set_visible(false);
        }

        // GPU panel update
        if stats.gpu_temp <= 0 || g_avg <= 0 {
            gpu_panel.temp_label.set_label("──");
        } else {
            gpu_panel.temp_label.set_label(&format!("{}°C", g_avg));
        }
        gpu_panel.load_bar.set_fraction(stats.gpu_load.clamp(0.0, 1.0));
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
            gpu_panel.status_badge.set_label(i18n::t("throttle_warning"));
            gpu_panel.status_badge.set_css_classes(&["badge-warn"]);
            gpu_panel.status_badge.set_visible(true);
        } else if stats.gpu_sample_state == GpuSampleState::Suspended
            || (stats.gpu_pwr < 0.0 && stats.gpu_pwr > -1.5)
        {
            gpu_panel.status_badge.set_label(i18n::t("mon_gpu_suspended"));
            gpu_panel.status_badge.set_css_classes(&["badge-neutral"]);
            gpu_panel.status_badge.set_visible(true);
        } else {
            gpu_panel.status_badge.set_visible(false);
        }

        // Update 60s ring buffers
        {
            let mut ch = cpu_history.borrow_mut();
            ch.remove(0);
            ch.push(HistorySample {
                power: stats.cpu_pwr.max(0.0),
                temp: c_avg.max(0) as f64,
                load_pct: (stats.cpu_load * 100.0).clamp(0.0, 100.0),
                throttled: cpu_throttled,
            });
        }
        cpu_panel.drawing_area.queue_draw();

        {
            let mut gh = gpu_history.borrow_mut();
            gh.remove(0);
            gh.push(HistorySample {
                power: if stats.gpu_pwr < 0.0 { 0.0 } else { stats.gpu_pwr },
                temp: g_avg.max(0) as f64,
                load_pct: (stats.gpu_load * 100.0).clamp(0.0, 100.0),
                throttled: gpu_throttled,
            });
        }
        gpu_panel.drawing_area.queue_draw();

        // Memory
        let ram_pct = (stats.ram_frac * 100.0).round() as i32;
        ram_bar.set_fraction(stats.ram_frac.clamp(0.0, 1.0));
        ram_val.set_label(&format!(
            "{:.1} / {:.1} GB  ({}%)",
            stats.ram_used_gb, stats.ram_total_gb, ram_pct
        ));

        // Drives
        for row in &drive_rows {
            if legacy {
                row.bar.set_fraction(stats.disk_frac.clamp(0.0, 1.0));
                row.val_label.set_label(&format!(
                    "{:.0} / {:.0} GB",
                    stats.disk_used_gb, stats.disk_total_gb
                ));
                continue;
            }
            let usage = stats.drives.iter().find(|u| u.name == row.name);
            let (text, frac) = usage_display(usage);
            row.bar.set_fraction(frac);
            row.val_label.set_label(&text);
            let tip = mounts_tooltip(usage);
            if row.val_label.tooltip_text().map(|t| t.to_string()) != tip {
                row.val_label.set_tooltip_text(tip.as_deref());
                row.bar.set_tooltip_text(tip.as_deref());
            }
        }

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
                (Some(now), Some(full)) => bat_wh_lbl.set_label(&format!("{:.1} / {:.1} Wh", now, full)),
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
                bat_src_val.set_label(&format!("{} ({}: {}%)", src_base, i18n::t("mon_charge_limit"), lim));
            } else {
                bat_src_val.set_label(src_base);
            }
        } else {
            bat_panel.set_visible(false);
        }
    });

    page
}

// ── Theme-Adaptive Cairo Area Chart ─────────────────────────

#[allow(clippy::too_many_arguments)]
fn draw_telemetry_chart(
    widget: &gtk::DrawingArea,
    cr: &gtk::cairo::Context,
    w: i32,
    h: i32,
    hist: &[HistorySample],
    metric: ChartMetric,
    max_watt: f64,
    (r, g, b): (f64, f64, f64),
) {
    let n = hist.len();
    if n < 2 || w <= 4 || h <= 4 {
        return;
    }

    let fg = widget.color();
    let (fr, fg_c, fb) = (fg.red() as f64, fg.green() as f64, fg.blue() as f64);

    let wf = w as f64;
    let hf = h as f64;
    let pad_t = 18.0;
    let pad_b = 6.0;
    let chart_h = (hf - pad_t - pad_b).max(1.0);
    let step = wf / (n - 1) as f64;

    let (max_val, unit, decimals) = match metric {
        ChartMetric::Power => (max_watt.max(10.0), "W", 1usize),
        ChartMetric::Temp => (105.0_f64, "°C", 0usize),
        ChartMetric::Load => (100.0_f64, "%", 0usize),
    };

    let sample_val = |s: &HistorySample| -> f64 {
        match metric {
            ChartMetric::Power => s.power,
            ChartMetric::Temp => s.temp,
            ChartMetric::Load => s.load_pct,
        }
    };

    // Transparent canvas
    cr.set_operator(gtk::cairo::Operator::Clear);
    let _ = cr.paint();
    cr.set_operator(gtk::cairo::Operator::Over);

    // Subtle horizontal guide lines (25%, 50%, 75%, 100%)
    cr.set_line_width(1.0);
    cr.set_source_rgba(fr, fg_c, fb, 0.07);
    for p in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let y = (pad_t + chart_h * (1.0 - p)).round() + 0.5;
        cr.move_to(0.0, y);
        cr.line_to(wf, y);
        let _ = cr.stroke();
    }

    // Soft vertical gradient fill under the telemetry line
    cr.move_to(0.0, pad_t + chart_h);
    for (i, item) in hist.iter().enumerate() {
        let yv = (sample_val(item) / max_val).clamp(0.0, 1.0);
        cr.line_to(i as f64 * step, pad_t + chart_h * (1.0 - yv));
    }
    cr.line_to(wf, pad_t + chart_h);
    cr.close_path();

    let grad = gtk::cairo::LinearGradient::new(0.0, pad_t, 0.0, pad_t + chart_h);
    grad.add_color_stop_rgba(0.0, r, g, b, 0.24);
    grad.add_color_stop_rgba(1.0, r, g, b, 0.01);
    let _ = cr.set_source(&grad);
    let _ = cr.fill();

    // Primary stroke (gold segment if throttled)
    cr.set_line_width(2.0);
    cr.set_line_join(gtk::cairo::LineJoin::Round);
    cr.set_line_cap(gtk::cairo::LineCap::Round);
    for i in 1..n {
        let prev_y = (sample_val(&hist[i - 1]) / max_val).clamp(0.0, 1.0);
        let curr_y = (sample_val(&hist[i]) / max_val).clamp(0.0, 1.0);
        cr.move_to((i - 1) as f64 * step, pad_t + chart_h * (1.0 - prev_y));
        cr.line_to(i as f64 * step, pad_t + chart_h * (1.0 - curr_y));
        if hist[i].throttled {
            cr.set_source_rgba(0.96, 0.65, 0.14, 1.0);
        } else {
            cr.set_source_rgba(r, g, b, 1.0);
        }
        let _ = cr.stroke();
    }

    // Top-left scale & peak caption
    let peak = hist.iter().map(sample_val).fold(0.0_f64, f64::max);
    let max_lbl = format!("{:.0}{}", max_val, unit);
    let peak_lbl = if decimals == 1 {
        format!("{} {:.1}{}", i18n::t("mon_peak"), peak, unit)
    } else {
        format!("{} {:.0}{}", i18n::t("mon_peak"), peak, unit)
    };

    cr.select_font_face(
        "Sans",
        gtk::cairo::FontSlant::Normal,
        gtk::cairo::FontWeight::Normal,
    );
    cr.set_font_size(9.5);
    cr.set_source_rgba(fr, fg_c, fb, 0.45);
    cr.move_to(2.0, 11.0);
    let _ = cr.show_text(&max_lbl);

    if peak > 0.0 {
        let tw = cr.text_extents(&peak_lbl).map(|e| e.width()).unwrap_or(48.0);
        cr.move_to((wf - tw - 4.0).max(46.0), 11.0);
        let _ = cr.show_text(&peak_lbl);
    }

    // Endpoint dot + clamped current value label
    let last_val = sample_val(&hist[n - 1]);
    let end_y = pad_t + chart_h * (1.0 - (last_val / max_val).clamp(0.0, 1.0));
    let val_str = if decimals == 1 {
        format!("{:.1}{}", last_val, unit)
    } else {
        format!("{:.0}{}", last_val, unit)
    };

    cr.select_font_face(
        "Sans",
        gtk::cairo::FontSlant::Normal,
        gtk::cairo::FontWeight::Bold,
    );
    cr.set_font_size(10.5);
    let vw = cr.text_extents(&val_str).map(|e| e.width()).unwrap_or(32.0);
    let label_y = (end_y - 7.0).clamp(pad_t + 12.0, pad_t + chart_h - 4.0);
    cr.set_source_rgba(fr, fg_c, fb, 0.90);
    cr.move_to((wf - vw - 10.0).max(2.0), label_y);
    let _ = cr.show_text(&val_str);

    // Endpoint indicator dot
    let dot_x = wf - 3.5;
    cr.set_source_rgba(r, g, b, 0.28);
    cr.arc(dot_x, end_y, 6.0, 0.0, std::f64::consts::TAU);
    let _ = cr.fill();

    if hist[n - 1].throttled {
        cr.set_source_rgba(0.96, 0.65, 0.14, 1.0);
    } else {
        cr.set_source_rgba(r, g, b, 1.0);
    }
    cr.arc(dot_x, end_y, 3.2, 0.0, std::f64::consts::TAU);
    let _ = cr.fill();
}

#[cfg(test)]
mod tests {
    use super::*;
    use omen_types::DriveKind;

    fn drive(name: &str, model: &str, kind: DriveKind, size_bytes: u64) -> DriveInfo {
        DriveInfo {
            name: name.into(),
            model: model.into(),
            kind,
            size_bytes,
        }
    }

    fn usage(name: &str, used: u64, total: u64, mounts: &[&str]) -> DriveUsage {
        DriveUsage {
            name: name.into(),
            used_bytes: used,
            total_bytes: total,
            mounts: mounts.iter().map(|m| m.to_string()).collect(),
            usage_complete: None,
        }
    }

    fn specs_with(drives: Vec<DriveInfo>, ssd_spec: &str) -> HardwareSpecs {
        HardwareSpecs {
            drives,
            ssd_spec: ssd_spec.into(),
            ..Default::default()
        }
    }

    #[test]
    fn partial_and_unavailable_samples_are_not_presented_as_complete_usage() {
        let mut sample = usage("sda", 100, 400, &["/"]);
        sample.usage_complete = Some(false);
        let (label, fraction) = usage_display(Some(&sample));
        assert!(label.contains("partial"));
        assert_eq!(fraction, 0.0);
        sample.used_bytes = 0;
        sample.total_bytes = 0;
        assert_eq!(usage_display(Some(&sample)).0, "Usage unavailable");
    }

    #[test]
    fn a_single_drive_title_is_just_its_kind() {
        let d = [drive("nvme0n1", "M", DriveKind::Nvme, 1)];
        assert_eq!(drive_titles(&d), vec!["NVMe"]);
    }

    #[test]
    fn drives_of_the_same_kind_are_numbered_and_other_kinds_are_not() {
        let d = [
            drive("nvme0n1", "A", DriveKind::Nvme, 1),
            drive("sda", "B", DriveKind::Hdd, 1),
            drive("nvme1n1", "C", DriveKind::Nvme, 1),
            drive("mmcblk0", "D", DriveKind::Emmc, 1),
        ];
        assert_eq!(drive_titles(&d), vec!["NVMe 1", "HDD", "NVMe 2", "eMMC"]);
    }

    #[test]
    fn spec_text_is_model_and_decimal_capacity() {
        let d = drive(
            "nvme0n1",
            "SAMSUNG MZVL81T0HFLB-00BH1",
            DriveKind::Nvme,
            1_024_209_543_168,
        );
        assert_eq!(drive_spec_text(&d), "SAMSUNG MZVL81T0HFLB-00BH1  ·  1024 GB");
        let unknown = drive("sdb", "", DriveKind::Hdd, 2_000_398_934_016);
        assert_eq!(
            drive_spec_text(&unknown),
            "2000 GB",
            "no dangling separator without a model"
        );
        assert_eq!(
            drive_card_text(&d),
            "SAMSUNG MZVL81T0HFLB-00BH1",
            "the card leaves the capacity to its usage caption"
        );
        assert_eq!(drive_card_text(&unknown), "2000 GB");
    }

    #[test]
    fn usage_is_shown_in_decimal_gb_so_it_matches_the_capacity() {
        assert_eq!(format_usage(47_602_962_432, 1_024_199_012_352), "48 / 1024 GB");
        assert_eq!(format_usage(218_836_664_320, 1_967_845_998_592), "219 / 1968 GB");
    }

    #[test]
    fn fraction_is_clamped_and_safe_for_empty_totals() {
        assert_eq!(usage_fraction(0, 0), 0.0);
        assert_eq!(usage_fraction(5, 0), 0.0);
        assert_eq!(usage_fraction(50, 100), 0.5);
        assert_eq!(usage_fraction(200, 100), 1.0);
    }

    #[test]
    fn blocks_carry_the_kernel_name_that_links_them_to_telemetry() {
        let specs = specs_with(
            vec![
                drive("nvme0n1", "Samsung", DriveKind::Nvme, 1_000_000_000_000),
                drive("nvme1n1", "Sabrent", DriveKind::Nvme, 2_000_000_000_000),
            ],
            "ignored when drives are present",
        );
        let blocks = drive_blocks(&specs);
        assert_eq!(blocks.len(), 2);
        assert_eq!(
            blocks[1],
            DriveBlock {
                name: "nvme1n1".into(),
                title: "NVMe 2".into(),
                spec: "Sabrent  ·  2000 GB".into(),
                card_spec: "Sabrent".into(),
            }
        );
    }

    #[test]
    fn an_older_daemon_without_a_drive_list_gets_one_legacy_block() {
        let specs = specs_with(vec![], "SAMSUNG MZVL81T0HFLB-00BH1  ·  1024 GB NVMe");
        let blocks = drive_blocks(&specs);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].spec, "SAMSUNG MZVL81T0HFLB-00BH1  ·  1024 GB NVMe");
        assert_eq!(blocks[0].card_spec, blocks[0].spec);
        assert!(blocks[0].name.is_empty());
    }

    #[test]
    fn usage_display_covers_mounted_unmounted_and_missing_drives() {
        let (text, frac) = usage_display(Some(&usage("a", 50, 100, &["/"])));
        assert_eq!(text, "0 / 0 GB");
        assert_eq!(frac, 0.5);

        let (text, frac) = usage_display(Some(&usage("a", 0, 0, &[])));
        assert_eq!(text, i18n::t("mon_disk_unmounted"));
        assert_eq!(frac, 0.0);

        let (text, frac) = usage_display(None);
        assert_eq!(text, "─.─ / ─.─ GB");
        assert_eq!(frac, 0.0);
    }

    #[test]
    fn mounts_tooltip_lists_mountpoints_only_when_there_are_some() {
        assert_eq!(mounts_tooltip(None), None);
        assert_eq!(mounts_tooltip(Some(&usage("a", 1, 2, &[]))), None);
        let tip = mounts_tooltip(Some(&usage("a", 1, 2, &["/", "/boot"]))).unwrap();
        assert!(tip.contains("/, /boot"), "{tip}");
        assert!(!tip.contains("{}"), "template placeholder must be filled: {tip}");
    }
}
