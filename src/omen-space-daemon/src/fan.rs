use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use zbus::interface;
use log::{info, warn};
use std::collections::HashMap;
use glob::glob;
use tokio::sync::Mutex;
use std::sync::Arc;
use crate::notifier::DesktopNotifier;
use std::sync::OnceLock;

static SENSOR_TEMP_PATHS: OnceLock<Vec<PathBuf>> = OnceLock::new();
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CurvePoint(pub f64, pub f64); // [Temp, Pct]

pub struct FanState {
    pub hwmon_path: Option<PathBuf>,
    pub found_fans: Vec<u32>,
    pub max_speeds: HashMap<u32, u32>,
    pub fallback_paths: HashMap<u32, PathBuf>,
    pub fan_count: u32,
    pub mode: String,
    pub custom_curve_json: String,
    pub last_targets: HashMap<u32, u32>,
    pub thermal_protection_active: bool,
    pub thermal_protection_enabled: bool,
    pub thermal_protection_entered_at: std::time::Instant,
    pub pre_protection_mode: Option<String>,
    /// When the user explicitly sets a percentage (e.g. "fan 50"),
    /// store it here so the monitor loop re-applies it as keep-alive
    /// instead of overwriting with a curve.
    pub manual_target_pct: Option<u32>,
    /// Last time we wrote a keep-alive for max mode
    pub last_keepalive: std::time::Instant,
    pub auto_fan_activated_at: Option<std::time::Instant>,
    pub temp_history: std::collections::VecDeque<f64>,
    pub last_auto_pct: u32,
    pub last_auto_pct_time: std::time::Instant,
    pub perf_cooldown_start: Option<std::time::Instant>,
    pub last_perf_pct: u32,
    pub last_perf_pct_time: std::time::Instant,
    pub last_power_profile_was_perf: bool,
    pub last_written_duty: Option<u32>,
    pub last_written_duty_time: Option<std::time::Instant>,
    pub last_hw_detect_attempt: std::time::Instant,
}

#[derive(Clone)]
pub struct FanService {
    state: Arc<Mutex<FanState>>,
}

// Helpers for sysfs using tokio::fs to avoid blocking the async executor
async fn sysfs_read_str<P: AsRef<Path>>(path: P) -> Option<String> {
    tokio::fs::read_to_string(path).await.ok().map(|s| s.trim().to_string())
}

async fn sysfs_read<P: AsRef<Path>>(path: P, default: i64) -> i64 {
    sysfs_read_str(path).await
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(default)
}

async fn sysfs_exists<P: AsRef<Path>>(path: P) -> bool {
    tokio::fs::try_exists(path).await.unwrap_or(false)
}

async fn sysfs_write<P: AsRef<Path>, S: AsRef<str>>(path: P, val: S) -> bool {
    tokio::fs::write(path, val.as_ref().as_bytes()).await.is_ok()
}

impl FanService {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let config_manager = crate::config::ConfigManager::new();
        let config = config_manager.load().await;

        let mut state = FanState {
            hwmon_path: None,
            found_fans: Vec::new(),
            max_speeds: HashMap::new(),
            fallback_paths: HashMap::new(),
            fan_count: 0,
            mode: config.fan_mode.clone(),
            custom_curve_json: config.custom_curve.clone(),
            last_targets: HashMap::new(),
            thermal_protection_active: false,
            thermal_protection_enabled: config.thermal_protection_enabled,
            thermal_protection_entered_at: std::time::Instant::now(),
            pre_protection_mode: None,
            manual_target_pct: None,
            last_keepalive: std::time::Instant::now(),
            auto_fan_activated_at: None,
            temp_history: std::collections::VecDeque::new(),
            last_auto_pct: 0,
            last_auto_pct_time: std::time::Instant::now(),
            perf_cooldown_start: None,
            last_perf_pct: 0,
            last_perf_pct_time: std::time::Instant::now(),
            last_power_profile_was_perf: false,
            last_written_duty: None,
            last_written_duty_time: None,
            last_hw_detect_attempt: std::time::Instant::now(),
        };
        Self::detect_hardware(&mut state).await;
        
        // Read current hardware mode
        let mut hw_mode = "auto".to_string();
        if let Some(ref hwmon) = state.hwmon_path {
            let pwm1_enable = hwmon.join("pwm1_enable");
            if sysfs_exists(&pwm1_enable).await {
                let val = sysfs_read(&pwm1_enable, 2).await;
                hw_mode = match val {
                    0 => "max".to_string(),
                    1 => "custom".to_string(),
                    _ => "auto".to_string(),
                };
            }
        }
        state.mode = hw_mode;
        
        // Apply saved config if different from hardware state
        if state.hwmon_path.is_some()
            && state.mode != config.fan_mode {
                Self::set_mode_internal(&mut state, &config.fan_mode).await;
            }
        
