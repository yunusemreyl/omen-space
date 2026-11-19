#![allow(dead_code)]
#![allow(unused_imports)]
/// MUX (GPU Switch) service - matches Python mux_service.py feature-for-feature.
///
/// D-Bus interface: com.yyl.hpmanager.mux (backward compat) +
///                  org.hp.omen.Mux (new canonical name)
///
/// Methods exposed:
///   SetGpuMode(mode: s)  -> result: s   ("OK_REBOOT_REQUIRED" or "FAIL")
///   GetGpuInfo()         -> j: s        (JSON with mode, backend, displays)
///   SetMuxBackend(backend: s) -> result: s
///   Ping()               -> resp: s
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use zbus::interface;
use log::{info, warn};
use glob::glob;

const HP_WMI_GRAPHICS_MODE_PATH: &str = "/sys/devices/platform/hp-wmi/gpu_mux_mode";
const HP_WMI_LEGACY_GRAPHICS_MODE_PATH: &str = "/sys/devices/platform/hp-wmi/graphics_mode";
const CONFIG_PATH: &str = "/etc/omen-space/mux.json";

#[derive(Serialize, Deserialize, Debug, Clone)]
struct MuxConfig {
    mux_backend: String,
}

impl Default for MuxConfig {
    fn default() -> Self {
        Self { mux_backend: "auto".to_string() }
    }
}

impl MuxConfig {
    fn load() -> Self {
        if let Ok(data) = std::fs::read_to_string(CONFIG_PATH) {
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            Self::default()
        }
    }
    fn save(&self) {
        if let Some(dir) = Path::new(CONFIG_PATH).parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(CONFIG_PATH, json);
        }
    }
}

struct MuxInner {
    config: MuxConfig,
    cached_mode: Option<String>,
    displays_cache: Option<Vec<serde_json::Value>>,
}

#[derive(Clone)]
pub struct MuxService {
    inner: Arc<Mutex<MuxInner>>,
}

impl MuxService {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let config = MuxConfig::load();
        let inner = Arc::new(Mutex::new(MuxInner {
            config,
            cached_mode: None,
            displays_cache: None,
        }));
        Ok(Self { inner })
    }

    fn wmi_available() -> bool {
        Path::new(HP_WMI_GRAPHICS_MODE_PATH).exists()
            || Path::new(HP_WMI_LEGACY_GRAPHICS_MODE_PATH).exists()
    }

    /// Read configured firmware MUX mode from `/sys/devices/platform/hp-wmi/gpu_mux_mode`
    /// (falling back to `/sys/devices/platform/hp-wmi/graphics_mode`, #313, #369).
    /// Driver mapping: 0 = hybrid, 1 = discrete, 2 = advanced (Advanced Optimus), 3 = integrated.
    async fn read_firmware_mux_mode() -> Option<String> {
        for path in [HP_WMI_GRAPHICS_MODE_PATH, HP_WMI_LEGACY_GRAPHICS_MODE_PATH] {
            if let Ok(raw) = tokio::fs::read_to_string(path).await {
                match raw.trim() {
                    "0" | "hybrid" => return Some("hybrid".to_string()),
                    "1" | "discrete" => return Some("discrete".to_string()),
                    "2" | "advanced" | "optimus" => return Some("advanced".to_string()),
                    "3" | "integrated" => return Some("integrated".to_string()),
                    _ => {}
                }
            }
        }
        None
    }

    /// Infer current display routing from DRM eDP connector and PCI bus topology.
    async fn detect_routing_mode() -> String {
        // 1. DRM eDP check
        if let Ok(entries) = glob("/sys/class/drm/card[0-9]*") {
            for entry in entries.filter_map(Result::ok) {
                let name = entry.file_name()
                    .and_then(|n| n.to_str()).unwrap_or("").to_uppercase();
                if !name.contains("EDP") { continue; }
                let status_path = entry.join("status");
                if let Ok(status) = tokio::fs::read_to_string(&status_path).await {
                    if status.trim() != "connected" { continue; }
                    let vendor_path = entry.join("device/device/vendor");
                    if let Ok(vendor) = tokio::fs::read_to_string(&vendor_path).await {
                        if vendor.trim().to_lowercase() == "0x10de" {
                            return "discrete".to_string();
                        }
                    }
                }
            }
        }

        // 2. Passive sysfs check (replaces lspci -D to prevent waking dGPU from D3cold)
        let mut has_nvidia = false;
        let mut has_igpu = false;

        if let Ok(entries) = std::fs::read_dir("/sys/bus/pci/devices") {
            for entry in entries.flatten() {
                let path = entry.path();
                let class = std::fs::read_to_string(path.join("class")).unwrap_or_default();
                let class = class.trim();

                // Check for display controllers: 0x030000 (VGA) or 0x030200 (3D Controller)
                if class.starts_with("0x03") {
                    let vendor = std::fs::read_to_string(path.join("vendor")).unwrap_or_default();
                    let vendor = vendor.trim().to_lowercase();

                    if vendor == "0x10de" {
                        has_nvidia = true;
                    } else if vendor == "0x1002" || vendor == "0x8086" {
                        // AMD (0x1002) or Intel (0x8086)
                        has_igpu = true;
                    }
                }
            }
        }

        if has_nvidia && has_igpu {
            return "hybrid".to_string();
        } else if has_nvidia {
            return "discrete".to_string();
        } else if has_igpu {
            return "integrated".to_string();
        }

        "unknown".to_string()
    }

    /// Detect configured GPU mode (#313): prefers `hp-wmi/gpu_mux_mode` firmware readback so
    /// Advanced Optimus (`2`) is reported accurately instead of always collapsing to `hybrid`.
    async fn detect_mode() -> String {
        if let Some(fw_mode) = Self::read_firmware_mux_mode().await {
            return fw_mode;
        }
        Self::detect_routing_mode().await
    }

    /// Enumerate connected displays with GPU vendor — mirrors Python _get_displays().
    async fn get_displays() -> Vec<serde_json::Value> {
        let vendors_map = [("0x10de", "NVIDIA"), ("0x8086", "Intel"), ("0x1002", "AMD")];
        let mut result = Vec::new();

        if let Ok(entries) = glob("/sys/class/drm/card[0-9]*-*") {
            for entry in entries.filter_map(Result::ok) {
                let status_path = entry.join("status");
                if let Ok(status) = tokio::fs::read_to_string(&status_path).await {
                    if status.trim() != "connected" { continue; }
                    let vendor_path = entry.join("device/device/vendor");
                    let vendor_str = tokio::fs::read_to_string(&vendor_path).await
                        .map(|s| s.trim().to_lowercase())
                        .unwrap_or_default();
                    let gpu_name = vendors_map.iter()
                        .find(|(id, _)| vendor_str == *id)
                        .map(|(_, name)| *name)
                        .unwrap_or("Unknown GPU");
                    let disp_name = entry.file_name()
                        .and_then(|n| n.to_str())
                        .and_then(|s| s.split_once('-').map(|x| x.1))
                        .unwrap_or("unknown")
                        .to_string();
                    result.push(serde_json::json!({ "display": disp_name, "gpu": gpu_name }));
                }
            }
        }
        result
    }
}

