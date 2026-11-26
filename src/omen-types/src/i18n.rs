#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Tr,
    En,
    Es,
}

/// Parses a language or POSIX locale identifier (e.g. `"tr"`, `"tr_TR.UTF-8"`,
/// `"es-ES"`, `"en_US"`, `"turkish"`, `"spanish"`, `"english"`) into a [`Lang`].
/// Returns `None` for `"auto"`, empty strings, or unrecognized locales.
pub fn parse_lang_code(raw: &str) -> Option<Lang> {
    let lower = raw.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return None;
    }
    if lower == "tr"
        || lower.starts_with("tr_")
        || lower.starts_with("tr-")
        || lower.starts_with("tr.")
        || lower.contains("turkish")
    {
        Some(Lang::Tr)
    } else if lower == "en"
        || lower.starts_with("en_")
        || lower.starts_with("en-")
        || lower.starts_with("en.")
        || lower.contains("english")
    {
        Some(Lang::En)
    } else if lower == "es"
        || lower.starts_with("es_")
        || lower.starts_with("es-")
        || lower.starts_with("es.")
        || lower.contains("spanish")
    {
        Some(Lang::Es)
    } else {
        None
    }
}

pub(crate) fn detect_env_lang_with<F>(lookup: F) -> Lang
where
    F: Fn(&str) -> Option<String>,
{
    for var in ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"] {
        let Some(val) = lookup(var) else {
            continue;
        };
        let trimmed = val.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_ascii_lowercase();
        if lower == "c" || lower == "posix" || lower.starts_with("c.") {
            return Lang::En;
        }
        for part in trimmed.split(':') {
            if let Some(lang) = parse_lang_code(part) {
                return lang;
            }
        }
    }
    Lang::En
}

/// Detects the active locale from `LC_ALL`, `LC_MESSAGES`, `LANG`, and `LANGUAGE`,
/// defaulting to [`Lang::En`].
pub fn detect_env_lang() -> Lang {
    detect_env_lang_with(|k| std::env::var(k).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn parses_supported_lang_codes_and_locale_strings() {
        for s in ["tr", "TR", "tr_TR", "tr_TR.UTF-8", "tr-TR", "Turkish"] {
            assert_eq!(parse_lang_code(s), Some(Lang::Tr), "failed for {s}");
        }
        for s in ["en", "EN", "en_US", "en_GB.UTF-8", "en-US", "English"] {
            assert_eq!(parse_lang_code(s), Some(Lang::En), "failed for {s}");
        }
        for s in ["es", "ES", "es_ES", "es_MX.UTF-8", "es-419", "Spanish"] {
            assert_eq!(parse_lang_code(s), Some(Lang::Es), "failed for {s}");
        }
        for s in ["", "   ", "auto", "C", "POSIX", "fr_FR.UTF-8", "de"] {
            assert_eq!(parse_lang_code(s), None, "expected None for {s}");
        }
    }

    #[test]
    fn env_detection_respects_precedence_and_defaults_to_english() {
        let make_lookup = |pairs: &[(&str, &str)]| {
            let map: HashMap<String, String> = pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            move |k: &str| map.get(k).cloned()
        };

        assert_eq!(detect_env_lang_with(make_lookup(&[])), Lang::En);
        assert_eq!(
            detect_env_lang_with(make_lookup(&[("LANG", "fr_FR.UTF-8")])),
            Lang::En
        );
        assert_eq!(
            detect_env_lang_with(make_lookup(&[("LANG", "tr_TR.UTF-8")])),
            Lang::Tr
        );
        assert_eq!(
            detect_env_lang_with(make_lookup(&[
                ("LC_MESSAGES", "es_ES.UTF-8"),
                ("LANG", "tr_TR.UTF-8"),
            ])),
            Lang::Es
        );
        assert_eq!(
            detect_env_lang_with(make_lookup(&[
                ("LC_ALL", "en_US.UTF-8"),
                ("LC_MESSAGES", "tr_TR.UTF-8"),
                ("LANG", "es_ES.UTF-8"),
            ])),
            Lang::En
        );
        assert_eq!(
            detect_env_lang_with(make_lookup(&[
                ("LC_ALL", ""),
                ("LC_MESSAGES", "tr_TR.UTF-8"),
            ])),
            Lang::Tr
        );
        assert_eq!(
            detect_env_lang_with(make_lookup(&[("LC_ALL", "C"), ("LANG", "tr_TR.UTF-8"),])),
            Lang::En
        );
        assert_eq!(
            detect_env_lang_with(make_lookup(&[("LANGUAGE", "fr:es:en")])),
            Lang::Es
        );
    }

    #[test]
    fn detect_env_lang_reads_process_environment() {
        let _guard = ENV_LOCK.lock().unwrap();
        let vars = ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"];
        let saved: Vec<(&str, Option<String>)> =
            vars.iter().map(|&k| (k, std::env::var(k).ok())).collect();

        unsafe {
            for k in vars {
                std::env::remove_var(k);
            }
            std::env::set_var("LANG", "tr_TR.UTF-8");
        }
        assert_eq!(detect_env_lang(), Lang::Tr);

        unsafe {
            std::env::set_var("LC_ALL", "es_ES.UTF-8");
        }
        assert_eq!(detect_env_lang(), Lang::Es);

        unsafe {
            for (k, v) in saved {
                match v {
                    Some(val) => std::env::set_var(k, val),
                    None => std::env::remove_var(k),
                }
            }
        }
    }
}
