//! HP Battery Charge Limiter engine.
//!
//! Hysteresis state machine, safe firmware mode whitelist (`auto` / `inhibit` /
//! `hold`), and shutdown hold policy adapted from `kralonur/hp-charge-limit-rs`
//! (MIT / Apache-2.0): https://github.com/kralonur/hp-charge-limit-rs
//!
//! Backend priority:
//!   1. Native Linux sysfs threshold (`/sys/class/power_supply/BAT*/charge_control_end_threshold`)
//!   2. `hp-wmi` WMI `0x2B` sysfs attribute (`/sys/devices/platform/hp-wmi/battery_charge_mode`)
//!      with a hysteretic `Auto` (`0x0000`) <-> `Inhibit` (`0x0500`) <-> `Hold` (`0x0300`) loop.

use glob::glob;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

pub const HP_WMI_BATTERY_CHARGE_MODE_PATH: &str = "/sys/devices/platform/hp-wmi/battery_charge_mode";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChargeMode {
    Auto,
    Inhibit,
    Hold,
}

impl ChargeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Inhibit => "inhibit",
            Self::Hold => "hold",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LimiterState {
    Disabled,
    Starting,
    Charging,
    Limiting,
    Bypassed,
}

impl LimiterState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Starting => "starting",
            Self::Charging => "charging",
            Self::Limiting => "limiting",
            Self::Bypassed => "bypassed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    NativeThreshold,
    HpWmiMode,
    None,
}

impl BackendKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NativeThreshold => "native_threshold",
            Self::HpWmiMode => "hp_wmi_0x2b",
            Self::None => "none",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatteryLimitConfig {
    pub enabled: bool,
    pub stop_pct: u8,
    pub start_pct: u8,
    pub off_hold: bool,
    pub bypass: bool,
}

impl BatteryLimitConfig {
    pub fn sanitized(mut self) -> Self {
        self.stop_pct = self.stop_pct.clamp(50, 95);
        let max_start = self.stop_pct.saturating_sub(1).max(20);
        self.start_pct = self.start_pct.clamp(20, max_start);
        if !self.enabled {
            self.bypass = false;
        }
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatteryReading {
    pub capacity: u8,
    pub on_ac: bool,
    pub is_full: bool,
}

pub trait ChargeModeWriter {
    fn set_mode(&mut self, mode: ChargeMode) -> Result<(), String>;
    fn read_mode(&self) -> Option<String>;
}

pub struct HpWmiSysfsBackend {
    pub path: PathBuf,
}

impl HpWmiSysfsBackend {
    pub fn system() -> Self {
        Self {
            path: PathBuf::from(HP_WMI_BATTERY_CHARGE_MODE_PATH),
        }
    }

    pub fn is_supported(&self) -> bool {
        self.read_mode()
            .map(|m| !m.is_empty() && m != "no_battery")
            .unwrap_or(false)
    }
}

impl ChargeModeWriter for HpWmiSysfsBackend {
    fn set_mode(&mut self, mode: ChargeMode) -> Result<(), String> {
        std::fs::write(&self.path, format!("{}\n", mode.as_str()))
            .map_err(|e| format!("{}: {e}", self.path.display()))
    }

    fn read_mode(&self) -> Option<String> {
        std::fs::read_to_string(&self.path)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }
}

/// Returns writable sysfs `charge_control_end_threshold` node paths, if any exist.
pub fn find_native_threshold_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(entries) = glob("/sys/class/power_supply/BAT*/charge_control_end_threshold") {
        for path in entries.filter_map(Result::ok) {
            if OpenOptions::new().write(true).open(&path).is_ok() {
                paths.push(path);
            }
        }
    }
    paths
}

pub fn apply_native_threshold(limit_pct: u8) -> Result<(), String> {
    let paths = find_native_threshold_paths();
    if paths.is_empty() {
        return Err("No writable charge_control_end_threshold node found".to_string());
    }
    let val = limit_pct.clamp(50, 100).to_string();
    let mut wrote = false;
    let mut last_err = None;
    for path in paths {
        match std::fs::write(&path, &val) {
            Ok(()) => wrote = true,
            Err(e) => last_err = Some(format!("{}: {e}", path.display())),
        }
    }
    if wrote {
        Ok(())
    } else {
        Err(last_err.unwrap_or_else(|| "Failed to write threshold".to_string()))
    }
}

pub fn detect_backend() -> BackendKind {
    if !find_native_threshold_paths().is_empty() {
        BackendKind::NativeThreshold
    } else if HpWmiSysfsBackend::system().is_supported() {
        BackendKind::HpWmiMode
    } else {
        BackendKind::None
    }
}

pub fn hp_wmi_charge_mode_supported() -> bool {
    Path::new(HP_WMI_BATTERY_CHARGE_MODE_PATH).exists()
        && HpWmiSysfsBackend::system().is_supported()
}

pub fn read_sysfs_battery_reading() -> Option<BatteryReading> {
    let bat = crate::sysmon::battery::current_battery()?;
    Some(BatteryReading {
        capacity: bat.charge_pct,
        on_ac: bat.ac_online,
        is_full: bat.status == omen_types::BatteryStatus::Full,
    })
}

/// Hysteretic firmware charge controller modeled after `kralonur/hp-charge-limit-rs`.
#[derive(Debug)]
pub struct HysteresisLimiter {
    pub state: LimiterState,
    last_written_mode: Option<ChargeMode>,
    failing: bool,
}

impl Default for HysteresisLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl HysteresisLimiter {
    pub fn new() -> Self {
        Self {
            state: LimiterState::Starting,
            last_written_mode: None,
            failing: false,
        }
    }