        let service = Self {
            state: Arc::new(Mutex::new(state)),
        };
        
        // Spawn the monitor loop
        let service_clone = service.clone();
        tokio::spawn(async move {
            service_clone.run_monitor_loop().await;
        });
        
        Ok(service)
    }

    async fn _find_hwmon() -> Option<PathBuf> {
        if let Ok(entries) = glob("/sys/class/hwmon/hwmon*/name") {
            for entry in entries.filter_map(Result::ok) {
                if let Some(name) = sysfs_read_str(&entry).await {
                    // Added hp_wmi to cover broader matches
                    if name == "hp" || name == "hp-omen" || name == "hp_wmi" {
                        if let Some(parent) = entry.parent() {
                            info!("Found HP/OMEN hwmon at {:?} (driver={})", parent, name);
                            return Some(parent.to_path_buf());
                        }
                    }
                }
            }
        }

        for platform_name in &["hp-wmi", "hp_wmi", "hp-omen"] {
            let platform_hwmon = format!("/sys/devices/platform/{}/hwmon", platform_name);
            if sysfs_exists(&platform_hwmon).await {
                if let Ok(mut entries) = tokio::fs::read_dir(&platform_hwmon).await {
                    let mut dirs = Vec::new();
                    while let Ok(Some(dir)) = entries.next_entry().await {
                        dirs.push(dir.path());
                    }
                    dirs.sort();
                    if let Some(first) = dirs.first() {
                        info!("Found HP hwmon via platform device at {:?}", first);
                        return Some(first.clone());
                    }
                }
            }
        }
        warn!("No HP hwmon device found");
        None
    }

    async fn _find_fallback_path(hwmon_path: &Path, fan_num: u32) -> Option<PathBuf> {
        if let Ok(entries) = glob("/sys/class/hwmon/hwmon*/fan*_input") {
            for entry in entries.filter_map(Result::ok) {
                if let Some(parent) = entry.parent() {
                    if parent == hwmon_path {
                        continue;
                    }
                }
                if let Some(file_name) = entry.file_name().and_then(|s| s.to_str()) {
                    let idx = file_name.replace("fan", "").replace("_input", "");
                    if idx == fan_num.to_string() {
                        return Some(entry);
                    }
                }
            }
        }
        None
    }

    async fn detect_hardware(state: &mut FanState) {
        state.found_fans.clear();
        state.max_speeds.clear();
        state.fallback_paths.clear();
        state.fan_count = 0;
        state.hwmon_path = Self::_find_hwmon().await;
        
        if let Some(ref hwmon) = state.hwmon_path {
            if let Ok(mut entries) = tokio::fs::read_dir(hwmon).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    if let Ok(file_name) = entry.file_name().into_string() {
                        if file_name.starts_with("fan") && file_name.ends_with("_input") {
                            let num_str = &file_name[3..file_name.len() - 6];
                            if let Ok(num) = num_str.parse::<u32>() {
                                state.found_fans.push(num);
                                if let Some(fallback) = Self::_find_fallback_path(hwmon, num).await {
                                    state.fallback_paths.insert(num, fallback);
                                }
                            }
                        }
                    }
                }
            }
            state.found_fans.sort();
            state.fan_count = state.found_fans.len() as u32;

            for &i in &state.found_fans {
                let max_path = hwmon.join(format!("fan{}_max", i));
                let mut max_val = sysfs_read(&max_path, 6000).await as u32;
                if max_val < 4000 {
                    info!("Sysfs fan{} max speed is unusually low ({}). Enforcing 6000 RPM safe limit.", i, max_val);
                    max_val = 6000;
                }
                state.max_speeds.insert(i, max_val);
            }

            let pwm_path = hwmon.join("pwm1_enable");
            let val = sysfs_read(&pwm_path, 2).await;
            state.mode = match val {
                0 => "max".to_string(),
                1 => "custom".to_string(),
                _ => "auto".to_string(),
            };
        }
    }

    fn should_retry_hw_detect(hwmon_available: bool, fan_count: u32, elapsed: std::time::Duration) -> bool {
        elapsed >= std::time::Duration::from_secs(5) && (!hwmon_available || fan_count == 0)
    }

    #[allow(dead_code)]
    pub fn evaluate_spline(points: &[CurvePoint], temp: f64) -> f64 {
        if points.is_empty() { return 0.0; }
        if temp <= points[0].0 { return points[0].1; }
        if let Some(last) = points.last() {
            if temp >= last.0 { return last.1; }
        }
        for i in 0..points.len() - 1 {
            let x0 = points[i].0; let y0 = points[i].1;
            let x1 = points[i + 1].0; let y1 = points[i + 1].1;
            if temp >= x0 && temp <= x1 {
                return y0 + (y1 - y0) * ((temp - x0) / (x1 - x0));
            }
        }
        points.last().map(|p| p.1).unwrap_or(0.0)
    }

    pub fn evaluate_step(points: &[CurvePoint], temp: f64) -> f64 {
        if points.is_empty() { return 0.0; }
        if temp < points[0].0 { return points[0].1; }
        let mut out = points[0].1;
        for p in points {
            if temp >= p.0 {
                out = p.1;
            } else {
                break;
            }
        }
        out
    }

    async fn get_max_temp() -> f64 {
        tokio::task::spawn_blocking(|| {
            let paths = SENSOR_TEMP_PATHS.get_or_init(|| {
                let mut p = Vec::new();
                if let Ok(entries) = glob("/sys/class/hwmon/hwmon*/temp*_input") {
                    for entry in entries.filter_map(Result::ok) {
                        let is_dgpu = if let Some(parent) = entry.parent() {
                            let name = std::fs::read_to_string(parent.join("name")).unwrap_or_default();
                            let name = name.trim().to_lowercase();
                            if name.contains("nvidia") || name.contains("nouveau") {
                                true
                            } else {
                                // Match PCI display controller (class 0x03*) from NVIDIA (vendor 0x10de)
                                let vendor = std::fs::read_to_string(parent.join("device/vendor")).unwrap_or_default();
                                let class = std::fs::read_to_string(parent.join("device/class")).unwrap_or_default();
                                vendor.trim().eq_ignore_ascii_case("0x10de") && class.trim().starts_with("0x03")
                            }
                        } else {
                            false
                        };

                        if !is_dgpu {
                            p.push(entry);
                        }
                    }
                }
                p
            });

            let mut max_temp = 45.0;
            // 1. Read CPU / motherboard sensors via standard hwmon
            for entry in paths {
                if let Ok(val_str) = std::fs::read_to_string(entry) {
                    if let Ok(milli) = val_str.trim().parse::<f64>() {
                        let temp = milli / 1000.0;
                        if temp > max_temp && temp < 150.0 {
                            max_temp = temp;
                        }
                    }
                }
            }

            // 2. Factor in dGPU temperature safely (returns 0.0 when sleeping or cooling down)
            let gpu_temp = crate::sysmon::get_safe_gpu_temp();
            if gpu_temp > max_temp && gpu_temp < 150.0 {
                max_temp = gpu_temp;
            }

            max_temp
        })
        .await
        .unwrap_or(45.0)
    }

    /// Convert a percentage (0-100) to a PWM duty cycle value (0-255)
    fn pct_to_pwm(pct: u32) -> u32 {
        let pct = pct.clamp(0, 100);
        let pwm = ((pct as f64 * 255.0) / 100.0).round() as u32;
        pwm.clamp(0, 255)
    }

    /// Write PWM duty cycle to hwmon. This sets both the mode (pwm1_enable=1)
    /// and the duty value (pwm1=0..255), matching OmenCore's SetHwmonPwmDutyPercent.
    async fn write_pwm_duty(state: &mut FanState, pct: u32) -> bool {
        let hwmon = match state.hwmon_path {
            Some(ref h) => h.clone(),
            None => return false,
        };

        let pwm_enable_path = hwmon.join("pwm1_enable");
        let pwm_path = hwmon.join("pwm1");

        // If pct is 100, use hardware max fan boost (pwm1_enable = 0) to bypass EC RPM limits
        let target_enable = if pct == 100 { 0 } else { 1 };

        if sysfs_exists(&pwm_enable_path).await {
            let current = sysfs_read(&pwm_enable_path, 2).await;
            if current != target_enable {
                if target_enable == 1 && current == 0 {
                    // Transition through EC hardware control (2) to clear stuck max mode
                    let _ = sysfs_write(&pwm_enable_path, "2").await;
                    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                }
                if !sysfs_write(&pwm_enable_path, &target_enable.to_string()).await {
                    warn!("Failed to set pwm1_enable={} for fan control", target_enable);
                    return false;
                }
            }
        }

        if target_enable == 0 {
            // In max fan boost, we don't need to write the PWM duty cycle
            state.last_written_duty = Some(255);
            state.last_written_duty_time = Some(std::time::Instant::now());
            let max_speed = state.max_speeds.values().max().copied().unwrap_or(6000);
            for &fan_num in &state.found_fans.clone() {
                state.last_targets.insert(fan_num, max_speed);
            }
            return true;
        }

        // Write duty cycle
        let duty = Self::pct_to_pwm(pct);
        // Minimum duty to actually spin the fan (avoid stall)
        let duty = if duty > 0 && duty < 50 { 50 } else { duty };

        // Prevent I/O flooding: only write if duty changed or 10 seconds elapsed
        let now = std::time::Instant::now();
        let should_write = state.last_written_duty != Some(duty) || 
            state.last_written_duty_time.is_none_or(|t| now.duration_since(t).as_secs() >= 10);

        if !should_write {
            return true;
        }

        if sysfs_write(&pwm_path, duty.to_string()).await {
            state.last_written_duty = Some(duty);
            state.last_written_duty_time = Some(now);
            // Track RPM equivalent for reporting
            let max_speed = state.max_speeds.values().max().copied().unwrap_or(6000);
            let rpm = ((max_speed as f64 * pct as f64) / 100.0).round() as u32;
            for &fan_num in &state.found_fans.clone() {
                state.last_targets.insert(fan_num, rpm);
            }
            true
        } else {
            warn!("Failed to write pwm1 duty cycle {}", duty);
            false
        }
    }

    async fn run_monitor_loop(&self) {
        // Run every 1 second to gather more frequent samples for the 5-second average
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));

        loop {
            interval.tick().await;

            {
                let mut state = self.state.lock().await;
                let hwmon_available = state.hwmon_path.is_some();
                if Self::should_retry_hw_detect(hwmon_available, state.fan_count, state.last_hw_detect_attempt.elapsed()) {
                    state.last_hw_detect_attempt = std::time::Instant::now();
                    let mode_to_restore = state.mode.clone();
                    Self::detect_hardware(&mut state).await;
                    if !hwmon_available && state.hwmon_path.is_some() && state.fan_count > 0 {
                        let _ = Self::set_mode_internal(&mut state, &mode_to_restore).await;
                        info!("Recovered fan hardware after delayed boot initialization");
                    }
                }
            }
            
            let temp = Self::get_max_temp().await;
            
            // 5-second Simple Moving Average (SMA) as requested by user
            let smoothed_temp = {
                let mut state = self.state.lock().await;
                state.temp_history.push_back(temp);
                if state.temp_history.len() > 5 {
                    state.temp_history.pop_front();
                }
                
                let sum: f64 = state.temp_history.iter().sum();
                sum / state.temp_history.len() as f64
            };

            let (mode, curve_json, fans, thermal_active, manual_pct) = {
                let mut state = self.state.lock().await;
                
                // Thermal Protection Logic ALWAYS uses raw temp for safety
                if state.thermal_protection_enabled && temp > 90.0 && !state.thermal_protection_active {
                    warn!("Temperature exceeded 90°C ({}°C). Activating Thermal Protection Mode (Max Fan).", temp);
                    state.thermal_protection_active = true;
                    state.thermal_protection_entered_at = std::time::Instant::now();
                    state.pre_protection_mode = Some(state.mode.clone());
                    Self::set_mode_internal(&mut state, "max").await;
                    tokio::spawn(async move {
                        let msg = if std::env::var("LANG").unwrap_or_default().starts_with("tr") {
                            "Yüksek sıcaklıklardan cihazınızı korumak için max fan modu aktif edildi."
                        } else {
                            "Max fan mode activated to protect your device from high temperatures."
                        };
                        let title = if std::env::var("LANG").unwrap_or_default().starts_with("tr") {
                            "Termal Koruma Modu"
                        } else {
                            "Thermal Protection"
                        };
                        DesktopNotifier::send_notification(title, msg, 1).await;
                    });
                } else if state.thermal_protection_active {
                    if !state.thermal_protection_enabled || temp <= 70.0 {
                        let elapsed = state.thermal_protection_entered_at.elapsed().as_secs_f64();
                        
                        info!("Temperature dropped to {}°C (active for {:.1}s) or protection disabled. Deactivating Thermal Protection Mode.", temp, elapsed);
                        state.thermal_protection_active = false;
                        let restore = state.pre_protection_mode.clone().unwrap_or_else(|| "auto".to_string());
                        state.last_written_duty = None;
                        state.last_auto_pct = 0;
                        Self::set_mode_internal(&mut state, &restore).await;
                        
                        let restore_msg = restore.clone();
                        tokio::spawn(async move {
                            let title = if std::env::var("LANG").unwrap_or_default().starts_with("tr") {
                                "Termal Koruma Modu"
                            } else {
                                "Thermal Protection"
                            };
                            let msg = if std::env::var("LANG").unwrap_or_default().starts_with("tr") {
                                format!("Sıcaklık normale döndü ({}°C). Fanlar {} moduna alındı.", temp as i32, restore_msg)
                            } else {
                                format!("Temperature normalized ({}°C). Fans set to {}.", temp as i32, restore_msg)
                            };
                            DesktopNotifier::send_notification(title, &msg, 0).await;
                        });
                    } else if state.mode != "max" {
                        Self::set_mode_internal(&mut state, "max").await;
                    }
                }

                (
                    state.mode.clone(),
                    state.custom_curve_json.clone(),
                    state.found_fans.clone(),
                    state.thermal_protection_active,
                    state.manual_target_pct,
                )
            };

            if thermal_active || fans.is_empty() {
                continue;
            }

            match mode.as_str() {
                "ec" => {
                    // Hardware EC mode (BIOS control).
                    // Daemon must not write anything. Watchdog will handle it.
                }
                "auto" => {
                    // Software Auto mode.
                    // Fallback to our own balanced curve if no manual target is set.
                    if let Some(pct) = manual_pct {
                        let mut state = self.state.lock().await;
                        Self::write_pwm_duty(&mut state, pct).await;
                        continue;
                    }

                    // Smart Hysteresis Logic for Auto Mode: On at 52C, Off at 43C
                    let mut state = self.state.lock().await;
                    let is_fan_on = state.auto_fan_activated_at.is_some();
                    
                    // Check if current system power profile is "performance"
                    let is_performance_power = {
                        let pp_path = "/sys/firmware/acpi/platform_profile";
                        let hp_path = "/sys/devices/platform/hp-wmi/platform_profile";
                        if let Ok(val) = std::fs::read_to_string(pp_path) {
                            val.trim() == "performance"
                        } else if let Ok(val) = std::fs::read_to_string(hp_path) {
                            val.trim() == "performance"
                        } else {
                            false
                        }
                    };

                    if state.last_power_profile_was_perf != is_performance_power {
                        state.last_power_profile_was_perf = is_performance_power;
                        // Reset hysteresis when power profile changes so fan adapts instantly
                        state.last_auto_pct = 0;
                        state.auto_fan_activated_at = None;
                    }

                    let mut desired_pct = if is_performance_power {
                        // Step-based aggressive curve for stability
                        let perf_curve = vec![
                            CurvePoint(0.0,  40.0),
                            CurvePoint(50.0, 55.0),
                            CurvePoint(60.0, 75.0),
                            CurvePoint(75.0, 85.0),
                            CurvePoint(85.0, 100.0),
                        ];
                        // When in performance power mode, we don't turn off the fans.
                        Self::evaluate_step(&perf_curve, smoothed_temp).round() as u32
                    } else {
                        // Standard Auto curve (Step-based)
                        let auto_curve = vec![
                            CurvePoint(0.0,  0.0),   
                            CurvePoint(50.0, 40.0),  
                            CurvePoint(60.0, 55.0),
                            CurvePoint(75.0, 75.0),
                            CurvePoint(85.0, 100.0),
                        ];

                        if !is_fan_on {
                            if smoothed_temp >= 52.0 {
                                let pct = Self::evaluate_step(&auto_curve, smoothed_temp);
                                pct.round() as u32
                            } else {
                                0
                            }
                        } else {
                            if smoothed_temp < 43.0 {
                                0
                            } else {
                                let pct = Self::evaluate_step(&auto_curve, smoothed_temp);
                                (pct.round() as u32).max(40) // Enforce minimum fan speed while ON
                            }
                        }
                    };

                    // 1. Peak Holding
                    if desired_pct > state.last_auto_pct {
                        // Immediate increase
                        state.last_auto_pct = desired_pct;
                        state.last_auto_pct_time = std::time::Instant::now();
                    } else if desired_pct < state.last_auto_pct {
                        // Decrease requested, check if 120s (2 mins) has passed since last increase
                        if state.last_auto_pct_time.elapsed().as_secs() < 120 {
                            // Hold the peak
                            desired_pct = state.last_auto_pct;
                        } else {
                            // Allowed to decrease
                            state.last_auto_pct = desired_pct;
                        }
                    }

                    // 2. Minimum Runtime (2 minutes)
                    if desired_pct == 0 {
                        if let Some(turned_on_at) = state.auto_fan_activated_at {
                            if turned_on_at.elapsed().as_secs() < 120 {
                                // Force it to stay on at minimum speed until 2 mins expire
                                desired_pct = 38;
                                state.last_auto_pct = 38;
                            } else {
                                // Allowed to turn off
                                state.auto_fan_activated_at = None;
                                state.last_auto_pct = 0;
                            }
                        }
                    } else {
                        // Fan is running
                        if state.auto_fan_activated_at.is_none() {
                            state.auto_fan_activated_at = Some(std::time::Instant::now());
                        }
                    }

                    Self::write_pwm_duty(&mut state, desired_pct).await;
                }
                "max" => {
                    let mut state = self.state.lock().await;
                    // Our caching in write_pwm_duty prevents I/O flooding,
                    // but calling this ensures max mode works even if pwm1_enable=0 fails.
                    Self::write_pwm_duty(&mut state, 100).await;
                }
                "performance" => {
                    if let Some(pct) = manual_pct {
                        let mut state = self.state.lock().await;
                        Self::write_pwm_duty(&mut state, pct).await;
                        continue;
                    }

                    // Step-based aggressive curve for stability
                    let perf_curve = vec![
                        CurvePoint(0.0,  40.0),
                        CurvePoint(50.0, 55.0),
                        CurvePoint(60.0, 75.0),
                        CurvePoint(75.0, 85.0),
                        CurvePoint(85.0, 100.0),
                    ];
                    
                    let mut desired_pct = Self::evaluate_step(&perf_curve, smoothed_temp).round() as u32;

                    let mut state = self.state.lock().await;

                    // Peak Holding specific to performance mode
                    if desired_pct > state.last_perf_pct {
                        state.last_perf_pct = desired_pct;
                        state.last_perf_pct_time = std::time::Instant::now();
                    } else if desired_pct < state.last_perf_pct {
                        if state.last_perf_pct_time.elapsed().as_secs() < 120 {
                            desired_pct = state.last_perf_pct;
                        } else {
                            state.last_perf_pct = desired_pct;
                        }
                    }

                    Self::write_pwm_duty(&mut state, desired_pct).await;
                }
                "custom" => {
                    // If user set a manual percentage (e.g. "fan 50"), re-write
                    // that duty as keep-alive. Don't evaluate any curve.
                    if let Some(pct) = manual_pct {
                        let mut state = self.state.lock().await;
                        Self::write_pwm_duty(&mut state, pct).await;
                        continue;
                    }

                    // Otherwise, evaluate custom curve and write duty
                    let mut curve_points: Vec<CurvePoint> = Vec::new();
                    
                    if let Ok(pts) = serde_json::from_str::<Vec<CurvePoint>>(&curve_json) {
                        curve_points = pts;
                    }
                    if curve_points.is_empty() {
                        curve_points = vec![
                            CurvePoint(40.0, 0.0),
                            CurvePoint(50.0, 30.0),
                            CurvePoint(65.0, 60.0),
                            CurvePoint(75.0, 85.0),
                            CurvePoint(85.0, 100.0),
                        ];
                    }
                    
                    if !curve_points.is_empty() {
                        let pct = Self::evaluate_step(&curve_points, smoothed_temp);
                        let target_pct = pct.round() as u32;
                        
                        let mut state = self.state.lock().await;
                        Self::write_pwm_duty(&mut state, target_pct).await;
                    }
                }
                _ => {}
            }
        }
    }

    async fn has_pwm_fallback(state: &FanState) -> bool {
        if let Some(ref hwmon) = state.hwmon_path {
            sysfs_exists(hwmon.join("pwm1")).await
        } else {
            false
        }
    }
    
    /// Set fan mode. This is the core function that configures the hardware.
    ///
    /// Modes and their hardware mapping (based on OmenCore SetFanProfileViaAcpiHwmon):
    ///   ec   → platform_profile=balanced, pwm1_enable=2 (BIOS hardware control)
    ///   auto/custom/performance → pwm1_enable=1 (manual), duty set by monitor loop or manual target
    ///   max  → platform_profile=performance, pwm1_enable=0 (full speed)
    async fn set_mode_internal(state: &mut FanState, mode: &str) -> bool {
        // Determine pwm1_enable value (OmenCore mapping)
        let pwm_enable_val = match mode {
            "ec" => 2,       // BIOS hardware control
            "max" => 0,      // BIOS max hardware speed
            "auto" | "custom" | "performance" => 1, // Manual/software control
            _ => return false,
        };

        if mode == "ec" {
            tokio::spawn(async move {
                DesktopNotifier::send_notification("Omen Space", "Donanım (EC) kontrolü devredildi. Watchdog mekanizması nedeniyle fanların BIOS'a teslim edilmesi 120 saniye kadar sürebilir.", 0).await;
            });
        }

        // Step 1: Write pwm1_enable
        let mut ok = false;
        if let Some(ref hwmon) = state.hwmon_path {
            if pwm_enable_val == 1 {
                let current = sysfs_read(hwmon.join("pwm1_enable"), 2).await;
                if current == 0 {
                    let _ = sysfs_write(hwmon.join("pwm1_enable"), "2").await;
                    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                }
            }
            ok = sysfs_write(hwmon.join("pwm1_enable"), pwm_enable_val.to_string()).await;
            if ok {
                info!("Set pwm1_enable={} (mode={})", pwm_enable_val, mode);
            }
        }
        
        if ok {
            state.mode = mode.to_string();
            state.last_targets.clear();
            state.last_keepalive = std::time::Instant::now();
            state.last_written_duty = None;
            state.last_auto_pct = 0;
            // Clear manual target when switching modes
            if mode == "auto" || mode == "max" || mode == "ec" || mode == "performance" {
                state.manual_target_pct = None;
            }
            if mode == "performance" {
                state.perf_cooldown_start = None;
                state.last_perf_pct = 50;
            }
            info!("Fan mode set to {}", mode);
        } else {
            // pwm1_enable write failed (e.g. board lacks pwm1_enable or is read-only).
            // Still store the mode so the monitor loop uses the right curve/logic.
            // The mode's target writes via pwm1 duty will self-recover on the next loop tick.
            warn!("Fan mode '{}': pwm1_enable write failed or path absent — mode stored, duty writes will proceed via monitor loop", mode);
            state.mode = mode.to_string();
            state.last_targets.clear();
            state.last_written_duty = None;
            state.last_auto_pct = 0;
            if mode == "auto" || mode == "max" || mode == "ec" || mode == "performance" {
                state.manual_target_pct = None;
            }
            if mode == "performance" {
                state.perf_cooldown_start = None;
                state.last_perf_pct = 50;
            }
        }
        
        // Step 3: EC thermal profile via register 0x59 (for boards that support it)
        // This is separate from the sysfs writes above; it sets the EC's internal
        // thermal policy to match.
        let mut ec = crate::ec::LinuxEcController::new();
        let needs_ec = ec.needs_ec_fallback();
        
        if mode == "auto" && needs_ec {
            // Full EC reset for legacy boards that need it
            ec.restore_auto_mode().await;
        }
        // Set EC perf mode for all boards that have EC access (including 8BBE via 0x59)
        let ec_mode = match mode {
            "max" => "max",
            "performance" => "performance",
            _ => "auto",
        };
        ec.set_perf_mode(ec_mode);

        ok
    }

    /// Set fan speed to a specific percentage. Called from D-Bus set_fan_target.
    /// Uses PWM duty cycle (pwm1) instead of fan_target sysfs.
    async fn set_fan_target_internal(state: &mut FanState, _fan_num: u32, rpm: u32) -> String {
        // Convert RPM to percentage using max speed
        let max_speed = state.max_speeds.values().max().copied().unwrap_or(6000);
        let pct = ((rpm as f64 / max_speed as f64) * 100.0).round() as u32;
        let pct = pct.clamp(0, 100);
        
        // Store as manual target so monitor loop re-applies it as keep-alive
        state.manual_target_pct = Some(pct);
        
        // Ensure we're in manual mode
        if state.mode != "custom" && state.mode != "performance" {
            // set_mode_internal will be called by the CLI before this,
            // but just in case:
            Self::set_mode_internal(state, "custom").await;
        }
        
        // Write the duty cycle
        if Self::write_pwm_duty(state, pct).await {
            info!("Fan target set to {}% ({}RPM) via pwm1 duty", pct, rpm);
            "true".to_string()
        } else {
            warn!("Fan target set to {}% failed", pct);
            "false".to_string()
        }
    }

    async fn get_target_speed(state: &FanState, fan_num: u32) -> u32 {
        if let Some(ref hwmon) = state.hwmon_path {
            let path = hwmon.join(format!("fan{}_target", fan_num));
            if sysfs_exists(&path).await {
                return sysfs_read(&path, 0).await as u32;
            }
        }
        
        if Self::has_pwm_fallback(state).await {
            if let Some(ref hwmon) = state.hwmon_path {
                let pwm = sysfs_read(hwmon.join("pwm1"), 0).await as u32;
                let max_speed = state.max_speeds.get(&fan_num).copied().unwrap_or(6000);
                return ((max_speed as f64 * pwm as f64) / 255.0).round() as u32;
            }
        }
        
        0
    }

    async fn save_config(mode_to_save: String, custom_curve_json: String) {
        let config_manager = crate::config::ConfigManager::new();
        let mut config = config_manager.load().await;
        config.fan_mode = mode_to_save;
        config.custom_curve = custom_curve_json;
        config_manager.save(&config).await;
    }
}

