use crate::capabilities::ModelCapabilities;
use glob::glob;
use log::{info, warn};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct RgbHardware {
    pub driver_path: Option<String>,
    pub is_new_driver: bool,
    pub zone_count: u32,
    pub available: bool,
    pub board_id: String,
}

pub fn parse_hex_color(hex: &str) -> (u8, u8, u8) {
    let h = hex.trim_start_matches('#');
    if h.len() < 6 {
        return (255, 0, 0);
    }
    let r = u8::from_str_radix(&h[0..2], 16).unwrap_or(255);
    let g = u8::from_str_radix(&h[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&h[4..6], 16).unwrap_or(0);
    (r, g, b)
}

pub fn scale_hex_color(hex: &str, scaler: f64) -> String {
    let (r, g, b) = parse_hex_color(hex);
    format!(
        "{:02X}{:02X}{:02X}",
        ((r as f64) * scaler) as u8,
        ((g as f64) * scaler) as u8,
        ((b as f64) * scaler) as u8
    )
}

pub fn lightbar_allowed_for(board_id: &str, caps: &ModelCapabilities, product_name: &str) -> bool {
    if caps.disable_wmi_rgb {
        return false;
    }
    let known_board = caps.product_id.eq_ignore_ascii_case(board_id.trim());
    if !known_board || caps.has_light_bar {
        return true;
    }
    let prod = product_name.to_lowercase();
    prod.contains("desktop") || prod.contains("transcend") || prod.contains("max")
}

pub fn map_four_zone_index(board_id: &str, zone: usize) -> usize {
    if matches!(
        board_id.trim().to_uppercase().as_str(),
        "8EEF" | "8EDD" | "8EEC"
    ) {
        // OMEN HyperX 15/16 (boards 8EEF / 8EDD / 8EEC) wire their 4-zone WMI buffer in the
        // reverse order of classic OMEN boards: hw zone0=WASD, zone1=Left,
        // zone2=Middle, zone3=Right (#309, #335).
        match zone {
            0 => 1,
            1 => 2,
            2 => 3,
            3 => 0,
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
    pub fn detect_with_override(zone_override: Option<u32>) -> Self {
        let mut hw = Self::detect_raw();
        if !hw.available {
            return hw;
        }
        if !hw.is_new_driver {
            hw.zone_count = match zone_override {
                Some(n) if n == 1 || n == 4 || n == 8 => n,
                _ if hw.zone_count == 1 => 1,
                _ => 4,
            };
        }
        hw
    }

    pub fn detect_with_caps(board_id: &str, caps: &ModelCapabilities, allow_sysfs: bool) -> Self {
        if caps.disable_wmi_rgb || !allow_sysfs {
            return Self {
                driver_path: None,
                is_new_driver: false,
                zone_count: 0,
                available: false,
                board_id: board_id.to_string(),
            };
        }

        let new_path = "/sys/devices/platform/omen-rgb-keyboard/rgb_zones";
        let custom_path = "/sys/devices/platform/hp-omen-extra";
        let hp_path2 = "/sys/devices/platform/hp_omen_extra";
        let max_zones = if caps.has_four_zone_rgb { 4 } else { 8 };

        if Path::new(new_path).exists() {
            let mut zone_count =
                if Path::new("/sys/devices/platform/omen-rgb-keyboard/rgb_zones/zone04").exists() {
                    8
                } else {
                    4
                };
            if zone_count > max_zones {
                zone_count = max_zones;
            }
            return Self {
                driver_path: Some(new_path.to_string()),
                is_new_driver: true,
                zone_count,
                available: true,
                board_id: board_id.to_string(),
            };
        }
        for p in [custom_path, hp_path2] {
            if Path::new(p).exists() {
                let mut zone_count = if caps.rgb_zone_count == Some(1) {
                    1
                } else if caps.has_four_zone_rgb {
                    4
                } else if Path::new(&format!("{}/zone4", p)).exists()
                    || Path::new(&format!("{}/zone04", p)).exists()
                {
                    8
                } else {
                    4
                };
                if zone_count > max_zones {
                    zone_count = max_zones;
                }
                return Self {
                    driver_path: Some(p.to_string()),
                    is_new_driver: false,
                    zone_count,
                    available: true,
                    board_id: board_id.to_string(),
                };
            }
        }
        if let Ok(mut entries) = glob("/sys/class/leds/hp::kbd_backlight*") {
            if let Some(Ok(path)) = entries.next() {
                return Self {
                    driver_path: Some(path.to_string_lossy().to_string()),
                    is_new_driver: false,
                    zone_count: 4,
                    available: true,
                    board_id: board_id.to_string(),
                };
            }
        }
        Self {
            driver_path: None,
            is_new_driver: false,
            zone_count: 4,
            available: false,
            board_id: board_id.to_string(),
        }
    }

    fn detect_raw() -> Self {
        let board_id = std::fs::read_to_string("/sys/class/dmi/id/board_name")
            .unwrap_or_default()
            .trim()
            .to_uppercase();
        let prod_name =
            std::fs::read_to_string("/sys/class/dmi/id/product_name").unwrap_or_default();
        let specs = crate::sysmon::get_hardware_specs();
        let caps = crate::capabilities::detect(&board_id, prod_name.trim(), &specs.cpu_spec);
        if caps.disable_wmi_rgb {
            info!(
                "RGB: WMI RGB sysfs disabled for board {} (caps.disable_wmi_rgb=true, #394)",
                board_id
            );
        }
        Self::detect_with_caps(&board_id, &caps, true)
    }

    pub fn write_zone(&self, zone: usize, hex_color: &str) {
        let Some(ref base) = self.driver_path else {
            return;
        };
        if zone > 7 {
            return;
        }

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

    pub fn write_all(&self, hex_color: &str) {
        let Some(ref base) = self.driver_path else {
            return;
        };
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

    pub fn write_brightness(&self, value: u32) {
        let Some(ref base) = self.driver_path else {
            return;
        };
        let path = format!("{}/brightness", base);
        if !Path::new(&path).exists() {
            return;
        }
        let val_str = if self.is_new_driver {
            value.to_string()
        } else if value > 0 {
            "1".to_string()
        } else {
            "0".to_string()
        };
        let _ = std::fs::write(&path, val_str);
    }

    pub fn write_win_lock(&self, locked: bool) {
        let Some(ref base) = self.driver_path else {
            return;
        };
        let path = format!("{}/win_lock", base);
        if Path::new(&path).exists() {
            let _ = std::fs::write(&path, if locked { "1" } else { "0" });
        }
    }

    pub fn write_zones_batch(&self, colors: &[String]) {
        let Some(ref base) = self.driver_path else {
            return;
        };
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
                    mapped_colors[actual] = colors
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| "000000".to_string());
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

    pub fn write_lightbar(&self, colors: &[String]) {
        let Some(ref base) = self.driver_path else {
            return;
        };
        let lightbar_path = format!("{}/lightbar", base);
        if Path::new(&lightbar_path).exists() {
            let payload = colors
                .iter()
                .take(4)
                .map(|c| {
                    if c.eq_ignore_ascii_case("FFFFFF") {
                        "FFFFFE".to_string()
                    } else {
                        c.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
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

    /// On per-key OMEN MAX boards (8D88 / 8D42 / 8E9A) the WMI "zones" belong to the
    /// 4-segment front lightbar while the keyboard is driven over USB HID LampArray (#355).
    pub fn write_lightbar_zones(&self, colors: &[String]) {
        let Some(ref base) = self.driver_path else {
            return;
        };
        let format_color = |c: &String| {
            if c.eq_ignore_ascii_case("FFFFFF") {
                "FFFFFE".to_string()
            } else {
                c.clone()
            }
        };
        let mut ordered: Vec<String> = colors.iter().take(4).map(format_color).collect();
        if self.board_id.eq_ignore_ascii_case("8E9A") && ordered.len() >= 3 {
            ordered.swap(0, 2);
        }
        let payload = ordered.join(" ");
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

    pub fn lightbar_write_allowed(&self) -> bool {
        let prod_name =
            std::fs::read_to_string("/sys/class/dmi/id/product_name").unwrap_or_default();
        let caps = crate::capabilities::detect(&self.board_id, "", "");
        lightbar_allowed_for(&self.board_id, &caps, &prod_name)
    }

    pub fn has_lightbar(&self) -> bool {
        if !self.available {
            return false;
        }
        if let Some(ref base) = self.driver_path {
            if Path::new(&format!("{}/lightbar", base)).exists() {
                return true;
            }
        }
        Path::new("/sys/class/leds/hp::lightbar-0/multi_intensity").exists()
    }

    pub fn write_mode(&self, mode: &str, speed: u32) {
        if !self.is_new_driver {
            return;
        }
        let Some(ref base) = self.driver_path else {
            return;
        };
        let hw_mode = if mode == "cycle" { "rainbow" } else { mode };
        let _ = std::fs::write(format!("{}/animation_mode", base), hw_mode);
        let mapped_speed = (speed / 10).clamp(1, 10);
        let _ = std::fs::write(
            format!("{}/animation_speed", base),
            mapped_speed.to_string(),
        );
    }
}
