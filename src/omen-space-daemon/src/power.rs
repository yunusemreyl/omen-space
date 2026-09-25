#![allow(dead_code)]
#![allow(unused_imports)]
/// Power management service - matches Python power_service.py feature-for-feature.
///
/// D-Bus interface: com.yyl.hpmanager.power  (backward compat) +
///                  org.hp.omen.Power          (new canonical name)
///
/// Methods exposed:
///   SetPowerProfile(profile: s) -> resp: s
///   GetPowerProfile()           -> j: s  (JSON)
///   SetPowerLimits(enabled: b, pl1: i, pl2: i) -> resp: s
///   SetUndervolt(mv: i)         -> resp: s
///   SetTccOffset(val: i)        -> resp: s
///   SetAppProfilesEnabled(enabled: b) -> resp: s
///   SetAppProfiles(profiles_json: s)  -> resp: s
///   Ping()                      -> resp: s
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use zbus::interface;
use log::{info, warn};

// ── Sysfs helpers ──────────────────────────────────────────────────────────────

fn sysfs_exists(path: &str) -> bool {
    Path::new(path).exists()
}

async fn sysfs_write_async(path: &str, value: &str) -> bool {
    tokio::fs::write(path, value.as_bytes()).await.is_ok()
}

async fn sysfs_read_async(path: &str) -> Option<String> {
    tokio::fs::read_to_string(path).await.ok().map(|s| s.trim().to_string())
}

// ── Config persistence ────────────────────────────────────────────────────────

const CONFIG_PATH: &str = "/etc/omen-space/power.json";

#[derive(Serialize, Deserialize, Debug, Clone)]
struct PowerConfig {
    power_profile: String,
    app_profiles_enabled: bool,
    app_profiles: HashMap<String, serde_json::Value>,
    undervolt_mv: i32,
    tcc_offset: i32,
    pl1_w: u32,
    pl2_w: u32,
    pl_enabled: bool,
    gpu_w: u32,
}

impl Default for PowerConfig {
    fn default() -> Self {
        Self {
            power_profile: "balanced".to_string(),
            app_profiles_enabled: false,
            app_profiles: HashMap::new(),
            undervolt_mv: 0,
            tcc_offset: 0,
            pl1_w: 45,
            pl2_w: 80,
            pl_enabled: false,
            gpu_w: 0,
        }
    }
}

impl PowerConfig {
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

// ── Service state ─────────────────────────────────────────────────────────────

struct AppState {
    active_app: Option<String>,
    pre_app_state: Option<(String, String)>,
}

#[derive(Clone)]
pub struct PowerService {
    config: Arc<Mutex<PowerConfig>>,
    app_state: Arc<Mutex<AppState>>,
}

impl PowerService {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let mut config = PowerConfig::load();
        // Detect real hardware profile on start
        config.power_profile = Self::detect_current_profile().await;

        let app_state = Arc::new(Mutex::new(AppState {
            active_app: None,
            pre_app_state: None,
        }));

        let svc = Self {
            config: Arc::new(Mutex::new(config)),
            app_state: app_state.clone(),
        };

        // Re-apply saved limits on startup (mirrors Python _apply_power_tuning)
        svc.apply_startup_tuning().await;

        let config_clone = svc.config.clone();
        tokio::spawn(async move {
            Self::app_monitor_loop(config_clone, app_state).await;
        });

        Ok(svc)
    }

    async fn apply_startup_tuning(&self) {
        let cfg = self.config.lock().await.clone();
        if cfg.pl_enabled {
            drop(cfg);
            // Apply power limits — re-read config inside helper
            let cfg2 = self.config.lock().await.clone();
            Self::apply_rapl_limits(cfg2.pl1_w, cfg2.pl2_w).await;
        }
    }

    // ── App Monitor Loop ───────────────────────────────────────────────────────
    
