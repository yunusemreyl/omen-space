use omen_types::i18n::{detect_env_lang, parse_lang_code, Lang};
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Auto,
    Tr,
    En,
}

impl From<Lang> for Language {
    fn from(lang: Lang) -> Self {
        match lang {
            Lang::Tr => Language::Tr,
            Lang::En | Lang::Es => Language::En,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct GuiConfig {
    #[serde(default)]
    language: String,
}

pub fn get_language() -> Language {
    let mut selected = Language::Auto;
    if let Some(home) = std::env::var_os("HOME") {
        let mut p = PathBuf::from(home);
        p.push(".config");
        p.push("omenspace");
        p.push("gui_config.json");

        if let Ok(content) = fs::read_to_string(&p) {
            if let Ok(cfg) = serde_json::from_str::<GuiConfig>(&content) {
                selected = parse_lang_code(&cfg.language)
                    .map(Language::from)
                    .unwrap_or(Language::Auto);
            }
        }
    }

    if selected == Language::Auto {
        detect_env_lang().into()
    } else {
        selected
    }
}

pub fn t(key: &'static str) -> &'static str {
    let lang = get_language();
    match lang {
        Language::Tr => translate_tr(key),
        _ => translate_en(key),
    }
}

fn translate_tr(key: &'static str) -> &'static str {
    match key {
        "ready" => "Hazır. Aşağıya bir komut yazın (örn: 'fan max', 'perf eco')",
        "os" => "İşletim Sistemi",
        "kernel" => "Çekirdek",
        "host" => "Cihaz",
        "power_profile" => "Güç Modu",
        "thermal_fans" => "Sıcaklık/Fanlar",
        "gpu_mux" => "Ekran Kartı MUX",
        "battery_care" => "Pil Sınırı",
        "limit" => "Sınır",
        "conflicts" => "Çakışmalar",
        "clean" => "Temiz",
        "warning" => "Uyarı",
        "usage_fan" => "Kullanım: fan auto | ec | max | <0-100>",
        "usage_perf" => "Kullanım: perf performance | balanced | eco | <pl1 pl2>",
        "usage_mux" => "Kullanım: mux hybrid | discrete | advanced",
        "usage_bat" => "Kullanım: bat <50-100>",

        "fan_changed" => "Fan modu '{}' olarak değiştirildi",
        "fan_error" => "Fan hatası:",
        "fan_set" => "Fan hızı %{} olarak ayarlandı (Hedef: {} RPM)",
        "fan_set_failed" => "Fan hızı ayarlanamadı",
        "fan_invalid" => "Geçersiz fan modu:",
        "fan_no_service" => "Fan D-Bus servisi kullanılamıyor",

        "perf_perf" => "Güç profili PERFORMANS olarak ayarlandı",
        "perf_bal" => "Güç profili DENGELİ olarak ayarlandı",
        "perf_eco" => "Güç profili TASARRUF olarak ayarlandı",
        "perf_limits" => "CPU Güç Sınırları ayarlandı: PL1={}W / PL2={}W",
        "perf_error" => "Güç profili hatası:",
        "perf_no_service" => "Güç D-Bus servisi kullanılamıyor",

        "bat_set" => "Pil Şarj Sınırı %{} olarak ayarlandı",
        "bat_no_service" => "Platform D-Bus servisi kullanılamıyor",

        "mux_set" => "GPU MUX Modu '{}' olarak ayarlandı [{}]",
        "mux_error" => "MUX hatası:",
        "mux_invalid" => "Geçersiz MUX modu:",
        "mux_no_service" => "MUX D-Bus servisi kullanılamıyor",

        "uv_set" => "CPU Undervolt Ofseti {}mV olarak ayarlandı",
        "uv_error" => "Undervolt hatası:",

        "rgb_off" => "Klavye RGB aydınlatması KAPATILDI",
        "rgb_on" => "Klavye RGB aydınlatması AÇILDI",
        "rgb_color" => "Klavye RGB rengi #{} olarak ayarlandı",
        "rgb_error" => "RGB hatası:",
        "rgb_no_service" => "RGB D-Bus servisi kullanılamıyor",

        "cache_cleared" => "Bellek (Page Cache) temizlendi",

        "diag_initiated" => "WMI Teşhisi başlatıldı",
        "diag_bundle" => "Teşhis paketi oluşturuldu: {}",

        "executed" => "'{}' çalıştırıldı",

        _ => key,
    }
}

fn translate_en(key: &'static str) -> &'static str {
    match key {
        "ready" => "Ready. Type command below (e.g. 'fan max', 'perf eco')",
        "os" => "OS",
        "kernel" => "Kernel",
        "host" => "Host",
        "power_profile" => "Power Profile",
        "thermal_fans" => "Thermal/Fans",
        "gpu_mux" => "GPU MUX",
        "battery_care" => "Battery Care",
        "limit" => "Limit",
        "conflicts" => "Conflicts",
        "clean" => "Clean",
        "warning" => "Warning",
        "usage_fan" => "Usage: fan auto | ec | max | <0-100>",
        "usage_perf" => "Usage: perf performance | balanced | eco | <pl1 pl2>",
        "usage_mux" => "Usage: mux hybrid | discrete | advanced",
        "usage_bat" => "Usage: bat <50-100>",

        "fan_changed" => "Fan mode changed to '{}'",
        "fan_error" => "Fan error:",
        "fan_set" => "Fan speed set to {}% (Target: {} RPM)",
        "fan_set_failed" => "Fan speed set failed",
        "fan_invalid" => "Invalid fan mode:",
        "fan_no_service" => "Fan D-Bus service unavailable",

        "perf_perf" => "Power profile set to PERFORMANCE",
        "perf_bal" => "Power profile set to BALANCED",
        "perf_eco" => "Power profile set to ECO / POWER SAVER",
        "perf_limits" => "CPU Power Limits set to PL1={}W / PL2={}W",
        "perf_error" => "Power error:",
        "perf_no_service" => "Power D-Bus service unavailable",

        "bat_set" => "Battery Charge Limit set to {}%",
        "bat_no_service" => "Platform D-Bus service unavailable",

        "mux_set" => "GPU MUX Mode set to '{}' [{}]",
        "mux_error" => "MUX error:",
        "mux_invalid" => "Invalid MUX mode:",
        "mux_no_service" => "MUX D-Bus service unavailable",

        "uv_set" => "CPU Undervolt Offset set to {}mV",
        "uv_error" => "Undervolt error:",

        "rgb_off" => "Keyboard RGB turned OFF",
        "rgb_on" => "Keyboard RGB turned ON",
        "rgb_color" => "Keyboard RGB Color set to #{}",
        "rgb_error" => "RGB error:",
        "rgb_no_service" => "RGB D-Bus service unavailable",

        "cache_cleared" => "Memory page cache cleared",

        "diag_initiated" => "WMI Diagnostics initiated",
        "diag_bundle" => "Triage bundle created: {}",

        "executed" => "Executed '{}'",

        _ => key,
    }
}
