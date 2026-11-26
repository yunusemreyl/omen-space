use super::curves::{pct_to_pwm, pwm_enable_without_pwm_node, should_latch_max_pwm_enable};
use super::FanState;
use glob::glob;
use log::{info, warn};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

static MISSING_HWMON_LOGGED: AtomicBool = AtomicBool::new(false);

pub async fn sysfs_read_str<P: AsRef<Path>>(path: P) -> Option<String> {
    tokio::fs::read_to_string(path)
        .await
        .ok()
        .map(|s| s.trim().to_string())
}

pub async fn sysfs_read<P: AsRef<Path>>(path: P, default: i64) -> i64 {
    sysfs_read_str(path)
        .await
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(default)
}

pub async fn sysfs_exists<P: AsRef<Path>>(path: P) -> bool {
    tokio::fs::try_exists(path).await.unwrap_or(false)
}

pub async fn sysfs_write<P: AsRef<Path>, S: AsRef<str>>(path: P, val: S) -> bool {
    tokio::fs::write(path, val.as_ref().as_bytes()).await.is_ok()
}

pub async fn find_hp_hwmon() -> Option<PathBuf> {
    if let Ok(entries) = glob("/sys/class/hwmon/hwmon*/name") {
        let mut matching_paths = Vec::new();
        for entry in entries.filter_map(Result::ok) {
            if let Some(name) = sysfs_read_str(&entry).await {
                if name == "hp" || name == "hp-omen" || name == "hp_wmi" {
                    if let Some(parent) = entry.parent() {
                        matching_paths.push(parent.to_path_buf());
                    }
                }
            }
        }

        // Prioritize paths that actually contain fan1_input
        for path in &matching_paths {
            if sysfs_exists(path.join("fan1_input")).await {
                MISSING_HWMON_LOGGED.store(false, Ordering::Relaxed);
                info!("Found HP/OMEN hwmon with active fan input at {:?}", path);
                return Some(path.clone());
            }
        }

        // Fallback to first matching if none have fan1_input
        if let Some(path) = matching_paths.first() {
            MISSING_HWMON_LOGGED.store(false, Ordering::Relaxed);
            info!("Found HP/OMEN hwmon at {:?}", path);
            return Some(path.clone());
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
                    MISSING_HWMON_LOGGED.store(false, Ordering::Relaxed);
                    info!("Found HP hwmon via platform device at {:?}", first);
                    return Some(first.clone());
                }
            }
        }
    }

    // Only log once per transition so systems without hp-wmi don't spam journals (#364).
    if !MISSING_HWMON_LOGGED.swap(true, Ordering::Relaxed) {
        warn!("No HP hwmon device found");
    }
    None
}