    async fn app_monitor_loop(config: Arc<Mutex<PowerConfig>>, app_state: Arc<Mutex<AppState>>) {
        let conn_res = zbus::Connection::system().await;
        if let Err(e) = &conn_res {
            warn!("app_monitor_loop failed to get zbus connection: {}", e);
        }
        let conn = conn_res.ok();

        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

            let (app_profiles_enabled, app_profiles, current_profile) = {
                let g = config.lock().await;
                (g.app_profiles_enabled, g.app_profiles.clone(), g.power_profile.clone())
            };

            let mut active_app = None;
            
            if app_profiles_enabled && !app_profiles.is_empty() {
                let profiles_map = app_profiles.clone();
                active_app = tokio::task::spawn_blocking(move || {
                    if let Ok(entries) = std::fs::read_dir("/proc") {
                        for entry in entries.filter_map(Result::ok) {
                            let name = entry.file_name().to_string_lossy().into_owned();
                            if !name.chars().all(|c| c.is_ascii_digit()) { continue; }
                            
                            if let Ok(env_data) = std::fs::read(entry.path().join("environ")) {
                                let mut has_steam = false;
                                let mut steam_id = String::new();
                                let mut has_lutris = false;
                                let mut lutris_id = String::new();
                                let mut has_flatpak = false;
                                let mut flatpak_id = String::new();

                                for item in env_data.split(|&b| b == 0) {
                                    let s = String::from_utf8_lossy(item);
                                    if s.starts_with("STEAM_COMPAT_APP_ID=") || s.starts_with("SteamAppId=") {
                                        has_steam = true;
                                        steam_id = s.split('=').nth(1).unwrap_or("").to_string();
                                    } else if s.starts_with("LUTRIS_GAME_UUID=") || s.starts_with("LUTRIS_GAME_SLUG=") {
                                        has_lutris = true;
                                        lutris_id = s.split('=').nth(1).unwrap_or("").to_string();
                                    } else if s.starts_with("FLATPAK_ID=") {
                                        has_flatpak = true;
                                        flatpak_id = s.split('=').nth(1).unwrap_or("").to_string();
                                    }
                                }
                                
                                let mut detected_id = None;
                                if has_steam && !steam_id.is_empty() {
                                    detected_id = Some(format!("steam_{}", steam_id));
                                } else if has_lutris && !lutris_id.is_empty() {
                                    detected_id = Some(format!("lutris_{}", lutris_id));
                                } else if has_flatpak && !flatpak_id.is_empty() {
                                    detected_id = Some(format!("flatpak_{}", flatpak_id));
                                }
                                
                                if let Some(id) = detected_id {
                                    if profiles_map.contains_key(&id) {
                                        return Some(id);
                                    }
                                }
                            }
                        }
                    }
                    None
                }).await.unwrap_or(None);
            }

            let mut st = app_state.lock().await;

            if !app_profiles_enabled || app_profiles.is_empty() {
                if st.active_app.is_some() {
                    Self::restore_pre_app_state(&mut st, &config, conn.as_ref()).await;
                }
                continue;
            }

            if let Some(app) = active_app {
                if st.active_app.as_deref() != Some(&app) {
                    info!("App Profiles: Detected game launch: {}", app);
                    // Save pre-app state
                    if st.pre_app_state.is_none() {
                        let current_fan = if let Some(c) = conn.as_ref() {
                            if let Ok(reply) = c.call_method(Some("org.hp.omen"), "/org/hp/omen/Fan", Some("org.hp.omen.Fan"), "GetFanMode", &()).await {
                                let body: String = reply.body().deserialize().unwrap_or_else(|_| "auto".to_string());
                                body
                            } else { "auto".to_string() }
                        } else { "auto".to_string() };
                        st.pre_app_state = Some((current_profile, current_fan));
                    }
                    
                    st.active_app = Some(app.clone());
                    
                    // Apply app profile
                    if let Some(prof) = app_profiles.get(&app) {
                        let p_prof = prof["power_profile"].as_str().unwrap_or("performance");
                        let p_fan = prof["fan_mode"].as_str().unwrap_or("auto");
                        
                        let mut cfg = config.lock().await;
                        cfg.power_profile = p_prof.to_string();
                        cfg.save();
                        drop(cfg); 
                        
                        Self::sync_omen_profile(p_prof).await;
                        Self::sync_gpu_power(p_prof).await;
                        
                        if let Some(c) = conn.as_ref() {
                            let _ = c.call_method(Some("org.hp.omen"), "/org/hp/omen/Fan", Some("org.hp.omen.Fan"), "SetFanMode", &p_fan).await;
                        }
                    }
                }
            } else {
                if st.active_app.is_some() {
                    info!("App Profiles: Game closed. Restoring previous state.");
                    Self::restore_pre_app_state(&mut st, &config, conn.as_ref()).await;
                }
            }
        }
    }

    async fn restore_pre_app_state(st: &mut tokio::sync::MutexGuard<'_, AppState>, config: &Arc<Mutex<PowerConfig>>, conn: Option<&zbus::Connection>) {
        if let Some((p_prof, p_fan)) = st.pre_app_state.take() {
            let mut cfg = config.lock().await;
            cfg.power_profile = p_prof.clone();
            cfg.save();
            drop(cfg);
            
            Self::sync_omen_profile(&p_prof).await;
            Self::sync_gpu_power(&p_prof).await;
            
            if let Some(c) = conn {
                let _ = c.call_method(Some("org.hp.omen"), "/org/hp/omen/Fan", Some("org.hp.omen.Fan"), "SetFanMode", &p_fan).await;
            }
        }
        st.active_app = None;
    }

    // ── Profile detection ──────────────────────────────────────────────────────

    fn has_custom_power_manager() -> bool {
        std::path::Path::new("/usr/bin/tlp").exists() || 
        std::path::Path::new("/usr/sbin/tlp").exists() || 
        std::path::Path::new("/usr/bin/auto-cpufreq").exists()
    }

    async fn detect_current_profile() -> String {
        if !Self::has_custom_power_manager() {
        // 1. Try system76-power if installed
        if let Ok(out) = tokio::process::Command::new("system76-power").arg("profile").output().await {
            if out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout).trim().to_lowercase();
                if stdout.contains("performance") { return "performance".to_string(); }
                if stdout.contains("battery") { return "power-saver".to_string(); }
                if stdout.contains("balanced") { return "balanced".to_string(); }
            }
        }

        // 2. Try powerprofilesctl if installed
        if let Ok(out) = tokio::process::Command::new("powerprofilesctl").arg("get").output().await {
            if out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout).trim().to_lowercase();
                return Self::normalize_profile(&stdout);
            }
        }
        }

        // 3. Fallback: Try platform_profile first (underscore then hyphen)
        let platform_paths = [
            "/sys/firmware/acpi/platform_profile",
            "/sys/devices/platform/hp-wmi/platform_profile",
            "/sys/devices/platform/hp-wmi/platform-profile",
        ];
        for p in platform_paths {
            if let Some(raw) = sysfs_read_async(p).await {
                return Self::normalize_profile(&raw);
            }
        }
        // Fallback: thermal_profile (both naming styles)
        for p in [
            "/sys/devices/platform/hp-wmi/thermal_profile",
            "/sys/devices/platform/hp-wmi/thermal-profile",
            "/sys/devices/platform/hp-omen/thermal_profile",
            "/sys/devices/platform/hp-omen/thermal-profile",
        ] {
            if let Some(raw) = sysfs_read_async(p).await {
                if raw.trim() == "1" { return "performance".to_string(); }
                return "balanced".to_string();
            }
        }
        "balanced".to_string()
    }

    async fn get_available_profiles() -> Vec<String> {
        // All known platform_profile paths — underscore and hyphen variants
        let paths = [
            "/sys/firmware/acpi/platform_profile",
            "/sys/devices/platform/hp-wmi/platform_profile",
            "/sys/devices/platform/hp-wmi/platform-profile",
        ];
        for p in paths {
            if !sysfs_exists(p) { continue; }
            // Try _choices suffix (underscore) then -choices (hyphen)
            for choices_suffix in ["_choices", "-choices"] {
                let choices_path = format!("{}{}", p, choices_suffix);
                if let Some(choices_raw) = sysfs_read_async(&choices_path).await {
                    let choices: Vec<String> = choices_raw
                        .split_whitespace()
                        .map(|s| s.trim_matches(|c| c == '[' || c == ']').to_lowercase())
                        .map(|s| Self::normalize_profile(&s))
                        .collect();
                    if !choices.is_empty() {
                        let mut unique = Vec::new();
                        for c in choices {
                            if !unique.contains(&c) {
                                unique.push(c);
                            }
                        }
                        return unique;
                    }
                }
            }
        }

        // Fallback to standard 3 profiles if WMI choices are missing
        vec!["power-saver".to_string(), "balanced".to_string(), "performance".to_string()]
    }

    fn normalize_profile(raw: &str) -> String {
        match raw.to_lowercase().as_str() {
            "performance" | "custom" => "performance".to_string(),
            "low-power" | "quiet" | "cool" | "power-saver" => "power-saver".to_string(),
            _ => "balanced".to_string(),
        }
    }

    // ── Profile application ────────────────────────────────────────────────────

    /// Writes platform_profile (checks _choices) + thermal_profile — mirrors
    /// Python PowerProfileController._sync_omen_profile().
    async fn sync_omen_profile(profile: &str) -> bool {
        let mut ok = false;

        if !Self::has_custom_power_manager() {
            // 1. Try setting via system76-power if installed
            let s76_profile = match profile {
                "performance" => "performance",
                "power-saver" => "battery",
                _ => "balanced",
            };
            if let Ok(mut child) = tokio::process::Command::new("system76-power").args(["profile", s76_profile]).spawn() {
                if let Ok(status) = child.wait().await {
                    if status.success() {
                        info!("Set system76-power profile to '{}'", s76_profile);
                        ok = true;
                    }
                }
            }

            // 2. Try setting via powerprofilesctl if installed
            if let Ok(mut child) = tokio::process::Command::new("powerprofilesctl").args(["set", profile]).spawn() {
                if let Ok(status) = child.wait().await {
                    if status.success() {
                        info!("Set powerprofilesctl profile to '{}'", profile);
                        ok = true;
                    }
                }
            }
        }

        // 3. Fallback/Explicit Override: ACPI platform_profile — underscore and hyphen variants
        let acpi_paths = [
            "/sys/firmware/acpi/platform_profile",
            "/sys/devices/platform/hp-wmi/platform_profile",
            "/sys/devices/platform/hp-wmi/platform-profile",
        ];
        for p in acpi_paths {
            if !sysfs_exists(p) { continue; }

            // Try _choices then -choices
            let mut choices_raw = String::new();
            for sfx in ["_choices", "-choices"] {
                let cp = format!("{}{}", p, sfx);
                if let Some(raw) = sysfs_read_async(&cp).await {
                    choices_raw = raw;
                    break;
                }
            }

            let choices: std::collections::HashSet<String> = if choices_raw.is_empty() {
                std::collections::HashSet::new()
            } else {
                choices_raw
                    .split_whitespace()
                    .map(|s| s.trim_matches(|c| c == '[' || c == ']').to_lowercase())
                    .collect()
            };

            let candidates: &[&str] = match profile {
                "performance" => &["performance"],
                "power-saver" => &["low-power", "quiet", "cool", "power-saver"],
                _             => &["balanced"],
            };

            let to_write = if choices.is_empty() {
                candidates[0]
            } else {
                candidates
                    .iter()
                    .find(|&&c| choices.contains(c))
                    .copied()
                    .unwrap_or_else(|| {
                        if choices.contains("balanced") { "balanced" } else { candidates[0] }
                    })
            };

            if sysfs_write_async(p, to_write).await {
                info!("Set platform_profile='{}' via {}", to_write, p);
                ok = true;
            }
        }

        // thermal_profile / thermal-profile (both naming styles, both hp-wmi paths)
        let thermal_val = if profile == "performance" { "1" } else { "0" };
        for p in [
            "/sys/devices/platform/hp-wmi/thermal_profile",
            "/sys/devices/platform/hp-wmi/thermal-profile",
            "/sys/devices/platform/hp-omen/thermal_profile",
            "/sys/devices/platform/hp-omen/thermal-profile",
        ] {
            if sysfs_exists(p) && sysfs_write_async(p, thermal_val).await {
                info!("Set thermal_profile={} via {}", thermal_val, p);
                ok = true;
            }
        }

        ok
    }

    /// Sync GPU TGP + PPAB — mirrors Python _sync_kernel_gpu_power().
    async fn sync_gpu_power(profile: &str) {
        let base = if sysfs_exists("/sys/devices/platform/hp-wmi") {
            "/sys/devices/platform/hp-wmi"
        } else {
            "/sys/devices/platform/hp-omen"
        };
        let tgp = format!("{}/gpu_tgp", base);
        let ppab = format!("{}/gpu_ppab", base);
        if !sysfs_exists(&tgp) { return; }
        match profile {
            "performance" => {
                let _ = sysfs_write_async(&tgp, "1").await;
                let _ = sysfs_write_async(&ppab, "1").await;
            }
            "balanced" => {
                let _ = sysfs_write_async(&tgp, "0").await;
                let _ = sysfs_write_async(&ppab, "1").await;
            }
            _ => {
                let _ = sysfs_write_async(&tgp, "0").await;
                let _ = sysfs_write_async(&ppab, "0").await;
            }
        }
        info!("GPU TGP/PPAB synced for profile '{}'", profile);
    }


    // ── Intel RAPL power limits ────────────────────────────────────────────────

    async fn apply_rapl_limits(pl1: u32, pl2: u32) {
        let specs = crate::sysmon::get_hardware_specs();
        let is_amd = specs.cpu_spec.to_uppercase().contains("AMD");

        if is_amd {
            let mw1 = pl1 * 1000;
            let mw2 = pl2 * 1000;
            let _ = tokio::process::Command::new("ryzenadj")
                .arg(format!("--stapm-limit={}", mw1))
                .arg(format!("--slow-limit={}", mw1))
                .arg(format!("--fast-limit={}", mw2))
                .output()
                .await;
            info!("AMD RyzenAdj limits set: STAPM/Slow={}W Fast={}W", pl1, pl2);
        } else {
            let rapl1 = "/sys/class/powercap/intel-rapl/intel-rapl:0/constraint_0_power_limit_uw";
            let rapl2 = "/sys/class/powercap/intel-rapl/intel-rapl:0/constraint_1_power_limit_uw";
            if sysfs_exists(rapl1) {
                let _ = sysfs_write_async(rapl1, &(pl1 * 1_000_000).to_string()).await;
            }
            if sysfs_exists(rapl2) {
                let _ = sysfs_write_async(rapl2, &(pl2 * 1_000_000).to_string()).await;
            }
            info!("Intel RAPL limits set: PL1={}W PL2={}W", pl1, pl2);
        }
    }
}

