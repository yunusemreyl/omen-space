#![allow(dead_code)]
#![allow(unused_imports)]
/// RGB LED service - matches Python rgb_service.py feature-for-feature.
///
/// D-Bus interface: com.yyl.hpmanager.rgb (backward compat) +
///                  org.hp.omen.Rgb (new canonical name)
///
/// Methods exposed:
///   SetColor(z: i, h: s) -> resp: s
///   SetMode(m: s, s: i)  -> resp: s
///   SetGlobal(p: b, b: i, d: s) -> resp: s
///   GetState()           -> j: s
///   SetWinLock(locked: b) -> result: s
///   TestSingleKey(index: i) -> resp: s
///   SavePerKeyMap(map_json: s) -> resp: s
///   Ping()               -> resp: s
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use zbus::interface;
use log::{info, warn, error};
use std::path::Path;
use glob::glob;
use tokio::io::AsyncReadExt;
use std::fs::File;

// Valid modes matching Python VALID_LIGHT_MODES
const VALID_MODES: &[&str] = &[
    "static", "per_key_custom", "breathing", "wave", "cycle", "rainbow",
    "pulse", "chase", "sparkle", "candle", "aurora", "disco", "gradient",
    "wave_rainbow", "starlight", "marquee", "reactive", "ripple", "raindrop", "blinking",
];
const VALID_DIRECTIONS: &[&str] = &["ltr", "rtl"];

// Config persistence
const CONFIG_PATH: &str = "/etc/omen-space/rgb.json";
const PER_KEY_MAP_PATH: &str = "/root/.config/omen-space/per_key_map.json";

// ── Hardware detection ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct RgbHardware {
    driver_path: Option<String>,
    is_new_driver: bool,
    zone_count: u32,
    available: bool,
    board_id: String,
}

pub(crate) fn lightbar_allowed_for(
    board_id: &str,
    caps: &crate::capabilities::ModelCapabilities,
    product_name: &str,
) -> bool {
    let known_board = caps.product_id.eq_ignore_ascii_case(board_id.trim());
    if !known_board || caps.has_light_bar {
        return true;
    }
    let prod = product_name.to_lowercase();
    prod.contains("desktop") || prod.contains("transcend") || prod.contains("max")
}

pub(crate) fn map_four_zone_index(board_id: &str, zone: usize) -> usize {
    if matches!(board_id.trim().to_uppercase().as_str(), "8EEF" | "8EDD" | "8EEC") {
        // OMEN HyperX 15/16 (boards 8EEF / 8EDD / 8EEC) wire their 4-zone WMI buffer in the
        // reverse order of classic OMEN boards: hw zone0=WASD, zone1=Left,
        // zone2=Middle, zone3=Right (#309, #335).
        match zone {
            0 => 1, // Software Left   -> Hardware Left (1)
            1 => 2, // Software Middle -> Hardware Middle (2)
            2 => 3, // Software Right  -> Hardware Right (3)
            3 => 0, // Software WASD   -> Hardware WASD (0)
            z => z,
        }
    } else {
        match zone {
            0 => 2, // Software Left   -> Hardware Left (2)
            1 => 1, // Software Middle -> Hardware Middle (1)
            2 => 0, // Software Right  -> Hardware Right (0)
            3 => 3, // Software WASD   -> Hardware WASD (3)
            z => z,
        }
    }
}

impl RgbHardware {
    fn detect() -> Self {
        Self::detect_with_override(None)
    }

    /// `zone_override`: Some(4) or Some(8) forces the zone count on the
    /// legacy hp-omen-extra/hp_omen_extra driver, whose sysfs interface
    /// always exposes zone0..zone7 regardless of whether the board is
    /// physically 4-zone or 8-zone (same kernel module for both — see
    /// driver/hp-omen-extra.c). None uses the built-in default (4).
    fn detect_with_override(zone_override: Option<u32>) -> Self {
        let mut hw = Self::detect_raw();
        if !hw.is_new_driver {
            hw.zone_count = match zone_override {
                Some(n) if n == 1 || n == 4 || n == 8 => n,
                // Single-zone boards from the capability DB: every extra zone
                // write is a costly no-op WMI call (~7-10 ms each on 8BD4).
                _ if hw.zone_count == 1 => 1,
                _ => 4, // Honor the built-in default of 4
            };
        }
        hw
    }

    fn detect_raw() -> Self {
        let new_path = "/sys/devices/platform/omen-rgb-keyboard/rgb_zones";
        let custom_path = "/sys/devices/platform/hp-omen-extra";
        let hp_path2 = "/sys/devices/platform/hp_omen_extra";
        let board_id = std::fs::read_to_string("/sys/class/dmi/id/board_name").unwrap_or_default().trim().to_uppercase();
        let prod_name = std::fs::read_to_string("/sys/class/dmi/id/product_name").unwrap_or_default();
        let specs = crate::sysmon::get_hardware_specs();
        let caps = crate::capabilities::detect(&board_id, prod_name.trim(), &specs.cpu_spec);
        let max_zones = if caps.has_four_zone_rgb { 4 } else { 8 };

        if Path::new(new_path).exists() {
            // Count zones for new driver (8 for per-key models, else 4)
            let mut zone_count = if Path::new("/sys/devices/platform/omen-rgb-keyboard/rgb_zones/zone04").exists() { 8 } else { 4 };
            if zone_count > max_zones { zone_count = max_zones; }
            return Self { driver_path: Some(new_path.to_string()), is_new_driver: true, zone_count, available: true, board_id };
        }
        for p in [custom_path, hp_path2] {
            if Path::new(p).exists() {
                // hp-omen-extra always registers zone0..zone7 sysfs files
                // unconditionally (see driver/hp-omen-extra.c) regardless of
                // whether the board is physically 4-zone or 8-zone, since
                // it's the same kernel module for both — file existence
                // alone can't distinguish them. Prefer the real per-model
                // capability DB (caps.has_four_zone_rgb, sourced from field
                // reports) over the file-existence heuristic; fall back to
                // the heuristic when the board isn't in the DB yet.
                let mut zone_count = if caps.rgb_zone_count == Some(1) {
                    1
                } else if caps.has_four_zone_rgb {
                    4
                } else if Path::new(&format!("{}/zone4", p)).exists()
                    || Path::new(&format!("{}/zone04", p)).exists() { 8 } else { 4 };
                if zone_count > max_zones { zone_count = max_zones; }
                return Self { driver_path: Some(p.to_string()), is_new_driver: false, zone_count, available: true, board_id };
            }
        }
        // Try keyboard brightness LEDs as fallback
        if let Ok(mut entries) = glob("/sys/class/leds/hp::kbd_backlight*") {
            if let Some(Ok(path)) = entries.next() {
                return Self { driver_path: Some(path.to_string_lossy().to_string()), is_new_driver: false, zone_count: 4, available: true, board_id };
            }
        }
        Self { driver_path: None, is_new_driver: false, zone_count: 4, available: false, board_id }
    }

    fn write_zone(&self, zone: usize, hex_color: &str) {
        let Some(ref base) = self.driver_path else { return; };
        if zone > 7 { return; }

        let actual_zone = if self.zone_count == 4 {
            map_four_zone_index(&self.board_id, zone)
        } else {
            zone
        };
        let filename = if self.is_new_driver {
            format!("zone{:02}", actual_zone)
        } else {
            format!("zone{}", actual_zone)
        };
        let path = format!("{}/{}", base, filename);
        let _ = std::fs::write(&path, hex_color);
    }

    fn write_all(&self, hex_color: &str) {
        let Some(ref base) = self.driver_path else { return; };
        if self.is_new_driver {
            let all_path = format!("{}/all", base);
            if Path::new(&all_path).exists() {
                let _ = std::fs::write(&all_path, hex_color);
                return;
            }
        }
        for i in 0..self.zone_count as usize {
            self.write_zone(i, hex_color);
        }
    }

    fn write_brightness(&self, value: u32) {
        let Some(ref base) = self.driver_path else { return; };
        let path = format!("{}/brightness", base);
        if !Path::new(&path).exists() { return; }
        let val_str = if self.is_new_driver { value.to_string() } else { if value > 0 { "1".to_string() } else { "0".to_string() } };
        let _ = std::fs::write(&path, val_str);
    }

    fn write_win_lock(&self, locked: bool) {
        let Some(ref base) = self.driver_path else { return; };
        let path = format!("{}/win_lock", base);
        if Path::new(&path).exists() {
            let _ = std::fs::write(&path, if locked { "1" } else { "0" });
        }
    }

    fn write_zones_batch(&self, colors: &[String]) {
        let Some(ref base) = self.driver_path else { return; };
        let all_zones_path = format!("{}/all_zones", base);
        if Path::new(&all_zones_path).exists() {
            let count = self.zone_count.clamp(1, 8) as usize;
            let mut mapped_colors = vec!["000000".to_string(); count];
            for i in 0..count {
                let actual = if self.zone_count == 4 {
                    map_four_zone_index(&self.board_id, i)
                } else {
                    i
                };
                if actual < count {
                    mapped_colors[actual] = colors.get(i).cloned().unwrap_or_else(|| "000000".to_string());
                }
            }
            let payload = mapped_colors.join(" ");
            if std::fs::write(&all_zones_path, payload).is_ok() {
                return;
            }
            warn!("RGB batch write failed; falling back to individual zones");
        }
        for (i, c) in colors.iter().enumerate().take(self.zone_count as usize) {
            self.write_zone(i, c);
        }
    }

    fn write_lightbar(&self, colors: &[String]) {
        let Some(ref base) = self.driver_path else { return; };
        let lightbar_path = format!("{}/lightbar", base);
        if Path::new(&lightbar_path).exists() {
            // Purple-white workaround for OMEN firmware (#FFFFFF -> #FFFFFE)
            let payload = colors.iter().take(4).map(|c| {
                if c.eq_ignore_ascii_case("FFFFFF") {
                    "FFFFFE".to_string()
                } else {
                    c.clone()
                }
            }).collect::<Vec<_>>().join(" ");
            let _ = std::fs::write(&lightbar_path, payload);
            return;
        }
        for (i, c) in colors.iter().take(4).enumerate() {
            let led_path = format!("/sys/class/leds/hp::lightbar-{}/multi_intensity", i);
            if Path::new(&led_path).exists() {
                let (r, g, b) = parse_hex_color(c);
                let _ = std::fs::write(&led_path, format!("{} {} {}", r, g, b));
            }
        }
    }

    /// On per-key OMEN MAX boards (8D88 / 8D42) the WMI "zones" belong to the
    /// 4-segment front lightbar while the keyboard is driven over USB HID LampArray (#355).
    fn write_lightbar_zones(&self, colors: &[String]) {
        let Some(ref base) = self.driver_path else { return; };
        let format_color = |c: &String| {
            if c.eq_ignore_ascii_case("FFFFFF") {
                "FFFFFE".to_string()
            } else {
                c.clone()
            }
        };
        let payload = colors.iter().take(4).map(format_color).collect::<Vec<_>>().join(" ");
        for name in ["all_zones", "zones"] {
            let p = format!("{}/{}", base, name);
            if Path::new(&p).exists() {
                let _ = std::fs::write(&p, &payload);
                return;
            }
        }
        for (i, c) in colors.iter().take(4).enumerate() {
            for name in [format!("zone{:02}", i), format!("zone{}", i)] {
                let p = format!("{}/{}", base, name);
                if Path::new(&p).exists() {
                    let _ = std::fs::write(&p, c);
                    break;
                }
            }
        }
    }

