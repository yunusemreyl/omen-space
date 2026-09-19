/// Platform service - matches Python platform_service.py feature-for-feature.
///
/// D-Bus interface: com.yyl.hpmanager.platform (backward compat) +
///                  org.hp.omen.Platform (new canonical name)
///
/// Methods exposed:
///   GetSystemInfo()        -> j: s
///   GetState()             -> j: s
///   SetKeyboardFixes(prtsc: b, f1: b) -> result: s
///   CleanMemory()          -> result: s
///   GenerateHardwareDump() -> dump: s
///   GetHardwareDumpJson()  -> dump: s
///   Ping()                 -> resp: s
///
/// Signals:
///   MacroKeyPressed(key_name: s)
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use zbus::interface;
use log::{info, warn};
use glob::glob;

const CONFIG_PATH: &str = "/etc/omen-space/platform.json";
const HWDB_PATH: &str = "/etc/udev/hwdb.d/90-hp-keyboard-fixes.hwdb";

// ── Config ─────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
struct PlatformConfig {
    prtsc_fix: bool,
    f1_fix: bool,
    battery_charge_limit: u32,
}

impl PlatformConfig {
    fn load() -> Self {
        if let Ok(data) = std::fs::read_to_string(CONFIG_PATH) {
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            Self {
                battery_charge_limit: 100,
                ..Default::default()
            }
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

// ── Static system info ─────────────────────────────────────────────────────────

fn read_dmi(name: &str) -> String {
    for prefix in ["/sys/class/dmi/id/", "/sys/devices/virtual/dmi/id/"] {
        let path = format!("{}{}", prefix, name);
        if let Ok(v) = std::fs::read_to_string(&path) {
            return v.trim().to_string();
        }
    }
    "Unknown".to_string()
}

fn get_cpu_model() -> String {
    if let Ok(data) = std::fs::read_to_string("/proc/cpuinfo") {
        for line in data.lines() {
            if line.starts_with("model name") {
                if let Some(val) = line.split_once(':').map(|x| x.1) {
                    return val.trim().to_string();
                }
            }
        }
    }
    "Unknown".to_string()
}

fn is_nixos() -> bool {
    Path::new("/etc/NIXOS").exists()
        || Path::new("/run/current-system/sw/bin/nixos-version").exists()
}

// ── Temperature detection ──────────────────────────────────────────────────────

fn find_best_cpu_temp_path() -> Option<String> {
    const RANK_DRV: &[(&str, i32)] = &[
        ("zenpower", 100), ("coretemp", 90), ("k10temp", 90),
        ("cpu_thermal", 80), ("hp_wmi", 60), ("acpitz", 30),
    ];
    const RANK_LBL: &[(&str, i32)] = &[
        ("tdie", 100), ("package id 0", 95), ("tctl", 90),
        ("core", 80), ("composite", 50),
    ];

    let mut best_score = i32::MIN;
    let mut best_path: Option<String> = None;

    if let Ok(entries) = std::fs::read_dir("/sys/class/hwmon") {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let name = std::fs::read_to_string(path.join("name"))
                .map(|s| s.trim().to_lowercase())
                .unwrap_or_default();
            let d_score = RANK_DRV.iter().find(|(n, _)| name.contains(n))
                .map(|(_, s)| *s).unwrap_or(10);

            if let Ok(inputs) = std::fs::read_dir(&path) {
                for inp in inputs.filter_map(Result::ok) {
                    let fname = inp.file_name().to_string_lossy().to_string();
                    if !fname.starts_with("temp") || !fname.ends_with("_input") { continue; }
                    // Skip if zero or negative
                    if let Ok(val) = std::fs::read_to_string(inp.path()) {
                        if val.trim().parse::<i32>().unwrap_or(0) <= 0 { continue; }
                    } else { continue; }
                    let prefix = fname.split('_').next().unwrap_or("");
                    let label_path = path.join(format!("{}_label", prefix));
                    let label = std::fs::read_to_string(&label_path)
                        .map(|s| s.trim().to_lowercase())
                        .unwrap_or_default();
                    let l_score = RANK_LBL.iter()
                        .filter(|(k, _)| label.contains(k))
                        .map(|(_, v)| *v).max().unwrap_or(0);
                    let score = d_score + l_score;
                    if score > best_score {
                        best_score = score;
                        best_path = Some(inp.path().to_string_lossy().to_string());
                    }
                }
            }
        }
    }
    best_path
}

fn find_gpu_temp_path() -> Option<String> {
    if let Ok(entries) = std::fs::read_dir("/sys/class/hwmon") {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if let Ok(name) = std::fs::read_to_string(path.join("name")) {
                let name = name.trim().to_lowercase();
                if ["amdgpu", "i915", "nouveau"].contains(&name.as_str()) {
                    let tp = path.join("temp1_input");
                    if tp.exists() {
                        return Some(tp.to_string_lossy().to_string());
                    }
                }
            }
        }
    }
    None
}

fn read_temp(path: &str) -> f64 {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<i32>().ok())
        .map(|v| v as f64 / 1000.0)
        .filter(|&t| t > 0.0 && t < 150.0)
        .unwrap_or(0.0)
}