// ── D-Bus interface ────────────────────────────────────────────────────────────

#[interface(name = "org.hp.omen.Power")]
impl PowerService {

    /// GetPowerProfile — returns JSON matching Python GetPowerProfile().
    async fn get_power_profile(&self) -> String {
        let cfg = self.config.lock().await.clone();
        let mut active = Self::detect_current_profile().await;
        
        let available_profiles = Self::get_available_profiles().await;
        
        // If hardware doesn't support power-saver natively but user selected it,
        // report it back so the UI doesn't bounce to "Balanced". Software limits (PL1/PL2, GPU)
        // are still applied under the hood.
        if active == "balanced" && cfg.power_profile == "power-saver" {
            if !available_profiles.iter().any(|c| c == "power-saver" || c == "low-power" || c == "quiet" || c == "cool") {
                active = "power-saver".to_string();
            }
        }

        let mut real_pl1 = cfg.pl1_w;
        let mut real_pl2 = cfg.pl2_w;
        if let Some(val) = sysfs_read_async("/sys/class/powercap/intel-rapl/intel-rapl:0/constraint_0_power_limit_uw").await {
            if let Ok(uw) = val.parse::<u32>() { real_pl1 = uw / 1_000_000; }
        }
        if let Some(val) = sysfs_read_async("/sys/class/powercap/intel-rapl/intel-rapl:0/constraint_1_power_limit_uw").await {
            if let Ok(uw) = val.parse::<u32>() { real_pl2 = uw / 1_000_000; }
        }

        let available_profiles = Self::get_available_profiles().await;

        let json = serde_json::json!({
            "available": true,
            "active": active,
            "profiles": available_profiles,
            "app_profiles_enabled": cfg.app_profiles_enabled,
            "app_profiles": cfg.app_profiles,
            "active_app": null,
            "undervolt_mv": cfg.undervolt_mv,
            "tcc_offset": cfg.tcc_offset,
            "pl1_w": real_pl1,
            "pl2_w": real_pl2,
            "pl_enabled": cfg.pl_enabled,
            "gpu_w": cfg.gpu_w,
        });
        json.to_string()
    }


