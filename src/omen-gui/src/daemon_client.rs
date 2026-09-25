use zbus::proxy;
use tokio::runtime::Runtime;
use std::sync::OnceLock;
use serde::{Serialize, Deserialize};

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
    pub ram_used_gb: f64,
    pub ram_total_gb: f64,
    pub ram_frac: f64,
    pub disk_used_gb: f64,
    pub disk_total_gb: f64,
    pub disk_frac: f64,
    pub total_pwr: f64,
    pub cpu_throttle_count: u32,
    /// Chassis / IR sensor (WMI 0x23) in °C. 0 means not available.
    #[serde(default)]
    pub chassis_temp: i32,
    /// True when this board ID is in the community-verified list.
    #[serde(default)]
    pub board_verified: bool,
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

// ── Power Service Proxy ────────────────────────────────────────────────────────

#[proxy(
    interface = "org.hp.omen.Power",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Power"
)]
trait Power {
    async fn set_power_profile(&self, profile: &str) -> zbus::Result<String>;
    async fn get_power_profile(&self) -> zbus::Result<String>;
    async fn set_power_limits(&self, enabled: bool, pl1: i32, pl2: i32) -> zbus::Result<String>;
    async fn set_app_profiles_enabled(&self, enabled: bool) -> zbus::Result<String>;
}

// ── Fan Service Proxy ─────────────────────────────────────────────────────────

#[proxy(
    interface = "org.hp.omen.Fan",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Fan"
)]
trait Fan {
    async fn set_fan_mode(&self, mode: &str) -> zbus::Result<String>;
    async fn get_fan_mode(&self) -> zbus::Result<String>;
    async fn get_fan_info(&self) -> zbus::Result<String>;
    async fn save_custom_curve(&self, curve_json: &str) -> zbus::Result<String>;
    async fn set_thermal_protection(&self, enabled: bool) -> zbus::Result<String>;
}

// ── SysMon Service Proxy ──────────────────────────────────────────────────────

#[proxy(
    interface = "org.hp.omen.SysMon",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/SysMon"
)]
trait SysMon {
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
trait Rgb {
    async fn set_color(&self, zone_val: i32, hex_color: &str) -> zbus::Result<String>;
    async fn set_mode(&self, mode_str: &str, speed_val: i32) -> zbus::Result<String>;
    async fn set_global(&self, power_val: bool, brightness_val: i32, direction_str: &str) -> zbus::Result<String>;
    async fn get_state(&self) -> zbus::Result<String>;
    async fn set_per_key_colors(&self, colors_json: &str) -> zbus::Result<String>;
    async fn start_per_key_wizard(&self) -> zbus::Result<String>;
    async fn light_key_index(&self, index: u32, hex_color: &str) -> zbus::Result<String>;
    async fn record_key_mapping(&self, index: u32, key_name: &str) -> zbus::Result<String>;
    async fn export_keymap_report(&self) -> zbus::Result<String>;
}

// ── Platform Service Proxy ───────────────────────────────────────────────────

#[proxy(
    interface = "org.hp.omen.Platform",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Platform"
)]
trait Platform {
    async fn set_battery_care(&self, limit: u32) -> zbus::Result<String>;
    async fn run_fan_cleaning(&self) -> zbus::Result<String>;
    async fn toggle_overlay(&self) -> zbus::Result<String>;
}

// ── Mux Service Proxy ────────────────────────────────────────────────────────

#[proxy(
    interface = "org.hp.omen.Mux",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Mux"
)]
trait Mux {
    async fn set_gpu_mode(&self, mode: &str) -> zbus::Result<String>;
    async fn get_gpu_info(&self) -> zbus::Result<String>;
}

// ── Undervolt Service Proxy ──────────────────────────────────────────────────

#[proxy(
    interface = "org.hp.omen.Undervolt",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Undervolt"
)]
trait Undervolt {
    async fn set_offset(&self, plane: &str, offset_mv: i32) -> zbus::Result<String>;
    async fn set_tcc_offset(&self, val: i32) -> zbus::Result<String>;
    async fn get_state(&self) -> zbus::Result<String>;
}