fn get_battery_info() -> serde_json::Value {
    let bat_base = "/sys/class/power_supply/BAT0";
    if !Path::new(bat_base).exists() { return serde_json::Value::Object(serde_json::Map::new()); }

    let mut bat = serde_json::Map::new();
    let read = |name: &str| std::fs::read_to_string(format!("{}/{}", bat_base, name))
        .map(|s| s.trim().to_string()).ok();

    if let Some(s) = read("status") { bat.insert("status".into(), s.into()); }
    if let Some(c) = read("capacity").and_then(|s| s.parse::<u32>().ok()) { bat.insert("capacity".into(), c.into()); }
    if let Some(cc) = read("cycle_count").and_then(|s| s.parse::<u32>().ok()) { bat.insert("cycle_count".into(), cc.into()); }
    if let (Some(cf), Some(cfd)) = (
        read("charge_full").and_then(|s| s.parse::<u64>().ok()),
        read("charge_full_design").and_then(|s| s.parse::<u64>().ok()),
    ) {
        if let Some(pct) = (cf * 100).checked_div(cfd) {
            bat.insert("health".into(), pct.min(100).into());
        }
    }
    if let Some(p) = read("power_now").and_then(|s| s.parse::<u64>().ok()) {
        bat.insert("power_now".into(), (p as f64 / 1_000_000.0).into());
    }
    serde_json::Value::Object(bat)
}

// ── Service ───────────────────────────────────────────────────────────────────

struct PlatformInner {
    config: PlatformConfig,
    static_info: HashMap<String, serde_json::Value>,
    cpu_temp_path: Option<String>,
    gpu_temp_path: Option<String>,
    info_cache: serde_json::Value,
    last_temp_path_refresh: std::time::Instant,
}

#[derive(Clone)]
pub struct PlatformService {
    inner: Arc<Mutex<PlatformInner>>,
}

impl PlatformService {
    fn temp_path_ready(path: Option<&str>) -> bool {
        path.is_some_and(|p| Path::new(p).exists())
    }

    fn should_refresh_temp_paths(cpu_ready: bool, gpu_ready: bool, elapsed: std::time::Duration) -> bool {
        if !cpu_ready {
            return elapsed >= std::time::Duration::from_secs(5);
        }
        if !gpu_ready {
            return elapsed >= std::time::Duration::from_secs(30);
        }
        false
    }

    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let config = PlatformConfig::load();

