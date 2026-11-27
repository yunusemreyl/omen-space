//! Saved per-key colour layouts for RGB Studio ("Save as preset").
//!
//! Deliberately mirrors [crate::fan_presets]: same directory, same
//! read-modify-write shape, no daemon involvement. A preset is only a named
//! copy of the 104-slot frame, so loading one is a local operation plus a
//! single SetPerKeyColors call by the caller.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

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

pub fn load_presets_from(path: &Path) -> Vec<KeyPreset> {
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str(&content).unwrap_or_default()
}

pub fn save_presets_at(path: &Path, presets: &[KeyPreset]) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(presets) {
        let _ = fs::write(path, json);
    }
}

pub fn load_presets() -> Vec<KeyPreset> {
    load_presets_from(&get_presets_path())
}

#[allow(dead_code)]
pub fn save_presets(presets: &[KeyPreset]) {
    save_presets_at(&get_presets_path(), presets);
}

/// Add `preset` in `path`, replacing any existing entry with a case-insensitive
/// name match and keeping entries sorted cleanly. Returns `true` if overwritten.
pub fn upsert_preset_at(path: &Path, preset: KeyPreset) -> bool {
    let mut presets = load_presets_from(path);
    let overwritten = if let Some(slot) = presets
        .iter_mut()
        .find(|p| p.name.eq_ignore_ascii_case(&preset.name))
    {
        *slot = preset;
        true
    } else {
        presets.push(preset);
        false
    };
    presets.sort_by_key(|p| p.name.to_lowercase());
    save_presets_at(path, &presets);
    overwritten
}

/// Add `preset`, replacing any existing preset with the same name.
///
/// Returns true when an older entry was overwritten, so the caller can tell
/// the user "updated" rather than "saved".
pub fn upsert_preset(preset: KeyPreset) -> bool {
    upsert_preset_at(&get_presets_path(), preset)
}

pub fn delete_preset_at(path: &Path, name: &str) -> bool {
    let mut presets = load_presets_from(path);
    let before = presets.len();
    presets.retain(|p| !p.name.eq_ignore_ascii_case(name));
    if presets.len() != before {
        save_presets_at(path, &presets);
        true
    } else {
        false
    }
}

pub fn delete_preset(name: &str) {
    delete_preset_at(&get_presets_path(), name);
}

/// Trim a user-supplied name; empty names are rejected by the caller.
pub fn normalize_name(raw: &str) -> String {
    raw.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_preset_file(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("omen-key-presets-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir.join("key_presets.json")
    }

    #[test]
    fn normalize_trims_whitespace() {
        assert_eq!(normalize_name("  Flag  "), "Flag");
        assert_eq!(normalize_name("   "), "");
    }

    #[test]
    fn missing_or_corrupt_file_returns_empty_list() {
        let path = temp_preset_file("missing");
        assert!(load_presets_from(&path).is_empty());

        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{not valid json").unwrap();
        assert!(load_presets_from(&path).is_empty());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn upsert_and_delete_presets_handle_sorting_overwrite_and_case() {
        let path = temp_preset_file("crud");
        let p_b = KeyPreset {
            name: "Zebra".to_string(),
            colors: vec!["#FF0000".to_string(); 4],
        };
        let p_a = KeyPreset {
            name: "Alpha".to_string(),
            colors: vec!["#00FF00".to_string(); 4],
        };
        assert!(!upsert_preset_at(&path, p_b));
        assert!(!upsert_preset_at(&path, p_a));

        let listed = load_presets_from(&path);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].name, "Alpha");
        assert_eq!(listed[1].name, "Zebra");

        // Overwrite case-insensitively and update casing + colours.
        let p_a2 = KeyPreset {
            name: "alpha".to_string(),
            colors: vec!["#0000FF".to_string(); 4],
        };
        assert!(upsert_preset_at(&path, p_a2));
        let listed = load_presets_from(&path);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].name, "alpha");
        assert_eq!(listed[0].colors[0], "#0000FF");

        assert!(!delete_preset_at(&path, "DoesNotExist"));
        assert!(delete_preset_at(&path, "ALPHA"));
        let remaining = load_presets_from(&path);
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].name, "Zebra");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