// ── AppProfiles Service Proxy ──────────────────────────────────────────────────

#[proxy(
    interface = "org.hp.omen.AppProfiles",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/AppProfiles"
)]
trait AppProfiles {
    async fn get_profiles(&self) -> zbus::Result<String>;
    async fn add_profile(&self, process_name: &str, power_profile: &str, fan_mode: &str) -> zbus::Result<String>;
    async fn remove_profile(&self, process_name: &str) -> zbus::Result<String>;
}

// ── Runtime Management ───────────────────────────────────────────────────────

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

pub fn get_runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("Failed to build tokio runtime for D-Bus client")
    })
}

// ── Helper ───────────────────────────────────────────────────────────────────

async fn get_conn() -> Result<zbus::Connection, zbus::Error> {
    match zbus::Connection::system().await {
        Ok(c) => Ok(c),
        Err(_) => zbus::Connection::session().await,
    }
}

// ── Desktop Notification Helper ──────────────────────────────────────────────

/// Send a desktop notification via org.freedesktop.Notifications.
/// urgency: 0 = low, 1 = normal, 2 = critical
async fn notify_dbus_error(context: &str, err: &zbus::Error) {
    let title = "OMEN Space — D-Bus Hatası";
    let body = format!("{}: {}", context, err);
    eprintln!("[omen-gui] {}", body);

    // Try session bus for desktop notification
    if let Ok(conn) = zbus::Connection::session().await {
        let mut hints = std::collections::HashMap::<&str, zbus::zvariant::Value>::new();
        hints.insert("urgency", zbus::zvariant::Value::U8(1));

        let _ = conn.call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(
                "OMEN Space",
                0u32,
                "dialog-error",
                title,
                body.as_str(),
                Vec::<&str>::new(),
                hints,
                7000i32,
            ),
        ).await;
    }
}

/// Fire-and-forget version safe to call from sync context via rt.spawn
fn notify_dbus_error_bg(context: &'static str, err_str: String) {
    let rt = get_runtime();
    rt.spawn(async move {
        let title = "OMEN Space — D-Bus Hatası";
        let body = format!("{}: {}", context, err_str);
        eprintln!("[omen-gui] {}", body);

        if let Ok(conn) = zbus::Connection::session().await {
            let mut hints = std::collections::HashMap::<&str, zbus::zvariant::Value>::new();
            hints.insert("urgency", zbus::zvariant::Value::U8(1));

            let _ = conn.call_method(
                Some("org.freedesktop.Notifications"),
                "/org/freedesktop/Notifications",
                Some("org.freedesktop.Notifications"),
                "Notify",
                &(
                    "OMEN Space",
                    0u32,
                    "dialog-error",
                    title,
                    body.as_str(),
                    Vec::<&str>::new(),
                    hints,
                    7000i32,
                ),
            ).await;
        }
    });
}

// ── Synchronous Wrappers for UI Callbacks ────────────────────────────────────

pub fn set_power_profile_sync(profile: String) {
    let rt = get_runtime();
    rt.spawn(async move {
        if let Ok(conn) = get_conn().await {
            if let Ok(proxy) = PowerProxy::new(&conn).await {
                if let Err(e) = proxy.set_power_profile(&profile).await {
                    notify_dbus_error("Güç profili ayarlanamıyor", &e).await;
                }
            } else {
                notify_dbus_error_bg("Daemon bağlantısı kurulamadı (Power)",
                    "Daemon çalışmıyor olabilir".into());
            }
        } else {
            notify_dbus_error_bg("D-Bus bağlantısı başarısız",
                "Sistem D-Bus'ına erişilemiyor".into());
        }
    });
}

