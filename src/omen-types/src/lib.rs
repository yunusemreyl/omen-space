use serde::{Deserialize, Serialize};
use zbus::proxy;

pub mod hotkey;
pub mod i18n;

/// Sampling availability is distinct from PCI runtime power state.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuSampleState {
    Active,
    Suspended,
    Deferred,
    Unavailable,
    #[default]
    #[serde(other)]
    Unknown,
}
impl GpuSampleState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "Active (sampled)",
            Self::Suspended => "Runtime suspended",
            Self::Deferred => "Not sampled (quiet window)",
            Self::Unavailable => "Sample unavailable",
            Self::Unknown => "Sampling state unknown",
        }
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct SystemStats {
    pub cpu_temp: i32,
    pub cpu_load: f64,
    pub cpu_pwr: f64,
    pub fan_rpm: i32,
    pub fan1_rpm: i32,
    pub fan2_rpm: i32,
    pub gpu_temp: i32,
    pub gpu_load: f64,
    pub gpu_pwr: f64,
    #[serde(default)]
    pub gpu_sample_state: GpuSampleState,
    #[serde(default)]
    pub total_power_estimated: Option<bool>,
    #[serde(default)]
    pub total_power_partial: Option<bool>,
    pub ram_used_gb: f64,
    pub ram_total_gb: f64,
    pub ram_frac: f64,
    pub disk_used_gb: f64,
    pub disk_total_gb: f64,
    pub disk_frac: f64,
    pub total_pwr: f64,
    pub cpu_throttle_count: u32,
    #[serde(default)]
    pub chassis_temp: i32,
    #[serde(default)]
    pub board_verified: bool,
    /// Live usage of every internal drive (see `HardwareSpecs::drives` for the static
    /// part). Added after the aggregate `disk_*` fields above, which stay populated for
    /// clients that predate it. `#[serde(default)]` lets a newer client read an older
    /// daemon's JSON, which simply has no such list.
    #[serde(default)]
    pub drives: Vec<DriveUsage>,
    /// Live battery charge, health, cycle count, and power metrics. `None` on desktop systems
    /// or when no system battery is present. `#[serde(default)]` preserves wire compatibility.
    #[serde(default)]
    pub battery: Option<BatteryTelemetry>,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatteryStatus {
    Charging,
    Discharging,
    NotCharging,
    Full,
    #[default]
    #[serde(other)]
    Unknown,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BatteryTelemetry {
    pub name: String,
    pub status: BatteryStatus,
    pub charge_pct: u8,
    pub health_pct: Option<f64>,
    pub energy_now_wh: Option<f64>,
    pub energy_full_wh: Option<f64>,
    pub energy_design_wh: Option<f64>,
    pub power_w: f64,
    pub voltage_v: Option<f64>,
    pub cycle_count: Option<u32>,
    pub ac_online: bool,
    pub charge_limit_pct: Option<u8>,
}

/// What kind of internal storage device a drive is. Unknown values sent by a newer daemon
/// deserialize to `Other` instead of failing the whole payload.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DriveKind {
    Nvme,
    Ssd,
    Hdd,
    Emmc,
    #[default]
    #[serde(other)]
    Other,
}

impl DriveKind {
    /// Short technical name for the UI. These are acronyms and are not translated.
    pub fn label(self) -> &'static str {
        match self {
            DriveKind::Nvme => "NVMe",
            DriveKind::Ssd => "SSD",
            DriveKind::Hdd => "HDD",
            DriveKind::Emmc => "eMMC",
            DriveKind::Other => "DRIVE",
        }
    }
}

/// Static description of one internal storage device (NVMe / SATA SSD / HDD / eMMC).
#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DriveInfo {
    /// Kernel block device name, e.g. `nvme0n1`. Also the key that ties this entry to its
    /// `DriveUsage` in the telemetry stream.
    pub name: String,
    /// Vendor model string with the kernel's space padding removed.
    pub model: String,
    pub kind: DriveKind,
    /// Raw capacity in bytes (`/sys/block/<name>/size` x 512).
    pub size_bytes: u64,
}

