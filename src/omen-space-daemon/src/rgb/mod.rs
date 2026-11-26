mod effects;
mod hardware;
mod hid_per_key;
pub mod key_layout;
mod lamparray;

use effects::{compute_anim_color, render_per_key_frame, should_stream_software_frame};
pub use hardware::{parse_hex_color, scale_hex_color, RgbHardware};
use hid_per_key::HidPerKeyBackend;
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
    hid_per_key: HidPerKeyBackend,
    config: RgbConfig,
    anim_step: f64,
    lightbar_step: f64,
    last_lightbar_level: i32,
    calibrating: bool,
    calibration_until: Option<std::time::Instant>,
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
            info!(
                "RGB: Driver at {:?} (new_driver={}, zone_count={})",
                hw.driver_path, hw.is_new_driver, hw.zone_count
            );
        } else {
            warn!("RGB: No RGB hardware driver found");
        }

        let hid_per_key = HidPerKeyBackend::new();
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
            anim_step: 0.0,
            lightbar_step: 0.0,
            last_lightbar_level: -1,
            calibrating: false,
            calibration_until: None,
            mcu_sig: None,
            wizard,
            evdev_monitor,
            desktop_rgb,
        }));

        let svc = Self {
            inner: inner.clone(),
        };

        Self::apply_state(inner.clone()).await;

        let deferred_inner = inner.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            Self::apply_state(deferred_inner).await;
            info!("RGB: deferred re-apply completed");
        });

        let anim_inner = inner.clone();
        tokio::spawn(async move {
            Self::software_animation_loop(anim_inner).await;
        });

        let lb_inner = inner.clone();
        tokio::spawn(async move {
            Self::lightbar_animation_loop(lb_inner).await;
        });

        let uleds_inner = inner.clone();
        tokio::spawn(async move {
            Self::uleds_monitor_loop(uleds_inner).await;
        });

        Ok(svc)
    }

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
                    let mut should_apply = false;
                    {
                        let mut g = inner.lock().await;
                        let clamped = new_brightness.clamp(0, 100) as u32;
                        if g.config.brightness != clamped || (clamped > 0 && !g.config.power) {
                            g.config.brightness = clamped;
                            g.config.power = clamped > 0;
                            g.config.save();
                            should_apply = true;
                        }
                    }
                    if should_apply {
                        Self::apply_state(inner.clone()).await;
                    }
                }
                Err(e) => {
                    warn!("uleds read error: {}, stopping listener", e);
                    break;
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
            let (r1, g1, b1) =
                parse_hex_color(colors.first().map(|s| s.as_str()).unwrap_or("FF0000"));
            let (r2, g2, b2) =
                parse_hex_color(colors.get(1).map(|s| s.as_str()).unwrap_or("0000FF"));
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
            if g.calibrating {
                if let Some(until) = g.calibration_until {
                    if std::time::Instant::now() < until {
                        drop(g);
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                        continue;
                    }
                }
                g.calibrating = false;
                g.calibration_until = None;
            }

            let has_per_key = g.hid_per_key.is_available();
            let has_old_sysfs = g.hw.available && !g.hw.is_new_driver;

            let is_interactive = g.config.power
                && has_per_key
                && (g.config.mode == "reactive" || g.config.mode == "ripple");
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

            // Hardware effects (#322, #355): let the keyboard MCU animate supported modes,
            // but fall through cleanly to software frame streaming when the selected mode has
            // no MCU opcode (e.g. marquee, reactive, ripple, wave, blinking — #385).
            if has_per_key {
                let mcu_rec = if g.config.hw_effects {
                    g.hid_per_key
                        .mcu_effect_for(&mode, speed, &direction, &colors)
                } else {
                    None
                };
                if !should_stream_software_frame(g.config.hw_effects, mcu_rec.is_some()) {
                    if let Some(rec) = mcu_rec {
                        let mut sig = rec.to_vec();
                        if g.hid_per_key.is_primax_mcu() {
                            sig.push(brightness.clamp(1, 100) as u8);
                        }
                        if g.mcu_sig.as_ref() != Some(&sig)
                            && g.hid_per_key.send_mcu_effect_with_brightness(&rec, brightness)
                        {
                            g.mcu_sig = Some(sig);
                        }
                    }
                    last_per_key_colors.clear();
                    drop(g);
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    continue;
                }
                if g.mcu_sig.take().is_some() {
                    g.hid_per_key.send_enter_per_key_mode(brightness);
                }
            } else {
                g.mcu_sig = None;
            }

            let tick_ms: u64 = if !has_per_key && has_old_sysfs {
                150
            } else {
                50
            };
            let step_inc = (speed as f64 / 100.0) * 0.25 * (tick_ms as f64 / 50.0);
            g.anim_step += step_inc;
            let step = g.anim_step;

            let primary = parse_hex_color(colors.first().map(|s| s.as_str()).unwrap_or("FF0000"));
            let secondary = parse_hex_color(colors.get(1).map(|s| s.as_str()).unwrap_or("0000FF"));

            if has_per_key {
                let recent_keys = {
                    let mut keys_guard = g.evdev_monitor.recent_keys.lock().await;
                    let now = std::time::Instant::now();
                    keys_guard.retain(|k| now.duration_since(k.timestamp).as_secs_f64() <= 2.5);
                    keys_guard
                        .iter()
                        .map(|k| (k.x, k.y, now.duration_since(k.timestamp).as_secs_f64()))
                        .collect::<Vec<_>>()
                };

                let out_colors = render_per_key_frame(
                    &mode,
                    step,
                    &direction,
                    primary,
                    secondary,
                    scaler,
                    &recent_keys,
                );

                if out_colors != last_per_key_colors {
                    let frame: Vec<_> = out_colors.iter().map(|c| parse_hex_color(c)).collect();
                    g.hid_per_key.write_per_key_colors(&frame);
                    last_per_key_colors = out_colors;
                }
            }

            if has_old_sysfs && !g.hid_per_key.is_lamparray() && !g.hid_per_key.is_primax_mcu() {
                let zone_count = g.hw.zone_count as usize;
                let mut current_frame = Vec::with_capacity(zone_count);
                for i in 0..zone_count {
                    let eff_idx = if direction == "ltr" {
                        i
                    } else {
                        zone_count - 1 - i
                    };
                    let (r, g_c, b) = compute_anim_color(
                        &mode,
                        step,
                        eff_idx,
                        zone_count,
                        primary.0,
                        primary.1,
                        primary.2,
                        secondary.0,
                        secondary.1,
                        secondary.2,
                    );
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

    async fn apply_lightbar(inner: Arc<Mutex<RgbInner>>) {
        let mut g = inner.lock().await;
        if !g.hw.available || !g.hw.lightbar_write_allowed() {
            return;
        }
        let effect_active = g.config.lightbar_mode != "static"
            && g.config.lightbar_power
            && g.config.lightbar_brightness > 0;
        if !effect_active {
            let lb_colors: Vec<String> =
                if !g.config.lightbar_power || g.config.lightbar_brightness == 0 {
                    vec!["000000".to_string(); 4]
                } else {
                    let scaler = (g.config.lightbar_brightness as f64 / 100.0).clamp(0.0, 1.0);
                    (0..4)
                        .map(|i| {
                            let raw = g
                                .config
                                .lightbar_colors
                                .get(i)
                                .cloned()
                                .unwrap_or_else(|| "000000".to_string());
                            scale_hex_color(&raw, scaler)
                        })
                        .collect()
                };
            if g.hid_per_key.is_lamparray() && g.hw.has_lightbar() {
                g.hw.write_lightbar_zones(&lb_colors);
            } else {
                g.hw.write_lightbar(&lb_colors);
            }
        }
        if g.hid_per_key.is_lamparray() && g.hw.has_lightbar() {
            let level = if g.hw.is_new_driver {
                if g.config.lightbar_power {
                    g.config.lightbar_brightness as i32
                } else {
                    0
                }
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
            g.hw = RgbHardware::detect_with_override(g.config.zone_count_override);
        } else if let Some(ref path) = g.hw.driver_path {
            if !Path::new(path).exists() {
                g.hw = RgbHardware::detect_with_override(g.config.zone_count_override);
            }
        }

        g.mcu_sig = None;
        let power = g.config.power;
        let brightness = g.config.brightness;
        let mode = g.config.mode.clone();
        let speed = g.config.speed;
        let colors = g.config.colors.clone();

        if g.hid_per_key.is_available() {
            let mcu_anim_active = g.config.hw_effects
                && power
                && brightness > 0
                && g.hid_per_key
                    .mcu_effect_for(&mode, speed, &g.config.direction, &colors)
                    .is_some();
            if g.config.hw_effects && g.hid_per_key.is_lamparray() && mcu_anim_active {
                g.hid_per_key.release_lamparray();
            } else if mcu_anim_active && g.hid_per_key.is_primax_mcu() {
                // Handled by software_animation_loop via send_mcu_effect_with_brightness
            } else if !power || brightness == 0 {
                g.hid_per_key.send_enter_per_key_mode(0);
                g.hid_per_key
                    .set_zone_colors(&vec!["000000".to_string(); 8]);
            } else {
                g.hid_per_key.send_enter_per_key_mode(brightness);
                if mode == "static" {
                    let static_colors =
                        if g.hid_per_key.is_lamparray() || g.hid_per_key.is_primax_mcu() {
                            let scaler = (brightness as f64 / 100.0).clamp(0.0, 1.0);
                            colors.iter().map(|c| scale_hex_color(c, scaler)).collect()
                        } else {
                            colors.clone()
                        };
                    g.hid_per_key.set_zone_colors(&static_colors);
                } else if mode == "per_key_custom" {
                    let scaler = if g.hid_per_key.is_lamparray() || g.hid_per_key.is_primax_mcu() {
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

        if g.desktop_rgb.is_available() {
            if !power || brightness == 0 {
                let _ = g.desktop_rgb.set_static_colors(&[(0, 0, 0); 7], 0);
            } else if mode == "static" {
                let mut parsed_colors = Vec::new();
                for i in 0..7 {
                    let hex = colors
                        .get(10 + i)
                        .cloned()
                        .unwrap_or_else(|| colors.first().cloned().unwrap_or_else(|| "FF0000".to_string()));
                    parsed_colors.push(parse_hex_color(&hex));
                }
                let _ = g
                    .desktop_rgb
                    .set_static_colors(&parsed_colors, brightness as u8);
            }
        }

        if g.hw.available && g.hw.lightbar_write_allowed() {
            let effect_active = g.config.lightbar_mode != "static"
                && g.config.lightbar_power
                && g.config.lightbar_brightness > 0;
            if !effect_active {
                let lb_colors: Vec<String> =
                    if !g.config.lightbar_power || g.config.lightbar_brightness == 0 {
                        vec!["000000".to_string(); 4]
                    } else {
                        let scaler = (g.config.lightbar_brightness as f64 / 100.0).clamp(0.0, 1.0);
                        (0..4)
                            .map(|i| {
                                let raw = g
                                    .config
                                    .lightbar_colors
                                    .get(i)
                                    .cloned()
                                    .unwrap_or_else(|| "000000".to_string());
                                scale_hex_color(&raw, scaler)
                            })
                            .collect()
                    };
                if g.hid_per_key.is_lamparray() && g.hw.has_lightbar() {
                    g.hw.write_lightbar_zones(&lb_colors);
                } else {
                    g.hw.write_lightbar(&lb_colors);
                }
            }
        }

        if !g.hw.available {
            return;
        }

        if g.hid_per_key.is_lamparray() || g.hid_per_key.is_primax_mcu() {
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
            g.config.save();
        }

        Self::apply_state(self.inner.clone()).await;
        "OK".to_string()
    }

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
            g.config.save();
        }
        Self::apply_lightbar(self.inner.clone()).await;
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
            g.config.save();
        }
        Self::apply_lightbar(self.inner.clone()).await;
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
            g.config.save();
            g.hw = RgbHardware::detect_with_override(normalized);
            info!(
                "SetZoneCountOverride: override={:?}, effective zone_count={}",
                normalized, g.hw.zone_count
            );
        }
        Self::apply_state(self.inner.clone()).await;
        "OK".to_string()
    }

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
        g.calibration_until =
            Some(std::time::Instant::now() + std::time::Duration::from_secs(600));
        serde_json::json!({
            "status": "calibrating",
            "lamps": g.hid_per_key.lamp_array_count(),
        })
        .to_string()
    }

    async fn stop_lamp_calibration(&self) -> String {
        {
            let mut g = self.inner.lock().await;
            g.calibrating = false;
            g.calibration_until = None;
        }
        Self::apply_state(self.inner.clone()).await;
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
        g.calibration_until =
            Some(std::time::Instant::now() + std::time::Duration::from_secs(600));
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
        g.mcu_sig = None;
        g.config.save();

        let scaler = if g.hid_per_key.is_lamparray() || g.hid_per_key.is_primax_mcu() {
            (g.config.brightness as f64 / 100.0).clamp(0.0, 1.0)
        } else {
            1.0
        };

        let key_colors: Vec<(u8, u8, u8)> = g
            .config
            .per_key_colors
            .iter()
            .map(|hex| {
                let (r, g_c, b) = parse_hex_color(hex);
                (
                    (r as f64 * scaler) as u8,
                    (g_c as f64 * scaler) as u8,
                    (b as f64 * scaler) as u8,
                )
            })
            .collect();

        g.hid_per_key.send_enter_per_key_mode(g.config.brightness);
        if g.hid_per_key.write_per_key_colors(&key_colors) {
            info!(
                "SetPerKeyColors: wrote {} key colors via HID",
                key_colors.len()
            );
            "OK".to_string()
        } else {
            warn!("SetPerKeyColors: HID write failed");
            "FAIL: HID write error".to_string()
        }
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
        HidPerKeyBackend {
            hidraw_paths: vec![],
            device_pid: pid,
            lamparray: None,
            is_primax_mcu: false,
        }
    }

    #[test]
    fn mcu_effects_only_on_darfon_or_primax_mcu() {
        let c = vec!["00FF00".to_string()];
        assert!(backend(Some(0x54BF))
            .mcu_effect_for("breathing", 50, "ltr", &c)
            .is_some());
        let primax = HidPerKeyBackend {
            hidraw_paths: vec![],
            device_pid: Some(0x4F03),
            lamparray: None,
            is_primax_mcu: true,
        };
        assert!(primax
            .mcu_effect_for("breathing", 50, "ltr", &c)
            .is_some());
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
        assert!(!lightbar_allowed_for("878A", &caps, "OMEN Laptop 15-ek0xxx"));
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
        std::fs::write(&path, [0u8; 64]).unwrap();
        let h = HidPerKeyBackend {
            hidraw_paths: vec![path.to_string_lossy().to_string()],
            device_pid: Some(0x54BF),
            lamparray: None,
            is_primax_mcu: false,
        };
        assert!(h.write_darfon_colors(&vec![(255, 255, 255); 104]));
        let packet = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(path);
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
        assert!(HidPerKeyBackend::is_primax_cmd_allowed(0x03));
        assert!(!HidPerKeyBackend::is_primax_cmd_allowed(0x0A));
        assert!(!HidPerKeyBackend::is_primax_cmd_allowed(0x10));
    }

    #[test]
    fn primax_static_pages_cover_180_leds_with_zero_blength() {
        let path = std::env::temp_dir().join(format!("omen-primax-hid-{}", std::process::id()));
        std::fs::write(&path, [0u8; 65]).unwrap();
        let h = HidPerKeyBackend {
            hidraw_paths: vec![path.to_string_lossy().to_string()],
            device_pid: Some(0x4F03),
            lamparray: None,
            is_primax_mcu: true,
        };
        assert!(h.write_primax_colors(&vec![(255, 128, 64); 104]));
        let packet = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(path);
        assert_eq!(packet.len(), 65);
        assert_eq!(&packet[..5], &[0x00, 0x07, 0x02, 0x00, 0x00]);
        assert_eq!(packet[5], 64);
        assert_eq!(packet[64], 64);
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
}