/// Returns whether the given board_id is in the community-verified list.
///
/// This is evaluated entirely in the GUI process (no D-Bus) by embedding the
/// same list as the daemon's `capabilities::LinuxCapabilityClassifier::is_board_verified`.
/// It's called once at startup to decide whether to show the unverified-board banner.
pub fn is_board_verified_sync(board_id: &str) -> bool {
    matches!(
        board_id.trim().to_uppercase().as_str(),
        "8A14" | "8A15" | "8574" | "8600" | "8787" | "878C" | "88D2" | "8BAD"
        | "8BAF" | "8BB0" | "8BCA" | "8BAB" | "8C76" | "8C77" | "8BA9"
        | "8BCD" | "8D24" | "8E35" | "8D26" | "8D2F"
        | "8BB1" | "8A18" | "8E10" | "8603" | "8B9D" | "8B9E"
        | "8C3A" | "8C3B" | "8C58" | "8E41"
        | "88D9" | "88DA" | "8A3E" | "8DCD" | "8A26" | "8A25"
        | "8BD4" | "8C2F" | "88DB" | "88EC" | "88EE" | "8C3F"
        | "8E5E" | "8A3D"
        | "88F8"
        | "8BBE" | "8BD5" | "8C99" | "8C9C"
        // Issue #245 – quickman1001, fully verified
        | "8E5D"
        // Issue #242 – OMEN 16 n0xxx, profile works via fallback (fan WMI degraded)
        | "8A42"
    )
}

pub fn set_fan_mode_sync(mode: String) {
    let rt = get_runtime();
    rt.spawn(async move {
        if let Ok(conn) = get_conn().await {
            if let Ok(proxy) = FanProxy::new(&conn).await {
                if let Err(e) = proxy.set_fan_mode(&mode).await {
                    notify_dbus_error("Fan modu ayarlanamıyor", &e).await;
                }
            } else {
                notify_dbus_error_bg("Daemon bağlantısı kurulamadı (Fan)",
                    "Daemon çalışmıyor olabilir".into());
            }
        }
    });
}

pub fn set_thermal_protection_sync(enabled: bool) {
    let rt = get_runtime();
    rt.spawn(async move {
        if let Ok(conn) = get_conn().await {
            if let Ok(proxy) = FanProxy::new(&conn).await {
                if let Err(e) = proxy.set_thermal_protection(enabled).await {
                    notify_dbus_error("Termal koruma ayarlanamıyor", &e).await;
                }
            }
        }
    });
}

pub fn set_app_profiles_enabled_sync(enabled: bool) {
    let rt = get_runtime();
    rt.spawn(async move {
        if let Ok(conn) = get_conn().await {
            if let Ok(proxy) = PowerProxy::new(&conn).await {
                let _ = proxy.set_app_profiles_enabled(enabled).await;
            }
        }
    });
}

pub async fn get_app_profiles_async() -> Result<String, Box<dyn std::error::Error>> {
    let conn = get_conn().await?;
    let proxy = AppProfilesProxy::new(&conn).await?;
    let json = proxy.get_profiles().await?;
    Ok(json)
}

pub fn add_app_profile_sync(process_name: String, power_profile: String, fan_mode: String) {
    let rt = get_runtime();
    rt.spawn(async move {
        if let Ok(conn) = get_conn().await {
            if let Ok(proxy) = AppProfilesProxy::new(&conn).await {
                if let Err(e) = proxy.add_profile(&process_name, &power_profile, &fan_mode).await {
                    notify_dbus_error("Uygulama profili eklenemedi", &e).await;
                }
            }
        }
    });
}

pub fn remove_app_profile_sync(process_name: String) {
    let rt = get_runtime();
    rt.spawn(async move {
        if let Ok(conn) = get_conn().await {
            if let Ok(proxy) = AppProfilesProxy::new(&conn).await {
                if let Err(e) = proxy.remove_profile(&process_name).await {
                    notify_dbus_error("Uygulama profili silinemedi", &e).await;
                }
            }
        }
    });
}

pub fn save_custom_curve_sync(curve_json: String) {
    let rt = get_runtime();
    rt.spawn(async move {
        if let Ok(conn) = get_conn().await {
            if let Ok(proxy) = FanProxy::new(&conn).await {
                if let Err(e) = proxy.save_custom_curve(&curve_json).await {
                    notify_dbus_error("Fan eğrisi kaydedilemedi", &e).await;
                }
            }
        }
    });
}

