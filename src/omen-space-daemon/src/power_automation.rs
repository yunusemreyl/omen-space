use crate::notifier::DesktopNotifier;
use crate::power::PowerService;
use crate::sysmon::battery::{find_battery_path, probe_ac_online};
use futures::StreamExt;
use log::info;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use zbus::fdo::PropertiesProxy;

#[derive(Clone)]
pub struct PowerAutomationService {
    last_ac_state: Arc<Mutex<Option<bool>>>,
    pre_battery_profile: Arc<Mutex<Option<String>>>,
    power_service: PowerService,
}

impl PowerAutomationService {
    pub fn new(power_service: PowerService) -> Self {
        Self {
            last_ac_state: Arc::new(Mutex::new(None)),
            pre_battery_profile: Arc::new(Mutex::new(None)),
            power_service,
        }
    }

    pub fn start_monitor(&self) {
        let power_ppd = self.power_service.clone();
        tokio::spawn(async move {
            if let Ok(conn) = zbus::Connection::system().await {
                if let Ok(proxy) = PropertiesProxy::builder(&conn)
                    .destination("net.hadess.PowerProfiles")
                    .unwrap()
                    .path("/net/hadess/PowerProfiles")
                    .unwrap()
                    .build()
                    .await
                {
                    if let Ok(mut stream) = proxy.receive_properties_changed().await {
                        while let Some(signal) = stream.next().await {
                            if let Ok(args) = signal.args() {
                                if args.interface_name() == "net.hadess.PowerProfiles" {
                                    if let Some(val) =
                                        args.changed_properties().get("ActiveProfile")
                                    {
                                        if let Ok(profile_str) = <&str>::try_from(val) {
                                            info!("PPD Profile changed to: {}", profile_str);
                                            let _ =
                                                power_ppd.set_profile_from_ppd(profile_str).await;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        let last_state = self.last_ac_state.clone();
        let pre_battery = self.pre_battery_profile.clone();
        let power_ac = self.power_service.clone();
        tokio::spawn(async move {
            loop {
                sleep(Duration::from_secs(4)).await;
                let current_ac = is_ac_power_connected();

                let mut state_lock = last_state.lock().await;
                if let Some(prev) = *state_lock {
                    if prev != current_ac {
                        *state_lock = Some(current_ac);
                        drop(state_lock);
                        if current_ac {
                            // Give firmware/EC a brief moment to complete its AC attach transition
                            // before restoring host/CPU/RAPL state (#398).
                            sleep(Duration::from_millis(600)).await;
                        }
                        let mut saved = pre_battery.lock().await;
                        handle_ac_transition(&power_ac, &mut saved, current_ac, true).await;
                    }
                } else {
                    *state_lock = Some(current_ac);
                }
            }
        });
    }
}

pub(crate) async fn handle_ac_transition(
    power_ac: &PowerService,
    pre_battery_profile: &mut Option<String>,
    current_ac: bool,
    notify: bool,
) {
    let auto_switch = power_ac.is_ac_auto_switch_enabled().await;
    if current_ac {
        let target = if auto_switch {
            let saved = pre_battery_profile.take().filter(|p| !p.is_empty());
            if saved.is_some() {
                saved
            } else {
                let current = power_ac.current_profile().await;
                if matches!(current.as_str(), "power-saver" | "quiet" | "low-power") {
                    Some("balanced".to_string())
                } else {
                    None
                }
            }
        } else {
            *pre_battery_profile = None;
            None
        };

        power_ac.restore_ac_power_state(target.as_deref()).await;

        if auto_switch && target.is_some() {
            let active = power_ac.current_profile().await;
            info!("AC Power connected. Restored profile '{}'...", active);
            if notify {
                DesktopNotifier::send_notification(
                    "OMENSpace Power Automation",
                    &format!("AC Power connected. Restored {} mode.", active),
                    0,
                )
                .await;
            }
        } else {
            info!("AC Power connected. Refreshed AC power state.");
        }
    } else if auto_switch {
        let current = power_ac.current_profile().await;
        *pre_battery_profile = Some(current.clone());
        info!(
            "Battery Power connected. Saving '{}' and applying Battery Saver Profile...",
            current
        );
        if notify {
            DesktopNotifier::send_notification(
                "OMENSpace Power Automation",
                "Running on Battery. Switched to Quiet/Saver mode.",
                1,
            )
            .await;
        }
        let _ = power_ac.set_profile_internal("power-saver").await;
    } else {
        *pre_battery_profile = None;
        info!("Battery Power connected, but AC/Battery auto-switch is disabled — keeping current profile.");
    }
}

fn is_ac_power_connected() -> bool {
    let ps = Path::new("/sys/class/power_supply");
    if find_battery_path(ps).is_none() {
        return true;
    }
    probe_ac_online(ps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::power::tests::{mock_paths, MockSysfsIo};

    #[tokio::test]
    async fn sim_ac_auto_switch_disabled_preserves_profile_on_unplug_and_refreshes_on_ac() {
        let dir = std::env::temp_dir().join(format!("omen-power-auto-off-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let paths = mock_paths(&dir);
        let io = MockSysfsIo::default();
        io.set_file(&paths.acpi_platform_profile, "performance");
        io.set_file(
            dir.join("acpi/platform_profile_choices"),
            "low-power balanced performance",
        );

        let svc = PowerService::new_with_paths(dir.join("power.json"), paths, Arc::new(io), false)
            .await
            .unwrap();
        assert!(
            !svc.is_ac_auto_switch_enabled().await,
            "AC/battery auto-switch must be opt-in (disabled by default)"
        );
        svc.set_profile_internal("performance").await;

        let mut saved = None;
        handle_ac_transition(&svc, &mut saved, false, false).await;
        assert_eq!(svc.current_profile().await, "performance");
        assert!(saved.is_none());

        handle_ac_transition(&svc, &mut saved, true, false).await;
        assert_eq!(svc.current_profile().await, "performance");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn sim_ac_auto_switch_enabled_restores_pre_battery_profile_on_reconnect() {
        let dir = std::env::temp_dir().join(format!("omen-power-auto-on-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let paths = mock_paths(&dir);
        let io = MockSysfsIo::default();
        io.set_file(&paths.acpi_platform_profile, "balanced");
        io.set_file(
            dir.join("acpi/platform_profile_choices"),
            "low-power balanced performance",
        );

        let svc = PowerService::new_with_paths(dir.join("power.json"), paths, Arc::new(io), false)
            .await
            .unwrap();
        svc.set_profile_internal("balanced").await;
        svc.set_ac_auto_switch_enabled(true).await;
        assert!(svc.is_ac_auto_switch_enabled().await);

        let mut saved = None;
        handle_ac_transition(&svc, &mut saved, false, false).await;
        assert_eq!(svc.current_profile().await, "power-saver");
        assert_eq!(saved.as_deref(), Some("balanced"));

        handle_ac_transition(&svc, &mut saved, true, false).await;
        assert_eq!(svc.current_profile().await, "balanced");
        assert!(saved.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
