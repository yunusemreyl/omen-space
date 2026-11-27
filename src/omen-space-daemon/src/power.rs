use log::{debug, info, warn};
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
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use zbus::interface;

// ── Sysfs backend & helpers ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct PowerSysfsPaths {
    pub acpi_platform_profile: PathBuf,
    pub hp_wmi_platform_profile: PathBuf,
    pub hp_wmi_platform_profile_hyphen: PathBuf,
    pub class_platform_profile_dir: PathBuf,
    pub thermal_profile_paths: Vec<PathBuf>,
    pub gpu_mux_mode: PathBuf,
}

impl Default for PowerSysfsPaths {
    fn default() -> Self {
        Self::system()
    }
}

impl PowerSysfsPaths {
    pub fn system() -> Self {
        Self {
            acpi_platform_profile: PathBuf::from("/sys/firmware/acpi/platform_profile"),
            hp_wmi_platform_profile: PathBuf::from("/sys/devices/platform/hp-wmi/platform_profile"),
            hp_wmi_platform_profile_hyphen: PathBuf::from(
                "/sys/devices/platform/hp-wmi/platform-profile",
            ),
            class_platform_profile_dir: PathBuf::from("/sys/class/platform-profile"),
            thermal_profile_paths: vec![
                PathBuf::from("/sys/devices/platform/hp-wmi/thermal_profile"),
                PathBuf::from("/sys/devices/platform/hp-wmi/thermal-profile"),
                PathBuf::from("/sys/devices/platform/hp-omen/thermal_profile"),
                PathBuf::from("/sys/devices/platform/hp-omen/thermal-profile"),
            ],
            gpu_mux_mode: PathBuf::from("/sys/devices/platform/hp-wmi/gpu_mux_mode"),
        }
    }

    pub fn platform_profile_candidates(&self, io: &dyn PowerSysfsIo) -> Vec<PathBuf> {
        let mut out = vec![
            self.acpi_platform_profile.clone(),
            self.hp_wmi_platform_profile.clone(),
            self.hp_wmi_platform_profile_hyphen.clone(),
        ];
        if let Some(hp_class) = find_hp_class_platform_profile(&self.class_platform_profile_dir, io)
        {
            if !out.contains(&hp_class) {
                out.push(hp_class);
            }
        }
        out
    }

    pub fn any_hw_node_exists(&self, io: &dyn PowerSysfsIo) -> bool {
        self.platform_profile_candidates(io)
            .iter()
            .any(|p| io.exists(p))
            || self.thermal_profile_paths.iter().any(|p| io.exists(p))
    }
}

/// Explicitly ignore non-HP platform_profile providers such as `amd-pmf` whose
/// sysfs writes may succeed as no-ops on HP OMEN laptops in UMA mode (#396).
fn find_hp_class_platform_profile(dir: &Path, io: &dyn PowerSysfsIo) -> Option<PathBuf> {
    let entries = io.list_class_profiles(dir);
    for entry in entries {
        if let Ok(name) = io.read_trimmed(&entry.join("name")) {
            let trimmed = name.trim();
            if trimmed.eq_ignore_ascii_case("hp-wmi") || trimmed.eq_ignore_ascii_case("hp-omen") {
                let profile_path = entry.join("profile");
                if io.exists(&profile_path) {
                    return Some(profile_path);
                }
            }
        }
    }
    None
}

pub trait PowerSysfsIo: Send + Sync {
    fn exists(&self, path: &Path) -> bool;
    fn read_trimmed(&self, path: &Path) -> std::io::Result<String>;
    fn write_value(&self, path: &Path, value: &str) -> std::io::Result<()>;
    fn list_class_profiles(&self, dir: &Path) -> Vec<PathBuf>;
}

#[derive(Clone, Default)]
pub struct RealSysfsIo;

impl PowerSysfsIo for RealSysfsIo {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn read_trimmed(&self, path: &Path) -> std::io::Result<String> {
        Ok(std::fs::read_to_string(path)?.trim().to_string())
    }

    fn write_value(&self, path: &Path, value: &str) -> std::io::Result<()> {
        std::fs::write(path, value.as_bytes())
    }

    fn list_class_profiles(&self, dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                out.push(e.path());
            }
        }
        out.sort();
        out
    }
}

fn sysfs_exists(path: &str) -> bool {
    Path::new(path).exists()
}

async fn sysfs_write_async(path: &str, value: &str) -> bool {
    tokio::fs::write(path, value.as_bytes()).await.is_ok()
}

async fn sysfs_read_async(path: &str) -> Option<String> {
    tokio::fs::read_to_string(path)
        .await
        .ok()
        .map(|s| s.trim().to_string())
}

// ── ACPI health guard ──────────────────────────────────────────────────────────
/// Returns `true` if ACPI platform_profile sysfs responds within the timeout.
/// If ACPI is stuck (AE_AML_LOOP_TIMEOUT / AE_AML_BUFFER_LIMIT storm), the
/// read either hangs or returns an I/O error — in that case we must NOT send
/// further WMI commands or we make the freeze worse.
async fn acpi_is_healthy_at(paths: &PowerSysfsPaths, io: Arc<dyn PowerSysfsIo>) -> bool {
    let check_paths = [
        paths.acpi_platform_profile.clone(),
        paths.hp_wmi_platform_profile.clone(),
        paths.hp_wmi_platform_profile_hyphen.clone(),
    ];
    for p in check_paths {
        if !io.exists(&p) {
            continue;
        }
        let io_c = io.clone();
        // Wrap the read in a short timeout so a frozen ACPI doesn't block us.
        let result = tokio::time::timeout(
            tokio::time::Duration::from_millis(500),
            tokio::task::spawn_blocking(move || io_c.read_trimmed(&p)),
        )
        .await;
        return matches!(result, Ok(Ok(Ok(_))));
    }
    // No platform_profile node — not an ACPI-driven board, assume healthy.
    true
}

// ── Config persistence ────────────────────────────────────────────────────────

const CONFIG_PATH: &str = "/etc/omen-space/power.json";

fn default_true() -> bool {
    true
}

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
    #[serde(default = "default_true")]
    ac_auto_switch: bool,
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
            ac_auto_switch: true,
        }
    }
}