#[interface(name = "org.hp.omen.Fan")]
impl FanService {
    async fn get_fan_info(&self) -> String {
        let state = self.state.lock().await;
        let mut fans_data = serde_json::Map::new();
        
        for &i in &state.found_fans {
            let mut current = 0;
            if let Some(ref hwmon) = state.hwmon_path {
                current = sysfs_read(hwmon.join(format!("fan{}_input", i)), 0).await;
            }
            if current == 0 {
                if let Some(path) = state.fallback_paths.get(&i) {
                    current = sysfs_read(path, 0).await;
                }
            }
            
            let max = state.max_speeds.get(&i).copied().unwrap_or(6000);
            let target = Self::get_target_speed(&state, i).await;
            
            fans_data.insert(i.to_string(), serde_json::json!({
                "current": current,
                "max": max,
                "target": target,
            }));
        }
        
        let display_mode = state.mode.clone();
        let is_available = state.hwmon_path.is_some() && state.fan_count > 0;
        let mut supports_custom = false;
        if let Some(ref hwmon) = state.hwmon_path {
            supports_custom = Self::has_pwm_fallback(&state).await || sysfs_exists(hwmon.join("fan1_target")).await;
        }
        let thermal_protection = state.thermal_protection_active;

        let info = serde_json::json!({
            "available": is_available,
            "fan_count": state.fan_count,
            "mode": display_mode,
            "supports_custom": supports_custom,
            "custom_curve": state.custom_curve_json,
            "fans": fans_data,
            "thermal_protection": thermal_protection
        });
        
        serde_json::to_string(&info).unwrap_or_else(|_| "{}".to_string())
    }

