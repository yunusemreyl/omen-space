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

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LampGridPos {
    pub row: u8,
    pub col: u8,
    pub key_x: f64,
    pub key_y: f64,
}

#[derive(Debug, Default)]
struct LampArrayRuntime {
    file: Option<std::fs::File>,
    last_colors: Vec<(u8, u8, u8)>,
    scratch_colors: Vec<(u8, u8, u8)>,
    scratch_reports: Vec<(usize, [u8; 51])>,
}

#[derive(Debug, Clone)]
pub struct LampArrayBackend {
    pub path: String,
    pub lamps: Arc<[LampInfo]>,
    pub width_um: u32,
    slot_for_lamp: Arc<[usize]>,
    pub(crate) lamp_grid: Arc<[LampGridPos]>,
    pub(crate) grid_cols: usize,
    acquired: Arc<AtomicBool>,
    runtime: Arc<Mutex<LampArrayRuntime>>,
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
        ((3u64 << 30) | ((size as u64) << 16) | ((b'H' as u64) << 8) | (nr as u64)) as libc::c_ulong
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
        let rc =
            unsafe { libc::ioctl(file.as_raw_fd(), Self::hidioc(0x07, size), buf.as_mut_ptr()) };
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
        let rc = if data.len() <= 64 {
            let mut buf = [0u8; 64];
            buf[..data.len()].copy_from_slice(data);
            unsafe {
                libc::ioctl(
                    file.as_raw_fd(),
                    Self::hidioc(0x06, data.len()),
                    buf.as_mut_ptr(),
                )
            }
        } else {
            let mut buf = data.to_vec();
            unsafe {
                libc::ioctl(
                    file.as_raw_fd(),
                    Self::hidioc(0x06, buf.len()),
                    buf.as_mut_ptr(),
                )
            }
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

    pub(crate) fn build_lamp_slot_map(lamps: &[LampInfo], is_omen_max: bool) -> Vec<usize> {
        let n = lamps.len();
        if n == 0 {
            return Vec::new();
        }
        let mut slots = vec![0usize; n];
        let mut mapped = vec![false; n];

        if is_omen_max && n == 120 {
            for &(lamp, slot) in LAMP_SLOT_OVERRIDES_OMEN_MAX {
                if lamp < n && slot < Self::KEY_USAGES.len() {
                    slots[lamp] = slot;
                    mapped[lamp] = true;
                }
            }
        }

        for (i, lamp) in lamps.iter().enumerate() {
            if mapped[i] || lamp.binding == 0 {
                continue;
            }
            if let Some(idx) = Self::KEY_USAGES
                .iter()
                .position(|&usage| usage != 0 && usage == lamp.binding)
            {
                slots[i] = idx;
                mapped[i] = true;
            }
        }

        for i in 0..n {
            if mapped[i] {
                continue;
            }
            let mut best: Option<usize> = None;
            let mut best_d = u64::MAX;
            for (j, other) in lamps.iter().enumerate() {
                if !mapped[j] {
                    continue;
                }
                let dx = lamps[i].x as i64 - other.x as i64;
                let dy = lamps[i].y as i64 - other.y as i64;
                let d = (dx * dx + dy * dy) as u64;
                if d < best_d {
                    best_d = d;
                    best = Some(j);
                }
            }
            slots[i] = best.map(|j| slots[j]).unwrap_or(0);
        }
        slots
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
        let slot_for_lamp = Self::build_lamp_slot_map(&lamps, is_omen_max_lamparray_board());
        let (lamp_grid, grid_cols) = Self::build_lamp_grid(&lamps);
        info!(
            "LampArray on {}: {} lamps ({} cols), {}x{} mm, min_interval={}us",
            path,
            lamps.len(),
            grid_cols,
            width_um / 1000,
            height_um / 1000,
            min_interval_us
        );
        Some(Self {
            path: path.to_string(),
            lamps: Arc::from(lamps),
            width_um,
            slot_for_lamp: Arc::from(slot_for_lamp),
            lamp_grid: Arc::from(lamp_grid),
            grid_cols,
            acquired: Arc::new(AtomicBool::new(false)),
            runtime: Arc::new(Mutex::new(LampArrayRuntime::default())),
        })
    }

    fn acquire(&self, file: &std::fs::File) {
        let _ = Self::set_feature(file, &[Self::R_CONTROL, 0]);
    }

    pub fn release(&self) {
        if let Ok(mut guard) = self.runtime.lock() {
            if guard.file.is_none() {
                if let Ok(file) = self.open() {
                    guard.file = Some(file);
                }
            }
            if let Some(ref file) = guard.file {
                let _ = Self::set_feature(file, &[Self::R_CONTROL, 1]);
            }
            guard.last_colors.clear();
        }
        self.acquired.store(false, Ordering::Relaxed);
    }

    pub fn invalidate_cache(&self) {
        if let Ok(mut guard) = self.runtime.lock() {
            guard.last_colors.clear();
        }
    }

    fn range_report(start: u16, end: u16, color: (u8, u8, u8)) -> [u8; 51] {
        let mut rep = [0u8; 51];
        rep[0] = Self::R_RANGE;
        rep[1] = Self::FLAG_COMPLETE;
        rep[2] = (start & 0xFF) as u8;
        rep[3] = (start >> 8) as u8;
        rep[4] = (end & 0xFF) as u8;
        rep[5] = (end >> 8) as u8;
        rep[6] = color.0;
        rep[7] = color.1;
        rep[8] = color.2;
        rep[9] = 255;
        rep
    }

    fn pack_multi_report(entries: &[(u16, (u8, u8, u8))], complete: bool) -> (usize, [u8; 51]) {
        let mut rep = [0u8; 51];
        rep[0] = Self::R_MULTI;
        rep[1] = entries.len() as u8;
        rep[2] = if complete { Self::FLAG_COMPLETE } else { 0 };
        for (k, &(id, (r, g, b))) in entries.iter().enumerate() {
            let id_p = 3 + k * 2;
            rep[id_p] = (id & 0xFF) as u8;
            rep[id_p + 1] = (id >> 8) as u8;
            let c_p = 19 + k * 4;
            rep[c_p] = r;
            rep[c_p + 1] = g;
            rep[c_p + 2] = b;
            rep[c_p + 3] = 255;
        }
        (51, rep)
    }

    /// Diff `colors` against `prev`, writing dirty HID reports into `out` and updating
    /// `prev` in-place so steady-state frames perform zero heap allocations once warmed up.
    pub(crate) fn diff_frame_reports_into(
        lamps: &[LampInfo],
        prev: &mut Vec<(u8, u8, u8)>,
        colors: &[(u8, u8, u8)],
        out: &mut Vec<(usize, [u8; 51])>,
    ) {
        out.clear();
        let n = lamps.len();
        if n == 0 {
            return;
        }
        let copy_len = colors.len().min(n);
        if prev.len() == n
            && prev[..copy_len] == colors[..copy_len]
            && prev[copy_len..].iter().all(|&c| c == (0, 0, 0))
        {
            return;
        }

        let first = colors.first().copied().unwrap_or((0, 0, 0));
        let all_uniform = (0..n).all(|i| colors.get(i).copied().unwrap_or((0, 0, 0)) == first);
        if all_uniform {
            prev.clear();
            prev.resize(n, first);
            out.push((10, Self::range_report(0, (n - 1) as u16, first)));
            return;
        }

        let mut batch = [(0u16, (0u8, 0u8, 0u8)); Self::LAMPS_PER_MULTI];
        let mut batch_len = 0usize;
        if prev.len() != n {
            prev.clear();
            prev.extend_from_slice(&colors[..copy_len]);
            prev.resize(n, (0, 0, 0));
            for (i, lamp) in lamps.iter().enumerate() {
                batch[batch_len] = (lamp.id, prev[i]);
                batch_len += 1;
                if batch_len == Self::LAMPS_PER_MULTI {
                    out.push(Self::pack_multi_report(&batch[..batch_len], false));
                    batch_len = 0;
                }
            }
        } else {
            for (i, lamp) in lamps.iter().enumerate() {
                let c = colors.get(i).copied().unwrap_or((0, 0, 0));
                if prev[i] == c {
                    continue;
                }
                prev[i] = c;
                batch[batch_len] = (lamp.id, c);
                batch_len += 1;
                if batch_len == Self::LAMPS_PER_MULTI {
                    out.push(Self::pack_multi_report(&batch[..batch_len], false));
                    batch_len = 0;
                }
            }
        }
        if batch_len > 0 {
            out.push(Self::pack_multi_report(&batch[..batch_len], false));
        }
        if let Some((_, last)) = out.last_mut() {
            last[2] = Self::FLAG_COMPLETE;
        }
    }

    #[cfg(test)]
    pub(crate) fn diff_frame_reports(
        lamps: &[LampInfo],
        prev: &mut Vec<(u8, u8, u8)>,
        colors: &[(u8, u8, u8)],
    ) -> Vec<(usize, [u8; 51])> {
        let mut out = Vec::new();
        Self::diff_frame_reports_into(lamps, prev, colors, &mut out);
        out
    }

    pub(crate) fn fill_gui_colors_into(
        lamp_count: usize,
        slot_for_lamp: &[usize],
        key_colors: &[(u8, u8, u8)],
        total_key_count: usize,
        out: &mut Vec<(u8, u8, u8)>,
    ) {
        out.clear();
        if lamp_count == 0 {
            return;
        }
        if key_colors.is_empty() {
            out.resize(lamp_count, (0, 0, 0));
        } else if key_colors.len() == lamp_count {
            out.extend_from_slice(key_colors);
        } else if key_colors.len() == total_key_count {
            let fallback = key_colors.first().copied().unwrap_or((0, 0, 0));
            out.extend(
                slot_for_lamp
                    .iter()
                    .take(lamp_count)
                    .map(|&slot| key_colors.get(slot).copied().unwrap_or(fallback)),
            );
            out.resize(lamp_count, fallback);
        } else {
            out.extend((0..lamp_count).map(|i| {
                let k = (i * key_colors.len()) / lamp_count;
                key_colors[k.min(key_colors.len() - 1)]
            }));
        }
    }

    fn send_runtime_reports(&self, guard: &mut LampArrayRuntime) -> bool {
        if guard.scratch_reports.is_empty() {
            return true;
        }
        if guard.file.is_none() {
            match self.open() {
                Ok(file) => guard.file = Some(file),
                Err(e) => {
                    warn!("LampArray {}: cannot open: {}", self.path, e);
                    guard.last_colors.clear();
                    return false;
                }
            }
        }
        let Some(file) = guard.file.as_ref() else {
            return false;
        };
        if !self.acquired.load(Ordering::Relaxed) {
            self.acquire(file);
            self.acquired.store(true, Ordering::Relaxed);
        }
        for (len, rep) in &guard.scratch_reports {
            if let Err(e) = Self::set_feature(file, &rep[..*len]) {
                warn!(
                    "LampArray {}: report 0x{:02X} failed: {}",
                    self.path, rep[0], e
                );
                self.acquired.store(false, Ordering::Relaxed);
                guard.file = None;
                guard.last_colors.clear();
                return false;
            }
        }
        true
    }

    pub fn set_frame(&self, colors: &[(u8, u8, u8)]) -> bool {
        if self.lamps.is_empty() {
            return false;
        }
        let mut guard = match self.runtime.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let LampArrayRuntime {
            ref mut last_colors,
            ref mut scratch_reports,
            ..
        } = *guard;
        Self::diff_frame_reports_into(&self.lamps, last_colors, colors, scratch_reports);
        self.send_runtime_reports(&mut guard)
    }

    pub fn set_gui_frame(&self, key_colors: &[(u8, u8, u8)], total_key_count: usize) -> bool {
        let n = self.lamps.len();
        if n == 0 {
            return false;
        }
        let mut guard = match self.runtime.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let LampArrayRuntime {
            ref mut last_colors,
            ref mut scratch_colors,
            ref mut scratch_reports,
            ..
        } = *guard;
        Self::fill_gui_colors_into(
            n,
            &self.slot_for_lamp,
            key_colors,
            total_key_count,
            scratch_colors,
        );
        Self::diff_frame_reports_into(&self.lamps, last_colors, scratch_colors, scratch_reports);
        self.send_runtime_reports(&mut guard)
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

    pub fn key_colors(
        &self,
        key_colors: &[(u8, u8, u8)],
        total_key_count: usize,
    ) -> Vec<(u8, u8, u8)> {
        let mut out = Vec::with_capacity(self.lamps.len());
        Self::fill_gui_colors_into(
            self.lamps.len(),
            &self.slot_for_lamp,
            key_colors,
            total_key_count,
            &mut out,
        );
        out
    }

    pub(crate) fn build_lamp_grid(lamps: &[LampInfo]) -> (Vec<LampGridPos>, usize) {
        let n = lamps.len();
        if n == 0 {
            return (Vec::new(), 0);
        }
        let mut min_x = u32::MAX;
        let mut max_x = 0u32;
        let mut min_y = u32::MAX;
        let mut max_y = 0u32;
        for lamp in lamps {
            min_x = min_x.min(lamp.x);
            max_x = max_x.max(lamp.x);
            min_y = min_y.min(lamp.y);
            max_y = max_y.max(lamp.y);
        }
        let span_x = (max_x - min_x).max(1) as f64;
        let span_y = (max_y - min_y).max(1) as f64;

        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by_key(|&i| (lamps[i].y, lamps[i].x, lamps[i].id));

        let mut rows: Vec<Vec<usize>> = Vec::new();
        let mut row_anchor_y = 0u32;
        for idx in order {
            let y = lamps[idx].y;
            if rows.is_empty() || y.abs_diff(row_anchor_y) > 9_000 {
                row_anchor_y = y;
                rows.push(vec![idx]);
            } else if let Some(last) = rows.last_mut() {
                last.push(idx);
            }
        }
        for row in &mut rows {
            row.sort_by_key(|&i| (lamps[i].x, lamps[i].id));
        }
        let grid_cols = rows.iter().map(|r| r.len()).max().unwrap_or(1).max(1);

        let mut grid = vec![
            LampGridPos {
                row: 0,
                col: 0,
                key_x: 0.5,
                key_y: 0.0,
            };
            n
        ];
        for (row_idx, row) in rows.iter().enumerate() {
            for (pos_in_row, &lamp_idx) in row.iter().enumerate() {
                let col = if row.len() == grid_cols {
                    pos_in_row
                } else if grid_cols <= 1 {
                    0
                } else {
                    (((lamps[lamp_idx].x.saturating_sub(min_x) as f64) / span_x)
                        * (grid_cols - 1) as f64)
                        .round() as usize
                };
                let key_x =
                    0.5 + ((lamps[lamp_idx].x.saturating_sub(min_x) as f64) / span_x) * 18.0;
                let key_y = ((lamps[lamp_idx].y.saturating_sub(min_y) as f64) / span_y) * 5.0;
                grid[lamp_idx] = LampGridPos {
                    row: row_idx.min(255) as u8,
                    col: col.min(255) as u8,
                    key_x,
                    key_y,
                };
            }
        }
        (grid, grid_cols)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_grid_into(
        grid: &[LampGridPos],
        grid_cols: usize,
        mode: &str,
        step: f64,
        speed: u32,
        direction: &str,
        primary: (u8, u8, u8),
        secondary: (u8, u8, u8),
        scaler: f64,
        recent_keys: &[(f64, f64, f64)],
        out: &mut [(u8, u8, u8)],
    ) -> usize {
        let n = grid.len().min(out.len());
        for (i, (gp, slot)) in grid.iter().zip(out.iter_mut()).take(n).enumerate() {
            *slot = super::effects::compute_point_effect_rgb(
                mode,
                step,
                speed,
                direction,
                i,
                gp.col as usize,
                grid_cols,
                gp.key_x,
                gp.key_y,
                primary,
                secondary,
                scaler,
                recent_keys,
            );
        }
        n
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render_effect_slice(
        &self,
        mode: &str,
        step: f64,
        speed: u32,
        direction: &str,
        primary: (u8, u8, u8),
        secondary: (u8, u8, u8),
        scaler: f64,
        recent_keys: &[(f64, f64, f64)],
        out: &mut [(u8, u8, u8)],
    ) -> usize {
        Self::render_grid_into(
            &self.lamp_grid,
            self.grid_cols,
            mode,
            step,
            speed,
            direction,
            primary,
            secondary,
            scaler,
            recent_keys,
            out,
        )
    }

    pub fn lamp_count(&self) -> usize {
        self.lamps.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_lamp(id: u16, x: u32, y: u32, binding: u8) -> LampInfo {
        LampInfo { id, x, y, binding }
    }

    #[test]
    fn precomputed_slot_map_handles_overrides_bindings_and_nearest_neighbor() {
        // 1. Binding + nearest-neighbor fallback
        let lamps = vec![
            mk_lamp(0, 0, 0, 0x29),    // Esc -> slot 0
            mk_lamp(1, 100, 0, 0x00),  // Unbound near Esc -> nearest is Esc (slot 0)
            mk_lamp(2, 1000, 0, 0x3A), // F1 -> slot 1
            mk_lamp(3, 950, 10, 0x00), // Unbound near F1 -> nearest is F1 (slot 1)
        ];
        let map = LampArrayBackend::build_lamp_slot_map(&lamps, false);
        assert_eq!(map, vec![0, 0, 1, 1]);

        // 2. OMEN MAX 120-lamp override
        let omen_lamps: Vec<LampInfo> = (0..120)
            .map(|i| mk_lamp(i as u16, i as u32 * 10, 0, 0x29))
            .collect();
        let omen_map = LampArrayBackend::build_lamp_slot_map(&omen_lamps, true);
        assert_eq!(omen_map[119], 85); // Keypad Enter override
        assert_eq!(omen_map[16], 14); // Omen key override
        assert_eq!(omen_map[101], 87); // Fn key override
    }

    #[test]
    fn diff_frame_reports_skips_unchanged_uses_range_for_uniform_and_packs_dirty_lamps() {
        let lamps: Vec<LampInfo> = (0..10)
            .map(|i| mk_lamp(100 + i as u16, i as u32 * 10, 0, 0))
            .collect();
        let mut prev = Vec::new();

        // 1. Uniform frame uses single R_RANGE report (len 10)
        let uniform = vec![(10, 20, 30); 10];
        let r1 = LampArrayBackend::diff_frame_reports(&lamps, &mut prev, &uniform);
        assert_eq!(r1.len(), 1);
        assert_eq!(r1[0].0, 10);
        assert_eq!(r1[0].1[0], LampArrayBackend::R_RANGE);
        assert_eq!(&r1[0].1[6..9], &[10, 20, 30]);

        // 2. Identical frame emits 0 reports
        let r2 = LampArrayBackend::diff_frame_reports(&lamps, &mut prev, &uniform);
        assert!(r2.is_empty());

        // 3. Single dirty lamp emits 1 R_MULTI report with count=1 and FLAG_COMPLETE
        let mut one_dirty = uniform.clone();
        one_dirty[4] = (255, 0, 128);
        let r3 = LampArrayBackend::diff_frame_reports(&lamps, &mut prev, &one_dirty);
        assert_eq!(r3.len(), 1);
        assert_eq!(r3[0].0, 51);
        assert_eq!(r3[0].1[0], LampArrayBackend::R_MULTI);
        assert_eq!(r3[0].1[1], 1);
        assert_eq!(r3[0].1[2], LampArrayBackend::FLAG_COMPLETE);
        assert_eq!(u16::from_le_bytes([r3[0].1[3], r3[0].1[4]]), 104);
        assert_eq!(&r3[0].1[19..22], &[255, 0, 128]);

        // 4. 9 dirty lamps split into 2 R_MULTI reports (8 + 1), FLAG_COMPLETE only on final report
        let mut nine_dirty = one_dirty.clone();
        for (idx, c) in nine_dirty.iter_mut().enumerate().take(9) {
            *c = (idx as u8 + 1, 50, 60);
        }
        let r4 = LampArrayBackend::diff_frame_reports(&lamps, &mut prev, &nine_dirty);
        assert_eq!(r4.len(), 2);
        assert_eq!(r4[0].1[1], 8);
        assert_eq!(r4[0].1[2], 0);
        assert_eq!(r4[1].1[1], 1);
        assert_eq!(r4[1].1[2], LampArrayBackend::FLAG_COMPLETE);

        // 5. Cache reset (prev cleared) forces full multi-report on non-uniform frame
        prev.clear();
        let r5 = LampArrayBackend::diff_frame_reports(&lamps, &mut prev, &nine_dirty);
        assert_eq!(r5.len(), 2);
        assert_eq!(r5[0].1[1], 8);
        assert_eq!(r5[1].1[1], 2);
        assert_eq!(r5[1].1[2], LampArrayBackend::FLAG_COMPLETE);
    }

    #[test]
    fn resource_set_gui_frame_reuses_scratch_buffers_with_zero_reallocations() {
        let lamps: Vec<LampInfo> = (0..120)
            .map(|i| {
                mk_lamp(
                    i as u16,
                    (i % 20) as u32 * 1000,
                    (i / 20) as u32 * 1000,
                    0x29,
                )
            })
            .collect();
        let slot_map = LampArrayBackend::build_lamp_slot_map(&lamps, true);
        let mut prev = Vec::new();
        let mut scratch_colors = Vec::new();
        let mut scratch_reports = Vec::new();
        let mut key_frame = [(0u8, 0u8, 0u8); 104];

        // Warm-up frame establishes capacities.
        LampArrayBackend::fill_gui_colors_into(
            lamps.len(),
            &slot_map,
            &key_frame,
            104,
            &mut scratch_colors,
        );
        LampArrayBackend::diff_frame_reports_into(
            &lamps,
            &mut prev,
            &scratch_colors,
            &mut scratch_reports,
        );
        // Reserve non-uniform max report capacity once on first non-uniform frame.
        for (i, slot) in key_frame.iter_mut().enumerate() {
            *slot = ((i * 2) as u8, 10, 20);
        }
        LampArrayBackend::fill_gui_colors_into(
            lamps.len(),
            &slot_map,
            &key_frame,
            104,
            &mut scratch_colors,
        );
        LampArrayBackend::diff_frame_reports_into(
            &lamps,
            &mut prev,
            &scratch_colors,
            &mut scratch_reports,
        );

        let cap_prev = prev.capacity();
        let cap_colors = scratch_colors.capacity();
        let cap_reports = scratch_reports.capacity();

        let mut prev_uniform = false;
        for step in 0..500usize {
            if step % 3 == 0 && !prev_uniform {
                // Unchanged frame must emit 0 reports.
                LampArrayBackend::fill_gui_colors_into(
                    lamps.len(),
                    &slot_map,
                    &key_frame,
                    104,
                    &mut scratch_colors,
                );
                LampArrayBackend::diff_frame_reports_into(
                    &lamps,
                    &mut prev,
                    &scratch_colors,
                    &mut scratch_reports,
                );
                assert!(scratch_reports.is_empty());
            } else if step % 5 == 0 {
                // Uniform frame emits 1 R_RANGE report.
                let uniform = [(55u8, 66u8, (step & 0xFF) as u8); 104];
                LampArrayBackend::fill_gui_colors_into(
                    lamps.len(),
                    &slot_map,
                    &uniform,
                    104,
                    &mut scratch_colors,
                );
                LampArrayBackend::diff_frame_reports_into(
                    &lamps,
                    &mut prev,
                    &scratch_colors,
                    &mut scratch_reports,
                );
                assert_eq!(scratch_reports.len(), 1);
                assert_eq!(scratch_reports[0].1[0], LampArrayBackend::R_RANGE);
                prev_uniform = true;
            } else {
                for (i, slot) in key_frame.iter_mut().enumerate() {
                    *slot = (((i + step) % 255) as u8, ((step * 3) % 255) as u8, 90);
                }
                LampArrayBackend::fill_gui_colors_into(
                    lamps.len(),
                    &slot_map,
                    &key_frame,
                    104,
                    &mut scratch_colors,
                );
                LampArrayBackend::diff_frame_reports_into(
                    &lamps,
                    &mut prev,
                    &scratch_colors,
                    &mut scratch_reports,
                );
                prev_uniform = false;
            }

            assert_eq!(prev.capacity(), cap_prev, "prev reallocated on step {step}");
            assert_eq!(
                scratch_colors.capacity(),
                cap_colors,
                "scratch_colors reallocated on step {step}"
            );
            assert_eq!(
                scratch_reports.capacity(),
                cap_reports,
                "scratch_reports reallocated on step {step}"
            );
        }
    }

    #[test]
    fn omen_max_8d88_120_lamp_grid_lights_all_20_columns_with_zero_gaps() {
        // Exact 8D88 GetLampMap layout: 6 rows at y = [9000, 25000, 44000, 62000, 81000, 99000],
        // 20 lamps per row spanning x = 12000..=328000 micrometres.
        let ys = [9_000u32, 25_000, 44_000, 62_000, 81_000, 99_000];
        let mut lamps = Vec::with_capacity(120);
        let mut id = 0u16;
        for &y in &ys {
            for c in 0..20u32 {
                let x = 12_000 + (c * (328_000 - 12_000)) / 19;
                lamps.push(mk_lamp(id, x, y, 0));
                id += 1;
            }
        }

        let (grid, grid_cols) = LampArrayBackend::build_lamp_grid(&lamps);
        assert_eq!(grid.len(), 120);
        assert_eq!(grid_cols, 20);
        for row in 0..6usize {
            let cols: Vec<u8> = grid[row * 20..(row + 1) * 20]
                .iter()
                .map(|g| g.col)
                .collect();
            let expected: Vec<u8> = (0..20).collect();
            assert_eq!(
                cols, expected,
                "row {row} must map to all 20 columns 0..=19"
            );
        }

        // Sweep marquee across the full grid and ensure every single one of the 120 lamps hits peak brightness.
        let mut buf = [(0u8, 0u8, 0u8); 120];
        let mut lit = [false; 120];
        for tick in 0..40 {
            let step = tick as f64 * 0.125;
            let written = LampArrayBackend::render_grid_into(
                &grid,
                grid_cols,
                "marquee",
                step,
                50,
                "ltr",
                (255, 0, 0),
                (0, 0, 0),
                1.0,
                &[],
                &mut buf,
            );
            assert_eq!(written, 120);
            for (i, &(r, _, _)) in buf.iter().enumerate() {
                if r >= 200 {
                    lit[i] = true;
                }
            }
        }
        assert!(
            lit.iter().all(|&v| v),
            "every lamp across all 6 rows and 20 columns must light during sweep"
        );
    }
}
