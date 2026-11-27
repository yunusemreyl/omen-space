use super::hardware::parse_hex_color;
use super::lamparray::{
    LampArrayBackend, DARFON_VENDOR_PREFIX, LAMPARRAY_PREFIX, PRIMAX_VENDOR_PREFIX,
};
use glob::glob;
use log::{info, warn};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
pub struct HidPageCache {
    pub primax: Option<[[u8; 65]; 9]>,
    pub darfon: Option<[[u8; 64]; 9]>,
}

#[derive(Debug, Clone)]
pub struct HidPerKeyBackend {
    pub hidraw_paths: Vec<String>,
    pub device_pid: Option<u16>,
    pub lamparray: Option<LampArrayBackend>,
    pub is_primax_mcu: bool,
    pub primax_mode_active: Arc<AtomicBool>,
    pub cached_pages: Arc<Mutex<HidPageCache>>,
}

impl HidPerKeyBackend {
    const HP_VID: u16 = 0x03F0;
    const DARFON_VID: u16 = 0x0D62;
    const PRIMAX_VID: u16 = 0x0461;
    const HP_KNOWN_PIDS: &'static [u16] = &[0x0538, 0x053A, 0x0547, 0x0549, 0x054E, 0x054F];
    const DARFON_KNOWN_PIDS: &'static [u16] = &[0x54BF, 0x30BF];
    const PRIMAX_KNOWN_PIDS: &'static [u16] = &[0x4E9A, 0x4E9B, 0x4F03, 0x4F11, 0x4F1E, 0x4ECB];
    const PRIMAX_LED_COUNT: usize = 180;
    const PACKET_SIZE: usize = 65;
    const REPORT_ID: u8 = 0x00;
    const CMD_BYTE: u8 = 0x0F;
    const SUB_ENTER_EFFECT: u8 = 0x42;
    const SUB_SET_COLORS: u8 = 0x52;
    const SUB_COMMIT: u8 = 0x50;
    const STATIC_MODE_ID: u8 = 0x03;
    const MCU_OP_SET_EFFECT: u8 = 0x03;
    pub const MCU_RECORD_LEN: usize = 36;
    const KEYS_PER_SEGMENT: usize = 20;
    pub const TOTAL_KEY_COUNT: usize = 104;

    pub fn new() -> Self {
        let (hidraw_paths, device_pid, lamparray_path, is_primax_mcu) = Self::find_devices();
        let lamparray = lamparray_path.as_deref().and_then(LampArrayBackend::probe);
        let backend = Self {
            hidraw_paths,
            device_pid,
            lamparray,
            is_primax_mcu,
            primax_mode_active: Arc::new(AtomicBool::new(false)),
            cached_pages: Arc::new(Mutex::new(HidPageCache::default())),
        };
        if backend.is_available() {
            info!(
                "Initialized HidPerKeyBackend (vendor nodes {:?}, lamparray: {:?}, primax_mcu: {})",
                backend.hidraw_paths,
                backend
                    .lamparray
                    .as_ref()
                    .map(|l| (l.path.clone(), l.lamp_count())),
                backend.is_primax_mcu
            );
            backend.send_enter_per_key_mode(100);
        }
        backend
    }

    pub fn is_primax_cmd_allowed(cmd: u8) -> bool {
        // Strict whitelist matching McuSDK2 safety boundary: never send 0x0A (flash), 0x10 (restore/fw update),
        // 0x0C, or 0x03.
        matches!(cmd, 0x80 | 0x83 | 0x09 | 0x05 | 0x06 | 0x07)
    }

    pub fn build_primax_report(
        cmd: u8,
        param: u8,
        len: u8,
        payload: &[u8],
    ) -> [u8; Self::PACKET_SIZE] {
        let mut r = [0u8; Self::PACKET_SIZE];
        r[0] = Self::REPORT_ID;
        r[1] = cmd;
        r[2] = param;
        r[3] = len;
        r[4] = 0x00;
        let n = payload.len().min(Self::PACKET_SIZE - 5);
        r[5..5 + n].copy_from_slice(&payload[..n]);
        r
    }

