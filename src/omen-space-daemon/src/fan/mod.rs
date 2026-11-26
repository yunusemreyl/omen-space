mod curves;
mod hwmon;
mod protection;
#[cfg(test)]
mod regression_fan;

use crate::notifier::DesktopNotifier;
use curves::{
    curve_mode_delegates_to_firmware, curve_target_pct, default_auto_step_curve,
    default_perf_step_curve, evaluate_step, select_cpu_temp_paths, CurvePoint, AUTO_PEAK_HOLD_SECS,
};
use glob::glob;
use hwmon::{
    detect_hardware_into, get_target_speed, has_pwm_node, leave_max_boost, sysfs_exists,
    sysfs_read, sysfs_write, write_fan_targets, write_pwm_duty,
};
use log::{info, warn};
use protection::{
    apply_thermal_protection_toggle, handle_ec_override_recovery, handle_stall_detection,
    ThermalMonitorTracker,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tokio::sync::Mutex;
use zbus::interface;

static SENSOR_TEMP_PATHS: OnceLock<Vec<PathBuf>> = OnceLock::new();

pub struct FanState {
    pub hwmon_path: Option<PathBuf>,
    pub minimum_manual_pwm: u32,
    pub zero_pct_manual_pwm: bool,
    pub found_fans: Vec<u32>,
    pub max_speeds: HashMap<u32, u32>,
    pub fallback_paths: HashMap<u32, PathBuf>,
    pub fan_count: u32,
    pub mode: String,
    pub custom_curve_json: String,
    /// GPU fan curve in custom mode; empty = both fans follow `custom_curve_json`.
    pub custom_curve_gpu_json: String,
    pub last_targets: HashMap<u32, u32>,
    pub thermal_protection_active: bool,
    pub thermal_protection_enabled: bool,
    pub thermal_protection_entered_at: std::time::Instant,
    /// Suppress the EC-handoff desktop notice for the next mode change (automatic restore, #327).
    pub suppress_ec_notice: bool,
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
    pub signal_ctx: Option<zbus::SignalContext<'static>>,
}

#[derive(Clone)]
pub struct FanService {
    state: Arc<Mutex<FanState>>,
}

impl FanService {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let config_manager = crate::config::ConfigManager::new();
        let config = config_manager.load().await;

        let board = std::fs::read_to_string("/sys/class/dmi/id/board_name").unwrap_or_default();
        let caps = crate::capabilities::detect(board.trim(), "", "");
        let effective_min_pwm = config
            .minimum_manual_pwm_override
            .unwrap_or(caps.minimum_manual_pwm)
            .clamp(1, 255);
        let mut state = FanState {
            hwmon_path: None,
            minimum_manual_pwm: effective_min_pwm,
            zero_pct_manual_pwm: config.zero_pct_manual_pwm,
            found_fans: Vec::new(),
            max_speeds: HashMap::new(),
            fallback_paths: HashMap::new(),
            fan_count: 0,
            mode: config.fan_mode.clone(),
            custom_curve_json: config.custom_curve.clone(),
            custom_curve_gpu_json: config.custom_curve_gpu.clone(),
            last_targets: HashMap::new(),
            thermal_protection_active: false,
            thermal_protection_enabled: config.thermal_protection_enabled,
            thermal_protection_entered_at: std::time::Instant::now(),
            suppress_ec_notice: false,
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
            signal_ctx: None,
        };
        detect_hardware_into(&mut state).await;

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

        if state.hwmon_path.is_some() && state.mode != config.fan_mode {
            Self::set_mode_internal(&mut state, &config.fan_mode).await;
        }

        let service = Self {
            state: Arc::new(Mutex::new(state)),
        };

        let service_clone = service.clone();
        tokio::spawn(async move {
            service_clone.run_monitor_loop().await;
        });

        Ok(service)
    }

    pub fn set_signal_ctx(&self, ctx: zbus::SignalContext<'static>) {
        let state = self.state.clone();
        tokio::spawn(async move {
            state.lock().await.signal_ctx = Some(ctx);
        });
    }

    pub async fn get_mode_internal(&self) -> String {
        self.state.lock().await.mode.clone()
    }

    pub async fn apply_mode_internal(&self, mode: &str) -> bool {
        let (mode_to_save, custom_curve_json, success) = {
            let mut state = self.state.lock().await;
            state.thermal_protection_active = false;
            state.manual_target_pct = None;
            let ok = Self::set_mode_internal(&mut state, mode).await;
            (state.mode.clone(), state.custom_curve_json.clone(), ok)
        };
        if success {
            Self::save_config(mode_to_save, custom_curve_json).await;
        }
        success
    }

    fn should_retry_hw_detect(
        hwmon_available: bool,
        fan_count: u32,
        elapsed: std::time::Duration,
    ) -> bool {
        elapsed >= std::time::Duration::from_secs(5) && (!hwmon_available || fan_count == 0)
    }

    /// (CPU/board temperature, dGPU temperature). Prefers CPU package `temp1_input`
    /// sensors so single-core spikes or noisy ACPI/PCH nodes do not saturate fan curves (#395).
    async fn get_temps() -> (f64, f64) {
        tokio::task::spawn_blocking(|| {
            let paths = SENSOR_TEMP_PATHS.get_or_init(|| {
                let mut candidates = Vec::new();
                if let Ok(entries) = glob("/sys/class/hwmon/hwmon*/temp*_input") {
                    for entry in entries.filter_map(Result::ok) {
                        let (name, vendor, class) = if let Some(parent) = entry.parent() {
                            (
                                std::fs::read_to_string(parent.join("name")).unwrap_or_default(),
                                std::fs::read_to_string(parent.join("device/vendor"))
                                    .unwrap_or_default(),
                                std::fs::read_to_string(parent.join("device/class"))
                                    .unwrap_or_default(),
                            )
                        } else {
                            Default::default()
                        };
                        candidates.push((name, vendor, class, entry));
                    }
                }
                select_cpu_temp_paths(&candidates)
            });

            let mut max_temp = 45.0;
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

            let gpu_temp = crate::sysmon::get_safe_gpu_temp();
            let gpu_temp = if gpu_temp < 150.0 { gpu_temp } else { 0.0 };

            (max_temp, gpu_temp)
        })
        .await
        .unwrap_or((45.0, 0.0))
    }

    fn is_performance_power_profile() -> bool {
        let pp_path = "/sys/firmware/acpi/platform_profile";
        let hp_path = "/sys/devices/platform/hp-wmi/platform_profile";
        let sysfs_perf = if let Ok(val) = std::fs::read_to_string(pp_path) {
            val.trim() == "performance"
        } else if let Ok(val) = std::fs::read_to_string(hp_path) {
            val.trim() == "performance"
        } else {
            false
        };
        sysfs_perf
            || std::fs::read_to_string("/etc/omen-space/power.json")
                .ok()
                .and_then(|data| serde_json::from_str::<serde_json::Value>(&data).ok())
                .and_then(|v| {
                    v.get("power_profile")
                        .and_then(|p| p.as_str().map(|s| s == "performance"))
                })
                .unwrap_or(false)
    }

    async fn run_monitor_loop(&self) {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
        let mut stall_ticks: u32 = 0;
        let mut ec_override_ticks: u32 = 0;
        let mut cpu_fan_history: std::collections::VecDeque<f64> =
            std::collections::VecDeque::new();
        let mut gpu_fan_history: std::collections::VecDeque<f64> =
            std::collections::VecDeque::new();
        let mut last_stall_alert: Option<std::time::Instant> = None;
        let mut last_ec_override_reset: Option<std::time::Instant> = None;
        let mut last_unreadable_warn: Option<std::time::Instant> = None;
        let mut thermal_tracker = ThermalMonitorTracker::new();

        loop {
            interval.tick().await;

            {
                let mut state = self.state.lock().await;
                let hwmon_available = state.hwmon_path.is_some();
                if Self::should_retry_hw_detect(
                    hwmon_available,
                    state.fan_count,
                    state.last_hw_detect_attempt.elapsed(),
                ) {
                    state.last_hw_detect_attempt = std::time::Instant::now();
                    let mode_to_restore = state.mode.clone();
                    detect_hardware_into(&mut state).await;
                    if !hwmon_available && state.hwmon_path.is_some() && state.fan_count > 0 {
                        let _ = Self::set_mode_internal(&mut state, &mode_to_restore).await;
                        info!("Recovered fan hardware after delayed boot initialization");
                    }
                }
            }

            let (cpu_temp, gpu_temp) = Self::get_temps().await;
            let temp = cpu_temp.max(gpu_temp);

            let gpu_fan_sample = if gpu_temp > 0.0 { gpu_temp } else { cpu_temp };
            for (history, value) in [
                (&mut cpu_fan_history, cpu_temp),
                (&mut gpu_fan_history, gpu_fan_sample),
            ] {
                history.push_back(value);
                if history.len() > 5 {
                    history.pop_front();
                }
            }
            let cpu_fan_temp = cpu_fan_history.iter().sum::<f64>() / cpu_fan_history.len() as f64;
            let gpu_fan_temp = gpu_fan_history.iter().sum::<f64>() / gpu_fan_history.len() as f64;

            let smoothed_temp = {
                let mut state = self.state.lock().await;
                state.temp_history.push_back(temp);
                if state.temp_history.len() > 5 {
                    state.temp_history.pop_front();
                }
                let sum: f64 = state.temp_history.iter().sum();
                sum / state.temp_history.len() as f64
            };

            let (mode, curve_json, gpu_curve_json, fans, thermal_active, manual_pct) = {
                let mut state = self.state.lock().await;

                handle_stall_detection(
                    &mut state,
                    temp,
                    &mut stall_ticks,
                    &mut last_unreadable_warn,
                    &mut last_stall_alert,
                )
                .await;

                handle_ec_override_recovery(
                    &mut state,
                    temp,
                    &mut ec_override_ticks,
                    &mut last_ec_override_reset,
                )
                .await;

                thermal_tracker.step(&mut state, temp, smoothed_temp).await;

                (
                    state.mode.clone(),
                    state.custom_curve_json.clone(),
                    state.custom_curve_gpu_json.clone(),
                    state.found_fans.clone(),
                    state.thermal_protection_active,
                    state.manual_target_pct,
                )
            };

            if thermal_active {
                let mut state = self.state.lock().await;
                write_pwm_duty(&mut state, 100).await;
                continue;
            }

            if fans.is_empty() {
                continue;
            }

            {
                let mut state = self.state.lock().await;
                let has_pwm = has_pwm_node(&state).await;
                if curve_mode_delegates_to_firmware(&mode, state.hwmon_path.is_some(), has_pwm) {
                    write_pwm_duty(&mut state, 0).await;
                    continue;
                }
            }

            match mode.as_str() {
                "ec" => {}
                "auto" => {
                    if let Some(pct) = manual_pct {
                        let mut state = self.state.lock().await;
                        write_pwm_duty(&mut state, pct).await;
                        continue;
                    }

                    let mut state = self.state.lock().await;
                    let is_fan_on = state.auto_fan_activated_at.is_some();
                    let is_performance_power = Self::is_performance_power_profile();

                    if state.last_power_profile_was_perf != is_performance_power {
                        state.last_power_profile_was_perf = is_performance_power;
                        state.last_auto_pct = 0;
                        state.auto_fan_activated_at = None;
                    }

                    let mut desired_pct = if is_performance_power {
                        let perf_curve = default_perf_step_curve();
                        evaluate_step(&perf_curve, smoothed_temp).round() as u32
                    } else {
                        let auto_curve = default_auto_step_curve();
                        if !is_fan_on {
                            if smoothed_temp >= 52.0 {
                                evaluate_step(&auto_curve, smoothed_temp).round() as u32
                            } else {
                                0
                            }
                        } else if smoothed_temp < 45.0 {
                            0
                        } else {
                            (evaluate_step(&auto_curve, smoothed_temp).round() as u32).max(32)
                        }
                    };

                    if desired_pct > state.last_auto_pct {
                        state.last_auto_pct = desired_pct;
                        state.last_auto_pct_time = std::time::Instant::now();
                    } else if desired_pct < state.last_auto_pct {
                        if state.last_auto_pct_time.elapsed().as_secs() < AUTO_PEAK_HOLD_SECS {
                            desired_pct = state.last_auto_pct;
                        } else {
                            state.last_auto_pct = desired_pct;
                        }
                    }

                    if desired_pct == 0 {
                        if let Some(turned_on_at) = state.auto_fan_activated_at {
                            if turned_on_at.elapsed().as_secs() < AUTO_PEAK_HOLD_SECS {
                                desired_pct = 38;
                                state.last_auto_pct = 38;
                            } else {
                                state.auto_fan_activated_at = None;
                                state.last_auto_pct = 0;
                            }
                        }
                    } else if state.auto_fan_activated_at.is_none() {
                        state.auto_fan_activated_at = Some(std::time::Instant::now());
                    }

                    write_pwm_duty(&mut state, desired_pct).await;
                }
                "max" => {
                    let mut state = self.state.lock().await;
                    write_pwm_duty(&mut state, 100).await;
                }
                "performance" => {
                    if let Some(pct) = manual_pct {
                        let mut state = self.state.lock().await;
                        write_pwm_duty(&mut state, pct).await;
                        continue;
                    }

                    let perf_curve = default_perf_step_curve();
                    let mut desired_pct = evaluate_step(&perf_curve, smoothed_temp).round() as u32;

                    let mut state = self.state.lock().await;
                    if desired_pct > state.last_perf_pct {
                        state.last_perf_pct = desired_pct;
                        state.last_perf_pct_time = std::time::Instant::now();
                    } else if desired_pct < state.last_perf_pct {
                        if state.last_perf_pct_time.elapsed().as_secs() < AUTO_PEAK_HOLD_SECS {
                            desired_pct = state.last_perf_pct;
                        } else {
                            state.last_perf_pct = desired_pct;
                        }
                    }

                    write_pwm_duty(&mut state, desired_pct).await;
                }
                "custom" => {
                    if let Some(pct) = manual_pct {
                        let mut state = self.state.lock().await;
                        write_pwm_duty(&mut state, pct).await;
                        continue;
                    }

                    let minimum_pwm = self.state.lock().await.minimum_manual_pwm;
                    let gpu_curve_set = serde_json::from_str::<Vec<CurvePoint>>(&gpu_curve_json)
                        .is_ok_and(|pts| !pts.is_empty());
                    let mut state = self.state.lock().await;
                    if gpu_curve_set {
                        let cpu_pct = curve_target_pct(&curve_json, cpu_fan_temp, minimum_pwm);
                        let gpu_pct = curve_target_pct(&gpu_curve_json, gpu_fan_temp, minimum_pwm);
                        write_fan_targets(&mut state, cpu_pct, gpu_pct).await;
                    } else {
                        let pct = curve_target_pct(&curve_json, smoothed_temp, minimum_pwm);
                        write_pwm_duty(&mut state, pct).await;
                    }
                }
                _ => {}
            }
        }
    }

    async fn set_mode_internal(state: &mut FanState, mode: &str) -> bool {
        let pwm_enable_val = match mode {
            "ec" => 2,
            "max" => 0,
            "auto" | "custom" | "performance" => 1,
            _ => return false,
        };

        let board_id = std::fs::read_to_string("/sys/class/dmi/id/board_name").unwrap_or_default();
        let board_id = board_id.trim();
        let caps = crate::capabilities::detect(board_id, "", "");

        let only_ec_safe = caps.supports_fan_control_ec && !caps.supports_fan_control_wmi;
        if only_ec_safe && matches!(mode, "auto" | "custom" | "performance") {
            warn!(
                "Board {} supports only EC fan control (WMI 0x2E writes proven rejected by BIOS). Forcing EC mode.",
                board_id
            );
            let _ = Box::pin(Self::set_mode_internal(state, "ec")).await;
            DesktopNotifier::send_notification(
                "Omen Space",
                "Auto/Custom/Performance fan modes are not supported on your OMEN BIOS. Fan control switched to Hardware (EC) mode.",
                0,
            )
            .await;
            return true;
        }

        let _suppress_ec = std::mem::take(&mut state.suppress_ec_notice);

        let was_max = state.mode == "max"
            || match state.hwmon_path {
                Some(ref hwmon) => sysfs_read(hwmon.join("pwm1_enable"), 2).await == 0,
                None => false,
            };

        let mut ok = false;
        let mut written_enable_val = pwm_enable_val;
        if let Some(ref hwmon) = state.hwmon_path {
            if pwm_enable_val == 0 {
                if sysfs_exists(hwmon.join("pwm1")).await {
                    let _ = sysfs_write(hwmon.join("pwm1_enable"), "1").await;
                    let _ = sysfs_write(hwmon.join("pwm1"), "255").await;
                }
                ok = sysfs_write(hwmon.join("pwm1_enable"), "0").await;
            } else if pwm_enable_val == 1 {
                if !sysfs_exists(hwmon.join("pwm1")).await {
                    if was_max {
                        leave_max_boost(hwmon).await;
                    }
                    ok = sysfs_write(hwmon.join("pwm1_enable"), "2").await;
                    written_enable_val = 2;
                } else {
                    if was_max {
                        leave_max_boost(hwmon).await;
                    }
                    ok = sysfs_write(hwmon.join("pwm1_enable"), "1").await;
                    if ok && was_max {
                        let safe_duty = curves::pct_to_pwm(38).max(state.minimum_manual_pwm);
                        let _ = sysfs_write(hwmon.join("pwm1"), safe_duty.to_string()).await;
                    }
                }
            } else if pwm_enable_val == 2 {
                leave_max_boost(hwmon).await;
                ok = sysfs_write(hwmon.join("pwm1_enable"), "2").await;
            }
            if ok {
                info!("Set pwm1_enable={} (mode={})", written_enable_val, mode);
                if mode == "ec" {
                    info!("Hardware (EC) fan control delegated; BIOS watchdog handoff may take up to 120s.");
                }
            }
        }

        if ok {
            state.mode = mode.to_string();
            state.last_targets.clear();
            state.last_keepalive = std::time::Instant::now();
            state.last_written_duty = None;
            state.last_auto_pct = 0;
            state.last_perf_pct = 0;
            state.auto_fan_activated_at = None;
            if matches!(mode, "auto" | "max" | "ec" | "performance") {
                state.manual_target_pct = None;
            }
            if mode == "max" {
                state.last_written_duty = Some(255);
                state.last_written_duty_time = Some(std::time::Instant::now());
                let max_speed = state.max_speeds.values().max().copied().unwrap_or(6000);
                for &fan_num in &state.found_fans.clone() {
                    state.last_targets.insert(fan_num, max_speed);
                }
            }
            if mode == "performance" {
                state.perf_cooldown_start = None;
                state.last_perf_pct = 50;
            }
            info!("Fan mode set to {}", mode);
        } else {
            warn!(
                "Fan mode '{}': pwm1_enable write failed or path absent — mode stored, duty writes will proceed via monitor loop",
                mode
            );
            state.mode = mode.to_string();
            state.last_targets.clear();
            state.last_written_duty = None;
            state.last_auto_pct = 0;
            if matches!(mode, "auto" | "max" | "ec" | "performance") {
                state.manual_target_pct = None;
            }
            if mode == "performance" {
                state.perf_cooldown_start = None;
                state.last_perf_pct = 50;
            }
        }

        let mut ec = crate::ec::LinuxEcController::new();
        if ec.needs_ec_fallback() {
            if mode == "auto" {
                ec.restore_auto_mode().await;
            }
            let ec_mode = match mode {
                "max" => "max",
                "performance" => "performance",
                _ => "auto",
            };
            ec.set_perf_mode(ec_mode);
        }

        ok
    }

    async fn set_fan_target_internal(state: &mut FanState, _fan_num: u32, rpm: u32) -> String {
        let max_speed = state.max_speeds.values().max().copied().unwrap_or(6000);
        let pct = ((rpm as f64 / max_speed as f64) * 100.0).round() as u32;
        let pct = pct.clamp(0, 100);

        state.thermal_protection_active = false;
        state.manual_target_pct = Some(pct);

        if state.mode != "custom" && state.mode != "performance" {
            Self::set_mode_internal(state, "custom").await;
        }

        if write_pwm_duty(state, pct).await {
            info!("Fan target set to {}% ({}RPM) via pwm1 duty", pct, rpm);
            "true".to_string()
        } else {
            warn!("Fan target set to {}% failed", pct);
            "false".to_string()
        }
    }

    async fn save_config(mode_to_save: String, custom_curve_json: String) {
        let config_manager = crate::config::ConfigManager::new();
        let mut config = config_manager.load().await;
        config.fan_mode = mode_to_save;
        config.custom_curve = custom_curve_json;
        config_manager.save(&config).await;
    }

    async fn save_gpu_curve_config(custom_curve_gpu_json: String) {
        let config_manager = crate::config::ConfigManager::new();
        let mut config = config_manager.load().await;
        config.custom_curve_gpu = custom_curve_gpu_json;
        config_manager.save(&config).await;
    }

    async fn apply_thermal_protection_toggle(
        state: &mut FanState,
        enabled: bool,
    ) -> Option<zbus::SignalContext<'static>> {
        apply_thermal_protection_toggle(state, enabled).await
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
            let target = get_target_speed(&state, i).await;

            fans_data.insert(
                i.to_string(),
                serde_json::json!({
                    "current": current,
                    "max": max,
                    "target": target,
                }),
            );
        }

        let display_mode = state.mode.clone();
        let is_available = state.hwmon_path.is_some() && state.fan_count > 0;
        let mut supports_custom = false;
        if let Some(ref hwmon) = state.hwmon_path {
            supports_custom =
                has_pwm_node(&state).await || sysfs_exists(hwmon.join("fan1_target")).await;
        }
        let thermal_protection = state.thermal_protection_active;
        let thermal_protection_enabled = state.thermal_protection_enabled;

        let info = serde_json::json!({
            "available": is_available,
            "fan_count": state.fan_count,
            "mode": display_mode,
            "supports_custom": supports_custom,
            "custom_curve": state.custom_curve_json,
            "custom_curve_gpu": state.custom_curve_gpu_json,
            "fans": fans_data,
            "thermal_protection": thermal_protection,
            "thermal_protection_enabled": thermal_protection_enabled
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
            state.thermal_protection_active = false;
            state.manual_target_pct = None;
            let success = Self::set_mode_internal(&mut state, mode).await;
            (state.mode.clone(), state.custom_curve_json.clone(), success)
        };

        if success {
            Self::save_config(mode_to_save, custom_curve_json).await;
            "OK".to_string()
        } else {
            let is_tr = std::env::var("LANG").unwrap_or_default().starts_with("tr");
            let (ctx, err_msg) = if is_tr {
                (
                    "Fan Modu Değiştirilemedi",
                    format!("Fan modu '{}' uygulanamadı. EC (Embedded Controller) komut arayüzüne erişilemiyor.", mode),
                )
            } else {
                (
                    "Failed to Change Fan Mode",
                    format!("Fan mode '{}' could not be applied. Hardware fan control interface is unavailable.", mode),
                )
            };
            warn!("{}", err_msg);
            DesktopNotifier::notify_error(ctx, &err_msg).await;
            "FAIL".to_string()
        }
    }

    async fn set_fan_target(&mut self, fan_num: u32, rpm: u32) -> String {
        let mut state = self.state.lock().await;
        let res = Self::set_fan_target_internal(&mut state, fan_num, rpm).await;
        if res == "true" {
            "OK".to_string()
        } else {
            "FAIL".to_string()
        }
    }

    async fn save_custom_gpu_curve(&mut self, curve_json: &str) -> String {
        if !curve_json.is_empty() && serde_json::from_str::<Vec<CurvePoint>>(curve_json).is_err() {
            return "FAIL".to_string();
        }
        {
            let mut state = self.state.lock().await;
            info!("SaveCustomGpuCurve called with: {}", curve_json);
            state.custom_curve_gpu_json = curve_json.to_string();
            state.manual_target_pct = None;
            state.thermal_protection_active = false;
            state.last_written_duty = None;
            state.last_targets.clear();
        }
        Self::save_gpu_curve_config(curve_json.to_string()).await;
        "OK".to_string()
    }

    async fn save_custom_curve(&mut self, curve_json: &str) -> String {
        let (mode_to_save, custom_curve_json, success) = {
            let mut state = self.state.lock().await;
            info!("SaveCustomCurve called with: {}", curve_json);
            if serde_json::from_str::<Vec<CurvePoint>>(curve_json).is_ok() {
                state.custom_curve_json = curve_json.to_string();
                state.manual_target_pct = None;
                state.thermal_protection_active = false;
                state.last_written_duty = None;
                (state.mode.clone(), state.custom_curve_json.clone(), true)
            } else {
                (String::new(), String::new(), false)
            }
        };

        if success {
            Self::save_config(mode_to_save, custom_curve_json).await;
            "OK".to_string()
        } else {
            let is_tr = std::env::var("LANG").unwrap_or_default().starts_with("tr");
            let (ctx, msg) = if is_tr {
                (
                    "Özel Fan Eğrisi Kaydedilemedi",
                    "Gönderilen eğri verisi geçersiz JSON formatında. Lütfen tekrar deneyin.",
                )
            } else {
                (
                    "Failed to Save Custom Fan Curve",
                    "The submitted fan curve data is not valid JSON. Please try again.",
                )
            };
            DesktopNotifier::notify_error(ctx, msg).await;
            "FAIL".to_string()
        }
    }

    async fn set_thermal_protection(&mut self, enabled: bool) -> String {
        let signal_ctx = {
            let mut state = self.state.lock().await;
            Self::apply_thermal_protection_toggle(&mut state, enabled).await
        };
        if let Some(ctx) = signal_ctx {
            let _ = Self::thermal_protection_alert(&ctx, false).await;
        }

        let mut config = crate::config::ConfigManager::new().load().await;
        config.thermal_protection_enabled = enabled;
        crate::config::ConfigManager::new().save(&config).await;

        info!(
            "Thermal protection {}",
            if enabled { "enabled" } else { "disabled" }
        );
        "OK".to_string()
    }

    async fn ping(&self) -> String {
        "pong".to_string()
    }

    #[zbus(signal)]
    pub async fn thermal_protection_alert(
        ctxt: &zbus::SignalContext<'_>,
        active: bool,
    ) -> zbus::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

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