    /// False only when the capability database has an exact entry for this board saying it
    /// has no light bar and the product name is not one of the light-bar families. Boards
    /// missing from the database keep the previous behaviour.
    fn lightbar_write_allowed(&self) -> bool {
        let prod_name = std::fs::read_to_string("/sys/class/dmi/id/product_name").unwrap_or_default();
        let caps = crate::capabilities::detect(&self.board_id, "", "");
        lightbar_allowed_for(&self.board_id, &caps, &prod_name)
    }

    fn has_lightbar(&self) -> bool {
        if let Some(ref base) = self.driver_path {
            if Path::new(&format!("{}/lightbar", base)).exists() {
                return true;
            }
        }
        Path::new("/sys/class/leds/hp::lightbar-0/multi_intensity").exists()
    }

    fn write_mode(&self, mode: &str, speed: u32) {
        if !self.is_new_driver { return; }
        let Some(ref base) = self.driver_path else { return; };
        let hw_mode = if mode == "cycle" { "rainbow" } else { mode };
        let _ = std::fs::write(format!("{}/animation_mode", base), hw_mode);
        let mapped_speed = (speed / 10).clamp(1, 10);
        let _ = std::fs::write(format!("{}/animation_speed", base), mapped_speed.to_string());
    }
}

// ── HID LampArray Per-Key Backend (Darfon 0d62:54bf / Usage Page 0x59, #355) ──

const LAMPARRAY_PREFIX: &[u8] = &[0x05, 0x59, 0x09, 0x01];
const DARFON_VENDOR_PREFIX: &[u8] = &[0x06, 0x01, 0xFF, 0x09, 0x01];

/// Per-lamp GUI slot overrides for board 8D88 (OMEN MAX 16-ak0xxx, 0d62:54bf).
const LAMP_SLOT_OVERRIDES_8D88: &[(usize, usize)] = &[
    (1, 0),    // Esc (2nd LED)
    (14, 13),  // Del
    (21, 17),  // Grave
    (42, 35),  // Tab
    (62, 53),  // Caps Lock
    (79, 52),  // Keypad +
    (119, 85), // Keypad Enter
    (16, 14),  // Omen
    (17, 15),  // Calculator
    (18, 98),  // Settings
    (19, 16),  // Power
    (101, 87), // Fn
    (103, 88), // Win
    (111, 92), // Menu / Copilot
];

fn is_board_8d88() -> bool {
    static CACHE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CACHE.get_or_init(|| {
        std::fs::read_to_string("/sys/class/dmi/id/board_name")
            .map(|s| s.trim().eq_ignore_ascii_case("8D88"))
            .unwrap_or(false)
    })
}

#[derive(Debug, Clone, Copy)]
struct LampInfo {
    id: u16,
    x: u32,
    y: u32,
    binding: u8,
}

#[derive(Debug, Clone)]
struct LampArrayBackend {
    path: String,
    lamps: Vec<LampInfo>,
    width_um: u32,
    height_um: u32,
    min_interval_us: u32,
    acquired: std::sync::Arc<std::sync::atomic::AtomicBool>,
    fd: std::sync::Arc<std::sync::Mutex<Option<std::fs::File>>>,
}

impl LampArrayBackend {
    const R_ATTRS: u8 = 1;
    const R_LAMP_REQ: u8 = 2;
    const R_LAMP_RESP: u8 = 3;
    const R_MULTI: u8 = 4;
    const R_RANGE: u8 = 5;
    const R_CONTROL: u8 = 6;

    const SZ_ATTRS: usize = 23;
    const SZ_LAMP_RESP: usize = 29;
    const LAMPS_PER_MULTI: usize = 8;
    const FLAG_COMPLETE: u8 = 1;

    const KEY_USAGES: [u8; 104] = [
        // row 1: Esc F1-F12 Del Omen Calc Power
        0x29, 0x3A, 0x3B, 0x3C, 0x3D, 0x3E, 0x3F, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x4C, 0x00, 0x00, 0x66,
        // row 2
        0x35, 0x1E, 0x1F, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x2D, 0x2E, 0x2A, 0x53, 0x54, 0x55, 0x56,
        // row 3
        0x2B, 0x14, 0x1A, 0x08, 0x15, 0x17, 0x1C, 0x18, 0x0C, 0x12, 0x13, 0x2F, 0x30, 0x31, 0x5F, 0x60, 0x61, 0x57,
        // row 4
        0x39, 0x04, 0x16, 0x07, 0x09, 0x0A, 0x0B, 0x0D, 0x0E, 0x0F, 0x33, 0x34, 0x28, 0x5C, 0x5D, 0x5E,
        // row 5
        0xE1, 0x1D, 0x1B, 0x06, 0x19, 0x05, 0x11, 0x10, 0x36, 0x37, 0x38, 0xE5, 0x52, 0x59, 0x5A, 0x5B, 0x58,
        // row 6
        0xE0, 0x00, 0xE3, 0xE2, 0x2C, 0xE6, 0x65, 0x50, 0x51, 0x4F, 0x62, 0x63,
        // placeholders
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    fn hidioc(nr: u8, size: usize) -> libc::c_ulong {
        ((3u64 << 30) | ((size as u64) << 16) | ((b'H' as u64) << 8) | (nr as u64))
            as libc::c_ulong
    }

    fn get_feature(file: &std::fs::File, report_id: u8, size: usize) -> std::io::Result<Vec<u8>> {
        use std::os::fd::AsRawFd;
        if size == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "empty report"));
        }
        let mut buf = vec![0u8; size];
        buf[0] = report_id;
        // SAFETY: HIDIOCGFEATURE ioctl writes at most `size` bytes into `buf`.
        let rc = unsafe {
            libc::ioctl(file.as_raw_fd(), Self::hidioc(0x07, size), buf.as_mut_ptr())
        };
        if rc < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(buf)
        }
    }

    fn set_feature(file: &std::fs::File, data: &[u8]) -> std::io::Result<()> {
        use std::os::fd::AsRawFd;
        if data.is_empty() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "empty report"));
        }
        let mut buf = data.to_vec();
        // SAFETY: HIDIOCSFEATURE ioctl reads `buf.len()` bytes from `buf`.
        let rc = unsafe {
            libc::ioctl(file.as_raw_fd(), Self::hidioc(0x06, buf.len()), buf.as_mut_ptr())
        };
        if rc < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn open(&self) -> std::io::Result<std::fs::File> {
        std::fs::OpenOptions::new().read(true).write(true).open(&self.path)
    }

    fn with_file<T>(&self, f: impl FnOnce(&std::fs::File) -> T) -> Option<T> {
        let mut guard = self.fd.lock().ok()?;
        if guard.is_none() {
            match self.open() {
                Ok(file) => *guard = Some(file),
                Err(e) => {
                    warn!("LampArray {}: cannot open: {}", self.path, e);
                    return None;
                }
            }
        }
        guard.as_ref().map(f)
    }

    fn probe(path: &str) -> Option<Self> {
        let file = std::fs::OpenOptions::new().read(true).write(true).open(path).ok()?;
        let attrs = Self::get_feature(&file, Self::R_ATTRS, Self::SZ_ATTRS).ok()?;
        if attrs.len() < Self::SZ_ATTRS || attrs[0] != Self::R_ATTRS {
            return None;
        }
        let count = u16::from_le_bytes([attrs[1], attrs[2]]) as usize;
        if count == 0 || count > 1024 {
            return None;
        }
        let width_um = u32::from_le_bytes([attrs[3], attrs[4], attrs[5], attrs[6]]);
        let height_um = u32::from_le_bytes([attrs[7], attrs[8], attrs[9], attrs[10]]);
        let min_interval_us = u32::from_le_bytes([attrs[19], attrs[20], attrs[21], attrs[22]]);

        let mut lamps = Vec::with_capacity(count);
        for id in 0..count as u16 {
            let req = [Self::R_LAMP_REQ, (id & 0xFF) as u8, (id >> 8) as u8];
            Self::set_feature(&file, &req).ok()?;
            let resp = Self::get_feature(&file, Self::R_LAMP_RESP, Self::SZ_LAMP_RESP).ok()?;
            if resp.len() < Self::SZ_LAMP_RESP {
                continue;
            }
            let lid = u16::from_le_bytes([resp[1], resp[2]]);
            let x = u32::from_le_bytes([resp[3], resp[4], resp[5], resp[6]]);
            let y = u32::from_le_bytes([resp[7], resp[8], resp[9], resp[10]]);
            let binding = resp[28];
            lamps.push(LampInfo { id: lid, x, y, binding });
        }
        if lamps.is_empty() {
            return None;
        }
        lamps.sort_by_key(|l| l.id);
        info!(
            "LampArray on {}: {} lamps, {}x{} mm, min_interval={}us",
            path,
            lamps.len(),
            width_um / 1000,
            height_um / 1000,
            min_interval_us
        );
        Some(Self {
            path: path.to_string(),
            lamps,
            width_um,
            height_um,
            min_interval_us,
            acquired: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            fd: std::sync::Arc::new(std::sync::Mutex::new(None)),
        })
    }

    fn acquire(&self, file: &std::fs::File) {
        let _ = Self::set_feature(file, &[Self::R_CONTROL, 0]);
    }

    fn release(&self) {
        use std::sync::atomic::Ordering;
        let _ = self.with_file(|file| {
            let _ = Self::set_feature(file, &[Self::R_CONTROL, 1]);
        });
        self.acquired.store(false, Ordering::Relaxed);
    }

    fn range_report(start: u16, end: u16, color: (u8, u8, u8)) -> Vec<u8> {
        vec![
            Self::R_RANGE,
            Self::FLAG_COMPLETE,
            (start & 0xFF) as u8,
            (start >> 8) as u8,
            (end & 0xFF) as u8,
            (end >> 8) as u8,
            color.0,
            color.1,
            color.2,
            255,
        ]
    }

    fn multi_report(lamps: &[LampInfo], offset: usize, chunk: &[(u8, u8, u8)], complete: bool) -> Vec<u8> {
        let mut rep = vec![0u8; 51];
        rep[0] = Self::R_MULTI;
        rep[1] = chunk.len() as u8;
        rep[2] = if complete { Self::FLAG_COMPLETE } else { 0 };
        for k in 0..Self::LAMPS_PER_MULTI {
            let id = lamps.get(offset + k).map(|l| l.id).unwrap_or(0);
            let p = 3 + k * 2;
            rep[p] = (id & 0xFF) as u8;
            rep[p + 1] = (id >> 8) as u8;
        }
        for (k, &(r, g, b)) in chunk.iter().enumerate() {
            let p = 19 + k * 4;
            rep[p] = r;
            rep[p + 1] = g;
            rep[p + 2] = b;
            rep[p + 3] = 255;
        }
        rep
    }

    fn frame_reports(&self, colors: &[(u8, u8, u8)]) -> Vec<Vec<u8>> {
        let n = self.lamps.len();
        if n == 0 {
            return Vec::new();
        }
        let mut cols = colors.to_vec();
        cols.resize(n, (0, 0, 0));
        cols.truncate(n);
        if cols.iter().all(|c| *c == cols[0]) {
            return vec![Self::range_report(0, (n - 1) as u16, cols[0])];
        }
        let mut reports = Vec::with_capacity(n.div_ceil(Self::LAMPS_PER_MULTI));
        let mut off = 0;
        while off < n {
            let end = (off + Self::LAMPS_PER_MULTI).min(n);
            reports.push(Self::multi_report(&self.lamps, off, &cols[off..end], end >= n));
            off = end;
        }
        reports
    }

    fn set_frame(&self, colors: &[(u8, u8, u8)]) -> bool {
        use std::sync::atomic::Ordering;
        let reports = self.frame_reports(colors);
        if reports.is_empty() {
            return false;
        }
        let result = self.with_file(|file| {
            if !self.acquired.load(Ordering::Relaxed) {
                self.acquire(file);
                self.acquired.store(true, Ordering::Relaxed);
            }
            for rep in &reports {
                if let Err(e) = Self::set_feature(file, rep) {
                    warn!("LampArray {}: report 0x{:02X} failed: {}", self.path, rep[0], e);
                    self.acquired.store(false, Ordering::Relaxed);
                    return false;
                }
            }
            true
        });
        if result != Some(true) {
            if let Ok(mut guard) = self.fd.lock() {
                *guard = None;
            }
        }
        result.unwrap_or(false)
    }

    fn zone_colors(&self, colors: &[(u8, u8, u8)]) -> Vec<(u8, u8, u8)> {
        let width = self.width_um.max(1) as u64;
        self.lamps
            .iter()
            .map(|l| {
                let zone = ((l.x as u64 * 4) / width).min(3) as usize;
                colors.get(zone).copied().unwrap_or((0, 0, 0))
            })
            .collect()
    }

    fn key_colors(&self, key_colors: &[(u8, u8, u8)]) -> Vec<(u8, u8, u8)> {
        let n = self.lamps.len();
        if key_colors.is_empty() {
            return vec![(0, 0, 0); n];
        }
        if key_colors.len() == n {
            return key_colors.to_vec();
        }
        if key_colors.len() == HidPerKeyBackend::TOTAL_KEY_COUNT {
            return self.map_gui_frame(key_colors);
        }
        (0..n)
            .map(|i| {
                let k = (i * key_colors.len()) / n;
                key_colors[k.min(key_colors.len() - 1)]
            })
            .collect()
    }

    fn map_gui_frame(&self, key_colors: &[(u8, u8, u8)]) -> Vec<(u8, u8, u8)> {
        let n = self.lamps.len();
        let mut out = vec![(0u8, 0u8, 0u8); n];
        let mut mapped = vec![false; n];

        if is_board_8d88() && n == 120 {
            for &(lamp, slot) in LAMP_SLOT_OVERRIDES_8D88 {
                if lamp < n && slot < key_colors.len() {
                    out[lamp] = key_colors[slot];
                    mapped[lamp] = true;
                }
            }
        }

        for (i, lamp) in self.lamps.iter().enumerate() {
            if mapped[i] || lamp.binding == 0 {
                continue;
            }
            if let Some(idx) = Self::KEY_USAGES
                .iter()
                .position(|&usage| usage != 0 && usage == lamp.binding)
            {
                if let Some(color) = key_colors.get(idx) {
                    out[i] = *color;
                    mapped[i] = true;
                }
            }
        }

        for i in 0..n {
            if mapped[i] {
                continue;
            }
            let mut best: Option<usize> = None;
            let mut best_d = u64::MAX;
            for (j, other) in self.lamps.iter().enumerate() {
                if !mapped[j] {
                    continue;
                }
                let dx = self.lamps[i].x as i64 - other.x as i64;
                let dy = self.lamps[i].y as i64 - other.y as i64;
                let d = (dx * dx + dy * dy) as u64;
                if d < best_d {
                    best_d = d;
                    best = Some(j);
                }
            }
            out[i] = best
                .map(|j| out[j])
                .unwrap_or_else(|| key_colors.first().copied().unwrap_or((0, 0, 0)));
        }
        out
    }

    fn lamp_count(&self) -> usize {
        self.lamps.len()
    }
}