    async fn get_fan_mode(&self) -> String {
        let state = self.state.lock().await;
        state.mode.clone()
    }

    async fn set_fan_mode(&mut self, mode: &str) -> String {
        let (mode_to_save, custom_curve_json, success) = {
            let mut state = self.state.lock().await;
            if state.thermal_protection_active {
                info!("Thermal protection is active. Recording target mode '{}' for post-protection restoration.", mode);
                state.pre_protection_mode = Some(mode.to_string());
                (mode.to_string(), state.custom_curve_json.clone(), true)
            } else {
                // Clear manual target when mode is explicitly changed
                state.manual_target_pct = None;
                let success = Self::set_mode_internal(&mut state, mode).await;
                (state.mode.clone(), state.custom_curve_json.clone(), success)
            }
        };
        
        if success {
            Self::save_config(mode_to_save, custom_curve_json).await;
            "OK".to_string()
        } else {
            "FAIL".to_string()
        }
    }

    async fn set_fan_target(&mut self, fan_num: u32, rpm: u32) -> String {
        let mut state = self.state.lock().await;
        let res = Self::set_fan_target_internal(&mut state, fan_num, rpm).await;
        if res == "true" { "OK".to_string() } else { "FAIL".to_string() }
    }

    async fn save_custom_curve(&mut self, curve_json: &str) -> String {
        let (mode_to_save, custom_curve_json, success) = {
            let mut state = self.state.lock().await;
            info!("SaveCustomCurve called with: {}", curve_json);
            if serde_json::from_str::<Vec<CurvePoint>>(curve_json).is_ok() {
                state.custom_curve_json = curve_json.to_string();
                // When a new curve is saved, clear manual target so curve takes over
                state.manual_target_pct = None;
                let mode_to_save = if state.thermal_protection_active {
                    state.pre_protection_mode.clone().unwrap_or_else(|| state.mode.clone())
                } else {
                    state.mode.clone()
                };
                (mode_to_save, state.custom_curve_json.clone(), true)
            } else {
                (String::new(), String::new(), false)
            }
        };

        if success {
            Self::save_config(mode_to_save, custom_curve_json).await;
            "OK".to_string()
        } else {
            "FAIL".to_string()
        }
    }

