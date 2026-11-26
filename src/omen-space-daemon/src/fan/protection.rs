use super::curves::{
    should_recover_ec_max_latch, should_trigger_thermal_protection,
    update_advisory_thermal_warn_state, EC_OVERRIDE_RECOVERY_COOLDOWN_SECS, NOTICE_PERSIST_TICKS,
    THERMAL_PROTECTION_RECOVER_TEMP, THERMAL_PROTECTION_TRIGGER_TEMP,
};
use super::hwmon::{refresh_active_platform_profile, sysfs_exists, sysfs_write};
use super::{FanService, FanState};
use crate::notifier::DesktopNotifier;
use log::{info, warn};

pub(super) struct ThermalMonitorTracker {
    overheat_ticks: u32,
    last_thermal_alert: Option<std::time::Instant>,
    warn_hot_since: Option<std::time::Instant>,
    warn_episode_notified: bool,
    max_notice_pending: bool,
    max_notice_sent: bool,
}

impl ThermalMonitorTracker {
    pub(super) fn new() -> Self {
        Self {
            overheat_ticks: 0,
            last_thermal_alert: None,
            warn_hot_since: None,
            warn_episode_notified: false,
            max_notice_pending: false,
            max_notice_sent: false,
        }
    }

    pub(super) async fn step(&mut self, state: &mut FanState, temp: f64, smoothed_temp: f64) {
        let now_inst = std::time::Instant::now();
        if update_advisory_thermal_warn_state(
            temp,
            state.thermal_protection_enabled,
            now_inst,
            &mut self.warn_hot_since,
            &mut self.warn_episode_notified,
            &mut self.last_thermal_alert,
            DesktopNotifier::are_thermal_alerts_enabled(),
        ) {
            let alert_temp = temp as i32;
            tokio::spawn(async move {
                let (title, msg) = if std::env::var("LANG").unwrap_or_default().starts_with("tr") {
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
                DesktopNotifier::send_notification_tagged(Some("thermal"), title, &msg, 1).await;
            });
        }

        if !state.thermal_protection_enabled {
            self.overheat_ticks = 0;
        }
        if state.thermal_protection_enabled
            && !state.thermal_protection_active
            && should_trigger_thermal_protection(temp, &mut self.overheat_ticks)
        {
            self.overheat_ticks = 0;
            warn!(
                "Sustained temperature exceeded {}°C (raw: {:.1}°C, smoothed: {:.1}°C). Activating Thermal Protection Mode (Max Fan).",
                THERMAL_PROTECTION_TRIGGER_TEMP, temp, smoothed_temp
            );
            state.thermal_protection_active = true;
            state.thermal_protection_entered_at = std::time::Instant::now();
            state.pre_protection_mode = Some(state.mode.clone());
            FanService::set_mode_internal(state, "max").await;

            if let Some(ctx) = state.signal_ctx.clone() {
                let _ = FanService::thermal_protection_alert(&ctx, true).await;
            }

            self.max_notice_pending = true;
            self.max_notice_sent = false;
        } else if state.thermal_protection_active {
            self.overheat_ticks = 0;
            if !state.thermal_protection_enabled || temp <= THERMAL_PROTECTION_RECOVER_TEMP {
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
                state.suppress_ec_notice = !self.max_notice_sent;
                FanService::set_mode_internal(state, &restore).await;
                state.suppress_ec_notice = false;

                if let Some(ctx) = state.signal_ctx.clone() {
                    let _ = FanService::thermal_protection_alert(&ctx, false).await;
                }

                let restore_msg = restore.clone();
                let send_recovery = self.max_notice_sent;
                self.max_notice_pending = false;
                self.max_notice_sent = false;
                if send_recovery {
                    tokio::spawn(async move {
                        let is_tr = std::env::var("LANG").unwrap_or_default().starts_with("tr");
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
                        DesktopNotifier::send_notification_tagged(Some("thermal"), title, &msg, 1)
                            .await;
                    });
                }
            } else if state.mode != "max" {
                FanService::set_mode_internal(state, "max").await;
            }
        }

        if self.max_notice_pending
            && state.thermal_protection_active
            && state.thermal_protection_entered_at.elapsed().as_secs()
                >= NOTICE_PERSIST_TICKS as u64
        {
            self.max_notice_pending = false;
            self.max_notice_sent = true;
            tokio::spawn(async move {
                let is_tr = std::env::var("LANG").unwrap_or_default().starts_with("tr");
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
                DesktopNotifier::send_notification_tagged(Some("thermal"), title, msg, 1).await;
            });
        }
    }
}

pub(super) async fn handle_stall_detection(
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
        FanService::set_mode_internal(state, "max").await;
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
pub(super) async fn handle_ec_override_recovery(
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
    refresh_active_platform_profile().await;
    tokio::time::sleep(tokio::time::Duration::from_millis(80)).await;
    if sysfs_exists(hwmon.join("pwm1")).await && sysfs_exists(&pwm_enable).await {
        let _ = sysfs_write(&pwm_enable, "1").await;
    }
    state.last_written_duty = None;
    state.last_targets.clear();
}

pub(super) async fn apply_thermal_protection_toggle(
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
        FanService::set_mode_internal(state, &restore).await;
        state.suppress_ec_notice = false;
        return state.signal_ctx.clone();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::super::curves::{
        update_advisory_thermal_warn_state, AUTO_PEAK_HOLD_SECS, THERMAL_PROTECTION_RECOVER_TEMP,
        THERMAL_PROTECTION_TRIGGER_TEMP,
    };

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
