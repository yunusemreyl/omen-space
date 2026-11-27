mod effects;
mod hardware;
mod hid_per_key;
pub mod key_layout;
mod lamparray;
mod primax_map;

use effects::{compute_anim_color, render_per_key_rgb, should_stream_software_frame};
pub use hardware::{parse_hex_color, scale_hex_color, RgbHardware};
use hid_per_key::HidPerKeyBackend;
#[cfg(test)]
use lamparray::LampArrayBackend;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;
use zbus::interface;

const VALID_MODES: &[&str] = &[
    "static",
    "per_key_custom",
    "breathing",
    "wave",
    "cycle",
    "rainbow",
    "pulse",
    "chase",
    "sparkle",
    "candle",
    "aurora",
    "disco",
    "gradient",
    "wave_rainbow",
    "starlight",
    "marquee",
    "reactive",
    "ripple",
    "raindrop",
    "blinking",
];
const VALID_DIRECTIONS: &[&str] = &["ltr", "rtl"];

const CONFIG_PATH: &str = "/etc/omen-space/rgb.json";
const PER_KEY_MAP_PATH: &str = "/root/.config/omen-space/per_key_map.json";

#[derive(Serialize, Deserialize, Debug, Clone)]
struct RgbConfig {
    mode: String,
    colors: Vec<String>,
    speed: u32,
    brightness: u32,
    direction: String,
    power: bool,
    win_lock: bool,
    #[serde(default)]
    zone_count_override: Option<u32>,
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
    vec![
        "E03454".to_string(),
        "0099EE".to_string(),
        "0099EE".to_string(),
        "E03454".to_string(),
    ]
}
fn default_true() -> bool {
    true
}
fn default_100() -> u32 {
    100
}
fn default_50() -> u32 {
    50
}
fn default_static() -> String {
    "static".to_string()
}

impl Default for RgbConfig {
    fn default() -> Self {
        Self {
            mode: "static".to_string(),
            colors: vec!["0099ED".to_string(); 8],
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
    fn parse_and_sanitize(data: &str) -> Self {
        let mut cfg: Self = serde_json::from_str(data).unwrap_or_default();
        if !VALID_MODES.contains(&cfg.mode.as_str()) {
            cfg.mode = "static".to_string();
        }
        cfg.colors = cfg
            .colors
            .into_iter()
            .filter(|c| c.len() == 6 && c.chars().all(|ch| ch.is_ascii_hexdigit()))
            .take(17)
            .collect();
        while cfg.colors.len() < 8 {
            cfg.colors.push("0099ED".to_string());
        }
        cfg
    }

    fn load() -> Self {
        if let Ok(data) = std::fs::read_to_string(CONFIG_PATH) {
            Self::parse_and_sanitize(&data)
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

struct RgbInner {
    evdev_monitor: crate::evdev_monitor::EvdevMonitor,
    hw: RgbHardware,
    hid_per_key: Arc<HidPerKeyBackend>,
    config: RgbConfig,
    last_lightbar_level: i32,
    calibrating: bool,
    calibration_until: Option<std::time::Instant>,
    wizard: Arc<crate::hid_wizard::HidPerKeyWizard>,
    desktop_rgb: Arc<crate::desktop_rgb::DesktopRgbController>,
    dirty_state: bool,
    dirty_lightbar: bool,
    dirty_save: bool,
    last_save_request: Option<std::time::Instant>,
}

#[derive(Clone, Copy)]
struct CompactStr24 {
    buf: [u8; 24],
    len: u8,
}

impl CompactStr24 {
    fn from_str(s: &str) -> Self {
        let mut buf = [0u8; 24];
        let bytes = s.as_bytes();
        let mut len = bytes.len().min(24);
        while len > 0 && !s.is_char_boundary(len) {
            len -= 1;
        }
        buf[..len].copy_from_slice(&bytes[..len]);
        Self {
            buf,
            len: len as u8,
        }
    }

    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.buf[..self.len as usize]).unwrap_or("")
    }
}

pub(crate) fn advance_anim_step(step: f64, speed: u32, dt: f64) -> f64 {
    let clamped_dt = dt.clamp(0.0, 0.12);
    step + (speed as f64 / 100.0) * 5.0 * clamped_dt
}

pub(crate) fn is_speed_only_update(
    current_mode: &str,
    new_mode: &str,
    power: bool,
    hw_effects: bool,
    is_new_driver: bool,
) -> bool {
    power
        && current_mode == new_mode
        && new_mode != "static"
        && new_mode != "per_key_custom"
        && !hw_effects
        && !is_new_driver
}

pub(crate) fn target_frame_ms(
    is_interactive: bool,
    has_per_key_streaming: bool,
    has_all_zones: bool,
) -> u64 {
    if is_interactive {
        25
    } else if has_per_key_streaming {
        33
    } else if has_all_zones {
        40
    } else {
        120
    }
}

pub(crate) fn static_colors_for_per_key(
    colors: &[String],
    brightness: u32,
    scale_brightness: bool,
) -> Vec<(u8, u8, u8)> {
    let primary_hex = colors.first().map(|s| s.as_str()).unwrap_or("0099ED");
    let (r, g, b) = parse_hex_color(primary_hex);
    let scaler = if scale_brightness {
        (brightness as f64 / 100.0).clamp(0.0, 1.0)
    } else {
        1.0
    };
    let scaled = (
        (r as f64 * scaler) as u8,
        (g as f64 * scaler) as u8,
        (b as f64 * scaler) as u8,
    );
    vec![scaled; HidPerKeyBackend::TOTAL_KEY_COUNT]
}

#[derive(Clone)]
pub struct RgbService {
    inner: Arc<Mutex<RgbInner>>,
    wakeup: Arc<tokio::sync::Notify>,
}

impl RgbService {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let config = RgbConfig::load();
        let hw = RgbHardware::detect_with_override(config.zone_count_override);
        if hw.available {
            info!(
                "RGB: Driver at {:?} (new_driver={}, zone_count={})",
                hw.driver_path, hw.is_new_driver, hw.zone_count
            );
        } else {
            warn!("RGB: No RGB hardware driver found");
        }

        let hid_per_key = Arc::new(HidPerKeyBackend::new());
        let wizard = Arc::new(crate::hid_wizard::HidPerKeyWizard::new());
        let evdev_monitor = crate::evdev_monitor::EvdevMonitor::new();

        let mut desktop_rgb = crate::desktop_rgb::DesktopRgbController::new();
        if let Err(e) = desktop_rgb.initialize() {
            warn!("RGB: Desktop RGB not initialized: {}", e);
        }

        let inner = Arc::new(Mutex::new(RgbInner {
            hw,
            hid_per_key,
            config,
            last_lightbar_level: -1,
            calibrating: false,
            calibration_until: None,
            wizard,
            evdev_monitor,
            desktop_rgb: Arc::new(desktop_rgb),
            dirty_state: false,
            dirty_lightbar: false,
            dirty_save: false,
            last_save_request: None,
        }));
        let wakeup = Arc::new(tokio::sync::Notify::new());

        let svc = Self {
            inner: inner.clone(),
            wakeup: wakeup.clone(),
        };

        Self::apply_state(inner.clone()).await;

        let deferred_inner = inner.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            Self::apply_state(deferred_inner).await;
            info!("RGB: deferred re-apply completed");
        });

        let engine_inner = inner.clone();
        let engine_wakeup = wakeup.clone();
        tokio::spawn(async move {
            Self::rgb_engine_loop(engine_inner, engine_wakeup).await;
        });

        let uleds_inner = inner.clone();
        let uleds_wakeup = wakeup.clone();
        tokio::spawn(async move {
            Self::uleds_monitor_loop(uleds_inner, uleds_wakeup).await;
        });

        Ok(svc)
    }

