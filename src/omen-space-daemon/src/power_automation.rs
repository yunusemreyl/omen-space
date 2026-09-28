use log::info;
use zbus::fdo::PropertiesProxy;
use futures::StreamExt;
use std::fs;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use crate::notifier::DesktopNotifier;
use std::sync::OnceLock;
use std::path::PathBuf;

#[zbus::proxy(
    interface = "org.hp.omen.Power",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Power"
)]
trait Power {
    async fn set_power_profile(&self, profile: &str) -> zbus::Result<String>;
    async fn get_power_profile(&self) -> zbus::Result<String>;
}

#[zbus::proxy(
    interface = "org.hp.omen.Fan",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Fan"
)]
trait Fan {
    async fn set_fan_mode(&self, mode: &str) -> zbus::Result<String>;
}

static AC_ONLINE_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();

#[derive(Clone)]
pub struct PowerAutomationService {
    last_applied_mode: Arc<Mutex<Option<bool>>>,
}

impl PowerAutomationService {
    pub fn new() -> Self {
        Self {
            last_applied_mode: Arc::new(Mutex::new(None)),
        }
    }

    pub fn start_monitor(&self) {
        let last_applied = self.last_applied_mode.clone();

        tokio::spawn(async move {
            if let Ok(conn) = zbus::Connection::system().await {
                if let Ok(proxy) = PropertiesProxy::builder(&conn)
                    .destination("net.hadess.PowerProfiles").unwrap()
                    .path("/net/hadess/PowerProfiles").unwrap()
                    .build().await {
                    
                    if let Ok(mut stream) = proxy.receive_properties_changed().await {
                        while let Some(signal) = stream.next().await {
                            if let Ok(args) = signal.args() {
                                if args.interface_name() == "net.hadess.PowerProfiles" {
                                    if let Some(val) = args.changed_properties().get("ActiveProfile") {
                                        if let Ok(profile_str) = <&str>::try_from(val) {
                                            info!("PPD Profile changed to: {}", profile_str);
                                            let mode = match profile_str {
                                                "performance" => "Performance",
                                                "power-saver" => "Quiet",
                                                _ => "Default",
                                            };
                                            let _ = crate::platform::set_thermal_policy_by_name(mode);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        tokio::spawn(async move {
            loop {
                sleep(Duration::from_secs(2)).await;
                let current_ac = is_ac_power_connected();
                let is_auto_enabled = is_ac_auto_performance_enabled();

                let mut applied_lock = last_applied.lock().await;

                if is_auto_enabled {
                    if *applied_lock != Some(current_ac) {
                        *applied_lock = Some(current_ac);
                        if current_ac {
                            info!("AC Power connected. Applying Performance profile and Max Fans...");
                            DesktopNotifier::send_notification(
                                "OMEN Space — Power Automation",
                                "AC Power connected. Switched to Performance mode & Max Fans.",
                                0,
                            ).await;
                            if let Ok(conn) = zbus::Connection::system().await {
                                if let Ok(proxy) = PowerProxy::new(&conn).await {
                                    let _ = proxy.set_power_profile("performance").await;
                                }
                                if let Ok(proxy) = FanProxy::new(&conn).await {
                                    let _ = proxy.set_fan_mode("max").await;
                                }
                            }
                        } else {
                            info!("Battery Power connected. Applying Battery Saver profile and Auto Fans...");
                            DesktopNotifier::send_notification(
                                "OMEN Space — Power Automation",
                                "Running on Battery. Switched to Quiet/Saver mode & Auto Fans.",
                                1,
                            ).await;
                            if let Ok(conn) = zbus::Connection::system().await {
                                if let Ok(proxy) = PowerProxy::new(&conn).await {
                                    let _ = proxy.set_power_profile("power-saver").await;
                                }
                                if let Ok(proxy) = FanProxy::new(&conn).await {
                                    let _ = proxy.set_fan_mode("auto").await;
                                }
                            }
                        }
                    }
                } else if applied_lock.is_some() {
                    *applied_lock = None;
                }
            }
        });
    }
}

fn is_ac_auto_performance_enabled() -> bool {
    if let Ok(json_str) = fs::read_to_string("/etc/omen-space/power.json") {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&json_str) {
            if let Some(enabled) = json.get("ac_auto_performance").and_then(|v| v.as_bool()) {
                return enabled;
            }
        }
    }
    if let Ok(json_str) = fs::read_to_string("/etc/omen-space/settings.json") {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&json_str) {
            if let Some(enabled) = json.get("ac_auto_performance").and_then(|v| v.as_bool()) {
                return enabled;
            }
        }
    }
    if let Ok(entries) = fs::read_dir("/home") {
        for entry in entries.flatten() {
            let path = entry.path().join(".config/omenspace/settings.json");
            if let Ok(json_str) = fs::read_to_string(&path) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&json_str) {
                    if let Some(enabled) = json.get("ac_auto_performance").and_then(|v| v.as_bool()) {
                        return enabled;
                    }
                }
            }
        }
    }
    false
}

fn is_ac_power_connected() -> bool {
    let online_path = AC_ONLINE_PATH.get_or_init(|| {
        if let Ok(entries) = fs::read_dir("/sys/class/power_supply") {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    let n_lower = name.to_lowercase();
                    if n_lower.starts_with("ac") || n_lower.starts_with("adp") || n_lower.contains("mains") {
                        let online_file = path.join("online");
                        if online_file.exists() {
                            return Some(online_file);
                        }
                    }
                }
            }
        }
        None
    });

    if let Some(p) = online_path {
        if let Ok(val) = fs::read_to_string(p) {
            return val.trim() == "1";
        }
    }
    true // default to AC if undetectable
}
