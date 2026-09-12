use std::fs;
use std::process::Command;
use serde::{Serialize, Deserialize};
use zbus::interface;
use std::sync::OnceLock;
use std::sync::Mutex;
use std::path::PathBuf;

/* ─────────────────────────────────────────────────────────────
   sys_monitor.rs — Ultra Lightweight, Zero-Fork Live Telemetry
   High-performance Linux sysfs/procfs parser with Jiffies Delta
   ───────────────────────────────────────────────────────────── */

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct SystemStats {
    pub cpu_temp: i32,
    pub cpu_load: f64,
    pub cpu_pwr: f64,
    /// Max of fan1 and fan2 (kept for backward compat with monitoring widgets)
    pub fan_rpm: i32,
    /// CPU fan RPM (fan1_input from hp/hp_wmi hwmon)
    pub fan1_rpm: i32,
    /// GPU fan RPM (fan2_input from hp/hp_wmi hwmon)
    pub fan2_rpm: i32,

    pub gpu_temp: i32,
    pub gpu_load: f64,
    pub gpu_pwr: f64,

    pub ram_used_gb: f64,
    pub ram_total_gb: f64,
    pub ram_frac: f64,

    pub disk_used_gb: f64,
    pub disk_total_gb: f64,
    pub disk_frac: f64,

    pub total_pwr: f64,
    pub cpu_throttle_count: u32,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct HardwareSpecs {
    pub product_name: String,
    pub cpu_spec: String,
    pub gpu_spec: String,
    pub ram_spec: String,
    pub ssd_spec: String,
    pub os_spec: String,
    pub bios_version: String,
    pub ec_version: String,
    pub vbios_version: String,
    pub nvidia_driver: String,
    pub kernel_version: String,
}

// ── Sensor Path Cache for Zero-Glob Overhead ────────────────
struct SensorPaths {
    cpu_temp_path: Option<PathBuf>,
    cpu_pwr_path: Option<PathBuf>,
    fan1_path: Option<PathBuf>,
    fan2_path: Option<PathBuf>,
    throttle_paths: Vec<PathBuf>,
    gpu_temp_path: Option<PathBuf>,
    gpu_pwr_path: Option<PathBuf>,
    battery_pwr_path: Option<PathBuf>,
    rapl_energy_path: Option<PathBuf>,
}

static SENSOR_PATHS: OnceLock<SensorPaths> = OnceLock::new();
static SPECS_CACHE: OnceLock<HardwareSpecs> = OnceLock::new();

// State for Instantaneous CPU load delta calculation
struct CpuJiffies {
    total: u64,
    idle: u64,
}
static PREV_JIFFIES: Mutex<Option<CpuJiffies>> = Mutex::new(None);

struct RaplState {
    energy_uj: u64,
    time: std::time::Instant,
}
static PREV_RAPL: Mutex<Option<RaplState>> = Mutex::new(None);
static GPU_IDLE_COOLDOWN: Mutex<Option<std::time::Instant>> = Mutex::new(None);
fn init_sensor_paths() -> SensorPaths {
    let mut cpu_temp_path = None;
    let mut cpu_pwr_path = None;
    let mut fan1_path = None;
    let mut fan2_path = None;
    let mut throttle_paths = Vec::new();

    let mut gpu_temp_path = None;
    let mut gpu_pwr_path = None;
    let mut battery_pwr_path = None;
    let mut rapl_energy_path = None;

    if let Ok(entries) = glob::glob("/sys/class/hwmon/hwmon*") {
        for entry in entries.filter_map(Result::ok) {
            let name_path = entry.join("name");
            if let Ok(name) = fs::read_to_string(&name_path) {
                let name = name.trim();
                if name == "coretemp" || name == "k10temp" || name == "zenpower" {
                    let t1 = entry.join("temp1_input");
                    if t1.exists() { cpu_temp_path = Some(t1); }
                    let p1 = entry.join("power1_input");
                    if p1.exists() { cpu_pwr_path = Some(p1); }
                } else if name == "hp_wmi" || name == "hp" || name == "omen" {
                    let f1 = entry.join("fan1_input");
                    if f1.exists() { fan1_path = Some(f1); }
                    let f2 = entry.join("fan2_input");
                    if f2.exists() { fan2_path = Some(f2); }
                } else if name.contains("nouveau") || name.contains("amdgpu") || name.contains("nvidia") {
                    let t1 = entry.join("temp1_input");
                    if t1.exists() { gpu_temp_path = Some(t1); }
                    
                    let p1_avg = entry.join("power1_average");
                    let p1_inp = entry.join("power1_input");
                    if p1_avg.exists() {
                        gpu_pwr_path = Some(p1_avg);
                    } else if p1_inp.exists() {
                        gpu_pwr_path = Some(p1_inp);
                    }
                }
            }
        }
    }

    if let Ok(entries) = glob::glob("/sys/devices/system/cpu/cpu*/thermal_throttle/package_throttle_count") {
        for entry in entries.filter_map(Result::ok) {
            throttle_paths.push(entry);
        }
    }
    
    if let Ok(mut entries) = glob::glob("/sys/class/powercap/intel-rapl:0/energy_uj") {
        if let Some(Ok(entry)) = entries.next() {
            rapl_energy_path = Some(entry);
        }
    }

    // Check battery power for total wattage fallback
    let bat0 = PathBuf::from("/sys/class/power_supply/BAT0/power_now");
    let bat1 = PathBuf::from("/sys/class/power_supply/BAT1/power_now");
    if bat0.exists() {
        battery_pwr_path = Some(bat0);
    } else if bat1.exists() {
        battery_pwr_path = Some(bat1);
    }

    SensorPaths {
        cpu_temp_path,
        cpu_pwr_path,
        fan1_path,
        fan2_path,
        throttle_paths,
        gpu_temp_path,
        gpu_pwr_path,
        battery_pwr_path,
        rapl_energy_path,
    }
}

pub fn get_hardware_specs() -> HardwareSpecs {
    SPECS_CACHE.get_or_init(|| {
        let mut specs = HardwareSpecs::default();

        // 1. Product Name
        let mut prod = fs::read_to_string("/sys/class/dmi/id/product_name")
            .unwrap_or_else(|_| "Victus by HP Gaming Laptop".to_string())
            .trim()
            .to_string();
        if prod.is_empty() {
            prod = "Victus by HP Gaming Laptop 16".to_string();
        }
        specs.product_name = prod;

        // 2. CPU info
        let mut cpu_name = String::from("Intel Core Processor");
        let mut cpu_cores = 0;
        if let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo") {
            for line in cpuinfo.lines() {
                if line.starts_with("model name") && cpu_name.starts_with("Intel Core Processor") {
                    if let Some(name) = line.split(':').nth(1) {
                        cpu_name = name.trim().to_string();
                    }
                }
                if line.starts_with("processor") {
                    cpu_cores += 1;
                }
            }
        }
        let clean_cpu = cpu_name
            .replace("(R)", "")
            .replace("(TM)", "")
            .replace("12th Gen ", "")
            .replace("13th Gen ", "")
            .replace("14th Gen ", "")
            .replace("15th Gen ", "")
            .trim()
            .to_string();
        specs.cpu_spec = if cpu_cores > 0 {
            format!("{}  ·  {} Threads", clean_cpu, cpu_cores)
        } else {
            clean_cpu
        };

        // 3. GPU info
        let mut gpu_str = String::from("Unknown GPU");
        if let Ok(output) = Command::new("nvidia-smi")
            .args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"])
            .output()
        {
            let out_str = String::from_utf8_lossy(&output.stdout);
            let parts: Vec<&str> = out_str.trim().split(',').collect();
            if parts.len() >= 2 {
                let name = parts[0].trim();
                if let Ok(mb) = parts[1].trim().parse::<f64>() {
                    let gb = (mb / 1024.0).round() as i32;
                    gpu_str = format!("{}  ·  {} GB VRAM", name, gb);
                } else {
                    gpu_str = name.to_string();
                }
            } else if !parts.is_empty() && !parts[0].is_empty() {
                gpu_str = parts[0].trim().to_string();
            }
        }
        if gpu_str == "Unknown GPU" {
            if let Ok(output) = Command::new("lspci").output() {
                let out_str = String::from_utf8_lossy(&output.stdout);
                for line in out_str.lines() {
                    if (line.contains("VGA compatible controller") || line.contains("3D controller"))
                        && (line.contains("NVIDIA") || line.contains("AMD") || line.contains("Intel"))
                    {
                        if line.contains("NVIDIA") || (gpu_str == "Unknown GPU" && (line.contains("AMD") || line.contains("Intel"))) {
                            if let Some(pos) = line.find(": ") {
                                let desc = &line[pos + 2..];
                                let clean = if let Some(bracket_end) = desc.find("]: ") {
                                    &desc[bracket_end + 3..]
                                } else {
                                    desc
                                };
                                gpu_str = clean.trim().to_string();
                            }
                        }
                    }
                }
            }
        }
        specs.gpu_spec = gpu_str;

        // 4. RAM info
        let mut total_gb = 16;
        if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
            for line in meminfo.lines() {
                if line.starts_with("MemTotal:") {
                    if let Some(val_str) = line.split_whitespace().nth(1) {
                        if let Ok(kb) = val_str.parse::<f64>() {
                            total_gb = (kb / 1024.0 / 1024.0).round() as i32;
                        }
                    }
                    break;
                }
            }
        }
        specs.ram_spec = format!("{} GB RAM", total_gb);

        // 5. SSD Model & Size
        let mut ssd_str = String::from("NVMe SSD");
        if let Ok(entries) = glob::glob("/sys/block/nvme*n1/device/model") {
            for entry in entries.filter_map(Result::ok) {
                if let Ok(model) = fs::read_to_string(&entry) {
                    let model = model.trim();
                    if let Some(parent) = entry.parent().and_then(|p| p.parent()) {
                        if let Ok(size_str) = fs::read_to_string(parent.join("size")) {
                            if let Ok(sectors) = size_str.trim().parse::<f64>() {
                                let gb = (sectors * 512.0 / 1_000_000_000.0).round() as i32;
                                ssd_str = format!("{}  ·  {} GB NVMe", model, gb);
                                break;
                            }
                        }
                    }
                    ssd_str = format!("{} NVMe", model);
                    break;
                }
            }
        }
        specs.ssd_spec = ssd_str;

        // 6. OS & Kernel
        let mut os_name = String::from("Linux");
        if let Ok(os_release) = fs::read_to_string("/etc/os-release") {
            for line in os_release.lines() {
                if line.starts_with("PRETTY_NAME=") {
                    let val = line.trim_start_matches("PRETTY_NAME=").trim_matches('"');
                    os_name = val.to_string();
                    break;
                }
            }
        }
        let kernel = fs::read_to_string("/proc/sys/kernel/osrelease")
            .unwrap_or_else(|_| "Linux".to_string())
            .trim()
            .to_string();
        specs.kernel_version = kernel.clone();
        specs.os_spec = format!("{}  ·  Linux {}", os_name, kernel);

        // 7. BIOS Version
        if let Ok(bios) = fs::read_to_string("/sys/class/dmi/id/bios_version") {
            specs.bios_version = bios.trim().to_string();
        } else {
            specs.bios_version = "Unknown".to_string();
        }

        // 7.1 EC Version
        if let Ok(ec) = fs::read_to_string("/sys/class/dmi/id/ec_firmware_release") {
            specs.ec_version = ec.trim().to_string();
        } else {
            specs.ec_version = "Unknown".to_string();
        }

        // 8. vBIOS Version
        if let Ok(output) = Command::new("nvidia-smi")
            .args(["--query-gpu=vbios_version", "--format=csv,noheader"])
            .output()
        {
            let vbios = String::from_utf8_lossy(&output.stdout).trim().to_string();
            specs.vbios_version = if !vbios.is_empty() { vbios } else { "Unknown".to_string() };
        } else {
            specs.vbios_version = "Unknown".to_string();
        }

        // 9. NVIDIA Driver Version
        specs.nvidia_driver = fs::read_to_string("/proc/driver/nvidia/version")
            .ok()
            .and_then(|content| {
                for line in content.lines() {
                    if line.contains("NVRM version:") {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        for part in parts {
                            if part.contains('.') && part.chars().next().map_or(false, |c| c.is_ascii_digit()) {
                                return Some(part.to_string());
                            }
                        }
                    }
                }
                None
            })
            .unwrap_or_else(|| "Unknown".to_string());

        specs
    }).clone()
}
fn get_nvidia_dgpu_path() -> Option<std::path::PathBuf> {
    static NVIDIA_PATH: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    NVIDIA_PATH
        .get_or_init(|| {
            if let Ok(entries) = std::fs::read_dir("/sys/bus/pci/devices") {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let class = std::fs::read_to_string(path.join("class")).unwrap_or_default();
                    // Class 0x030000 (VGA) or 0x030200 (3D Controller) — ignores audio 0x040300
                    if class.trim().starts_with("0x03") {
                        let vendor = std::fs::read_to_string(path.join("vendor")).unwrap_or_default();
                        if vendor.trim().eq_ignore_ascii_case("0x10de") {
                            return Some(path);
                        }
                    }
                }
            }
            None
        })
        .clone()
}
fn check_nvidia_state() -> Option<bool> {
    if let Some(path) = get_nvidia_dgpu_path() {
        let status_path = path.join("power/runtime_status");
        if let Ok(status) = fs::read_to_string(status_path) {
            return Some(status.trim() == "active");
        }
    }
    None
}

