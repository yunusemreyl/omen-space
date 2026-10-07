//! Internal storage drives: which ones exist and how full they are.
//!
//! The functions here are deliberately pure: they take a sysfs root and plain strings instead of
//! reading the live system, so the unit tests below can run them against fake trees. The thin
//! wrappers at the bottom (`current_drives`, `current_usage`, `refresh_specs`) bind them to the
//! real `/sys`, `/proc/self/mountinfo` and `statvfs`. Nothing here spawns a process: the GUI's
//! hardware-specs call has a 500 ms timeout, and sysfs reads are sub-millisecond.
//!
//! Known limits (documented, not bugs):
//! * A filesystem that spans several disks (md, LVM, multi-device btrfs) is attributed to the
//!   first underlying disk.
//! * Thunderbolt/USB4 NVMe enclosures look like internal PCIe NVMe drives to the kernel and
//!   are listed.
//! * The daemon's unit sets `ProtectHome=true`, which removes `/home` (a separate partition
//!   only; a `/home` subvolume of `/` is fine) from the mount table the daemon sees.

use omen_types::{DriveInfo, DriveKind, DriveUsage, HardwareSpecs};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// sysfs reports sizes in 512-byte units regardless of the device's logical sector size.
const SECTOR_BYTES: u64 = 512;
/// How many mountpoints are reported per drive (one per mounted filesystem on it).
const MAX_REPORTED_MOUNTS: usize = 6;
/// Guard against dm/md stacking cycles.
const MAX_STACK_DEPTH: usize = 8;

/// Block device name prefixes that are never a physical internal drive: loop and RAM disks,
/// compressed RAM (zram swap), device-mapper / md / ZFS volumes, optical, floppy, network and
/// null block devices.
const EXCLUDED_PREFIXES: [&str; 11] =
    ["loop", "ram", "zram", "dm-", "md", "sr", "fd", "nbd", "nullb", "zd", "rbd"];

// ── Enumeration ──────────────────────────────────────────────────────────────

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Collapse the kernel's space padding ("SAMSUNG X   ") and any runs of blanks.
fn tidy(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// NVMe multipath "head" nodes such as `nvme0c0n1` back a visible `nvme0n1`; skip them.
fn is_nvme_multipath_node(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("nvme") else {
        return false;
    };
    let Some((ctrl, tail)) = rest.split_once('c') else {
        return false;
    };
    !ctrl.is_empty() && ctrl.bytes().all(|b| b.is_ascii_digit()) && tail.contains('n')
}

fn is_excluded_name(name: &str) -> bool {
    EXCLUDED_PREFIXES.iter().any(|p| name.starts_with(p))
        || is_nvme_multipath_node(name)
        || (name.starts_with("mmcblk") && (name.contains("boot") || name.contains("rpmb")))
}

/// True for devices that hang off a USB bus or live under `/devices/virtual/`.
fn is_external_or_virtual(real_device_path: &Path) -> bool {
    let text = real_device_path.to_string_lossy();
    if text.contains("/devices/virtual/") {
        return true;
    }
    real_device_path.components().any(|c| {
        let c = c.as_os_str().to_string_lossy();
        c.len() > 3 && c.starts_with("usb") && c[3..].bytes().all(|b| b.is_ascii_digit())
    })
}

fn classify(name: &str, rotational: bool) -> DriveKind {
    if name.starts_with("nvme") {
        DriveKind::Nvme
    } else if name.starts_with("mmcblk") {
        DriveKind::Emmc
    } else if rotational {
        DriveKind::Hdd
    } else {
        DriveKind::Ssd
    }
}

/// Probe one `/sys/block/<name>` entry; `None` if it is not an internal physical drive.
fn probe_disk(block_dir: &Path, name: &str) -> Option<DriveInfo> {
    if is_excluded_name(name) {
        return None;
    }
    let dir = block_dir.join(name);
    if read_trimmed(&dir.join("hidden")).as_deref() == Some("1")
        || read_trimmed(&dir.join("removable")).as_deref() == Some("1")
    {
        return None;
    }
    let sectors: u64 = read_trimmed(&dir.join("size"))?.parse().ok()?;
    if sectors == 0 {
        return None;
    }
    // A real drive has a `device` link to its controller; purely virtual block devices do not.
    let device = dir.join("device");
    let device_real = fs::canonicalize(&device).ok()?;
    if is_external_or_virtual(&device_real) {
        return None;
    }
    // eMMC reports type MMC; SD cards (also `mmcblk*`) report SD and are removable media.
    if name.starts_with("mmcblk") && read_trimmed(&device.join("type")).as_deref() != Some("MMC") {
        return None;
    }
    let rotational = read_trimmed(&dir.join("queue/rotational")).as_deref() == Some("1");
    let model = read_trimmed(&device.join("model"))
        .or_else(|| read_trimmed(&device.join("name")))
        .map(|m| tidy(&m))
        .unwrap_or_default();
    Some(DriveInfo {
        name: name.to_string(),
        model,
        kind: classify(name, rotational),
        size_bytes: sectors.saturating_mul(SECTOR_BYTES),
    })
}

/// Order device names the way people expect: NVMe first, then SATA/SCSI, then eMMC; digits
/// compare numerically (`nvme2n1 < nvme10n1`) and `sdb < sdaa` (shorter letter run first).
fn name_order_key(name: &str) -> (u8, Vec<(u8, usize, String, u64)>) {
    let family = if name.starts_with("nvme") {
        0
    } else if name.starts_with("mmcblk") {
        2
    } else if name.starts_with("sd") || name.starts_with("vd") || name.starts_with("hd") {
        1
    } else {
        3
    };
    let mut tokens = Vec::new();
    let mut chars = name.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            let mut n: u64 = 0;
            while let Some(d) = chars.peek().and_then(|c| c.to_digit(10)) {
                n = n.saturating_mul(10).saturating_add(d as u64);
                chars.next();
            }
            tokens.push((1, 0, String::new(), n));
        } else {
            let mut text = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_ascii_digit() {
                    break;
                }
                text.push(c);
                chars.next();
            }
            tokens.push((0, text.len(), text, 0));
        }
    }
    (family, tokens)
}