/// Live usage of one internal drive, summed over its mounted filesystems (same numbers as `df`:
/// used = blocks - free blocks). A drive with nothing mounted reports `total_bytes == 0`.
#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DriveUsage {
    pub name: String,
    pub used_bytes: u64,
    pub total_bytes: u64,
    /// Where this drive's filesystems are mounted (for a tooltip), e.g. `["/", "/boot"]`.
    pub mounts: Vec<String>,
    /// None for older daemons; false means at least one mounted filesystem was unreadable.
    pub usage_complete: Option<bool>,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
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
    #[serde(default)]
    pub board_name: String,
    /// Primary battery description, e.g. "Primary Li-ion (55.6 Wh · 100% Health)". Empty on desktops.
    #[serde(default)]
    pub battery_spec: String,
    /// Every internal drive. Empty when talking to a daemon that predates this field,
    /// in which case clients fall back to the single `ssd_spec` line.
    #[serde(default)]
    pub drives: Vec<DriveInfo>,
}

// ── Power Service Proxy ────────────────────────────────────────────────────────
#[proxy(
    interface = "org.hp.omen.Power",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Power"
)]
pub trait Power {
    async fn set_power_profile(&self, profile: &str) -> zbus::Result<String>;
    async fn get_power_profile(&self) -> zbus::Result<String>;
    async fn set_power_limits(&self, enabled: bool, pl1: i32, pl2: i32) -> zbus::Result<String>;
    async fn set_undervolt(&self, mv: i32) -> zbus::Result<String>;
    async fn set_tcc_offset(&self, val: i32) -> zbus::Result<String>;
    async fn set_app_profiles_enabled(&self, enabled: bool) -> zbus::Result<String>;
    async fn set_app_profiles(&self, profiles_json: &str) -> zbus::Result<String>;
    async fn set_ac_auto_switch(&self, enabled: bool) -> zbus::Result<String>;
    async fn ping(&self) -> zbus::Result<String>;
}

// ── Fan Service Proxy ─────────────────────────────────────────────────────────
#[proxy(
    interface = "org.hp.omen.Fan",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Fan"
)]
pub trait Fan {
    async fn set_fan_mode(&self, mode: &str) -> zbus::Result<String>;
    async fn set_fan_target(&self, fan_num: u32, rpm: u32) -> zbus::Result<String>;
    async fn get_fan_mode(&self) -> zbus::Result<String>;
    async fn get_fan_info(&self) -> zbus::Result<String>;
    async fn save_custom_curve(&self, curve_json: &str) -> zbus::Result<String>;
    async fn save_custom_gpu_curve(&self, curve_json: &str) -> zbus::Result<String>;
    async fn set_thermal_protection(&self, enabled: bool) -> zbus::Result<String>;
    async fn ping(&self) -> zbus::Result<String>;

    #[zbus(signal)]
    fn thermal_protection_alert(&self, active: bool) -> zbus::Result<()>;
}

// ── SysMon Service Proxy ──────────────────────────────────────────────────────
#[proxy(
    interface = "org.hp.omen.SysMon",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/SysMon"
)]
pub trait SysMon {
    async fn get_diagnostics(&self) -> zbus::Result<String>;
    async fn get_hardware_specs(&self) -> zbus::Result<String>;
    async fn generate_diagnostic_report(&self) -> zbus::Result<String>;
    async fn generate_rgb_issue(&self) -> zbus::Result<String>;

    #[zbus(signal)]
    fn telemetry_updated(&self, json_stats: &str) -> zbus::Result<()>;
}