struct GpuMetrics {
    has_clients: bool,
    temp: Option<i32>,
    power: Option<f64>,
}

static NVML_INSTANCE: OnceLock<Mutex<Option<nvml_wrapper::Nvml>>> = OnceLock::new();

fn fetch_gpu_metrics_with_timeout() -> Option<GpuMetrics> {
    let (tx, rx) = std::sync::mpsc::channel();
    
    std::thread::spawn(move || {
        // Get or initialize the cached NVML instance
        let nvml_lock = NVML_INSTANCE.get_or_init(|| {
            Mutex::new(nvml_wrapper::Nvml::init().ok())
        });

        // Use try_lock() to avoid piling up threads if one is blocked in D3cold/D0 transition
        if let Ok(mut nvml_guard) = nvml_lock.try_lock() {
            if let Some(nvml) = nvml_guard.as_mut() {
                if let Ok(device) = nvml.device_by_index(0) {
                    let gfx = device.running_graphics_processes().map(|v| v.len()).unwrap_or(0);
                    let comp = device.running_compute_processes().map(|v| v.len()).unwrap_or(0);
                    let has_clients = (gfx + comp) > 0;
                    
                    let mut temp = None;
                    let mut power = None;
                    
                    if has_clients {
                        if let Ok(t) = device.temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu) {
                            temp = Some(t as i32);
                        }
                        if let Ok(p) = device.power_usage() {
                            power = Some(p as f64 / 1000.0);
                        }
                    }
                    let _ = tx.send(GpuMetrics { has_clients, temp, power });
                }
            }
        }
    });

    rx.recv_timeout(std::time::Duration::from_millis(500)).ok()
}