pub fn set_mode_sync(mode_str: &str, speed_val: i32) {
    let mode = mode_str.to_string();
    let rt = get_runtime();
    rt.spawn(async move {
        if let Ok(conn) = get_conn().await {
            if let Ok(proxy) = RgbProxy::new(&conn).await {
                let _ = proxy.set_mode(&mode, speed_val).await;
            }
        }
    });
}

pub fn set_global_sync(power_val: bool, brightness_val: i32, direction_str: &str) {
    let direction = direction_str.to_string();
    let rt = get_runtime();
    rt.spawn(async move {
        if let Ok(conn) = get_conn().await {
            if let Ok(proxy) = RgbProxy::new(&conn).await {
                let _ = proxy.set_global(power_val, brightness_val, &direction).await;
            }
        }
    });
}

pub async fn run_fan_cleaning_async() -> Result<String, Box<dyn std::error::Error>> {
    let conn = get_conn().await?;
    let proxy = PlatformProxy::new(&conn).await?;
    let result = proxy.run_fan_cleaning().await?;
    Ok(result)
}

#[allow(dead_code)]
pub async fn get_diagnostics_async() -> Result<String, Box<dyn std::error::Error>> {
    let conn = get_conn().await?;
    let proxy = SysMonProxy::new(&conn).await?;
    let json = proxy.get_diagnostics().await?;
    Ok(json)
}

#[allow(dead_code)]
pub fn get_diagnostics_sync() -> SystemStats {
    let rt = get_runtime();
    let json_str = rt.block_on(async {
        match tokio::time::timeout(std::time::Duration::from_millis(500), get_diagnostics_async()).await {
            Ok(Ok(s)) => s,
            _ => "{}".to_string()
        }
    });
    serde_json::from_str(&json_str).unwrap_or_default()
}

// ── Daemon Status ─────────────────────────────────────────────────────────────

/// Broadcast to subscribers whenever the daemon connection changes state.
#[derive(Clone, Debug, PartialEq)]
pub enum DaemonStatus {
    /// Daemon is reachable and sending telemetry.
    Online,
    /// Daemon connection was lost (crashed, stopped, or D-Bus dropped).
    Offline,
}

static TELEMETRY_SENDERS: OnceLock<std::sync::Mutex<Vec<glib::Sender<SystemStats>>>> = OnceLock::new();
static DAEMON_STATUS_SENDERS: OnceLock<std::sync::Mutex<Vec<glib::Sender<DaemonStatus>>>> = OnceLock::new();
static TELEMETRY_STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Subscribe to daemon online/offline transitions.
/// The callback is invoked on the GTK main thread.
#[allow(deprecated)]
pub fn subscribe_daemon_status<F>(mut callback: F)
where
    F: FnMut(DaemonStatus) + 'static,
{
    let (tx, rx) = glib::MainContext::channel(glib::Priority::default());
    rx.attach(None, move |status| {
        callback(status);
        glib::ControlFlow::Continue
    });
    let senders = DAEMON_STATUS_SENDERS.get_or_init(|| std::sync::Mutex::new(Vec::new()));
    senders.lock().unwrap_or_else(|e| e.into_inner()).push(tx);
}

fn broadcast_daemon_status(status: DaemonStatus) {
    if let Some(mutex) = DAEMON_STATUS_SENDERS.get() {
        let senders = mutex.lock().unwrap_or_else(|e| e.into_inner());
        for tx in senders.iter() {
            let _ = tx.send(status.clone());
        }
    }
}

