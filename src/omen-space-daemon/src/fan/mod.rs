mod curves;
mod hwmon;

use crate::notifier::DesktopNotifier;
pub use curves::{
    curve_mode_delegates_to_firmware, curve_target_pct, default_auto_step_curve,
    default_perf_step_curve, evaluate_step, select_cpu_temp_paths, should_recover_ec_max_latch,
    should_trigger_thermal_protection, update_advisory_thermal_warn_state, CurvePoint,
    AUTO_PEAK_HOLD_SECS, EC_OVERRIDE_RECOVERY_COOLDOWN_SECS, NOTICE_PERSIST_TICKS,
    THERMAL_PROTECTION_RECOVER_TEMP, THERMAL_PROTECTION_TRIGGER_TEMP,
};
use glob::glob;
use hwmon::{
    detect_hardware_into, get_target_speed, has_pwm_node, leave_max_boost, sysfs_exists,
    sysfs_read, sysfs_write, write_fan_targets, write_pwm_duty,
};
use log::{info, warn};
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

    async fn handle_stall_detection(
        state: &mut FanState,
        temp: f64,
        stall_ticks: &mut u32,
        last_unreadable_warn: &mut Option<std::time::Instant>,
        last_stall_alert: &mut Option<std::time::Instant>,
    ) {
        let commanded_duty = state.last_written_duty.unwrap_or(0);
        if temp < 78.0 || commanded_duty < 80 || state.found_fans.is_empty() {
            *stall_ticks = 0;
            return;
        }

        let (all_confirmed_zero, any_unreadable) = if let Some(ref hwmon) = state.hwmon_path {
            let mut all_zero = true;
            let mut unreadable = false;
            for idx in &state.found_fans {
                match std::fs::read_to_string(hwmon.join(format!("fan{}_input", idx)))
                    .ok()
                    .and_then(|s| s.trim().parse::<u32>().ok())
                {
                    Some(0) => {}
                    Some(_) => all_zero = false,
                    None => {
                        all_zero = false;
                        unreadable = true;
                    }
                }
            }
            (all_zero, unreadable)
        } else {
            (false, true)
        };

        if any_unreadable {
            *stall_ticks = 0;
            if last_unreadable_warn
                .map(|t| t.elapsed() >= std::time::Duration::from_secs(60))
                .unwrap_or(true)
            {
                *last_unreadable_warn = Some(std::time::Instant::now());
                warn!(
                    "Fan telemetry unavailable while temp is {:.1}°C (fan*_input unreadable); skipping stall kick (#311).",
                    temp
                );
            }
            return;
        }

        if !all_confirmed_zero {
            *stall_ticks = 0;
            return;
        }

        *stall_ticks = stall_ticks.saturating_add(1);
        let cooldown_ok = last_stall_alert
            .map(|t| t.elapsed() >= std::time::Duration::from_secs(60))
            .unwrap_or(true);
        if *stall_ticks >= 5 && cooldown_ok {
            *last_stall_alert = Some(std::time::Instant::now());
            warn!(
                "CRITICAL FAN STALL DETECTED: temp is {:.1}°C while all fans read 0 RPM for {}s! Attempting Max Fan kick (#301).",
                temp, *stall_ticks
            );
            Self::set_mode_internal(state, "max").await;
            let alert_temp = temp as i32;
            tokio::spawn(async move {
                let (title, msg) = if std::env::var("LANG").unwrap_or_default().starts_with("tr") {
                    (
                        "KRİTİK UYARI: Fan Durması Tespit Edildi!",
                        format!("Sıcaklık {}°C iken tüm fanlar 0 RPM bildiriyor! Acil durum maksimum fan komutu gönderildi.", alert_temp),
                    )
                } else {
                    (
                        "CRITICAL: Fan Stall Detected!",
                        format!("Temperature is {}°C while all fans report 0 RPM! Emergency Max Fan command triggered.", alert_temp),
                    )
                };
                DesktopNotifier::send_notification(title, &msg, 2).await;
            });
        }
    }

    /// Detect when EC thermal-guard leaves fans latched near max speed well after
    /// cooling down (<80°C) even though daemon targets are low, and clear the latch by
    /// re-applying the active platform_profile (WMI 0x1A) + bouncing pwm1_enable (#395).
    async fn handle_ec_override_recovery(
        state: &mut FanState,
        temp: f64,
        override_ticks: &mut u32,
        last_reset: &mut Option<std::time::Instant>,
    ) {
        let Some(ref hwmon) = state.hwmon_path.clone() else {
            *override_ticks = 0;
            return;
        };
        let Some(commanded_duty) = state.last_written_duty else {
            *override_ticks = 0;
            return;
        };

        let mut max_actual_rpm = 0u32;
        for &idx in &state.found_fans {
            if let Ok(s) = std::fs::read_to_string(hwmon.join(format!("fan{}_input", idx))) {
                if let Ok(rpm) = s.trim().parse::<u32>() {
                    max_actual_rpm = max_actual_rpm.max(rpm);
                }
            }
        }
        let target_rpm = state.last_targets.values().copied().max().unwrap_or(0);
        let max_rpm = state.max_speeds.values().copied().max().unwrap_or(6000);

        if !should_recover_ec_max_latch(
            &state.mode,
            state.thermal_protection_active,
            temp,
            commanded_duty,
            max_actual_rpm,
            target_rpm,
            max_rpm,
            override_ticks,
        ) {
            return;
        }

        let cooldown_ok = last_reset
            .map(|t| t.elapsed().as_secs() >= EC_OVERRIDE_RECOVERY_COOLDOWN_SECS)
            .unwrap_or(true);
        if !cooldown_ok {
            return;
        }

        *override_ticks = 0;
        *last_reset = Some(std::time::Instant::now());
        warn!(
            "Detected persistent EC max-fan latch ({} RPM actual vs {} RPM target at {:.1}°C); refreshing platform_profile and resetting pwm1_enable (#395).",
            max_actual_rpm, target_rpm, temp
        );

        let pwm_enable = hwmon.join("pwm1_enable");
        if sysfs_exists(&pwm_enable).await {
            let _ = sysfs_write(&pwm_enable, "2").await;
        }
        for p in [
            "/sys/firmware/acpi/platform_profile",
            "/sys/devices/platform/hp-wmi/platform_profile",
        ] {
            if let Ok(active) = tokio::fs::read_to_string(p).await {
                let trimmed = active.trim();
                if !trimmed.is_empty() && sysfs_write(p, trimmed).await {
                    break;
                }
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(80)).await;
        if sysfs_exists(hwmon.join("pwm1")).await && sysfs_exists(&pwm_enable).await {
            let _ = sysfs_write(&pwm_enable, "1").await;
        }
        state.last_written_duty = None;
        state.last_targets.clear();
    }

    async fn run_monitor_loop(&self) {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
        let mut stall_ticks: u32 = 0;
        let mut ec_override_ticks: u32 = 0;
        let mut overheat_ticks: u32 = 0;
        let mut cpu_fan_history: std::collections::VecDeque<f64> = std::collections::VecDeque::new();
        let mut gpu_fan_history: std::collections::VecDeque<f64> = std::collections::VecDeque::new();
        let mut last_stall_alert: Option<std::time::Instant> = None;
        let mut last_ec_override_reset: Option<std::time::Instant> = None;
        let mut last_unreadable_warn: Option<std::time::Instant> = None;
        let mut last_thermal_alert: Option<std::time::Instant> = None;
        let mut warn_hot_since: Option<std::time::Instant> = None;
        let mut warn_episode_notified = false;
        let mut max_notice_pending = false;
        let mut max_notice_sent = false;

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

                Self::handle_stall_detection(
                    &mut state,
                    temp,
                    &mut stall_ticks,
                    &mut last_unreadable_warn,
                    &mut last_stall_alert,
                )
                .await;

                Self::handle_ec_override_recovery(
                    &mut state,
                    temp,
                    &mut ec_override_ticks,
                    &mut last_ec_override_reset,
                )
                .await;

                let now_inst = std::time::Instant::now();
                if update_advisory_thermal_warn_state(
                    temp,
                    state.thermal_protection_enabled,
                    now_inst,
                    &mut warn_hot_since,
                    &mut warn_episode_notified,
                    &mut last_thermal_alert,
                    DesktopNotifier::are_thermal_alerts_enabled(),
                ) {
                    let alert_temp = temp as i32;
                    tokio::spawn(async move {
                        let (title, msg) =
                            if std::env::var("LANG").unwrap_or_default().starts_with("tr") {
                                (
                                    "Yüksek Sıcaklık Uyarısı",
                                    format!("CPU/GPU sıcaklığı {}°C seviyesine ulaştı.", alert_temp),
                                )
                            } else {
                                (
                                    "High Temperature Warning",
                                    format!("CPU/GPU temperature reached {}°C.", alert_temp),
                                )
                            };
                        DesktopNotifier::send_notification_tagged(Some("thermal"), title, &msg, 1)
                            .await;
                    });
                }

                if !state.thermal_protection_enabled {
                    overheat_ticks = 0;
                }
                if state.thermal_protection_enabled
                    && !state.thermal_protection_active
                    && should_trigger_thermal_protection(temp, &mut overheat_ticks)
                {
                    overheat_ticks = 0;
                    warn!(
                        "Sustained temperature exceeded {}°C (raw: {:.1}°C, smoothed: {:.1}°C). Activating Thermal Protection Mode (Max Fan).",
                        THERMAL_PROTECTION_TRIGGER_TEMP, temp, smoothed_temp
                    );
                    state.thermal_protection_active = true;
                    state.thermal_protection_entered_at = std::time::Instant::now();
                    state.pre_protection_mode = Some(state.mode.clone());
                    Self::set_mode_internal(&mut state, "max").await;

                    if let Some(ctx) = state.signal_ctx.clone() {
                        let _ = Self::thermal_protection_alert(&ctx, true).await;
                    }

                    max_notice_pending = true;
                    max_notice_sent = false;
                } else if state.thermal_protection_active {
                    overheat_ticks = 0;
                    if !state.thermal_protection_enabled || temp <= THERMAL_PROTECTION_RECOVER_TEMP
                    {
                        let elapsed = state.thermal_protection_entered_at.elapsed().as_secs_f64();
                        info!(
                            "Temperature dropped to {}°C (active for {:.1}s) or protection disabled. Deactivating Thermal Protection Mode.",
                            temp, elapsed
                        );
                        state.thermal_protection_active = false;
                        let restore = state
                            .pre_protection_mode
                            .clone()
                            .unwrap_or_else(|| "auto".to_string());
                        state.last_written_duty = None;
                        state.last_auto_pct = 0;
                        state.suppress_ec_notice = !max_notice_sent;
                        Self::set_mode_internal(&mut state, &restore).await;
                        state.suppress_ec_notice = false;

                        if let Some(ctx) = state.signal_ctx.clone() {
                            let _ = Self::thermal_protection_alert(&ctx, false).await;
                        }

                        let restore_msg = restore.clone();
                        let send_recovery = max_notice_sent;
                        max_notice_pending = false;
                        max_notice_sent = false;
                        if send_recovery {
                            tokio::spawn(async move {
                                let is_tr = std::env::var("LANG")
                                    .unwrap_or_default()
                                    .starts_with("tr");
                                let title = if is_tr {
                                    "Termal Koruma Modu"
                                } else {
                                    "Thermal Protection"
                                };
                                let msg = if is_tr {
                                    format!(
                                        "Sıcaklık normale döndü ({}°C). Fanlar {} moduna alındı.",
                                        temp as i32, restore_msg
                                    )
                                } else {
                                    format!(
                                        "Temperature normalized ({}°C). Fans set to {}.",
                                        temp as i32, restore_msg
                                    )
                                };
                                DesktopNotifier::send_notification_tagged(
                                    Some("thermal"),
                                    title,
                                    &msg,
                                    1,
                                )
                                .await;
                            });
                        }
                    } else if state.mode != "max" {
                        Self::set_mode_internal(&mut state, "max").await;
                    }
                }

                if max_notice_pending
                    && state.thermal_protection_active
                    && state.thermal_protection_entered_at.elapsed().as_secs()
                        >= NOTICE_PERSIST_TICKS as u64
                {
                    max_notice_pending = false;
                    max_notice_sent = true;
                    tokio::spawn(async move {
                        let is_tr = std::env::var("LANG")
                            .unwrap_or_default()
                            .starts_with("tr");
                        let msg = if is_tr {
                            "Yüksek sıcaklıklardan cihazınızı korumak için max fan modu aktif edildi."
                        } else {
                            "Max fan mode activated to protect your device from high temperatures."
                        };
                        let title = if is_tr {
                            "Termal Koruma Modu"
                        } else {
                            "Thermal Protection"
                        };
                        DesktopNotifier::send_notification_tagged(Some("thermal"), title, msg, 1)
                            .await;
                    });
                }

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
                    ok = sysfs_write(hwmon.join("pwm1_enable"), "2").await;
                    written_enable_val = 2;
                } else {
                    leave_max_boost(hwmon).await;
                    ok = sysfs_write(hwmon.join("pwm1_enable"), "1").await;
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
        state.thermal_protection_enabled = enabled;
        if !enabled && state.thermal_protection_active {
            state.thermal_protection_active = false;
            let restore = state
                .pre_protection_mode
                .take()
                .unwrap_or_else(|| "auto".to_string());
            state.last_written_duty = None;
            state.last_auto_pct = 0;
            state.suppress_ec_notice = true;
            Self::set_mode_internal(state, &restore).await;
            state.suppress_ec_notice = false;
            return state.signal_ctx.clone();
        }
        None
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
    use super::curves::{evaluate_spline, is_ignored_sensor_device, pct_to_pwm};
    use super::*;

    #[test]
    fn test_golden_vectors() {
        let vectors =
            serde_json::from_str::<serde_json::Value>(include_str!("../../tests/fixtures/vectors.json"))
                .unwrap();
        for test_case in vectors.as_array().unwrap() {
            let mut points = Vec::new();
            for pt in test_case["curve"].as_array().unwrap() {
                points.push(CurvePoint(pt[0].as_f64().unwrap(), pt[1].as_f64().unwrap()));
            }
            let input_temp = test_case["temp"].as_f64().unwrap();
            let expected_pct = test_case["expected"].as_f64().unwrap();
            let actual_pct = evaluate_spline(&points, input_temp);
            assert!(
                (actual_pct - expected_pct).abs() < 1e-5,
                "Failed for temp {} with curve {:?}",
                input_temp,
                points
            );
        }
    }

    #[test]
    fn test_pct_to_pwm() {
        assert_eq!(pct_to_pwm(0), 0);
        assert_eq!(pct_to_pwm(50), 128);
        assert_eq!(pct_to_pwm(100), 255);
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

    #[test]
    fn test_is_ignored_sensor_device() {
        assert!(is_ignored_sensor_device("nvme", "", ""));
        assert!(is_ignored_sensor_device("mt7921_phy0", "", ""));
        assert!(is_ignored_sensor_device("iwlwifi_1", "", ""));
        assert!(is_ignored_sensor_device("wireless", "", ""));
        assert!(is_ignored_sensor_device("wifi_core", "", ""));
        assert!(is_ignored_sensor_device("nvidia", "", ""));
        assert!(is_ignored_sensor_device("nouveau", "", ""));
        assert!(is_ignored_sensor_device("temp_sensor", "0x10de", "0x030000"));

        assert!(!is_ignored_sensor_device("coretemp", "", ""));
        assert!(!is_ignored_sensor_device("k10temp", "", ""));
        assert!(!is_ignored_sensor_device("zenpower", "", ""));
        assert!(!is_ignored_sensor_device("acpitz", "", ""));
    }

    #[test]
    fn test_thermal_protection_constants() {
        const {
            assert!(
                THERMAL_PROTECTION_TRIGGER_TEMP >= 95.0,
                "Trigger must protect at critical temp"
            );
            assert!(
                THERMAL_PROTECTION_RECOVER_TEMP <= 85.0,
                "Recover must allow normal operation"
            );
            assert!(
                THERMAL_PROTECTION_TRIGGER_TEMP > THERMAL_PROTECTION_RECOVER_TEMP,
                "Trigger must be higher than recover"
            );
        }
        assert_eq!(AUTO_PEAK_HOLD_SECS, 15, "Peak hold must be responsive");
    }

    #[test]
    fn test_evaluate_spline_sorting() {
        let mut points = vec![
            CurvePoint(75.0, 85.0),
            CurvePoint(40.0, 0.0),
            CurvePoint(85.0, 100.0),
            CurvePoint(50.0, 30.0),
            CurvePoint(65.0, 60.0),
        ];
        points.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        assert_eq!(evaluate_spline(&points, 40.0), 0.0);
        assert_eq!(evaluate_spline(&points, 85.0), 100.0);

        let mid = evaluate_spline(&points, 57.5);
        assert!((mid - 45.0).abs() < 1e-4);
    }

    #[test]
    fn test_advisory_thermal_warn_requires_continuous_3s_and_ignores_alternating_spikes() {
        let t0 = std::time::Instant::now();
        let mut warn_hot_since = None;
        let mut warn_episode_notified = false;
        let mut last_thermal_alert = None;

        for (sec, temp) in [(0, 91.0), (1, 87.0), (2, 91.0), (3, 87.0), (4, 91.0)] {
            let fired = update_advisory_thermal_warn_state(
                temp,
                true,
                t0 + std::time::Duration::from_secs(sec),
                &mut warn_hot_since,
                &mut warn_episode_notified,
                &mut last_thermal_alert,
                true,
            );
            assert!(!fired, "Alternating spike at t={}s must not fire", sec);
        }

        warn_hot_since = None;
        for sec in [10, 11, 12] {
            let fired = update_advisory_thermal_warn_state(
                91.0,
                true,
                t0 + std::time::Duration::from_secs(sec),
                &mut warn_hot_since,
                &mut warn_episode_notified,
                &mut last_thermal_alert,
                true,
            );
            assert!(
                !fired,
                "Must not fire before 3 full seconds elapsed (t={}s)",
                sec
            );
        }
        let fired_at_3s = update_advisory_thermal_warn_state(
            91.0,
            true,
            t0 + std::time::Duration::from_secs(13),
            &mut warn_hot_since,
            &mut warn_episode_notified,
            &mut last_thermal_alert,
            true,
        );
        assert!(
            fired_at_3s,
            "Must fire after 3 full seconds of continuous >=90°C"
        );
        assert!(warn_episode_notified);
    }
}

#[cfg(test)]
mod regression_fan {
    use super::curves::{pwm_enable_without_pwm_node, should_latch_max_pwm_enable};
    use super::*;

    fn test_state(hwmon: PathBuf) -> FanState {
        let mut max_speeds = HashMap::new();
        max_speeds.insert(1, 6000);
        max_speeds.insert(2, 6000);
        FanState {
            hwmon_path: Some(hwmon),
            minimum_manual_pwm: 50,
            zero_pct_manual_pwm: false,
            found_fans: vec![1, 2],
            max_speeds,
            fallback_paths: HashMap::new(),
            fan_count: 2,
            mode: "auto".to_string(),
            custom_curve_json: "[]".to_string(),
            custom_curve_gpu_json: String::new(),
            last_targets: HashMap::new(),
            thermal_protection_active: false,
            thermal_protection_enabled: true,
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
        }
    }

    #[test]
    fn balanced_auto_curve_stays_below_max_boost_under_90c_load() {
        let pct_85 = evaluate_step(&default_auto_step_curve(), 85.0);
        let pct_89 = evaluate_step(&default_auto_step_curve(), 89.0);
        assert!(pct_85 < 95.0, "85C should not request 100% max latch ({})", pct_85);
        assert!(pct_89 < 95.0, "89C should not request 100% max latch ({})", pct_89);
        assert_eq!(evaluate_step(&default_auto_step_curve(), 93.0), 100.0);
    }

    #[test]
    fn should_latch_max_pwm_enable_avoids_sticky_latch_in_curve_modes() {
        assert!(!should_latch_max_pwm_enable("auto", false, 100, true));
        assert!(!should_latch_max_pwm_enable("custom", false, 100, true));
        assert!(!should_latch_max_pwm_enable("performance", false, 100, true));
        assert!(should_latch_max_pwm_enable("max", false, 100, true));
        assert!(should_latch_max_pwm_enable("auto", true, 100, true));
        assert!(should_latch_max_pwm_enable("auto", false, 100, false));
    }

    #[tokio::test]
    async fn curve_at_100pct_uses_manual_255_when_pwm1_exists_no_max_latch() {
        let dir = std::env::temp_dir().join(format!("omen-fan-manual100-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pwm1_enable"), "1").unwrap();
        std::fs::write(dir.join("pwm1"), "128").unwrap();
        let mut state = test_state(dir.clone());
        state.mode = "custom".to_string();

        assert!(write_pwm_duty(&mut state, 100).await);
        let enable_val = std::fs::read_to_string(dir.join("pwm1_enable")).unwrap();
        let pwm_val = std::fs::read_to_string(dir.join("pwm1")).unwrap();
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(enable_val, "1", "custom 100% must stay in pwm1_enable=1 (#395)");
        assert_eq!(pwm_val, "255");
    }

    #[tokio::test]
    async fn explicit_max_mode_sets_pwm_enable_zero_and_leave_max_boost_resets_via_auto() {
        let dir = std::env::temp_dir().join(format!("omen-fan-maxleave-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pwm1_enable"), "1").unwrap();
        std::fs::write(dir.join("pwm1"), "128").unwrap();
        let mut state = test_state(dir.clone());
        state.mode = "max".to_string();

        assert!(write_pwm_duty(&mut state, 100).await);
        assert_eq!(std::fs::read_to_string(dir.join("pwm1_enable")).unwrap(), "0");

        // Now exit max boost directly via leave_max_boost: must write "2" (AUTO) to reset EC latch
        leave_max_boost(&dir).await;
        let reset_val = std::fs::read_to_string(dir.join("pwm1_enable")).unwrap();
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(reset_val, "2", "leave_max_boost must transition 0 -> 2 (#395)");
    }

    #[test]
    fn curve_modes_delegate_to_firmware_only_without_pwm_node() {
        for mode in ["auto", "performance", "custom"] {
            assert!(
                curve_mode_delegates_to_firmware(mode, true, false),
                "{}",
                mode
            );
            assert!(
                !curve_mode_delegates_to_firmware(mode, true, true),
                "{}",
                mode
            );
            assert!(
                !curve_mode_delegates_to_firmware(mode, false, false),
                "{}",
                mode
            );
        }
        for mode in ["max", "ec"] {
            assert!(
                !curve_mode_delegates_to_firmware(mode, true, false),
                "{}",
                mode
            );
        }
    }

    #[test]
    fn pwm_enable_without_pwm_node_never_requests_manual() {
        assert_eq!(pwm_enable_without_pwm_node(100), 0);
        for pct in [0, 1, 35, 75, 99] {
            assert_eq!(pwm_enable_without_pwm_node(pct), 2, "pct {}", pct);
        }
    }

    #[tokio::test]
    async fn regression_max_boost_released_without_pwm_node() {
        let dir = std::env::temp_dir().join(format!("omen-fan-nopwm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pwm1_enable"), "2").unwrap();
        let mut state = test_state(dir.clone());

        assert!(write_pwm_duty(&mut state, 100).await);
        let at_max = std::fs::read_to_string(dir.join("pwm1_enable")).unwrap();
        assert!(!state.last_targets.is_empty());

        assert!(write_pwm_duty(&mut state, 75).await);
        let after_cooldown = std::fs::read_to_string(dir.join("pwm1_enable")).unwrap();
        let targets_cleared = state.last_targets.is_empty();

        assert!(write_pwm_duty(&mut state, 0).await);
        let at_zero = std::fs::read_to_string(dir.join("pwm1_enable")).unwrap();
        let _ = std::fs::remove_dir_all(dir);

        assert_eq!(at_max, "0", "100% must still engage max boost");
        assert_eq!(
            after_cooldown, "2",
            "a sub-100% request must release max boost to firmware control"
        );
        assert!(
            targets_cleared,
            "stale max-boost targets must be dropped once firmware owns the fans"
        );
        assert_eq!(at_zero, "2");
    }

    #[tokio::test]
    async fn regression_preserve_working_25_percent_request() {
        let dir = std::env::temp_dir().join(format!("omen-review-fan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pwm1"), "0").unwrap();
        std::fs::write(dir.join("pwm1_enable"), "1").unwrap();
        let mut state = test_state(dir.clone());
        state.mode = "custom".to_string();

        assert!(write_pwm_duty(&mut state, 25).await);
        let actual = std::fs::read_to_string(dir.join("pwm1")).unwrap();
        let _ = std::fs::remove_dir_all(dir);
        assert_eq!(
            actual, "64",
            "a known-working 25% target should remain 25% instead of a global board-independent 35% override"
        );
    }

    #[test]
    fn thermal_protection_requires_three_consecutive_samples_above_95() {
        let mut ticks = 0;
        assert!(!should_trigger_thermal_protection(97.0, &mut ticks));
        assert_eq!(ticks, 1);
        assert!(!should_trigger_thermal_protection(88.0, &mut ticks));
        assert_eq!(ticks, 0);
        assert!(!should_trigger_thermal_protection(96.0, &mut ticks));
        assert!(!should_trigger_thermal_protection(96.0, &mut ticks));
        assert!(should_trigger_thermal_protection(96.0, &mut ticks));
        ticks = 0;
        assert!(should_trigger_thermal_protection(102.0, &mut ticks));
    }

    #[tokio::test]
    async fn independent_fan_targets_write_fan1_and_fan2_targets() {
        let dir = std::env::temp_dir().join(format!("omen-fan-targets-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pwm1_enable"), "1").unwrap();
        std::fs::write(dir.join("pwm1"), "0").unwrap();
        std::fs::write(dir.join("fan1_target"), "0").unwrap();
        std::fs::write(dir.join("fan2_target"), "0").unwrap();
        let mut state = test_state(dir.clone());

        assert!(write_fan_targets(&mut state, 50, 80).await);
        let f1 = std::fs::read_to_string(dir.join("fan1_target")).unwrap();
        let f2 = std::fs::read_to_string(dir.join("fan2_target")).unwrap();
        let _ = std::fs::remove_dir_all(dir);

        assert_eq!(f1, "3000");
        assert_eq!(f2, "4800");
    }

    #[test]
    fn select_cpu_temp_paths_prefers_package_sensor_over_core_and_acpi_spikes() {
        let candidates = vec![
            ("acpitz".into(), "".into(), "".into(), PathBuf::from("/sys/class/hwmon/hwmon0/temp1_input")),
            ("coretemp".into(), "".into(), "".into(), PathBuf::from("/sys/class/hwmon/hwmon3/temp1_input")),
            ("coretemp".into(), "".into(), "".into(), PathBuf::from("/sys/class/hwmon/hwmon3/temp2_input")),
            ("spd5118".into(), "".into(), "".into(), PathBuf::from("/sys/class/hwmon/hwmon4/temp1_input")),
            ("nvme".into(), "".into(), "".into(), PathBuf::from("/sys/class/hwmon/hwmon1/temp1_input")),
        ];
        let picked = select_cpu_temp_paths(&candidates);
        assert_eq!(picked, vec![PathBuf::from("/sys/class/hwmon/hwmon3/temp1_input")]);
    }

    #[test]
    fn ec_max_latch_detector_triggers_on_8c77_cool_idle_mismatch_and_ignores_real_load() {
        let mut ticks = 0;
        for _ in 0..7 {
            assert!(!should_recover_ec_max_latch("auto", false, 58.0, 85, 5700, 2000, 6000, &mut ticks));
        }
        assert!(should_recover_ec_max_latch("auto", false, 58.0, 85, 5700, 2000, 6000, &mut ticks));

        // Must reset immediately under high load/temp or high commanded PWM
        assert!(!should_recover_ec_max_latch("auto", false, 84.0, 85, 5700, 2000, 6000, &mut ticks));
        assert_eq!(ticks, 0);
        assert!(!should_recover_ec_max_latch("auto", false, 58.0, 220, 5700, 5200, 6000, &mut ticks));
    }

    #[tokio::test]
    async fn disabling_thermal_protection_restores_previous_fan_mode() {
        let dir = std::env::temp_dir().join(format!("omen-fan-tprestore-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pwm1_enable"), "0").unwrap();
        std::fs::write(dir.join("pwm1"), "255").unwrap();
        let mut state = test_state(dir.clone());
        state.mode = "max".to_string();
        state.thermal_protection_active = true;
        state.pre_protection_mode = Some("auto".to_string());

        FanService::apply_thermal_protection_toggle(&mut state, false).await;
        let _ = std::fs::remove_dir_all(&dir);

        assert!(!state.thermal_protection_enabled);
        assert!(!state.thermal_protection_active);
        assert_eq!(state.mode, "auto");
    }
}
