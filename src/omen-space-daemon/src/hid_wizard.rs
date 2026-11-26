use crate::notifier::DesktopNotifier;
use log::info;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct KeymapExport {
    pub board_id: String,
    pub product_name: String,
    pub total_keys_calibrated: usize,
    pub keymap: HashMap<usize, String>,
    pub timestamp: String,
}

#[derive(Clone)]
pub struct HidPerKeyWizard {
    current_index: Arc<Mutex<usize>>,
    calibrated_map: Arc<Mutex<HashMap<usize, String>>>,
    is_active: Arc<Mutex<bool>>,
}

impl HidPerKeyWizard {
    pub fn new() -> Self {
        Self {
            current_index: Arc::new(Mutex::new(0)),
            calibrated_map: Arc::new(Mutex::new(HashMap::new())),
            is_active: Arc::new(Mutex::new(false)),
        }
    }

    pub async fn start_wizard(&self) -> String {
        info!("Starting HID Per-Key RGB Calibration Wizard...");
        {
            let mut active = self.is_active.lock().await;
            *active = true;
            let mut idx = self.current_index.lock().await;
            *idx = 0;
            let mut map = self.calibrated_map.lock().await;
            map.clear();
        }

        // Illuminate first key index (0) in bright white
        let _ = crate::rgb::test_single_key_static(0, 255, 255, 255).await;

        DesktopNotifier::send_notification(
            "OMENSpace RGB Wizard Started",
            "HID Per-Key RGB Calibration Wizard started. Key Index 0 is illuminated in White.",
            0,
        )
        .await;

        serde_json::json!({
            "status": "Wizard Started",
            "current_index": 0,
            "total_keys": 104,
            "instruction": "Identify which physical key on your keyboard is currently illuminated."
        })
        .to_string()
    }

    pub async fn light_key_index(&self, index: usize, hex_color: &str) -> String {
        let (r, g, b) = parse_hex_color(hex_color);
        info!("Lighting key index {} with RGB({}, {}, {})", index, r, g, b);

        let success = crate::rgb::test_single_key_static(index, r, g, b).await;
        {
            let mut idx = self.current_index.lock().await;
            *idx = index;
        }

        serde_json::json!({
            "success": success,
            "current_index": index,
            "color": hex_color
        })
        .to_string()
    }

    pub async fn record_key_mapping(&self, index: usize, physical_label: &str) -> String {
        let label = physical_label.trim().to_string();
        info!("Recorded key mapping: Index {} -> '{}'", index, label);

        {
            let mut map = self.calibrated_map.lock().await;
            map.insert(index, label.clone());
        }

        let next_index = index + 1;
        {
            let mut idx = self.current_index.lock().await;
            *idx = next_index;
        }

        // Automatically light up next index in bright white if within bounds (104 keys)
        if next_index < 104 {
            let _ = crate::rgb::test_single_key_static(next_index, 255, 255, 255).await;
        }

        serde_json::json!({
            "recorded": { "index": index, "label": label },
            "next_index": next_index,
            "total_recorded": self.calibrated_map.lock().await.len()
        })
        .to_string()
    }

    pub async fn export_keymap(&self) -> String {
        let board_id = read_dmi_value("board_name");
        let product_name = read_dmi_value("product_name");
        let map = self.calibrated_map.lock().await.clone();

        let export_data = KeymapExport {
            board_id: board_id.clone(),
            product_name: product_name.clone(),
            total_keys_calibrated: map.len(),
            keymap: map.clone(),
            timestamp: chrono_secs().to_string(),
        };

        let json_str = serde_json::to_string_pretty(&export_data).unwrap_or_default();

        // Write to system keymap dir & /tmp
        let sys_keymap_path = format!("/etc/omen-space/keymaps/hid-perkey-map-{}.json", board_id);
        let tmp_keymap_path = "/tmp/hid-perkey-map.json".to_string();

        if let Some(parent) = Path::new(&sys_keymap_path).parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        let _ = tokio::fs::write(&sys_keymap_path, &json_str).await;
        let _ = tokio::fs::write(&tmp_keymap_path, &json_str).await;

        // Generate Markdown table summary
        let mut md_lines = vec![
            format!(
                "# HID Per-Key RGB Keymap Calibration Report - {}",
                product_name
            ),
            format!("- **Board ID:** {}", board_id),
            format!("- **Total Keys Calibrated:** {}", map.len()),
            String::new(),
            "| HID Index | Physical Key Label |".to_string(),
            "| --- | --- |".to_string(),
        ];

        let mut sorted_keys: Vec<_> = map.keys().collect();
        sorted_keys.sort();
        for &k in sorted_keys {
            if let Some(lbl) = map.get(&k) {
                md_lines.push(format!("| `{}` | `{}` |", k, lbl));
            }
        }
        let md_report = md_lines.join("\n");
        let _ = tokio::fs::write("/tmp/hid-perkey-map.md", &md_report).await;

        DesktopNotifier::send_notification(
            "OMENSpace Keymap Calibration Exported",
            &format!(
                "Saved {} mapped keys to /tmp/hid-perkey-map.json",
                map.len()
            ),
            0,
        )
        .await;

        // Auto open directory in user desktop file manager & launch GitHub issue browser
        let product_name_clone = product_name.clone();
        let board_id_clone = board_id.clone();
        let map_len = map.len();
        tokio::task::spawn_blocking(move || {
            DesktopNotifier::open_in_user_session("/tmp/hid-perkey-map.md");
            DesktopNotifier::open_github_issue(
                &format!("[Keymap Submission] HID Per-Key RGB Map for {} ({})", product_name_clone, board_id_clone),
                &format!("Calibrated {} physical keys for HP OMEN {} (Board ID: `{}`).\n\nPlease find the generated `/tmp/hid-perkey-map.json` attached.", map_len, product_name_clone, board_id_clone),
            );
        });

        json_str
    }
}

fn parse_hex_color(hex: &str) -> (u8, u8, u8) {
    let clean = hex.trim_start_matches('#');
    let r = u8::from_str_radix(clean.get(0..2).unwrap_or("FF"), 16).unwrap_or(255);
    let g = u8::from_str_radix(clean.get(2..4).unwrap_or("FF"), 16).unwrap_or(255);
    let b = u8::from_str_radix(clean.get(4..6).unwrap_or("FF"), 16).unwrap_or(255);
    (r, g, b)
}

fn read_dmi_value(entry: &str) -> String {
    let path = format!("/sys/class/dmi/id/{}", entry);
    fs::read_to_string(path)
        .unwrap_or_else(|_| "Unknown".to_string())
        .trim()
        .to_string()
}

fn chrono_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
