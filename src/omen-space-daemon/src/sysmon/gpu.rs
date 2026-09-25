use std::fs;
use std::sync::OnceLock;
use std::sync::Mutex;
use crate::sysmon::sensors::*;
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
pub fn check_nvidia_state() -> Option<bool> {
    if let Some(path) = get_nvidia_dgpu_path() {
        let status_path = path.join("power/runtime_status");
        if let Ok(status) = fs::read_to_string(status_path) {
            return Some(status.trim() == "active");
        }
    }
    None
}

#[derive(Clone)]
pub struct GpuMetrics {
    pub has_clients: bool,
    pub temp: Option<i32>,
    pub power: Option<f64>,
}

static NVML_INSTANCE: OnceLock<Mutex<Option<nvml_wrapper::Nvml>>> = OnceLock::new();
static CACHED_GPU_METRICS: Mutex<Option<(GpuMetrics, std::time::Instant)>> = Mutex::new(None);
static FETCHING_GPU: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn fetch_gpu_metrics_with_timeout() -> Option<GpuMetrics> {
    // 1. Check if cached metrics are still fresh (< 1500ms) to avoid querying NVML every second
    {
        if let Ok(guard) = CACHED_GPU_METRICS.lock() {
            if let Some((ref cached, ref timestamp)) = *guard {
                if timestamp.elapsed() < std::time::Duration::from_millis(1500) {
                    return Some(cached.clone());
                }
            }
        }
    }

    // 2. Prevent thread accumulation: if a query is already in progress, return cached or None
    if FETCHING_GPU.compare_exchange(
        false,
        true,
        std::sync::atomic::Ordering::SeqCst,
        std::sync::atomic::Ordering::SeqCst,
    ).is_err() {
        if let Ok(guard) = CACHED_GPU_METRICS.lock() {
            return guard.as_ref().map(|(c, _)| c.clone());
        }
        return None;
    }

    let (tx, rx) = std::sync::mpsc::channel();
    
    std::thread::spawn(move || {
        // Ensure flag is reset when thread finishes
        struct FetchGuard;
        impl Drop for FetchGuard {
            fn drop(&mut self) {
                FETCHING_GPU.store(false, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let _guard = FetchGuard;

        // CRITICAL BATTERY SAVER: Do not wake the GPU if it is runtime suspended.
        if check_nvidia_state() == Some(false) {
            let res = GpuMetrics { has_clients: false, temp: None, power: None };
            let _ = tx.send(res.clone());
            if let Ok(mut c_guard) = CACHED_GPU_METRICS.lock() {
                *c_guard = Some((res, std::time::Instant::now()));
            }
            return;
        }

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
                    let res = GpuMetrics { has_clients, temp, power };
                    let _ = tx.send(res.clone());
                    if let Ok(mut c_guard) = CACHED_GPU_METRICS.lock() {
                        *c_guard = Some((res, std::time::Instant::now()));
                    }
                }
            }
        }
    });

    let res = rx.recv_timeout(std::time::Duration::from_millis(500)).ok();
    if res.is_none() {
        if let Ok(guard) = CACHED_GPU_METRICS.lock() {
            return guard.as_ref().map(|(c, _)| c.clone());
        }
    }
    res
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

    let mut gpu_temp = 0.0;
    let has_active_clients;

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
