pub use omen_types::*;
use std::sync::OnceLock;
use tokio::runtime::Runtime;

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

pub fn normalize_power_profile(profile: &str) -> &'static str {
    match profile.trim().to_lowercase().as_str() {
        "quiet" | "eco" | "power-saver" | "low-power" | "cool" | "sessiz" | "eko"
        | "silencioso" => "power-saver",
        "performance" | "perf" | "max" | "performans" | "rendimiento" => "performance",
        _ => "balanced",
    }
}

pub fn normalize_fan_mode(mode: &str) -> &'static str {
    match mode.trim().to_lowercase().as_str() {
        "max" | "turbo" | "maksimum" | "máximo" => "max",
        "custom" | "manual" | "curve" | "özel" | "personalizado" => "custom",
        _ => "auto",
    }
}

pub async fn set_power_profile(profile: &str) -> Result<String, zbus::Error> {
    let normalized = normalize_power_profile(profile);
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
                        return normalize_power_profile(active).to_string();
                    }
                }
                return normalize_power_profile(&json_str).to_string();
            }
        }
    }
    "balanced".to_string()
}

pub async fn set_fan_mode(mode: &str) -> Result<String, zbus::Error> {
    let normalized = normalize_fan_mode(mode);
    let conn = get_conn().await?;
    let proxy = FanProxy::new(&conn).await?;
    proxy.set_fan_mode(normalized).await
}

pub async fn get_fan_mode() -> String {
    if let Ok(conn) = get_conn().await {
        if let Ok(proxy) = FanProxy::new(&conn).await {
            if let Ok(mode) = proxy.get_fan_mode().await {
                return normalize_fan_mode(&mode).to_string();
            }
        }
    }
    "auto".to_string()
}

// ── Telemetry and Hotkey Subscribers ─────────────────────────────────────────

static TELEMETRY_SENDERS: std::sync::OnceLock<std::sync::Mutex<Vec<glib::Sender<SystemStats>>>> =
    std::sync::OnceLock::new();
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
                                    if let Ok(stats) = serde_json::from_str::<SystemStats>(json_str)
                                    {
                                        if let Some(mutex) = TELEMETRY_SENDERS.get() {
                                            let senders =
                                                mutex.lock().unwrap_or_else(|e| e.into_inner());
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

#[cfg(test)]
mod tests {
    use super::{normalize_fan_mode, normalize_power_profile};

    #[test]
    fn normalizes_power_profiles_across_languages() {
        for s in [
            "power-saver",
            "quiet",
            "eco",
            "low-power",
            "Sessiz",
            "Eko",
            "Silencioso",
        ] {
            assert_eq!(normalize_power_profile(s), "power-saver", "failed for {s}");
        }
        for s in [
            "balanced",
            "default",
            "Varsayılan",
            "Dengeli",
            "Predeterminado",
            "Equilibrado",
        ] {
            assert_eq!(normalize_power_profile(s), "balanced", "failed for {s}");
        }
        for s in ["performance", "perf", "max", "Performans", "Rendimiento"] {
            assert_eq!(normalize_power_profile(s), "performance", "failed for {s}");
        }
    }

    #[test]
    fn normalizes_fan_modes_across_languages() {
        for s in ["auto", "ec", "default", "Otomatik", "Automático"] {
            assert_eq!(normalize_fan_mode(s), "auto", "failed for {s}");
        }
        for s in ["max", "turbo", "Maksimum", "Máximo"] {
            assert_eq!(normalize_fan_mode(s), "max", "failed for {s}");
        }
        for s in ["custom", "manual", "curve", "Özel", "Personalizado"] {
            assert_eq!(normalize_fan_mode(s), "custom", "failed for {s}");
        }
    }
}
