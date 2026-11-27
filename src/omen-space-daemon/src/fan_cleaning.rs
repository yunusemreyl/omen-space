use crate::notifier::DesktopNotifier;
use log::{error, info};
use omen_types::FanProxy;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use tokio::sync::Notify;
use tokio::time::{sleep, Duration};
use zbus::Connection;

static CANCEL_NOTIFY: OnceLock<Notify> = OnceLock::new();
static CANCEL_REQUESTED: AtomicBool = AtomicBool::new(false);

fn cancel_notifier() -> &'static Notify {
    CANCEL_NOTIFY.get_or_init(Notify::new)
}

/// Wait up to `duration` unless `cancel_notifier()` is signalled earlier.
/// Returns `true` if cancelled early (#364, #385).
async fn wait_or_cancel(duration: Duration) -> bool {
    if CANCEL_REQUESTED.swap(false, Ordering::SeqCst) {
        return true;
    }
    tokio::select! {
        _ = sleep(duration) => false,
        _ = cancel_notifier().notified() => {
            CANCEL_REQUESTED.store(false, Ordering::SeqCst);
            true
        }
    }
}

pub struct FanCleaningService;

impl FanCleaningService {
    pub fn cancel_cleaning() -> String {
        info!("Fan Dust Cleaning cancellation requested by user (#385, #364).");
        CANCEL_REQUESTED.store(true, Ordering::SeqCst);
        cancel_notifier().notify_waiters();
        "Fan Dust Cleaning stopping and restoring previous fan mode...".to_string()
    }

    pub async fn run_cleaning_routine() -> String {
        CANCEL_REQUESTED.store(false, Ordering::SeqCst);
        info!("Starting Fan Dust Cleaning routine...");
        DesktopNotifier::send_notification(
            "OMEN Space Fan Maintenance",
            "Fan Dust Cleaning routine started. Operating fans at high airflow bursts...",
            1,
        )
        .await;

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
        let prev_mode = proxy
            .get_fan_mode()
            .await
            .unwrap_or_else(|_| "auto".to_string());
        let restore_mode = if prev_mode == "max" || prev_mode.is_empty() {
            "auto"
        } else {
            prev_mode.as_str()
        };

        // Step 1: Max out fans (interruptible at any moment via CancelFanCleaning)
        let _ = proxy.set_fan_mode("max").await;
        let cancelled = wait_or_cancel(Duration::from_secs(10)).await;

        // Step 2: Explicitly leave max boost and return to previous mode (auto/ec/custom)
        let _ = proxy.set_fan_mode(restore_mode).await;

        let summary = if cancelled {
            format!(
                "Fan Dust Cleaning stopped early (restored '{}' mode)",
                restore_mode
            )
        } else {
            format!(
                "Fan Dust Cleaning completed successfully (restored '{}' mode)",
                restore_mode
            )
        };

        DesktopNotifier::send_notification("OMEN Space Fan Maintenance", &summary, 0).await;
        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancel_cleaning_interrupts_wait_immediately() {
        let task = tokio::spawn(wait_or_cancel(Duration::from_secs(30)));
        tokio::time::sleep(Duration::from_millis(20)).await;
        let msg = FanCleaningService::cancel_cleaning();
        assert!(msg.contains("stopping"));
        let cancelled = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("cancel must abort cleaning wait without timing out")
            .unwrap();
        assert!(cancelled);
    }
}
