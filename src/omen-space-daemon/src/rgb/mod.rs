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
    "static", "breathing", "wave", "cycle", "rainbow",
    "pulse", "chase", "sparkle", "candle", "aurora", "disco", "gradient",
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
            if let Some(n) = zone_override {
                if n == 4 || n == 8 {
                    hw.zone_count = n;
                }
            }
        }
        hw
    }

    fn detect_raw() -> Self {
        let new_path = "/sys/devices/platform/omen-rgb-keyboard/rgb_zones";
        let custom_path = "/sys/devices/platform/hp-omen-extra";
        let hp_path2 = "/sys/devices/platform/hp_omen_extra";
        let board_id = std::fs::read_to_string("/sys/class/dmi/id/board_name").unwrap_or_default();
        let prod_name = std::fs::read_to_string("/sys/class/dmi/id/product_name").unwrap_or_default();
        let specs = crate::sysmon::get_hardware_specs();
        let caps = crate::capabilities::detect(board_id.trim(), prod_name.trim(), &specs.cpu_spec);
        let max_zones = if caps.has_four_zone_rgb { 4 } else { 8 };

        if Path::new(new_path).exists() {
            // Count zones for new driver (8 for per-key models, else 4)
            let mut zone_count = if Path::new("/sys/devices/platform/omen-rgb-keyboard/rgb_zones/zone04").exists() { 8 } else { 4 };
            if zone_count > max_zones { zone_count = max_zones; }
            return Self { driver_path: Some(new_path.to_string()), is_new_driver: true, zone_count, available: true };
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
                let mut zone_count = if caps.has_four_zone_rgb {
                    4
                } else if Path::new(&format!("{}/zone4", p)).exists()
                    || Path::new(&format!("{}/zone04", p)).exists() { 8 } else { 4 };
                if zone_count > max_zones { zone_count = max_zones; }
                return Self { driver_path: Some(p.to_string()), is_new_driver: false, zone_count, available: true };
            }
        }
        // Try keyboard brightness LEDs as fallback
        if let Ok(mut entries) = glob("/sys/class/leds/hp::kbd_backlight*") {
            if let Some(Ok(path)) = entries.next() {
                return Self { driver_path: Some(path.to_string_lossy().to_string()), is_new_driver: false, zone_count: 4, available: true };
            }
        }
        Self { driver_path: None, is_new_driver: false, zone_count: 4, available: false }
    }

    fn write_zone(&self, zone: usize, hex_color: &str) {
        let Some(ref base) = self.driver_path else { return; };
        if zone > 7 { return; }

        // Hardware zone remap differs by driver generation — the physical
        // wiring order isn't the same between the new omen-rgb-keyboard
        // driver and the legacy hp-omen-extra/hp_omen_extra driver, so
        // each needs its own verified table rather than sharing one.
        let actual_zone = if self.is_new_driver && self.zone_count == 4 {
            // omen-rgb-keyboard, 4-zone: the driver only exposes zone00–zone03
            // (zone_count is 4 precisely because zone04 does not exist), so the
            // WASD zone lives in zone03 — a write to zone07 would silently no-op.
            match zone {
                0 => 2, // Left
                1 => 1, // Middle
                2 => 0, // Right
                _ => 3, // WASD
            }
        } else if !self.is_new_driver && self.zone_count == 4 {
            // hp-omen-extra/hp_omen_extra, 4-zone: empirically verified
            // against real hardware (hardware zone0=Right, zone1=Middle,
            // zone2=Left, zone3=WASD) — upstream's 4af2522 dropped this
            // remap for the legacy driver entirely, which un-inverted
            // zones for boards whose raw wiring isn't already sequential.
            match zone {
                0 => 2, // Software Left   -> Hardware Left (2)
                1 => 1, // Software Middle -> Hardware Middle (1)
                2 => 0, // Software Right  -> Hardware Right (0)
                3 => 3, // Software WASD   -> Hardware WASD (3)
                z => z,
            }
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

    fn write_mode(&self, mode: &str, speed: u32) {
        if !self.is_new_driver { return; }
        let Some(ref base) = self.driver_path else { return; };
        let hw_mode = if mode == "cycle" { "rainbow" } else { mode };
        let _ = std::fs::write(format!("{}/animation_mode", base), hw_mode);
        let mapped_speed = (speed / 10).clamp(1, 10);
        let _ = std::fs::write(format!("{}/animation_speed", base), mapped_speed.to_string());
    }
}

// ── HID Per-Key Backend ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct HidPerKeyBackend {
    hidraw_path: Option<String>,
    device_pid: Option<u16>,
}

