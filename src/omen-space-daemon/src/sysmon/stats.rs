use std::fs;
pub use omen_types::SystemStats;
use crate::sysmon::sensors::*;
use crate::sysmon::gpu::*;
use crate::sysmon::utils::*;
/// Instantaneous, Zero-Fork Telemetry Fetch
pub fn fetch_system_stats() -> SystemStats {
    let mut stats = SystemStats::default();
    let paths = {
        let mut guard = SENSOR_PATHS.lock().unwrap_or_else(|e| e.into_inner());
        let (cached_paths, last_update) = guard.get_or_insert_with(|| {
            (init_sensor_paths(), std::time::Instant::now())
        });
        if (cached_paths.fan1_path.is_none() || cached_paths.fan2_path.is_none()) && last_update.elapsed().as_secs() > 5 {
            *cached_paths = init_sensor_paths();
            *last_update = std::time::Instant::now();
        }
        cached_paths.clone()
    };

    // ── 1. Instantaneous CPU Load from /proc/stat (Delta Calculation) ──
    if let Ok(stat_content) = fs::read_to_string("/proc/stat") {
        if let Some(first_line) = stat_content.lines().next() {
            if first_line.starts_with("cpu ") {
                let parts: Vec<u64> = first_line
                    .split_whitespace()
                    .skip(1)
                    .filter_map(|s| s.parse::<u64>().ok())
                    .collect();
                if parts.len() >= 4 {
                    let idle = parts[3] + parts.get(4).unwrap_or(&0); // idle + iowait
                    let total: u64 = parts.iter().sum();
                    
                    let mut prev_guard = PREV_JIFFIES.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(ref prev) = *prev_guard {
                        let total_diff = total.saturating_sub(prev.total);
                        let idle_diff = idle.saturating_sub(prev.idle);
                        if total_diff > 0 {
                            let load = 1.0 - (idle_diff as f64 / total_diff as f64);
                            stats.cpu_load = load.clamp(0.0, 1.0);
                        }
                    }
                    *prev_guard = Some(CpuJiffies { total, idle });
                }
            }
        }
    }

    // ── 2. RAM from /proc/meminfo ──────────────────────────────
    if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
        let mut total = 0.0;
        let mut available = 0.0;
        for line in meminfo.lines() {
            if line.starts_with("MemTotal:") {
                total = parse_kb(line);
            } else if line.starts_with("MemAvailable:") {
                available = parse_kb(line);
            }
        }
        let used = (total - available).max(0.0);
        stats.ram_total_gb = total / (1024.0 * 1024.0);
        stats.ram_used_gb = used / (1024.0 * 1024.0);
        if total > 0.0 {
            stats.ram_frac = used / total;
        }
    }

    // ── 3. Disk Usage using libc::statvfs (instantaneous, zero-fork) ─────────
    unsafe {
        let mut stat: libc::statvfs = std::mem::zeroed();
        let path = std::ffi::CString::new("/").unwrap();
        if libc::statvfs(path.as_ptr(), &mut stat) == 0 {
            let block_size = stat.f_frsize as f64;
            let total_bytes = stat.f_blocks as f64 * block_size;
            let free_bytes = stat.f_bavail as f64 * block_size;
            let used_bytes = (total_bytes - free_bytes).max(0.0);
            let total_gb = total_bytes / (1024.0 * 1024.0 * 1024.0);
            let used_gb = used_bytes / (1024.0 * 1024.0 * 1024.0);
            stats.disk_total_gb = total_gb;
            stats.disk_used_gb = used_gb;
            if total_gb > 0.0 {
                stats.disk_frac = (used_gb / total_gb).clamp(0.0, 1.0);
            }
        }
    }

    // ── 4. CPU Temp & Power from Direct Cached Paths ───────────
    if let Some(ref p) = paths.cpu_temp_path {
        if let Ok(s) = fs::read_to_string(p) {
            if let Ok(milli) = s.trim().parse::<f64>() {
                let raw = (milli / 1000.0) as i32;
                // Apply EWMA smoothing: fast up, slow down
                stats.cpu_temp = smooth_cpu_temp(raw);
            }
        }
    }
    if let Some(ref p) = paths.rapl_energy_path {
        if let Ok(s) = fs::read_to_string(p) {
            if let Ok(energy) = s.trim().parse::<u64>() {
                let now = std::time::Instant::now();
                let mut prev_guard = PREV_RAPL.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(ref prev) = *prev_guard {
                    let elapsed = now.duration_since(prev.time).as_secs_f64();
                    if elapsed > 0.0 {
                        let diff = energy.saturating_sub(prev.energy_uj);
                        stats.cpu_pwr = (diff as f64 / elapsed) / 1_000_000.0;
                    }
                }
                *prev_guard = Some(RaplState { energy_uj: energy, time: now });
            }
        }
    } else if let Some(ref p) = paths.cpu_pwr_path {
        if let Ok(s) = fs::read_to_string(p) {
            if let Ok(micro) = s.trim().parse::<f64>() {
                stats.cpu_pwr = micro / 1_000_000.0;
            }
        }
    }

    // ── 5. Fan Speeds from Direct Cached Paths ─────────────────
    if let Some(ref p) = paths.fan1_path {
        if let Ok(s) = fs::read_to_string(p) {
            if let Ok(rpm) = s.trim().parse::<i32>() {
                stats.fan1_rpm = rpm;
            }
        }
    }
    if let Some(ref p) = paths.fan2_path {
        if let Ok(s) = fs::read_to_string(p) {
            if let Ok(rpm) = s.trim().parse::<i32>() {
                stats.fan2_rpm = rpm;
            }
        }
    }
    // ── EC duty-cycle RPM fallback for boards without hwmon fan_input ──────
    // Board 8A42 (and siblings like 8A43, 8E35) have no fan1_input/fan2_input
    // in the hp_wmi hwmon; the amdgpu sensor reports 0. Read EC registers 0x2E
    // (fan1 duty %) and 0x2F (fan2 duty %) and approximate RPM as duty×50
    // (range 0–5000, typical max ~4800 RPM). Tagged "(EC approx)" in the
    // diagnostic report. Closes #233.
    if stats.fan1_rpm == 0 && stats.fan2_rpm == 0 {
        let board_id = std::fs::read_to_string("/sys/class/dmi/id/board_name")
            .unwrap_or_default();
        if crate::ec::LinuxEcController::needs_ec_fallback_for_board(board_id.trim()) {
            use std::io::{Read, Seek, SeekFrom};
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .read(true)
                .open("/sys/kernel/debug/ec/ec0/io")
            {
                let mut buf = [0u8; 1];
                // Fan 1 duty %
                if f.seek(SeekFrom::Start(0x2E)).is_ok()
                    && f.read_exact(&mut buf).is_ok()
                    && buf[0] > 0
                {
                    stats.fan1_rpm = (buf[0] as i32).saturating_mul(50);
                }
                // Fan 2 duty %
                if f.seek(SeekFrom::Start(0x2F)).is_ok()
                    && f.read_exact(&mut buf).is_ok()
                    && buf[0] > 0
                {
                    stats.fan2_rpm = (buf[0] as i32).saturating_mul(50);
                }
            }
        }
    }
    stats.fan_rpm = stats.fan1_rpm.max(stats.fan2_rpm);

    // ── 6. Thermal Throttle Count ──────────────────────────────
    for p in &paths.throttle_paths {
        if let Ok(s) = fs::read_to_string(p) {
            if let Ok(c) = s.trim().parse::<u32>() {
                stats.cpu_throttle_count += c;
            }
        }
    }

    // ── 7. GPU Telemetry ───────────────────────────────────────────
    let nvidia_state = check_nvidia_state();
    let is_nvidia_awake = nvidia_state.unwrap_or(false);
    let has_nvidia = nvidia_state.is_some();
    let mut nvml_timed_out = false;

    // 1. Check if we are in an intentional quiet window to let the driver sleep
    let in_cooldown = {
        let guard = GPU_IDLE_COOLDOWN.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(until) = *guard {
            std::time::Instant::now() < until
        } else {
            false
        }
    };

    // 2. If NVIDIA is sleeping (D3cold) OR in cooldown, DO NOT TOUCH NVML OR HWMON AT ALL
    if has_nvidia && (!is_nvidia_awake || in_cooldown) {
        stats.gpu_temp = 0;
        stats.gpu_pwr = -1.0;
    } else if is_nvidia_awake {
        // 3. GPU is active AND cooldown expired. Safe to sample NVML for active processes.
        let mut has_active_clients = false;

        if let Some(metrics) = fetch_gpu_metrics_with_timeout() {
            has_active_clients = metrics.has_clients;
            if has_active_clients {
                // Workload running: clear cooldown and stream live metrics
                {
                    let mut guard = GPU_IDLE_COOLDOWN.lock().unwrap_or_else(|e| e.into_inner());
                    *guard = None;
                }

                if let Some(t) = metrics.temp {
                    stats.gpu_temp = t;
                }
                if let Some(p) = metrics.power {
                    stats.gpu_pwr = p;
                }
            }
        }
        else {
            // NVML timed out! The NVIDIA driver is busy/hanging.
            nvml_timed_out = true; 
        }
        if !has_active_clients {
            // Zero game/render clients detected. Arm 6s quiet window so kernel can suspend.
            let mut guard = GPU_IDLE_COOLDOWN.lock().unwrap_or_else(|e| e.into_inner());
            *guard = Some(std::time::Instant::now() + std::time::Duration::from_secs(6));
            stats.gpu_temp = 0;
            stats.gpu_pwr = -1.0;
        }
    }

    // Fallback to sysfs hwmon GPU paths if NVML did not yield metrics (e.g. open source drivers like nouveau/amdgpu)
    if stats.gpu_temp == 0 && !has_nvidia && !nvml_timed_out {
        if let Some(ref p) = paths.gpu_temp_path {
            if let Ok(s) = fs::read_to_string(p) {
                if let Ok(milli) = s.trim().parse::<f64>() {
                    stats.gpu_temp = (milli / 1000.0) as i32;
                }
            }
        }
    }
    if stats.gpu_pwr <= 0.0 && !has_nvidia && !nvml_timed_out {
        if let Some(ref p) = paths.gpu_pwr_path {
            if let Ok(s) = fs::read_to_string(p) {
                if let Ok(micro) = s.trim().parse::<f64>() {
                    stats.gpu_pwr = micro / 1_000_000.0;
                }
            }
        }
    }

    // Total System Power
    let mut real_pwr = false;
    if stats.cpu_pwr > 0.0 || stats.gpu_pwr > 0.0 {
        stats.total_pwr = stats.cpu_pwr + stats.gpu_pwr + 11.5;
        real_pwr = true;
    } else {
        // Fallback to battery discharge wattage
        if let Some(ref p) = paths.battery_pwr_path {
            if let Ok(s) = fs::read_to_string(p) {
                if let Ok(micro) = s.trim().parse::<f64>() {
                    stats.total_pwr = micro / 1_000_000.0;
                    real_pwr = true;
                }
            }
        }
        if stats.total_pwr == 0.0 {
            stats.total_pwr = 16.0;
        }
    }

    if stats.cpu_temp == 0 { stats.cpu_temp = 45; }
    if stats.gpu_temp == 0 { stats.gpu_temp = stats.cpu_temp.saturating_sub(4); }
    if stats.cpu_pwr == 0.0 && !real_pwr { stats.cpu_pwr = stats.total_pwr * 0.45; }
    if stats.gpu_pwr == 0.0 && !real_pwr { stats.gpu_pwr = stats.total_pwr * 0.15; }

    if has_nvidia && !is_nvidia_awake {
        stats.gpu_pwr = -1.0;
    }

    // Sanitize any NaNs that might crash JSON serialization
    if stats.cpu_load.is_nan() { stats.cpu_load = 0.0; }
    if stats.cpu_pwr.is_nan() { stats.cpu_pwr = 0.0; }
    if stats.gpu_load.is_nan() { stats.gpu_load = 0.0; }
    if stats.gpu_pwr.is_nan() { stats.gpu_pwr = 0.0; }
    if stats.ram_frac.is_nan() { stats.ram_frac = 0.0; }
    if stats.disk_frac.is_nan() { stats.disk_frac = 0.0; }
    if stats.total_pwr.is_nan() { stats.total_pwr = 0.0; }

    // ── 8. Chassis / IR Temperature (WMI 0x23 via hp-wmi sysfs) ──────────
    // The file is only created when the firmware supports the query, so a
    // missing file is a normal "not supported" condition, not an error.
    let chassis_sysfs = "/sys/devices/platform/hp-wmi/chassis_temp";
    if let Ok(s) = fs::read_to_string(chassis_sysfs) {
        if let Ok(v) = s.trim().parse::<i32>() {
            stats.chassis_temp = v;
        }
    }

    // ── 9. Board verification flag ────────────────────────────────────────
    let board_id = fs::read_to_string("/sys/class/dmi/id/board_name")
        .unwrap_or_default();
    stats.board_verified =
        crate::capabilities::LinuxCapabilityClassifier::is_board_verified(board_id.trim());

    stats
}