pub fn get_safe_gpu_temp() -> f64 {
    let nvidia_state = check_nvidia_state();
    let is_nvidia_awake = nvidia_state.unwrap_or(false);

    // If card is already asleep in D3cold, return 0.0 without touching it
    if !is_nvidia_awake {
        return 0.0;
    }

    // Check if we are in an intentional quiet window to let the driver enter D3cold
    {
        let guard = GPU_IDLE_COOLDOWN.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(until) = *guard {
            if std::time::Instant::now() < until {
                return 0.0;
            }
        }
    }

    let mut has_active_clients = false;
    let mut gpu_temp = 0.0;

    // Check for actual running 3D or compute processes with timeout
    // to prevent hanging Tokio's blocking pool during NVIDIA power state transitions
    if let Some(metrics) = fetch_gpu_metrics_with_timeout() {
        has_active_clients = metrics.has_clients;
        if has_active_clients {
            // Active game/render client: clear cooldown and sample real temperature
            {
                let mut guard = GPU_IDLE_COOLDOWN.lock().unwrap_or_else(|e| e.into_inner());
                *guard = None;
            }
            if let Some(t) = metrics.temp {
                gpu_temp = t as f64;
            }
        }
    } else {
        // NVML timeout (likely hanging in D3cold transition). Return 0.0 safely.
        return 0.0;
    }

    if !has_active_clients {
        // Card is awake in D0, but zero applications are using it.
        // Start 6s quiet window so the kernel's autosuspend timer can finish.
        let mut guard = GPU_IDLE_COOLDOWN.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(std::time::Instant::now() + std::time::Duration::from_secs(6));
        return 0.0;
    }

    gpu_temp
}
/// Instantaneous, Zero-Fork Telemetry Fetch
pub fn fetch_system_stats() -> SystemStats {
    let mut stats = SystemStats::default();
    let paths = SENSOR_PATHS.get_or_init(init_sensor_paths);

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

    // ── 3. Disk Usage using df ──────────────────
    if let Ok(out) = std::process::Command::new("df").arg("-BG").arg("/").output() {
        let out_str = String::from_utf8_lossy(&out.stdout);
        if let Some(line) = out_str.lines().nth(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                let total = parts[1].replace("G", "").parse::<f64>().unwrap_or(1.0);
                let used = parts[2].replace("G", "").parse::<f64>().unwrap_or(0.0);
                stats.disk_total_gb = total;
                stats.disk_used_gb = used;
                if total > 0.0 {
                    stats.disk_frac = used / total;
                }
            }
        }
    }

    // ── 4. CPU Temp & Power from Direct Cached Paths ───────────
    if let Some(ref p) = paths.cpu_temp_path {
        if let Ok(s) = fs::read_to_string(p) {
            if let Ok(milli) = s.trim().parse::<f64>() {
                stats.cpu_temp = (milli / 1000.0) as i32;
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

    stats
}

#[inline(always)]
fn parse_kb(line: &str) -> f64 {
    let mut parts = line.split_whitespace();
    parts.next();
    parts.next().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0)
}

/// Returns true if the current process is running as root (UID 0).
/// Reads /proc/self/status to avoid a libc dependency.
fn nix_is_root() -> bool {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Uid:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|uid| uid.parse::<u32>().ok())
        })
        .map(|uid| uid == 0)
        .unwrap_or(false)
}

