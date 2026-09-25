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
    pub total_pwr: f64,
}

// ── Proxies ──────────────────────────────────────────────────────────────────

#[proxy(
    interface = "org.hp.omen.Power",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Power"
)]
pub trait Power {
    async fn set_power_profile(&self, profile: &str) -> zbus::Result<String>;
    async fn get_power_profile(&self) -> zbus::Result<String>;
}

#[proxy(
    interface = "org.hp.omen.Fan",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Fan"
)]
pub trait Fan {
    async fn set_fan_mode(&self, mode: &str) -> zbus::Result<String>;
    async fn get_fan_mode(&self) -> zbus::Result<String>;
}

#[proxy(
    interface = "org.hp.omen.SysMon",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/SysMon"
)]
pub trait SysMon {
    #[zbus(signal)]
    fn telemetry_updated(&self, json_stats: &str) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.hp.omen.Platform",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Platform"
)]
pub trait Platform {
    async fn toggle_overlay(&self) -> zbus::Result<String>;

    #[zbus(signal)]
    fn macro_key_pressed(&self, key_name: &str) -> zbus::Result<()>;
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

pub async fn get_conn() -> Result<zbus::Connection, zbus::Error> {
    match zbus::Connection::system().await {
        Ok(c) => Ok(c),
        Err(_) => zbus::Connection::session().await,
    }
}

// ── Profile and Fan Control API ──────────────────────────────────────────────

pub async fn set_power_profile(profile: &str) -> Result<String, zbus::Error> {
    let normalized = match profile.to_lowercase().as_str() {
        "quiet" | "eco" | "power-saver" | "low-power" => "power-saver",
        "performance" | "perf" | "max" => "performance",
        _ => "balanced",
    };
    let conn = get_conn().await?;
    let proxy = PowerProxy::new(&conn).await?;
    proxy.set_power_profile(normalized).await
}

pub async fn get_power_profile() -> String {
    if let Ok(conn) = get_conn().await {
        if let Ok(proxy) = PowerProxy::new(&conn).await {
            if let Ok(json_str) = proxy.get_power_profile().await {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str) {
                    if let Some(active) = val.get("active").and_then(|v| v.as_str()) {
                        return active.to_string();
                    }
                }
                return json_str;
            }
        }
    }
    "balanced".to_string()
}

pub async fn set_fan_mode(mode: &str) -> Result<String, zbus::Error> {
    let normalized = match mode.to_lowercase().as_str() {
        "max" | "turbo" => "max",
        "custom" | "manual" => "custom",
        _ => "auto",
    };
    let conn = get_conn().await?;
    let proxy = FanProxy::new(&conn).await?;
    proxy.set_fan_mode(normalized).await
}

pub async fn get_fan_mode() -> String {
    if let Ok(conn) = get_conn().await {
        if let Ok(proxy) = FanProxy::new(&conn).await {
            if let Ok(mode) = proxy.get_fan_mode().await {
                return mode.trim().to_string();
            }
        }
    }
    "auto".to_string()
}

// ── Telemetry and Hotkey Subscribers ─────────────────────────────────────────

static TELEMETRY_SENDERS: std::sync::OnceLock<std::sync::Mutex<Vec<glib::Sender<SystemStats>>>> = std::sync::OnceLock::new();
static TELEMETRY_STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

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
            use futures::StreamExt;
            loop {
                if let Ok(conn) = get_conn().await {
                    if let Ok(proxy) = SysMonProxy::new(&conn).await {
                        if let Ok(mut stream) = proxy.receive_telemetry_updated().await {
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
                                    }
                                }
                            }
                        }
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
        });
    }
}