#[allow(deprecated)]
pub fn subscribe_telemetry<F>(mut callback: F)
where
    F: FnMut(SystemStats) + 'static,
{
    let (tx, rx) = glib::MainContext::channel(glib::Priority::default());
    rx.attach(None, move |stats| {
        callback(stats);
        glib::ControlFlow::Continue
    });

    let senders = TELEMETRY_SENDERS.get_or_init(|| std::sync::Mutex::new(Vec::new()));
    senders.lock().unwrap_or_else(|e| e.into_inner()).push(tx);

    if !TELEMETRY_STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        let rt = get_runtime();
        rt.spawn(async move {
            use zbus::export::futures_util::StreamExt;
            #[allow(unused_assignments)]
            let mut was_online = false;
            loop {
                if let Ok(conn) = get_conn().await {
                    if let Ok(proxy) = SysMonProxy::new(&conn).await {
                        if let Ok(mut stream) = proxy.receive_telemetry_updated().await {
                            // We successfully connected — broadcast Online if we weren't before
                            if !was_online {
                                was_online = true;
                                broadcast_daemon_status(DaemonStatus::Online);
                            }
                            while let Some(signal) = stream.next().await {
                                if let Ok(args) = signal.args() {
                                    let json_str = args.json_stats();
                                    if let Ok(stats) = serde_json::from_str::<SystemStats>(json_str) {
                                        if let Some(mutex) = TELEMETRY_SENDERS.get() {
                                            let senders = mutex.lock().unwrap_or_else(|e| e.into_inner());
                                            for tx in senders.iter() {
                                                let _ = tx.send(stats.clone());
                                            }
                                        }
                                    } else {
                                        eprintln!("Telemetry parse error. JSON: {}", json_str);
                                    }
                                }
                            }
                            // Stream ended — daemon went offline
                            was_online = false;
                            broadcast_daemon_status(DaemonStatus::Offline);
                        }
                    }
                } else {
                    // Could not connect at all
                    if was_online {
                        was_online = false;
                        broadcast_daemon_status(DaemonStatus::Offline);
                    }
                }
                // Backoff before retrying
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
        });
    }
}

pub async fn get_hardware_specs_async() -> Result<String, Box<dyn std::error::Error>> {
    let conn = get_conn().await?;
    let proxy = SysMonProxy::new(&conn).await?;
    let json = proxy.get_hardware_specs().await?;
    Ok(json)
}

pub async fn generate_diagnostic_report_async() -> Result<String, Box<dyn std::error::Error>> {
    let conn = get_conn().await?;
    let proxy = SysMonProxy::new(&conn).await?;
    let res = proxy.generate_diagnostic_report().await?;
    Ok(res)
}

pub async fn generate_rgb_issue_async() -> Result<String, Box<dyn std::error::Error>> {
    let conn = get_conn().await?;
    let proxy = SysMonProxy::new(&conn).await?;
    let res = proxy.generate_rgb_issue().await?;
    Ok(res)
}

pub fn get_hardware_specs_sync() -> HardwareSpecs {
    let rt = get_runtime();
    let json_str = rt.block_on(async {
        match tokio::time::timeout(std::time::Duration::from_millis(500), get_hardware_specs_async()).await {
            Ok(Ok(s)) => s,
            _ => "{}".to_string()
        }
    });
    let mut specs: HardwareSpecs = match serde_json::from_str(&json_str) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("HardwareSpecs parse error: {} | JSON: {}", e, json_str);
            HardwareSpecs::default()
        }
    };
    if specs.product_name.is_empty() { specs.product_name = "HP Device".to_string(); }
    if specs.cpu_spec.is_empty() { specs.cpu_spec = "Unknown CPU".to_string(); }
    if specs.gpu_spec.is_empty() { specs.gpu_spec = "Unknown GPU".to_string(); }
    if specs.ram_spec.is_empty() { specs.ram_spec = "Unknown RAM".to_string(); }
    if specs.ssd_spec.is_empty() { specs.ssd_spec = "Unknown SSD".to_string(); }
    if specs.os_spec.is_empty() { specs.os_spec = "Linux".to_string(); }
    specs
}

pub fn set_color_sync(zone_val: i32, hex_color: String) {
    let rt = get_runtime();
    rt.spawn(async move {
        if let Ok(conn) = get_conn().await {
            if let Ok(proxy) = RgbProxy::new(&conn).await {
                if let Err(e) = proxy.set_color(zone_val, &hex_color).await {
                    notify_dbus_error("RGB renk ayarlanamıyor", &e).await;
                }
            }
        }
    });
}

