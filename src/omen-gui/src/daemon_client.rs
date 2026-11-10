use tokio::runtime::Runtime;
use std::sync::OnceLock;
pub use omen_types::*;

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

static SHARED_CONN: tokio::sync::Mutex<Option<zbus::Connection>> = tokio::sync::Mutex::const_new(None);

pub async fn get_conn() -> Result<zbus::Connection, zbus::Error> {
    let mut guard = SHARED_CONN.lock().await;
    if let Some(conn) = &*guard {
        return Ok(conn.clone());
    }
    let conn = match zbus::Connection::system().await {
        Ok(c) => c,
        Err(_) => zbus::Connection::session().await?,
    };
    *guard = Some(conn.clone());
    Ok(conn)
}

// ── Desktop Notification Helper ──────────────────────────────────────────────

/// Send a desktop notification via org.freedesktop.Notifications.
/// urgency: 0 = low, 1 = normal, 2 = critical
async fn notify_dbus_error(context: &str, err: impl std::fmt::Display) {
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
    static VERIFIED: std::sync::OnceLock<std::collections::HashSet<String>> = std::sync::OnceLock::new();
    let set = VERIFIED.get_or_init(|| {
        let raw = include_str!("../../omen-space-daemon/src/boards.json");
        serde_json::from_str::<serde_json::Value>(raw)
            .ok()
            .and_then(|v| v.get("verified_boards")?.as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|x| x.as_str().map(|s| s.trim().to_uppercase()))
            .collect()
    });
    set.contains(&board_id.trim().to_uppercase())
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

pub fn set_hw_effects_sync(enabled: bool) {
    let rt = get_runtime();
    rt.spawn(async move {
        if let Ok(conn) = get_conn().await {
            if let Ok(proxy) = RgbProxy::new(&conn).await {
                let _ = proxy.set_hw_effects(enabled).await;
            }
        }
    });
}

pub fn set_global_sync(power_val: bool, brightness_val: i32, direction_str: &str) {
    static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    static IO:tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let generation=GENERATION.fetch_add(1,std::sync::atomic::Ordering::SeqCst)+1;
    let direction = direction_str.to_string();
    let rt = get_runtime();
    rt.spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        let _io=IO.lock().await;
        if GENERATION.load(std::sync::atomic::Ordering::SeqCst)!=generation {return;}
        if let Ok(conn) = get_conn().await {
            if let Ok(proxy) = RgbProxy::new(&conn).await {
                let _ = proxy.set_global(power_val, brightness_val, &direction).await;
            }
        }
    });
}

pub fn set_lightbar_color_sync(zone_val: i32, hex_color: String) {
    let rt = get_runtime();
    rt.spawn(async move {
        if let Ok(conn) = get_conn().await {
            if let Ok(proxy) = RgbProxy::new(&conn).await {
                let _ = proxy.set_lightbar_color(zone_val, &hex_color).await;
            }
        }
    });
}