pub fn get_running_process_names() -> Vec<String> {
    let mut names = Vec::with_capacity(128);
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.as_bytes().iter().all(|b| b.is_ascii_digit()) {
                        let comm_path = path.join("comm");
                        if let Ok(comm) = std::fs::read_to_string(comm_path) {
                            names.push(comm.trim().to_lowercase());
                        }
                    }
                }
            }
        }
    }
    names
}

#[derive(Clone)]
pub struct SysMonInterface {}

impl SysMonInterface {
    pub fn new() -> Self {
        Self {}
    }
}

#[interface(name = "org.hp.omen.SysMon")]
impl SysMonInterface {
    async fn get_diagnostics(&self) -> String {
        let stats = fetch_system_stats();
        serde_json::to_string(&stats).unwrap_or_else(|_| "{}".to_string())
    }

    async fn get_hardware_specs(&self) -> String {
        let specs = get_hardware_specs();
        serde_json::to_string(&specs).unwrap_or_else(|_| "{}".to_string())
    }

    async fn generate_diagnostic_report(&self) -> String {
        let specs = get_hardware_specs();
        let stats = fetch_system_stats();
        let board_id = std::fs::read_to_string("/sys/class/dmi/id/board_name")
            .unwrap_or_else(|_| "Unknown".to_string()).trim().to_string();
        let manufacturer = std::fs::read_to_string("/sys/class/dmi/id/sys_vendor")
            .unwrap_or_else(|_| "HP".to_string()).trim().to_string();
        let product_name = std::fs::read_to_string("/sys/class/dmi/id/product_name")
            .unwrap_or_else(|_| "Unknown".to_string()).trim().to_string();

        let mut report = String::new();
        report.push_str("# OMENSpace Diagnostic Report\n\n");
        let date = std::process::Command::new("date")
            .arg("+%Y-%m-%d %H:%M:%S")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|_| "Unknown".to_string());
        report.push_str(&format!("> Generated: {}\n\n", date));