/// Every internal physical drive under `sys_root` (normally `/sys`), in natural order.
pub fn list_internal_drives(sys_root: &Path) -> Vec<DriveInfo> {
    let block_dir = sys_root.join("block");
    let Ok(entries) = fs::read_dir(&block_dir) else {
        return Vec::new();
    };
    let mut drives: Vec<DriveInfo> = entries
        .flatten()
        .filter_map(|e| probe_disk(&block_dir, &e.file_name().to_string_lossy()))
        .collect();
    drives.sort_by(|a, b| name_order_key(&a.name).cmp(&name_order_key(&b.name)));
    drives
}

// ── Mount table ──────────────────────────────────────────────────────────────

/// One mounted block-device filesystem from `/proc/self/mountinfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountEntry {
    /// `major:minor` of the mount (anonymous `0:N` for btrfs).
    pub dev: String,
    pub mountpoint: String,
    pub fstype: String,
    /// The mount source, e.g. `/dev/nvme0n1p2`.
    pub source: String,
}

/// Undo the kernel's octal escaping of mount fields (`\040` space, `\011` tab, `\134` backslash).
pub fn unescape_mount_field(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 3 < bytes.len()
            && bytes[i + 1..=i + 3].iter().all(|b| (b'0'..=b'7').contains(b))
        {
            let value = (bytes[i + 1] - b'0') as u32 * 64
                + (bytes[i + 2] - b'0') as u32 * 8
                + (bytes[i + 3] - b'0') as u32;
            out.push((value & 0xFF) as u8);
            i += 4;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parse `/proc/self/mountinfo`, keeping only filesystems backed by a `/dev/...` source and
/// dropping read-only media (squashfs, iso9660, udf) that are not "drive usage".
pub fn parse_mountinfo(text: &str) -> Vec<MountEntry> {
    text.lines()
        .filter_map(|line| {
            // "36 35 98:0 /root /mnt rw,noatime shared:1 - ext3 /dev/root rw"
            let (left, right) = line.split_once(" - ")?;
            let mut left = left.split_whitespace();
            let dev = left.nth(2)?;
            let mountpoint = left.nth(1)?;
            let mut right = right.split_whitespace();
            let fstype = right.next()?;
            let source = right.next()?;
            if !source.starts_with("/dev/") || matches!(fstype, "squashfs" | "iso9660" | "udf") {
                return None;
            }
            Some(MountEntry {
                dev: dev.to_string(),
                mountpoint: unescape_mount_field(mountpoint),
                fstype: fstype.to_string(),
                source: unescape_mount_field(source),
            })
        })
        .collect()
}

// ── Mount source -> physical disk ────────────────────────────────────────────

/// Kernel block device name for a mount source, without touching `/dev`.
fn block_name_from_source(sys_root: &Path, source: &str) -> Option<String> {
    let base = source.strip_prefix("/dev/")?;
    if let Some(mapper) = base.strip_prefix("mapper/") {
        // /dev/mapper/<name> -> the dm-N whose `dm/name` is <name>
        let entries = fs::read_dir(sys_root.join("block")).ok()?;
        return entries.flatten().find_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            (n.starts_with("dm-")
                && read_trimmed(&e.path().join("dm/name")).as_deref() == Some(mapper))
            .then_some(n)
        });
    }
    if base.contains('/') {
        // /dev/disk/by-uuid/... and friends are symlinks to the real node.
        return fs::canonicalize(source)
            .ok()?
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
    }
    Some(base.to_string())
}

/// Fallback for sources that are not real nodes (e.g. `/dev/root`): use the mount's major:minor.
fn block_name_from_devnum(sys_root: &Path, devnum: &str) -> Option<String> {
    if devnum.starts_with("0:") {
        return None; // anonymous device (btrfs, nfs...), nothing to look up
    }
    fs::canonicalize(sys_root.join("dev/block").join(devnum))
        .ok()?
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
}

/// Walk from a block device (partition, dm, md or whole disk) down to the physical disk.
fn physical_disk_of(sys_root: &Path, name: &str, depth: usize) -> Option<String> {
    if depth > MAX_STACK_DEPTH {
        return None;
    }
    let block_dir = sys_root.join("block");
    let top = block_dir.join(name);
    if top.exists() {
        // Whole device. dm/md devices list what they are built on under `slaves/`.
        let mut slaves: Vec<String> = fs::read_dir(top.join("slaves"))
            .map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        slaves.sort_by(|a, b| name_order_key(a).cmp(&name_order_key(b)));
        return match slaves.first() {
            Some(slave) => physical_disk_of(sys_root, slave, depth + 1),
            None => Some(name.to_string()),
        };
    }
    // A partition: its directory sits inside the parent disk's directory.
    for entry in fs::read_dir(&block_dir).ok()?.flatten() {
        if entry.path().join(name).is_dir() {
            let parent = entry.file_name().to_string_lossy().into_owned();
            return physical_disk_of(sys_root, &parent, depth + 1);
        }
    }
    None
}

/// The physical disk a mount lives on, or `None` for virtual/unknown sources.
pub fn resolve_physical_disk(sys_root: &Path, mount: &MountEntry) -> Option<String> {
    let name = block_name_from_source(sys_root, &mount.source)
        .filter(|n| sys_root.join("class/block").join(n).exists() || sys_root.join("block").join(n).exists())
        .or_else(|| block_name_from_devnum(sys_root, &mount.dev))?;
    physical_disk_of(sys_root, &name, 0)
}

// ── Usage ────────────────────────────────────────────────────────────────────

/// Used/total bytes of one filesystem, computed the way `df` does (used = blocks - free blocks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FsUsage {
    pub used: u64,
    pub total: u64,
}

