use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FanPreset {
    pub name: String,
    pub points: Vec<(f64, f64)>,
}

fn get_presets_path() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config/omenspace/fan_presets.json")
    } else {
        PathBuf::from("/tmp/fan_presets.json")
    }
}

pub fn load_presets() -> Vec<FanPreset> {
    let path = get_presets_path();
    if let Ok(content) = fs::read_to_string(&path) {
        serde_json::from_str(&content).unwrap_or_else(|_| Vec::new())
    } else {
        let default_quiet = FanPreset {
            name: "Quiet".to_string(),
            points: vec![
                (40.0, 0.0),
                (55.0, 15.0),
                (70.0, 35.0),
                (85.0, 60.0),
                (100.0, 100.0),
            ],
        };
        let defaults = vec![default_quiet];
        // Don't auto-save here to avoid permission issues if dir doesn't exist,
        // it will be saved if the user adds/removes presets.
        defaults
    }
}

pub fn save_presets(presets: &[FanPreset]) {
    let path = get_presets_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(presets) {
        let _ = fs::write(path, json);
    }
}

pub fn delete_preset(name: &str) {
    let mut presets = load_presets();
    let original_len = presets.len();
    presets.retain(|p| p.name != name);
    if presets.len() < original_len {
        save_presets(&presets);
    }
}