impl PowerConfig {
    fn load_from(path: &Path) -> Self {
        if let Ok(data) = std::fs::read_to_string(path) {
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    fn save_to(&self, path: &Path) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }
}

// ── Service state ─────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct PowerService {
    config: Arc<Mutex<PowerConfig>>,
    config_path: PathBuf,
    sysfs_paths: PowerSysfsPaths,
    sysfs_io: Arc<dyn PowerSysfsIo>,
    rapl_baseline: Option<(crate::rapl_baseline::Baseline, bool)>,
    last_error: Arc<Mutex<Option<String>>>,
    apply_lock: Arc<Mutex<()>>,
    ppd_suppress_until: Arc<Mutex<Option<std::time::Instant>>>,
}

impl PowerService {
    pub async fn check_acpi_healthy() -> bool {
        acpi_is_healthy_at(&PowerSysfsPaths::system(), Arc::new(RealSysfsIo)).await
    }

    pub async fn is_app_profiles_enabled(&self) -> bool {
        self.config.lock().await.app_profiles_enabled
    }

    pub async fn is_ac_auto_switch_enabled(&self) -> bool {
        self.config.lock().await.ac_auto_switch
    }

    pub async fn current_profile(&self) -> String {
        self.config.lock().await.power_profile.clone()
    }

    /// Arm echo suppression so our own `sync_ppd` write doesn't feed back
    /// through the PPD PropertiesChanged listener (#395).
    async fn arm_ppd_suppression(&self) {
        let mut guard = self.ppd_suppress_until.lock().await;
        *guard = Some(std::time::Instant::now() + std::time::Duration::from_millis(1800));
    }

    async fn is_ppd_echo_suppressed(&self) -> bool {
        let guard = self.ppd_suppress_until.lock().await;
        guard.is_some_and(|until| std::time::Instant::now() < until)
    }

    pub async fn set_profile_result(&self, profile: &str, sync_to_ppd: bool) -> Result<(), String> {
        let _guard = self.apply_lock.lock().await;
        let normalized = Self::normalize_profile(profile);

        match self.sync_omen_profile_with(&normalized, sync_to_ppd).await {
            Ok(()) => {
                *self.last_error.lock().await = None;
                {
                    let mut cfg = self.config.lock().await;
                    cfg.power_profile = normalized.clone();
                    cfg.save_to(&self.config_path);
                }
                if self.sysfs_paths.acpi_platform_profile
                    == Path::new("/sys/firmware/acpi/platform_profile")
                {
                    let p = normalized.clone();
                    let sync_epp = !Self::has_custom_power_manager();
                    tokio::spawn(async move {
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                        Self::sync_gpu_power(&p).await;
                        if sync_epp {
                            let p_epp = p;
                            let _ = tokio::task::spawn_blocking(move || Self::sync_cpu_epp(&p_epp))
                                .await;
                        }
                    });
                }
                info!("Power profile set to '{}'", normalized);
                Ok(())
            }
            Err(err_msg) => {
                let real_hw = self.detect_current_profile_with().await;
                {
                    let mut cfg = self.config.lock().await;
                    cfg.power_profile = real_hw;
                }
                *self.last_error.lock().await = Some(err_msg.clone());
                Err(err_msg)
            }
        }
    }

    pub async fn set_profile_internal(&self, profile: &str) -> bool {
        self.set_profile_result(profile, true).await.is_ok()
    }

    /// Handle external `net.hadess.PowerProfiles` `ActiveProfile` updates without
    /// echoing back to `powerprofilesctl` and while suppressing self-triggered signals (#395).
    pub async fn set_profile_from_ppd(&self, profile: &str) -> bool {
        if self.is_ppd_echo_suppressed().await {
            debug!(
                "[PPD sync] Ignoring echo ActiveProfile '{}' during suppression window",
                profile
            );
            return true;
        }
        let target = Self::normalize_profile(profile);
        let current_hw = self.detect_current_profile_with().await;
        let current_cfg = self.config.lock().await.power_profile.clone();
        if current_hw == target && current_cfg == target {
            return true;
        }
        self.set_profile_result(profile, false).await.is_ok()
    }

    async fn read_current_rapl_limits() -> (Option<u32>, Option<u32>) {
        let p1 = sysfs_read_async(
            "/sys/class/powercap/intel-rapl/intel-rapl:0/constraint_0_power_limit_uw",
        )
        .await
        .and_then(|v| v.parse::<u64>().ok())
        .map(|uw| (uw / 1_000_000) as u32)
        .filter(|&w| (5..=250).contains(&w));
        let p2 = sysfs_read_async(
            "/sys/class/powercap/intel-rapl/intel-rapl:0/constraint_1_power_limit_uw",
        )
        .await
        .and_then(|v| v.parse::<u64>().ok())
        .map(|uw| (uw / 1_000_000) as u32)
        .filter(|&w| (5..=250).contains(&w));
        (p1, p2)
    }