// ── Rgb Service Proxy ─────────────────────────────────────────────────────────
#[proxy(
    interface = "org.hp.omen.Rgb",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Rgb"
)]
pub trait Rgb {
    async fn set_color(&self, zone_val: i32, hex_color: &str) -> zbus::Result<String>;
    async fn set_mode(&self, mode_str: &str, speed_val: i32) -> zbus::Result<String>;
    async fn set_global(
        &self,
        power_val: bool,
        brightness_val: i32,
        direction_str: &str,
    ) -> zbus::Result<String>;
    async fn set_lightbar_color(&self, zone_val: i32, hex_color: &str) -> zbus::Result<String>;
    async fn set_lightbar_global(
        &self,
        power_val: bool,
        brightness_val: i32,
        mode_str: &str,
        speed_val: i32,
    ) -> zbus::Result<String>;
    async fn get_state(&self) -> zbus::Result<String>;
    async fn set_zone_count_override(&self, zone_count: i32) -> zbus::Result<String>;
    async fn set_per_key_colors(&self, colors_json: &str) -> zbus::Result<String>;
    async fn set_hw_effects(&self, enabled: bool) -> zbus::Result<String>;
    async fn set_win_lock(&self, locked: bool) -> zbus::Result<String>;
    async fn start_lamp_calibration(&self) -> zbus::Result<String>;
    async fn stop_lamp_calibration(&self) -> zbus::Result<String>;
    async fn get_lamp_map(&self) -> zbus::Result<String>;
    async fn set_lamp_colors(&self, colors_json: &str) -> zbus::Result<String>;
    async fn test_single_key(&self, index: i32) -> zbus::Result<String>;
    async fn start_per_key_wizard(&self) -> zbus::Result<String>;
    async fn light_key_index(&self, index: u32, hex_color: &str) -> zbus::Result<String>;
    async fn record_key_mapping(&self, index: u32, key_name: &str) -> zbus::Result<String>;
    async fn export_keymap_report(&self) -> zbus::Result<String>;
    async fn save_per_key_map(&self, map_json: &str) -> zbus::Result<String>;
    async fn ping(&self) -> zbus::Result<String>;
}

// ── Platform Service Proxy ───────────────────────────────────────────────────
#[proxy(
    interface = "org.hp.omen.Platform",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Platform"
)]
pub trait Platform {
    async fn get_system_info(&self) -> zbus::Result<String>;
    async fn get_state(&self) -> zbus::Result<String>;
    async fn set_keyboard_fixes(&self, prtsc: bool, f1: bool) -> zbus::Result<String>;
    async fn set_battery_care(&self, limit: u32) -> zbus::Result<String>;
    async fn set_battery_limit_config(
        &self,
        enabled: bool,
        stop: u32,
        start: u32,
        off_hold: bool,
    ) -> zbus::Result<String>;
    async fn set_battery_bypass(&self, enabled: bool) -> zbus::Result<String>;
    async fn get_battery_limit_state(&self) -> zbus::Result<String>;
    async fn clean_memory(&self) -> zbus::Result<String>;
    async fn generate_hardware_dump(&self) -> zbus::Result<String>;
    async fn get_hardware_dump_json(&self) -> zbus::Result<String>;
    async fn run_wmi_diagnostics(&self) -> zbus::Result<String>;
    async fn run_fan_cleaning(&self) -> zbus::Result<String>;
    async fn cancel_fan_cleaning(&self) -> zbus::Result<String>;
    async fn check_conflicts(&self) -> zbus::Result<String>;
    async fn analyze_acpi(&self) -> zbus::Result<String>;
    async fn generate_triage_bundle(&self) -> zbus::Result<String>;
    async fn check_bios_update(&self) -> zbus::Result<String>;
    async fn check_app_update(&self) -> zbus::Result<String>;
    async fn apply_app_update(&self) -> zbus::Result<String>;
    async fn toggle_overlay(&self) -> zbus::Result<String>;
    async fn ping(&self) -> zbus::Result<String>;
    #[zbus(signal)]
    fn macro_key_pressed(&self, key_name: &str) -> zbus::Result<()>;
}

// ── Mux Service Proxy ────────────────────────────────────────────────────────
#[proxy(
    interface = "org.hp.omen.Mux",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Mux"
)]
pub trait Mux {
    async fn set_gpu_mode(&self, mode: &str) -> zbus::Result<String>;
    async fn get_gpu_info(&self) -> zbus::Result<String>;
    async fn set_mux_backend(&self, backend: &str) -> zbus::Result<String>;
    async fn ping(&self) -> zbus::Result<String>;
}

// ── Undervolt Service Proxy ──────────────────────────────────────────────────
#[proxy(
    interface = "org.hp.omen.Undervolt",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Undervolt"
)]
pub trait Undervolt {
    async fn set_offset(&self, plane: &str, offset_mv: i32) -> zbus::Result<String>;
    async fn set_tcc_offset(&self, val: i32) -> zbus::Result<String>;
    async fn get_state(&self) -> zbus::Result<String>;
    async fn read_offsets(&self) -> zbus::Result<String>;
    async fn ping(&self) -> zbus::Result<String>;
    async fn confirm(&self) -> zbus::Result<String>;
}