    /// SetPowerProfile — mirrors Python SetPowerProfile().
    async fn set_power_profile(&self, profile: String) -> String {
        let normalized = Self::normalize_profile(&profile);
        let ok = Self::sync_omen_profile(&normalized).await;

        if ok {
            {
                let mut cfg = self.config.lock().await;
                cfg.power_profile = normalized.clone();
                cfg.save();
            }
            // Async GPU sync (non-blocking, like Python threads)
            let p = normalized.clone();
            let _cfg_clone = self.config.clone();
            tokio::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                Self::sync_gpu_power(&p).await;
            });
            info!("Power profile set to '{}'", normalized);
            "OK".to_string()
        } else {
            let err_msg = format!(
                "Güç profili '{}' uygulanamadı. BIOS OMEN komut arayüzü erişilebilir değil veya donanım desteklemiyor.",
                profile
            );
            warn!("{}", err_msg);
            crate::notifier::DesktopNotifier::notify_error(
                "Güç Profili Değiştirilemedi",
                &err_msg,
            ).await;
            "FAIL".to_string()
        }
    }

    /// SetPowerLimits — mirrors Python SetPowerLimits(enabled, pl1, pl2).
    async fn set_power_limits(&self, enabled: bool, pl1: i32, pl2: i32) -> String {
        // Strict range validation (same as Python)
        if !(1..=200).contains(&pl1) {
            warn!("SetPowerLimits: pl1={} out of safe range [1, 200]", pl1);
            return "FAIL".to_string();
        }
        if !(1..=250).contains(&pl2) {
            warn!("SetPowerLimits: pl2={} out of safe range [1, 250]", pl2);
            return "FAIL".to_string();
        }
        let pl2 = pl2.max(pl1); // clamp: pl2 must >= pl1

        {
            let mut cfg = self.config.lock().await;
            cfg.pl_enabled = enabled;
            cfg.pl1_w = pl1 as u32;
            cfg.pl2_w = pl2 as u32;
            cfg.save();
        }

        if enabled {
            Self::apply_rapl_limits(pl1 as u32, pl2 as u32).await;
        }

        info!("SetPowerLimits: enabled={}, PL1={}W, PL2={}W", enabled, pl1, pl2);
        "OK".to_string()
    }

    /// SetUndervolt — mirrors Python SetUndervolt(mv).
    /// Saves to config; actual MSR write done by undervolt.rs.
    async fn set_undervolt(&self, mv: i32) -> String {
        let mv = mv.clamp(-250, 250);
        let mut cfg = self.config.lock().await;
        cfg.undervolt_mv = mv;
        cfg.save();
        info!("SetUndervolt: {}mV (saved; apply via undervolt service)", mv);
        "OK".to_string()
    }

    /// SetTccOffset — mirrors Python SetTccOffset(val).
    async fn set_tcc_offset(&self, val: i32) -> String {
        let val = val.clamp(0, 15);
        let mut cfg = self.config.lock().await;
        cfg.tcc_offset = val;
        cfg.save();
        info!("SetTccOffset: {} (saved)", val);
        "OK".to_string()
    }

    /// SetAppProfilesEnabled — mirrors Python SetAppProfilesEnabled(enabled).
    async fn set_app_profiles_enabled(&self, enabled: bool) -> String {
        let mut cfg = self.config.lock().await;
        cfg.app_profiles_enabled = enabled;
        cfg.save();
        info!("SetAppProfilesEnabled: {}", enabled);
        "OK".to_string()
    }

    /// SetAppProfiles — mirrors Python SetAppProfiles(profiles_json).
    async fn set_app_profiles(&self, profiles_json: String) -> String {
        match serde_json::from_str::<HashMap<String, serde_json::Value>>(&profiles_json) {
            Ok(data) => {
                let mut cfg = self.config.lock().await;
                cfg.app_profiles = data;
                cfg.save();
                info!("SetAppProfiles: updated {} entries", cfg.app_profiles.len());
                "OK".to_string()
            }
            Err(e) => {
                warn!("SetAppProfiles: JSON parse error: {}", e);
                "FAIL".to_string()
            }
        }
    }

    async fn ping(&self) -> String {
        "OK".to_string()
    }
}
