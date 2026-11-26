use gtk::prelude::*;
use omen_types::{DriveInfo, DriveUsage, HardwareSpecs, SystemStats};

use crate::i18n;

pub(super) struct DriveRowUI {
    pub(super) name: String,
    pub(super) bar: gtk::ProgressBar,
    pub(super) val_label: gtk::Label,
}

// ── Drive presentation helpers (pure, unit-tested below) ────

/// What one drive looks like in the UI: the kernel name that links it to telemetry, a short
/// title ("NVMe 1") and two renderings of its description.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct DriveBlock {
    pub(super) name: String,
    pub(super) title: String,
    /// Full line for the spec header and tooltips: "SAMSUNG MZVL81T0HFLB-00BH1  ·  1024 GB".
    pub(super) spec: String,
    /// Short line for compact device status rows.
    pub(super) card_spec: String,
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
pub(super) fn drive_blocks(specs: &HardwareSpecs) -> Vec<DriveBlock> {
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
pub(super) fn usage_display(u: Option<&DriveUsage>) -> (String, f64) {
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
pub(super) fn mounts_tooltip(u: Option<&DriveUsage>) -> Option<String> {
    let u = u.filter(|u| !u.mounts.is_empty())?;
    Some(i18n::t("mon_drive_mounts").replace("{}", &u.mounts.join(", ")))
}

pub(super) fn build_storage_row(
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

pub(super) fn update_drive_rows(drive_rows: &[DriveRowUI], legacy: bool, stats: &SystemStats) {
    for row in drive_rows {
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
        assert_eq!(
            drive_spec_text(&d),
            "SAMSUNG MZVL81T0HFLB-00BH1  ·  1024 GB"
        );
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
        assert_eq!(
            format_usage(47_602_962_432, 1_024_199_012_352),
            "48 / 1024 GB"
        );
        assert_eq!(
            format_usage(218_836_664_320, 1_967_845_998_592),
            "219 / 1968 GB"
        );
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
        assert_eq!(
            blocks[0].spec,
            "SAMSUNG MZVL81T0HFLB-00BH1  ·  1024 GB NVMe"
        );
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
        assert!(
            !tip.contains("{}"),
            "template placeholder must be filled: {tip}"
        );
    }
}