pub async fn get_power_profile_async() -> Result<String, Box<dyn std::error::Error>> {
    let conn = get_conn().await?;
    let proxy = PowerProxy::new(&conn).await?;
    let res = proxy.get_power_profile().await?;
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&res) {
        if let Some(active) = val.get("active").and_then(|v| v.as_str()) {
            return Ok(active.to_string());
        }
    }
    Ok(res)
}

pub async fn get_power_profile_raw_async() -> Result<String, Box<dyn std::error::Error>> {
    let conn = get_conn().await?;
    let proxy = PowerProxy::new(&conn).await?;
    let res = proxy.get_power_profile().await?;
    Ok(res)
}

pub fn get_power_profile_sync() -> String {
    let rt = get_runtime();
    rt.block_on(async {
        match tokio::time::timeout(std::time::Duration::from_millis(500), get_power_profile_async()).await {
            Ok(Ok(s)) => s,
            _ => "balanced".to_string()
        }
    })
}

pub async fn get_fan_mode_async() -> Result<String, Box<dyn std::error::Error>> {
    let conn = get_conn().await?;
    let proxy = FanProxy::new(&conn).await?;
    let res = proxy.get_fan_mode().await?;
    Ok(res)
}

pub fn get_fan_mode_sync() -> String {
    let rt = get_runtime();
    rt.block_on(async {
        match tokio::time::timeout(std::time::Duration::from_millis(500), get_fan_mode_async()).await {
            Ok(Ok(s)) => s,
            _ => "auto".to_string()
        }
    })
}

#[allow(dead_code)]
pub async fn get_fan_info_async() -> Result<String, Box<dyn std::error::Error>> {
    let conn = get_conn().await?;
    let proxy = FanProxy::new(&conn).await?;
    let res = proxy.get_fan_info().await?;
    Ok(res)
}

// ── Mux wrappers ─────────────────────────────────────────────────────────────

pub fn set_gpu_mode_sync(mode: String) {
    let rt = get_runtime();
    rt.spawn(async move {
        if let Ok(conn) = get_conn().await {
            if let Ok(proxy) = MuxProxy::new(&conn).await {
                if let Err(e) = proxy.set_gpu_mode(&mode).await {
                    notify_dbus_error("GPU modu ayarlanamıyor (MUX)", &e).await;
                }
            } else {
                notify_dbus_error_bg("Daemon bağlantısı kurulamadı (MUX)",
                    "Daemon çalışmıyor olabilir".into());
            }
        }
    });
}

pub async fn get_gpu_info_async() -> Result<String, Box<dyn std::error::Error>> {
    let conn = get_conn().await?;
    let proxy = MuxProxy::new(&conn).await?;
    let res = proxy.get_gpu_info().await?;
    Ok(res)
}

// ── Undervolt wrappers ───────────────────────────────────────────────────────

pub fn set_undervolt_sync(core_mv: i32, cache_mv: i32) {
    let rt = get_runtime();
    rt.spawn(async move {
        if let Ok(conn) = get_conn().await {
            if let Ok(proxy) = UndervoltProxy::new(&conn).await {
                if let Err(e) = proxy.set_offset("core", core_mv).await {
                    notify_dbus_error("Undervolt (core) uygulanamıyor", &e).await;
                }
                if let Err(e) = proxy.set_offset("cache", cache_mv).await {
                    notify_dbus_error("Undervolt (cache) uygulanamıyor", &e).await;
                }
            } else {
                notify_dbus_error_bg("Daemon bağlantısı kurulamadı (Undervolt)",
                    "Daemon çalışmıyor olabilir".into());
            }
        }
    });
}

pub fn set_power_limits_sync(pl1: i32, pl2: i32) {
    let rt = get_runtime();
    rt.spawn(async move {
        if let Ok(conn) = get_conn().await {
            if let Ok(proxy) = PowerProxy::new(&conn).await {
                if let Err(e) = proxy.set_power_limits(true, pl1, pl2).await {
                    notify_dbus_error("Güç limitleri ayarlanamıyor (RAPL)", &e).await;
                }
            }
        }
    });
}