// ── HID Per-Key Backend ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct HidPerKeyBackend {
    hidraw_paths: Vec<String>,
    device_pid: Option<u16>,
    lamparray: Option<LampArrayBackend>,
}

impl HidPerKeyBackend {
    const HP_VID: u16 = 0x03F0;
    /// Darfon is HP's keyboard ODM — present on OMEN MAX 16 (8D87/8D88/8D42) and OMEN 16-ap0xxx (8D41).
    const DARFON_VID: u16 = 0x0D62;
    /// Primax is HP's ODM for FY22 OMEN 16 KBM (8A4C, #364).
    const PRIMAX_VID: u16 = 0x0461;
    /// Known per-key PIDs for HP VID (OpenRGB + OmenCore confirmed)
    const HP_KNOWN_PIDS: &'static [u16] = &[0x0538, 0x053A, 0x0547, 0x0549, 0x054E, 0x054F];
    /// Known per-key PIDs for Darfon VID
    const DARFON_KNOWN_PIDS: &'static [u16] = &[0x54BF];
    /// Known Primax HP OMEN keyboard PIDs (LampArray-probed safely by report descriptor)
    const PRIMAX_KNOWN_PIDS: &'static [u16] = &[0x4F03];
    const PACKET_SIZE: usize = 65;
    const REPORT_ID: u8 = 0x00;
    const CMD_BYTE: u8 = 0x0F;
    const SUB_ENTER_EFFECT: u8 = 0x42;
    const SUB_SET_COLORS: u8 = 0x52;
    const SUB_COMMIT: u8 = 0x50;
    const STATIC_MODE_ID: u8 = 0x03;
    const MCU_OP_SET_EFFECT: u8 = 0x03;
    const MCU_RECORD_LEN: usize = 36;
    const KEYS_PER_SEGMENT: usize = 20;
    const TOTAL_KEY_COUNT: usize = 104;

    fn new() -> Self {
        let (hidraw_paths, device_pid, lamparray_path) = Self::find_devices();
        let lamparray = lamparray_path.as_deref().and_then(LampArrayBackend::probe);
        let backend = Self { hidraw_paths, device_pid, lamparray };
        if backend.is_available() {
            info!(
                "Initialized HidPerKeyBackend (vendor nodes {:?}, lamparray: {:?})",
                backend.hidraw_paths,
                backend.lamparray.as_ref().map(|l| (l.path.clone(), l.lamp_count()))
            );
            backend.send_enter_per_key_mode(100);
        }
        backend
    }

    fn has_vendor_usage_page(hidraw_sysfs: &Path) -> bool {
        let desc_path = hidraw_sysfs.join("device/report_descriptor");
        if let Ok(bytes) = std::fs::read(&desc_path) {
            for window in bytes.windows(3) {
                if window[0] == 0x06 && window[2] == 0xFF {
                    return true;
                }
            }
        }
        false
    }

