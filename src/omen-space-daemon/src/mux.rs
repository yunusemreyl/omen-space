#![allow(dead_code)]
#![allow(unused_imports)]
/// MUX (GPU Switch) service - exposes hardware-validated GPU MUX switching & topology info.
///
/// D-Bus interface: org.hp.omen.Mux
///
/// Methods exposed:
///   SetGpuMode(mode: s)  -> result: s   ("OK_REBOOT_REQUIRED", "Error: UNSUPPORTED_MODE ...", or "FAIL")
///   GetGpuInfo()         -> j: s        (JSON with mode, supported_modes, boot_mode, reboot_pending,
///                                        routing_mode, prime_offload_available, dgpu_power_state, displays)
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
const HP_WMI_SUPPORTED_MODES_PATH: &str = "/sys/devices/platform/hp-wmi/gpu_mux_supported_modes";
const CONFIG_PATH: &str = "/etc/omen-space/mux.json";

const CANONICAL_MUX_MODES: [&str; 4] = ["hybrid", "discrete", "advanced", "integrated"];

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

#[derive(Debug, Clone, Default)]
struct GpuTopology {
    has_nvidia: bool,
    has_igpu: bool,
    dgpu_power_state: Option<String>,
}

fn normalize_mux_mode(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "0" | "hybrid" | "mshybrid" => Some("hybrid"),
        "1" | "discrete" | "dgpu" => Some("discrete"),
        "2" | "advanced" | "optimus" | "advanced_optimus" => Some("advanced"),
        "3" | "integrated" | "uma" | "igpu" => Some("integrated"),
        _ => None,
    }
}

/// Parse space/whitespace-delimited MUX mode tokens from sysfs and return
/// deduplicated mode names in canonical order (`hybrid`, `discrete`, `advanced`, `integrated`).
pub fn parse_supported_modes_str(raw: &str) -> Vec<String> {
    let mut found = [false; 4];
    for token in raw.split_whitespace() {
        if let Some(norm) = normalize_mux_mode(token) {
            if let Some(idx) = CANONICAL_MUX_MODES.iter().position(|m| *m == norm) {
                found[idx] = true;
            }
        }
    }
    CANONICAL_MUX_MODES
        .iter()
        .enumerate()
        .filter(|&(i, _)| found[i])
        .map(|(_, m)| (*m).to_string())
        .collect()
}

/// Resolve supported GPU MUX modes using sysfs (`gpu_mux_supported_modes`) first,
/// falling back to `boards.json` `supports_advanced_optimus` if the sysfs node is missing
/// (older kernel driver). Never invents `advanced` on unsupported boards.
pub fn resolve_supported_modes(
    wmi_available: bool,
    sysfs_raw: Option<&str>,
    caps_supports_ao: bool,
    current_fw_mode: Option<&str>,
) -> Vec<String> {
    if !wmi_available {
        return Vec::new();
    }

    let mut modes = match sysfs_raw.map(parse_supported_modes_str) {
        Some(parsed) if !parsed.is_empty() => parsed,
        _ => {
            let mut fallback = vec!["hybrid".to_string(), "discrete".to_string()];
            if caps_supports_ao {
                fallback.push("advanced".to_string());
            }
            fallback
        }
    };

    if let Some(fw) = current_fw_mode.and_then(normalize_mux_mode) {
        if !modes.iter().any(|m| m == fw) {
            modes.push(fw.to_string());
            modes.sort_by_key(|m| {
                CANONICAL_MUX_MODES
                    .iter()
                    .position(|c| *c == m.as_str())
                    .unwrap_or(usize::MAX)
            });
        }
    }

    modes
}

/// Determine whether a reboot is currently needed for a GPU MUX switch.
pub fn compute_reboot_pending(
    boot_mode: Option<&str>,
    current_fw_mode: &str,
    pending_mode: Option<&str>,
    routing_mode: &str,
) -> bool {
    if let Some(p) = pending_mode {
        return boot_mode.map_or(p != current_fw_mode, |b| p != b);
    }
    if let Some(b) = boot_mode {
        if b != "unknown" && b != current_fw_mode {
            return true;
        }
    }
    matches!(
        (current_fw_mode, routing_mode),
        ("discrete", "hybrid" | "integrated") | ("hybrid", "discrete")
    )
}

