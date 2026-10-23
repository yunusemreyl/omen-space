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
                if lower.starts_with("es") || lower.contains("spanish") {
                    return "es".to_string();
                }
            }
        }
        return "en".to_string();
    }
    
    lang_code
}

pub fn t(key: &str) -> String {
    let lang = get_lang();

    match lang.as_str() {
        "tr" => match key {
            "title" => "OMEN HIZLI AYARLAR",
            "perf_mode" => "Performans Modu",
            "quiet" => "Sessiz",
            "eco_silent" => "Eko / Sessiz",
            "default" => "Varsayılan",
            "balanced" => "Dengeli",
            "performance" => "Performans",
            "max_power" => "Maksimum Güç",
            "fan_mode" => "Fan Modu",
            "auto" => "Otomatik",
            "auto_desc" => "Dinamik soğutma eğrisi",
            "max" => "Maksimum",
            "max_desc" => "Tam devir soğutma",
            "custom" => "Özel",
            "custom_desc" => "Kullanıcı fan profili",
            "fans" => "FANLAR",
            "battery" => "PİL",
            "close" => "Kapat",
            _ => key,
        },
        "es" => match key {
            "title" => "CONTROL RÁPIDO OMEN",
            "perf_mode" => "Modo de Rendimiento",
            "quiet" => "Silencioso",
            "eco_silent" => "Eco / Silencioso",
            "default" => "Predeterminado",
            "balanced" => "Equilibrado",
            "performance" => "Rendimiento",
            "max_power" => "Máxima Potencia",
            "fan_mode" => "Modo de Ventilador",
            "auto" => "Automático",
            "auto_desc" => "Curva térmica adaptativa",
            "max" => "Máximo (100% Turbo)",
            "max_desc" => "Refrigeración a máxima velocidad",
            "custom" => "Personalizado",
            "custom_desc" => "Perfil de curva de usuario",
            "fans" => "VENTILADORES",
            "battery" => "BATERÍA",
            "close" => "Cerrar",
            _ => key,
        },
        _ => match key {
            "title" => "OMEN QUICK CONTROL",
            "perf_mode" => "Performance Mode",
            "quiet" => "Quiet",
            "eco_silent" => "Eco / Silent",
            "default" => "Default",
            "balanced" => "Balanced",
            "performance" => "Performance",
            "max_power" => "Max Power & Clock",
            "fan_mode" => "Fan Mode",
            "auto" => "Auto (Dynamic)",
            "auto_desc" => "Adaptive thermal curve",
            "max" => "Max (100% Turbo)",
            "max_desc" => "Full speed cooling",
            "custom" => "Custom Preset",
            "custom_desc" => "User curve profile",
            "fans" => "FANS",
            "battery" => "BATTERY",
            "close" => "Close",
            _ => key,
        },
    }
    .to_string()
}

