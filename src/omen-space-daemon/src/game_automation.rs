use crate::fan::FanService;
use crate::notifier::DesktopNotifier;
use crate::power::PowerService;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

const APP_PROFILES_PATH: &str = "/etc/omen-space/app_profiles.json";
const LEGACY_APP_PROFILES_PATH: &str = "/etc/omenspace/app_profiles.json";
const PROFILE_SWITCH_COOLDOWN: Duration = Duration::from_secs(30);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AppProfile {
    pub process_name: String,
    pub power_profile: String,
    pub fan_mode: String,
}

#[derive(Clone)]
pub struct GameAutomationService {
    game_profiles: Arc<Mutex<HashMap<String, AppProfile>>>,
    active_game: Arc<Mutex<Option<String>>>,
    pre_app_state: Arc<Mutex<Option<(String, String)>>>,
    last_profile_switch: Arc<Mutex<Option<std::time::Instant>>>,
    power_service: PowerService,
    fan_service: FanService,
}

pub(crate) fn load_profiles_from_paths(
    primary: &Path,
    legacy: &Path,
) -> HashMap<String, AppProfile> {
    for path in [primary, legacy] {
        if let Ok(content) = fs::read_to_string(path) {
            if let Ok(profiles) = serde_json::from_str::<Vec<AppProfile>>(&content) {
                let mut map = HashMap::new();
                for p in profiles {
                    map.insert(p.process_name.to_lowercase(), p);
                }
                return map;
            }
        }
    }
    HashMap::new()
}

pub(crate) fn match_running_profile(
    running_procs: &[String],
    profiles: &HashMap<String, AppProfile>,
) -> Option<AppProfile> {
    for (proc_name, profile) in profiles {
        let needle = proc_name.trim().to_lowercase();
        if !needle.is_empty() && running_procs.iter().any(|p| p.contains(&needle)) {
            return Some(profile.clone());
        }
    }
    None
}

impl GameAutomationService {
    pub fn new(power_service: PowerService, fan_service: FanService) -> Self {
        let default_games = load_profiles_from_paths(
            Path::new(APP_PROFILES_PATH),
            Path::new(LEGACY_APP_PROFILES_PATH),
        );
        Self {
            game_profiles: Arc::new(Mutex::new(default_games)),
            active_game: Arc::new(Mutex::new(None)),
            pre_app_state: Arc::new(Mutex::new(None)),
            last_profile_switch: Arc::new(Mutex::new(None)),
            power_service,
            fan_service,
        }
    }

    fn save_profiles(map: &HashMap<String, AppProfile>) {
        let path = Path::new(APP_PROFILES_PATH);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let profiles: Vec<AppProfile> = map.values().cloned().collect();
        if let Ok(json) = serde_json::to_string_pretty(&profiles) {
            if let Err(e) = fs::write(path, json) {
                error!("Failed to save app profiles: {}", e);
            }
        }
    }

    async fn restore_previous_state(&self) {
        let prev = self.pre_app_state.lock().await.take();
        *self.active_game.lock().await = None;
        if let Some((prev_power, prev_fan)) = prev {
            let _ = self.power_service.set_profile_internal(&prev_power).await;
            let _ = self.fan_service.apply_mode_internal(&prev_fan).await;
        }
    }