// ── AppProfiles Service Proxy ──────────────────────────────────────────────────
#[proxy(
    interface = "org.hp.omen.AppProfiles",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/AppProfiles"
)]
pub trait AppProfiles {
    async fn get_profiles(&self) -> zbus::Result<String>;
    async fn add_profile(
        &self,
        process_name: &str,
        power_profile: &str,
        fan_mode: &str,
    ) -> zbus::Result<String>;
    async fn remove_profile(&self, process_name: &str) -> zbus::Result<String>;
}

// ── Graphical Session Environment Auto-Repair ─────────────────────────────────

#[cfg(unix)]
fn connect_unix_nonblocking(
    path: &std::path::Path,
) -> std::result::Result<libc::c_int, libc::c_int> {
    use std::os::unix::ffi::OsStrExt;
    let bytes = path.as_os_str().as_bytes();
    if bytes.is_empty() || bytes.contains(&0) {
        return Err(libc::EINVAL);
    }

    // SAFETY: sockaddr_un is POD and zero-initialized below.
    let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if bytes.len() >= addr.sun_path.len() {
        return Err(libc::ENAMETOOLONG);
    }
    addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (dst, &b) in addr.sun_path.iter_mut().zip(bytes.iter()) {
        *dst = b as libc::c_char;
    }

    // SAFETY: AF_UNIX non-blocking stream socket closed on every error path.
    let fd = unsafe {
        libc::socket(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
            0,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::EINVAL));
    }

    let addr_len =
        (std::mem::offset_of!(libc::sockaddr_un, sun_path) + bytes.len() + 1) as libc::socklen_t;
    let rc = unsafe {
        libc::connect(
            fd,
            (&addr as *const libc::sockaddr_un).cast::<libc::sockaddr>(),
            addr_len,
        )
    };
    if rc == 0 {
        Ok(fd)
    } else {
        let err = std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::ECONNREFUSED);
        unsafe {
            libc::close(fd);
        }
        Err(err)
    }
}

/// Non-blocking AF_UNIX liveness probe.
/// Must NEVER use blocking `UnixStream::connect`: when a compositor socket (e.g. KWin's
/// `/tmp/.X11-unix/X1` opened with `listen(backlog = 1)`) has a full accept queue,
/// a blocking connect sleeps forever in kernel `unix_wait_for_peer`.
#[cfg(unix)]
fn is_unix_socket_alive(path: &std::path::Path) -> bool {
    match connect_unix_nonblocking(path) {
        Ok(fd) => {
            unsafe {
                libc::close(fd);
            }
            true
        }
        Err(_) => false,
    }
}