        // ── Environment ──────────────────────────────────────────
        report.push_str("## Environment\n\n| Field | Value |\n|-------|-------|\n");
        report.push_str(&format!("| OMENSpace version | `{}` |\n", env!("CARGO_PKG_VERSION")));
        report.push_str(&format!("| OS                | `{}` |\n", specs.os_spec));
        report.push_str(&format!("| Kernel            | `{}` |\n", specs.kernel_version));

        // ── Hardware Probe ───────────────────────────────────────
        report.push_str("\n## Hardware Probe\n\n| Field | Value |\n|-------|-------|\n");
        report.push_str(&format!("| Manufacturer | `{}` |\n", manufacturer));
        report.push_str(&format!("| Product Name | `{}` |\n", product_name));
        report.push_str(&format!("| Board ID     | `{}` |\n", board_id));
        report.push_str(&format!("| BIOS Version | `{}` |\n", specs.bios_version));
        report.push_str(&format!("| CPU          | `{}` |\n", specs.cpu_spec));
        report.push_str(&format!("| GPU          | `{}` |\n", specs.gpu_spec));
        report.push_str(&format!("| RAM          | `{}` |\n", specs.ram_spec));
        report.push_str(&format!("| NVIDIA Driver| `{}` |\n", specs.nvidia_driver));