/// Ordering key that prefers `/` over `/boot` over `/var/log` (fewest path components, then
/// shortest text).
fn mount_rank(mountpoint: &str) -> (usize, usize) {
    (mountpoint.split('/').filter(|c| !c.is_empty()).count(), mountpoint.len())
}

/// Per-drive usage, one entry per drive in `drives` (same order). Mounts are grouped by source
/// so that e.g. seven btrfs subvolume mounts of one partition count once, and each source
/// contributes one representative mountpoint to `DriveUsage::mounts`. For each source the
/// first mountpoint whose `statvfs` succeeds is used, because some mountpoints can be
/// unreadable from inside the service's sandbox. A drive with nothing mounted keeps zeros.
pub fn drive_usage(
    drives: &[DriveInfo],
    mounts: &[MountEntry],
    sys_root: &Path,
    statvfs: &dyn Fn(&str) -> Option<FsUsage>,
) -> Vec<DriveUsage> {
    let mut usage: Vec<DriveUsage> = drives
        .iter()
        .map(|d| DriveUsage { name: d.name.clone(), ..Default::default() })
        .collect();

    let mut order: Vec<&str> = Vec::new();
    let mut by_source: HashMap<&str, Vec<&MountEntry>> = HashMap::new();
    for m in mounts {
        by_source
            .entry(m.source.as_str())
            .or_insert_with(|| {
                order.push(m.source.as_str());
                Vec::new()
            })
            .push(m);
    }

    for source in order {
        let group = &by_source[source];
        let Some(disk) = resolve_physical_disk(sys_root, group[0]) else {
            continue;
        };
        let Some(slot) = usage.iter_mut().find(|u| u.name == disk) else {
            continue; // a USB or virtual disk: not one of the internal drives
        };
        // One mountpoint per filesystem is enough for the tooltip: the shortest one, so seven
        // btrfs subvolumes (or the service sandbox's own bind mounts such as
        // `/etc/omen-space`) do not crowd out the partitions that matter.
        if let Some(shortest) = group.iter().min_by_key(|m| mount_rank(&m.mountpoint)) {
            if slot.mounts.len() < MAX_REPORTED_MOUNTS && !slot.mounts.contains(&shortest.mountpoint) {
                slot.mounts.push(shortest.mountpoint.clone());
            }
        }
        if let Some(fs) = group.iter().find_map(|m| statvfs(&m.mountpoint)) {
            slot.used_bytes = slot.used_bytes.saturating_add(fs.used);
            slot.total_bytes = slot.total_bytes.saturating_add(fs.total);
        }
    }
    usage
}

// ── Legacy fields for clients that predate per-drive support ─────────────────

/// Decimal gigabytes, matching how drives are sold and the old "1024 GB NVMe" text.
pub fn size_gb(bytes: u64) -> u64 {
    (bytes as f64 / 1e9).round() as u64
}

/// The old single-line `ssd_spec` text, now describing the first drive; empty if there is none
/// (the GUI then shows its own "Unknown" default instead of the old fake "NVMe SSD").
pub fn legacy_ssd_spec(drives: &[DriveInfo]) -> String {
    let Some(first) = drives.first() else {
        return String::new();
    };
    let model = if first.model.is_empty() { first.kind.label() } else { first.model.as_str() };
    format!("{}  ·  {} GB {}", model, size_gb(first.size_bytes), first.kind.label())
}

/// `(used_gb, total_gb, fraction)` of the first drive, for the old scalar `disk_*` fields.
/// They describe the same drive as `legacy_ssd_spec`, so an old GUI's label and bar agree.
pub fn legacy_disk_fields(usage: &[DriveUsage]) -> (f64, f64, f64) {
    let Some(first) = usage.first() else {
        return (0.0, 0.0, 0.0);
    };
    let used = first.used_bytes as f64 / 1e9;
    let total = first.total_bytes as f64 / 1e9;
    let frac = if first.total_bytes > 0 { (used / total).clamp(0.0, 1.0) } else { 0.0 };
    (used, total, frac)
}

// ── Bindings to the live system ──────────────────────────────────────────────

/// Internal drives of the machine this daemon runs on.
pub fn current_drives() -> Vec<DriveInfo> {
    list_internal_drives(Path::new("/sys"))
}

/// Live per-drive usage for `drives` (one entry each, same order).
pub fn current_usage(drives: &[DriveInfo]) -> Vec<DriveUsage> {
    let text = fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
    drive_usage(drives, &parse_mountinfo(&text), Path::new("/sys"), &statvfs_bytes)
}

/// Refresh the drive-related fields of a (possibly cached) specs struct in place.
pub fn refresh_specs(specs: &mut HardwareSpecs) {
    specs.drives = current_drives();
    specs.ssd_spec = legacy_ssd_spec(&specs.drives);
}

fn statvfs_bytes(mountpoint: &str) -> Option<FsUsage> {
    let path = std::ffi::CString::new(mountpoint).ok()?;
    // SAFETY: `path` is a valid NUL-terminated string and `stat` is a zeroed plain-old-data
    // struct that statvfs(3) fills in; neither outlives this call.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(path.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    let frsize = stat.f_frsize as u64;
    let total = (stat.f_blocks as u64).saturating_mul(frsize);
    if total == 0 {
        return None;
    }
    let free = (stat.f_bfree as u64).saturating_mul(frsize);
    Some(FsUsage { used: total.saturating_sub(free), total })
}

#[cfg(test)]
mod tests {
    use super::*;
    use omen_types::{HardwareSpecs, SystemStats};
    use serde::Deserialize;
    use std::cell::Cell;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;

    /// A fake `/sys` (plus `dev/block`) under a unique temp dir, removed on drop.
    struct Tree {
        root: PathBuf,
    }

    impl Tree {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().join(format!("omen-drives-{}-{}", std::process::id(), tag));
            let _ = fs::remove_dir_all(&root);
            for d in ["block", "class/block", "dev/block"] {
                fs::create_dir_all(root.join(d)).unwrap();
            }
            Tree { root }
        }

