use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

pub const THERMAL_PROTECTION_TRIGGER_TEMP: f64 = 95.0;
pub const THERMAL_PROTECTION_RECOVER_TEMP: f64 = 82.0;
/// Seconds (1 Hz samples) a thermal condition must persist before a desktop notice is shown (#327).
pub const NOTICE_PERSIST_TICKS: u32 = 3;
pub const AUTO_PEAK_HOLD_SECS: u64 = 15;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CurvePoint(pub f64, pub f64); // [Temp, Pct]

/// Balanced auto step-curve with 0 RPM fan stop below 48°C (#347) and
/// <100% headroom below 93°C so normal 85–89°C gaming loads do not latch Max Boost (#395).
pub fn default_auto_step_curve() -> Vec<CurvePoint> {
    vec![
        CurvePoint(0.0, 0.0),
        CurvePoint(48.0, 0.0),
        CurvePoint(52.0, 32.0),
        CurvePoint(64.0, 52.0),
        CurvePoint(74.0, 72.0),
        CurvePoint(82.0, 82.0),
        CurvePoint(88.0, 88.0),
        CurvePoint(93.0, 100.0),
    ]
}

/// Aggressive performance step-curve reaching 100% at >=92°C.
pub fn default_perf_step_curve() -> Vec<CurvePoint> {
    vec![
        CurvePoint(0.0, 40.0),
        CurvePoint(50.0, 55.0),
        CurvePoint(60.0, 75.0),
        CurvePoint(75.0, 85.0),
        CurvePoint(85.0, 92.0),
        CurvePoint(92.0, 100.0),
    ]
}

/// Default fallback linear curve when no custom curve points are saved.
pub fn default_custom_curve() -> Vec<CurvePoint> {
    vec![
        CurvePoint(40.0, 0.0),
        CurvePoint(50.0, 30.0),
        CurvePoint(65.0, 60.0),
        CurvePoint(78.0, 82.0),
        CurvePoint(86.0, 90.0),
        CurvePoint(92.0, 100.0),
    ]
}

pub fn evaluate_spline(points: &[CurvePoint], temp: f64) -> f64 {
    if points.is_empty() {
        return 0.0;
    }
    if temp <= points[0].0 {
        return points[0].1;
    }
    if let Some(last) = points.last() {
        if temp >= last.0 {
            return last.1;
        }
    }
    for i in 0..points.len() - 1 {
        let x0 = points[i].0;
        let y0 = points[i].1;
        let x1 = points[i + 1].0;
        let y1 = points[i + 1].1;
        if temp >= x0 && temp <= x1 {
            return y0 + (y1 - y0) * ((temp - x0) / (x1 - x0));
        }
    }
    points.last().map(|p| p.1).unwrap_or(0.0)
}