    pub fn last_written_mode(&self) -> Option<ChargeMode> {
        self.last_written_mode
    }

    fn write_mode(
        &mut self,
        writer: &mut impl ChargeModeWriter,
        mode: ChargeMode,
        reason: &str,
        capacity: u8,
    ) -> bool {
        match writer.set_mode(mode) {
            Ok(()) => {
                if self.last_written_mode != Some(mode) || self.failing {
                    info!("BatteryLimiter [{reason}]: cap={capacity}% -> {}", mode.as_str());
                }
                self.last_written_mode = Some(mode);
                self.failing = false;
                true
            }
            Err(e) => {
                if !self.failing {
                    warn!("BatteryLimiter [{reason}]: cap={capacity}% -> {} failed: {e}", mode.as_str());
                }
                self.failing = true;
                false
            }
        }
    }

    /// Force fresh evaluation on the next tick (called when user updates stop/start/enabled).
    pub fn reset_for_reconfigure(&mut self) {
        if self.state != LimiterState::Disabled {
            self.state = LimiterState::Starting;
        }
    }

    /// Evaluates one poll cycle against `reading` and `cfg`.
    /// Returns `true` if `cfg.bypass` was automatically cleared after reaching full charge.
    pub fn tick(
        &mut self,
        reading: BatteryReading,
        cfg: &mut BatteryLimitConfig,
        writer: &mut impl ChargeModeWriter,
    ) -> bool {
        let mut cleared_bypass = false;
        let sanitized = cfg.sanitized();
        *cfg = sanitized;

        if !cfg.enabled {
            if self.state != LimiterState::Disabled || self.last_written_mode != Some(ChargeMode::Auto) {
                if self.write_mode(writer, ChargeMode::Auto, "disable", reading.capacity) {
                    self.state = LimiterState::Disabled;
                }
            }
            return false;
        }

        if cfg.bypass && (reading.is_full || reading.capacity >= 99) {
            info!("BatteryLimiter: 100% full charge reached; clearing one-shot bypass");
            cfg.bypass = false;
            cleared_bypass = true;
        }

        // Do not alter EC charge flags while unplugged on battery power.
        if !reading.on_ac {
            if self.state == LimiterState::Disabled {
                self.state = LimiterState::Starting;
            }
            return cleared_bypass;
        }

        if cfg.bypass {
            if self.state == LimiterState::Bypassed {
                self.write_mode(writer, ChargeMode::Auto, "bypass_keep", reading.capacity);
            } else if self.write_mode(writer, ChargeMode::Auto, "bypass", reading.capacity) {
                self.state = LimiterState::Bypassed;
            }
            return cleared_bypass;
        }

        if matches!(
            self.state,
            LimiterState::Disabled | LimiterState::Starting | LimiterState::Bypassed
        ) {
            let (target_mode, next_state, reason) = if reading.capacity <= cfg.start_pct {
                (ChargeMode::Auto, LimiterState::Charging, "start_below")
            } else {
                (ChargeMode::Inhibit, LimiterState::Limiting, "start_limit")
            };
            if self.write_mode(writer, target_mode, reason, reading.capacity) {
                self.state = next_state;
            }
            return cleared_bypass;
        }

        match self.state {
            LimiterState::Charging => {
                if reading.capacity >= cfg.stop_pct {
                    if self.write_mode(writer, ChargeMode::Inhibit, "stop_reached", reading.capacity) {
                        self.state = LimiterState::Limiting;
                    }
                } else if reading.capacity <= cfg.start_pct {
                    self.write_mode(writer, ChargeMode::Auto, "reassert_auto", reading.capacity);
                }
            }
            LimiterState::Limiting => {
                if reading.capacity <= cfg.start_pct {
                    if self.write_mode(writer, ChargeMode::Auto, "resume_charge", reading.capacity) {
                        self.state = LimiterState::Charging;
                    }
                } else if reading.capacity >= cfg.stop_pct {
                    self.write_mode(writer, ChargeMode::Inhibit, "reassert_inhibit", reading.capacity);
                }
            }
            LimiterState::Disabled | LimiterState::Starting | LimiterState::Bypassed => {}
        }

        cleared_bypass
    }