    async fn load_or_capture_boot_rapl_baseline() -> Option<(crate::rapl_baseline::Baseline, bool)>
    {
        let boot_id =
            std::fs::read_to_string("/proc/sys/kernel/random/boot_id").unwrap_or_default();
        let (pl1, pl2) = Self::read_current_rapl_limits().await;
        crate::rapl_baseline::load_or_capture(
            std::path::Path::new("/var/lib/omen-space-daemon/rapl_baseline.json"),
            std::path::Path::new("/run/omen-space/rapl_baseline.json"),
            std::path::Path::new("/var/lib/omen-space/firmware_rapl_baseline.json"),
            boot_id.trim(),
            pl1.zip(pl2),
        )
    }

    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Self::new_with_paths(
            PathBuf::from(CONFIG_PATH),
            PowerSysfsPaths::system(),
            Arc::new(RealSysfsIo),
            true,
        )
        .await
    }

    pub async fn new_with_paths(
        config_path: PathBuf,
        sysfs_paths: PowerSysfsPaths,
        sysfs_io: Arc<dyn PowerSysfsIo>,
        load_rapl: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut config = PowerConfig::load_from(&config_path);
        // Detect real hardware profile on start
        config.power_profile =
            Self::detect_current_profile_for(&sysfs_paths, sysfs_io.as_ref()).await;

        let rapl_baseline = if load_rapl {
            let b = Self::load_or_capture_boot_rapl_baseline().await;
            if !config.pl_enabled {
                if let Some((ref base, _)) = b {
                    config.pl1_w = base.pl1_w;
                    config.pl2_w = base.pl2_w;
                }
            }
            b
        } else {
            None
        };

        let svc = Self {
            config: Arc::new(Mutex::new(config)),
            config_path,
            sysfs_paths,
            sysfs_io,
            rapl_baseline,
            last_error: Arc::new(Mutex::new(None)),
            apply_lock: Arc::new(Mutex::new(())),
            ppd_suppress_until: Arc::new(Mutex::new(None)),
        };

        if load_rapl {
            svc.apply_startup_tuning().await;
        }
        Ok(svc)
    }

    async fn apply_startup_tuning(&self) {
        let cfg = self.config.lock().await.clone();
        if cfg.pl_enabled {
            Self::apply_rapl_limits(cfg.pl1_w, cfg.pl2_w).await;
        }
    }

    // ── Profile detection ──────────────────────────────────────────────────────

    fn has_custom_power_manager() -> bool {
        std::path::Path::new("/usr/bin/tlp").exists()
            || std::path::Path::new("/usr/sbin/tlp").exists()
            || std::path::Path::new("/usr/bin/auto-cpufreq").exists()
    }

    // ── PPD two-way sync (fix #283) ────────────────────────────────────────────
    /// Notify power-profiles-daemon of the new profile so desktop environments
    /// (GNOME, KDE, Noctalia, etc.) that subscribe to PPD stay in sync.
    /// Returns `true` when either `powerprofilesctl` or the PPD D-Bus call succeeded.
    async fn sync_ppd_once(ppd_profile: &str) -> Result<(), bool> {
        let mut rejected = false;
        // 1. Try powerprofilesctl CLI (works on most distros).
        let cli_result = tokio::time::timeout(
            tokio::time::Duration::from_secs(3),
            tokio::process::Command::new("powerprofilesctl")
                .args(["set", ppd_profile])
                .output(),
        )
        .await;
        match cli_result {
            Ok(Ok(out)) if out.status.success() => {
                info!("[PPD sync] powerprofilesctl set '{}' OK", ppd_profile);
                return Ok(());
            }
            Ok(Ok(out)) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                if out.status.code() == Some(2) || stderr.contains("choose from ") {
                    rejected = true;
                    log::debug!(
                        "[PPD sync] powerprofilesctl set '{}' unavailable (exit {}): {}",
                        ppd_profile,
                        out.status,
                        stderr.trim()
                    );
                } else {
                    warn!(
                        "[PPD sync] powerprofilesctl set '{}' failed (exit {}): {}",
                        ppd_profile,
                        out.status,
                        stderr.trim()
                    );
                }
            }
            _ => {
                // Not installed or timed out — fall through to D-Bus path.
            }
        }

        // 2. Fallback: D-Bus call to net.hadess.PowerProfiles.
        if let Ok(conn) = zbus::Connection::system().await {
            let result = tokio::time::timeout(
                tokio::time::Duration::from_secs(3),
                conn.call_method(
                    Some("net.hadess.PowerProfiles"),
                    "/net/hadess/PowerProfiles",
                    Some("org.freedesktop.DBus.Properties"),
                    "Set",
                    &(
                        "net.hadess.PowerProfiles",
                        "ActiveProfile",
                        zbus::zvariant::Value::from(ppd_profile),
                    ),
                ),
            )
            .await;
            match result {
                Ok(Ok(_)) => {
                    info!("[PPD sync] D-Bus ActiveProfile set to '{}' OK", ppd_profile);
                    return Ok(());
                }
                Ok(Err(e)) => {
                    let msg = e.to_string();
                    if msg.contains("Invalid")
                        || msg.contains("not valid")
                        || msg.contains("performance")
                    {
                        rejected = true;
                    }
                    if msg.contains("NameHasNoOwner")
                        || msg.contains("ServiceUnknown")
                        || msg.contains("unit is masked")
                    {
                        log::debug!("[PPD sync] PPD service inactive/masked: {}", msg);
                    } else {
                        warn!("[PPD sync] D-Bus call failed: {}", msg);
                    }
                }
                Err(_) => warn!("[PPD sync] D-Bus call to net.hadess.PowerProfiles timed out"),
            }
        }
        Err(rejected)
    }

    async fn sync_ppd(profile: &str) -> bool {
        // Map omen-space profile names to the PPD profile IDs.
        let ppd_profile = match profile {
            "performance" => "performance",
            "power-saver" => "power-saver",
            _ => "balanced",
        };

        match Self::sync_ppd_once(ppd_profile).await {
            Ok(()) => true,
            Err(true) if ppd_profile == "performance" => {
                // Many HP laptops expose only `power-saver` and `balanced` in PPD; fall back to
                // `balanced` so PPD does not remain stuck in `power-saver` after AC reconnect (#398).
                Self::sync_ppd_once("balanced").await.is_ok()
            }
            Err(_) => false,
        }
    }

    async fn detect_current_profile_with(&self) -> String {
        Self::detect_current_profile_for(&self.sysfs_paths, self.sysfs_io.as_ref()).await
    }

    async fn detect_current_profile_for(paths: &PowerSysfsPaths, io: &dyn PowerSysfsIo) -> String {
        // 1. Prefer direct OMEN/ACPI hardware state (never amd-pmf!)
        // so placeholder power-profiles-daemon states never mask the real OMEN mode (#293, #297, #396).
        for p in paths.platform_profile_candidates(io) {
            if let Ok(raw) = io.read_trimmed(&p) {
                return Self::normalize_profile(&raw);
            }
        }
        // Fallback: thermal_profile (both naming styles)
        for p in &paths.thermal_profile_paths {
            if let Ok(raw) = io.read_trimmed(p) {
                if raw.trim() == "1" {
                    return "performance".to_string();
                }
                return "balanced".to_string();
            }
        }

        if paths.acpi_platform_profile == Path::new("/sys/firmware/acpi/platform_profile")
            && !Self::has_custom_power_manager()
        {
            if let Ok(out) = tokio::process::Command::new("system76-power")
                .arg("profile")
                .output()
                .await
            {
                if out.status.success() {
                    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_lowercase();
                    if stdout.contains("performance") {
                        return "performance".to_string();
                    }
                    if stdout.contains("battery") {
                        return "power-saver".to_string();
                    }
                    if stdout.contains("balanced") {
                        return "balanced".to_string();
                    }
                }
            }

            if let Ok(out) = tokio::process::Command::new("powerprofilesctl")
                .arg("get")
                .output()
                .await
            {
                if out.status.success() {
                    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_lowercase();
                    return Self::normalize_profile(&stdout);
                }
            }
        }

        "balanced".to_string()
    }

    async fn get_available_profiles_for(
        paths: &PowerSysfsPaths,
        io: &dyn PowerSysfsIo,
    ) -> Vec<String> {
        for p in paths.platform_profile_candidates(io) {
            if !io.exists(&p) {
                continue;
            }
            for choices_suffix in ["_choices", "-choices"] {
                let choices_path = PathBuf::from(format!("{}{}", p.display(), choices_suffix));
                if let Ok(choices_raw) = io.read_trimmed(&choices_path) {
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
        vec![
            "power-saver".to_string(),
            "balanced".to_string(),
            "performance".to_string(),
        ]
    }

    fn normalize_profile(raw: &str) -> String {
        match raw.to_lowercase().as_str() {
            "performance" | "custom" => "performance".to_string(),
            "low-power" | "quiet" | "cool" | "power-saver" => "power-saver".to_string(),
            _ => "balanced".to_string(),
        }
    }

    // ── Profile application ────────────────────────────────────────────────────

    /// Writes `platform_profile` / `thermal_profile`, verifies readback from sysfs
    /// so firmware rejections (`EINVAL` in UMA mode on 8D88, or silent no-ops)
    /// fail loudly instead of returning `"OK"`, and only then syncs PPD (#396, #395).
    async fn sync_omen_profile_with(&self, profile: &str, sync_to_ppd: bool) -> Result<(), String> {
        let paths = &self.sysfs_paths;
        let io = self.sysfs_io.as_ref();

        if !acpi_is_healthy_at(paths, self.sysfs_io.clone()).await {
            let msg = format!(
                "Aborted profile switch '{}': ACPI platform_profile is unresponsive (timeout)",
                profile
            );
            warn!("{}", msg);
            return Err(msg);
        }

        let is_real_sys =
            paths.acpi_platform_profile == Path::new("/sys/firmware/acpi/platform_profile");
        let mux_mode = io.read_trimmed(&paths.gpu_mux_mode).ok();
        let is_uma_mode = mux_mode.as_deref() == Some("3");

        let mut platform_found = false;
        let mut platform_ok = false;
        let mut thermal_found = false;
        let mut thermal_ok = false;
        let mut first_error: Option<String> = None;

        for p in paths.platform_profile_candidates(io) {
            if !io.exists(&p) {
                continue;
            }
            platform_found = true;

            let mut choices_raw = String::new();
            for sfx in ["_choices", "-choices"] {
                let cp = PathBuf::from(format!("{}{}", p.display(), sfx));
                if let Ok(raw) = io.read_trimmed(&cp) {
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

            let candidates: &[&str] = match profile.to_lowercase().as_str() {
                "performance" => &["performance"],
                "power-saver" => &["low-power", "quiet", "cool", "power-saver"],
                _ => &["balanced"],
            };

            let to_write = if choices.is_empty() {
                candidates[0]
            } else {
                candidates
                    .iter()
                    .find(|&&c| choices.contains(c))
                    .copied()
                    .unwrap_or_else(|| {
                        if choices.contains("balanced") {
                            "balanced"
                        } else {
                            candidates[0]
                        }
                    })
            };

            let current = io.read_trimmed(&p).ok();
            if current.as_deref() == Some(to_write) {
                info!(
                    "platform_profile='{}' already set via {} — skipping write",
                    to_write,
                    p.display()
                );
                platform_ok = true;
                continue;
            }

            let p_c = p.clone();
            let io_c = self.sysfs_io.clone();
            let val_s = to_write.to_string();
            let write_res = tokio::task::spawn_blocking(move || io_c.write_value(&p_c, &val_s))
                .await
                .unwrap_or_else(|e| Err(std::io::Error::other(e.to_string())));

            match write_res {
                Ok(()) => match io.read_trimmed(&p) {
                    Ok(readback)
                        if readback == to_write
                            || Self::normalize_profile(&readback)
                                == Self::normalize_profile(to_write) =>
                    {
                        info!(
                            "Set platform_profile='{}' via {} (verified)",
                            readback,
                            p.display()
                        );
                        platform_ok = true;
                    }
                    Ok(readback) => {
                        let msg = if is_uma_mode {
                            format!(
                                "Firmware did not apply platform_profile='{}' on {} (hardware remained '{}' in UMA/integrated-GPU mode, gpu_mux_mode=3).",
                                to_write,
                                p.display(),
                                readback
                            )
                        } else {
                            format!(
                                "Firmware ignored platform_profile='{}' on {} (readback stayed '{}').",
                                to_write,
                                p.display(),
                                readback
                            )
                        };
                        warn!("{}", msg);
                        if first_error.is_none() {
                            first_error = Some(msg);
                        }
                    }
                    Err(e) => {
                        let msg = format!(
                            "Failed to verify platform_profile readback on {}: {}",
                            p.display(),
                            e
                        );
                        warn!("{}", msg);
                        if first_error.is_none() {
                            first_error = Some(msg);
                        }
                    }
                },
                Err(e) => {
                    let remained = current.unwrap_or_else(|| "unknown".to_string());
                    let is_einval = e.raw_os_error() == Some(libc::EINVAL)
                        || e.kind() == std::io::ErrorKind::InvalidInput;
                    let msg = if is_uma_mode && is_einval {
                        format!(
                            "BIOS rejected platform_profile='{}' on {} ({}): HP firmware disables thermal profile switching in UMA/integrated-GPU mode (gpu_mux_mode=3); hardware remained '{}'. Switch GPU MUX back to Hybrid or Discrete to change power profiles.",
                            to_write,
                            p.display(),
                            e,
                            remained
                        )
                    } else {
                        format!(
                            "Failed to write platform_profile='{}' to {}: {} (hardware remained '{}')",
                            to_write,
                            p.display(),
                            e,
                            remained
                        )
                    };
                    warn!("{}", msg);
                    if first_error.is_none() {
                        first_error = Some(msg);
                    }
                }
            }
        }

        let thermal_val = if profile == "performance" { "1" } else { "0" };
        for p in &paths.thermal_profile_paths {
            if !io.exists(p) {
                continue;
            }
            thermal_found = true;
            let current = io.read_trimmed(p).ok();
            if current.as_deref() == Some(thermal_val) {
                info!(
                    "thermal_profile={} already set via {} — skipping write",
                    thermal_val,
                    p.display()
                );
                thermal_ok = true;
                continue;
            }
            let p_c = p.clone();
            let io_c = self.sysfs_io.clone();
            let val_s = thermal_val.to_string();
            let write_res = tokio::task::spawn_blocking(move || io_c.write_value(&p_c, &val_s))
                .await
                .unwrap_or_else(|e| Err(std::io::Error::other(e.to_string())));

            match write_res {
                Ok(()) => match io.read_trimmed(p) {
                    Ok(rb) if rb == thermal_val => {
                        info!("Set thermal_profile={} via {} (verified)", rb, p.display());
                        thermal_ok = true;
                    }
                    Ok(rb) => {
                        let msg = format!(
                            "thermal_profile={} ignored on {} (readback stayed '{}')",
                            thermal_val,
                            p.display(),
                            rb
                        );
                        warn!("{}", msg);
                        if first_error.is_none() {
                            first_error = Some(msg);
                        }
                    }
                    Err(e) => {
                        if first_error.is_none() {
                            first_error = Some(format!(
                                "Failed to read back thermal_profile on {}: {}",
                                p.display(),
                                e
                            ));
                        }
                    }
                },
                Err(e) => {
                    if first_error.is_none() {
                        first_error = Some(format!(
                            "Failed to write thermal_profile={} to {}: {}",
                            thermal_val,
                            p.display(),
                            e
                        ));
                    }
                }
            }
        }

        let hw_found = platform_found || thermal_found;
        let hw_succeeded = if platform_found {
            platform_ok
        } else if thermal_found {
            thermal_ok
        } else {
            false
        };

        if hw_found {
            if !hw_succeeded {
                return Err(first_error
                    .unwrap_or_else(|| format!("Hardware rejected power profile '{}'", profile)));
            }
            if is_real_sys && sync_to_ppd && !Self::has_custom_power_manager() {
                let s76_profile = match profile {
                    "performance" => "performance",
                    "power-saver" => "battery",
                    _ => "balanced",
                };
                if let Ok(mut child) = tokio::process::Command::new("system76-power")
                    .args(["profile", s76_profile])
                    .spawn()
                {
                    let _ = child.wait().await;
                }
                self.arm_ppd_suppression().await;
                let _ = Self::sync_ppd(profile).await;
            }
            return Ok(());
        }

        // Fallback when no hardware platform_profile/thermal_profile node exists:
        if is_real_sys && !Self::has_custom_power_manager() {
            if sync_to_ppd {
                self.arm_ppd_suppression().await;
                if Self::sync_ppd(profile).await {
                    return Ok(());
                }
            } else {
                return Ok(());
            }
        }

        Err(format!(
            "No OMEN thermal/platform_profile sysfs interface is available to apply '{}'",
            profile
        ))
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
        if !sysfs_exists(&tgp) {
            return;
        }
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

    pub async fn set_ac_auto_switch_enabled(&self, enabled: bool) {
        let mut cfg = self.config.lock().await;
        cfg.ac_auto_switch = enabled;
        cfg.save_to(&self.config_path);
        info!("AC/Battery auto-switch set to {}", enabled);
    }

    /// Sync CPU `energy_performance_preference` (EPP) under a given `cpufreq` sysfs root so
    /// switching out of Eco / Power Saver never leaves CPUs throttled in `power` EPP (#398).
    pub(crate) fn sync_cpu_epp_in(cpufreq_root: &Path, profile: &str) {
        let candidates: &[&str] = match profile {
            "performance" => &["performance", "balance_performance"],
            "power-saver" | "quiet" | "low-power" => &["power", "balance_power"],
            _ => &["balance_performance", "default", "balance_power"],
        };

        let Ok(entries) = std::fs::read_dir(cpufreq_root) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            let dir = entry.path();
            let Some(name) = dir.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !name.starts_with("policy") || !dir.is_dir() {
                continue;
            }
            let epp_path = dir.join("energy_performance_preference");
            if !epp_path.exists() {
                continue;
            }

            let target = {
                let avail_path = dir.join("energy_performance_available_preferences");
                if let Ok(avail_raw) = std::fs::read_to_string(&avail_path) {
                    let tokens: Vec<&str> = avail_raw.split_whitespace().collect();
                    if tokens.is_empty() {
                        candidates[0]
                    } else {
                        match candidates.iter().copied().find(|c| tokens.contains(c)) {
                            Some(found) => found,
                            None => continue,
                        }
                    }
                } else {
                    candidates[0]
                }
            };

            let already_set = std::fs::read_to_string(&epp_path)
                .ok()
                .is_some_and(|cur| cur.trim() == target);
            if !already_set {
                let _ = std::fs::write(&epp_path, target);
            }
        }
    }

    pub fn sync_cpu_epp(profile: &str) {
        Self::sync_cpu_epp_in(Path::new("/sys/devices/system/cpu/cpufreq"), profile);
    }

    /// Restore full AC performance state after AC attach: optionally switch to `target_profile`,
    /// re-sync PPD + GPU TGP/PPAB + CPU EPP, and unclamp RAPL PL1/PL2 if firmware left them low (#398).
    pub async fn restore_ac_power_state(&self, target_profile: Option<&str>) {
        if let Some(target) = target_profile {
            let _ = self.set_profile_result(target, true).await;
        }
        let active = self.current_profile().await;
        let is_real_sys = self.sysfs_paths.acpi_platform_profile
            == Path::new("/sys/firmware/acpi/platform_profile");
        if !is_real_sys {
            return;
        }

        if !Self::has_custom_power_manager() {
            self.arm_ppd_suppression().await;
            let _ = Self::sync_ppd(&active).await;
            let active_epp = active.clone();
            let _ = tokio::task::spawn_blocking(move || Self::sync_cpu_epp(&active_epp)).await;
        }
        Self::sync_gpu_power(&active).await;

        let (pl_enabled, pl1_w, pl2_w) = {
            let cfg = self.config.lock().await;
            (cfg.pl_enabled, cfg.pl1_w, cfg.pl2_w)
        };
        if pl_enabled {
            Self::apply_rapl_limits(pl1_w, pl2_w).await;
        } else if let Some((baseline, _)) = &self.rapl_baseline {
            if baseline.pl1_w >= 35 {
                if let Some(val) = sysfs_read_async(
                    "/sys/class/powercap/intel-rapl/intel-rapl:0/constraint_0_power_limit_uw",
                )
                .await
                {
                    if let Ok(uw) = val.parse::<u32>() {
                        let current_w = uw / 1_000_000;
                        if current_w < baseline.pl1_w.saturating_sub(10) {
                            info!(
                                "AC reconnect: RAPL PL1 clamped at {}W (baseline {}W); restoring baseline limits",
                                current_w, baseline.pl1_w
                            );
                            Self::apply_rapl_limits(baseline.pl1_w, baseline.pl2_w).await;
                        }
                    }
                }
            }
        }
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
        let mut active = self.detect_current_profile_with().await;

        let available_profiles =
            Self::get_available_profiles_for(&self.sysfs_paths, self.sysfs_io.as_ref()).await;
        let has_hw_profile_node = self.sysfs_paths.any_hw_node_exists(self.sysfs_io.as_ref());

        // If hardware doesn't expose a sysfs profile readback node (e.g. #297 on 8786 when
        // stock hp-wmi is loaded) or doesn't support power-saver/performance natively in PPD,
        // report the user-selected profile so the UI doesn't bounce back to "Balanced" (#293, #297).
        if !has_hw_profile_node && !cfg.power_profile.is_empty() {
            active = Self::normalize_profile(&cfg.power_profile);
        } else if active == "balanced" {
            if cfg.power_profile == "power-saver"
                && !available_profiles
                    .iter()
                    .any(|c| c == "power-saver" || c == "low-power" || c == "quiet" || c == "cool")
            {
                active = "power-saver".to_string();
            } else if cfg.power_profile == "performance"
                && !available_profiles.iter().any(|c| c == "performance")
            {
                active = "performance".to_string();
            }
        }

        let mut real_pl1 = cfg.pl1_w;
        let mut real_pl2 = cfg.pl2_w;
        if let Some(val) = sysfs_read_async(
            "/sys/class/powercap/intel-rapl/intel-rapl:0/constraint_0_power_limit_uw",
        )
        .await
        {
            if let Ok(uw) = val.parse::<u32>() {
                real_pl1 = uw / 1_000_000;
            }
        }
        if let Some(val) = sysfs_read_async(
            "/sys/class/powercap/intel-rapl/intel-rapl:0/constraint_1_power_limit_uw",
        )
        .await
        {
            if let Ok(uw) = val.parse::<u32>() {
                real_pl2 = uw / 1_000_000;
            }
        }

        let last_error = self.last_error.lock().await.clone();
        let json = serde_json::json!({
            "available": true,
            "active": active,
            "profiles": available_profiles,
            "app_profiles_enabled": cfg.app_profiles_enabled,
            "app_profiles": cfg.app_profiles,
            "active_app": null,
            "last_error": last_error,
            "undervolt_mv": cfg.undervolt_mv,
            "tcc_offset": cfg.tcc_offset,
            "pl1_w": real_pl1,
            "pl2_w": real_pl2,
            "boot_pl1_w": self.rapl_baseline.as_ref().map(|(b,_)| b.pl1_w),
            "boot_pl2_w": self.rapl_baseline.as_ref().map(|(b,_)| b.pl2_w),
            "baseline_available": self.rapl_baseline.is_some(),
            "baseline_provenance": self.rapl_baseline.as_ref().map(|(b,_)| &b.provenance),
            "baseline_persisted": self.rapl_baseline.as_ref().is_some_and(|(_,persisted)| *persisted),
            "pl_enabled": cfg.pl_enabled,
            "gpu_w": cfg.gpu_w,
            "ac_auto_switch": cfg.ac_auto_switch,
        });
        json.to_string()
    }

    async fn set_ac_auto_switch(&self, enabled: bool) -> String {
        self.set_ac_auto_switch_enabled(enabled).await;
        "OK".to_string()
    }

    /// SetPowerProfile — applies and verifies hardware profile, returning a D-Bus
    /// error (`Failed`) whenever firmware rejects or ignores the write (#396).
    async fn set_power_profile(&self, profile: String) -> zbus::fdo::Result<String> {
        match self.set_profile_result(&profile, true).await {
            Ok(()) => Ok("OK".to_string()),
            Err(err_msg) => {
                let is_tr = std::env::var("LANG").unwrap_or_default().starts_with("tr");
                let title = if is_tr {
                    "Güç Profili Değiştirilemedi"
                } else {
                    "Failed to Change Power Profile"
                };
                warn!("{}", err_msg);
                crate::notifier::DesktopNotifier::notify_error(title, &err_msg).await;
                Err(zbus::fdo::Error::Failed(err_msg))
            }
        }
    }

    /// SetPowerLimits — mirrors Python SetPowerLimits(enabled, pl1, pl2).
    async fn set_power_limits(&self, enabled: bool, pl1: i32, pl2: i32) -> String {
        // Strict range validation (allows 210W+ HX laptops such as 8E9A)
        if enabled && !(1..=250).contains(&pl1) {
            warn!("SetPowerLimits: pl1={} out of safe range [1, 250]", pl1);
            return "FAIL".to_string();
        }
        if enabled && !(1..=300).contains(&pl2) {
            warn!("SetPowerLimits: pl2={} out of safe range [1, 300]", pl2);
            return "FAIL".to_string();
        }
        let pl2 = pl2.max(pl1); // clamp: pl2 must >= pl1

        let (target_pl1, target_pl2) = if enabled {
            (pl1 as u32, pl2 as u32)
        } else if let Some((b, _)) = &self.rapl_baseline {
            (b.pl1_w, b.pl2_w)
        } else {
            return "FAIL: no observed power-limit baseline is available".to_string();
        };

        {
            let mut cfg = self.config.lock().await;
            cfg.pl_enabled = enabled;
            cfg.pl1_w = target_pl1;
            cfg.pl2_w = target_pl2;
            cfg.save_to(&self.config_path);
        }

        Self::apply_rapl_limits(target_pl1, target_pl2).await;

        info!(
            "SetPowerLimits: enabled={}, PL1={}W, PL2={}W",
            enabled, target_pl1, target_pl2
        );
        "OK".to_string()
    }

    /// SetUndervolt — mirrors Python SetUndervolt(mv).
    /// Saves to config; actual MSR write done by undervolt.rs.
    async fn set_undervolt(&self, mv: i32) -> String {
        let mv = mv.clamp(-250, 250);
        let mut cfg = self.config.lock().await;
        cfg.undervolt_mv = mv;
        cfg.save_to(&self.config_path);
        info!(
            "SetUndervolt: {}mV (saved; apply via undervolt service)",
            mv
        );
        "OK".to_string()
    }

    /// SetTccOffset — mirrors Python SetTccOffset(val).
    async fn set_tcc_offset(&self, val: i32) -> String {
        let val = val.clamp(0, 15);
        let mut cfg = self.config.lock().await;
        cfg.tcc_offset = val;
        cfg.save_to(&self.config_path);
        info!("SetTccOffset: {} (saved)", val);
        "OK".to_string()
    }

    /// SetAppProfilesEnabled — mirrors Python SetAppProfilesEnabled(enabled).
    async fn set_app_profiles_enabled(&self, enabled: bool) -> String {
        let mut cfg = self.config.lock().await;
        cfg.app_profiles_enabled = enabled;
        cfg.save_to(&self.config_path);
        info!("SetAppProfilesEnabled: {}", enabled);
        "OK".to_string()
    }

    /// SetAppProfiles — mirrors Python SetAppProfiles(profiles_json).
    async fn set_app_profiles(&self, profiles_json: String) -> String {
        match serde_json::from_str::<HashMap<String, serde_json::Value>>(&profiles_json) {
            Ok(data) => {
                let mut cfg = self.config.lock().await;
                cfg.app_profiles = data;
                cfg.save_to(&self.config_path);
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Default)]
    pub(crate) struct MockSysfsState {
        files: BTreeMap<PathBuf, String>,
        einval_paths: Vec<PathBuf>,
        silent_noop_paths: Vec<PathBuf>,
        class_dirs: Vec<PathBuf>,
        writes: Vec<(PathBuf, String)>,
    }

    #[derive(Clone, Default)]
    pub(crate) struct MockSysfsIo(Arc<std::sync::Mutex<MockSysfsState>>);

    impl MockSysfsIo {
        pub(crate) fn set_file(&self, path: impl AsRef<Path>, val: &str) {
            self.0
                .lock()
                .unwrap()
                .files
                .insert(path.as_ref().to_path_buf(), val.to_string());
        }

        fn fail_einval(&self, path: impl AsRef<Path>) {
            self.0
                .lock()
                .unwrap()
                .einval_paths
                .push(path.as_ref().to_path_buf());
        }

        fn make_silent_noop(&self, path: impl AsRef<Path>) {
            self.0
                .lock()
                .unwrap()
                .silent_noop_paths
                .push(path.as_ref().to_path_buf());
        }

        fn add_class_dir(&self, dir: impl AsRef<Path>) {
            self.0
                .lock()
                .unwrap()
                .class_dirs
                .push(dir.as_ref().to_path_buf());
        }

        fn writes(&self) -> Vec<(PathBuf, String)> {
            self.0.lock().unwrap().writes.clone()
        }
    }

    impl PowerSysfsIo for MockSysfsIo {
        fn exists(&self, path: &Path) -> bool {
            self.0.lock().unwrap().files.contains_key(path)
        }

        fn read_trimmed(&self, path: &Path) -> std::io::Result<String> {
            self.0
                .lock()
                .unwrap()
                .files
                .get(path)
                .map(|s| s.trim().to_string())
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "missing"))
        }

        fn write_value(&self, path: &Path, value: &str) -> std::io::Result<()> {
            let mut st = self.0.lock().unwrap();
            st.writes.push((path.to_path_buf(), value.to_string()));
            if st.einval_paths.iter().any(|p| p == path) {
                return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
            }
            if !st.files.contains_key(path) {
                return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "missing"));
            }
            if st.silent_noop_paths.iter().any(|p| p == path) {
                // Accepts write cleanly without changing firmware state.
                return Ok(());
            }
            st.files.insert(path.to_path_buf(), value.to_string());
            Ok(())
        }

        fn list_class_profiles(&self, _dir: &Path) -> Vec<PathBuf> {
            self.0.lock().unwrap().class_dirs.clone()
        }
    }

    pub(crate) fn mock_paths(root: &Path) -> PowerSysfsPaths {
        PowerSysfsPaths {
            acpi_platform_profile: root.join("acpi/platform_profile"),
            hp_wmi_platform_profile: root.join("hp-wmi/platform_profile"),
            hp_wmi_platform_profile_hyphen: root.join("hp-wmi/platform-profile"),
            class_platform_profile_dir: root.join("class/platform-profile"),
            thermal_profile_paths: vec![root.join("hp-wmi/thermal_profile")],
            gpu_mux_mode: root.join("hp-wmi/gpu_mux_mode"),
        }
    }

    #[tokio::test]
    async fn sim_8d88_uma_einval_fails_loudly_and_ignores_amd_pmf() {
        let dir = std::env::temp_dir().join(format!("omen-power-8d88-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cfg_path = dir.join("power.json");
        let paths = mock_paths(&dir);
        let io = MockSysfsIo::default();

        // Simulate 8D88 in UMA mode (gpu_mux_mode=3): acpi platform_profile stays balanced
        // and rejects writes with EINVAL, while amd-pmf sits on platform-profile-1.
        io.set_file(&paths.acpi_platform_profile, "balanced");
        io.set_file(
            dir.join("acpi/platform_profile_choices"),
            "low-power balanced performance",
        );
        io.fail_einval(&paths.acpi_platform_profile);
        io.set_file(&paths.gpu_mux_mode, "3");

        let amd_pmf_dir = paths.class_platform_profile_dir.join("platform-profile-1");
        io.add_class_dir(&amd_pmf_dir);
        io.set_file(amd_pmf_dir.join("name"), "amd-pmf");
        io.set_file(amd_pmf_dir.join("profile"), "low-power");

        let svc = PowerService::new_with_paths(
            cfg_path.clone(),
            paths.clone(),
            Arc::new(io.clone()),
            false,
        )
        .await
        .unwrap();

        let err = svc
            .set_profile_result("power-saver", true)
            .await
            .expect_err("EINVAL in UMA mode must fail loudly (#396)");
        assert!(err.contains("gpu_mux_mode=3"), "err was: {}", err);
        assert!(
            io.writes()
                .iter()
                .all(|(p, _)| p != &amd_pmf_dir.join("profile")),
            "amd-pmf must never be written"
        );

        let state_json: serde_json::Value =
            serde_json::from_str(&svc.get_power_profile().await).unwrap();
        assert_eq!(state_json["active"], "balanced");
        assert!(
            state_json["last_error"]
                .as_str()
                .is_some_and(|s| s.contains("UMA")),
            "GetPowerProfile must report last_error: {}",
            state_json
        );
        assert!(
            !cfg_path.exists(),
            "rejected profile must not persist to power.json"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn sim_silent_firmware_noop_is_caught_by_readback_verification() {
        let dir = std::env::temp_dir().join(format!("omen-power-noop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let paths = mock_paths(&dir);
        let io = MockSysfsIo::default();

        io.set_file(&paths.acpi_platform_profile, "balanced");
        io.set_file(
            dir.join("acpi/platform_profile_choices"),
            "low-power balanced performance",
        );
        io.make_silent_noop(&paths.acpi_platform_profile);

        let svc = PowerService::new_with_paths(dir.join("power.json"), paths, Arc::new(io), false)
            .await
            .unwrap();

        let err = svc
            .set_profile_result("performance", true)
            .await
            .expect_err("silent firmware no-op must fail readback check (#396)");
        assert!(
            err.contains("readback stayed 'balanced'") || err.contains("remained 'balanced'"),
            "unexpected msg: {}",
            err
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn sim_valid_switch_verifies_readback_and_suppresses_ppd_echo_loop() {
        let dir = std::env::temp_dir().join(format!("omen-power-ok-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cfg_path = dir.join("power.json");
        let paths = mock_paths(&dir);
        let io = MockSysfsIo::default();

        io.set_file(&paths.acpi_platform_profile, "balanced");
        io.set_file(
            dir.join("acpi/platform_profile_choices"),
            "low-power balanced performance",
        );

        let svc = PowerService::new_with_paths(
            cfg_path.clone(),
            paths.clone(),
            Arc::new(io.clone()),
            false,
        )
        .await
        .unwrap();

        // Successful switch persists and clears last_error
        svc.set_profile_result("performance", true).await.unwrap();
        assert_eq!(svc.current_profile().await, "performance");
        let state_json: serde_json::Value =
            serde_json::from_str(&svc.get_power_profile().await).unwrap();
        assert_eq!(state_json["active"], "performance");
        assert!(state_json["last_error"].is_null());

        // Arm suppression (as real sync_ppd does) -> immediate echo back to "balanced" MUST be ignored (#395)
        svc.arm_ppd_suppression().await;
        let writes_before = io.writes().len();
        assert!(svc.set_profile_from_ppd("balanced").await);
        assert_eq!(
            svc.current_profile().await,
            "performance",
            "echo during suppression window must not oscillate back to balanced"
        );
        assert_eq!(io.writes().len(), writes_before);

        // Once suppression expires, an external PPD change updates hardware without extra writes if repeated
        *svc.ppd_suppress_until.lock().await = None;
        assert!(svc.set_profile_from_ppd("power-saver").await);
        assert_eq!(svc.current_profile().await, "power-saver");
        let count_after = io.writes().len();
        assert!(svc.set_profile_from_ppd("power-saver").await);
        assert_eq!(
            io.writes().len(),
            count_after,
            "idempotent PPD signal must not re-write sysfs"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sim_epp_sync_restores_cpu_preference_out_of_power_saver() {
        let dir = std::env::temp_dir().join(format!("omen-epp-sync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let p0 = dir.join("policy0");
        let p1 = dir.join("policy1");
        std::fs::create_dir_all(&p0).unwrap();
        std::fs::create_dir_all(&p1).unwrap();

        std::fs::write(
            p0.join("energy_performance_available_preferences"),
            "default performance balance_performance balance_power power\n",
        )
        .unwrap();
        std::fs::write(p0.join("energy_performance_preference"), "power\n").unwrap();
        std::fs::write(
            p1.join("energy_performance_available_preferences"),
            "performance balance_performance power\n",
        )
        .unwrap();
        std::fs::write(p1.join("energy_performance_preference"), "power\n").unwrap();

        PowerService::sync_cpu_epp_in(&dir, "balanced");
        assert_eq!(
            std::fs::read_to_string(p0.join("energy_performance_preference")).unwrap(),
            "balance_performance"
        );
        assert_eq!(
            std::fs::read_to_string(p1.join("energy_performance_preference")).unwrap(),
            "balance_performance"
        );

        PowerService::sync_cpu_epp_in(&dir, "performance");
        assert_eq!(
            std::fs::read_to_string(p0.join("energy_performance_preference")).unwrap(),
            "performance"
        );
        assert_eq!(
            std::fs::read_to_string(p1.join("energy_performance_preference")).unwrap(),
            "performance"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