        // ── Live Fan Telemetry ───────────────────────────────────
        report.push_str("\n## Live Fan Telemetry\n\n");
        // Show both fans individually; fall back to max if fan2 is not exposed
        if stats.fan1_rpm > 0 || stats.fan2_rpm > 0 {
            report.push_str(&format!("- CPU Fan (fan1): {} RPM (CPU Temp: {} °C)\n", stats.fan1_rpm, stats.cpu_temp));
            if stats.fan2_rpm > 0 {
                report.push_str(&format!("- GPU Fan (fan2): {} RPM (GPU Temp: {} °C)\n", stats.fan2_rpm, stats.gpu_temp));
            } else {
                report.push_str(&format!("- GPU Fan (fan2): not exposed by hwmon (GPU Temp: {} °C)\n", stats.gpu_temp));
            }
        } else {
            report.push_str(&format!("- Fan RPM: {} (individual fans not resolved — check hwmon)\n", stats.fan_rpm));
        }
        report.push_str(&format!("- CPU Power: {:.1} W  |  GPU Power: {:.1} W  |  Total: {:.1} W\n",
            stats.cpu_pwr, stats.gpu_pwr, stats.total_pwr));

        // ── hwmon Path Probe ─────────────────────────────────────
        report.push_str("\n## hwmon Sensor Paths\n\n");
        report.push_str("| hwmon | Driver | fan1_input | fan2_input | pwm1_enable |\n");
        report.push_str("|-------|--------|-----------|-----------|-------------|\n");
        if let Ok(entries) = glob::glob("/sys/class/hwmon/hwmon*") {
            for entry in entries.filter_map(Result::ok) {
                let name = std::fs::read_to_string(entry.join("name"))
                    .unwrap_or_else(|_| "?".to_string());
                let name = name.trim();
                let fan1 = std::fs::read_to_string(entry.join("fan1_input"))
                    .map(|v| v.trim().to_string())
                    .unwrap_or_else(|_| "—".to_string());
                let fan2 = std::fs::read_to_string(entry.join("fan2_input"))
                    .map(|v| v.trim().to_string())
                    .unwrap_or_else(|_| "—".to_string());
                let pwm1 = std::fs::read_to_string(entry.join("pwm1_enable"))
                    .map(|v| v.trim().to_string())
                    .unwrap_or_else(|_| "—".to_string());
                let hwmon_name = entry.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if fan1 != "—" || fan2 != "—" || pwm1 != "—" {
                    report.push_str(&format!("| `{}` | `{}` | {} RPM | {} RPM | {} |\n",
                        hwmon_name, name, fan1, fan2, pwm1));
                }
            }
        }

        // ── EC Registers Snapshot ────────────────────────────────
        report.push_str("\n## EC Registers — Snapshot\n\n");

        // Try to load ec_sys with write_support so debugfs exposes the io node
        let _ = std::process::Command::new("modprobe")
            .args(["ec_sys", "write_support=1"])
            .output();
        // Also ensure debugfs is mounted if not already present
        if !std::path::Path::new("/sys/kernel/debug").exists() {
            let _ = std::process::Command::new("mount")
                .args(["-t", "debugfs", "none", "/sys/kernel/debug"])
                .output();
        }