    async fn set_thermal_protection(&mut self, enabled: bool) -> String {
        let mut state = self.state.lock().await;
        state.thermal_protection_enabled = enabled;
        
        let mut config = crate::config::ConfigManager::new().load().await;
        config.thermal_protection_enabled = enabled;
        crate::config::ConfigManager::new().save(&config).await;
        
        info!("Thermal protection {}", if enabled { "enabled" } else { "disabled" });
        "OK".to_string()
    }

    async fn ping(&self) -> String {
        "pong".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_vectors() {
        let vectors = serde_json::from_str::<serde_json::Value>(include_str!("../tests/fixtures/vectors.json")).unwrap();
        for test_case in vectors.as_array().unwrap() {
            let mut points = Vec::new();
            for pt in test_case["curve"].as_array().unwrap() {
                points.push(CurvePoint(
                    pt[0].as_f64().unwrap(),
                    pt[1].as_f64().unwrap(),
                ));
            }
            let input_temp = test_case["temp"].as_f64().unwrap();
            let expected_pct = test_case["expected"].as_f64().unwrap();
            let actual_pct = FanService::evaluate_spline(&points, input_temp);
            assert!((actual_pct - expected_pct).abs() < 1e-5, "Failed for temp {} with curve {:?}", input_temp, points);
        }
    }

    #[test]
    fn test_pct_to_pwm() {
        assert_eq!(FanService::pct_to_pwm(0), 0);
        assert_eq!(FanService::pct_to_pwm(50), 128);
        assert_eq!(FanService::pct_to_pwm(100), 255);
    }

    #[test]
    fn test_should_retry_hw_detect_when_missing_hwmon() {
        assert!(FanService::should_retry_hw_detect(
            false,
            0,
            std::time::Duration::from_secs(5),
        ));
        assert!(!FanService::should_retry_hw_detect(
            false,
            0,
            std::time::Duration::from_secs(4),
        ));
    }

    #[test]
    fn test_should_retry_hw_detect_when_no_fans() {
        assert!(FanService::should_retry_hw_detect(
            true,
            0,
            std::time::Duration::from_secs(5),
        ));
        assert!(!FanService::should_retry_hw_detect(
            true,
            2,
            std::time::Duration::from_secs(30),
        ));
    }
}