/// Repairs `XDG_RUNTIME_DIR`, `DBUS_SESSION_BUS_ADDRESS`, `WAYLAND_DISPLAY`,
/// `XAUTHORITY`, and `DISPLAY` when a process (`omen-tray`, `omen-overlay`, or
/// `omen-gui`) is launched from `su`/`sudo`/`pkexec` or across desktop session
/// restarts where stale sockets (e.g. a dead `wayland-0` alongside a live
/// `wayland-1`) exist in `/run/user/<uid>`.
pub fn repair_graphical_session_env() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        use std::path::{Path, PathBuf};

        let uid = match std::fs::metadata("/proc/self") {
            Ok(m) => m.uid(),
            Err(_) => return,
        };
        if uid == 0 {
            return;
        }

        let default_runtime = format!("/run/user/{}", uid);
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .ok()
            .filter(|s| !s.is_empty() && Path::new(s).is_dir())
            .unwrap_or_else(|| {
                if Path::new(&default_runtime).is_dir() {
                    unsafe {
                        std::env::set_var("XDG_RUNTIME_DIR", &default_runtime);
                    }
                }
                default_runtime
            });

        // 1. Session D-Bus socket
        let has_dbus = std::env::var("DBUS_SESSION_BUS_ADDRESS")
            .ok()
            .is_some_and(|s| !s.trim().is_empty());
        if !has_dbus {
            let bus_socket = format!("{}/bus", runtime_dir);
            if Path::new(&bus_socket).exists() {
                unsafe {
                    std::env::set_var(
                        "DBUS_SESSION_BUS_ADDRESS",
                        format!("unix:path={}", bus_socket),
                    );
                }
            }
        }

        // 2. Wayland display socket: verify current WAYLAND_DISPLAY accepts a connection;
        // if unset or stale (ECONNREFUSED), pick the newest live wayland-* socket.
        let current_wayland_ok = std::env::var("WAYLAND_DISPLAY").ok().is_some_and(|wd| {
            let trimmed = wd.trim();
            if trimmed.is_empty() {
                return false;
            }
            let sock_path = if trimmed.starts_with('/') {
                PathBuf::from(trimmed)
            } else {
                Path::new(&runtime_dir).join(trimmed)
            };
            is_unix_socket_alive(&sock_path)
        });

        if !current_wayland_ok {
            let mut candidates: Vec<(std::time::SystemTime, String, PathBuf)> = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&runtime_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with("wayland-") && !name.ends_with(".lock") {
                        let path = entry.path();
                        let mtime = entry
                            .metadata()
                            .and_then(|m| m.modified())
                            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                        candidates.push((mtime, name, path));
                    }
                }
            }
            candidates.sort_by_key(|a| std::cmp::Reverse(a.0));

            let mut found_live = false;
            for (_mtime, name, path) in candidates {
                if is_unix_socket_alive(&path) {
                    unsafe {
                        std::env::set_var("WAYLAND_DISPLAY", &name);
                    }
                    found_live = true;
                    break;
                }
            }
            if !found_live {
                unsafe {
                    std::env::remove_var("WAYLAND_DISPLAY");
                }
            }
        }

        // 3. XAUTHORITY: if unset or pointing to a missing file, find the newest active
        // session auth file in /run/user/<uid> (xauth_* on KDE/SDDM, .mutter-Xwaylandauth.*
        // on GNOME) or fall back to ~/.Xauthority.
        let current_xauth_ok = std::env::var("XAUTHORITY")
            .ok()
            .is_some_and(|p| !p.trim().is_empty() && Path::new(p.trim()).is_file());
        if !current_xauth_ok {
            let mut auth_candidates: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&runtime_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with("xauth_") || name.starts_with(".mutter-Xwaylandauth.") {
                        let path = entry.path();
                        if path.is_file() {
                            let mtime = entry
                                .metadata()
                                .and_then(|m| m.modified())
                                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                            auth_candidates.push((mtime, path));
                        }
                    }
                }
            }
            auth_candidates.sort_by_key(|a| std::cmp::Reverse(a.0));
            if let Some((_, newest_auth)) = auth_candidates.first() {
                unsafe {
                    std::env::set_var("XAUTHORITY", newest_auth);
                }
            } else if let Ok(home) = std::env::var("HOME") {
                let fallback = Path::new(&home).join(".Xauthority");
                if fallback.is_file() {
                    unsafe {
                        std::env::set_var("XAUTHORITY", fallback);
                    }
                }
            }
        }

        // 4. X11 DISPLAY: verify current DISPLAY socket in /tmp/.X11-unix/X<N> is alive.
        let current_display_ok = std::env::var("DISPLAY").ok().is_some_and(|disp| {
            let num = disp
                .trim()
                .trim_start_matches(':')
                .split('.')
                .next()
                .unwrap_or("");
            !num.is_empty()
                && num.chars().all(|c| c.is_ascii_digit())
                && is_unix_socket_alive(Path::new(&format!("/tmp/.X11-unix/X{}", num)))
        });

        if !current_display_ok {
            let mut x_candidates: Vec<(u32, String, PathBuf)> = Vec::new();
            if let Ok(entries) = std::fs::read_dir("/tmp/.X11-unix") {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if let Some(num) = name.strip_prefix('X')
                        && !num.is_empty()
                        && num.chars().all(|c| c.is_ascii_digit())
                        && let Ok(num_val) = num.parse::<u32>()
                    {
                        x_candidates.push((num_val, format!(":{}", num), entry.path()));
                    }
                }
            }
            // Prefer the primary lowest-numbered X display (:0) over secondary displays (:1).
            x_candidates.sort_by_key(|a| a.0);
            let mut found_x11 = false;
            for (_num, disp, path) in x_candidates {
                if is_unix_socket_alive(&path) {
                    unsafe {
                        std::env::set_var("DISPLAY", disp);
                    }
                    found_x11 = true;
                    break;
                }
            }
            if !found_x11 {
                unsafe {
                    std::env::remove_var("DISPLAY");
                }
            }
        }
    }
}