pub fn set_lightbar_global_sync(power_val: bool, brightness_val: i32, mode_str: &str, speed_val: i32) {
    static IO:tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let generation = GENERATION.fetch_add(1,std::sync::atomic::Ordering::SeqCst) + 1;
    let mode = mode_str.to_string();
    let rt = get_runtime();
    rt.spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        let _io=IO.lock().await;
        if GENERATION.load(std::sync::atomic::Ordering::SeqCst) != generation { return; }
        if let Ok(conn) = get_conn().await {
            if let Ok(proxy) = RgbProxy::new(&conn).await {
                let _ = proxy.set_lightbar_global(power_val, brightness_val, &mode, speed_val).await;
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
    let json_str = tokio::task::block_in_place(|| {
        get_runtime().block_on(async {
            match tokio::time::timeout(std::time::Duration::from_millis(500), get_diagnostics_async()).await {
                Ok(Ok(s)) => s,
                               _ => "{}".to_string()
            }
        })
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
static CURRENT_DAEMON_STATUS: OnceLock<std::sync::Mutex<Option<DaemonStatus>>> = OnceLock::new();
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
    senders.lock().unwrap_or_else(|e| e.into_inner()).push(tx.clone());

    if let Some(cur) = CURRENT_DAEMON_STATUS.get() {
        let status = cur.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(status)=status { let _ = tx.send(status); }
    }
}

fn broadcast_daemon_status(status: DaemonStatus) {
    let cur = CURRENT_DAEMON_STATUS.get_or_init(|| std::sync::Mutex::new(None));
    {
        let mut previous = cur.lock().unwrap_or_else(|e| e.into_inner());
        if previous.as_ref() == Some(&status) { return; }
        *previous = Some(status.clone());
    }

    if let Some(mutex) = DAEMON_STATUS_SENDERS.get() {
        let mut senders = mutex.lock().unwrap_or_else(|e| e.into_inner());
        senders.retain(|tx| tx.send(status.clone()).is_ok());
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
            loop {
                let timeout=std::time::Duration::from_secs(3);
                if let Ok(Ok(conn)) = tokio::time::timeout(timeout,get_conn()).await {
                    if let Ok(Ok(proxy)) = tokio::time::timeout(timeout,SysMonProxy::new(&conn)).await {
                        if let Ok(Ok(mut stream)) = tokio::time::timeout(timeout,proxy.receive_telemetry_updated()).await {
                            let mut deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
                            loop {
                                let Ok(Some(signal)) = tokio::time::timeout_at(deadline, stream.next()).await else { break; };
                                if let Ok(args) = signal.args() {
                                    if let Ok(stats) = serde_json::from_str::<SystemStats>(args.json_stats()) {
                                        deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
                                        broadcast_daemon_status(DaemonStatus::Online);
                                        if let Some(mutex) = TELEMETRY_SENDERS.get() {
                                            let mut senders = mutex.lock().unwrap_or_else(|e| e.into_inner());
                                            senders.retain(|tx| tx.send(stats.clone()).is_ok());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                broadcast_daemon_status(DaemonStatus::Offline);
                // Drop a stale bus connection before retrying; other calls acquire the fresh one.
                *SHARED_CONN.lock().await = None;
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

pub async fn generate_triage_bundle_async() -> Result<String, Box<dyn std::error::Error>> {
    let conn = get_conn().await?;
    let proxy = PlatformProxy::new(&conn).await?;
    let res = proxy.generate_triage_bundle().await?;
    Ok(res)
}

pub async fn generate_rgb_issue_async() -> Result<String, Box<dyn std::error::Error>> {
    let conn = get_conn().await?;
    let proxy = SysMonProxy::new(&conn).await?;
    let res = proxy.generate_rgb_issue().await?;
    Ok(res)
}

pub fn get_hardware_specs_sync() -> HardwareSpecs {
    let json_str = tokio::task::block_in_place(|| {
        get_runtime().block_on(async {
            match tokio::time::timeout(std::time::Duration::from_millis(2500), get_hardware_specs_async()).await {
                Ok(Ok(s)) => s,
                _ => "{}".to_string()
            }
        })
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
    tokio::task::block_in_place(|| {
        get_runtime().block_on(async {
            match tokio::time::timeout(std::time::Duration::from_millis(500), get_power_profile_async()).await {
                Ok(Ok(s)) => s,
                _ => "balanced".to_string()
            }
        })
    })
}

pub async fn get_fan_mode_async() -> Result<String, Box<dyn std::error::Error>> {
    let conn = get_conn().await?;
    let proxy = FanProxy::new(&conn).await?;
    let res = proxy.get_fan_mode().await?;
    Ok(res)
}

pub fn get_fan_mode_sync() -> String {
    tokio::task::block_in_place(|| {
        get_runtime().block_on(async {
            match tokio::time::timeout(std::time::Duration::from_millis(500), get_fan_mode_async()).await {
                Ok(Ok(s)) => s,
                _ => "auto".to_string()
            }
        })
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

pub async fn set_offset_async(plane: String, mv: i32) -> Result<String, Box<dyn std::error::Error>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let rt = get_runtime();
    rt.spawn(async move {
        let res: Result<String, String> = async {
            let conn = get_conn().await.map_err(|e| e.to_string())?;
            let proxy = UndervoltProxy::new(&conn).await.map_err(|e| e.to_string())?;
            let r = proxy.set_offset(&plane, mv).await.map_err(|e| e.to_string())?;
            if r.starts_with("FAIL") {
                notify_dbus_error(&format!("Undervolt ({}) uygulanamıyor", plane), &r).await;
            }
            Ok(r)
        }
        .await;
        let _ = tx.send(res);
    });
    match rx.await {
        Ok(Ok(s)) => Ok(s),
        Ok(Err(e)) => Err(e.into()),
        Err(e) => Err(e.into()),
    }
}


pub fn set_power_limits_sync(pl1: i32, pl2: i32) {
    let rt = get_runtime();
    rt.spawn(async move {
        if let Ok(conn) = get_conn().await {
            if let Ok(proxy) = PowerProxy::new(&conn).await {
                match proxy.set_power_limits(true, pl1, pl2).await {
                    Ok(resp) if resp.starts_with("FAIL") => {
                        notify_dbus_error("Güç limitleri ayarlanamıyor (RAPL)", &resp).await;
                    }
                    Err(e) => {
                        notify_dbus_error("Güç limitleri ayarlanamıyor (RAPL)", &e).await;
                    }
                    _ => {}
                }
            }
        }
    });
}

pub fn reset_power_limits_sync(pl1: i32, pl2: i32) {
    let rt = get_runtime();
    rt.spawn(async move {
        if let Ok(conn) = get_conn().await {
            if let Ok(proxy) = PowerProxy::new(&conn).await {
                if let Err(e) = proxy.set_power_limits(false, pl1, pl2).await {
                    notify_dbus_error("Güç limitleri sıfırlanamıyor (RAPL)", &e).await;
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
                match proxy.set_tcc_offset(val).await {
                    Ok(resp) if resp.starts_with("FAIL") => {
                        notify_dbus_error("TCC offset ayarlanamıyor", &resp).await;
                    }
                    Err(e) => {
                        notify_dbus_error("TCC offset ayarlanamıyor", &e).await;
                    }
                    _ => {}
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
    tokio::task::block_in_place(|| {
        get_runtime().block_on(async {
            match tokio::time::timeout(std::time::Duration::from_millis(500), get_rgb_state_async()).await {
                Ok(Ok(s)) => s,
                _ => "{}".to_string()
            }
        })
    })
}

/// Ping the daemon to check if it's alive — returns true if reachable.
pub fn ping_daemon_sync() -> bool {
    tokio::task::block_in_place(|| {
        get_runtime().block_on(async {
            match tokio::time::timeout(std::time::Duration::from_millis(500), get_fan_mode_async()).await {
                Ok(Ok(_)) => true,
                _ => false
            }
        })
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

pub fn light_key_index_sync(index: u32, hex_color: String) {
    let rt = get_runtime();
    rt.spawn(async move {
        if let Ok(conn) = get_conn().await {
            if let Ok(proxy) = RgbProxy::new(&conn).await {
                let _ = proxy.light_key_index(index, &hex_color).await;
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