    fn find_devices() -> (Vec<String>, Option<u16>, Option<String>) {
        let mut vendor_paths = Vec::new();
        let mut all_paths = Vec::new();
        let mut found_pid = None;
        let mut lamparray_path = None;

        if let Ok(entries) = glob("/sys/class/hidraw/hidraw*") {
            for entry in entries.flatten() {
                let uevent_path = entry.join("device/uevent");
                if let Ok(uevent) = std::fs::read_to_string(&uevent_path) {
                    for line in uevent.lines() {
                        if let Some(vals) = line.strip_prefix("HID_ID=") {
                            let parts: Vec<&str> = vals.split(':').collect();
                            if parts.len() == 3 {
                                if let (Ok(vid), Ok(pid)) = (
                                    u16::from_str_radix(parts[1], 16),
                                    u16::from_str_radix(parts[2], 16),
                                ) {
                                    let is_hp = vid == Self::HP_VID
                                        && Self::HP_KNOWN_PIDS.contains(&pid);
                                    let is_darfon = vid == Self::DARFON_VID
                                        && Self::DARFON_KNOWN_PIDS.contains(&pid);
                                    let is_primax = vid == Self::PRIMAX_VID
                                        && Self::PRIMAX_KNOWN_PIDS.contains(&pid);
                                    if is_hp || is_darfon || is_primax {
                                        let dev_path = format!(
                                            "/dev/{}",
                                            entry.file_name().unwrap_or_default().to_string_lossy()
                                        );

                                        if is_darfon {
                                            found_pid = Some(pid);
                                            let descriptor = std::fs::read(
                                                entry.join("device/report_descriptor"),
                                            )
                                            .unwrap_or_default();
                                            if descriptor.starts_with(LAMPARRAY_PREFIX) {
                                                info!("Found Darfon HID LampArray (per-key keyboard): {}", dev_path);
                                                lamparray_path = Some(dev_path.clone());
                                            } else if descriptor.starts_with(DARFON_VENDOR_PREFIX) {
                                                info!("Found Darfon MCU vendor interface: {}", dev_path);
                                                vendor_paths.push(dev_path.clone());
                                            } else {
                                                let dev_dir = entry.join("device");
                                                let canonical = std::fs::canonicalize(&dev_dir).unwrap_or(dev_dir);
                                                let path_str = canonical.to_string_lossy();
                                                if path_str.contains(":1.3") {
                                                    vendor_paths.push(dev_path.clone());
                                                }
                                            }
                                        } else if is_primax {
                                            let descriptor = std::fs::read(
                                                entry.join("device/report_descriptor"),
                                            )
                                            .unwrap_or_default();
                                            if descriptor.starts_with(LAMPARRAY_PREFIX) {
                                                info!("Found Primax HID LampArray keyboard: {}", dev_path);
                                                found_pid = Some(pid);
                                                lamparray_path = Some(dev_path.clone());
                                            }
                                        } else {
                                            found_pid = Some(pid);
                                            let is_vendor = Self::has_vendor_usage_page(&entry);
                                            info!(
                                                "Found HP Per-Key RGB hidraw node {} (VID={:04X} PID={:04X}, vendor_page={})",
                                                dev_path, vid, pid, is_vendor
                                            );
                                            if is_vendor {
                                                vendor_paths.push(dev_path.clone());
                                            }
                                            all_paths.push(dev_path);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if !vendor_paths.is_empty() {
            (vendor_paths, found_pid, lamparray_path)
        } else {
            all_paths.reverse();
            (all_paths, found_pid, lamparray_path)
        }
    }

    fn is_available(&self) -> bool {
        !self.hidraw_paths.is_empty() || self.lamparray.is_some()
    }

    fn is_darfon(&self) -> bool {
        self.device_pid == Some(0x54BF)
    }

    fn is_lamparray(&self) -> bool {
        self.lamparray.is_some()
    }

    fn release_lamparray(&self) {
        if let Some(ref la) = self.lamparray {
            la.release();
        }
    }

    fn write_darfon_colors(&self, key_colors: &[(u8, u8, u8)]) -> bool {
        // Step 1: Lighting enable prelude (CMD 0x09)
        let mut on_pkt = [0u8; 64];
        on_pkt[0] = 0x09;
        on_pkt[2] = 0x01;
        on_pkt[4] = 0x01;
        let _ = self.write_packet(&on_pkt);

        // Step 2: Map key_colors to 176 Darfon LED positions
        let mut r_leds = [0u8; 176];
        let mut g_leds = [0u8; 176];
        let mut b_leds = [0u8; 176];

        let total_keys = key_colors.len().max(1);
        for (i, &(r, g, b)) in key_colors.iter().enumerate().take(104) {
            let base_led = (i * 176) / total_keys;
            let end_led = ((i + 1) * 176) / total_keys;
            for led in base_led..end_led.min(176) {
                r_leds[led] = r;
                g_leds[led] = g;
                b_leds[led] = b;
            }
        }

        // Step 3: Write Red (0x05), Green (0x06), Blue (0x07) across 3 pages each
        let channels = [(0x05u8, &r_leds), (0x06u8, &g_leds), (0x07u8, &b_leds)];
        for (cmd, leds) in channels {
            for chunk_idx in 0..3 {
                let mut pkt = [0u8; 64];
                pkt[0] = cmd;
                pkt[1] = chunk_idx as u8;
                pkt[2] = 0x00; // Native static pages require BLength=0 (60 payload bytes follow).
                pkt[3] = 0x00;
                let start_idx = chunk_idx * 60;
                for led in 0..60 {
                    let pos = start_idx + led;
                    if pos < 176 {
                        pkt[4 + led] = leds[pos];
                    }
                }
                if !self.write_packet(&pkt) {
                    return false;
                }
            }
        }
        true
    }

    fn build_packet(&self, sub_command: u8) -> [u8; Self::PACKET_SIZE] {
        let mut packet = [0u8; Self::PACKET_SIZE];
        packet[0] = Self::REPORT_ID;
        packet[1] = Self::CMD_BYTE;
        packet[2] = sub_command;
        packet
    }

    fn write_packet(&self, packet: &[u8]) -> bool {
        let mut any_ok = false;
        for path in &self.hidraw_paths {
            match std::fs::OpenOptions::new().write(true).open(path) {
                Ok(mut file) => {
                    use std::io::Write;
                    if let Err(e) = file.write_all(packet) {
                        warn!("Failed to write to {}: {}", path, e);
                    } else {
                        any_ok = true;
                    }
                }
                Err(e) => {
                    warn!("Failed to open {} for writing: {}", path, e);
                }
            }
        }
        any_ok
    }

    fn send_enter_per_key_mode(&self, brightness: u32) -> bool {
        if self.lamparray.is_some() {
            return true;
        }
        let mut packet = self.build_packet(Self::SUB_ENTER_EFFECT);
        packet[3] = Self::STATIC_MODE_ID;
        packet[4] = brightness.clamp(0, 100) as u8;
        self.write_packet(&packet)
    }

    fn send_commit(&self) -> bool {
        self.write_packet(&self.build_packet(Self::SUB_COMMIT))
    }

    fn write_per_key_colors(&self, key_colors: &[(u8, u8, u8)]) -> bool {
        if let Some(ref la) = self.lamparray {
            let mapped = la.key_colors(key_colors);
            return la.set_frame(&mapped);
        }
        if self.is_darfon() {
            return self.write_darfon_colors(key_colors);
        }
        let segment_count = key_colors.len().div_ceil(Self::KEYS_PER_SEGMENT);
        for seg in 0..segment_count {
            let mut packet = self.build_packet(Self::SUB_SET_COLORS);
            packet[3] = seg as u8;
            let start_key = seg * Self::KEYS_PER_SEGMENT;
            let end_key = std::cmp::min(start_key + Self::KEYS_PER_SEGMENT, key_colors.len());

            for (i, &(r, g, b)) in key_colors[start_key..end_key].iter().enumerate() {
                let offset = 4 + i * 3;
                if offset + 2 >= Self::PACKET_SIZE {
                    break;
                }
                packet[offset] = r;
                packet[offset + 1] = g;
                packet[offset + 2] = b;
            }

            if !self.write_packet(&packet) {
                return false;
            }
        }
        self.send_commit()
    }

    /// Map an OMEN Space mode to the closest built-in effect of the Darfon keyboard MCU
    /// (opcode 0x03, "SetLightingEffect", see arfelious/omen-rgb-linux docs/PROTOCOL.md).
    /// Returns None for modes the MCU does not offer (reactive, ripple, custom, gradient...),
    /// which keep using the software streaming path.
    fn mcu_effect_for(&self, mode: &str, speed: u32, direction: &str, colors: &[String]) -> Option<[u8; Self::MCU_RECORD_LEN]> {
        if self.device_pid != Some(0x54BF) {
            return None;
        }
        let parse = |i: usize| parse_hex_color(colors.get(i).map(|s| s.as_str()).unwrap_or("FF0000"));
        let led_speed: u8 = match speed {
            0..=33 => 0,
            34..=66 => 1,
            _ => 2,
        };
        let dir: u8 = if direction == "rtl" { 2 } else { 3 };
        let mut rec = [0u8; Self::MCU_RECORD_LEN];
        rec[3] = led_speed;
        // Match the frames captured from OMEN Gaming Hub: brightness 0 (ignored by the
        // firmware), medium ripple size, RaindropFrequency mirroring LedSpeed.
        rec[4] = 0;
        rec[5] = dir;
        rec[6] = 1;
        rec[7] = led_speed;
        fn put(rec: &mut [u8], slot: usize, c: (u8, u8, u8)) {
            rec[24 + slot * 3] = c.0;
            rec[25 + slot * 3] = c.1;
            rec[26 + slot * 3] = c.2;
        }
        match mode {
            // Wave only supports the built-in Rainbow preset on the MCU.
            "wave_rainbow" => {
                rec[0] = 10;
                rec[1] = 5;
                rec[2] = 4;
            }
            // Colour Cycle through red/green/blue.
            "rainbow" | "cycle" => {
                rec[0] = 4;
                rec[1] = 1;
                rec[2] = 2;
                put(&mut rec, 0, (255, 0, 0));
                put(&mut rec, 1, (0, 255, 0));
                put(&mut rec, 2, (0, 0, 255));
            }
            "breathing" | "pulse" => {
                rec[0] = 2;
                put(&mut rec, 0, parse(0));
            }
            "starlight" => {
                rec[0] = 7;
                put(&mut rec, 0, parse(0));
            }
            "raindrop" => {
                rec[0] = 12;
                put(&mut rec, 0, parse(0));
            }
            _ => return None,
        }
        Some(rec)
    }

    /// Send one effect record; the MCU keeps animating without any host repaint thread.
    /// Volatile only (never issues the flash-store opcode).
    fn send_mcu_effect(&self, record: &[u8; Self::MCU_RECORD_LEN]) -> bool {
        // Release LampArray so the MCU firmware effect can take over (#355)
        self.release_lamparray();
        // OGH always sends "lighting on" (0x09, BLength 1, arg 0x01) right before the effect.
        let mut on = [0u8; 64];
        on[0] = 0x09;
        on[2] = 1;
        on[4] = 0x01;
        self.write_packet(&on);
        let mut packet = [0u8; 64];
        packet[0] = Self::MCU_OP_SET_EFFECT;
        packet[1] = 0; // ALL_LED_AREA
        packet[2] = Self::MCU_RECORD_LEN as u8;
        packet[3] = 0;
        packet[4..4 + Self::MCU_RECORD_LEN].copy_from_slice(record);
        self.write_packet(&packet)
    }

    fn set_zone_colors(&self, colors_hex: &[String]) -> bool {
        let mut colors = Vec::new();
        for c in colors_hex {
            let hex = c.trim_start_matches('#');
            let r = u8::from_str_radix(hex.get(0..2).unwrap_or("00"), 16).unwrap_or(0);
            let g = u8::from_str_radix(hex.get(2..4).unwrap_or("00"), 16).unwrap_or(0);
            let b = u8::from_str_radix(hex.get(4..6).unwrap_or("00"), 16).unwrap_or(0);
            colors.push((r, g, b));
        }
        while colors.len() < 4 {
            colors.push((0, 0, 0));
        }

        if let Some(ref la) = self.lamparray {
            if colors.len() >= Self::TOTAL_KEY_COUNT {
                let key_colors = la.key_colors(&colors[..Self::TOTAL_KEY_COUNT]);
                return la.set_frame(&key_colors);
            }
            let lamp_colors = la.zone_colors(&colors[..4]);
            return la.set_frame(&lamp_colors);
        }

        let mut key_colors = Vec::with_capacity(Self::TOTAL_KEY_COUNT);
        let keys_per_zone = Self::TOTAL_KEY_COUNT / 4;
        for i in 0..Self::TOTAL_KEY_COUNT {
            let zone_idx = std::cmp::min(i / keys_per_zone, 3);
            key_colors.push(colors[zone_idx]);
        }
        self.write_per_key_colors(&key_colors)
    }

    fn test_single_key(&self, key_index: usize, r: u8, g: u8, b: u8) -> bool {
        if !self.is_available() {
            return false;
        }
        self.send_enter_per_key_mode(100);
        let mut key_colors = vec![(0, 0, 0); Self::TOTAL_KEY_COUNT];
        if key_index < Self::TOTAL_KEY_COUNT {
            key_colors[key_index] = (r, g, b);
        }
        self.write_per_key_colors(&key_colors)
    }
}

// ── Service config ────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
struct RgbConfig {
    mode: String,
    colors: Vec<String>,
    speed: u32,
    brightness: u32,
    direction: String,
    power: bool,
    win_lock: bool,
    /// User-declared physical zone count for the hp-omen-extra driver
    /// (4 or 8). None means auto/default. Set via GUI Settings ->
    /// Zone Mode Override, since the driver's sysfs files can't tell us
    /// which one the board actually has.
    #[serde(default)]
    zone_count_override: Option<u32>,
    /// Experimental (#322): on Darfon 0d62:54bf per-key boards, use the keyboard MCU's
    /// built-in effects instead of streaming frames from the daemon. Off by default.
    #[serde(default)]
    hw_effects: bool,
    #[serde(default = "default_lightbar_colors")]
    lightbar_colors: Vec<String>,
    #[serde(default = "default_true")]
    lightbar_power: bool,
    #[serde(default = "default_100")]
    lightbar_brightness: u32,
    #[serde(default = "default_static")]
    lightbar_mode: String,
    #[serde(default = "default_50")]
    lightbar_speed: u32,
    #[serde(default = "default_per_key_colors")]
    per_key_colors: Vec<String>,
}

fn default_per_key_colors() -> Vec<String> {
    vec!["0099ED".to_string(); HidPerKeyBackend::TOTAL_KEY_COUNT]
}
fn default_lightbar_colors() -> Vec<String> {
    vec!["E03454".to_string(), "0099EE".to_string(), "0099EE".to_string(), "E03454".to_string()]
}
fn default_true() -> bool { true }
fn default_100() -> u32 { 100 }
fn default_50() -> u32 { 50 }
fn default_static() -> String { "static".to_string() }

impl Default for RgbConfig {
    fn default() -> Self {
        Self {
            mode: "static".to_string(),
            colors: vec!["FF0000".to_string(); 8],
            speed: 50,
            brightness: 100,
            direction: "ltr".to_string(),
            power: true,
            win_lock: false,
            zone_count_override: None,
            hw_effects: false,
            lightbar_colors: default_lightbar_colors(),
            lightbar_power: true,
            lightbar_brightness: 100,
            lightbar_mode: "static".to_string(),
            lightbar_speed: 50,
            per_key_colors: default_per_key_colors(),
        }
    }
}

impl RgbConfig {
    fn load() -> Self {
        if let Ok(data) = std::fs::read_to_string(CONFIG_PATH) {
            let mut cfg: Self = serde_json::from_str(&data).unwrap_or_default();
            // Validate mode
            if !VALID_MODES.contains(&cfg.mode.as_str()) {
                cfg.mode = "static".to_string();
            }
            // Ensure 8 colors, validate hex
            cfg.colors = cfg.colors.into_iter()
                .filter(|c| c.len() == 6 && c.chars().all(|ch| ch.is_ascii_hexdigit()))
                .take(8)
                .collect();
            while cfg.colors.len() < 8 {
                cfg.colors.push("FF0000".to_string());
            }
            cfg
        } else {
            Self::default()
        }
    }

    fn save(&self) {
        if let Some(dir) = Path::new(CONFIG_PATH).parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(CONFIG_PATH, json);
        }
    }
}

// ── Service ───────────────────────────────────────────────────────────────────

struct RgbInner {
    evdev_monitor: crate::evdev_monitor::EvdevMonitor,
    hw: RgbHardware,
    hid_per_key: HidPerKeyBackend,
    config: RgbConfig,
    per_key_map: HashMap<String, serde_json::Value>,
    color_cache: HashMap<usize, String>,
    anim_step: f64,
    lightbar_step: f64,
    last_lightbar_level: i32,
    /// Signature of the last effect record sent to the keyboard MCU (None = resend).
    mcu_sig: Option<Vec<u8>>,
    wizard: Arc<crate::hid_wizard::HidPerKeyWizard>,
    desktop_rgb: crate::desktop_rgb::DesktopRgbController,
}

#[derive(Clone)]
pub struct RgbService {
    inner: Arc<Mutex<RgbInner>>,
}

impl RgbService {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let config = RgbConfig::load();
        let hw = RgbHardware::detect_with_override(config.zone_count_override);
        if hw.available {
            info!("RGB: Driver at {:?} (new_driver={}, zone_count={})", hw.driver_path, hw.is_new_driver, hw.zone_count);
        } else {
            warn!("RGB: No RGB hardware driver found");
        }

        let per_key_map = Self::load_per_key_map();
        let hid_per_key = HidPerKeyBackend::new();
        let wizard = Arc::new(crate::hid_wizard::HidPerKeyWizard::new());
        let evdev_monitor = crate::evdev_monitor::EvdevMonitor::new();
        
        let mut desktop_rgb = crate::desktop_rgb::DesktopRgbController::new();
        if let Err(e) = desktop_rgb.initialize() {
            warn!("RGB: Desktop RGB not initialized: {}", e);
        }

        let inner = Arc::new(Mutex::new(RgbInner {
            hw, hid_per_key, config, per_key_map, color_cache: HashMap::new(), anim_step: 0.0, lightbar_step: 0.0, last_lightbar_level: -1, mcu_sig: None, wizard, evdev_monitor, desktop_rgb,
        }));

        let svc = Self { inner: inner.clone() };

        // Apply initial state to hardware
        Self::apply_state(inner.clone()).await;

        // Deferred re-apply: some boards (e.g. 88F7) have the kernel driver reset
        // LED state a few seconds after module initialization. Re-applying after 5s
        // ensures the saved power=false / color config wins (issue #168).
        let deferred_inner = inner.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            Self::apply_state(deferred_inner).await;
            info!("RGB: deferred re-apply completed");
        });

        // Spawn software animation loop
        let anim_inner = inner.clone();
        tokio::spawn(async move {
            Self::software_animation_loop(anim_inner).await;
        });

        // Spawn independent lightbar animation loop for LampArray per-key boards (#355)
        let lb_inner = inner.clone();
        tokio::spawn(async move {
            Self::lightbar_animation_loop(lb_inner).await;
        });
        
        // Spawn uleds listener loop
        let uleds_inner = inner.clone();
        tokio::spawn(async move {
            Self::uleds_monitor_loop(uleds_inner).await;
        });

        Ok(svc)
    }

    fn load_per_key_map() -> HashMap<String, serde_json::Value> {
        if let Ok(data) = std::fs::read_to_string(PER_KEY_MAP_PATH) {
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            HashMap::new()
        }
    }

    /// Listens for brightness changes from /dev/uleds to integrate natively with Linux OS brightness sliders
    async fn uleds_monitor_loop(inner: Arc<Mutex<RgbInner>>) {
        #[repr(C)]
        struct UledsUserDev {
            name: [u8; 64],
            max_brightness: std::ffi::c_int,
        }

        let mut dev = UledsUserDev {
            name: [0; 64],
            max_brightness: 100,
        };
        let name_str = b"omen::kbd_backlight";
        dev.name[..name_str.len()].copy_from_slice(name_str);

        let file = match File::options().read(true).write(true).open("/dev/uleds") {
            Ok(f) => f,
            Err(e) => {
                info!("uleds module not available or accessible, skipping native OS brightness integration: {}", e);
                return;
            }
        };

        // Write the struct to register the LED
        let struct_bytes = unsafe {
            std::slice::from_raw_parts(
                (&dev as *const UledsUserDev) as *const u8,
                std::mem::size_of::<UledsUserDev>(),
            )
        };
        
        use std::io::Write;
        let mut std_file = file;
        if let Err(e) = std_file.write_all(struct_bytes) {
            warn!("Failed to register uleds device: {}", e);
            return;
        }

        info!("Successfully registered /sys/class/leds/omen::kbd_backlight");

        // Now convert to async file to avoid blocking executor during read
        let mut async_file = tokio::fs::File::from_std(std_file);

        loop {
            let mut val_bytes = [0u8; 4];
            match async_file.read_exact(&mut val_bytes).await {
                Ok(_) => {
                    let new_brightness = i32::from_ne_bytes(val_bytes);
                    let mut should_apply = false;
                    {
                        let mut g = inner.lock().await;
                        let clamped = new_brightness.clamp(0, 100) as u32;
                        if g.config.brightness != clamped || (clamped > 0 && !g.config.power) {
                            g.config.brightness = clamped;
                            if clamped > 0 {
                                g.config.power = true;
                            } else {
                                g.config.power = false; // Usually 0 means off
                            }
                            g.config.save();
                            should_apply = true;
                        }
                    }
                    if should_apply {
                        Self::apply_state(inner.clone()).await;
                    }
                }
                Err(e) => {
                    warn!("uleds read error: {}, reconnecting...", e);
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    break; // Could implement reconnect logic if needed
                }
            }
        }
    }

    async fn lightbar_animation_loop(inner: Arc<Mutex<RgbInner>>) {
        loop {
            let mut g = inner.lock().await;
            if !g.hw.available || !g.hw.has_lightbar() || !g.hid_per_key.is_lamparray() {
                drop(g);
                tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
                continue;
            }
            let power = g.config.lightbar_power && g.config.lightbar_brightness > 0;
            let mode = g.config.lightbar_mode.clone();
            if !power || mode == "static" {
                drop(g);
                tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
                continue;
            }
            let speed = g.config.lightbar_speed;
            let scaler = (g.config.lightbar_brightness as f64 / 100.0).clamp(0.0, 1.0);
            let colors = g.config.lightbar_colors.clone();
            let (r1, g1, b1) = parse_hex_color(colors.first().map(|s| s.as_str()).unwrap_or("FF0000"));
            let (r2, g2, b2) = parse_hex_color(colors.get(1).map(|s| s.as_str()).unwrap_or("0000FF"));
            g.lightbar_step += (speed as f64 / 100.0) * 0.25;
            let step = g.lightbar_step;
            let mut frame = Vec::with_capacity(4);
            for i in 0..4 {
                let (r, gc, b) = compute_anim_color(&mode, step, i, 4, r1, g1, b1, r2, g2, b2);
                frame.push(format!(
                    "{:02X}{:02X}{:02X}",
                    ((r as f64) * scaler) as u8,
                    ((gc as f64) * scaler) as u8,
                    ((b as f64) * scaler) as u8
                ));
            }
            g.hw.write_lightbar_zones(&frame);
            drop(g);
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
    }

    async fn software_animation_loop(inner: Arc<Mutex<RgbInner>>) {
        let mut last_per_key_colors: Vec<String> = Vec::new();
        let mut last_sysfs_frame: Vec<String> = Vec::new();
        loop {
            let mut g = inner.lock().await;
            let has_per_key = g.hid_per_key.is_available();
            let has_old_sysfs = g.hw.available && !g.hw.is_new_driver;

            // Only activate evdev key monitoring when interactive effects (reactive, ripple) are chosen
            let is_interactive = g.config.power && has_per_key && (g.config.mode == "reactive" || g.config.mode == "ripple");
            g.evdev_monitor.set_active(is_interactive);

            if !has_per_key && !has_old_sysfs {
                drop(g);
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                continue;
            }
            if !g.config.power || g.config.mode == "static" || g.config.mode == "per_key_custom" { 
                last_sysfs_frame.clear();
                drop(g);
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                continue; 
            }

            let brightness = g.config.brightness;
            let scaler = (brightness as f64 / 100.0).clamp(0.0, 1.0);
            let speed = g.config.speed;
            let direction = g.config.direction.clone();
            let colors = g.config.colors.clone();
            let mode = g.config.mode.clone();

            // Hardware effects (#322, #355): let the keyboard MCU animate by itself.
            // If the selected mode has no MCU equivalent, release the LampArray so Fn+1/Fn+2 work.
            if has_per_key && g.config.hw_effects {
                if let Some(rec) = g.hid_per_key.mcu_effect_for(&mode, speed, &direction, &colors) {
                    let sig = rec.to_vec();
                    if g.mcu_sig.as_ref() != Some(&sig) && g.hid_per_key.send_mcu_effect(&rec) {
                        g.mcu_sig = Some(sig);
                    }
                } else {
                    g.hid_per_key.release_lamparray();
                    g.mcu_sig = None;
                }
                last_per_key_colors.clear();
                drop(g);
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                continue;
            }
            g.mcu_sig = None;

            let tick_ms: u64 = if !has_per_key && has_old_sysfs { 150 } else { 50 };
            let step_inc = (speed as f64 / 100.0) * 0.25 * (tick_ms as f64 / 50.0);
            g.anim_step += step_inc;
            let step = g.anim_step;

            let (r1, g1, b1) = parse_hex_color(colors.first().map(|s| s.as_str()).unwrap_or("FF0000"));
            let (r2, g2, b2) = parse_hex_color(colors.get(1).map(|s| s.as_str()).unwrap_or("0000FF"));

            if has_per_key {
                let mut out_colors = Vec::with_capacity(104);
                let recent_keys = g.evdev_monitor.recent_keys.lock().await.clone();
                let now = std::time::Instant::now();

                for i in 0..104 {
                    // map i to approximate x, y
                    let x = (i % 15) as f64;
                    let y = (i / 15) as f64;

                    let mut r_final = 0.0;
                    let mut g_final = 0.0;
                    let mut b_final = 0.0;

                    if mode == "reactive" {
                        // Find most recent key near this spot
                        let mut max_intensity = 0.0;
                        for key in &recent_keys {
                            let dist = ((key.x - x).powi(2) + (key.y - y).powi(2)).sqrt();
                            if dist < 1.0 { // exact key
                                let age = now.duration_since(key.timestamp).as_secs_f64();
                                let intensity = (1.0 - (age * 1.5)).clamp(0.0, 1.0); // fade out in ~0.66s
                                if intensity > max_intensity { max_intensity = intensity; }
                            }
                        }
                        r_final = r1 as f64 * max_intensity;
                        g_final = g1 as f64 * max_intensity;
                        b_final = b1 as f64 * max_intensity;
                    } else if mode == "ripple" {
                        for key in &recent_keys {
                            let dist = ((key.x - x).powi(2) + (key.y - y).powi(2)).sqrt();
                            let age = now.duration_since(key.timestamp).as_secs_f64();
                            // ripple radius grows over time
                            let radius = age * 15.0; // speed
                            let thickness = 1.5;
                            if (dist - radius).abs() < thickness {
                                let intensity = (1.0 - (age * 0.8)).clamp(0.0, 1.0);
                                r_final += r1 as f64 * intensity;
                                g_final += g1 as f64 * intensity;
                                b_final += b1 as f64 * intensity;
                            }
                        }
                        r_final = r_final.clamp(0.0, 255.0);
                        g_final = g_final.clamp(0.0, 255.0);
                        b_final = b_final.clamp(0.0, 255.0);
                    } else if mode == "starlight" {
                        use std::hash::{Hash, Hasher};
                        use std::collections::hash_map::DefaultHasher;
                        let mut h = DefaultHasher::new();
                        let slow_step = (step * 2.0) as u64;
                        (slow_step, i).hash(&mut h);
                        if h.finish().is_multiple_of(20) {
                            r_final = r1 as f64; g_final = g1 as f64; b_final = b1 as f64;
                        } else {
                            r_final = 0.0; g_final = 0.0; b_final = 0.0;
                        }
                    } else if mode == "raindrop" {
                        let speed_factor = step * 10.0;
                        let col = (i % 15) as f64;
                        let drop_pos = (speed_factor + (col * 3.7)) % 10.0; // random offset per column
                        let dist = (y - drop_pos).abs();
                        if dist < 1.0 {
                            r_final = r1 as f64; g_final = g1 as f64; b_final = b1 as f64;
                        }
                    } else {
                        // wave, cycle, breathing
                        let eff_idx = if direction == "ltr" { i % 15 } else if direction == "rtl" { 14 - (i % 15) } else { i };
                        let (r_c, g_c, b_c) = compute_anim_color(&mode, step, eff_idx, 15, r1, g1, b1, r2, g2, b2);
                        r_final = r_c as f64; g_final = g_c as f64; b_final = b_c as f64;
                    }

                    out_colors.push(format!("{:02X}{:02X}{:02X}", 
                        (r_final * scaler) as u8, 
                        (g_final * scaler) as u8, 
                        (b_final * scaler) as u8));
                }
                
                if out_colors != last_per_key_colors {
                    let frame: Vec<_> = out_colors.iter().map(|c| parse_hex_color(c)).collect();
                    g.hid_per_key.write_per_key_colors(&frame);
                    last_per_key_colors = out_colors;
                }
            }

            if has_old_sysfs && !g.hid_per_key.is_lamparray() {
                let zone_count = g.hw.zone_count as usize;
                let mut current_frame = Vec::with_capacity(zone_count);
                for i in 0..zone_count {
                    let eff_idx = if direction == "ltr" { i } else { zone_count - 1 - i };
                    let (r, g_c, b) = compute_anim_color(&mode, step, eff_idx, zone_count, r1, g1, b1, r2, g2, b2);
                    let scaled = format!(
                        "{:02X}{:02X}{:02X}",
                        ((r as f64) * scaler) as u8,
                        ((g_c as f64) * scaler) as u8,
                        ((b as f64) * scaler) as u8
                    );
                    current_frame.push(scaled);
                }
                if current_frame != last_sysfs_frame {
                    g.hw.write_zones_batch(&current_frame);
                    last_sysfs_frame = current_frame;
                }
            }

            drop(g);
            tokio::time::sleep(tokio::time::Duration::from_millis(tick_ms)).await;
        }
    }
    /// Apply only the lightbar so changing lightbar segments/mode never re-applies or resets keyboard (#355).
    async fn apply_lightbar(inner: Arc<Mutex<RgbInner>>) {
        let mut g = inner.lock().await;
        if !g.hw.available || !g.hw.lightbar_write_allowed() {
            return;
        }
        let effect_active = g.config.lightbar_mode != "static"
            && g.config.lightbar_power
            && g.config.lightbar_brightness > 0;
        if !effect_active {
            let lb_colors: Vec<String> = if !g.config.lightbar_power || g.config.lightbar_brightness == 0 {
                vec!["000000".to_string(); 4]
            } else {
                let scaler = (g.config.lightbar_brightness as f64 / 100.0).clamp(0.0, 1.0);
                (0..4).map(|i| {
                    let raw = g.config.lightbar_colors.get(i).cloned().unwrap_or_else(|| "000000".to_string());
                    scale_hex_color(&raw, scaler)
                }).collect()
            };
            if g.hid_per_key.is_lamparray() && g.hw.has_lightbar() {
                g.hw.write_lightbar_zones(&lb_colors);
            } else {
                g.hw.write_lightbar(&lb_colors);
            }
        }
        if g.hid_per_key.is_lamparray() && g.hw.has_lightbar() {
            let level = if g.hw.is_new_driver {
                if g.config.lightbar_power { g.config.lightbar_brightness as i32 } else { 0 }
            } else if g.config.lightbar_power && g.config.lightbar_brightness > 0 {
                1
            } else {
                0
            };
            if g.last_lightbar_level != level {
                g.hw.write_brightness(level as u32);
                g.last_lightbar_level = level;
            }
        }
    }

    async fn apply_state(inner: Arc<Mutex<RgbInner>>) {
        let mut g = inner.lock().await;

        if !g.hw.available {
            g.hw = RgbHardware::detect();
        } else if let Some(ref path) = g.hw.driver_path {
            if !Path::new(path).exists() {
                g.hw = RgbHardware::detect();
            }
        }

        g.mcu_sig = None;
        let power     = g.config.power;
        let brightness = g.config.brightness;
        let mode      = g.config.mode.clone();
        let speed     = g.config.speed;
        let colors    = g.config.colors.clone();

        // ── HID per-key backend (USB direct) ────────────────────────────────────
        if g.hid_per_key.is_available() {
            if g.config.hw_effects && g.hid_per_key.is_lamparray() {
                g.hid_per_key.release_lamparray();
            } else if !power || brightness == 0 {
                g.hid_per_key.send_enter_per_key_mode(0);
                g.hid_per_key.set_zone_colors(&vec!["000000".to_string(); 8]);
            } else {
                g.hid_per_key.send_enter_per_key_mode(brightness);
                if mode == "static" {
                    let static_colors = if g.hid_per_key.is_lamparray() {
                        let scaler = (brightness as f64 / 100.0).clamp(0.0, 1.0);
                        colors.iter().map(|c| scale_hex_color(c, scaler)).collect()
                    } else {
                        colors.clone()
                    };
                    g.hid_per_key.set_zone_colors(&static_colors);
                } else if mode == "per_key_custom" {
                    let scaler = if g.hid_per_key.is_lamparray() {
                        (brightness as f64 / 100.0).clamp(0.0, 1.0)
                    } else {
                        1.0
                    };
                    let per_key = g.config.per_key_colors.clone();
                    let key_colors: Vec<(u8, u8, u8)> = per_key
                        .iter()
                        .take(HidPerKeyBackend::TOTAL_KEY_COUNT)
                        .map(|hex| {
                            let (r, g_c, b) = parse_hex_color(hex);
                            (
                                (r as f64 * scaler) as u8,
                                (g_c as f64 * scaler) as u8,
                                (b as f64 * scaler) as u8,
                            )
                        })
                        .collect();
                    g.hid_per_key.write_per_key_colors(&key_colors);
                }
            }
        }
        
        // ── Desktop RGB ─────────────────────────────────────────────────────────
        if g.desktop_rgb.is_available() {
            if !power || brightness == 0 {
                let _ = g.desktop_rgb.set_static_colors(&[(0,0,0); 7], 0);
            } else if mode == "static" {
                let mut parsed_colors = Vec::new();
                for i in 0..7 {
                    let hex = colors.get(10 + i).cloned().unwrap_or_else(|| colors.first().cloned().unwrap_or_else(|| "FF0000".to_string()));
                    parsed_colors.push(parse_hex_color(&hex));
                }
                let _ = g.desktop_rgb.set_static_colors(&parsed_colors, brightness as u8);
            }
        }

        // ── Lightbar ────────────────────────────────────────────────────────────
        if g.hw.available && g.hw.lightbar_write_allowed() {
            let effect_active = g.config.lightbar_mode != "static"
                && g.config.lightbar_power
                && g.config.lightbar_brightness > 0;
            if !effect_active {
                let lb_colors: Vec<String> = if !g.config.lightbar_power || g.config.lightbar_brightness == 0 {
                    vec!["000000".to_string(); 4]
                } else {
                    let scaler = (g.config.lightbar_brightness as f64 / 100.0).clamp(0.0, 1.0);
                    (0..4).map(|i| {
                        let raw = g.config.lightbar_colors.get(i).cloned().unwrap_or_else(|| "000000".to_string());
                        scale_hex_color(&raw, scaler)
                    }).collect()
                };
                if g.hid_per_key.is_lamparray() && g.hw.has_lightbar() {
                    g.hw.write_lightbar_zones(&lb_colors);
                } else {
                    g.hw.write_lightbar(&lb_colors);
                }
            }
        }

        // ── Standard sysfs driver ───────────────────────────────────────────────
        if !g.hw.available { return; }

        // On LampArray per-key boards (8D88 / 8D42), WMI zone attributes belong to the
        // front lightbar — do NOT overwrite them with keyboard zone colors (#355).
        if g.hid_per_key.is_lamparray() {
            return;
        }

        if !power || brightness == 0 {
            g.hw.write_brightness(0);
            g.hw.write_all("000000");
            return;
        }

        if g.hw.is_new_driver {
            g.hw.write_brightness(brightness);
            g.hw.write_mode(&mode, speed);
            if mode == "static" {
                let count = g.hw.zone_count as usize;
                let mut zone_colors = Vec::with_capacity(count);
                for i in 0..count {
                    zone_colors.push(colors.get(i).cloned().unwrap_or_else(|| colors[0].clone()));
                }
                g.hw.write_zones_batch(&zone_colors);
            }
        } else {
            g.hw.write_brightness(1);
            if mode == "static" {
                let scaler = (brightness as f64 / 100.0).clamp(0.0, 1.0);
                let count = g.hw.zone_count as usize;
                let mut scaled_colors = Vec::with_capacity(count);
                for i in 0..count {
                    let raw = colors.get(i).cloned().unwrap_or_else(|| colors[0].clone());
                    scaled_colors.push(scale_hex_color(&raw, scaler));
                }
                g.hw.write_zones_batch(&scaled_colors);
            }
        }
    }
}

// ── D-Bus interface ────────────────────────────────────────────────────────────

#[interface(name = "org.hp.omen.Rgb")]
impl RgbService {
    /// SetColor(z, h) — mirrors Python SetColor().
    /// z=8 means all zones.
    async fn set_color(&self, zone_val: i32, hex_color: String) -> String {
        let hex = hex_color.trim_start_matches('#').to_uppercase();
        if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return "FAIL".to_string();
        }
        if zone_val != 8 && !(0..17).contains(&zone_val) {
            return "FAIL".to_string();
        }
        {
            let mut g = self.inner.lock().await;
            g.config.mode = "static".to_string();
            g.config.power = true;
            g.config.hw_effects = false;
            if g.config.colors.len() < 17 {
                g.config.colors.resize(17, "FF0000".to_string());
            }
            if zone_val == 8 {
                for slot in g.config.colors.iter_mut().take(8) { *slot = hex.clone(); }
            } else {
                g.config.colors[zone_val as usize] = hex.clone();
            }
            g.config.save();
        }
        
        Self::apply_state(self.inner.clone()).await;
        "OK".to_string()
    }

    /// SetMode(m, s) — mirrors Python SetMode().
    async fn set_mode(&self, mode_str: String, speed_val: i32) -> String {
        if !VALID_MODES.contains(&mode_str.as_str()) {
            return "FAIL".to_string();
        }
        {
            let mut g = self.inner.lock().await;
            g.config.mode = mode_str;
            g.config.speed = (speed_val as u32).clamp(1, 100);
            g.config.power = true;
            g.config.save();
        }
        
        Self::apply_state(self.inner.clone()).await;
        "OK".to_string()
    }

    /// SetGlobal(p, b, d) — mirrors Python SetGlobal().
    async fn set_global(&self, power_val: bool, brightness_val: i32, direction_str: String) -> String {
        if !VALID_DIRECTIONS.contains(&direction_str.as_str()) {
            return "FAIL".to_string();
        }
        {
            let mut g = self.inner.lock().await;
            g.config.power = power_val;
            g.config.brightness = (brightness_val as u32).clamp(0, 100);
            g.config.direction = direction_str;
            g.config.save();
        }
        
        Self::apply_state(self.inner.clone()).await;
        "OK".to_string()
    }

    /// SetLightbarColor(z, h)
    async fn set_lightbar_color(&self, zone_val: i32, hex_color: String) -> String {
        let hex = hex_color.trim_start_matches('#').to_uppercase();
        if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return "FAIL".to_string();
        }
        {
            let mut g = self.inner.lock().await;
            if g.config.lightbar_colors.len() < 4 {
                g.config.lightbar_colors.resize(4, "000000".to_string());
            }
            if zone_val == 4 {
                for slot in g.config.lightbar_colors.iter_mut().take(4) { *slot = hex.clone(); }
            } else if (0..4).contains(&zone_val) {
                g.config.lightbar_colors[zone_val as usize] = hex;
            } else {
                return "FAIL".to_string();
            }
            g.config.lightbar_power = true;
            g.config.save();
        }
        Self::apply_lightbar(self.inner.clone()).await;
        "OK".to_string()
    }

    /// SetLightbarGlobal(power, brightness, mode, speed)
    async fn set_lightbar_global(&self, power_val: bool, brightness_val: i32, mode_str: String, speed_val: i32) -> String {
        {
            let mut g = self.inner.lock().await;
            g.config.lightbar_power = power_val;
            g.config.lightbar_brightness = (brightness_val as u32).clamp(0, 100);
            g.config.lightbar_mode = mode_str;
            g.config.lightbar_speed = (speed_val as u32).clamp(1, 100);
            g.config.save();
        }
        Self::apply_lightbar(self.inner.clone()).await;
        "OK".to_string()
    }

    /// GetState — mirrors Python GetState().
    async fn get_state(&self) -> String {
        let g = self.inner.lock().await;
        let any_driver = g.hw.available || g.hid_per_key.is_available() || g.desktop_rgb.is_available();
        let mut snap = serde_json::json!({
            "mode": g.config.mode,
            "colors": g.config.colors,
            "per_key_colors": g.config.per_key_colors,
            "speed": g.config.speed,
            "brightness": g.config.brightness,
            "direction": g.config.direction,
            "power": g.config.power,
            "win_lock": g.config.win_lock,
            "unsupported": !any_driver,
            "driver_active": any_driver,
            "driver_path": g.hw.driver_path.clone().unwrap_or_default(),
            "is_new_driver": g.hw.is_new_driver,
            "zone_count": g.hw.zone_count,
            "zone_count_override": g.config.zone_count_override,
            "per_key_available": g.hid_per_key.is_available(),
            "is_lamparray": g.hid_per_key.is_lamparray(),
            "hw_effects": g.config.hw_effects,
            "hid_device_pid": g.hid_per_key.device_pid.map(|p| format!("{:04X}", p)).unwrap_or_default(),
            "has_lightbar": g.hw.has_lightbar(),
            "lightbar_colors": g.config.lightbar_colors,
            "lightbar_power": g.config.lightbar_power,
            "lightbar_brightness": g.config.lightbar_brightness,
            "lightbar_mode": g.config.lightbar_mode,
            "lightbar_speed": g.config.lightbar_speed,
        });
        if !g.hw.available && !g.hid_per_key.is_available() {
            snap["unavailable_reason"] = serde_json::Value::String(
                "RGB kernel module not loaded. Install 'hp_omen_extra' or 'omen-rgb-keyboard'. \
                 No HID per-key device found either.".to_string()
            );
        }
        snap.to_string()
    }

    /// SetZoneCountOverride(n) — Force the physical zone count (4 or 8) used
    /// for the legacy hp-omen-extra/hp_omen_extra driver, since its sysfs
    /// interface always exposes zone0..zone7 regardless of the board's real
    /// zone count and there is no reliable way to auto-detect it from the
    /// WMI interface. Pass 0 to clear the override and fall back to the
    /// default (4). Re-detects hardware and re-applies state immediately so
    /// the change is visible without restarting the daemon.
    async fn set_zone_count_override(&self, zone_count: i32) -> String {
        let normalized = match zone_count {
            0 => None,
            1 => Some(1),
            4 => Some(4),
            8 => Some(8),
            _ => return "FAIL: zone_count must be 0 (clear), 1, 4, or 8".to_string(),
        };
        {
            let mut g = self.inner.lock().await;
            g.config.zone_count_override = normalized;
            g.config.save();
            g.hw = RgbHardware::detect_with_override(normalized);
            info!("SetZoneCountOverride: override={:?}, effective zone_count={}", normalized, g.hw.zone_count);
        }
        Self::apply_state(self.inner.clone()).await;
        "OK".to_string()
    }

    /// SetHwEffects(enabled) — experimental (#322). On Darfon 0d62:54bf keyboards,
    /// run supported effects on the keyboard MCU instead of streaming frames.
    async fn set_hw_effects(&self, enabled: bool) -> String {
        {
            let mut g = self.inner.lock().await;
            g.config.hw_effects = enabled;
            g.mcu_sig = None;
            g.config.save();
            info!("SetHwEffects: {}", enabled);
        }
        Self::apply_state(self.inner.clone()).await;
        "OK".to_string()
    }

    /// SetWinLock(locked) — mirrors Python SetWinLock().
    async fn set_win_lock(&self, locked: bool) -> String {
        let mut g = self.inner.lock().await;
        g.config.win_lock = locked;
        g.hw.write_win_lock(locked);
        g.config.save();
        info!("SetWinLock: {}", if locked { "LOCKED" } else { "UNLOCKED" });
        "OK".to_string()
    }

    /// TestSingleKey(index) — mirrors Python TestSingleKey().
    async fn test_single_key(&self, index: i32) -> String {
        info!("TestSingleKey: index={}", index);
        let g = self.inner.lock().await;
        if g.hid_per_key.is_available()
            && g.hid_per_key.test_single_key(index as usize, 255, 0, 0) {
                return "OK".to_string();
            }
        "FAIL".to_string()
    }

    /// SetPerKeyColors(colors_json) — Write per-key colors for all 104 keys.
    ///
    /// colors_json must be a JSON array of exactly 104 hex color strings ("RRGGBB" or "#RRGGBB").
    /// Sends the full HID packet sequence: EnterEffect → SetColors (6 segments) → Commit.
    async fn set_per_key_colors(&self, colors_json: String) -> String {
        let parsed: Result<Vec<String>, _> = serde_json::from_str(&colors_json);
        let colors = match parsed {
            Ok(c) => c,
            Err(e) => {
                warn!("SetPerKeyColors: JSON parse error: {}", e);
                return "FAIL: invalid JSON array".to_string();
            }
        };
        if colors.len() != HidPerKeyBackend::TOTAL_KEY_COUNT {
            warn!("SetPerKeyColors: expected {} colors, got {}", HidPerKeyBackend::TOTAL_KEY_COUNT, colors.len());
            return format!("FAIL: expected {} colors, got {}", HidPerKeyBackend::TOTAL_KEY_COUNT, colors.len());
        }

        let mut g = self.inner.lock().await;
        if !g.hid_per_key.is_available() {
            return "FAIL: no HID per-key device".to_string();
        }

        // Persist the 104-key frame and take ownership back from firmware hw_effects (#355)
        g.config.per_key_colors = colors
            .iter()
            .map(|c| c.trim_start_matches('#').to_uppercase())
            .collect();
        g.config.mode = "per_key_custom".to_string();
        g.config.power = true;
        g.config.hw_effects = false;
        g.mcu_sig = None;
        g.config.save();

        let scaler = if g.hid_per_key.is_lamparray() {
            (g.config.brightness as f64 / 100.0).clamp(0.0, 1.0)
        } else {
            1.0
        };

        let key_colors: Vec<(u8, u8, u8)> = g.config.per_key_colors.iter().map(|hex| {
            let (r, g_c, b) = parse_hex_color(hex);
            (
                (r as f64 * scaler) as u8,
                (g_c as f64 * scaler) as u8,
                (b as f64 * scaler) as u8,
            )
        }).collect();

        g.hid_per_key.send_enter_per_key_mode(g.config.brightness);
        if g.hid_per_key.write_per_key_colors(&key_colors) {
            info!("SetPerKeyColors: wrote {} key colors via HID", key_colors.len());
            "OK".to_string()
        } else {
            warn!("SetPerKeyColors: HID write failed");
            "FAIL: HID write error".to_string()
        }
    }
    /// StartPerKeyWizard — Starts interactive HID Per-Key RGB Calibration Wizard
    async fn start_per_key_wizard(&self) -> String {
        let wizard = self.inner.lock().await.wizard.clone();
        wizard.start_wizard().await
    }

    /// LightKeyIndex — Lights up a specific HID key index with specified hex color
    async fn light_key_index(&self, index: u32, hex_color: String) -> String {
        let wizard = self.inner.lock().await.wizard.clone();
        wizard.light_key_index(index as usize, &hex_color).await
    }

    /// RecordKeyMapping — Maps index to physical key name and advances to next key
    async fn record_key_mapping(&self, index: u32, key_name: String) -> String {
        let wizard = self.inner.lock().await.wizard.clone();
        wizard.record_key_mapping(index as usize, &key_name).await
    }

    /// ExportKeymapReport — Exports the calibrated keymap JSON & Markdown dump
    async fn export_keymap_report(&self) -> String {
        let wizard = self.inner.lock().await.wizard.clone();
        wizard.export_keymap().await
    }

    /// SavePerKeyMap(map_json) — mirrors Python SavePerKeyMap().
    async fn save_per_key_map(&self, map_json: String) -> String {
        // Validate JSON then persist raw string
        if serde_json::from_str::<serde_json::Value>(&map_json).is_err() {
            warn!("SavePerKeyMap: invalid JSON");
            return "FAIL".to_string();
        }
        if let Some(dir) = Path::new(PER_KEY_MAP_PATH).parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match std::fs::write(PER_KEY_MAP_PATH, &map_json) {
            Ok(_) => {
                info!("SavePerKeyMap: saved");
                "OK".to_string()
            }
            Err(e) => { warn!("SavePerKeyMap write error: {}", e); "FAIL".to_string() }
        }
    }

    async fn ping(&self) -> String {
        "OK".to_string()
    }
}

pub async fn test_single_key_static(index: usize, r: u8, g: u8, b: u8) -> bool {
    static HID: std::sync::OnceLock<HidPerKeyBackend> = std::sync::OnceLock::new();
    let hid = HID.get_or_init(HidPerKeyBackend::new);
    if hid.is_available() {
        hid.test_single_key(index, r, g, b)
    } else {
        false
    }
}

// ── Color math helpers ─────────────────────────────────────────────────────────

fn parse_hex_color(hex: &str) -> (u8, u8, u8) {
    let h = hex.trim_start_matches('#');
    if h.len() < 6 { return (255, 0, 0); }
    let r = u8::from_str_radix(&h[0..2], 16).unwrap_or(255);
    let g = u8::from_str_radix(&h[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&h[4..6], 16).unwrap_or(0);
    (r, g, b)
}

fn scale_hex_color(hex: &str, scaler: f64) -> String {
    let (r, g, b) = parse_hex_color(hex);
    format!("{:02X}{:02X}{:02X}",
        ((r as f64) * scaler) as u8,
        ((g as f64) * scaler) as u8,
        ((b as f64) * scaler) as u8)
}

/// Compute animation color per zone — mirrors Python _software_animation_loop logic.
#[allow(clippy::too_many_arguments)]
fn compute_anim_color(
    mode: &str, step: f64, eff_idx: usize, zone_count: usize,
    r1: u8, g1: u8, b1: u8, r2: u8, g2: u8, b2: u8,
) -> (u8, u8, u8) {
    use std::f64::consts::PI;
    match mode {
        "wave" => {
            let phase = step + (eff_idx as f64 * (2.0 * PI / zone_count as f64));
            let factor = (phase.sin() * 0.5) + 0.5;
            let r = (r1 as f64 * factor + r2 as f64 * (1.0 - factor)) as u8;
            let g = (g1 as f64 * factor + g2 as f64 * (1.0 - factor)) as u8;
            let b = (b1 as f64 * factor + b2 as f64 * (1.0 - factor)) as u8;
            (r, g, b)
        }
        "cycle" => {
            let hue = step;
            let r = ((hue.sin() * 127.0) + 128.0) as u8;
            let g = (((hue + 2.0 * PI / 3.0).sin() * 127.0) + 128.0) as u8;
            let b = (((hue + 4.0 * PI / 3.0).sin() * 127.0) + 128.0) as u8;
            (r, g, b)
        }
        "rainbow" | "wave_rainbow" => {
            let hue = step + (eff_idx as f64 * (2.0 * PI / zone_count as f64));
            let r = ((hue.sin() * 127.0) + 128.0) as u8;
            let g = (((hue + 2.0 * PI / 3.0).sin() * 127.0) + 128.0) as u8;
            let b = (((hue + 4.0 * PI / 3.0).sin() * 127.0) + 128.0) as u8;
            (r, g, b)
        }
        "breathing" | "pulse" => {
            let factor = (step.sin() * 0.5) + 0.5;
            ((r1 as f64 * factor) as u8, (g1 as f64 * factor) as u8, (b1 as f64 * factor) as u8)
        }
        "chase" => {
            let pos = (step * 2.0) as usize % zone_count;
            let factor = if eff_idx == pos { 1.0 } else { 0.15 };
            ((r1 as f64 * factor) as u8, (g1 as f64 * factor) as u8, (b1 as f64 * factor) as u8)
        }
        "sparkle" => {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut h = DefaultHasher::new();
            (eff_idx as u64 + step as u64).hash(&mut h);
            let val = h.finish();
            let factor = if val.is_multiple_of(4) { 0.1 + (val % 90) as f64 / 100.0 } else { 0.2 };
            ((r1 as f64 * factor) as u8, (g1 as f64 * factor) as u8, (b1 as f64 * factor) as u8)
        }
        "candle" => {
            let noise = (step.sin() * 0.3) + ((step * 2.3).sin() * 0.15);
            let factor = (0.6 + noise).clamp(0.3, 1.0);
            ((r1 as f64 * factor) as u8, (g1 as f64 * 0.6 * factor) as u8, (b1 as f64 * 0.2 * factor) as u8)
        }
        "aurora" => {
            let hs = (step * 0.3) + (eff_idx as f64 * 0.5);
            let r = ((hs.sin() * 40.0) + 40.0) as u8;
            let g = (((hs + 1.0).cos() * 100.0) + 120.0) as u8;
            let b = (((hs + 2.0).sin() * 90.0) + 140.0) as u8;
            (r, g, b)
        }
        "disco" => {
            let beat = (step * 1.5) as u64;
            let seed = beat.wrapping_add(eff_idx as u64);
            // Simple LCG pseudo-random
            let lcg = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let r = (lcg >> 56) as u8;
            let g = (lcg >> 48) as u8;
            let b = (lcg >> 40) as u8;
            (r, g, b)
        }
        "gradient" => {
            let blend = ((step + eff_idx as f64 * 0.7).sin() * 0.5) + 0.5;
            let r = (r1 as f64 * (1.0 - blend) + r2 as f64 * blend) as u8;
            let g = (g1 as f64 * (1.0 - blend) + g2 as f64 * blend) as u8;
            let b = (b1 as f64 * (1.0 - blend) + b2 as f64 * blend) as u8;
            (r, g, b)
        }
        _ => (r1, g1, b1),
    }
}

#[cfg(test)]
mod mcu_effect_tests {
    use super::*;

    fn backend(pid: Option<u16>) -> HidPerKeyBackend {
        HidPerKeyBackend { hidraw_paths: vec![], device_pid: pid, lamparray: None }
    }

    #[test]
    fn mcu_effects_only_on_darfon() {
        let c = vec!["00FF00".to_string()];
        assert!(backend(Some(0x54BF)).mcu_effect_for("breathing", 50, "ltr", &c).is_some());
        assert!(backend(None).mcu_effect_for("breathing", 50, "ltr", &c).is_none());
    }

    #[test]
    fn mcu_effect_record_layout() {
        let c = vec!["00FF00".to_string()];
        let b = backend(Some(0x54BF));
        let r = b.mcu_effect_for("raindrop", 100, "ltr", &c).unwrap();
        assert_eq!((r[0], r[3], r[7]), (12, 2, 2));
        // Byte-identical to the OGH capture of 'wave, rainbow, fast, left-to-right'.
        let w = b.mcu_effect_for("wave_rainbow", 100, "ltr", &c).unwrap();
        assert_eq!(&w[..10], &[0x0A, 0x05, 0x04, 0x02, 0x00, 0x03, 0x01, 0x02, 0x00, 0x00]);
        assert!(w[10..].iter().all(|&x| x == 0));
        assert_eq!(&r[24..27], &[0, 255, 0]);
        assert!(b.mcu_effect_for("reactive", 50, "ltr", &c).is_none());
        assert!(b.mcu_effect_for("wave", 50, "ltr", &c).is_none());
    }
}

#[cfg(test)]
mod lightbar_gate_tests {
    use super::lightbar_allowed_for;

    #[test]
    fn known_board_without_light_bar_is_not_written() {
        let caps = crate::capabilities::detect("878A", "", "");
        assert_eq!(caps.product_id, "878A");
        assert!(!lightbar_allowed_for("878A", &caps, "OMEN Laptop 15-ek0xxx"));
    }

    #[test]
    fn unknown_and_light_bar_boards_keep_previous_behaviour() {
        let unknown = crate::capabilities::detect("ZZZZ", "", "");
        assert!(lightbar_allowed_for("ZZZZ", &unknown, "Some HP Laptop"));
        let with_bar = crate::capabilities::detect("8D41", "", "");
        assert!(with_bar.has_light_bar);
        assert!(lightbar_allowed_for("8D41", &with_bar, "OMEN MAX Gaming Laptop 16-ah0xxx"));
        // Known board flagged without a light bar, but in a light-bar product family.
        let mut odd = crate::capabilities::detect("878A", "", "");
        odd.has_light_bar = false;
        assert!(lightbar_allowed_for("878A", &odd, "OMEN Transcend 16"));
    }
}

#[cfg(test)]
mod regression_rgb {
    use super::*;
    #[test]
    fn regression_darfon_static_pages_require_zero_blength() {
        let path=std::env::temp_dir().join(format!("omen-review-hid-{}",std::process::id()));
        std::fs::write(&path,[0u8;64]).unwrap();
        let h=HidPerKeyBackend{hidraw_paths:vec![path.to_string_lossy().to_string()],device_pid:Some(0x54BF),lamparray:None};
        assert!(h.write_darfon_colors(&vec![(255,255,255);104]));
        let packet=std::fs::read(&path).unwrap();let _=std::fs::remove_file(path);
        assert_eq!(&packet[..4],&[0x07,0x02,0x00,0x00],"native static channel pages require BLength=0, despite carrying 60 data bytes");
    }
    #[test]
    fn regression_batch_preserves_eight_zone_payload() {
        let dir=std::env::temp_dir().join(format!("omen-review-zones-{}",std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();std::fs::write(dir.join("all_zones"),"").unwrap();
        let h=RgbHardware{driver_path:Some(dir.to_string_lossy().to_string()),is_new_driver:false,zone_count:8,available:true,board_id:"TEST".into()};
        let colors=(0..8).map(|i|format!("{:06X}",i+1)).collect::<Vec<_>>();
        h.write_zones_batch(&colors);
        let payload=std::fs::read_to_string(dir.join("all_zones")).unwrap();let _=std::fs::remove_dir_all(dir);
        assert_eq!(payload.split_whitespace().count(),8,"adding all_zones must not drop zones 4-7");
    }
}