/// GitHub commit endpoints must supply a complete object ID, not an error payload.
pub fn is_commit_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedSemver<'a> {
    core: Vec<u32>,
    pre: Option<&'a str>,
}

fn parse_semver(raw: &str) -> Option<ParsedSemver<'_>> {
    let trimmed = raw.trim();
    let no_v = trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed);
    let without_build = no_v.split('+').next().unwrap_or("");
    let (core_str, pre) = match without_build.split_once('-') {
        Some((c, p)) => {
            if p.is_empty() {
                return None;
            }
            (c, Some(p))
        }
        None => (without_build, None),
    };
    if core_str.is_empty() {
        return None;
    }
    let mut core = Vec::new();
    for seg in core_str.split('.') {
        if seg.is_empty() || !seg.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        core.push(seg.parse::<u32>().ok()?);
    }
    Some(ParsedSemver { core, pre })
}

fn compare_prerelease(a: &str, b: &str) -> std::cmp::Ordering {
    let mut ai = a.split('.');
    let mut bi = b.split('.');
    loop {
        match (ai.next(), bi.next()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(sa), Some(sb)) => {
                let ord = match (sa.parse::<u64>(), sb.parse::<u64>()) {
                    (Ok(na), Ok(nb)) => na.cmp(&nb),
                    (Ok(_), Err(_)) => std::cmp::Ordering::Less,
                    (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
                    (Err(_), Err(_)) => sa.cmp(sb),
                };
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
            }
        }
    }
}

/// Returns `true` if `remote` represents a strictly newer semantic version than `current`.
pub fn is_newer_semver(current: &str, remote: &str) -> bool {
    let (Some(c), Some(r)) = (parse_semver(current), parse_semver(remote)) else {
        return false;
    };
    let len = c.core.len().max(r.core.len());
    for i in 0..len {
        let rv = *r.core.get(i).unwrap_or(&0);
        let cv = *c.core.get(i).unwrap_or(&0);
        if rv > cv {
            return true;
        }
        if cv > rv {
            return false;
        }
    }
    match (c.pre, r.pre) {
        (Some(_), None) => true,
        (None, Some(_)) => false,
        (None, None) => false,
        (Some(cp), Some(rp)) => compare_prerelease(rp, cp) == std::cmp::Ordering::Greater,
    }
}

#[cfg(test)]
mod semver_tests {
    use super::is_newer_semver;

    #[test]
    fn handles_equal_and_patch_minor_major_bumps() {
        assert!(!is_newer_semver("2.1.8", "2.1.8"));
        assert!(!is_newer_semver("2.1.8", "2.1.7"));
        assert!(is_newer_semver("2.1.7", "2.1.8"));
        assert!(is_newer_semver("2.1.8", "2.2.0"));
        assert!(is_newer_semver("2.9.9", "3.0.0"));
        assert!(is_newer_semver("2.1", "2.1.1"));
        assert!(!is_newer_semver("2.1.0", "2.1"));
    }

    #[test]
    fn handles_v_prefix_and_prerelease_suffixes() {
        assert!(is_newer_semver("2.1.7", "v2.1.8"));
        assert!(is_newer_semver("v2.1.7", "2.1.8"));
        assert!(!is_newer_semver("2.1.8", "v2.1.7"));
        assert!(!is_newer_semver("2.1.8", "v2.1.8"));

        assert!(!is_newer_semver("2.5.2", "2.5.2-rc1"));
        assert!(!is_newer_semver("2.5.2", "2.5.2-beta"));
        assert!(is_newer_semver("2.5.2-rc1", "2.5.2"));
        assert!(is_newer_semver("2.5.2", "2.5.3-rc1"));
        assert!(is_newer_semver("2.5.2-beta.1", "2.5.2-rc.1"));
        assert!(!is_newer_semver("2.5.2-rc.1", "2.5.2-beta.1"));
    }

    #[test]
    fn rejects_invalid_version_strings() {
        for invalid in ["", " ", "unknown", "v", "2.5.", "2.5.x", "1.2.3-"] {
            assert!(!is_newer_semver("2.5.2", invalid), "remote={invalid}");
            assert!(!is_newer_semver(invalid, "2.5.3"), "current={invalid}");
        }
    }
}