    async fn uleds_monitor_loop(inner: Arc<Mutex<RgbInner>>, wakeup: Arc<tokio::sync::Notify>) {
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
                info!(
                    "uleds module not available or accessible, skipping native OS brightness integration: {}",
                    e
                );
                return;
            }
        };

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
        let mut async_file = tokio::fs::File::from_std(std_file);

        loop {
            let mut val_bytes = [0u8; 4];
            match async_file.read_exact(&mut val_bytes).await {
                Ok(_) => {
                    let new_brightness = i32::from_ne_bytes(val_bytes);
                    let mut should_notify = false;
                    {
                        let mut g = inner.lock().await;
                        let clamped = new_brightness.clamp(0, 100) as u32;
                        if g.config.brightness != clamped || (clamped > 0 && !g.config.power) {
                            g.config.brightness = clamped;
                            g.config.power = clamped > 0;
                            g.dirty_state = true;
                            g.dirty_save = true;
                            g.last_save_request = Some(std::time::Instant::now());
                            should_notify = true;
                        }
                    }
                    if should_notify {
                        wakeup.notify_one();
                    }
                }
                Err(e) => {
                    warn!("uleds read error: {}, stopping listener", e);
                    break;
                }
            }
        }
    }

    async fn rgb_engine_loop(inner: Arc<Mutex<RgbInner>>, wakeup: Arc<tokio::sync::Notify>) {
        struct EngineSnapshot {
            calibrating: bool,
            dirty_state: bool,
            dirty_lightbar: bool,
            apply_config: Option<RgbConfig>,
            save_config: Option<RgbConfig>,
            save_pending_wait_ms: Option<u64>,
            power: bool,
            brightness: u32,
            speed: u32,
            hw_effects: bool,
            direction_ltr: bool,
            mode: CompactStr24,
            primary_rgb: (u8, u8, u8),
            secondary_rgb: (u8, u8, u8),
            mcu_rec: Option<[u8; HidPerKeyBackend::MCU_RECORD_LEN]>,
            lb_power: bool,
            lb_brightness: u32,
            lb_speed: u32,
            lb_mode: CompactStr24,
            lb_primary: (u8, u8, u8),
            lb_secondary: (u8, u8, u8),
            hw_available: bool,
            hw_is_new_driver: bool,
            hw_zone_count: u32,
            hw_has_lightbar: bool,
            hw_has_all_zones: bool,
            hw: Option<RgbHardware>,
            hid_per_key: Arc<HidPerKeyBackend>,
            desktop_rgb: Arc<crate::desktop_rgb::DesktopRgbController>,
            recent_keys_arc: Arc<Mutex<Vec<crate::evdev_monitor::KeyEventInfo>>>,
            wakeup_handle: Arc<tokio::sync::Notify>,
            last_lightbar_level: i32,
        }

        type LampFrameCache = ([(u8, u8, u8); HidPerKeyBackend::MAX_STACK_LAMPS], usize);
        let mut last_per_key_rgb: Option<[(u8, u8, u8); 104]> = None;
        let mut last_lamp_rgb: Option<LampFrameCache> = None;
        let mut last_sysfs_rgb: Option<[(u8, u8, u8); 8]> = None;
        let mut last_sysfs_zones: usize = 0;
        let mut last_lightbar_rgb: Option<[(u8, u8, u8); 4]> = None;
        let mut anim_step: f64 = 0.0;
        let mut lightbar_step: f64 = 0.0;
        let mut mcu_sig: Option<[u8; HidPerKeyBackend::MCU_RECORD_LEN]> = None;
        let mut last_frame_at = std::time::Instant::now();
        let mut recent_keys: Vec<(f64, f64, f64)> = Vec::with_capacity(32);

        loop {
            let frame_start = std::time::Instant::now();
            let dt = frame_start
                .duration_since(last_frame_at)
                .as_secs_f64()
                .clamp(0.0, 0.12);
            last_frame_at = frame_start;

            // Lock `inner` ONLY long enough to snapshot Copy state / dirty configs and clone Arcs.
            // `inner` is dropped BEFORE locking `recent_keys` and BEFORE any `spawn_blocking` I/O.
            let snap = {
                let mut g = inner.lock().await;
                if g.calibrating {
                    if let Some(until) = g.calibration_until {
                        if frame_start >= until {
                            g.calibrating = false;
                            g.calibration_until = None;
                        }
                    } else {
                        g.calibrating = false;
                    }
                }

                let has_per_key = g.hid_per_key.is_available();
                let is_primax_mcu = g.hid_per_key.is_primax_mcu();
                let is_lamparray = g.hid_per_key.is_lamparray();
                let is_interactive = !g.calibrating
                    && g.config.power
                    && g.config.brightness > 0
                    && has_per_key
                    && (g.config.mode == "reactive" || g.config.mode == "ripple")
                    && (!g.config.hw_effects || !is_primax_mcu);
                g.evdev_monitor.set_active(is_interactive);

                let dirty_state = std::mem::take(&mut g.dirty_state);
                let dirty_lightbar = std::mem::take(&mut g.dirty_lightbar);
                let apply_config = if dirty_state || dirty_lightbar {
                    Some(g.config.clone())
                } else {
                    None
                };
                let mut save_config = None;
                let mut save_pending_wait_ms = None;
                if g.dirty_save {
                    let due = g
                        .last_save_request
                        .map(|t| {
                            frame_start.duration_since(t) >= std::time::Duration::from_millis(400)
                        })
                        .unwrap_or(true);
                    if due {
                        g.dirty_save = false;
                        g.last_save_request = None;
                        save_config = Some(g.config.clone());
                    } else if let Some(t) = g.last_save_request {
                        let rem = std::time::Duration::from_millis(400)
                            .saturating_sub(frame_start.duration_since(t))
                            .as_millis() as u64;
                        save_pending_wait_ms = Some(rem.max(1));
                    }
                }

                let kb_anim_active = !g.calibrating
                    && g.config.power
                    && g.config.brightness > 0
                    && g.config.mode != "static"
                    && g.config.mode != "per_key_custom";
                let mcu_rec =
                    if kb_anim_active && g.config.hw_effects && has_per_key && !is_primax_mcu {
                        g.hid_per_key.mcu_effect_for(
                            &g.config.mode,
                            g.config.speed,
                            &g.config.direction,
                            &g.config.colors,
                        )
                    } else {
                        None
                    };

                let hw_has_lightbar = g.hw.available && g.hw.has_lightbar();
                let hw_has_all_zones = g.hw.has_all_zones_node();
                let need_sysfs_kb = kb_anim_active
                    && g.hw.available
                    && !g.hw.is_new_driver
                    && !is_lamparray
                    && !is_primax_mcu;
                let need_lb = hw_has_lightbar
                    && is_lamparray
                    && g.config.lightbar_power
                    && g.config.lightbar_brightness > 0
                    && g.config.lightbar_mode != "static";
                let hw = if dirty_state || dirty_lightbar || need_sysfs_kb || need_lb {
                    Some(g.hw.clone())
                } else {
                    None
                };

                EngineSnapshot {
                    calibrating: g.calibrating,
                    dirty_state,
                    dirty_lightbar,
                    apply_config,
                    save_config,
                    save_pending_wait_ms,
                    power: g.config.power,
                    brightness: g.config.brightness,
                    speed: g.config.speed,
                    hw_effects: g.config.hw_effects,
                    direction_ltr: g.config.direction == "ltr",
                    mode: CompactStr24::from_str(&g.config.mode),
                    primary_rgb: parse_hex_color(
                        g.config
                            .colors
                            .first()
                            .map(|s| s.as_str())
                            .unwrap_or("FF0000"),
                    ),
                    secondary_rgb: parse_hex_color(
                        g.config
                            .colors
                            .get(1)
                            .map(|s| s.as_str())
                            .unwrap_or("0000FF"),
                    ),
                    mcu_rec,
                    lb_power: g.config.lightbar_power,
                    lb_brightness: g.config.lightbar_brightness,
                    lb_speed: g.config.lightbar_speed,
                    lb_mode: CompactStr24::from_str(&g.config.lightbar_mode),
                    lb_primary: parse_hex_color(
                        g.config
                            .lightbar_colors
                            .first()
                            .map(|s| s.as_str())
                            .unwrap_or("FF0000"),
                    ),
                    lb_secondary: parse_hex_color(
                        g.config
                            .lightbar_colors
                            .get(1)
                            .map(|s| s.as_str())
                            .unwrap_or("0000FF"),
                    ),
                    hw_available: g.hw.available,
                    hw_is_new_driver: g.hw.is_new_driver,
                    hw_zone_count: g.hw.zone_count,
                    hw_has_lightbar,
                    hw_has_all_zones,
                    hw,
                    hid_per_key: g.hid_per_key.clone(),
                    desktop_rgb: g.desktop_rgb.clone(),
                    recent_keys_arc: g.evdev_monitor.recent_keys.clone(),
                    wakeup_handle: g.evdev_monitor.wakeup_handle(),
                    last_lightbar_level: g.last_lightbar_level,
                }
            };

            if snap.dirty_state {
                last_per_key_rgb = None;
                last_lamp_rgb = None;
                last_sysfs_rgb = None;
                last_sysfs_zones = 0;
                mcu_sig = None;
            }
            if snap.dirty_lightbar {
                last_lightbar_rgb = None;
            }

            if snap.calibrating {
                last_per_key_rgb = None;
                last_lamp_rgb = None;
                last_sysfs_rgb = None;
                last_sysfs_zones = 0;
                if let Some(cfg) = snap.save_config {
                    let _ = tokio::task::spawn_blocking(move || cfg.save()).await;
                }
                let wait_ms = snap.save_pending_wait_ms.unwrap_or(100).min(100);
                tokio::select! {
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(wait_ms)) => {}
                    _ = wakeup.notified() => {}
                }
                continue;
            }

            let mode_str = snap.mode.as_str();
            let dir_str = if snap.direction_ltr { "ltr" } else { "rtl" };
            let has_per_key = snap.hid_per_key.is_available();
            let is_lamparray = snap.hid_per_key.is_lamparray();
            let is_primax_mcu = snap.hid_per_key.is_primax_mcu();
            let has_old_sysfs = snap.hw_available && !snap.hw_is_new_driver;
            let kb_anim_mode = snap.power
                && snap.brightness > 0
                && mode_str != "static"
                && mode_str != "per_key_custom"
                && (has_per_key || has_old_sysfs);

            let mut kb_streaming = false;
            let mut enter_per_key_brightness: Option<u32> = None;
            let mut mcu_to_send: Option<([u8; HidPerKeyBackend::MCU_RECORD_LEN], u32)> = None;

            if kb_anim_mode {
                if has_per_key {
                    if snap.hw_effects && is_primax_mcu {
                        last_per_key_rgb = None;
                        last_lamp_rgb = None;
                        mcu_sig = None;
                    } else if !should_stream_software_frame(snap.hw_effects, snap.mcu_rec.is_some())
                    {
                        if let Some(rec) = snap.mcu_rec {
                            if mcu_sig != Some(rec) {
                                mcu_to_send = Some((rec, snap.brightness));
                            }
                        }
                        last_per_key_rgb = None;
                        last_lamp_rgb = None;
                    } else {
                        if mcu_sig.take().is_some() {
                            enter_per_key_brightness = Some(snap.brightness);
                        }
                        kb_streaming = true;
                    }
                } else {
                    mcu_sig = None;
                    kb_streaming = true;
                }
            } else {
                last_per_key_rgb = None;
                last_lamp_rgb = None;
                last_sysfs_rgb = None;
                last_sysfs_zones = 0;
                mcu_sig = None;
            }

            let is_interactive =
                kb_streaming && has_per_key && (mode_str == "reactive" || mode_str == "ripple");

            recent_keys.clear();
            if is_interactive {
                let mut keys_guard = snap.recent_keys_arc.lock().await;
                let now = std::time::Instant::now();
                keys_guard.retain(|k| now.duration_since(k.timestamp).as_secs_f64() <= 2.5);
                for k in keys_guard.iter() {
                    recent_keys.push((k.x, k.y, now.duration_since(k.timestamp).as_secs_f64()));
                }
            }
            let recent_keys_empty = recent_keys.is_empty();

            type SysfsZonesFrame = ([(u8, u8, u8); 8], usize);
            let mut per_key_to_write: Option<[(u8, u8, u8); 104]> = None;
            let mut lamp_to_write: Option<LampFrameCache> = None;
            let mut sysfs_to_write: Option<SysfsZonesFrame> = None;

            if kb_streaming {
                anim_step = advance_anim_step(anim_step, snap.speed, dt);
                let scaler = (snap.brightness as f64 / 100.0).clamp(0.0, 1.0);

                if has_per_key {
                    let mut lamp_buf = [(0u8, 0u8, 0u8); HidPerKeyBackend::MAX_STACK_LAMPS];
                    if let Some(n) = snap.hid_per_key.render_lamparray_rgb(
                        mode_str,
                        anim_step,
                        snap.speed,
                        dir_str,
                        snap.primary_rgb,
                        snap.secondary_rgb,
                        scaler,
                        &recent_keys,
                        &mut lamp_buf,
                    ) {
                        let changed = match last_lamp_rgb {
                            Some((ref prev, prev_n)) => prev_n != n || prev[..n] != lamp_buf[..n],
                            None => true,
                        };
                        if changed {
                            lamp_to_write = Some((lamp_buf, n));
                            last_lamp_rgb = Some((lamp_buf, n));
                        }
                        last_per_key_rgb = None;
                    } else {
                        let rgb_frame = render_per_key_rgb(
                            mode_str,
                            anim_step,
                            snap.speed,
                            dir_str,
                            snap.primary_rgb,
                            snap.secondary_rgb,
                            scaler,
                            &recent_keys,
                        );
                        if last_per_key_rgb.as_ref() != Some(&rgb_frame) {
                            per_key_to_write = Some(rgb_frame);
                            last_per_key_rgb = Some(rgb_frame);
                        }
                        last_lamp_rgb = None;
                    }
                }

                if has_old_sysfs && !is_lamparray && !is_primax_mcu {
                    let zone_count = (snap.hw_zone_count as usize).clamp(1, 8);
                    let mut sysfs_arr = [(0u8, 0u8, 0u8); 8];
                    for (i, slot) in sysfs_arr.iter_mut().enumerate().take(zone_count) {
                        let eff_idx = if snap.direction_ltr {
                            i
                        } else {
                            zone_count - 1 - i
                        };
                        let (r, g_c, b) = compute_anim_color(
                            mode_str,
                            anim_step,
                            eff_idx,
                            zone_count,
                            snap.primary_rgb.0,
                            snap.primary_rgb.1,
                            snap.primary_rgb.2,
                            snap.secondary_rgb.0,
                            snap.secondary_rgb.1,
                            snap.secondary_rgb.2,
                        );
                        *slot = (
                            (r as f64 * scaler) as u8,
                            (g_c as f64 * scaler) as u8,
                            (b as f64 * scaler) as u8,
                        );
                    }
                    if last_sysfs_rgb != Some(sysfs_arr) || last_sysfs_zones != zone_count {
                        sysfs_to_write = Some((sysfs_arr, zone_count));
                        last_sysfs_rgb = Some(sysfs_arr);
                        last_sysfs_zones = zone_count;
                    }
                }
            }

            let lb_mode_str = snap.lb_mode.as_str();
            let lb_streaming = snap.hw_has_lightbar
                && is_lamparray
                && snap.lb_power
                && snap.lb_brightness > 0
                && lb_mode_str != "static";

            let mut lb_to_write: Option<[(u8, u8, u8); 4]> = None;
            if lb_streaming {
                lightbar_step = advance_anim_step(lightbar_step, snap.lb_speed, dt);
                let scaler = (snap.lb_brightness as f64 / 100.0).clamp(0.0, 1.0);
                let (r1, g1, b1) = snap.lb_primary;
                let (r2, g2, b2) = snap.lb_secondary;
                let mut lb_arr = [(0u8, 0u8, 0u8); 4];
                for (i, slot) in lb_arr.iter_mut().enumerate() {
                    let (r, gc, b) = compute_anim_color(
                        lb_mode_str,
                        lightbar_step,
                        i,
                        4,
                        r1,
                        g1,
                        b1,
                        r2,
                        g2,
                        b2,
                    );
                    *slot = (
                        ((r as f64) * scaler) as u8,
                        ((gc as f64) * scaler) as u8,
                        ((b as f64) * scaler) as u8,
                    );
                }
                if last_lightbar_rgb != Some(lb_arr) {
                    lb_to_write = Some(lb_arr);
                    last_lightbar_rgb = Some(lb_arr);
                }
            } else {
                last_lightbar_rgb = None;
            }

            let has_blocking_work = snap.dirty_state
                || snap.dirty_lightbar
                || snap.save_config.is_some()
                || enter_per_key_brightness.is_some()
                || mcu_to_send.is_some()
                || per_key_to_write.is_some()
                || lamp_to_write.is_some()
                || sysfs_to_write.is_some()
                || lb_to_write.is_some();

            if has_blocking_work {
                let hw_opt = snap.hw;
                let hid_per_key = snap.hid_per_key;
                let desktop_rgb = snap.desktop_rgb;
                let apply_config = snap.apply_config;
                let dirty_state = snap.dirty_state;
                let dirty_lightbar = snap.dirty_lightbar;
                let save_config = snap.save_config;
                let mut lb_level = snap.last_lightbar_level;

                let res = tokio::task::spawn_blocking(move || {
                    if let (Some(ref hw), Some(ref config)) = (&hw_opt, &apply_config) {
                        if dirty_state {
                            Self::apply_state_sync(
                                hw,
                                &hid_per_key,
                                &desktop_rgb,
                                config,
                                &mut lb_level,
                            );
                        } else if dirty_lightbar {
                            Self::apply_lightbar_sync(hw, &hid_per_key, config, &mut lb_level);
                        }
                    }

                    if let Some(b) = enter_per_key_brightness {
                        hid_per_key.send_enter_per_key_mode(b);
                    }
                    let mcu_sent_sig = if let Some((rec, b)) = mcu_to_send {
                        if hid_per_key.send_mcu_effect_with_brightness(&rec, b) {
                            Some(rec)
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if let Some((lamp_frame, n)) = lamp_to_write {
                        hid_per_key.write_lamparray_colors(&lamp_frame[..n]);
                    }
                    if let Some(frame) = per_key_to_write {
                        hid_per_key.write_per_key_colors(&frame);
                    }
                    if let (Some((arr, zc)), Some(ref hw)) = (sysfs_to_write, &hw_opt) {
                        hw.write_zones_rgb(&arr[..zc]);
                    }
                    if let (Some(lb_arr), Some(ref hw)) = (lb_to_write, &hw_opt) {
                        hw.write_lightbar_zones_rgb(&lb_arr);
                    }
                    if let Some(cfg) = save_config {
                        cfg.save();
                    }
                    (lb_level, mcu_sent_sig)
                })
                .await;

                if let Ok((new_lb_level, sent_sig)) = res {
                    if let Some(sig) = sent_sig {
                        mcu_sig = Some(sig);
                    }
                    if new_lb_level != snap.last_lightbar_level {
                        let mut g = inner.lock().await;
                        g.last_lightbar_level = new_lb_level;
                    }
                }
            }

            let interactive_idle = is_interactive
                && recent_keys_empty
                && if is_lamparray {
                    last_lamp_rgb
                        .as_ref()
                        .is_none_or(|(arr, n)| arr[..*n].iter().all(|&c| c == (0, 0, 0)))
                } else {
                    last_per_key_rgb
                        .as_ref()
                        .is_none_or(|f| f.iter().all(|&c| c == (0, 0, 0)))
                };

            if interactive_idle && !lb_streaming {
                if let Some(wait_ms) = snap.save_pending_wait_ms {
                    tokio::select! {
                        _ = tokio::time::sleep(tokio::time::Duration::from_millis(wait_ms)) => {}
                        _ = wakeup.notified() => {}
                        _ = snap.wakeup_handle.notified() => {}
                    }
                } else {
                    // Zero-CPU idle park until an actual key press (evdev) or D-Bus update.
                    tokio::select! {
                        _ = wakeup.notified() => {}
                        _ = snap.wakeup_handle.notified() => {}
                    }
                }
            } else if kb_streaming || lb_streaming {
                let target_ms = target_frame_ms(
                    is_interactive,
                    has_per_key && kb_streaming,
                    snap.hw_has_all_zones,
                );
                let deadline = tokio::time::Instant::from_std(
                    frame_start + std::time::Duration::from_millis(target_ms),
                );
                tokio::select! {
                    _ = tokio::time::sleep_until(deadline) => {}
                    _ = wakeup.notified() => {}
                    _ = snap.wakeup_handle.notified(), if is_interactive => {}
                }
            } else if let Some(wait_ms) = snap.save_pending_wait_ms {
                tokio::select! {
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(wait_ms)) => {}
                    _ = wakeup.notified() => {}
                }
            } else {
                wakeup.notified().await;
            }
        }
    }

    fn apply_lightbar_sync(
        hw: &RgbHardware,
        hid_per_key: &HidPerKeyBackend,
        config: &RgbConfig,
        last_lightbar_level: &mut i32,
    ) {
        if !hw.available || !hw.lightbar_write_allowed() {
            return;
        }
        let effect_active = config.lightbar_mode != "static"
            && config.lightbar_power
            && config.lightbar_brightness > 0;
        if !effect_active {
            let lb_colors: Vec<String> =
                if !config.lightbar_power || config.lightbar_brightness == 0 {
                    vec!["000000".to_string(); 4]
                } else {
                    let scaler = (config.lightbar_brightness as f64 / 100.0).clamp(0.0, 1.0);
                    (0..4)
                        .map(|i| {
                            let raw = config
                                .lightbar_colors
                                .get(i)
                                .map(|s| s.as_str())
                                .unwrap_or("000000");
                            scale_hex_color(raw, scaler)
                        })
                        .collect()
                };
            if hid_per_key.is_lamparray() && hw.has_lightbar() {
                hw.write_lightbar_zones(&lb_colors);
            } else {
                hw.write_lightbar(&lb_colors);
            }
        }
        if hid_per_key.is_lamparray() && hw.has_lightbar() {
            let level = if hw.is_new_driver {
                if config.lightbar_power {
                    config.lightbar_brightness as i32
                } else {
                    0
                }
            } else if config.lightbar_power && config.lightbar_brightness > 0 {
                1
            } else {
                0
            };
            if *last_lightbar_level != level {
                hw.write_brightness(level as u32);
                *last_lightbar_level = level;
            }
        }
    }

    fn apply_state_sync(
        hw: &RgbHardware,
        hid_per_key: &HidPerKeyBackend,
        desktop_rgb: &crate::desktop_rgb::DesktopRgbController,
        config: &RgbConfig,
        last_lightbar_level: &mut i32,
    ) {
        let power = config.power;
        let brightness = config.brightness;
        let mode = config.mode.as_str();
        let speed = config.speed;
        let colors = &config.colors;

        if hid_per_key.is_available() {
            hid_per_key.invalidate_cache();
            let primax_yield_hw = config.hw_effects && hid_per_key.is_primax_mcu();
            let mcu_anim_active = config.hw_effects
                && power
                && brightness > 0
                && hid_per_key
                    .mcu_effect_for(mode, speed, &config.direction, colors)
                    .is_some();
            if primax_yield_hw {
                // Yield hardware control to the keyboard's native Fn preset.
            } else if config.hw_effects && hid_per_key.is_lamparray() && mcu_anim_active {
                hid_per_key.release_lamparray();
            } else if !power || brightness == 0 {
                hid_per_key.send_enter_per_key_mode(0);
                hid_per_key.set_zone_colors(&vec!["000000".to_string(); 8]);
                hid_per_key.invalidate_cache();
            } else {
                hid_per_key.send_enter_per_key_mode(brightness);
                if mode == "static" {
                    let scale = hid_per_key.is_lamparray() || hid_per_key.is_primax_mcu();
                    let key_colors = static_colors_for_per_key(colors, brightness, scale);
                    hid_per_key.write_per_key_colors(&key_colors);
                } else if mode == "per_key_custom" {
                    let scaler = if hid_per_key.is_lamparray() || hid_per_key.is_primax_mcu() {
                        (brightness as f64 / 100.0).clamp(0.0, 1.0)
                    } else {
                        1.0
                    };
                    let key_colors: Vec<(u8, u8, u8)> = config
                        .per_key_colors
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
                    hid_per_key.write_per_key_colors(&key_colors);
                }
            }
        }

        if desktop_rgb.is_available() {
            if !power || brightness == 0 {
                let _ = desktop_rgb.set_static_colors(&[(0, 0, 0); 7], 0);
            } else if mode == "static" {
                let mut parsed_colors = Vec::with_capacity(7);
                for i in 0..7 {
                    let hex = colors
                        .get(10 + i)
                        .map(|s| s.as_str())
                        .unwrap_or_else(|| colors.first().map(|s| s.as_str()).unwrap_or("FF0000"));
                    parsed_colors.push(parse_hex_color(hex));
                }
                let _ = desktop_rgb.set_static_colors(&parsed_colors, brightness as u8);
            }
        }

        Self::apply_lightbar_sync(hw, hid_per_key, config, last_lightbar_level);

        if !hw.available {
            return;
        }

        if hid_per_key.is_lamparray() || hid_per_key.is_primax_mcu() {
            return;
        }

        if !power || brightness == 0 {
            hw.write_brightness(0);
            hw.write_all("000000");
            return;
        }

        if hw.is_new_driver {
            hw.write_brightness(brightness);
            hw.write_mode(mode, speed);
            if mode == "static" {
                let count = hw.zone_count as usize;
                let mut zone_colors = Vec::with_capacity(count);
                for i in 0..count {
                    zone_colors.push(colors.get(i).cloned().unwrap_or_else(|| colors[0].clone()));
                }
                hw.write_zones_batch(&zone_colors);
            }
        } else {
            hw.write_brightness(1);
            if mode == "static" {
                let scaler = (brightness as f64 / 100.0).clamp(0.0, 1.0);
                let count = (hw.zone_count as usize).clamp(1, 8);
                let mut scaled_rgb = [(0u8, 0u8, 0u8); 8];
                for (i, slot) in scaled_rgb.iter_mut().enumerate().take(count) {
                    let raw = colors
                        .get(i)
                        .map(|s| s.as_str())
                        .unwrap_or(colors[0].as_str());
                    let (r, g_c, b) = parse_hex_color(raw);
                    *slot = (
                        (r as f64 * scaler) as u8,
                        (g_c as f64 * scaler) as u8,
                        (b as f64 * scaler) as u8,
                    );
                }
                hw.write_zones_rgb(&scaled_rgb[..count]);
            }
        }
    }

    async fn apply_state(inner: Arc<Mutex<RgbInner>>) {
        let (hw, hid_per_key, desktop_rgb, config, mut last_lightbar_level) = {
            let mut g = inner.lock().await;
            if !g.hw.available {
                g.hw = RgbHardware::detect_with_override(g.config.zone_count_override);
            } else if let Some(ref path) = g.hw.driver_path {
                if !Path::new(path).exists() {
                    g.hw = RgbHardware::detect_with_override(g.config.zone_count_override);
                }
            }
            (
                g.hw.clone(),
                g.hid_per_key.clone(),
                g.desktop_rgb.clone(),
                g.config.clone(),
                g.last_lightbar_level,
            )
        };

        let new_lb_level = tokio::task::spawn_blocking(move || {
            Self::apply_state_sync(
                &hw,
                &hid_per_key,
                &desktop_rgb,
                &config,
                &mut last_lightbar_level,
            );
            last_lightbar_level
        })
        .await
        .unwrap_or(-1);

        if new_lb_level >= 0 {
            let mut g = inner.lock().await;
            g.last_lightbar_level = new_lb_level;
        }
    }
}