async fn find_fallback_path(hwmon_path: &Path, fan_num: u32) -> Option<PathBuf> {
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

pub async fn detect_hardware_into(state: &mut FanState) {
    state.found_fans.clear();
    state.max_speeds.clear();
    state.fallback_paths.clear();
    state.fan_count = 0;
    state.hwmon_path = find_hp_hwmon().await;

    if let Some(ref hwmon) = state.hwmon_path {
        if let Ok(mut entries) = tokio::fs::read_dir(hwmon).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if let Ok(file_name) = entry.file_name().into_string() {
                    if file_name.starts_with("fan") && file_name.ends_with("_input") {
                        let num_str = &file_name[3..file_name.len() - 6];
                        if let Ok(num) = num_str.parse::<u32>() {
                            state.found_fans.push(num);
                            if let Some(fallback) = find_fallback_path(hwmon, num).await {
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
                info!(
                    "Sysfs fan{} max speed is unusually low ({}). Enforcing 6000 RPM safe limit.",
                    i, max_val
                );
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

pub async fn has_pwm_node(state: &FanState) -> bool {
    if let Some(ref hwmon) = state.hwmon_path {
        sysfs_exists(hwmon.join("pwm1")).await
    } else {
        false
    }
}

/// Clear the EC's sticky max-boost latch (`pwm1_enable = 0`) by transitioning through
/// `pwm1_enable = 2` (`PWM_MODE_AUTO`), which invokes `hp_wmi_fan_speed_max_reset` /
/// `omen_thermal_profile_set` in firmware before switching back to manual duty (#364, #395).
pub async fn leave_max_boost(hwmon: &Path) {
    let pwm_enable_path = hwmon.join("pwm1_enable");
    if sysfs_read(&pwm_enable_path, 2).await == 0 {
        let _ = sysfs_write(&pwm_enable_path, "2").await;
        tokio::time::sleep(tokio::time::Duration::from_millis(60)).await;
    }
}

/// Write PWM duty cycle to hwmon.
pub async fn write_pwm_duty(state: &mut FanState, pct: u32) -> bool {
    let hwmon = match state.hwmon_path {
        Some(ref h) => h.clone(),
        None => return false,
    };

    let pwm_enable_path = hwmon.join("pwm1_enable");
    let pwm_path = hwmon.join("pwm1");
    let has_pwm = sysfs_exists(&pwm_path).await;
    let now = std::time::Instant::now();

    if should_latch_max_pwm_enable(&state.mode, state.thermal_protection_active, pct, has_pwm) {
        if sysfs_exists(&pwm_enable_path).await {
            let current = sysfs_read(&pwm_enable_path, 2).await;
            if current != 0 {
                if has_pwm {
                    let _ = sysfs_write(&pwm_enable_path, "1").await;
                    let _ = sysfs_write(&pwm_path, "255").await;
                }
                let _ = sysfs_write(&pwm_enable_path, "0").await;
            }
        } else if has_pwm {
            let _ = sysfs_write(&pwm_path, "255").await;
        }
        state.last_written_duty = Some(255);
        state.last_written_duty_time = Some(now);
        let max_speed = state.max_speeds.values().max().copied().unwrap_or(6000);
        for &fan_num in &state.found_fans.clone() {
            state.last_targets.insert(fan_num, max_speed);
        }
        return true;
    }

    let (duty, effective_pct) = if pct == 0 {
        (0, 0)
    } else if pct >= 100 {
        (255, 100)
    } else {
        let d = pct_to_pwm(pct).max(state.minimum_manual_pwm);
        (d, ((d as f64 * 100.0) / 255.0).round() as u32)
    };

    // Boards without a manual `pwm1` node can only delegate <100% back to firmware (2).
    if !has_pwm {
        if sysfs_exists(&pwm_enable_path).await {
            let current = sysfs_read(&pwm_enable_path, 2).await;
            let target_mode = pwm_enable_without_pwm_node(pct);
            if current != target_mode {
                if sysfs_write(&pwm_enable_path, target_mode.to_string()).await {
                    if current == 0 {
                        info!(
                            "Released max fan boost: pwm1_enable=2 (no manual PWM node, requested {}%)",
                            pct
                        );
                    }
                } else {
                    warn!(
                        "Failed to set pwm1_enable={} (no manual PWM node, requested {}%)",
                        target_mode, pct
                    );
                }
            }
        }
        state.last_targets.clear();
        state.last_written_duty = Some(duty);
        state.last_written_duty_time = Some(now);
        return true;
    }

    if pct == 0 {
        if sysfs_exists(&pwm_enable_path).await {
            leave_max_boost(&hwmon).await;
            if state.zero_pct_manual_pwm {
                let _ = sysfs_write(&pwm_enable_path, "1").await;
                let _ = sysfs_write(&pwm_path, "0").await;
            } else {
                let _ = sysfs_write(&pwm_enable_path, "2").await;
            }
        }
        state.last_written_duty = Some(0);
        state.last_written_duty_time = Some(now);
        for &fan_num in &state.found_fans.clone() {
            state.last_targets.insert(fan_num, 0);
        }
        return true;
    }

    if sysfs_exists(&pwm_enable_path).await {
        let current = sysfs_read(&pwm_enable_path, 2).await;
        if current != 1 {
            if current == 0 {
                leave_max_boost(&hwmon).await;
            }
            let _ = sysfs_write(&pwm_enable_path, "1").await;
        }
    }

    let should_write = state.last_written_duty != Some(duty)
        || state
            .last_written_duty_time
            .is_none_or(|t| now.duration_since(t).as_secs() >= 10);

    if !should_write {
        return true;
    }

    if sysfs_write(&pwm_path, duty.to_string()).await {
        state.last_written_duty = Some(duty);
        state.last_written_duty_time = Some(now);
        let max_speed = state.max_speeds.values().max().copied().unwrap_or(6000);
        let rpm = ((max_speed as f64 * effective_pct as f64) / 100.0).round() as u32;
        for &fan_num in &state.found_fans.clone() {
            state.last_targets.insert(fan_num, rpm);
        }
        true
    } else {
        warn!("Failed to write pwm1 duty cycle {}", duty);
        false
    }
}

/// Independent manual targets for fan 1 (CPU) and fan 2 (GPU) through `fanN_target`.
/// Falls back to shared `write_pwm_duty` when percentages are equal or `fan*_target` is absent.
pub async fn write_fan_targets(state: &mut FanState, cpu_pct: u32, gpu_pct: u32) -> bool {
    if cpu_pct == gpu_pct {
        return write_pwm_duty(state, cpu_pct).await;
    }
    let hwmon = match state.hwmon_path {
        Some(ref h) => h.clone(),
        None => return false,
    };
    let (fan1_target, fan2_target) = (hwmon.join("fan1_target"), hwmon.join("fan2_target"));
    if !sysfs_exists(&fan1_target).await || !sysfs_exists(&fan2_target).await {
        return write_pwm_duty(state, cpu_pct.max(gpu_pct)).await;
    }

    let pwm_enable_path = hwmon.join("pwm1_enable");
    if sysfs_exists(&pwm_enable_path).await {
        let current = sysfs_read(&pwm_enable_path, 2).await;
        if current == 0 {
            leave_max_boost(&hwmon).await;
        }
        if current != 1 {
            let _ = sysfs_write(&pwm_enable_path, "1").await;
        }
    }

    let minimum_pct = (state.minimum_manual_pwm * 100).div_ceil(255);
    let rpm_for = |fan: u32, pct: u32| -> u32 {
        let max_rpm = state.max_speeds.get(&fan).copied().unwrap_or(6000);
        ((max_rpm as f64 * pct.clamp(minimum_pct, 100) as f64) / 100.0).round() as u32
    };
    let (cpu_rpm, gpu_rpm) = (rpm_for(1, cpu_pct), rpm_for(2, gpu_pct));

    let now = std::time::Instant::now();
    let unchanged = state.last_written_duty.is_none()
        && state.last_targets.get(&1) == Some(&cpu_rpm)
        && state.last_targets.get(&2) == Some(&gpu_rpm)
        && state
            .last_written_duty_time
            .is_some_and(|t| now.duration_since(t).as_secs() < 10);
    if unchanged {
        return true;
    }

    if sysfs_write(&fan1_target, cpu_rpm.to_string()).await
        && sysfs_write(&fan2_target, gpu_rpm.to_string()).await
    {
        state.last_targets.insert(1, cpu_rpm);
        state.last_targets.insert(2, gpu_rpm);
        state.last_written_duty = None;
        state.last_written_duty_time = Some(now);
        true
    } else {
        warn!(
            "Failed to write fan targets (CPU {} RPM, GPU {} RPM)",
            cpu_rpm, gpu_rpm
        );
        false
    }
}

pub async fn get_target_speed(state: &FanState, fan_num: u32) -> u32 {
    if let Some(ref hwmon) = state.hwmon_path {
        let path = hwmon.join(format!("fan{}_target", fan_num));
        if sysfs_exists(&path).await {
            return sysfs_read(&path, 0).await as u32;
        }
    }

    if has_pwm_node(state).await {
        if let Some(ref hwmon) = state.hwmon_path {
            let pwm = sysfs_read(hwmon.join("pwm1"), 0).await as u32;
            let max_speed = state.max_speeds.get(&fan_num).copied().unwrap_or(6000);
            return ((max_speed as f64 * pwm as f64) / 255.0).round() as u32;
        }
    }

    0
}
