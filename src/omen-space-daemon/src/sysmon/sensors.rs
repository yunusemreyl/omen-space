use std::fs;
use std::process::Command;
use std::sync::OnceLock;
use std::sync::Mutex;
use std::path::PathBuf;
pub use omen_types::HardwareSpecs;
// ── Sensor Path Cache for Zero-Glob Overhead ────────────────
#[derive(Clone)]
pub struct SensorPaths {
    pub cpu_temp_path: Option<PathBuf>,
    pub cpu_pwr_path: Option<PathBuf>,
    pub fan1_path: Option<PathBuf>,
    pub fan2_path: Option<PathBuf>,
    pub throttle_paths: Vec<PathBuf>,
    pub gpu_temp_path: Option<PathBuf>,
    pub gpu_pwr_path: Option<PathBuf>,
    pub battery_pwr_path: Option<PathBuf>,
    pub rapl_energy_path: Option<PathBuf>,
}

pub static SENSOR_PATHS: Mutex<Option<(SensorPaths, std::time::Instant)>> = Mutex::new(None);
pub static SPECS_CACHE: OnceLock<HardwareSpecs> = OnceLock::new();

// State for Instantaneous CPU load delta calculation
pub struct CpuJiffies {
    pub total: u64,
    pub idle: u64,
}
pub static PREV_JIFFIES: Mutex<Option<CpuJiffies>> = Mutex::new(None);

pub struct RaplState {
    pub energy_uj: u64,
    pub time: std::time::Instant,
}
pub static PREV_RAPL: Mutex<Option<RaplState>> = Mutex::new(None);
pub static GPU_IDLE_COOLDOWN: Mutex<Option<std::time::Instant>> = Mutex::new(None);

// ── CPU Temperature EWMA Smoother ───────────────────────────────────────────
// Rising temps are accepted immediately (safety-first: fans must respond).
// Falling temps decay at 40 % per tick to prevent fan hunting caused by
// short-lived sensor noise spikes (matches ohman's AutoTick algorithm).
static SMOOTHED_CPU_TEMP: Mutex<Option<f64>> = Mutex::new(None);

pub fn smooth_cpu_temp(raw_celsius: i32) -> i32 {
    let raw = raw_celsius as f64;
    let mut guard = SMOOTHED_CPU_TEMP.lock().unwrap_or_else(|e| e.into_inner());
    let smoothed = match *guard {
        None => raw,
        Some(prev) => {
            if raw >= prev {
                // Rising: take the new value immediately
                raw
            } else {
                // Falling: decay 40 % toward the new value per tick
                prev + 0.40 * (raw - prev)
            }
        }
    };
    *guard = Some(smoothed);
    smoothed.round() as i32
}
pub fn init_sensor_paths() -> SensorPaths {
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
                    let board_id = std::fs::read_to_string("/sys/class/dmi/id/board_name").unwrap_or_default();
                    if !crate::capabilities::LinuxCapabilityClassifier::is_wmaa_abort_prone_board(&board_id) {
                        let f1 = entry.join("fan1_input");
                        if f1.exists() { fan1_path = Some(f1); }
                        let f2 = entry.join("fan2_input");
                        if f2.exists() { fan2_path = Some(f2); }
                    }
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
                        && (line.contains("NVIDIA") || (gpu_str == "Unknown GPU" && (line.contains("AMD") || line.contains("Intel")))) {
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
                            if part.contains('.') && part.chars().next().is_some_and(|c| c.is_ascii_digit()) {
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