    /// Applies the configured off-state firmware charge mode before daemon exit / shutdown.
    pub fn on_shutdown(
        &mut self,
        reading: Option<BatteryReading>,
        cfg: &BatteryLimitConfig,
        writer: &mut impl ChargeModeWriter,
    ) -> Option<ChargeMode> {
        let cap = reading.map(|r| r.capacity).unwrap_or(100);
        if !cfg.enabled || cfg.bypass {
            if self.state != LimiterState::Disabled {
                let _ = self.write_mode(writer, ChargeMode::Auto, "shutdown_auto", cap);
            }
            return Some(ChargeMode::Auto);
        }

        let target = if cfg.off_hold && cap > cfg.start_pct {
            ChargeMode::Hold
        } else {
            ChargeMode::Auto
        };
        self.write_mode(writer, target, "shutdown", cap)
            .then_some(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeWriter {
        calls: Vec<ChargeMode>,
        fail_next: bool,
        current: String,
    }

    impl FakeWriter {
        fn new() -> Self {
            Self {
                calls: Vec::new(),
                fail_next: false,
                current: "auto".to_string(),
            }
        }
    }

    impl ChargeModeWriter for FakeWriter {
        fn set_mode(&mut self, mode: ChargeMode) -> Result<(), String> {
            if self.fail_next {
                self.fail_next = false;
                return Err("simulated WMI error".to_string());
            }
            self.calls.push(mode);
            self.current = mode.as_str().to_string();
            Ok(())
        }

        fn read_mode(&self) -> Option<String> {
            Some(self.current.clone())
        }
    }

    fn cfg(stop: u8, start: u8) -> BatteryLimitConfig {
        BatteryLimitConfig {
            enabled: true,
            stop_pct: stop,
            start_pct: start,
            off_hold: true,
            bypass: false,
        }
    }

    #[test]
    fn hysteresis_band_transitions_and_deadband() {
        let mut lim = HysteresisLimiter::new();
        let mut fw = FakeWriter::new();
        let mut c = cfg(80, 75);

        // Starting below start_pct enables charging (`Auto`).
        lim.tick(BatteryReading { capacity: 60, on_ac: true, is_full: false }, &mut c, &mut fw);
        assert_eq!(lim.state, LimiterState::Charging);
        assert_eq!(fw.calls.last().copied(), Some(ChargeMode::Auto));

        // At 78% inside deadband while Charging: stays Charging.
        lim.tick(BatteryReading { capacity: 78, on_ac: true, is_full: false }, &mut c, &mut fw);
        assert_eq!(lim.state, LimiterState::Charging);

        // Reaching stop_pct (80%) switches to Limiting (`Inhibit`).
        lim.tick(BatteryReading { capacity: 80, on_ac: true, is_full: false }, &mut c, &mut fw);
        assert_eq!(lim.state, LimiterState::Limiting);
        assert_eq!(fw.calls.last().copied(), Some(ChargeMode::Inhibit));

        // Dropping to 77% inside deadband stays Limiting.
        lim.tick(BatteryReading { capacity: 77, on_ac: true, is_full: false }, &mut c, &mut fw);
        assert_eq!(lim.state, LimiterState::Limiting);

        // Dropping to start_pct (75%) resumes Charging (`Auto`).
        lim.tick(BatteryReading { capacity: 75, on_ac: true, is_full: false }, &mut c, &mut fw);
        assert_eq!(lim.state, LimiterState::Charging);
        assert_eq!(fw.calls.last().copied(), Some(ChargeMode::Auto));
    }

    #[test]
    fn unplugged_battery_ticks_leave_firmware_untouched() {
        let mut lim = HysteresisLimiter::new();
        let mut fw = FakeWriter::new();
        let mut c = cfg(80, 75);

        lim.tick(BatteryReading { capacity: 82, on_ac: true, is_full: false }, &mut c, &mut fw);
        assert_eq!(fw.calls.len(), 1);

        // Running on battery should not emit WMI writes even below start_pct.
        lim.tick(BatteryReading { capacity: 50, on_ac: false, is_full: false }, &mut c, &mut fw);
        assert_eq!(fw.calls.len(), 1);
        assert_eq!(lim.state, LimiterState::Limiting);
    }

    #[test]
    fn bypass_allows_full_charge_and_auto_clears_at_100_pct() {
        let mut lim = HysteresisLimiter::new();
        let mut fw = FakeWriter::new();
        let mut c = cfg(80, 75);

        lim.tick(BatteryReading { capacity: 80, on_ac: true, is_full: false }, &mut c, &mut fw);
        assert_eq!(lim.state, LimiterState::Limiting);

        c.bypass = true;
        assert!(!lim.tick(BatteryReading { capacity: 85, on_ac: true, is_full: false }, &mut c, &mut fw));
        assert_eq!(lim.state, LimiterState::Bypassed);
        assert_eq!(fw.calls.last().copied(), Some(ChargeMode::Auto));

        // Reaching full charge automatically clears one-shot bypass and resumes limiting.
        let cleared = lim.tick(BatteryReading { capacity: 100, on_ac: true, is_full: true }, &mut c, &mut fw);
        assert!(cleared);
        assert!(!c.bypass);
        assert_eq!(lim.state, LimiterState::Limiting);
        assert_eq!(fw.calls.last().copied(), Some(ChargeMode::Inhibit));
    }

    #[test]
    fn write_failure_keeps_state_and_retries_next_tick() {
        let mut lim = HysteresisLimiter::new();
        let mut fw = FakeWriter::new();
        let mut c = cfg(80, 75);

        lim.tick(BatteryReading { capacity: 70, on_ac: true, is_full: false }, &mut c, &mut fw);
        assert_eq!(lim.state, LimiterState::Charging);

        fw.fail_next = true;
        lim.tick(BatteryReading { capacity: 80, on_ac: true, is_full: false }, &mut c, &mut fw);
        assert_eq!(lim.state, LimiterState::Charging);

        lim.tick(BatteryReading { capacity: 80, on_ac: true, is_full: false }, &mut c, &mut fw);
        assert_eq!(lim.state, LimiterState::Limiting);
        assert_eq!(fw.calls.last().copied(), Some(ChargeMode::Inhibit));
    }

    #[test]
    fn shutdown_hold_respects_off_hold_and_low_battery_floor() {
        let mut lim = HysteresisLimiter::new();
        let mut fw = FakeWriter::new();
        let mut c = cfg(80, 75);

        let high = Some(BatteryReading { capacity: 79, on_ac: false, is_full: false });
        assert_eq!(lim.on_shutdown(high, &c, &mut fw), Some(ChargeMode::Hold));

        let low = Some(BatteryReading { capacity: 40, on_ac: false, is_full: false });
        assert_eq!(lim.on_shutdown(low, &c, &mut fw), Some(ChargeMode::Auto));

        c.off_hold = false;
        assert_eq!(lim.on_shutdown(high, &c, &mut fw), Some(ChargeMode::Auto));
    }
}