async fn read_connector_vendor(entry: &Path) -> Option<String> {
    for rel in ["device/device/vendor", "device/vendor"] {
        if let Ok(vendor) = tokio::fs::read_to_string(entry.join(rel)).await {
            let v = vendor.trim().to_lowercase();
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

struct MuxInner {
    config: MuxConfig,
    boot_mode: Option<String>,
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
        let initial = Self::detect_mode().await;
        let boot_mode = (initial != "unknown").then_some(initial);
        let inner = Arc::new(Mutex::new(MuxInner {
            config,
            boot_mode,
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
                if let Some(norm) = normalize_mux_mode(&raw) {
                    return Some(norm.to_string());
                }
            }
        }
        None
    }

    /// Passive PCI scan for display adapters + dGPU runtime power state
    /// (never wakes dGPU from D3cold).
    fn detect_gpu_topology() -> GpuTopology {
        let mut topo = GpuTopology::default();

        if let Ok(entries) = std::fs::read_dir("/sys/bus/pci/devices") {
            for entry in entries.flatten() {
                let path = entry.path();
                let class = std::fs::read_to_string(path.join("class")).unwrap_or_default();
                let class = class.trim();

                // Display controllers: 0x030000 (VGA) or 0x030200 (3D Controller)
                if class.starts_with("0x03") {
                    let vendor = std::fs::read_to_string(path.join("vendor"))
                        .unwrap_or_default()
                        .trim()
                        .to_lowercase();

                    if vendor == "0x10de" {
                        topo.has_nvidia = true;
                        if topo.dgpu_power_state.is_none() {
                            if let Ok(rt) = std::fs::read_to_string(path.join("power/runtime_status")) {
                                let trimmed = rt.trim().to_lowercase();
                                if !trimmed.is_empty() {
                                    topo.dgpu_power_state = Some(trimmed);
                                }
                            }
                        }
                    } else if vendor == "0x1002" || vendor == "0x8086" {
                        topo.has_igpu = true;
                    }
                }
            }
        }

        topo
    }

    /// Infer current display routing from DRM eDP connector and passive PCI topology.
    async fn detect_routing_mode(topo: &GpuTopology) -> String {
        // 1. DRM eDP check
        if let Ok(entries) = glob("/sys/class/drm/card[0-9]*") {
            for entry in entries.filter_map(Result::ok) {
                let name = entry
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_uppercase();
                if !name.contains("EDP") {
                    continue;
                }
                let status_path = entry.join("status");
                if let Ok(status) = tokio::fs::read_to_string(&status_path).await {
                    if status.trim() != "connected" {
                        continue;
                    }
                    if let Some(vendor) = read_connector_vendor(&entry).await {
                        if vendor == "0x10de" {
                            return "discrete".to_string();
                        }
                    }
                }
            }
        }

        if topo.has_nvidia && topo.has_igpu {
            return "hybrid".to_string();
        } else if topo.has_nvidia {
            return "discrete".to_string();
        } else if topo.has_igpu {
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
        let topo = Self::detect_gpu_topology();
        Self::detect_routing_mode(&topo).await
    }

    /// Detect supported BIOS MUX modes using sysfs (`gpu_mux_supported_modes`) first,
    /// falling back to `boards.json` capabilities.
    async fn detect_supported_modes(current_fw_mode: Option<&str>) -> Vec<String> {
        let available = Self::wmi_available();
        if !available {
            return Vec::new();
        }

        let sysfs_raw = tokio::fs::read_to_string(HP_WMI_SUPPORTED_MODES_PATH).await.ok();
        let caps_supports_ao = if sysfs_raw.as_ref().is_some_and(|s| !parse_supported_modes_str(s).is_empty()) {
            false
        } else {
            let board_id = tokio::fs::read_to_string("/sys/class/dmi/id/board_name")
                .await
                .unwrap_or_default()
                .trim()
                .to_string();
            let product_name = tokio::fs::read_to_string("/sys/class/dmi/id/product_name")
                .await
                .unwrap_or_default()
                .trim()
                .to_string();
            let cpu_model = tokio::fs::read_to_string("/proc/cpuinfo")
                .await
                .ok()
                .and_then(|c| {
                    c.lines()
                        .find(|l| l.starts_with("model name"))
                        .and_then(|l| l.split_once(':'))
                        .map(|(_, v)| v.trim().to_string())
                })
                .unwrap_or_default();
            crate::capabilities::detect(&board_id, &product_name, &cpu_model).supports_advanced_optimus
        };

        resolve_supported_modes(available, sysfs_raw.as_deref(), caps_supports_ao, current_fw_mode)
    }

    /// Enumerate connected displays with GPU vendor.
    async fn get_displays() -> Vec<serde_json::Value> {
        let vendors_map = [("0x10de", "NVIDIA"), ("0x8086", "Intel"), ("0x1002", "AMD")];
        let mut result = Vec::new();

        if let Ok(entries) = glob("/sys/class/drm/card[0-9]*-*") {
            for entry in entries.filter_map(Result::ok) {
                let status_path = entry.join("status");
                if let Ok(status) = tokio::fs::read_to_string(&status_path).await {
                    if status.trim() != "connected" {
                        continue;
                    }
                    let vendor_str = read_connector_vendor(&entry).await.unwrap_or_default();
                    let gpu_name = vendors_map
                        .iter()
                        .find(|(id, _)| vendor_str == *id)
                        .map(|(_, name)| *name)
                        .unwrap_or("Unknown GPU");
                    let disp_name = entry
                        .file_name()
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
    /// SetGpuMode — validates requested mode against BIOS/hardware capabilities before writing sysfs.
    async fn set_gpu_mode(&self, mode: String) -> String {
        let mode = mode.trim().to_lowercase();
        if !CANONICAL_MUX_MODES.contains(&mode.as_str()) {
            return "FAIL".to_string();
        }

        if !Self::wmi_available() {
            warn!("SetGpuMode: WMI MUX interface not found at {}", HP_WMI_GRAPHICS_MODE_PATH);
            return "Error: WMI MUX interface not found".to_string();
        }

        let current_fw = Self::read_firmware_mux_mode().await;
        let supported = Self::detect_supported_modes(current_fw.as_deref()).await;
        if !supported.iter().any(|m| m == &mode) {
            warn!(
                "SetGpuMode: '{}' rejected (supported on this device: {:?})",
                mode, supported
            );
            return format!(
                "Error: UNSUPPORTED_MODE ({} is not supported on this hardware; supported: {})",
                mode,
                supported.join(", ")
            );
        }

        let val = match mode.as_str() {
            "discrete" => "1",
            "advanced" => "2",
            "integrated" => "3",
            _ => "0",
        };
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
                        if g.boot_mode.as_deref() == Some(mode.as_str()) {
                            g.cached_mode = None;
                        } else {
                            g.cached_mode = Some(mode.clone());
                        }
                    }
                    info!(
                        "SetGpuMode: '{}' (val={}) written to {} (readback={})",
                        mode, val, path, readback
                    );
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

    /// GetGpuInfo — returns current mode, supported modes, boot state, PRIME availability, and displays.
    async fn get_gpu_info(&self) -> String {
        let g = self.inner.lock().await;
        let available = Self::wmi_available();
        let backend = if available { "wmi-native" } else { "none" };
        let available_backends: Vec<&str> = if available { vec!["wmi-native"] } else { vec![] };
        let forced_backend = g.config.mux_backend.clone();
        let cached = g.cached_mode.clone();
        let boot_mode = g.boot_mode.clone();
        drop(g);

        let topo = Self::detect_gpu_topology();
        let routing_mode = Self::detect_routing_mode(&topo).await;
        let fw_mode = Self::read_firmware_mux_mode().await;
        let mode = fw_mode.clone().unwrap_or_else(|| routing_mode.clone());
        let supported_modes = Self::detect_supported_modes(fw_mode.as_deref()).await;
        let supports_advanced_optimus = supported_modes.iter().any(|m| m == "advanced");
        let supports_integrated = supported_modes.iter().any(|m| m == "integrated");
        let reboot_pending = compute_reboot_pending(
            boot_mode.as_deref(),
            &mode,
            cached.as_deref(),
            &routing_mode,
        );
        let displays = Self::get_displays().await;
        let prime_offload_available = topo.has_nvidia && topo.has_igpu;
        let has_prime_run = Path::new("/usr/bin/prime-run").exists();

        let json = serde_json::json!({
            "available": available,
            "backend": backend,
            "available_backends": available_backends,
            "forced_backend": forced_backend,
            "mode": mode,
            "boot_mode": boot_mode,
            "pending_mode": cached,
            "reboot_pending": reboot_pending,
            "routing_mode": routing_mode,
            "supported_modes": supported_modes,
            "supports_advanced_optimus": supports_advanced_optimus,
            "supports_integrated": supports_integrated,
            "prime_offload_available": prime_offload_available,
            "has_prime_run": has_prime_run,
            "dgpu_power_state": topo.dgpu_power_state,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_supported_modes_normalizes_and_deduplicates() {
        assert_eq!(
            parse_supported_modes_str("discrete hybrid discrete\n"),
            vec!["hybrid".to_string(), "discrete".to_string()]
        );
        assert_eq!(
            parse_supported_modes_str("hybrid discrete advanced integrated"),
            vec![
                "hybrid".to_string(),
                "discrete".to_string(),
                "advanced".to_string(),
                "integrated".to_string(),
            ]
        );
        assert_eq!(
            parse_supported_modes_str("0 1 2 3"),
            vec![
                "hybrid".to_string(),
                "discrete".to_string(),
                "advanced".to_string(),
                "integrated".to_string(),
            ]
        );
        assert!(parse_supported_modes_str("   \n").is_empty());
    }

    #[test]
    fn resolve_supported_modes_gates_advanced_optimus_strictly() {
        // No WMI MUX at all -> empty
        assert!(resolve_supported_modes(false, Some("hybrid discrete advanced"), true, None).is_empty());

        // Sysfs present without advanced (older Victus / non-AO OMEN): even if boards.json claimed AO, trust BIOS
        assert_eq!(
            resolve_supported_modes(true, Some("hybrid discrete\n"), true, Some("hybrid")),
            vec!["hybrid".to_string(), "discrete".to_string()]
        );

        // Sysfs present with advanced -> includes advanced
        assert_eq!(
            resolve_supported_modes(true, Some("hybrid discrete advanced\n"), false, Some("hybrid")),
            vec!["hybrid".to_string(), "discrete".to_string(), "advanced".to_string()]
        );

        // Sysfs missing (legacy hp-wmi) -> fallback to boards.json caps
        assert_eq!(
            resolve_supported_modes(true, None, false, Some("hybrid")),
            vec!["hybrid".to_string(), "discrete".to_string()]
        );
        assert_eq!(
            resolve_supported_modes(true, None, true, Some("hybrid")),
            vec!["hybrid".to_string(), "discrete".to_string(), "advanced".to_string()]
        );

        // If BIOS currently reports advanced, preserve it even if fallback has false
        assert_eq!(
            resolve_supported_modes(true, None, false, Some("advanced")),
            vec!["hybrid".to_string(), "discrete".to_string(), "advanced".to_string()]
        );
    }

    #[test]
    fn compute_reboot_pending_tracks_boot_vs_target_and_routing() {
        // Same as boot mode -> false
        assert!(!compute_reboot_pending(Some("hybrid"), "hybrid", None, "hybrid"));
        // Switched to discrete while booted in hybrid -> true
        assert!(compute_reboot_pending(Some("hybrid"), "discrete", Some("discrete"), "hybrid"));
        // Switched back to boot mode -> false
        assert!(!compute_reboot_pending(Some("hybrid"), "hybrid", Some("hybrid"), "hybrid"));
        // After GUI reopen without cached pending_mode, boot_mode differs from fw mode -> true
        assert!(compute_reboot_pending(Some("hybrid"), "discrete", None, "hybrid"));
        // Even if daemon restarted and lost boot_mode, firmware=discrete vs routing=hybrid -> true
        assert!(compute_reboot_pending(None, "discrete", None, "hybrid"));
        assert!(compute_reboot_pending(None, "hybrid", None, "discrete"));
    }
}
