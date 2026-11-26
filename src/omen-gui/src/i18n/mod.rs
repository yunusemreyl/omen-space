use omen_types::i18n::{detect_env_lang, parse_lang_code, Lang};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::RwLock;

mod en;
mod es;
mod tr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "tr")]
    Tr,
    #[serde(rename = "en")]
    En,
    #[serde(rename = "es")]
    Es,
}

impl From<Lang> for Language {
    fn from(lang: Lang) -> Self {
        match lang {
            Lang::Tr => Language::Tr,
            Lang::En => Language::En,
            Lang::Es => Language::Es,
        }
    }
}

impl Language {
    pub fn display_name(&self) -> &'static str {
        match self {
            Language::Auto => "Sistem Dili (Otomatik) / System Default",
            Language::Tr => "Türkçe (TR)",
            Language::En => "English (EN)",
            Language::Es => "Español (ES)",
        }
    }

    pub fn to_code(self) -> &'static str {
        match self {
            Language::Auto => "auto",
            Language::Tr => "tr",
            Language::En => "en",
            Language::Es => "es",
        }
    }

    pub fn from_index(index: u32) -> Self {
        match index {
            1 => Language::Tr,
            2 => Language::En,
            3 => Language::Es,
            _ => Language::Auto,
        }
    }

    pub fn to_index(self) -> u32 {
        match self {
            Language::Auto => 0,
            Language::Tr => 1,
            Language::En => 2,
            Language::Es => 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GuiConfig {
    #[serde(default = "default_language")]
    language: String,
}

fn default_language() -> String {
    "auto".to_string()
}

fn get_config_path() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("HOME") {
        let mut p = PathBuf::from(home);
        p.push(".config");
        p.push("omenspace");
        p.push("gui_config.json");
        Some(p)
    } else {
        None
    }
}

fn detect_system_language() -> Language {
    detect_env_lang().into()
}

struct I18nState {
    selected_language: Language,
    active_language: Language,
}

static I18N_STATE: RwLock<I18nState> = RwLock::new(I18nState {
    selected_language: Language::Auto,
    active_language: Language::En,
});

pub fn init() {
    let mut selected = Language::Auto;
    if let Some(path) = get_config_path() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(cfg) = serde_json::from_str::<GuiConfig>(&content) {
                selected = parse_lang_code(&cfg.language)
                    .map(Language::from)
                    .unwrap_or(Language::Auto);
            }
        }
    }

    let active = match selected {
        Language::Auto => detect_system_language(),
        other => other,
    };

    if let Ok(mut state) = I18N_STATE.write() {
        state.selected_language = selected;
        state.active_language = active;
    }
}

pub fn get_selected_language() -> Language {
    I18N_STATE
        .read()
        .map(|s| s.selected_language)
        .unwrap_or(Language::Auto)
}

pub fn get_active_language() -> Language {
    I18N_STATE
        .read()
        .map(|s| s.active_language)
        .unwrap_or(Language::En)
}

pub fn set_language(lang: Language) {
    let active = match lang {
        Language::Auto => detect_system_language(),
        other => other,
    };

    if let Ok(mut state) = I18N_STATE.write() {
        state.selected_language = lang;
        state.active_language = active;
    }

    // Save config
    if let Some(path) = get_config_path() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let cfg = GuiConfig {
            language: lang.to_code().to_string(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&cfg) {
            let _ = fs::write(path, json);
        }
    }
}

fn translate_for(lang: Language, key: &'static str) -> &'static str {
    match lang {
        Language::Tr => tr::translate(key)
            .or_else(|| en::translate(key))
            .unwrap_or(key),
        Language::Es => es::translate(key)
            .or_else(|| en::translate(key))
            .unwrap_or(key),
        _ => en::translate(key).unwrap_or(key),
    }
}

/// Translate key using active language
pub fn t(key: &'static str) -> &'static str {
    translate_for(get_active_language(), key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn collect_and_validate(
        lang_name: &str,
        entries: &[(&'static str, &'static str)],
    ) -> BTreeSet<&'static str> {
        let mut keys = BTreeSet::new();
        let mut duplicates = Vec::new();
        let mut empty_values = Vec::new();

        for &(k, v) in entries {
            if !keys.insert(k) {
                duplicates.push(k);
            }
            if v.trim().is_empty() {
                empty_values.push(k);
            }
        }

        assert!(
            duplicates.is_empty(),
            "[{lang_name}] duplicate translation keys: {duplicates:?}"
        );
        assert!(
            empty_values.is_empty(),
            "[{lang_name}] empty translation values for keys: {empty_values:?}"
        );
        keys
    }

    #[test]
    fn translations_have_full_key_parity_and_no_empty_or_duplicate_entries() {
        let en_keys = collect_and_validate("en", en::ENTRIES);
        let tr_keys = collect_and_validate("tr", tr::ENTRIES);
        let es_keys = collect_and_validate("es", es::ENTRIES);

        let missing_in_tr: Vec<_> = en_keys.difference(&tr_keys).copied().collect();
        let extra_in_tr: Vec<_> = tr_keys.difference(&en_keys).copied().collect();
        assert!(
            missing_in_tr.is_empty() && extra_in_tr.is_empty(),
            "tr parity mismatch — missing in tr: {missing_in_tr:?}, extra in tr: {extra_in_tr:?}"
        );

        let missing_in_es: Vec<_> = en_keys.difference(&es_keys).copied().collect();
        let extra_in_es: Vec<_> = es_keys.difference(&en_keys).copied().collect();
        assert!(
            missing_in_es.is_empty() && extra_in_es.is_empty(),
            "es parity mismatch — missing in es: {missing_in_es:?}, extra in es: {extra_in_es:?}"
        );
    }

    #[test]
    fn lookup_resolves_known_keys_and_falls_back_for_unknown() {
        assert_eq!(translate_for(Language::En, "nav_settings"), "Settings");
        assert_eq!(translate_for(Language::Tr, "nav_settings"), "Ayarlar");
        assert_eq!(translate_for(Language::Es, "nav_settings"), "Ajustes");
        assert_eq!(
            translate_for(Language::En, "nonexistent_key_xyz"),
            "nonexistent_key_xyz"
        );
    }
}