#[cfg(test)]
mod update_metadata_tests {
    #[test]
    fn reject_missing_short_and_nonhex_commit_ids() {
        assert!(super::is_commit_sha(
            "6a5adaed55a6f74b14a489ec31d355727ac45c22"
        ));
        for invalid in [
            "",
            "unknown",
            "6a5adae",
            "API rate limit exceeded",
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
        ] {
            assert!(!super::is_commit_sha(invalid));
        }
    }
}

fn extract_commit_fields(v: &serde_json::Value) -> Result<(String, String), String> {
    let sha = v["sha"]
        .as_str()
        .filter(|s| is_commit_sha(s))
        .ok_or("Missing or invalid upstream commit SHA")?;
    Ok((
        sha.to_string(),
        v["commit"]["message"].as_str().unwrap_or("").to_string(),
    ))
}

/// Require a valid GitHub commit object (or non-empty commit list array) rather than
/// accepting rate-limit/error JSON.
pub fn parse_commit_metadata(raw: &str) -> Result<(String, String), String> {
    let v: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if let Some(arr) = v.as_array() {
        let first = arr.first().ok_or("Empty upstream commit list")?;
        return extract_commit_fields(first);
    }
    extract_commit_fields(&v)
}

pub type CommitsAhead = (u32, Vec<(String, String)>);

