//! Language resolution shared by the MCP server and `vox init`.
//!
//! Order of precedence: explicit flag, then the stored `lang` preference,
//! then the system locale. When nothing resolves to a supported language we
//! return `None` and callers tell the agent to match the user's own language
//! rather than forcing one.

use crate::config::SUPPORTED_LANGS;
use crate::db;

/// English name of a supported language code, for use inside English prompts.
pub fn lang_name(code: &str) -> Option<&'static str> {
    Some(match code {
        "en" => "English",
        "fr" => "French",
        "es" => "Spanish",
        "de" => "German",
        "it" => "Italian",
        "pt" => "Portuguese",
        "zh" => "Chinese",
        "ja" => "Japanese",
        "ko" => "Korean",
        "ru" => "Russian",
        "ar" => "Arabic",
        "nl" => "Dutch",
        _ => return None,
    })
}

/// Short "task finished" phrase used by the Stop hook, in the given language.
/// Falls back to English for anything unknown.
pub fn done_phrase(lang: Option<&str>) -> &'static str {
    match lang {
        Some("fr") => "Terminé.",
        Some("es") => "Listo.",
        Some("de") => "Fertig.",
        Some("it") => "Fatto.",
        Some("pt") => "Pronto.",
        Some("zh") => "完成。",
        Some("ja") => "完了しました。",
        Some("ko") => "완료했습니다.",
        Some("ru") => "Готово.",
        Some("ar") => "تم.",
        Some("nl") => "Klaar.",
        _ => "Done.",
    }
}

/// Reduce a locale such as `fr-FR` or `zh_Hant` to a supported language code.
pub fn normalize_locale(locale: &str) -> Option<String> {
    let lang = locale.split(['-', '_']).next()?.to_lowercase();
    SUPPORTED_LANGS.contains(&lang.as_str()).then_some(lang)
}

/// The `lang` preference stored in the database, if any.
pub fn configured_lang() -> Option<String> {
    db::open()
        .ok()
        .and_then(|conn| db::get_preferences(&conn).ok())
        .and_then(|prefs| prefs.lang)
        .filter(|lang| !lang.is_empty())
}

/// The system locale, when it maps to a language vox supports.
pub fn system_lang() -> Option<String> {
    normalize_locale(&sys_locale::get_locale()?)
}

/// Stored preference, else system locale.
pub fn default_lang() -> Option<String> {
    configured_lang().or_else(system_lang)
}

/// Resolve the language for a command: explicit flag wins, then the stored
/// preference, then the system locale. An unsupported explicit code is an
/// error so the user finds out instead of silently getting another language.
pub fn resolve(explicit: Option<&str>) -> anyhow::Result<Option<String>> {
    match explicit.map(str::trim).filter(|s| !s.is_empty()) {
        Some(code) => {
            let code = code.to_lowercase();
            if !SUPPORTED_LANGS.contains(&code.as_str()) {
                anyhow::bail!(
                    "Unsupported language '{code}'. Supported: {}",
                    SUPPORTED_LANGS.join(", ")
                );
            }
            Ok(Some(code))
        }
        None => Ok(default_lang()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_locale_maps_primary_subtag_when_supported() {
        assert_eq!(normalize_locale("fr-FR"), Some("fr".to_string()));
        assert_eq!(normalize_locale("en_US"), Some("en".to_string()));
        assert_eq!(normalize_locale("de-AT"), Some("de".to_string()));
        assert_eq!(normalize_locale("FR"), Some("fr".to_string()));
        assert_eq!(normalize_locale("zh-Hant"), Some("zh".to_string()));
    }

    #[test]
    fn normalize_locale_rejects_unsupported_or_empty() {
        assert_eq!(normalize_locale("cy-GB"), None);
        assert_eq!(normalize_locale("C"), None);
        assert_eq!(normalize_locale(""), None);
    }

    #[test]
    fn system_lang_is_supported_or_none() {
        assert!(system_lang().is_none_or(|lang| SUPPORTED_LANGS.contains(&lang.as_str())));
    }

    #[test]
    fn every_supported_lang_has_a_name_and_phrase() {
        for code in SUPPORTED_LANGS {
            assert!(lang_name(code).is_some(), "no name for {code}");
            assert!(!done_phrase(Some(code)).is_empty(), "no phrase for {code}");
        }
        assert_eq!(lang_name("xx"), None);
    }

    #[test]
    fn done_phrase_defaults_to_english() {
        assert_eq!(done_phrase(None), "Done.");
        assert_eq!(done_phrase(Some("xx")), "Done.");
        assert_eq!(done_phrase(Some("fr")), "Terminé.");
        assert_eq!(done_phrase(Some("ja")), "完了しました。");
    }

    #[test]
    fn resolve_accepts_supported_and_normalizes_case() {
        assert_eq!(resolve(Some("de")).unwrap(), Some("de".to_string()));
        assert_eq!(resolve(Some(" JA ")).unwrap(), Some("ja".to_string()));
    }

    #[test]
    fn resolve_rejects_unsupported_language() {
        let err = resolve(Some("klingon")).unwrap_err().to_string();
        assert!(err.contains("Unsupported language"));
        assert!(err.contains("en, fr"));
    }

    #[test]
    fn resolve_without_flag_falls_back_to_environment() {
        // Whatever it resolves to, it must be a supported language or nothing.
        assert!(
            resolve(None)
                .unwrap()
                .is_none_or(|l| SUPPORTED_LANGS.contains(&l.as_str()))
        );
    }
}
