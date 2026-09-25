use std::env;
use std::fs;

fn get_lang() -> String {
    let mut lang_code = "auto".to_string();
    
    if let Ok(home) = env::var("HOME") {
        let path = format!("{}/.config/omenspace/gui_config.json", home);
        if let Ok(json_str) = fs::read_to_string(&path) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&json_str) {
                if let Some(lang) = json.get("language").and_then(|v| v.as_str()) {
                    lang_code = lang.to_string();
                }
            }
        }
    }
    
    if lang_code == "auto" {
        for var in &["LC_MESSAGES", "LC_ALL", "LANG", "LANGUAGE"] {
            if let Ok(val) = env::var(var) {
                let lower = val.to_lowercase();
                if lower.starts_with("tr") || lower.contains("tr_tr") || lower.contains("turkish") {
                    return "tr".to_string();
                }
            }
        }
        return "en".to_string();
    }
    
    lang_code
}

pub fn t(key: &str) -> String {
    let lang = get_lang();
    let is_tr = lang == "tr";

    match key {
        "title" => if is_tr { "OMEN HIZLI AYARLAR" } else { "OMEN QUICK CONTROL" },
        "perf_mode" => if is_tr { "Performans Modu" } else { "Performance Mode" },
        "quiet" => if is_tr { "Sessiz" } else { "Quiet" },
        "eco_silent" => if is_tr { "Eko / Sessiz" } else { "Eco / Silent" },
        "default" => if is_tr { "Varsayılan" } else { "Default" },
        "balanced" => if is_tr { "Dengeli" } else { "Balanced" },
        "performance" => if is_tr { "Performans" } else { "Performance" },
        "max_power" => if is_tr { "Maksimum Güç" } else { "Max Power & Clock" },
        
        "fan_mode" => if is_tr { "Fan Modu" } else { "Fan Mode" },
        "auto" => if is_tr { "Otomatik" } else { "Auto (Dynamic)" },
        "auto_desc" => if is_tr { "Dinamik soğutma eğrisi" } else { "Adaptive thermal curve" },
        "max" => if is_tr { "Maksimum" } else { "Max (100% Turbo)" },
        "max_desc" => if is_tr { "Tam devir soğutma" } else { "Full speed cooling" },
        "custom" => if is_tr { "Özel" } else { "Custom Preset" },
        "custom_desc" => if is_tr { "Kullanıcı fan profili" } else { "User curve profile" },
        
        "fans" => if is_tr { "FANLAR" } else { "FANS" },
        "close" => if is_tr { "Kapat" } else { "Close" },
        _ => key,
    }.to_string()
}