    pub fn start_monitor(&self) {
        let this = self.clone();
        tokio::spawn(async move {
            loop {
                sleep(Duration::from_secs(5)).await;

                if !this.power_service.is_app_profiles_enabled().await {
                    if this.active_game.lock().await.is_some() {
                        info!("App Profiles disabled; restoring previous power and fan state.");
                        this.restore_previous_state().await;
                    }
                    continue;
                }

                let game_map = this.game_profiles.lock().await.clone();
                if game_map.is_empty() {
                    if this.active_game.lock().await.is_some() {
                        this.restore_previous_state().await;
                    }
                    continue;
                }

                let running_procs = tokio::task::spawn_blocking(crate::sysmon::get_running_process_names)
                    .await
                    .unwrap_or_default();
                let found_game = match_running_profile(&running_procs, &game_map);
                let current_active = this.active_game.lock().await.clone();

                match (current_active, found_game) {
                    (None, Some(profile)) => {
                        let cooldown_ok = this
                            .last_profile_switch
                            .lock()
                            .await
                            .is_none_or(|t| t.elapsed() >= PROFILE_SWITCH_COOLDOWN);
                        if !cooldown_ok {
                            warn!(
                                "App Profiles: switch throttled by cooldown (#282) for '{}'",
                                profile.process_name
                            );
                            *this.active_game.lock().await = Some(profile.process_name.clone());
                            continue;
                        }
                        if !PowerService::check_acpi_healthy().await {
                            warn!(
                                "App Profiles: ACPI platform_profile unresponsive — skipping switch (#282)"
                            );
                            *this.active_game.lock().await = Some(profile.process_name.clone());
                            continue;
                        }

                        {
                            let mut pre = this.pre_app_state.lock().await;
                            if pre.is_none() {
                                let prev_power = this.power_service.current_profile().await;
                                let prev_fan = this.fan_service.get_mode_internal().await;
                                *pre = Some((prev_power, prev_fan));
                            }
                        }

                        *this.active_game.lock().await = Some(profile.process_name.clone());
                        *this.last_profile_switch.lock().await = Some(std::time::Instant::now());

                        info!(
                            "App Profiles: '{}' launched -> power='{}', fan='{}'",
                            profile.process_name, profile.power_profile, profile.fan_mode
                        );
                        let _ = this
                            .power_service
                            .set_profile_internal(&profile.power_profile)
                            .await;
                        let _ = this.fan_service.apply_mode_internal(&profile.fan_mode).await;

                        DesktopNotifier::send_notification(
                            "OMENSpace App Profile Activated",
                            &format!(
                                "App '{}' launched. Power: '{}', Fan: '{}'.",
                                profile.process_name, profile.power_profile, profile.fan_mode
                            ),
                            1,
                        )
                        .await;
                    }
                    (Some(current), None) => {
                        info!(
                            "App Profiles: '{}' closed. Restoring previous power and fan modes.",
                            current
                        );
                        this.restore_previous_state().await;
                        DesktopNotifier::send_notification(
                            "OMENSpace App Profile Deactivated",
                            &format!("App '{}' closed. Restored previous profile.", current),
                            0,
                        )
                        .await;
                    }
                    _ => {}
                }
            }
        });
    }
}

#[zbus::interface(name = "org.hp.omen.AppProfiles")]
impl GameAutomationService {
    pub async fn get_profiles(&self) -> String {
        let map = self.game_profiles.lock().await;
        let profiles: Vec<AppProfile> = map.values().cloned().collect();
        serde_json::to_string(&profiles).unwrap_or_default()
    }

    pub async fn add_profile(
        &self,
        process_name: String,
        power_profile: String,
        fan_mode: String,
    ) -> String {
        let mut map = self.game_profiles.lock().await;
        let proc_lower = process_name.to_lowercase();
        map.insert(
            proc_lower.clone(),
            AppProfile {
                process_name: proc_lower,
                power_profile,
                fan_mode,
            },
        );
        Self::save_profiles(&map);
        "OK".to_string()
    }

    pub async fn remove_profile(&self, process_name: String) -> String {
        let mut map = self.game_profiles.lock().await;
        map.remove(&process_name.to_lowercase());
        Self::save_profiles(&map);
        "OK".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_legacy_app_profiles_when_canonical_path_absent() {
        let dir = std::env::temp_dir().join(format!("omen-appprof-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let canonical = dir.join("canonical.json");
        let legacy = dir.join("legacy.json");
        fs::write(
            &legacy,
            r#"[{"process_name":"cyberpunk2077","power_profile":"performance","fan_mode":"max"}]"#,
        )
        .unwrap();

        let map = load_profiles_from_paths(&canonical, &legacy);
        let procs = vec!["steam".to_string(), "cyberpunk2077".to_string()];
        let matched = match_running_profile(&procs, &map).unwrap();
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(matched.power_profile, "performance");
        assert_eq!(matched.fan_mode, "max");
    }
}
