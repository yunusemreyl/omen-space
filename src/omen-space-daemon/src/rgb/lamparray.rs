use log::{info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

pub const LAMPARRAY_PREFIX: &[u8] = &[0x05, 0x59, 0x09, 0x01];
pub const DARFON_VENDOR_PREFIX: &[u8] = &[0x06, 0x01, 0xFF, 0x09, 0x01];
pub const PRIMAX_VENDOR_PREFIX: &[u8] = &[0x06, 0x13, 0xFF, 0x09, 0x01];

/// Per-lamp GUI slot overrides for 120-lamp OMEN MAX 16 (8D87 / 8D88 / 8D42 / 8E9A / 8E9F, 0d62:54bf).
const LAMP_SLOT_OVERRIDES_OMEN_MAX: &[(usize, usize)] = &[
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

fn is_omen_max_lamparray_board() -> bool {
    static CACHE: OnceLock<bool> = OnceLock::new();
    *CACHE.get_or_init(|| {
        std::fs::read_to_string("/sys/class/dmi/id/board_name")
            .map(|s| {
                matches!(
                    s.trim().to_ascii_uppercase().as_str(),
                    "8D88" | "8D87" | "8D42" | "8E9A" | "8E9F"
                )
            })
            .unwrap_or(false)
    })
}

#[derive(Debug, Clone, Copy)]
pub struct LampInfo {
    pub id: u16,
    pub x: u32,
    pub y: u32,
    pub binding: u8,
}

#[derive(Debug, Clone)]
pub struct LampArrayBackend {
    pub path: String,
    pub lamps: Vec<LampInfo>,
    pub width_um: u32,
    #[allow(dead_code)]
    pub height_um: u32,
    #[allow(dead_code)]
    pub min_interval_us: u32,
    acquired: Arc<AtomicBool>,
    fd: Arc<Mutex<Option<std::fs::File>>>,
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
        0x29, 0x3A, 0x3B, 0x3C, 0x3D, 0x3E, 0x3F, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x4C, 0x00,
        0x00, 0x66, // row 2
        0x35, 0x1E, 0x1F, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x2D, 0x2E, 0x2A, 0x53,
        0x54, 0x55, 0x56, // row 3
        0x2B, 0x14, 0x1A, 0x08, 0x15, 0x17, 0x1C, 0x18, 0x0C, 0x12, 0x13, 0x2F, 0x30, 0x31, 0x5F,
        0x60, 0x61, 0x57, // row 4
        0x39, 0x04, 0x16, 0x07, 0x09, 0x0A, 0x0B, 0x0D, 0x0E, 0x0F, 0x33, 0x34, 0x28, 0x5C, 0x5D,
        0x5E, // row 5
        0xE1, 0x1D, 0x1B, 0x06, 0x19, 0x05, 0x11, 0x10, 0x36, 0x37, 0x38, 0xE5, 0x52, 0x59, 0x5A,
        0x5B, 0x58, // row 6
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
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "empty report",
            ));
        }
        let mut buf = vec![0u8; size];
        buf[0] = report_id;
        let rc = unsafe {
            libc::ioctl(
                file.as_raw_fd(),
                Self::hidioc(0x07, size),
                buf.as_mut_ptr(),
            )
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
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "empty report",
            ));
        }
        let mut buf = data.to_vec();
        let rc = unsafe {
            libc::ioctl(
                file.as_raw_fd(),
                Self::hidioc(0x06, buf.len()),
                buf.as_mut_ptr(),
            )
        };
        if rc < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn open(&self) -> std::io::Result<std::fs::File> {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.path)
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

    pub fn probe(path: &str) -> Option<Self> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .ok()?;
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
            lamps.push(LampInfo {
                id: lid,
                x,
                y,
                binding,
            });
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
            acquired: Arc::new(AtomicBool::new(false)),
            fd: Arc::new(Mutex::new(None)),
        })
    }

    fn acquire(&self, file: &std::fs::File) {
        let _ = Self::set_feature(file, &[Self::R_CONTROL, 0]);
    }

    pub fn release(&self) {
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

    fn multi_report(
        lamps: &[LampInfo],
        offset: usize,
        chunk: &[(u8, u8, u8)],
        complete: bool,
    ) -> Vec<u8> {
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
            reports.push(Self::multi_report(
                &self.lamps,
                off,
                &cols[off..end],
                end >= n,
            ));
            off = end;
        }
        reports
    }

    pub fn set_frame(&self, colors: &[(u8, u8, u8)]) -> bool {
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
                    warn!(
                        "LampArray {}: report 0x{:02X} failed: {}",
                        self.path, rep[0], e
                    );
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

    pub fn zone_colors(&self, colors: &[(u8, u8, u8)]) -> Vec<(u8, u8, u8)> {
        let width = self.width_um.max(1) as u64;
        self.lamps
            .iter()
            .map(|l| {
                let zone = ((l.x as u64 * 4) / width).min(3) as usize;
                colors.get(zone).copied().unwrap_or((0, 0, 0))
            })
            .collect()
    }

    pub fn key_colors(&self, key_colors: &[(u8, u8, u8)], total_key_count: usize) -> Vec<(u8, u8, u8)> {
        let n = self.lamps.len();
        if key_colors.is_empty() {
            return vec![(0, 0, 0); n];
        }
        if key_colors.len() == n {
            return key_colors.to_vec();
        }
        if key_colors.len() == total_key_count {
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

        if is_omen_max_lamparray_board() && n == 120 {
            for &(lamp, slot) in LAMP_SLOT_OVERRIDES_OMEN_MAX {
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

    pub fn lamp_count(&self) -> usize {
        self.lamps.len()
    }
}