impl HidPerKeyBackend {
    const HP_VID: u16 = 0x03F0;
    /// Darfon is HP's keyboard ODM — present on OMEN MAX 16 (board 8D87) internal keyboard.
    const DARFON_VID: u16 = 0x0D62;
    /// Known per-key PIDs for HP VID (OpenRGB + OmenCore confirmed)
    const HP_KNOWN_PIDS: &'static [u16] = &[0x0538, 0x053A, 0x0547, 0x0549, 0x054E, 0x054F];
    /// Known per-key PIDs for Darfon VID
    const DARFON_KNOWN_PIDS: &'static [u16] = &[0x54BF];
    const PACKET_SIZE: usize = 65;
    const REPORT_ID: u8 = 0x00;
    const CMD_BYTE: u8 = 0x0F;
    const SUB_ENTER_EFFECT: u8 = 0x42;
    const SUB_SET_COLORS: u8 = 0x52;
    const SUB_COMMIT: u8 = 0x50;
    const STATIC_MODE_ID: u8 = 0x03;
    const KEYS_PER_SEGMENT: usize = 20;
    const TOTAL_KEY_COUNT: usize = 104;

    fn new() -> Self {
        let (hidraw_path, device_pid) = Self::find_device();
        let backend = Self { hidraw_path, device_pid };
        if backend.is_available() {
            info!("Initialized HidPerKeyBackend on {:?}", backend.hidraw_path);
            backend.send_enter_per_key_mode(100);
        }
        backend
    }

    fn find_device() -> (Option<String>, Option<u16>) {
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
                                    if is_hp || is_darfon {
                                        info!(
                                            "Found HP Per-Key RGB device at {:?} VID={:04X} PID={:04X}",
                                            entry, vid, pid
                                        );
                                        let dev_path = format!(
                                            "/dev/{}",
                                            entry.file_name().unwrap_or_default().to_string_lossy()
                                        );
                                        return (Some(dev_path), Some(pid));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        (None, None)
    }

    fn is_available(&self) -> bool {
        self.hidraw_path.is_some()
    }

    fn build_packet(&self, sub_command: u8) -> [u8; Self::PACKET_SIZE] {
        let mut packet = [0u8; Self::PACKET_SIZE];
        packet[0] = Self::REPORT_ID;
        packet[1] = Self::CMD_BYTE;
        packet[2] = sub_command;
        packet
    }

    fn write_packet(&self, packet: &[u8]) -> bool {
        if let Some(ref path) = self.hidraw_path {
            match std::fs::OpenOptions::new().write(true).open(path) {
                Ok(mut file) => {
                    use std::io::Write;
                    if let Err(e) = file.write_all(packet) {
                        warn!("Failed to write to {}: {}", path, e);
                        return false;
                    }
                    return true;
                }
                Err(e) => {
                    warn!("Failed to open {} for writing: {}", path, e);
                    return false;
                }
            }
        }
        false
    }

    fn send_enter_per_key_mode(&self, brightness: u32) -> bool {
        let mut packet = self.build_packet(Self::SUB_ENTER_EFFECT);
        packet[3] = Self::STATIC_MODE_ID;
        packet[4] = brightness.clamp(0, 100) as u8;
        self.write_packet(&packet)
    }

    fn send_commit(&self) -> bool {
        self.write_packet(&self.build_packet(Self::SUB_COMMIT))
    }

    fn write_per_key_colors(&self, key_colors: &[(u8, u8, u8)]) -> bool {
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
}

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
            hw, hid_per_key, config, per_key_map, color_cache: HashMap::new(), anim_step: 0.0, wizard, evdev_monitor, desktop_rgb,
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

    async fn software_animation_loop(inner: Arc<Mutex<RgbInner>>) {
        let mut last_per_key_colors: Vec<String> = Vec::new();
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

            let step_inc = (speed as f64 / 100.0) * 0.25;
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
                    g.hid_per_key.set_zone_colors(&out_colors);
                    last_per_key_colors = out_colors;
                }
            }

            if has_old_sysfs {
                let zone_count = g.hw.zone_count as usize;
                for i in 0..zone_count {
                    let eff_idx = if direction == "ltr" { i } else { zone_count - 1 - i };
                    let (r, g_c, b) = compute_anim_color(&mode, step, eff_idx, zone_count, r1, g1, b1, r2, g2, b2);
                    let scaled = format!(
                        "{:02X}{:02X}{:02X}",
                        ((r as f64) * scaler) as u8,
                        ((g_c as f64) * scaler) as u8,
                        ((b as f64) * scaler) as u8
                    );
                    if g.color_cache.get(&i) != Some(&scaled) {
                        g.hw.write_zone(i, &scaled);
                        g.color_cache.insert(i, scaled);
                    }
                }
            }

