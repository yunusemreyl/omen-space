use log::{info, error};
use tokio::time::{sleep, Duration};
use crate::notifier::DesktopNotifier;
use zbus::Connection;

#[zbus::proxy(
    interface = "org.hp.omen.Fan",
    default_service = "org.hp.omen",
    default_path = "/org/hp/omen/Fan"
)]
trait Fan {
    async fn get_fan_mode(&self) -> zbus::Result<String>;
    async fn set_fan_mode(&self, mode: &str) -> zbus::Result<String>;
}

pub struct FanCleaningService;

impl FanCleaningService {
    pub async fn run_cleaning_routine() -> String {
        info!("Starting Fan Dust Cleaning routine...");
        DesktopNotifier::send_notification(
            "OMEN Space Fan Maintenance",
            "Fan Dust Cleaning routine started. Operating fans at high airflow bursts...",
            1,
        ).await;

        let conn = match Connection::system().await {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to connect to system bus for fan cleaning: {}", e);
                return format!("Error: {}", e);
            }
        };

        let proxy = match FanProxy::new(&conn).await {
            Ok(p) => p,
            Err(e) => {
                error!("Failed to create FanProxy: {}", e);
                return format!("Error: {}", e);
            }
        };

        // Remember user's previous mode so we restore it cleanly instead of staying in max (#364)
        let prev_mode = proxy.get_fan_mode().await.unwrap_or_else(|_| "auto".to_string());
        let restore_mode = if prev_mode == "max" || prev_mode.is_empty() {
            "auto"
        } else {
            prev_mode.as_str()
        };

        // Step 1: Max out fans
        let _ = proxy.set_fan_mode("max").await;
        sleep(Duration::from_secs(10)).await;

        // Step 2: Explicitly leave max boost and return to previous mode (auto/ec/custom)
        let _ = proxy.set_fan_mode(restore_mode).await;

        DesktopNotifier::send_notification(
            "OMEN Space Fan Maintenance",
            &format!("Fan Dust Cleaning completed successfully. Restored fan mode to '{}'.", restore_mode),
            0,
        ).await;

        format!("Fan Dust Cleaning completed successfully (restored '{}' mode)", restore_mode)
    }
}