    fn probe_primax_mcu(dev_path: &str) -> Option<[u8; 4]> {
        use std::io::{Read, Write};
        use std::os::fd::AsRawFd;

        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(dev_path)
            .ok()?;

        let fd = file.as_raw_fd();
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            if flags >= 0 {
                let _ = libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
            }
        }

        let mut drain = [0u8; 64];
        while file.read(&mut drain).is_ok() {}

        let probe_pkt = Self::build_primax_report(0x80, 0x01, 0, &[]);
        file.write_all(&probe_pkt).ok()?;

        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(250);
        while let Some(rem) = deadline.checked_duration_since(std::time::Instant::now()) {
            let mut pfd = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let timeout_ms = rem.as_millis().min(250) as libc::c_int;
            let ready = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
            if ready <= 0 {
                break;
            }
            let mut resp = [0u8; 64];
            if let Ok(n) = file.read(&mut resp) {
                if n >= 8 && resp[0] == 0x80 && resp[1] == 0x01 {
                    if resp[4] == 0xEC && resp[5] == 0xFA {
                        return None;
                    }
                    return Some([resp[4], resp[5], resp[6], resp[7]]);
                }
            }
        }
        None
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

    fn find_devices() -> (Vec<String>, Option<u16>, Option<String>, bool) {
        let mut vendor_paths = Vec::new();
        let mut all_paths = Vec::new();
        let mut found_pid = None;
        let mut lamparray_path = None;
        let mut is_primax_mcu = false;

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
                                    let is_hp =
                                        vid == Self::HP_VID && Self::HP_KNOWN_PIDS.contains(&pid);
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
                                                info!(
                                                    "Found Darfon HID LampArray (per-key keyboard): {}",
                                                    dev_path
                                                );
                                                lamparray_path = Some(dev_path.clone());
                                            } else if descriptor.starts_with(DARFON_VENDOR_PREFIX) {
                                                info!(
                                                    "Found Darfon MCU vendor interface: {}",
                                                    dev_path
                                                );
                                                vendor_paths.push(dev_path.clone());
                                            } else {
                                                let dev_dir = entry.join("device");
                                                let canonical = std::fs::canonicalize(&dev_dir)
                                                    .unwrap_or(dev_dir);
                                                if canonical.to_string_lossy().contains(":1.3") {
                                                    vendor_paths.push(dev_path.clone());
                                                }
                                            }
                                        } else if is_primax {
                                            let descriptor = std::fs::read(
                                                entry.join("device/report_descriptor"),
                                            )
                                            .unwrap_or_default();
                                            if descriptor.starts_with(LAMPARRAY_PREFIX) {
                                                info!(
                                                    "Found Primax HID LampArray keyboard: {}",
                                                    dev_path
                                                );
                                                found_pid = Some(pid);
                                                lamparray_path = Some(dev_path.clone());
                                            } else if descriptor.starts_with(PRIMAX_VENDOR_PREFIX)
                                                || Self::has_vendor_usage_page(&entry)
                                            {
                                                if let Some(fw) = Self::probe_primax_mcu(&dev_path)
                                                {
                                                    info!(
                                                        "Found Primax McuSDK2 keyboard {} (VID=0461 PID={:04X}, fw={:02X}.{:02X}.{:02X}.{:02X})",
                                                        dev_path, pid, fw[0], fw[1], fw[2], fw[3]
                                                    );
                                                    found_pid = Some(pid);
                                                    is_primax_mcu = true;
                                                    vendor_paths.push(dev_path.clone());
                                                }
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
            (vendor_paths, found_pid, lamparray_path, is_primax_mcu)
        } else {
            all_paths.reverse();
            (all_paths, found_pid, lamparray_path, is_primax_mcu)
        }
    }

    pub fn is_available(&self) -> bool {
        !self.hidraw_paths.is_empty() || self.lamparray.is_some()
    }

    pub fn is_darfon(&self) -> bool {
        self.device_pid == Some(0x54BF)
    }

    pub fn is_primax_mcu(&self) -> bool {
        self.is_primax_mcu
    }

    pub fn is_lamparray(&self) -> bool {
        self.lamparray.is_some()
    }

    pub fn release_lamparray(&self) {
        if let Some(ref la) = self.lamparray {
            la.release();
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        hidraw_paths: Vec<String>,
        device_pid: Option<u16>,
        is_primax_mcu: bool,
    ) -> Self {
        Self {
            hidraw_paths,
            device_pid,
            lamparray: None,
            is_primax_mcu,
            primax_mode_active: Arc::new(AtomicBool::new(false)),
            cached_pages: Arc::new(Mutex::new(HidPageCache::default())),
        }
    }

    pub fn invalidate_cache(&self) {
        self.primax_mode_active.store(false, Ordering::SeqCst);
        if let Ok(mut guard) = self.cached_pages.lock() {
            guard.primax = None;
            guard.darfon = None;
        }
        if let Some(ref la) = self.lamparray {
            la.invalidate_cache();
        }
    }

    #[cfg(test)]
    pub fn invalidate_primax_cache(&self) {
        self.invalidate_cache();
    }

    fn write_primax_report(&self, report: &[u8; Self::PACKET_SIZE]) -> bool {
        if !Self::is_primax_cmd_allowed(report[1]) {
            warn!("Blocked non-whitelisted Primax command 0x{:02X}", report[1]);
            return false;
        }
        self.write_packet(report)
    }

    /// Primax McuSDK2 (0461:4F03/4E9A/4E9B/4F1E/4ECB) static colour write across 3x60 LED pages (180 slots)
    /// using dirty-page diffing and single-open batched HID writes.
    pub fn write_primax_colors(&self, key_colors: &[(u8, u8, u8)]) -> bool {
        let mut guard = match self.cached_pages.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let mut mode_active = self.primax_mode_active.load(Ordering::SeqCst);
        let packets = plan_primax_packets(
            self.device_pid,
            key_colors,
            &mut guard.primax,
            &mut mode_active,
        );
        if packets.is_empty() {
            return true;
        }
        if self.write_packets_batch(&packets) {
            self.primax_mode_active.store(mode_active, Ordering::SeqCst);
            true
        } else {
            self.primax_mode_active.store(false, Ordering::SeqCst);
            guard.primax = None;
            false
        }
    }

    pub fn write_darfon_colors(&self, key_colors: &[(u8, u8, u8)]) -> bool {
        let mut guard = match self.cached_pages.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let mut mode_active = self.primax_mode_active.load(Ordering::SeqCst);
        let packets = plan_darfon_packets(key_colors, &mut guard.darfon, &mut mode_active);
        if packets.is_empty() {
            return true;
        }
        let slices: [&[u8]; 10] =
            std::array::from_fn(|i| packets.get(i).map(|p| p.as_slice()).unwrap_or(&[]));
        if self.write_slices_batch(&slices[..packets.len()]) {
            self.primax_mode_active.store(mode_active, Ordering::SeqCst);
            true
        } else {
            self.primax_mode_active.store(false, Ordering::SeqCst);
            guard.darfon = None;
            false
        }
    }

    fn build_packet(&self, sub_command: u8) -> [u8; Self::PACKET_SIZE] {
        let mut packet = [0u8; Self::PACKET_SIZE];
        packet[0] = Self::REPORT_ID;
        packet[1] = Self::CMD_BYTE;
        packet[2] = sub_command;
        packet
    }

    fn drain_hidraw_input(file: &mut std::fs::File) {
        use std::io::Read;
        let mut drain = [0u8; 64];
        while let Ok(n) = file.read(&mut drain) {
            if n == 0 {
                break;
            }
        }
    }

    fn write_packet(&self, packet: &[u8]) -> bool {
        self.write_slices_batch(&[packet])
    }

    pub fn write_slices_batch(&self, packets: &[&[u8]]) -> bool {
        use std::io::Write;
        use std::os::unix::fs::{FileTypeExt, OpenOptionsExt};

        if packets.is_empty() {
            return true;
        }
        let mut any_ok = false;
        for path in &self.hidraw_paths {
            let open_res = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(path);
            match open_res {
                Ok(mut file) => {
                    let is_char_dev = file
                        .metadata()
                        .map(|m| m.file_type().is_char_device())
                        .unwrap_or(false);
                    if is_char_dev {
                        Self::drain_hidraw_input(&mut file);
                    }
                    let mut all_ok = true;
                    let mut wrote_any = false;
                    for pkt in packets {
                        if self.is_primax_mcu
                            && pkt.len() > 1
                            && !Self::is_primax_cmd_allowed(pkt[1])
                        {
                            warn!("Blocked non-whitelisted Primax command 0x{:02X}", pkt[1]);
                            all_ok = false;
                            break;
                        }
                        if wrote_any && is_char_dev {
                            std::thread::sleep(std::time::Duration::from_micros(150));
                        }
                        if let Err(e) = file.write_all(pkt) {
                            warn!("Failed to write to {}: {}", path, e);
                            all_ok = false;
                            break;
                        }
                        wrote_any = true;
                    }
                    if is_char_dev {
                        Self::drain_hidraw_input(&mut file);
                    }
                    if all_ok {
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

    pub fn write_packets_batch(&self, packets: &[[u8; Self::PACKET_SIZE]]) -> bool {
        if packets.is_empty() {
            return true;
        }
        if packets.len() <= 16 {
            let refs: [&[u8]; 16] =
                std::array::from_fn(|i| packets.get(i).map(|p| p.as_slice()).unwrap_or(&[]));
            self.write_slices_batch(&refs[..packets.len()])
        } else {
            let refs: Vec<&[u8]> = packets.iter().map(|p| p.as_slice()).collect();
            self.write_slices_batch(&refs)
        }
    }

    pub fn send_enter_per_key_mode(&self, brightness: u32) -> bool {
        if self.lamparray.is_some() {
            return true;
        }
        if self.is_primax_mcu {
            let val = if brightness == 0 { 0x00 } else { 0x01 };
            let pkt = Self::build_primax_report(0x09, 0x00, 0x01, &[val]);
            let ok = self.write_primax_report(&pkt);
            if let Ok(mut guard) = self.cached_pages.lock() {
                guard.primax = None;
            }
            self.primax_mode_active
                .store(ok && brightness > 0, Ordering::SeqCst);
            return ok;
        }
        if self.is_darfon() {
            if let Ok(mut guard) = self.cached_pages.lock() {
                guard.darfon = None;
            }
            self.primax_mode_active.store(false, Ordering::SeqCst);
        }
        let mut packet = self.build_packet(Self::SUB_ENTER_EFFECT);
        packet[3] = Self::STATIC_MODE_ID;
        packet[4] = brightness.clamp(0, 100) as u8;
        self.write_packet(&packet)
    }

    pub fn write_per_key_colors(&self, key_colors: &[(u8, u8, u8)]) -> bool {
        if let Some(ref la) = self.lamparray {
            return la.set_gui_frame(key_colors, Self::TOTAL_KEY_COUNT);
        }
        if self.is_primax_mcu {
            return self.write_primax_colors(key_colors);
        }
        if self.is_darfon() {
            return self.write_darfon_colors(key_colors);
        }
        let segment_count = key_colors.len().div_ceil(Self::KEYS_PER_SEGMENT);
        let mut packets = Vec::with_capacity(segment_count + 1);
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

            packets.push(packet);
        }
        packets.push(self.build_packet(Self::SUB_COMMIT));
        self.write_packets_batch(&packets)
    }

    /// Map an OMEN Space mode to the closest built-in effect of the Darfon MCU
    /// (opcode 0x03). Returns `None` for Primax McuSDK2 devices and for modes streamed
    /// from software (`marquee`, `reactive`, `ripple`, `wave`, `blinking`, `per_key_custom`, etc.).
    pub fn mcu_effect_for(
        &self,
        mode: &str,
        speed: u32,
        direction: &str,
        colors: &[String],
    ) -> Option<[u8; Self::MCU_RECORD_LEN]> {
        if self.is_primax_mcu || self.device_pid != Some(0x54BF) {
            return None;
        }
        let parse =
            |i: usize| parse_hex_color(colors.get(i).map(|s| s.as_str()).unwrap_or("FF0000"));
        let led_speed: u8 = match speed {
            0..=33 => 0,
            34..=66 => 1,
            _ => 2,
        };
        let dir: u8 = if direction == "rtl" { 2 } else { 3 };
        let mut rec = [0u8; Self::MCU_RECORD_LEN];
        rec[3] = led_speed;
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
            "wave_rainbow" => {
                rec[0] = 10;
                rec[1] = 5;
                rec[2] = 4;
            }
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

    pub fn send_mcu_effect_with_brightness(
        &self,
        record: &[u8; Self::MCU_RECORD_LEN],
        _brightness: u32,
    ) -> bool {
        if self.is_primax_mcu {
            return false;
        }
        self.release_lamparray();
        self.invalidate_cache();
        let mut on = [0u8; 64];
        on[0] = 0x09;
        on[2] = 1;
        on[4] = 0x01;
        self.write_packet(&on);
        let mut packet = [0u8; 64];
        packet[0] = Self::MCU_OP_SET_EFFECT;
        packet[1] = 0;
        packet[2] = Self::MCU_RECORD_LEN as u8;
        packet[3] = 0;
        packet[4..4 + Self::MCU_RECORD_LEN].copy_from_slice(record);
        self.write_packet(&packet)
    }

    pub fn set_zone_colors(&self, colors_hex: &[String]) -> bool {
        let mut colors: Vec<(u8, u8, u8)> = colors_hex.iter().map(|c| parse_hex_color(c)).collect();
        while colors.len() < 4 {
            colors.push((0, 0, 0));
        }

        if let Some(ref la) = self.lamparray {
            if colors.len() >= Self::TOTAL_KEY_COUNT {
                let key_colors =
                    la.key_colors(&colors[..Self::TOTAL_KEY_COUNT], Self::TOTAL_KEY_COUNT);
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

    pub fn lamp_array_count(&self) -> usize {
        self.lamparray.as_ref().map(|l| l.lamp_count()).unwrap_or(0)
    }

    pub fn lamp_map_json(&self) -> String {
        let Some(ref la) = self.lamparray else {
            return serde_json::json!({"count": 0, "lamps": []}).to_string();
        };
        let lamps: Vec<serde_json::Value> = la
            .lamps
            .iter()
            .enumerate()
            .map(|(index, lamp)| {
                serde_json::json!({
                    "index": index,
                    "id": lamp.id,
                    "x": lamp.x,
                    "y": lamp.y,
                    "binding": lamp.binding,
                })
            })
            .collect();
        serde_json::json!({"count": la.lamp_count(), "lamps": lamps}).to_string()
    }

    pub fn set_lamp_colors(&self, colors: &[(u8, u8, u8)]) -> bool {
        let Some(ref la) = self.lamparray else {
            return false;
        };
        let _ = self.send_enter_per_key_mode(100);
        la.set_frame(colors)
    }

    pub fn test_single_key(&self, key_index: usize, r: u8, g: u8, b: u8) -> bool {
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

pub(crate) fn plan_primax_packets(
    pid: Option<u16>,
    key_colors: &[(u8, u8, u8)],
    cache: &mut Option<[[u8; 65]; 9]>,
    mode_active: &mut bool,
) -> Vec<[u8; 65]> {
    let (r_leds, g_leds, b_leds) = super::primax_map::encode_primax_channels(pid, key_colors);
    let mut pages = [[0u8; 65]; 9];
    let channels = [(0x05u8, &r_leds), (0x06u8, &g_leds), (0x07u8, &b_leds)];
    for (ch_idx, (cmd, leds)) in channels.into_iter().enumerate() {
        for chunk_idx in 0..3 {
            let start = chunk_idx * 60;
            let end = (start + 60).min(HidPerKeyBackend::PRIMAX_LED_COUNT);
            let pkt = HidPerKeyBackend::build_primax_report(
                cmd,
                chunk_idx as u8,
                0x00,
                &leds[start..end],
            );
            pages[ch_idx * 3 + chunk_idx] = pkt;
        }
    }

    let mut out = Vec::with_capacity(10);
    if !*mode_active || cache.is_none() {
        let on_pkt = HidPerKeyBackend::build_primax_report(0x09, 0x00, 0x01, &[0x01]);
        if HidPerKeyBackend::is_primax_cmd_allowed(on_pkt[1]) {
            out.push(on_pkt);
        }
        for pkt in &pages {
            if HidPerKeyBackend::is_primax_cmd_allowed(pkt[1]) {
                out.push(*pkt);
            }
        }
        *mode_active = true;
        *cache = Some(pages);
    } else if let Some(prev) = cache.as_ref() {
        for (i, pkt) in pages.iter().enumerate() {
            if *pkt != prev[i] && HidPerKeyBackend::is_primax_cmd_allowed(pkt[1]) {
                out.push(*pkt);
            }
        }
        *cache = Some(pages);
    }
    out
}

pub(crate) fn plan_darfon_packets(
    key_colors: &[(u8, u8, u8)],
    cache: &mut Option<[[u8; 64]; 9]>,
    mode_active: &mut bool,
) -> Vec<[u8; 64]> {
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

    let mut pages = [[0u8; 64]; 9];
    let channels = [(0x05u8, &r_leds), (0x06u8, &g_leds), (0x07u8, &b_leds)];
    for (ch_idx, (cmd, leds)) in channels.into_iter().enumerate() {
        for chunk_idx in 0..3 {
            let mut pkt = [0u8; 64];
            pkt[0] = cmd;
            pkt[1] = chunk_idx as u8;
            pkt[2] = 0x00;
            pkt[3] = 0x00;
            let start_idx = chunk_idx * 60;
            for led in 0..60 {
                let pos = start_idx + led;
                if pos < 176 {
                    pkt[4 + led] = leds[pos];
                }
            }
            pages[ch_idx * 3 + chunk_idx] = pkt;
        }
    }

    let mut out = Vec::with_capacity(10);
    if !*mode_active || cache.is_none() {
        let mut on_pkt = [0u8; 64];
        on_pkt[0] = 0x09;
        on_pkt[2] = 0x01;
        on_pkt[4] = 0x01;
        out.push(on_pkt);
        for pkt in &pages {
            out.push(*pkt);
        }
        *mode_active = true;
        *cache = Some(pages);
    } else if let Some(prev) = cache.as_ref() {
        for (i, pkt) in pages.iter().enumerate() {
            if *pkt != prev[i] {
                out.push(*pkt);
            }
        }
        *cache = Some(pages);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primax_packet_diffing_minimizes_bus_traffic() {
        let mut cache = None;
        let mut mode_active = false;
        let base_colors = vec![(10u8, 20u8, 30u8); HidPerKeyBackend::TOTAL_KEY_COUNT];

        // 1. First frame: 0x09 enter-per-key + 9 LED pages = 10 packets.
        let first = plan_primax_packets(Some(0x4F03), &base_colors, &mut cache, &mut mode_active);
        assert_eq!(first.len(), 10);
        assert_eq!(first[0][1], 0x09);
        assert!(mode_active);
        assert!(cache.is_some());
        for pkt in &first {
            assert!(HidPerKeyBackend::is_primax_cmd_allowed(pkt[1]));
        }

        // 2. Identical frame: 0 packets.
        let identical =
            plan_primax_packets(Some(0x4F03), &base_colors, &mut cache, &mut mode_active);
        assert_eq!(identical.len(), 0);

        // 3. Single-key reactive change (slot 58 'G' -> LED 84, chunk 1): <= 3 pages, no 0x09.
        let mut one_key = base_colors.clone();
        one_key[58] = (255, 0, 128);
        let diff = plan_primax_packets(Some(0x4F03), &one_key, &mut cache, &mut mode_active);
        assert!(!diff.is_empty() && diff.len() <= 3);
        assert!(diff.iter().all(|pkt| pkt[1] != 0x09));
        for pkt in &diff {
            assert!(HidPerKeyBackend::is_primax_cmd_allowed(pkt[1]));
        }

        // 4. Invalidation forces full 10-packet refresh.
        mode_active = false;
        cache = None;
        let after_inv = plan_primax_packets(Some(0x4F03), &one_key, &mut cache, &mut mode_active);
        assert_eq!(after_inv.len(), 10);
        assert_eq!(after_inv[0][1], 0x09);
    }

    #[test]
    fn write_packets_batch_writes_all_packets_to_temp_file_in_one_open() {
        let path = std::env::temp_dir().join(format!(
            "omen-primax-batch-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&path, []).unwrap();

        let backend = HidPerKeyBackend::for_test(
            vec![path.to_string_lossy().to_string()],
            Some(0x4F03),
            true,
        );

        assert!(backend.write_packets_batch(&[]));

        let batch = vec![
            HidPerKeyBackend::build_primax_report(0x09, 0x00, 0x01, &[0x01]),
            HidPerKeyBackend::build_primax_report(0x05, 0x00, 0x00, &[0xAA, 0xBB]),
            HidPerKeyBackend::build_primax_report(0x07, 0x02, 0x00, &[0xCC]),
        ];
        assert!(backend.write_packets_batch(&batch));

        let bytes = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(bytes.len(), batch.len() * 65);
        assert_eq!(&bytes[0..65], &batch[0][..]);
        assert_eq!(&bytes[65..130], &batch[1][..]);
        assert_eq!(&bytes[130..195], &batch[2][..]);

        // Verify invalidate_primax_cache resets active flag and page cache.
        backend.primax_mode_active.store(true, Ordering::SeqCst);
        backend.cached_pages.lock().unwrap().primax = Some([[0u8; 65]; 9]);
        backend.invalidate_primax_cache();
        assert!(!backend.primax_mode_active.load(Ordering::SeqCst));
        assert!(backend.cached_pages.lock().unwrap().primax.is_none());
    }

    #[test]
    fn darfon_dirty_pages_and_single_open_batch_preserve_zero_blength() {
        let mut cache = None;
        let mut mode_active = false;
        let base_colors = vec![(15u8, 30u8, 45u8); HidPerKeyBackend::TOTAL_KEY_COUNT];

        // 1. Initial frame emits 0x09 + 9 channel pages (10 packets), all pages keep BLength=0.
        let first = plan_darfon_packets(&base_colors, &mut cache, &mut mode_active);
        assert_eq!(first.len(), 10);
        assert_eq!(first[0][0], 0x09);
        assert!(mode_active);
        for pkt in &first[1..] {
            assert_eq!(
                &pkt[2..4],
                &[0x00, 0x00],
                "Darfon pages must preserve BLength=0"
            );
        }

        // 2. Identical frame emits 0 packets.
        let same = plan_darfon_packets(&base_colors, &mut cache, &mut mode_active);
        assert!(same.is_empty());

        // 3. Single-key change emits only dirty channel pages (1..=3), no 0x09.
        let mut changed = base_colors.clone();
        changed[50] = (255, 0, 128);
        let diff = plan_darfon_packets(&changed, &mut cache, &mut mode_active);
        assert!(!diff.is_empty() && diff.len() <= 3);
        assert!(diff
            .iter()
            .all(|pkt| pkt[0] != 0x09 && pkt[2] == 0x00 && pkt[3] == 0x00));

        // 4. Single-open write_darfon_colors writes all 10 packets (640 bytes) in one pass.
        let path = std::env::temp_dir().join(format!(
            "omen-darfon-batch-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&path, []).unwrap();
        let backend = HidPerKeyBackend::for_test(
            vec![path.to_string_lossy().to_string()],
            Some(0x54BF),
            false,
        );
        assert!(backend.write_darfon_colors(&changed));
        let bytes = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(bytes.len(), 640);
        assert_eq!(bytes[0], 0x09);
        assert_eq!(&bytes[640 - 64..640 - 60], &[0x07, 0x02, 0x00, 0x00]);
    }
}