#[interface(name = "org.hp.omen.Mux")]
impl MuxService {
    /// SetGpuMode — mirrors Python MUXService.SetGpuMode().
    async fn set_gpu_mode(&self, mode: String) -> String {
        let valid = ["hybrid", "discrete", "advanced"];
        if !valid.contains(&mode.as_str()) {
            return "FAIL".to_string();
        }

        if !Self::wmi_available() {
            warn!("SetGpuMode: WMI MUX interface not found at {}", HP_WMI_GRAPHICS_MODE_PATH);
            return "Error: WMI MUX interface not found".to_string();
        }

        let val = if mode == "discrete" { "1" } else if mode == "advanced" { "2" } else { "0" };
        let mut last_err = String::new();
        for path in [HP_WMI_GRAPHICS_MODE_PATH, HP_WMI_LEGACY_GRAPHICS_MODE_PATH] {
            if !Path::new(path).exists() {
                continue;
            }
            match tokio::fs::write(path, val).await {
                Ok(()) => {
                    let readback = tokio::fs::read_to_string(path)
                        .await
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|_| "unreadable".to_string());
                    {
                        let mut g = self.inner.lock().await;
                        g.cached_mode = Some(mode.clone());
                    }
                    info!("SetGpuMode: '{}' (val={}) written to {} (readback={})", mode, val, path, readback);
                    return "OK_REBOOT_REQUIRED".to_string();
                }
                Err(e) => {
                    warn!("SetGpuMode: Failed to write '{}' to {}: {}", val, path, e);
                    last_err = format!("{}: {}", path, e);
                }
            }
        }
        format!("Error: Failed to write to WMI sysfs ({})", last_err)
    }

    /// GetGpuInfo — mirrors Python MUXService.GetGpuInfo().
    async fn get_gpu_info(&self) -> String {
        let g = self.inner.lock().await;
        let available = Self::wmi_available();
        let backend = if available { "wmi-native" } else { "none" };
        let available_backends: Vec<&str> = if available { vec!["wmi-native"] } else { vec![] };
        let forced_backend = g.config.mux_backend.clone();
        let cached = g.cached_mode.clone();
        drop(g);

        let routing_mode = Self::detect_routing_mode().await;
        let mode = Self::detect_mode().await;
        let displays = Self::get_displays().await;

        let json = serde_json::json!({
            "available": available,
            "backend": backend,
            "available_backends": available_backends,
            "forced_backend": forced_backend,
            "mode": mode,
            "pending_mode": cached,
            "routing_mode": routing_mode,
            "displays": displays,
        });
        json.to_string()
    }

    /// SetMuxBackend — mirrors Python MUXService.SetMuxBackend().
    async fn set_mux_backend(&self, backend: String) -> String {
        if backend != "auto" && backend != "wmi-native" {
            return "FAIL".to_string();
        }
        let mut g = self.inner.lock().await;
        g.config.mux_backend = backend.clone();
        g.config.save();
        info!("SetMuxBackend: '{}'", backend);
        "OK".to_string()
    }

    async fn ping(&self) -> String {
        "OK".to_string()
    }
}