pub fn evaluate_step(points: &[CurvePoint], temp: f64) -> f64 {
    if points.is_empty() {
        return 0.0;
    }
    if temp < points[0].0 {
        return points[0].1;
    }
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

/// Convert a percentage (0–100) to a PWM duty cycle value (0–255).
pub fn pct_to_pwm(pct: u32) -> u32 {
    let pct = pct.clamp(0, 100);
    let pwm = ((pct as f64 * 255.0) / 100.0).round() as u32;
    pwm.clamp(0, 255)
}

/// Target percentage of a custom curve at `temp`: empty/invalid falls back to default,
/// and fan-stop (0%) is only permitted below 50°C (#346).
pub fn curve_target_pct(curve_json: &str, temp: f64, minimum_manual_pwm: u32) -> u32 {
    let mut curve_points: Vec<CurvePoint> = serde_json::from_str(curve_json).unwrap_or_default();
    if curve_points.is_empty() {
        curve_points = default_custom_curve();
    }
    curve_points.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut target_pct = (evaluate_spline(&curve_points, temp).round() as u32).clamp(0, 100);
    if target_pct <= 5 {
        target_pct = if temp < 50.0 { 0 } else { 1 };
    }
    if target_pct > 0 {
        target_pct = target_pct.max((minimum_manual_pwm * 100).div_ceil(255));
    }
    target_pct
}

/// True when a curve-driven mode cannot be honoured because the hwmon has no `pwm1` node.
pub fn curve_mode_delegates_to_firmware(mode: &str, has_hwmon: bool, has_pwm_node: bool) -> bool {
    has_hwmon && !has_pwm_node && matches!(mode, "auto" | "performance" | "custom")
}

/// `pwm1_enable` value to request on boards whose hwmon has no `pwm1` node, i.e. where
/// only max boost (0) and firmware control (2) exist. Manual mode (1) is never valid there.
pub fn pwm_enable_without_pwm_node(pct: u32) -> i64 {
    if pct >= 100 { 0 } else { 2 }
}

/// Returns `true` only when hardware `pwm1_enable = 0` (`PWM_MODE_MAX`) should be latched:
/// - Explicit `"max"` mode
/// - Active `>= 95°C` thermal protection emergency override
/// - `pct >= 100` on legacy hwmon devices that lack a writable `pwm1` duty node
///
/// When `pwm1` exists and the user is in a curve mode (`auto`, `performance`, `custom`), even
/// a 100% curve target stays in `pwm1_enable = 1` (`pwm1 = 255`) so firmware never latches
/// WMI `0x27=1` hardware max-fan mode (#395).
pub fn should_latch_max_pwm_enable(
    mode: &str,
    thermal_protection_active: bool,
    pct: u32,
    has_pwm_node: bool,
) -> bool {
    mode == "max" || thermal_protection_active || (!has_pwm_node && pct >= 100)
}

/// Requires 3 consecutive 1 Hz samples above 95°C (or emergency >= 102°C) so single
/// boot/turbo spikes do not latch thermal protection (#371).
pub fn should_trigger_thermal_protection(temp: f64, overheat_ticks: &mut u32) -> bool {
    if temp > THERMAL_PROTECTION_TRIGGER_TEMP {
        *overheat_ticks = overheat_ticks.saturating_add(1);
    } else {
        *overheat_ticks = 0;
        return false;
    }
    *overheat_ticks >= NOTICE_PERSIST_TICKS || temp >= 102.0
}

pub fn is_ignored_sensor_device(name: &str, vendor: &str, class: &str) -> bool {
    let name_lower = name.trim().to_lowercase();
    if name_lower.contains("nvidia")
        || name_lower.contains("nouveau")
        || name_lower.contains("nvme")
        || name_lower.contains("mt79")
        || name_lower.contains("iwl")
        || name_lower.contains("wireless")
        || name_lower.contains("wifi")
    {
        return true;
    }
    let v = vendor.trim();
    let c = class.trim();
    v.eq_ignore_ascii_case("0x10de") && c.starts_with("0x03")
}

/// Evaluate and update advisory >=90°C thermal warning debounce and episode hysteresis (#338).
pub fn update_advisory_thermal_warn_state(
    temp: f64,
    thermal_protection_enabled: bool,
    now: Instant,
    warn_hot_since: &mut Option<Instant>,
    warn_episode_notified: &mut bool,
    last_thermal_alert: &mut Option<Instant>,
    alerts_enabled: bool,
) -> bool {
    let eligible = temp >= 90.0
        && (temp <= THERMAL_PROTECTION_TRIGGER_TEMP || !thermal_protection_enabled);
    if !eligible {
        *warn_hot_since = None;
        if temp < 85.0 {
            *warn_episode_notified = false;
        }
        return false;
    }

    let since = *warn_hot_since.get_or_insert(now);
    if now.saturating_duration_since(since) < Duration::from_secs(NOTICE_PERSIST_TICKS as u64) {
        return false;
    }

    if !alerts_enabled {
        return false;
    }

    let cooldown_ok = last_thermal_alert
        .map(|t| now.saturating_duration_since(t) >= Duration::from_secs(120))
        .unwrap_or(true);

    let sustained_reminder =
        now.saturating_duration_since(since) >= Duration::from_secs(120);

    if cooldown_ok && (!*warn_episode_notified || sustained_reminder) {
        *warn_episode_notified = true;
        *last_thermal_alert = Some(now);
        true
    } else {
        false
    }
}