        fn sys(&self) -> &Path {
            &self.root
        }

        /// A disk. `hw` is the controller directory under `devices/` that the disk's `device`
        /// link points at (it holds `model`, `type`, ...).
        fn disk(&self, name: &str, hw: &str, sectors: u64, rotational: bool, model: Option<&str>) {
            let ctrl = self.root.join("devices").join(hw);
            fs::create_dir_all(&ctrl).unwrap();
            if let Some(m) = model {
                fs::write(ctrl.join("model"), format!("{m}\n")).unwrap();
            }
            let dir = ctrl.join(name);
            fs::create_dir_all(dir.join("queue")).unwrap();
            fs::write(dir.join("size"), format!("{sectors}\n")).unwrap();
            fs::write(dir.join("removable"), "0\n").unwrap();
            fs::write(dir.join("queue/rotational"), if rotational { "1\n" } else { "0\n" }).unwrap();
            symlink(&ctrl, dir.join("device")).unwrap();
            symlink(&dir, self.root.join("block").join(name)).unwrap();
            symlink(&dir, self.root.join("class/block").join(name)).unwrap();
        }

        fn hw_attr(&self, hw: &str, file: &str, value: &str) {
            fs::write(self.root.join("devices").join(hw).join(file), format!("{value}\n")).unwrap();
        }

        fn disk_attr(&self, name: &str, file: &str, value: &str) {
            fs::write(self.root.join("block").join(name).join(file), format!("{value}\n")).unwrap();
        }

