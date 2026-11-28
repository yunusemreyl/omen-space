use omen_types::i18n::{detect_env_lang, parse_lang_code, Lang};
use std::env;
use std::fs;

fn get_lang() -> Lang {
    if let Ok(home) = env::var("HOME") {
        let path = format!("{}/.config/omenspace/gui_config.json", home);
        if let Ok(json_str) = fs::read_to_string(&path) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&json_str) {
                if let Some(lang) = json.get("language").and_then(|v| v.as_str()) {
                    if let Some(parsed) = parse_lang_code(lang) {
                        return parsed;
                    }
                }
            }
        }
    }

    detect_env_lang()
}

pub fn t(key: &str) -> String {
    let lang = get_lang();

    match lang {
        Lang::Tr => match key {
            "title" => "OMEN Hızlı Kontrol",
            "perf_mode" => "Performans Modu",
            "quiet" => "Sessiz",
            "eco_silent" => "Eko / Sessiz",
            "default" => "Varsayılan",
            "balanced" => "Dengeli",
            "performance" => "Performans",
            "max_power" => "Maksimum Güç",
            "fan_mode" => "Fan Modu",
            "auto" => "Otomatik",
            "auto_desc" => "Dinamik soğutma",
            "max" => "Maksimum",
            "max_desc" => "Tam devir",
            "custom" => "Özel",
            "custom_desc" => "Kullanıcı eğrisi",
            "fans" => "Fanlar",
            "battery" => "Pil",
            "close" => "Kapat",
            "gpu_idle" => "Boşta",
            _ => key,
        },
        Lang::Es => match key {
            "title" => "Control Rápido OMEN",
            "perf_mode" => "Modo de rendimiento",
            "quiet" => "Silencioso",
            "eco_silent" => "Eco / Silencioso",
            "default" => "Equilibrado",
            "balanced" => "Uso diario",
            "performance" => "Rendimiento",
            "max_power" => "Máxima potencia",
            "fan_mode" => "Modo de ventilador",
            "auto" => "Automático",
            "auto_desc" => "Curva adaptativa",
            "max" => "Máximo",
            "max_desc" => "Velocidad máxima",
            "custom" => "Personalizado",
            "custom_desc" => "Curva de usuario",
            "fans" => "Ventiladores",
            "battery" => "Batería",
            "close" => "Cerrar",
            "gpu_idle" => "Inactiva",
            _ => key,
        },
        Lang::En => match key {
            "title" => "OMEN Quick Control",
            "perf_mode" => "Performance Mode",
            "quiet" => "Quiet",
            "eco_silent" => "Eco / Silent",
            "default" => "Balanced",
            "balanced" => "Everyday use",
            "performance" => "Performance",
            "max_power" => "Max power",
            "fan_mode" => "Fan Mode",
            "auto" => "Auto",
            "auto_desc" => "Adaptive curve",
            "max" => "Max",
            "max_desc" => "Full speed",
            "custom" => "Custom",
            "custom_desc" => "User curve",
            "fans" => "Fans",
            "battery" => "Battery",
            "close" => "Close",
            "gpu_idle" => "Idle",
            _ => key,
        },
    }
    .to_string()
}