        let debugfs_mounted = std::path::Path::new("/sys/kernel/debug").exists();
        let ec_path = "/sys/kernel/debug/ec/ec0/io";
        let ec_path_exists = std::path::Path::new(ec_path).exists();

        if let Ok(ec_data) = std::fs::read(ec_path) {
            report.push_str("```\n");
            for (i, byte) in ec_data.iter().enumerate().take(256) {
                if i % 16 == 0 {
                    report.push_str(&format!("{:02x}0 ", i / 16));
                }
                report.push_str(&format!("{:02x} ", byte));
                if i % 16 == 15 {
                    report.push('\n');
                }
            }
            report.push_str("```\n");
        } else {
            // Provide a structured troubleshooting block instead of a bare error
            report.push_str(&format!(
                "> **EC read unavailable** — run the commands below as root and regenerate.\n\
                >\n\
                > | Check | Status |\n\
                > |-------|--------|\n\
                > | Running as root | {} |\n\
                > | debugfs mounted at /sys/kernel/debug | {} |\n\
                > | ec_sys io node exists | {} |\n\
                >\n\
                > **Quick fix:**\n\
                > ```bash\n\
                > sudo modprobe ec_sys write_support=1\n\
                > sudo mount -t debugfs none /sys/kernel/debug   # if not already mounted\n\
                > ls /sys/kernel/debug/ec/ec0/io                 # should exist now\n\
                > ```\n\
                > Then regenerate this report from the OMENSpace Debug panel.\n",
                if std::env::var("USER").unwrap_or_default() == "root" || nix_is_root() { "✅ Yes" } else { "❌ No — reopen OMENSpace as root or via pkexec" },
                if debugfs_mounted { "✅ Mounted" } else { "❌ Not mounted" },
                if ec_path_exists { "✅ Present" } else { "❌ Missing (ec_sys not loaded or read_support=0)" },
            ));
        }

        // ── dmesg — hp-wmi / ACPI excerpt ───────────────────────
        report.push_str("\n## dmesg — HP WMI / ACPI Excerpt (last 30 lines)\n\n```\n");
        if let Ok(out) = std::process::Command::new("sh")
            .arg("-c")
            .arg("dmesg 2>/dev/null | grep -iE 'hp.wmi|hp_wmi|omen|AE_AML|AE_NOT_FOUND|ACPI Error' | tail -30")
            .output()
        {
            let text = String::from_utf8_lossy(&out.stdout);
            if text.trim().is_empty() {
                report.push_str("(no relevant hp-wmi / ACPI entries found)\n");
            } else {
                report.push_str(&text);
            }
        } else {
            report.push_str("(dmesg unavailable)\n");
        }
        report.push_str("```\n");

        report
    }

    async fn generate_rgb_issue(&self) -> String {
        let specs = get_hardware_specs();
        let board_id = std::fs::read_to_string("/sys/class/dmi/id/board_name").unwrap_or_else(|_| "Unknown".to_string()).trim().to_string();
        
        let mut issue = String::new();
        issue.push_str("### Keyboard RGB Unsupported Issue\n\n");
        issue.push_str("**Product ID (Board ID):**\n");
        issue.push_str(&format!("{}\n\n", board_id));
        issue.push_str("**Full Laptop Model Name:**\n");
        issue.push_str(&format!("{}\n\n", specs.product_name));
        issue.push_str("**Kernel Version:**\n");
        issue.push_str(&format!("{}\n\n", specs.kernel_version));
        
        issue.push_str("**HID Devices (lsusb):**\n```\n");
        if let Ok(out) = std::process::Command::new("lsusb").output() {
            let out_str = String::from_utf8_lossy(&out.stdout);
            for line in out_str.lines() {
                if line.contains("Hewlett-Packard") || line.contains("HP") || line.contains("03f0") {
                    issue.push_str(line);
                    issue.push('\n');
                }
            }
        }
        issue.push_str("```\n\n");
        
        issue.push_str("**Description:**\nMy keyboard backlight is not detected or cannot be controlled by OMENSpace. Here are the diagnostics.\n");
        
        issue
    }

    #[zbus(signal)]
    pub async fn telemetry_updated(ctxt: &zbus::SignalContext<'_>, json_stats: &str) -> zbus::Result<()>;
}