/// Compute how many commits `main` is ahead of `installed_sha` from the lightweight
/// `/commits?sha=main&per_page=N` array without downloading large file diffs.
/// Returns `Ok(Some((ahead_count, recent_commits)))` when `installed_sha` is found
/// in the page, or `Ok(None)` when older than the fetched page.
pub fn parse_commits_list_ahead(
    raw: &str,
    installed_sha: &str,
) -> Result<Option<CommitsAhead>, String> {
    let v: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    let arr = v
        .as_array()
        .ok_or("Expected JSON array from commits list")?;
    let first = arr.first().ok_or("Empty upstream commit list")?;
    let _ = extract_commit_fields(first)?;

    let needle = installed_sha.trim();
    let needle_is_hex =
        (7..=40).contains(&needle.len()) && needle.bytes().all(|b| b.is_ascii_hexdigit());
    if !needle_is_hex {
        return Ok(None);
    }
    let needle_lower = needle.to_ascii_lowercase();

    let mut ahead_items = Vec::new();
    for (idx, item) in arr.iter().enumerate() {
        let Some(sha) = item["sha"].as_str().filter(|s| is_commit_sha(s)) else {
            continue;
        };
        let sha_lower = sha.to_ascii_lowercase();
        let matches = if needle_lower.len() == 40 {
            sha_lower == needle_lower
        } else {
            sha_lower.starts_with(&needle_lower)
        };
        if matches {
            return Ok(Some((idx as u32, ahead_items)));
        }
        let short: String = sha.chars().take(7).collect();
        let msg = item["commit"]["message"]
            .as_str()
            .unwrap_or("")
            .lines()
            .next()
            .unwrap_or("")
            .to_string();
        ahead_items.push((short, msg));
    }

    Ok(None)
}
/// Compare API errors must never become a zero-commits-behind success.
pub fn parse_comparison_ahead(raw: &str) -> Result<u32, String> {
    let v: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if !matches!(
        v["status"].as_str(),
        Some("ahead" | "behind" | "diverged" | "identical")
    ) {
        return Err("Missing or invalid comparison status".into());
    }
    u32::try_from(v["ahead_by"].as_u64().ok_or("Missing comparison count")?)
        .map_err(|e| e.to_string())
}
#[cfg(test)]
mod github_response_tests {
    use super::*;
    #[test]
    fn api_error_objects_never_report_up_to_date() {
        for raw in [
            r#"{"message":"API rate limit exceeded"}"#,
            r#"{"sha":""}"#,
            r#"{"status":"ahead"}"#,
            "not json",
        ] {
            assert!(parse_commit_metadata(raw).is_err());
            assert!(parse_comparison_ahead(raw).is_err());
        }
        assert!(
            parse_commit_metadata(r#"{"sha":"6a5adaed55a6f74b14a489ec31d355727ac45c22"}"#).is_ok()
        );
        assert_eq!(
            parse_comparison_ahead(r#"{"status":"identical","ahead_by":0}"#).unwrap(),
            0
        );
        assert_eq!(
            parse_comparison_ahead(r#"{"status":"ahead","ahead_by":2}"#).unwrap(),
            2
        );
    }

    #[test]
    fn commit_list_endpoint_parses_head_and_computes_ahead_count_locally() {
        let head = "6a5adaed55a6f74b14a489ec31d355727ac45c22";
        let mid = "1111111111111111111111111111111111111111";
        let old = "2222222222222222222222222222222222222222";
        let arr = serde_json::json!([
            {"sha": head, "commit": {"message": "release: v2.5.4\n\nbody"}},
            {"sha": mid, "commit": {"message": "fix: middle commit"}},
            {"sha": old, "commit": {"message": "release: v2.5.3"}}
        ])
        .to_string();

        let (sha, msg) = parse_commit_metadata(&arr).unwrap();
        assert_eq!(sha, head);
        assert!(msg.starts_with("release: v2.5.4"));

        assert_eq!(
            parse_commits_list_ahead(&arr, head).unwrap(),
            Some((0, vec![]))
        );
        assert_eq!(
            parse_commits_list_ahead(&arr, old).unwrap(),
            Some((
                2,
                vec![
                    ("6a5adae".to_string(), "release: v2.5.4".to_string()),
                    ("1111111".to_string(), "fix: middle commit".to_string()),
                ]
            ))
        );
        // Short 7-char prefix match (case-insensitive)
        assert_eq!(
            parse_commits_list_ahead(&arr, "1111111").unwrap(),
            Some((
                1,
                vec![("6a5adae".to_string(), "release: v2.5.4".to_string())]
            ))
        );
        // Installed SHA older than the 30-commit window returns Ok(None) so caller falls back to compare
        assert_eq!(
            parse_commits_list_ahead(&arr, "3333333333333333333333333333333333333333").unwrap(),
            None
        );

        assert!(parse_commit_metadata("[]").is_err());
        assert!(parse_commits_list_ahead("[]", head).is_err());
        assert!(
            parse_commits_list_ahead(r#"{"message":"API rate limit exceeded"}"#, head).is_err()
        );
    }
}

#[cfg(all(test, unix))]
mod unix_socket_probe_tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;
    use std::time::{Duration, Instant};

    #[test]
    fn nonblocking_socket_probe_never_hangs_on_full_backlog() {
        let sock_path = std::env::temp_dir().join(format!(
            "omen-types-sock-probe-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&sock_path);

        let srv_fd =
            unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
        assert!(srv_fd >= 0);

        let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        let bytes = sock_path.as_os_str().as_bytes();
        assert!(bytes.len() < addr.sun_path.len());
        addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
        for (dst, &b) in addr.sun_path.iter_mut().zip(bytes.iter()) {
            *dst = b as libc::c_char;
        }
        let addr_len = (std::mem::offset_of!(libc::sockaddr_un, sun_path) + bytes.len() + 1)
            as libc::socklen_t;

        let bound =
            unsafe { libc::bind(srv_fd, (&addr as *const libc::sockaddr_un).cast(), addr_len) };
        assert_eq!(bound, 0, "bind failed: {}", std::io::Error::last_os_error());
        assert_eq!(unsafe { libc::listen(srv_fd, 1) }, 0);

        // Fresh socket with room in accept queue reports alive quickly.
        assert!(is_unix_socket_alive(&sock_path));

        // Fill listen(1) backlog without calling accept(), reproducing KWin's stuck X1 socket.
        let mut clients = Vec::new();
        for _ in 0..16 {
            match connect_unix_nonblocking(&sock_path) {
                Ok(fd) => clients.push(fd),
                Err(_) => break,
            }
        }
        assert!(
            !clients.is_empty(),
            "expected at least one client in backlog"
        );

        // Probe on full backlog must return false immediately instead of hanging in unix_wait_for_peer.
        let t0 = Instant::now();
        let alive = is_unix_socket_alive(&sock_path);
        let elapsed = t0.elapsed();
        assert!(
            elapsed < Duration::from_millis(200),
            "probe took too long: {elapsed:?}"
        );
        assert!(!alive, "full backlog should return false immediately");

        for fd in clients {
            unsafe {
                libc::close(fd);
            }
        }
        unsafe {
            libc::close(srv_fd);
        }
        let _ = std::fs::remove_file(&sock_path);

        assert!(!is_unix_socket_alive(std::path::Path::new(
            "/tmp/omen-nonexistent-sock-99999"
        )));
    }
}
