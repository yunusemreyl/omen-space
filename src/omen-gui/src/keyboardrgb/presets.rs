//! Saved per-key colour layouts for RGB Studio ("Save as preset").
//!
//! Deliberately mirrors [crate::fan_presets]: same directory, same
//! read-modify-write shape, no daemon involvement. A preset is only a named
//! copy of the 104-slot frame, so loading one is a local operation plus a
//! single SetPerKeyColors call by the caller.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// One saved keyboard colouring: a name plus the full per-key frame.
///
/// colors is stored as-is (usually 104 entries) rather than trimmed, so a
/// preset round-trips exactly. Loading pads or truncates to whatever the
/// current GUI frame expects instead.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeyPreset {
    pub name: String,
    pub colors: Vec<String>,
}

fn get_presets_path() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config/omenspace/key_presets.json")
    } else {
        PathBuf::from("/tmp/key_presets.json")
    }
}

pub fn load_presets() -> Vec<KeyPreset> {
    let Ok(content) = fs::read_to_string(get_presets_path()) else {
        return Vec::new();
    };
    serde_json::from_str(&content).unwrap_or_default()
}

pub fn save_presets(presets: &[KeyPreset]) {
    let path = get_presets_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(presets) {
        let _ = fs::write(path, json);
    }
}

/// Add `preset`, replacing any existing preset with the same name.
///
/// Returns true when an older entry was overwritten, so the caller can tell
/// the user "updated" rather than "saved".
pub fn upsert_preset(preset: KeyPreset) -> bool {
    let mut presets = load_presets();
    let overwritten = if let Some(slot) = presets.iter_mut().find(|p| p.name == preset.name) {
        *slot = preset;
        true
    } else {
        presets.push(preset);
        false
    };
    save_presets(&presets);
    overwritten
}

pub fn delete_preset(name: &str) {
    let mut presets = load_presets();
    let before = presets.len();
    presets.retain(|p| p.name != name);
    if presets.len() != before {
        save_presets(&presets);
    }
}

/// Trim a user-supplied name; empty names are rejected by the caller.
pub fn normalize_name(raw: &str) -> String {
    raw.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_trims_whitespace() {
        assert_eq!(normalize_name("  Flag  "), "Flag");
        assert_eq!(normalize_name("   "), "");
    }
}