#[interface(name = "org.hp.omen.Rgb")]
impl RgbService {
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
            if g.config.mode == "per_key_custom" {
                g.config.mode = "static".to_string();
            }
            g.config.power = true;
            g.config.hw_effects = false;
            if g.config.colors.len() < 17 {
                g.config.colors.resize(17, "0099ED".to_string());
            }
            if zone_val == 8 {
                for slot in g.config.colors.iter_mut().take(8) {
                    *slot = hex.clone();
                }
            } else {
                g.config.colors[zone_val as usize] = hex.clone();
            }
            g.dirty_state = true;
            g.dirty_save = true;
            g.last_save_request = Some(std::time::Instant::now());
        }

        self.wakeup.notify_one();
        "OK".to_string()
    }

    async fn set_mode(&self, mode_str: String, speed_val: i32) -> String {
        if !VALID_MODES.contains(&mode_str.as_str()) {
            return "FAIL".to_string();
        }
        {
            let mut g = self.inner.lock().await;
            let new_speed = (speed_val as u32).clamp(1, 100);
            let speed_only = is_speed_only_update(
                &g.config.mode,
                &mode_str,
                g.config.power,
                g.config.hw_effects,
                g.hw.is_new_driver,
            );
            if speed_only {
                g.config.speed = new_speed;
            } else {
                g.config.mode = mode_str;
                g.config.speed = new_speed;
                g.config.power = true;
                g.dirty_state = true;
            }
            g.dirty_save = true;
            g.last_save_request = Some(std::time::Instant::now());
        }

        self.wakeup.notify_one();
        "OK".to_string()
    }

    async fn set_global(
        &self,
        power_val: bool,
        brightness_val: i32,
        direction_str: String,
    ) -> String {
        if !VALID_DIRECTIONS.contains(&direction_str.as_str()) {
            return "FAIL".to_string();
        }
        {
            let mut g = self.inner.lock().await;
            g.config.power = power_val;
            g.config.brightness = (brightness_val as u32).clamp(0, 100);
            g.config.direction = direction_str;
            g.dirty_state = true;
            g.dirty_save = true;
            g.last_save_request = Some(std::time::Instant::now());
        }

        self.wakeup.notify_one();
        "OK".to_string()
    }

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
                for slot in g.config.lightbar_colors.iter_mut().take(4) {
                    *slot = hex.clone();
                }
            } else if (0..4).contains(&zone_val) {
                g.config.lightbar_colors[zone_val as usize] = hex;
            } else {
                return "FAIL".to_string();
            }
            g.config.lightbar_power = true;
            g.dirty_lightbar = true;
            g.dirty_save = true;
            g.last_save_request = Some(std::time::Instant::now());
        }
        self.wakeup.notify_one();
        "OK".to_string()
    }

    async fn set_lightbar_global(
        &self,
        power_val: bool,
        brightness_val: i32,
        mode_str: String,
        speed_val: i32,
    ) -> String {
        {
            let mut g = self.inner.lock().await;
            g.config.lightbar_power = power_val;
            g.config.lightbar_brightness = (brightness_val as u32).clamp(0, 100);
            g.config.lightbar_mode = mode_str;
            g.config.lightbar_speed = (speed_val as u32).clamp(1, 100);
            g.dirty_lightbar = true;
            g.dirty_save = true;
            g.last_save_request = Some(std::time::Instant::now());
        }
        self.wakeup.notify_one();
        "OK".to_string()
    }

    async fn get_state(&self) -> String {
        let g = self.inner.lock().await;
        let any_driver =
            g.hw.available || g.hid_per_key.is_available() || g.desktop_rgb.is_available();
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
                 No HID per-key device found either."
                    .to_string(),
            );
        }
        snap.to_string()
    }

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
            g.hw = RgbHardware::detect_with_override(normalized);
            g.dirty_state = true;
            g.dirty_save = true;
            g.last_save_request = Some(std::time::Instant::now());
            info!(
                "SetZoneCountOverride: override={:?}, effective zone_count={}",
                normalized, g.hw.zone_count
            );
        }
        self.wakeup.notify_one();
        "OK".to_string()
    }

    async fn set_hw_effects(&self, enabled: bool) -> String {
        {
            let mut g = self.inner.lock().await;
            g.config.hw_effects = enabled;
            g.dirty_state = true;
            g.dirty_save = true;
            g.last_save_request = Some(std::time::Instant::now());
            info!("SetHwEffects: {}", enabled);
        }
        self.wakeup.notify_one();
        "OK".to_string()
    }

    async fn set_win_lock(&self, locked: bool) -> String {
        let mut g = self.inner.lock().await;
        g.config.win_lock = locked;
        g.hw.write_win_lock(locked);
        g.config.save();
        info!("SetWinLock: {}", if locked { "LOCKED" } else { "UNLOCKED" });
        "OK".to_string()
    }

    async fn start_lamp_calibration(&self) -> String {
        let mut g = self.inner.lock().await;
        if g.hid_per_key.lamp_array_count() == 0 {
            return "FAIL: no HID LampArray".to_string();
        }
        g.calibrating = true;
        g.calibration_until = Some(std::time::Instant::now() + std::time::Duration::from_secs(600));
        let count = g.hid_per_key.lamp_array_count();
        drop(g);
        self.wakeup.notify_one();
        serde_json::json!({
            "status": "calibrating",
            "lamps": count,
        })
        .to_string()
    }

    async fn stop_lamp_calibration(&self) -> String {
        {
            let mut g = self.inner.lock().await;
            g.calibrating = false;
            g.calibration_until = None;
            g.dirty_state = true;
        }
        self.wakeup.notify_one();
        "OK".to_string()
    }

    async fn get_lamp_map(&self) -> String {
        let g = self.inner.lock().await;
        g.hid_per_key.lamp_map_json()
    }

    async fn set_lamp_colors(&self, colors_json: String) -> String {
        let Ok(colors) = serde_json::from_str::<Vec<String>>(&colors_json) else {
            return "FAIL: invalid JSON".to_string();
        };
        let mut g = self.inner.lock().await;
        let count = g.hid_per_key.lamp_array_count();
        if count == 0 {
            return "FAIL: no HID LampArray".to_string();
        }
        if colors.len() != count {
            return format!("FAIL: expected {} colours, got {}", count, colors.len());
        }
        let rgb: Vec<(u8, u8, u8)> = colors.iter().map(|hex| parse_hex_color(hex)).collect();
        g.calibrating = true;
        g.calibration_until = Some(std::time::Instant::now() + std::time::Duration::from_secs(600));
        if g.hid_per_key.set_lamp_colors(&rgb) {
            "OK".to_string()
        } else {
            "FAIL: LampArray write failed".to_string()
        }
    }

    async fn test_single_key(&self, index: i32) -> String {
        info!("TestSingleKey: index={}", index);
        let g = self.inner.lock().await;
        if g.hid_per_key.is_available() && g.hid_per_key.test_single_key(index as usize, 255, 0, 0)
        {
            return "OK".to_string();
        }
        "FAIL".to_string()
    }

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
            warn!(
                "SetPerKeyColors: expected {} colors, got {}",
                HidPerKeyBackend::TOTAL_KEY_COUNT,
                colors.len()
            );
            return format!(
                "FAIL: expected {} colors, got {}",
                HidPerKeyBackend::TOTAL_KEY_COUNT,
                colors.len()
            );
        }

        let mut g = self.inner.lock().await;
        if !g.hid_per_key.is_available() {
            return "FAIL: no HID per-key device".to_string();
        }

        let normalized: Vec<String> = colors
            .iter()
            .map(|c| c.trim_start_matches('#').to_uppercase())
            .collect();
        // If all keys were painted the exact same colour, keep config.colors[0..8] in sync so
        // switching back to Static or an animated effect inherits that colour cleanly (#385).
        if let Some(first) = normalized.first() {
            if normalized.iter().all(|c| c == first) {
                if g.config.colors.len() < 8 {
                    g.config.colors.resize(8, first.clone());
                }
                for slot in g.config.colors.iter_mut().take(8) {
                    *slot = first.clone();
                }
            }
        }
        g.config.per_key_colors = normalized;
        g.config.mode = "per_key_custom".to_string();
        g.config.power = true;
        g.config.hw_effects = false;
        g.dirty_state = true;
        g.dirty_save = true;
        g.last_save_request = Some(std::time::Instant::now());
        drop(g);
        self.wakeup.notify_one();
        "OK".to_string()
    }

    async fn start_per_key_wizard(&self) -> String {
        let wizard = self.inner.lock().await.wizard.clone();
        wizard.start_wizard().await
    }

    async fn light_key_index(&self, index: u32, hex_color: String) -> String {
        let wizard = self.inner.lock().await.wizard.clone();
        wizard.light_key_index(index as usize, &hex_color).await
    }

    async fn record_key_mapping(&self, index: u32, key_name: String) -> String {
        let wizard = self.inner.lock().await.wizard.clone();
        wizard.record_key_mapping(index as usize, &key_name).await
    }

    async fn export_keymap_report(&self) -> String {
        let wizard = self.inner.lock().await.wizard.clone();
        wizard.export_keymap().await
    }

    async fn save_per_key_map(&self, map_json: String) -> String {
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
            Err(e) => {
                warn!("SavePerKeyMap write error: {}", e);
                "FAIL".to_string()
            }
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

#[cfg(test)]
mod mcu_effect_tests {
    use super::*;

    fn backend(pid: Option<u16>) -> HidPerKeyBackend {
        HidPerKeyBackend::for_test(vec![], pid, false)
    }

    #[test]
    fn mcu_effects_only_on_darfon_not_primax_mcu() {
        let c = vec!["00FF00".to_string()];
        assert!(backend(Some(0x54BF))
            .mcu_effect_for("breathing", 50, "ltr", &c)
            .is_some());
        let primax = HidPerKeyBackend::for_test(vec![], Some(0x4F03), true);
        assert!(primax.mcu_effect_for("breathing", 50, "ltr", &c).is_none());
        assert!(backend(None)
            .mcu_effect_for("breathing", 50, "ltr", &c)
            .is_none());
    }

    #[test]
    fn mcu_effect_record_layout() {
        let c = vec!["00FF00".to_string()];
        let b = backend(Some(0x54BF));
        let r = b.mcu_effect_for("raindrop", 100, "ltr", &c).unwrap();
        assert_eq!((r[0], r[3], r[7]), (12, 2, 2));
        let w = b.mcu_effect_for("wave_rainbow", 100, "ltr", &c).unwrap();
        assert_eq!(
            &w[..10],
            &[0x0A, 0x05, 0x04, 0x02, 0x00, 0x03, 0x01, 0x02, 0x00, 0x00]
        );
        assert!(w[10..].iter().all(|&x| x == 0));
        assert_eq!(&r[24..27], &[0, 255, 0]);
        assert!(b.mcu_effect_for("reactive", 50, "ltr", &c).is_none());
        assert!(b.mcu_effect_for("wave", 50, "ltr", &c).is_none());
        assert!(b.mcu_effect_for("marquee", 50, "ltr", &c).is_none());
        assert!(b.mcu_effect_for("ripple", 50, "ltr", &c).is_none());
    }
}

#[cfg(test)]
mod lightbar_gate_tests {
    use super::hardware::lightbar_allowed_for;

    #[test]
    fn known_board_without_light_bar_is_not_written() {
        let caps = crate::capabilities::detect("878A", "", "");
        assert_eq!(caps.product_id, "878A");
        assert!(!lightbar_allowed_for(
            "878A",
            &caps,
            "OMEN Laptop 15-ek0xxx"
        ));
    }

    #[test]
    fn unknown_and_light_bar_boards_keep_previous_behaviour() {
        let unknown = crate::capabilities::detect("ZZZZ", "", "");
        assert!(lightbar_allowed_for("ZZZZ", &unknown, "Some HP Laptop"));
        let with_bar = crate::capabilities::detect("8D41", "", "");
        assert!(with_bar.has_light_bar);
        assert!(lightbar_allowed_for(
            "8D41",
            &with_bar,
            "OMEN MAX Gaming Laptop 16-ah0xxx"
        ));
        let mut odd = crate::capabilities::detect("878A", "", "");
        odd.has_light_bar = false;
        assert!(lightbar_allowed_for("878A", &odd, "OMEN Transcend 16"));
    }

    #[test]
    fn board_8edf_disables_wmi_rgb_hardware_detection() {
        let caps = crate::capabilities::detect("8EDF", "", "");
        let hw = super::RgbHardware::detect_with_caps("8EDF", &caps, true);
        assert!(!hw.available, "8EDF must never enable WMI RGB sysfs (#394)");
        assert!(!hw.lightbar_write_allowed());
    }
}

#[cfg(test)]
mod regression_rgb {
    use super::*;

    #[test]
    fn regression_darfon_static_pages_require_zero_blength() {
        let path = std::env::temp_dir().join(format!("omen-review-hid-{}", std::process::id()));
        std::fs::write(&path, []).unwrap();
        let h = HidPerKeyBackend::for_test(
            vec![path.to_string_lossy().to_string()],
            Some(0x54BF),
            false,
        );
        assert!(h.write_darfon_colors(&vec![(255, 255, 255); 104]));
        let bytes = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(path);
        assert_eq!(bytes.len(), 640);
        let packet = &bytes[bytes.len() - 64..];
        assert_eq!(
            &packet[..4],
            &[0x07, 0x02, 0x00, 0x00],
            "native static channel pages require BLength=0, despite carrying 60 data bytes"
        );
    }

    #[test]
    fn primax_report_framing_and_whitelist() {
        let pkt = HidPerKeyBackend::build_primax_report(0x80, 0x01, 0x00, &[0x12, 0x34]);
        assert_eq!(pkt.len(), 65);
        assert_eq!(&pkt[..7], &[0x00, 0x80, 0x01, 0x00, 0x00, 0x12, 0x34]);
        assert!(HidPerKeyBackend::is_primax_cmd_allowed(0x80));
        assert!(HidPerKeyBackend::is_primax_cmd_allowed(0x05));
        assert!(!HidPerKeyBackend::is_primax_cmd_allowed(0x03));
        assert!(!HidPerKeyBackend::is_primax_cmd_allowed(0x0C));
        assert!(!HidPerKeyBackend::is_primax_cmd_allowed(0x0A));
        assert!(!HidPerKeyBackend::is_primax_cmd_allowed(0x10));
    }

    #[test]
    fn primax_static_pages_cover_180_leds_with_zero_blength() {
        let path = std::env::temp_dir().join(format!("omen-primax-hid-{}", std::process::id()));
        std::fs::write(&path, [0u8; 65]).unwrap();
        let h = HidPerKeyBackend::for_test(
            vec![path.to_string_lossy().to_string()],
            Some(0x4F03),
            true,
        );
        assert!(h.write_primax_colors(&vec![(255, 128, 64); 104]));
        let bytes = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(path);
        assert_eq!(bytes.len(), 650);
        let packet = &bytes[bytes.len() - 65..];
        assert_eq!(&packet[..5], &[0x00, 0x07, 0x02, 0x00, 0x00]);
        assert_eq!(packet[5], 64);
        assert_eq!(packet[5 + 46], 64);
        assert_eq!(packet[64], 0);
    }

    #[tokio::test]
    async fn hw_effects_on_primax_yields_to_native_fn_without_writing_reports() {
        let path = std::env::temp_dir().join(format!("omen-primax-hw-{}", std::process::id()));
        let sentinel = [0xAAu8; 65];
        std::fs::write(&path, sentinel).unwrap();
        let cfg = RgbConfig {
            hw_effects: true,
            power: true,
            mode: "static".to_string(),
            ..Default::default()
        };
        let inner = Arc::new(Mutex::new(RgbInner {
            evdev_monitor: crate::evdev_monitor::EvdevMonitor::new(),
            hw: RgbHardware {
                driver_path: None,
                is_new_driver: false,
                zone_count: 0,
                available: false,
                board_id: "TEST".into(),
            },
            hid_per_key: Arc::new(HidPerKeyBackend::for_test(
                vec![path.to_string_lossy().to_string()],
                Some(0x4F03),
                true,
            )),
            config: cfg,
            last_lightbar_level: -1,
            calibrating: false,
            calibration_until: None,
            wizard: Arc::new(crate::hid_wizard::HidPerKeyWizard::new()),
            desktop_rgb: Arc::new(crate::desktop_rgb::DesktopRgbController::new()),
            dirty_state: false,
            dirty_lightbar: false,
            dirty_save: false,
            last_save_request: None,
        }));

        RgbService::apply_state(inner).await;
        let after = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            after, sentinel,
            "hw_effects on Primax must yield to native Fn preset without writing HID reports"
        );
    }

    #[tokio::test]
    async fn static_mode_on_per_key_applies_primary_color_uniformly_without_mint_bleed() {
        let path = std::env::temp_dir().join(format!("omen-static-uniform-{}", std::process::id()));
        std::fs::write(&path, [0u8; 65]).unwrap();
        let cfg = RgbConfig {
            mode: "static".to_string(),
            hw_effects: false,
            power: true,
            brightness: 100,
            colors: vec![
                "FF0000".into(),
                "9900FF".into(),
                "0066FF".into(),
                "00FFAA".into(),
                "0099ED".into(),
                "0099ED".into(),
                "0099ED".into(),
                "0099ED".into(),
            ],
            ..Default::default()
        };
        let inner = Arc::new(Mutex::new(RgbInner {
            evdev_monitor: crate::evdev_monitor::EvdevMonitor::new(),
            hw: RgbHardware {
                driver_path: None,
                is_new_driver: false,
                zone_count: 0,
                available: false,
                board_id: "TEST".into(),
            },
            hid_per_key: Arc::new(HidPerKeyBackend::for_test(
                vec![path.to_string_lossy().to_string()],
                Some(0x4F03),
                true,
            )),
            config: cfg,
            last_lightbar_level: -1,
            calibrating: false,
            calibration_until: None,
            wizard: Arc::new(crate::hid_wizard::HidPerKeyWizard::new()),
            desktop_rgb: Arc::new(crate::desktop_rgb::DesktopRgbController::new()),
            dirty_state: false,
            dirty_lightbar: false,
            dirty_save: false,
            last_save_request: None,
        }));

        RgbService::apply_state(inner).await;
        let bytes = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let b_page2 = &bytes[bytes.len() - 65..];
        // Primary color is FF0000 -> Blue channel MUST be 0 on Right arrow (LED 166 => byte 5+46=51), NOT 0xAA!
        assert_eq!(
            b_page2[5 + 46],
            0,
            "Static mode must not bleed colors[3] (#00FFAA) onto right-side keys"
        );
    }

    #[test]
    fn regression_batch_preserves_eight_zone_payload() {
        let dir = std::env::temp_dir().join(format!("omen-review-zones-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("all_zones"), "").unwrap();
        let h = RgbHardware {
            driver_path: Some(dir.to_string_lossy().to_string()),
            is_new_driver: false,
            zone_count: 8,
            available: true,
            board_id: "TEST".into(),
        };
        let colors = (0..8).map(|i| format!("{:06X}", i + 1)).collect::<Vec<_>>();
        h.write_zones_batch(&colors);
        let payload = std::fs::read_to_string(dir.join("all_zones")).unwrap();
        let _ = std::fs::remove_dir_all(dir);
        assert_eq!(
            payload.split_whitespace().count(),
            8,
            "adding all_zones must not drop zones 4-7"
        );
    }

    #[test]
    fn rgb_config_preserves_desktop_rgb_slots_10_to_16() {
        let mut colors = vec!["112233".to_string(); 17];
        colors[10] = "AABBCC".to_string();
        colors[16] = "DDEEFF".to_string();
        let json = serde_json::json!({
            "mode": "static",
            "colors": colors,
            "speed": 50,
            "brightness": 100,
            "direction": "ltr",
            "power": true,
            "win_lock": false
        })
        .to_string();
        let parsed = RgbConfig::parse_and_sanitize(&json);
        assert_eq!(parsed.colors.len(), 17);
        assert_eq!(parsed.colors[10], "AABBCC");
        assert_eq!(parsed.colors[16], "DDEEFF");
    }

    #[test]
    fn lightbar_zones_swap_segments_0_and_2_only_for_8e9a() {
        let dir = std::env::temp_dir().join(format!("omen-lb-8e9a-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("all_zones"), "").unwrap();

        let h_8e9a = RgbHardware {
            driver_path: Some(dir.to_string_lossy().to_string()),
            is_new_driver: false,
            zone_count: 4,
            available: true,
            board_id: "8E9A".into(),
        };
        let cols = vec![
            "111111".into(),
            "222222".into(),
            "333333".into(),
            "444444".into(),
        ];
        h_8e9a.write_lightbar_zones(&cols);
        let p_8e9a = std::fs::read_to_string(dir.join("all_zones")).unwrap();

        let h_8d88 = RgbHardware {
            driver_path: Some(dir.to_string_lossy().to_string()),
            is_new_driver: false,
            zone_count: 4,
            available: true,
            board_id: "8D88".into(),
        };
        h_8d88.write_lightbar_zones(&cols);
        let p_8d88 = std::fs::read_to_string(dir.join("all_zones")).unwrap();

        let _ = std::fs::remove_dir_all(dir);
        assert_eq!(p_8e9a, "333333 222222 111111 444444");
        assert_eq!(p_8d88, "111111 222222 333333 444444");
    }

    #[test]
    fn wall_clock_step_and_speed_only_update_behave_smoothly() {
        // Wall-clock advancement clamps huge dt (e.g., resuming from park) to 0.12s.
        let s0 = advance_anim_step(0.0, 100, 0.028);
        assert!((s0 - 0.14).abs() < 1e-6);
        let s_clamped = advance_anim_step(1.0, 100, 5.0);
        assert!((s_clamped - 1.60).abs() < 1e-6);

        // Speed-only change while streaming same software mode avoids state/cache reset.
        assert!(is_speed_only_update("wave", "wave", true, false, false));
        assert!(!is_speed_only_update("wave", "rainbow", true, false, false));
        assert!(!is_speed_only_update(
            "static", "static", true, false, false
        ));
        assert!(!is_speed_only_update(
            "per_key_custom",
            "per_key_custom",
            true,
            false,
            false
        ));
        assert!(!is_speed_only_update("wave", "wave", false, false, false));
        assert!(!is_speed_only_update("wave", "wave", true, true, false));
        assert!(!is_speed_only_update("wave", "wave", true, false, true));

        // Frame cadences match hardware capabilities.
        assert_eq!(target_frame_ms(true, true, false), 25);
        assert_eq!(target_frame_ms(false, true, false), 33);
        assert_eq!(target_frame_ms(false, false, true), 40);
        assert_eq!(target_frame_ms(false, false, false), 120);
    }

    #[tokio::test]
    async fn set_mode_same_streaming_mode_updates_speed_without_dirty_state() {
        let inner = Arc::new(Mutex::new(RgbInner {
            evdev_monitor: crate::evdev_monitor::EvdevMonitor::new(),
            hw: RgbHardware {
                driver_path: None,
                is_new_driver: false,
                zone_count: 4,
                available: true,
                board_id: "TEST".into(),
            },
            hid_per_key: Arc::new(HidPerKeyBackend::for_test(vec![], None, false)),
            config: RgbConfig {
                mode: "wave".to_string(),
                speed: 50,
                power: true,
                hw_effects: false,
                ..Default::default()
            },
            last_lightbar_level: -1,
            calibrating: false,
            calibration_until: None,
            wizard: Arc::new(crate::hid_wizard::HidPerKeyWizard::new()),
            desktop_rgb: Arc::new(crate::desktop_rgb::DesktopRgbController::new()),
            dirty_state: false,
            dirty_lightbar: false,
            dirty_save: false,
            last_save_request: None,
        }));
        let svc = RgbService {
            inner: inner.clone(),
            wakeup: Arc::new(tokio::sync::Notify::new()),
        };

        assert_eq!(svc.set_mode("wave".to_string(), 85).await, "OK");
        {
            let g = inner.lock().await;
            assert_eq!(g.config.speed, 85);
            assert!(
                !g.dirty_state,
                "speed-only change must not reset state/cache"
            );
            assert!(g.dirty_save);
        }

        assert_eq!(svc.set_mode("rainbow".to_string(), 40).await, "OK");
        {
            let g = inner.lock().await;
            assert_eq!(g.config.mode, "rainbow");
            assert_eq!(g.config.speed, 40);
            assert!(g.dirty_state, "mode change must trigger state transition");
        }
    }

    #[tokio::test]
    async fn resource_idle_and_inactive_reactive_consume_zero_background_writes() {
        let path = std::env::temp_dir().join(format!(
            "omen-rgb-idle-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&path, []).unwrap();
        let inner = Arc::new(Mutex::new(RgbInner {
            evdev_monitor: crate::evdev_monitor::EvdevMonitor::new(),
            hw: RgbHardware {
                driver_path: None,
                is_new_driver: false,
                zone_count: 0,
                available: false,
                board_id: "TEST".into(),
            },
            hid_per_key: Arc::new(HidPerKeyBackend::for_test(
                vec![path.to_string_lossy().to_string()],
                Some(0x4F03),
                true,
            )),
            config: RgbConfig {
                mode: "static".to_string(),
                power: true,
                brightness: 100,
                hw_effects: false,
                ..Default::default()
            },
            last_lightbar_level: -1,
            calibrating: false,
            calibration_until: None,
            wizard: Arc::new(crate::hid_wizard::HidPerKeyWizard::new()),
            desktop_rgb: Arc::new(crate::desktop_rgb::DesktopRgbController::new()),
            dirty_state: true,
            dirty_lightbar: false,
            dirty_save: false,
            last_save_request: None,
        }));
        let wakeup = Arc::new(tokio::sync::Notify::new());
        let handle = tokio::spawn(RgbService::rgb_engine_loop(inner.clone(), wakeup.clone()));

        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        std::fs::write(&path, []).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert_eq!(
            std::fs::metadata(&path).unwrap().len(),
            0,
            "static mode must park with zero background writes"
        );

        // Switch to reactive with empty recent_keys -> should settle all-off, then park at 0 writes
        {
            let mut g = inner.lock().await;
            g.config.mode = "reactive".to_string();
            g.dirty_state = true;
        }
        wakeup.notify_one();
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        std::fs::write(&path, []).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(220)).await;
        assert_eq!(
            std::fs::metadata(&path).unwrap().len(),
            0,
            "idle reactive mode with no recent keys must park with zero wakeups/writes"
        );

        // Inject a keypress -> engine must wake immediately via Notify and render a frame
        let (kx, ky) = super::key_layout::slot_coords(0);
        let key_wakeup = {
            let g = inner.lock().await;
            g.evdev_monitor
                .recent_keys
                .lock()
                .await
                .push(crate::evdev_monitor::KeyEventInfo {
                    x: kx,
                    y: ky,
                    timestamp: std::time::Instant::now(),
                });
            g.evdev_monitor.wakeup_handle()
        };
        key_wakeup.notify_one();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let reactive_bytes = std::fs::metadata(&path).unwrap().len();
        println!(
            "RGB_IDLE_PARK_BENCH: static_idle_bytes=0 reactive_idle_bytes=0 keypress_wakeup_bytes={}",
            reactive_bytes
        );
        assert!(
            reactive_bytes > 0,
            "keypress wakeup must emit reactive frame promptly"
        );

        handle.abort();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn resource_streaming_cpu_time_stays_under_strict_budget() {
        fn thread_cpu_us() -> u64 {
            let mut usage = unsafe { std::mem::zeroed::<libc::rusage>() };
            unsafe { libc::getrusage(libc::RUSAGE_THREAD, &mut usage) };
            let u = usage.ru_utime.tv_sec as u64 * 1_000_000 + usage.ru_utime.tv_usec as u64;
            let s = usage.ru_stime.tv_sec as u64 * 1_000_000 + usage.ru_stime.tv_usec as u64;
            u + s
        }

        fn process_rss_kb() -> u64 {
            std::fs::read_to_string("/proc/self/status")
                .ok()
                .and_then(|s| {
                    s.lines()
                        .find(|l| l.starts_with("VmRSS:"))
                        .and_then(|l| l.split_whitespace().nth(1))
                        .and_then(|v| v.parse::<u64>().ok())
                })
                .unwrap_or(0)
        }

        let lamps: Vec<super::lamparray::LampInfo> = (0..120)
            .map(|i| super::lamparray::LampInfo {
                id: i as u16,
                x: (i % 20) as u32 * 1000,
                y: (i / 20) as u32 * 1000,
                binding: 0,
            })
            .collect();
        let slot_map = LampArrayBackend::build_lamp_slot_map(&lamps, true);
        let mut prev = Vec::with_capacity(120);
        let mut scratch_colors = Vec::with_capacity(120);
        let mut scratch_reports = Vec::with_capacity(16);

        let primax = HidPerKeyBackend::for_test(vec!["/dev/null".to_string()], Some(0x4F03), true);
        let modes = ["wave_rainbow", "ripple", "starlight", "raindrop"];
        let sample_keys = [(4.0, 2.0, 0.08), (7.0, 3.0, 0.22), (10.0, 1.0, 0.35)];

        let cpu_start = thread_cpu_us();
        let mut total_reports = 0usize;
        for f in 0..300usize {
            let mode = modes[f % modes.len()];
            let frame = render_per_key_rgb(
                mode,
                f as f64 * 0.12,
                65,
                "ltr",
                (0, 180, 255),
                (255, 0, 128),
                1.0,
                &sample_keys,
            );
            LampArrayBackend::fill_gui_colors_into(
                lamps.len(),
                &slot_map,
                &frame,
                104,
                &mut scratch_colors,
            );
            LampArrayBackend::diff_frame_reports_into(
                &lamps,
                &mut prev,
                &scratch_colors,
                &mut scratch_reports,
            );
            total_reports += scratch_reports.len();
            let _ = primax.write_per_key_colors(&frame);
        }
        let cpu_us = thread_cpu_us().saturating_sub(cpu_start);
        let per_frame_us = cpu_us as f64 / 300.0;
        let fps30_cpu_pct = (per_frame_us / 33_333.0) * 100.0;
        let rss_kb = process_rss_kb();
        println!(
            "RGB_PIPELINE_BENCH: frames=300 total_cpu_us={} per_frame_us={:.2} fps30_single_core_pct={:.4}% reports={} rss_kb={}",
            cpu_us, per_frame_us, fps30_cpu_pct, total_reports, rss_kb
        );
        assert!(
            fps30_cpu_pct < 5.0,
            "streaming RGB pipeline CPU exceeds strict budget: {:.3}%",
            fps30_cpu_pct
        );
    }
}
