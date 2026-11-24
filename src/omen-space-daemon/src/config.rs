use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;
use log::{info, warn};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FanConfig {
    pub fan_mode: String,
    pub custom_curve: String,
    pub thermal_protection_enabled: bool,
    /// GPU fan curve for custom mode; empty = both fans follow `custom_curve`.
    #[serde(default)]
    pub custom_curve_gpu: String,
    /// Optional override for the board's minimum nonzero manual PWM duty (1..=255, #359).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_manual_pwm_override: Option<u32>,
    /// When true, 0% curve points write manual PWM 0 (pwm1_enable=1, EC minimum RPM)
    /// instead of handing control to BIOS auto (pwm1_enable=2, allowing 0 RPM fan stop) (#359).
    #[serde(default)]
    pub zero_pct_manual_pwm: bool,
}

impl Default for FanConfig {
    fn default() -> Self {
        Self {
            fan_mode: "auto".to_string(),
            custom_curve: "[]".to_string(),
            thermal_protection_enabled: true,
            custom_curve_gpu: String::new(),
            minimum_manual_pwm_override: None,
            zero_pct_manual_pwm: false,
        }
    }
}

pub struct ConfigManager {
    path: PathBuf,
}

impl ConfigManager {
    pub fn new() -> Self {
        let path = PathBuf::from("/var/lib/omen-space-daemon/fan_config.json");
        Self { path }
    }

    pub async fn load(&self) -> FanConfig {
        match fs::read_to_string(&self.path).await {
            Ok(content) => {
                match serde_json::from_str(&content) {
                    Ok(config) => {
                        info!("Loaded fan config from {:?}", self.path);
                        config
                    }
                    Err(e) => {
                        warn!("Failed to parse config file {:?}: {}. Using defaults.", self.path, e);
                        FanConfig::default()
                    }
                }
            }
            Err(_) => {
                info!("No existing config found at {:?}. Using defaults.", self.path);
                FanConfig::default()
            }
        }
    }

    pub async fn save(&self, config: &FanConfig) {
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent).await;
        }

        match serde_json::to_string_pretty(config) {
            Ok(json) => {
                if let Err(e) = fs::write(&self.path, json).await {
                    warn!("Failed to save fan config to {:?}: {}", self.path, e);
                } else {
                    info!("Saved fan config to {:?}", self.path);
                }
            }
            Err(e) => {
                warn!("Failed to serialize fan config: {}", e);
            }
        }
    }
}