        fn partition(&self, disk: &str, name: &str, number: u32) {
            let dir = fs::canonicalize(self.root.join("block").join(disk)).unwrap().join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("partition"), format!("{number}\n")).unwrap();
            symlink(&dir, self.root.join("class/block").join(name)).unwrap();
        }

        /// A device-mapper node `dm-N` named `mapper` (as in /dev/mapper/<mapper>) built on `slave`.
        fn dm(&self, name: &str, mapper: &str, slave: &str) {
            let dir = self.root.join("devices/virtual/block").join(name);
            fs::create_dir_all(dir.join("dm")).unwrap();
            fs::create_dir_all(dir.join("slaves").join(slave)).unwrap();
            fs::write(dir.join("dm/name"), format!("{mapper}\n")).unwrap();
            symlink(&dir, self.root.join("block").join(name)).unwrap();
            symlink(&dir, self.root.join("class/block").join(name)).unwrap();
        }

        /// `/sys/dev/block/<major:minor>` -> the named block device.
        fn devnum(&self, majmin: &str, class_name: &str) {
            let target = fs::canonicalize(self.root.join("class/block").join(class_name)).unwrap();
            symlink(target, self.root.join("dev/block").join(majmin)).unwrap();
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    const NVME0_HW: &str = "pci0000:00/0000:00:01.2/0000:02:00.0/nvme/nvme0";
    const NVME1_HW: &str = "pci0000:00/0000:00:02.1/0000:06:00.0/nvme/nvme1";

    /// This machine: two NVMe drives, a 4 GB vfat /boot + btrfs root on the first, ext4 on the
    /// second, plus zram swap. Sizes and model strings (with the kernel's padding) are real.
    fn this_machine(tag: &str) -> Tree {
        let t = Tree::new(tag);
        t.disk("nvme0n1", NVME0_HW, 2_000_409_264, false, Some("SAMSUNG MZVL81T0HFLB-00BH1              "));
        t.disk("nvme1n1", NVME1_HW, 3_907_029_168, false, Some("Sabrent Rocket Q4                       "));
        t.partition("nvme0n1", "nvme0n1p1", 1);
        t.partition("nvme0n1", "nvme0n1p2", 2);
        t.partition("nvme1n1", "nvme1n1p1", 1);
        t.disk("zram0", "virtual/block/zram0", 63_735_808, false, None);
        t
    }

    /// The real block-device lines of this machine's /proc/self/mountinfo (plus pseudo noise).
    const THIS_MACHINE_MOUNTINFO: &str = "\
22 1 0:21 / /proc rw,nosuid,nodev,noexec,relatime shared:12 - proc proc rw
23 1 0:22 / /sys rw,nosuid,nodev,noexec,relatime shared:2 - sysfs sysfs rw
24 1 0:5 / /dev rw,nosuid shared:3 - devtmpfs dev rw,size=4096k
40 1 0:33 /@ / rw,noatime shared:1 - btrfs /dev/nvme0n1p2 rw,compress=zstd:1,ssd,discard=async,space_cache=v2,subvolid=256,subvol=/@
55 40 0:33 /@home /home rw,noatime shared:71 - btrfs /dev/nvme0n1p2 rw,compress=zstd:1,ssd,discard=async,space_cache=v2,subvolid=257,subvol=/@home
60 40 0:33 /@root /root rw,noatime shared:75 - btrfs /dev/nvme0n1p2 rw,compress=zstd:1,ssd,discard=async,space_cache=v2,subvolid=258,subvol=/@root
61 40 0:33 /@srv /srv rw,noatime shared:117 - btrfs /dev/nvme0n1p2 rw,compress=zstd:1,ssd,discard=async,space_cache=v2,subvolid=259,subvol=/@srv
72 40 0:33 /@cache /var/cache rw,noatime shared:125 - btrfs /dev/nvme0n1p2 rw,compress=zstd:1,ssd,discard=async,space_cache=v2,subvolid=260,subvol=/@cache
126 40 0:33 /@log /var/log rw,noatime shared:129 - btrfs /dev/nvme0n1p2 rw,compress=zstd:1,ssd,discard=async,space_cache=v2,subvolid=262,subvol=/@log
134 40 0:33 /@tmp /var/tmp rw,noatime shared:133 - btrfs /dev/nvme0n1p2 rw,compress=zstd:1,ssd,discard=async,space_cache=v2,subvolid=261,subvol=/@tmp
165 40 259:3 / /boot rw,relatime shared:137 - vfat /dev/nvme0n1p1 rw,fmask=0077,dmask=0077,codepage=437,iocharset=ascii,shortname=mixed,utf8,errors=remount-ro
180 40 259:2 / /mnt/games rw,nosuid,nodev,noatime shared:145 - ext4 /dev/nvme1n1p1 rw
200 1 0:50 / /run/user/1000 rw,nosuid,nodev,relatime shared:150 - tmpfs tmpfs rw,size=3M
";

    fn table(rows: &[(&str, u64, u64)]) -> HashMap<String, FsUsage> {
        rows.iter()
            .map(|(mp, used, total)| (mp.to_string(), FsUsage { used: *used, total: *total }))
            .collect()
    }

    // ── enumeration ──

    #[test]
    fn this_machine_lists_both_nvme_drives_and_hides_zram() {
        let t = this_machine("machine");
        let drives = list_internal_drives(t.sys());
        assert_eq!(drives.len(), 2, "zram0 must not be listed: {drives:?}");
        assert_eq!(drives[0].name, "nvme0n1");
        assert_eq!(drives[0].model, "SAMSUNG MZVL81T0HFLB-00BH1", "kernel padding must be trimmed");
        assert_eq!(drives[0].kind, DriveKind::Nvme);
        assert_eq!(drives[0].size_bytes, 1_024_209_543_168);
        assert_eq!(drives[1].name, "nvme1n1");
        assert_eq!(drives[1].model, "Sabrent Rocket Q4");
        assert_eq!(drives[1].size_bytes, 2_000_398_934_016);
    }

    #[test]
    fn sata_ssd_and_hdd_are_told_apart_by_rotational() {
        let t = Tree::new("sata");
        t.disk("sda", "pci0000:00/0000:00:17.0/ata1/host0/target0:0:0/0:0:0:0", 976_773_168, false, Some("Samsung SSD  860 EVO 500GB"));
        t.disk("sdb", "pci0000:00/0000:00:17.0/ata2/host1/target1:0:0/1:0:0:0", 3_907_029_168, true, Some("WDC WD20SPZX-22UA7T0"));
        let drives = list_internal_drives(t.sys());
        assert_eq!((drives[0].kind, drives[0].model.as_str()), (DriveKind::Ssd, "Samsung SSD 860 EVO 500GB"));
        assert_eq!((drives[1].kind, drives[1].name.as_str()), (DriveKind::Hdd, "sdb"));
        assert_eq!(DriveKind::Hdd.label(), "HDD", "an HDD must never be labelled SSD");
    }

    #[test]
    fn emmc_is_listed_from_its_name_and_boot_partitions_and_sd_cards_are_not() {
        let t = Tree::new("mmc");
        t.disk("mmcblk0", "platform/soc/mmc_host/mmc0/mmc0:0001", 15_269_888, false, None);
        t.hw_attr("platform/soc/mmc_host/mmc0/mmc0:0001", "type", "MMC");
        t.hw_attr("platform/soc/mmc_host/mmc0/mmc0:0001", "name", "DG4064");
        t.disk("mmcblk0boot0", "platform/soc/mmc_host/mmc0/mmc0:0001", 8192, false, None);
        t.disk("mmcblk0rpmb", "platform/soc/mmc_host/mmc0/mmc0:0001", 128, false, None);
        t.disk("mmcblk1", "platform/soc/mmc_host/mmc1/mmc1:aaaa", 62_333_952, false, None);
        t.hw_attr("platform/soc/mmc_host/mmc1/mmc1:aaaa", "type", "SD");
        let drives = list_internal_drives(t.sys());
        assert_eq!(drives.len(), 1, "{drives:?}");
        assert_eq!((drives[0].name.as_str(), drives[0].kind, drives[0].model.as_str()), ("mmcblk0", DriveKind::Emmc, "DG4064"));
    }

    #[test]
    fn usb_and_removable_disks_are_hidden() {
        let t = Tree::new("usb");
        t.disk("sda", "pci0000:00/0000:00:17.0/ata1/host0/target0:0:0/0:0:0:0", 1_000_000, false, Some("Internal SSD"));
        t.disk("sdb", "pci0000:00/0000:00:14.0/usb1/1-2/1-2:1.0/host4/target4:0:0/4:0:0:0", 1_000_000, false, Some("Portable SSD"));
        t.disk("sdc", "pci0000:00/0000:00:17.0/ata3/host2/target2:0:0/2:0:0:0", 1_000_000, false, Some("Card Reader"));
        t.disk_attr("sdc", "removable", "1");
        let names: Vec<_> = list_internal_drives(t.sys()).into_iter().map(|d| d.name).collect();
        assert_eq!(names, vec!["sda"]);
    }

    #[test]
    fn virtual_optical_empty_and_multipath_devices_are_hidden() {
        let t = Tree::new("virt");
        t.disk("nvme0n1", NVME0_HW, 1_000_000, false, Some("Real NVMe"));
        t.disk("nvme0c0n1", NVME0_HW, 1_000_000, false, Some("Multipath head"));
        t.disk_attr("nvme0c0n1", "hidden", "1");
        t.disk("sr0", "pci0000:00/0000:00:17.0/ata4/host3/target3:0:0/3:0:0:0", 2_097_151, false, Some("DVD"));
        t.disk("loop0", "virtual/block/loop0", 1_000_000, false, None);
        t.disk("ram0", "virtual/block/ram0", 65_536, false, None);
        t.disk("nbd0", "virtual/block/nbd0", 1_000_000, false, None);
        t.disk("sdz", "pci0000:00/0000:00:17.0/ata5/host4/target4:0:0/4:0:0:0", 0, false, Some("Empty tray"));
        t.dm("dm-0", "cryptroot", "nvme0n1");
        let names: Vec<_> = list_internal_drives(t.sys()).into_iter().map(|d| d.name).collect();
        assert_eq!(names, vec!["nvme0n1"]);
    }

    #[test]
    fn namespaces_of_one_controller_are_separate_rows_in_natural_order() {
        let t = Tree::new("ns");
        for name in ["nvme10n1", "nvme2n1", "nvme0n2", "nvme0n1"] {
            t.disk(name, &format!("pci/{name}"), 1_000_000, false, Some("NVMe"));
        }
        t.disk("sdaa", "pci/sdaa", 1_000_000, false, Some("SATA"));
        t.disk("sdb", "pci/sdb", 1_000_000, false, Some("SATA"));
        let names: Vec<_> = list_internal_drives(t.sys()).into_iter().map(|d| d.name).collect();
        assert_eq!(names, vec!["nvme0n1", "nvme0n2", "nvme2n1", "nvme10n1", "sdb", "sdaa"]);
    }

    #[test]
    fn a_drive_without_a_model_does_not_panic() {
        let t = Tree::new("nomodel");
        t.disk("nvme0n1", NVME0_HW, 1_000_000, false, None);
        let d = list_internal_drives(t.sys());
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].model, "");
        assert_eq!(legacy_ssd_spec(&d), "NVMe  ·  1 GB NVMe", "falls back to the kind when the model is unknown");
    }

    #[test]
    fn missing_sysfs_yields_no_drives() {
        assert!(list_internal_drives(Path::new("/nonexistent/omen-sys")).is_empty());
    }

    // ── mount table ──

    #[test]
    fn mount_fields_are_octal_unescaped() {
        assert_eq!(unescape_mount_field(r"/mnt/My\040Games"), "/mnt/My Games");
        assert_eq!(unescape_mount_field(r"a\011b\134c"), "a\tb\\c");
        assert_eq!(unescape_mount_field(r"/plain"), "/plain");
        assert_eq!(unescape_mount_field(r"bad\9escape\04"), r"bad\9escape\04", "malformed escapes stay as they are");
    }

    #[test]
    fn mountinfo_keeps_only_block_device_filesystems() {
        let mounts = parse_mountinfo(THIS_MACHINE_MOUNTINFO);
        assert_eq!(mounts.len(), 9, "7 btrfs subvolumes + /boot + /mnt/games: {mounts:?}");
        assert!(mounts.iter().all(|m| m.source.starts_with("/dev/nvme")));
        let games = mounts.iter().find(|m| m.mountpoint == "/mnt/games").unwrap();
        assert_eq!((games.fstype.as_str(), games.dev.as_str()), ("ext4", "259:2"));
    }

    #[test]
    fn mountinfo_handles_spaces_in_mountpoint_and_source_and_optional_tags() {
        let text = "\
50 1 8:17 / /mnt/My\\040Games rw,relatime - ext4 /dev/disk/by-label/My\\040Games rw
51 1 8:33 / /mnt/plain rw,relatime shared:5 master:6 - xfs /dev/sdc1 rw
52 1 7:0 / /snap/x ro - squashfs /dev/loop0 ro
53 1 11:0 / /run/media/cd ro - iso9660 /dev/sr0 ro
54 1 0:60 / /mnt/nas rw - nfs4 server:/export rw
";
        let m = parse_mountinfo(text);
        assert_eq!(m.len(), 2, "{m:?}");
        assert_eq!(m[0].mountpoint, "/mnt/My Games");
        assert_eq!(m[0].source, "/dev/disk/by-label/My Games");
        assert_eq!(m[1].mountpoint, "/mnt/plain");
    }

    // ── mount source -> physical disk ──

    #[test]
    fn partitions_and_whole_disk_filesystems_resolve_to_their_disk() {
        let t = this_machine("resolve");
        t.disk("sda", "pci/sda", 1_000_000, false, Some("Whole disk fs"));
        let m = |src: &str, dev: &str| MountEntry { dev: dev.into(), mountpoint: "/x".into(), fstype: "ext4".into(), source: src.into() };
        assert_eq!(resolve_physical_disk(t.sys(), &m("/dev/nvme0n1p2", "0:33")).as_deref(), Some("nvme0n1"));
        assert_eq!(resolve_physical_disk(t.sys(), &m("/dev/nvme1n1p1", "259:2")).as_deref(), Some("nvme1n1"));
        assert_eq!(resolve_physical_disk(t.sys(), &m("/dev/sda", "8:0")).as_deref(), Some("sda"));
        assert_eq!(resolve_physical_disk(t.sys(), &m("/dev/zram0", "254:0")).as_deref(), Some("zram0"), "resolves, but is not an internal drive so it never gets a row");
        assert_eq!(resolve_physical_disk(t.sys(), &m("/dev/nope", "0:1")), None);
    }

    #[test]
    fn luks_and_lvm_stacks_resolve_down_to_the_physical_disk() {
        let t = this_machine("luks");
        t.dm("dm-0", "luks-1234", "nvme0n1p2");
        t.dm("dm-1", "vg0-root", "dm-0");
        let m = |src: &str| MountEntry { dev: "254:1".into(), mountpoint: "/".into(), fstype: "ext4".into(), source: src.into() };
        assert_eq!(resolve_physical_disk(t.sys(), &m("/dev/mapper/luks-1234")).as_deref(), Some("nvme0n1"));
        assert_eq!(resolve_physical_disk(t.sys(), &m("/dev/mapper/vg0-root")).as_deref(), Some("nvme0n1"));
        assert_eq!(resolve_physical_disk(t.sys(), &m("/dev/dm-1")).as_deref(), Some("nvme0n1"));
    }

    #[test]
    fn a_stacking_cycle_cannot_loop_forever() {
        let t = Tree::new("cycle");
        t.dm("dm-0", "a", "dm-1");
        t.dm("dm-1", "b", "dm-0");
        let m = MountEntry { dev: "254:0".into(), mountpoint: "/".into(), fstype: "ext4".into(), source: "/dev/dm-0".into() };
        assert_eq!(resolve_physical_disk(t.sys(), &m), None);
    }

    #[test]
    fn dev_root_falls_back_to_the_mount_major_minor() {
        let t = this_machine("devroot");
        t.devnum("259:4", "nvme0n1p2");
        let m = MountEntry { dev: "259:4".into(), mountpoint: "/".into(), fstype: "ext4".into(), source: "/dev/root".into() };
        assert_eq!(resolve_physical_disk(t.sys(), &m).as_deref(), Some("nvme0n1"));
    }

    // ── usage ──

    #[test]
    fn this_machine_usage_counts_btrfs_subvolumes_once_and_matches_df() {
        let t = this_machine("usage");
        let drives = list_internal_drives(t.sys());
        let mounts = parse_mountinfo(THIS_MACHINE_MOUNTINFO);
        // `df -B1` on this machine
        let fs_table = table(&[
            ("/", 46_900_000_000, 1_019_900_000_000),
            ("/boot", 628_000_000, 4_298_000_000),
            ("/mnt/games", 218_836_664_320, 1_967_845_998_592),
        ]);
        let calls = Cell::new(0);
        let stat = |mp: &str| {
            calls.set(calls.get() + 1);
            fs_table.get(mp).copied()
        };
        let usage = drive_usage(&drives, &mounts, t.sys(), &stat);

        assert_eq!(usage.len(), 2);
        assert_eq!(usage[0].name, "nvme0n1");
        assert_eq!(usage[0].used_bytes, 46_900_000_000 + 628_000_000, "root + /boot, subvolumes not repeated");
        assert_eq!(usage[0].total_bytes, 1_019_900_000_000 + 4_298_000_000);
        assert_eq!(usage[1].name, "nvme1n1");
        assert_eq!((usage[1].used_bytes, usage[1].total_bytes), (218_836_664_320, 1_967_845_998_592));
        assert_eq!(calls.get(), 3, "one statvfs per source, not per mount");
        assert_eq!(usage[0].mounts, vec!["/", "/boot"], "one mountpoint per filesystem, not one per subvolume");
        assert_eq!(usage[1].mounts, vec!["/mnt/games"]);
    }

    #[test]
    fn bind_mounts_of_the_service_sandbox_do_not_show_up_as_mounts() {
        let t = this_machine("sandbox");
        // What the daemon itself sees under `ReadWritePaths=` / `PrivateTmp=`
        let mounts = parse_mountinfo("\
404 28 0:33 /@ / ro - btrfs /dev/nvme0n1p2 rw,subvol=/@
526 404 0:33 /@tmp /var/tmp rw - btrfs /dev/nvme0n1p2 rw,subvol=/@tmp
529 404 0:33 /@/etc/omen-space /etc/omen-space rw - btrfs /dev/nvme0n1p2 rw,subvol=/@
518 404 0:33 /@/var/lib/omen-space-daemon /var/lib/omen-space-daemon rw - btrfs /dev/nvme0n1p2 rw,subvol=/@
527 404 259:3 / /boot ro - vfat /dev/nvme0n1p1 rw
");
        let usage = drive_usage(&list_internal_drives(t.sys()), &mounts, t.sys(), &|_| Some(FsUsage { used: 1, total: 2 }));
        assert_eq!(usage[0].mounts, vec!["/", "/boot"]);
        assert!(mount_rank("/") < mount_rank("/boot") && mount_rank("/boot") < mount_rank("/var/log"));
    }

    #[test]
    fn statvfs_falls_through_to_the_next_mountpoint_of_the_same_device() {
        let t = Tree::new("fallthrough");
        t.disk("sda", "pci/sda", 1_000_000, false, Some("SATA"));
        t.partition("sda", "sda1", 1);
        let mounts = parse_mountinfo("\
10 1 8:1 / /mnt/hidden rw - ext4 /dev/sda1 rw
11 1 8:1 / /mnt/visible rw - ext4 /dev/sda1 rw
");
        let fs_table = table(&[("/mnt/visible", 5, 10)]); // /mnt/hidden fails, as under ProtectHome
        let usage = drive_usage(&list_internal_drives(t.sys()), &mounts, t.sys(), &|mp| fs_table.get(mp).copied());
        assert_eq!((usage[0].used_bytes, usage[0].total_bytes), (5, 10));
    }

    #[test]
    fn luks_root_usage_lands_on_the_underlying_drive() {
        let t = this_machine("luksusage");
        t.dm("dm-0", "cryptroot", "nvme0n1p2");
        let mounts = parse_mountinfo("40 1 254:0 / / rw - ext4 /dev/mapper/cryptroot rw\n");
        let fs_table = table(&[("/", 100, 400)]);
        let usage = drive_usage(&list_internal_drives(t.sys()), &mounts, t.sys(), &|mp| fs_table.get(mp).copied());
        assert_eq!((usage[0].used_bytes, usage[0].total_bytes), (100, 400));
        assert_eq!(usage[1].total_bytes, 0);
    }

    #[test]
    fn a_drive_with_nothing_mounted_reports_zero_and_external_disks_are_ignored() {
        let t = this_machine("unmounted");
        t.disk("sdb", "pci0000:00/0000:00:14.0/usb1/1-1/host5/target5:0:0/5:0:0:0", 1_000_000, false, Some("Stick"));
        t.partition("sdb", "sdb1", 1);
        let mounts = parse_mountinfo("60 1 8:17 / /run/media/stick rw - vfat /dev/sdb1 rw\n");
        let usage = drive_usage(&list_internal_drives(t.sys()), &mounts, t.sys(), &|_| Some(FsUsage { used: 1, total: 2 }));
        assert_eq!(usage.len(), 2, "one entry per internal drive, none for the USB stick");
        assert!(usage.iter().all(|u| u.total_bytes == 0 && u.mounts.is_empty()));
    }

    // ── legacy fields ──

    #[test]
    fn legacy_ssd_spec_keeps_the_exact_old_text_for_the_first_drive() {
        let t = this_machine("legacy");
        let drives = list_internal_drives(t.sys());
        assert_eq!(legacy_ssd_spec(&drives), "SAMSUNG MZVL81T0HFLB-00BH1  ·  1024 GB NVMe");
        assert_eq!(legacy_ssd_spec(&[]), "");
        let hdd = DriveInfo { name: "sdb".into(), model: "WDC WD20SPZX".into(), kind: DriveKind::Hdd, size_bytes: 2_000_398_934_016 };
        assert_eq!(legacy_ssd_spec(&[hdd]), "WDC WD20SPZX  ·  2000 GB HDD");
    }

    #[test]
    fn legacy_disk_fields_describe_the_first_drive_in_decimal_gb() {
        let usage = vec![
            DriveUsage { name: "a".into(), used_bytes: 48_000_000_000, total_bytes: 1_024_000_000_000, mounts: vec![] },
            DriveUsage { name: "b".into(), used_bytes: 9, total_bytes: 10, mounts: vec![] },
        ];
        let (used, total, frac) = legacy_disk_fields(&usage);
        assert!((used - 48.0).abs() < 1e-9 && (total - 1024.0).abs() < 1e-9);
        assert!((frac - 48.0 / 1024.0).abs() < 1e-9);
        assert_eq!(legacy_disk_fields(&[]), (0.0, 0.0, 0.0));
        let unmounted = [DriveUsage::default()];
        assert_eq!(legacy_disk_fields(&unmounted), (0.0, 0.0, 0.0), "no NaN from a zero total");
    }

    // ── wire compatibility (old daemon <-> new GUI, and the reverse) ──

    const OLD_SPECS: &str = r#"{"product_name":"OMEN Gaming Laptop 16-ap0xxx","cpu_spec":"AMD Ryzen 9 8940HX","gpu_spec":"RTX 5060","ram_spec":"30 GB RAM","ssd_spec":"SAMSUNG MZVL81T0HFLB-00BH1  ·  1024 GB NVMe","os_spec":"CachyOS","bios_version":"F.13","ec_version":"99.24","vbios_version":"98.06","nvidia_driver":"615.71.09","kernel_version":"7.2.9"}"#;
    const OLD_STATS: &str = r#"{"cpu_temp":55,"cpu_load":0.1,"cpu_pwr":12.0,"fan_rpm":2400,"fan1_rpm":2400,"fan2_rpm":2400,"gpu_temp":44,"gpu_load":0.0,"gpu_pwr":10.0,"ram_used_gb":9.0,"ram_total_gb":30.0,"ram_frac":0.3,"disk_used_gb":248.0,"disk_total_gb":2787.0,"disk_frac":0.09,"total_pwr":40.0,"cpu_throttle_count":0}"#;

    #[test]
    fn json_from_an_old_daemon_still_parses_with_no_drives() {
        let specs: HardwareSpecs = serde_json::from_str(OLD_SPECS).expect("old specs must parse");
        assert_eq!(specs.product_name, "OMEN Gaming Laptop 16-ap0xxx");
        assert!(specs.drives.is_empty(), "a missing list must default to empty, not fail the parse");
        assert!(specs.ssd_spec.contains("SAMSUNG"));
        let stats: SystemStats = serde_json::from_str(OLD_STATS).expect("old stats must parse");
        assert_eq!(stats.cpu_temp, 55);
        assert!(stats.drives.is_empty());
        assert!((stats.disk_total_gb - 2787.0).abs() < f64::EPSILON);
    }

    #[test]
    fn new_json_round_trips_and_still_carries_the_legacy_fields() {
        let t = this_machine("json");
        let mut specs = HardwareSpecs::default();
        specs.drives = list_internal_drives(t.sys());
        specs.ssd_spec = legacy_ssd_spec(&specs.drives);
        let json = serde_json::to_string(&specs).unwrap();
        assert!(json.contains(r#""ssd_spec":"SAMSUNG MZVL81T0HFLB-00BH1  ·  1024 GB NVMe""#), "old GUIs still read this: {json}");
        assert!(json.contains(r#""kind":"nvme""#));
        assert_eq!(serde_json::from_str::<HardwareSpecs>(&json).unwrap().drives, specs.drives);

        let mut stats = SystemStats::default();
        stats.drives = vec![DriveUsage { name: "nvme0n1".into(), used_bytes: 48, total_bytes: 1024, mounts: vec!["/".into()] }];
        stats.disk_total_gb = 1.0;
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("\"disk_total_gb\""), "old GUIs still read the scalar fields");
        assert_eq!(serde_json::from_str::<SystemStats>(&json).unwrap().drives, stats.drives);
    }

    #[test]
    fn an_old_client_ignores_the_new_fields_and_a_new_client_survives_unknown_kinds() {
        #[derive(Deserialize)]
        struct OldSpecs {
            ssd_spec: String,
            product_name: String,
        }
        let mut specs = HardwareSpecs { ssd_spec: "x".into(), product_name: "p".into(), ..Default::default() };
        specs.drives.push(DriveInfo::default());
        let old: OldSpecs = serde_json::from_str(&serde_json::to_string(&specs).unwrap()).expect("unknown `drives` must be ignored");
        assert_eq!((old.ssd_spec.as_str(), old.product_name.as_str()), ("x", "p"));

        let future: DriveInfo = serde_json::from_str(r#"{"name":"x","kind":"optane","extra":1}"#).expect("unknown kind and fields must not fail");
        assert_eq!(future.kind, DriveKind::Other);
    }

    /// Prints what this very machine reports. Run with:
    /// `cargo test -p omen-space-daemon host_smoke -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn host_smoke() {
        let drives = current_drives();
        let usage = current_usage(&drives);
        println!("drives: {}", serde_json::to_string_pretty(&drives).unwrap());
        println!("usage:  {}", serde_json::to_string_pretty(&usage).unwrap());
        println!("legacy: {:?} {:?}", legacy_ssd_spec(&drives), legacy_disk_fields(&usage));
        assert_eq!(drives.len(), usage.len());
    }
}