pub fn set_tcc_offset_sync(val: i32) {
    let rt = get_runtime();
    rt.spawn(async move {
        if let Ok(conn) = get_conn().await {
            if let Ok(proxy) = UndervoltProxy::new(&conn).await {
                if let Err(e) = proxy.set_tcc_offset(val).await {
                    notify_dbus_error("TCC offset ayarlanamıyor", &e).await;
                }
            }
        }
    });
}

pub async fn get_undervolt_state_async() -> Result<String, Box<dyn std::error::Error>> {
    let conn = get_conn().await?;
    let proxy = UndervoltProxy::new(&conn).await?;
    let res = proxy.get_state().await?;
    Ok(res)
}

pub async fn get_rgb_state_async() -> Result<String, Box<dyn std::error::Error>> {
    let conn = get_conn().await?;
    let proxy = RgbProxy::new(&conn).await?;
    let res = proxy.get_state().await?;
    Ok(res)
}

pub fn get_rgb_state_sync() -> String {
    let rt = get_runtime();
    rt.block_on(async {
        match tokio::time::timeout(std::time::Duration::from_millis(500), get_rgb_state_async()).await {
            Ok(Ok(s)) => s,
            _ => "{}".to_string()
        }
    })
}

/// Ping the daemon to check if it's alive — returns true if reachable.
pub fn ping_daemon_sync() -> bool {
    let rt = get_runtime();
    rt.block_on(async {
        match tokio::time::timeout(std::time::Duration::from_millis(500), get_fan_mode_async()).await {
            Ok(Ok(_)) => true,
            _ => false
        }
    })
}

pub fn set_per_key_colors_sync(colors: Vec<String>) {
    let rt = get_runtime();
    rt.spawn(async move {
        if let Ok(conn) = get_conn().await {
            if let Ok(proxy) = RgbProxy::new(&conn).await {
                let json = serde_json::to_string(&colors).unwrap_or_else(|_| "[]".to_string());
                if let Err(e) = proxy.set_per_key_colors(&json).await {
                    notify_dbus_error("Per-key RGB renkleri ayarlanamıyor", &e).await;
                }
            }
        }
    });
}

pub fn set_battery_care_sync(limit: u32) {
    let rt = get_runtime();
    rt.spawn(async move {
        if let Ok(conn) = get_conn().await {
            if let Ok(proxy) = PlatformProxy::new(&conn).await {
                if let Err(e) = proxy.set_battery_care(limit).await {
                    notify_dbus_error("Pil bakım limiti ayarlanamıyor", &e).await;
                }
            }
        }
    });
}

pub async fn start_per_key_wizard_async() -> Result<String, Box<dyn std::error::Error>> {
    let conn = get_conn().await?;
    let proxy = RgbProxy::new(&conn).await?;
    let res = proxy.start_per_key_wizard().await?;
    Ok(res)
}

pub async fn light_key_index_async(index: u32, hex_color: &str) -> Result<String, Box<dyn std::error::Error>> {
    let conn = get_conn().await?;
    let proxy = RgbProxy::new(&conn).await?;
    let res = proxy.light_key_index(index, hex_color).await?;
    Ok(res)
}

pub async fn record_key_mapping_async(index: u32, key_name: &str) -> Result<String, Box<dyn std::error::Error>> {
    let conn = get_conn().await?;
    let proxy = RgbProxy::new(&conn).await?;
    let res = proxy.record_key_mapping(index, key_name).await?;
    Ok(res)
}

pub async fn export_keymap_report_async() -> Result<String, Box<dyn std::error::Error>> {
    let conn = get_conn().await?;
    let proxy = RgbProxy::new(&conn).await?;
    let res = proxy.export_keymap_report().await?;
    Ok(res)
}

pub async fn send_toggle_overlay_signal() -> Result<(), zbus::Error> {
    let conn = get_conn().await?;
    let proxy = PlatformProxy::new(&conn).await?;
    let _ = proxy.toggle_overlay().await;
    Ok(())
}