        let mut static_info: HashMap<String, serde_json::Value> = HashMap::new();
        let hostname = if !read_dmi("hostname").is_empty() && read_dmi("hostname") != "Unknown" {
            read_dmi("hostname")
        } else {
            std::fs::read_to_string("/etc/hostname")
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|_| "Linux".to_string())
        };
        static_info.insert("hostname".into(), hostname.into());
        static_info.insert("kernel".into(), std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .map(|s| s.trim().to_string()).unwrap_or_else(|_| "Linux".to_string()).into());
        static_info.insert("os_name".into(), "Linux".into());
        
        let prod_name = read_dmi("product_name");
        let board_id = read_dmi("board_name");
        let cpu_name = get_cpu_model();
        
        static_info.insert("product_name".into(), prod_name.clone().into());
        static_info.insert("board_id".into(), board_id.clone().into());
        static_info.insert("cpu_name".into(), cpu_name.clone().into());
        static_info.insert("bios_version".into(), read_dmi("bios_version").into());
        static_info.insert("bios_date".into(), read_dmi("bios_date").into());

        let caps = crate::capabilities::detect(&board_id, &prod_name, &cpu_name);
        static_info.insert("capabilities".into(), serde_json::to_value(caps).unwrap_or_default());
        
        let ec = crate::ec::LinuxEcController::new();
        static_info.insert("ec_access".into(), ec.has_ec_access().into());
        static_info.insert("is_unsafe_ec".into(), ec.needs_ec_fallback().into());

        let cpu_temp_path = find_best_cpu_temp_path();
        let gpu_temp_path = find_gpu_temp_path();

        let info_cache = serde_json::json!({
            "cpu_temp": 0.0,
            "gpu_temp": 0.0,
            "gpu_vram": 0.0,
            "battery": {}
        });

        let svc = Self {
            inner: Arc::new(Mutex::new(PlatformInner {
                config,
                static_info,
                cpu_temp_path,
                gpu_temp_path,
                info_cache,
                last_temp_path_refresh: std::time::Instant::now(),
            })),
        };

        // Restore keyboard fixes
        {
            let g = svc.inner.lock().await;
            if g.config.prtsc_fix || g.config.f1_fix {
                let (prtsc, f1) = (g.config.prtsc_fix, g.config.f1_fix);
                drop(g);
                write_hwdb_rules(prtsc, f1);
            }
        }

        // Background monitor loop
        let inner_clone = svc.inner.clone();
        tokio::spawn(Self::monitor_loop(inner_clone));

        Ok(svc)
    }

    async fn monitor_loop(inner: Arc<Mutex<PlatformInner>>) {
        loop {
            let (cpu_path, gpu_path) = {
                let mut g = inner.lock().await;
                let cpu_ready = Self::temp_path_ready(g.cpu_temp_path.as_deref());
                let gpu_ready = Self::temp_path_ready(g.gpu_temp_path.as_deref());
                if Self::should_refresh_temp_paths(cpu_ready, gpu_ready, g.last_temp_path_refresh.elapsed()) {
                    g.last_temp_path_refresh = std::time::Instant::now();
                    g.cpu_temp_path = find_best_cpu_temp_path();
                    g.gpu_temp_path = find_gpu_temp_path();
                }
                (g.cpu_temp_path.clone(), g.gpu_temp_path.clone())
            };

            let (cpu_temp, gpu_temp, battery) = tokio::task::spawn_blocking(move || {
                let c_temp = cpu_path.as_deref().map(read_temp).unwrap_or(0.0);
                let g_temp = gpu_path.as_deref().map(read_temp).unwrap_or(0.0);
                let bat = get_battery_info();
                (c_temp, g_temp, bat)
            }).await.unwrap_or((0.0, 0.0, serde_json::Value::Object(serde_json::Map::new())));

            {
                let mut g = inner.lock().await;
                // Merge static info + dynamic values
                let mut info: HashMap<String, serde_json::Value> = g.static_info.clone();
                info.insert("cpu_temp".into(), cpu_temp.into());
                info.insert("gpu_temp".into(), gpu_temp.into());
                info.insert("gpu_vram".into(), 0.0_f64.into());
                info.insert("battery".into(), battery);
                g.info_cache = serde_json::to_value(info).unwrap_or_default();
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
    }
}

// ── Hwdb helpers ───────────────────────────────────────────────────────────────

fn write_hwdb_rules(prtsc: bool, f1: bool) {
    if is_nixos() {
        info!("NixOS detected — skipping hwdb write");
        return;
    }
    if !prtsc && !f1 {
        if Path::new(HWDB_PATH).exists() {
            let _ = std::fs::remove_file(HWDB_PATH);
            tokio::spawn(async {
                let _ = tokio::process::Command::new("systemd-hwdb").arg("update").output().await;
                let _ = tokio::process::Command::new("udevadm").args(["trigger", "-s", "input"]).output().await;
            });
        }
        return;
    }
    let mut lines = vec![
        "# HP Keyboard Fixes & Macro Mappings - Generated by Omen Space".to_string(),
        "evdev:atkbd:dmi:bvn*:bvr*:bd*:svnHP*:pn*:*".to_string(),
    ];
    if prtsc { lines.push(" KEYBOARD_KEY_b7=sysrq".to_string()); }
    if f1    { lines.push(" KEYBOARD_KEY_ab=f1".to_string()); }
    
    // Always map macro keys to standard keysyms
    lines.push(" KEYBOARD_KEY_8c=calc".to_string());   // 140 -> Calculator
    lines.push(" KEYBOARD_KEY_94=prog1".to_string());  // 148 -> Omen Key
    lines.push(" KEYBOARD_KEY_95=prog2".to_string());  // 149 -> P1/P2/Prog2
    lines.push(" KEYBOARD_KEY_bf=f21".to_string());    // 191 -> f21
    lines.push(" KEYBOARD_KEY_100=prog3".to_string()); // 256 -> P3/Prog3
    
    let content = lines.join("\n") + "\n";

    // Skip if unchanged
    if let Ok(existing) = std::fs::read_to_string(HWDB_PATH) {
        if existing == content { return; }
    }

    if let Some(dir) = Path::new(HWDB_PATH).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::write(HWDB_PATH, &content).is_ok() {
        tokio::spawn(async {
            let _ = tokio::process::Command::new("systemd-hwdb").arg("update").output().await;
            let _ = tokio::process::Command::new("udevadm").args(["trigger", "-s", "input"]).output().await;
            info!("Keyboard fixes applied via hwdb");
        });
    }
}

// ── D-Bus interface ────────────────────────────────────────────────────────────

#[interface(name = "org.hp.omen.Platform")]
impl PlatformService {
    /// GetSystemInfo — mirrors Python GetSystemInfo().
    async fn get_system_info(&self) -> String {
        let g = self.inner.lock().await;
        g.info_cache.to_string()
    }

    /// GetState — mirrors Python GetState().
    async fn get_state(&self) -> String {
        let g = self.inner.lock().await;
        let snap = serde_json::json!({
            "prtsc_fix": g.config.prtsc_fix,
            "f1_fix": g.config.f1_fix,
            "battery_charge_limit": g.config.battery_charge_limit,
        });
        snap.to_string()
    }

    /// SetKeyboardFixes(prtsc, f1) — mirrors Python SetKeyboardFixes().
    async fn set_keyboard_fixes(&self, prtsc: bool, f1: bool) -> String {
        {
            let mut g = self.inner.lock().await;
            g.config.prtsc_fix = prtsc;
            g.config.f1_fix = f1;
            g.config.save();
        }
        write_hwdb_rules(prtsc, f1);
        info!("SetKeyboardFixes: prtsc={}, f1={}", prtsc, f1);
        "OK".to_string()
    }

    /// SetBatteryCare(limit) — clamps to [50,100], writes to sysfs.
    async fn set_battery_care(&self, limit: u32) -> String {
        let limit = limit.clamp(50, 100);
        {
            let mut g = self.inner.lock().await;
            g.config.battery_charge_limit = limit;
            g.config.save();
        }

        let mut set = false;
        if let Ok(entries) = glob("/sys/class/power_supply/BAT*/charge_control_end_threshold") {
            for entry in entries.filter_map(Result::ok) {
                if tokio::fs::write(&entry, limit.to_string()).await.is_ok() { set = true; }
            }
        }

        if set {
            info!("Battery care limit set to {}%", limit);
            "OK".to_string()
        } else {
            warn!("Battery care sysfs node not available");
            "FAIL".to_string()
        }
    }

    /// CleanMemory — mirrors Python CleanMemory().
    async fn clean_memory(&self) -> String {
        let _ = tokio::process::Command::new("sync").output().await;
        match tokio::fs::write("/proc/sys/vm/drop_caches", "3\n").await {
            Ok(_) => { info!("CleanMemory: page cache dropped"); "OK".to_string() }
            Err(e) => { warn!("CleanMemory failed: {}", e); format!("Error: {}", e) }
        }
    }

    /// GenerateHardwareDump — mirrors Python GenerateHardwareDump() (Markdown).
    async fn generate_hardware_dump(&self) -> String {
        let g = self.inner.lock().await;
        let mut lines = vec![
            "# Omen Space Hardware Report".to_string(),
            String::new(),
            "Paste this into a new GitHub issue at https://github.com/yunusemreyl/omen-space/issues".to_string(),
            String::new(),
            "## System".to_string(),
        ];
        for (k, v) in &g.static_info {
            lines.push(format!("- **{}:** {}", k, v.as_str().unwrap_or_default()));
        }
        lines.join("\n")
    }

    /// GetHardwareDumpJson — mirrors Python GetHardwareDumpJson().
    async fn get_hardware_dump_json(&self) -> String {
        let g = self.inner.lock().await;

        let mut sys = serde_json::Map::new();
        sys.insert("product_name".into(), g.static_info.get("product_name").cloned().unwrap_or_default());
        sys.insert("board_id".into(), g.static_info.get("board_id").cloned().unwrap_or_default());
        sys.insert("cpu_name".into(), g.static_info.get("cpu_name").cloned().unwrap_or_default());
        sys.insert("kernel".into(), g.static_info.get("kernel").cloned().unwrap_or_default());
        sys.insert("bios_version".into(), g.static_info.get("bios_version").cloned().unwrap_or_default());
        sys.insert("bios_date".into(), g.static_info.get("bios_date").cloned().unwrap_or_default());

        // Secure Boot check
        let mut secure_boot = "Unknown".to_string();
        if let Ok(mut entries) = glob("/sys/firmware/efi/efivars/SecureBoot-*") {
            if let Some(Ok(p)) = entries.next() {
                if let Ok(b) = tokio::fs::read(&p).await {
                    secure_boot = if *b.last().unwrap_or(&0) == 1 { "Enabled".to_string() } else { "Disabled".to_string() };
                }
            }
        }
        sys.insert("secure_boot".into(), secure_boot.into());

        let json = serde_json::json!({
            "system": sys,
        });
        json.to_string()
    }

    /// RunWmiDiagnostics — Runs 1000-point WMI & EC hardware diagnostic suite.
    async fn run_wmi_diagnostics(&self) -> String {
        let report = tokio::task::spawn_blocking(|| {
            crate::wmi_diagnostics::WmiDiagnosticRunner::run_full_suite()
        }).await.unwrap_or_else(|_| crate::wmi_diagnostics::WmiDiagnosticReport {
            total_tests: 0,
            passed_tests: 0,
            failed_tests: 0,
            score_percent: 0.0,
            status_summary: "Diagnostic task panicked".to_string(),
            board_id: "Unknown".to_string(),
            product_name: "Unknown".to_string(),
            bios_version: "Unknown".to_string(),
            kernel_version: "Unknown".to_string(),
            wmi_supported: false,
            ec_supported: false,
            category_scores: std::collections::HashMap::new(),
            test_results: vec![],
        });
        let summary = report.status_summary.clone();
        
        let report_json = serde_json::to_string_pretty(&report).unwrap_or_default();
        let _ = tokio::fs::write("/tmp/wmi-diagnostics-report.json", &report_json).await;
        
        crate::notifier::DesktopNotifier::send_notification(
            "OMENSpace WMI Diagnostics Complete",
            &summary,
            if report.score_percent >= 90.0 { 0 } else { 1 },
        ).await;

        report_json
    }

    /// RunFanCleaning — Runs fan dust cleaning sequence.
    async fn run_fan_cleaning(&self) -> String {
        crate::fan_cleaning::FanCleaningService::run_cleaning_routine().await
    }

    /// CheckConflicts — Checks for conflicting thermal background daemons.
    async fn check_conflicts(&self) -> String {
        tokio::task::spawn_blocking(|| {
            let report = crate::conflict_detector::ConflictDetector::check_conflicts();
            serde_json::to_string_pretty(&report).unwrap_or_default()
        }).await.unwrap_or_default()
    }

    /// AnalyzeAcpi — Dumps and analyzes ACPI DSDT & SSDT tables for WMI GUIDs and methods.
    async fn analyze_acpi(&self) -> String {
        tokio::task::spawn_blocking(|| {
            let report = crate::acpi_diagnostics::AcpiDiagnosticRunner::analyze_acpi_tables();
            serde_json::to_string_pretty(&report).unwrap_or_default()
        }).await.unwrap_or_default()
    }

    /// GenerateTriageBundle — Generates a complete triage log bundle archive (.tar.gz) & GitHub issue template.
    async fn generate_triage_bundle(&self) -> String {
        let archive_path = tokio::task::spawn_blocking(|| {
            crate::acpi_diagnostics::AcpiDiagnosticRunner::generate_triage_bundle()
        }).await.unwrap_or_default();
        crate::notifier::DesktopNotifier::send_notification(
            "OMENSpace Triage Bundle Created",
            &format!("Diagnostic bundle saved at {}", archive_path),
            0,
        ).await;
        archive_path
    }

    /// CheckBiosUpdate — Queries HP catalog for BIOS updates for the motherboard.
    async fn check_bios_update(&self) -> String {
        let info = crate::bios_checker::BiosUpdateChecker::check_for_updates().await;
        serde_json::to_string_pretty(&info).unwrap_or_default()
    }

    /// CheckAppUpdate — Queries GitHub Releases for Omen Space updates.
    async fn check_app_update(&self) -> String {
        let info = crate::auto_updater::AutoUpdateService::check_for_updates().await;
        serde_json::to_string_pretty(&info).unwrap_or_default()
    }

    /// ApplyAppUpdate — Downloads and installs the latest Omen Space application update.
    async fn apply_app_update(&self) -> String {
        crate::auto_updater::AutoUpdateService::apply_update().await
    }

    /// ToggleOverlay — Requests the UI/overlay process to toggle visibility.
    async fn toggle_overlay(&self, #[zbus(signal_context)] ctxt: zbus::SignalContext<'_>) -> String {
        let _ = Self::macro_key_pressed(&ctxt, "overlay").await;
        "OK".to_string()
    }

    #[zbus(signal)]
    pub async fn macro_key_pressed(ctxt: &zbus::SignalContext<'_>, key_name: &str) -> zbus::Result<()>;

    async fn ping(&self) -> String {
        "OK".to_string()
    }
}

pub fn set_thermal_policy_by_name(profile: &str) -> bool {
    let mode_str = match profile.to_lowercase().as_str() {
        "performance" | "gaming" | "max" => "1",
        "quiet" | "cool" | "saver" => "2",
        _ => "0",
    };

    let mut set = false;
    // Both underscore and hyphen naming, both hp-wmi and hp-omen platform device paths
    for node in [
        "/sys/devices/platform/hp-wmi/thermal_profile",
        "/sys/devices/platform/hp-wmi/thermal-profile",
        "/sys/devices/platform/hp_wmi/thermal_profile",
        "/sys/devices/platform/hp-omen/thermal_profile",
        "/sys/devices/platform/hp-omen/thermal-profile",
    ] {
        if std::path::Path::new(node).exists()
            && std::fs::write(node, mode_str).is_ok() {
                set = true;
            }
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_refresh_temp_paths_cpu_missing() {
        assert!(PlatformService::should_refresh_temp_paths(
            false,
            false,
            std::time::Duration::from_secs(5),
        ));
        assert!(!PlatformService::should_refresh_temp_paths(
            false,
            false,
            std::time::Duration::from_secs(4),
        ));
    }

    #[test]
    fn test_should_refresh_temp_paths_gpu_missing() {
        assert!(PlatformService::should_refresh_temp_paths(
            true,
            false,
            std::time::Duration::from_secs(30),
        ));
        assert!(!PlatformService::should_refresh_temp_paths(
            true,
            false,
            std::time::Duration::from_secs(29),
        ));
        assert!(!PlatformService::should_refresh_temp_paths(
            true,
            true,
            std::time::Duration::from_secs(120),
        ));
    }
}