            drop(g);
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
    }
    async fn apply_state(inner: Arc<Mutex<RgbInner>>) {
        let mut g = inner.lock().await;

        // detect() is only run once at startup and cached in `hw`. If the kernel
        // module wasn't loaded (or its sysfs wasn't ready) at that moment, the
        // daemon caches available=false forever and every RGB change silently
        // no-ops while still returning "OK" and saving the config. Re-probe when
        // the cached state is stale so a late/missing driver is picked up (and a
        // disappeared driver is re-detected when it comes back).
        if !g.hw.available {
            g.hw = RgbHardware::detect();
        } else if let Some(ref path) = g.hw.driver_path {
            if !Path::new(path).exists() {
                g.hw = RgbHardware::detect();
            }
        }

        let power     = g.config.power;
        let brightness = g.config.brightness;
        let mode      = g.config.mode.clone();
        let speed     = g.config.speed;
        let colors    = g.config.colors.clone();

        // ── HID per-key backend (USB direct) ────────────────────────────────────
        // Runs regardless of whether the sysfs driver is present, since HID and
        // sysfs are independent hardware paths (USB vendor vs. kernel LED class).
        if g.hid_per_key.is_available() {
            if !power || brightness == 0 {
                g.hid_per_key.send_enter_per_key_mode(0);
                g.hid_per_key.set_zone_colors(&vec!["000000".to_string(); 8]);
            } else {
                g.hid_per_key.send_enter_per_key_mode(brightness);
                if mode == "static" {
                    g.hid_per_key.set_zone_colors(&colors);
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

        // ── Standard sysfs driver ───────────────────────────────────────────────
        if !g.hw.available { return; }

        if !power || brightness == 0 {
            g.hw.write_brightness(0);
            g.hw.write_all("000000");
            return;
        }

        if g.hw.is_new_driver {
            g.hw.write_brightness(brightness);
            g.hw.write_mode(&mode, speed);
            if mode == "static" {
                for i in 0..g.hw.zone_count as usize {
                    let hex = colors.get(i).cloned().unwrap_or_else(|| colors[0].clone());
                    g.hw.write_zone(i, &hex);
                }
            }
        } else {
            g.hw.write_brightness(1);
            if mode == "static" {
                let scaler = (brightness as f64 / 100.0).clamp(0.0, 1.0);
                for i in 0..g.hw.zone_count as usize {
                    let raw = colors.get(i).cloned().unwrap_or_else(|| colors[0].clone());
                    let scaled = scale_hex_color(&raw, scaler);
                    g.hw.write_zone(i, &scaled);
                }
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
            if g.config.colors.len() < 17 {
                g.config.colors.resize(17, "FF0000".to_string());
            }
            if zone_val == 8 {
                for i in 0..8 { g.config.colors[i] = hex.clone(); }
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

    /// GetState — mirrors Python GetState().
    async fn get_state(&self) -> String {
        let g = self.inner.lock().await;
        let mut snap = serde_json::json!({
            "mode": g.config.mode,
            "colors": g.config.colors,
            "speed": g.config.speed,
            "brightness": g.config.brightness,
            "direction": g.config.direction,
            "power": g.config.power,
            "win_lock": g.config.win_lock,
            "unsupported": !g.hw.available,
            "driver_active": g.hw.available,
            "driver_path": g.hw.driver_path.clone().unwrap_or_default(),
            "is_new_driver": g.hw.is_new_driver,
            "zone_count": g.hw.zone_count,
            "zone_count_override": g.config.zone_count_override,
            "per_key_available": g.hid_per_key.is_available(),
            "hid_device_pid": g.hid_per_key.device_pid.map(|p| format!("{:04X}", p)).unwrap_or_default(),
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
            4 => Some(4),
            8 => Some(8),
            _ => return "FAIL: zone_count must be 0 (clear), 4, or 8".to_string(),
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

        let g = self.inner.lock().await;
        if !g.hid_per_key.is_available() {
            return "FAIL: no HID per-key device".to_string();
        }

        let key_colors: Vec<(u8, u8, u8)> = colors.iter().map(|hex| {
            let h = hex.trim_start_matches('#');
            let r = u8::from_str_radix(h.get(0..2).unwrap_or("00"), 16).unwrap_or(0);
            let g_c = u8::from_str_radix(h.get(2..4).unwrap_or("00"), 16).unwrap_or(0);
            let b = u8::from_str_radix(h.get(4..6).unwrap_or("00"), 16).unwrap_or(0);
            (r, g_c, b)
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
        "rainbow" | "cycle" | "wave_rainbow" => {
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
