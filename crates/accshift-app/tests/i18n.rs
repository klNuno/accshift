//! The TS i18n tests (`src/lib/i18n/{index,messages}.test.ts`) against the
//! embedded dictionaries, plus the export check: every key of the TS message
//! files must be in the JSON `tools/i18n.dump.ts` wrote. The JSON may hold
//! more: the egui app's own strings.

use accshift_app::i18n::{
    boot_locale, detect_preferred_locale, dictionary, format_template, is_locale, normalize_locale,
    translate, translate_with, Locale, TranslationValue, DEFAULT_LOCALE, LANGUAGE_OPTIONS, LOCALES,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

// ------------------------------------------------------------ isLocale, normalizeLocale

#[test]
fn is_locale_accepts_en() {
    assert!(is_locale(Some(&json!("en"))));
}

#[test]
fn is_locale_accepts_fr() {
    assert!(is_locale(Some(&json!("fr"))));
}

#[test]
fn is_locale_rejects_de() {
    assert!(!is_locale(Some(&json!("de"))));
}

#[test]
fn is_locale_rejects_null() {
    assert!(!is_locale(Some(&Value::Null)));
    assert!(!is_locale(None));
}

#[test]
fn is_locale_rejects_a_number() {
    assert!(!is_locale(Some(&json!(42))));
}

#[test]
fn normalize_locale_passes_a_valid_locale_through() {
    assert_eq!(normalize_locale(Some(&json!("fr"))), Locale::Fr);
}

#[test]
fn normalize_locale_defaults_an_invalid_one_to_en() {
    assert_eq!(normalize_locale(Some(&json!("xx"))), Locale::En);
}

#[test]
fn normalize_locale_defaults_null_to_en() {
    assert_eq!(normalize_locale(Some(&Value::Null)), Locale::En);
}

// ------------------------------------------------------------ translate

#[test]
fn translates_a_known_key_in_en() {
    let result = translate(Locale::En, "common.close");
    assert!(!result.is_empty());
    assert_ne!(result, "common.close");
}

#[test]
fn interpolates_parameters() {
    let result = translate_with(
        Locale::En,
        "time.minutesAgo",
        &[("count", TranslationValue::from(5))],
    );
    assert!(result.contains('5'), "{result}");
}

#[test]
fn serves_fr_strings() {
    assert_eq!(translate(Locale::Fr, "common.close"), "Fermer");
}

#[test]
fn an_unknown_key_renders_as_itself() {
    assert_eq!(translate(Locale::Fr, "no.such.key"), "no.such.key");
}

#[test]
fn format_template_empties_missing_params_and_renders_values() {
    assert_eq!(
        format_template(
            "{a} {b} {missing}",
            Some(&[
                ("a", TranslationValue::from("x")),
                ("b", TranslationValue::from(true))
            ])
        ),
        "x true "
    );
    assert_eq!(format_template("{a}", None), "{a}");
}

// ------------------------------------------------------------ detectPreferredLocale

#[test]
fn detect_returns_an_exact_match() {
    assert_eq!(detect_preferred_locale(&["fr"]), Locale::Fr);
}

#[test]
fn detect_falls_back_from_a_regional_tag_to_its_base() {
    assert_eq!(detect_preferred_locale(&["fr-CA"]), Locale::Fr);
}

#[test]
fn detect_defaults_unsupported_locales_to_en() {
    assert_eq!(detect_preferred_locale(&["de-DE"]), Locale::En);
}

#[test]
fn detect_prefers_the_regional_dictionary_when_both_ship() {
    assert_eq!(detect_preferred_locale(&["pt-BR"]), Locale::PtBr);
}

#[test]
fn detect_maps_a_regional_tag_without_a_dictionary_to_its_base() {
    assert_eq!(detect_preferred_locale(&["pt-PT"]), Locale::Pt);
    assert_eq!(detect_preferred_locale(&["es-419"]), Locale::Es);
    assert_eq!(detect_preferred_locale(&["zh-CN"]), Locale::Zh);
}

#[test]
fn detect_defaults_to_en_with_no_languages() {
    assert_eq!(detect_preferred_locale::<&str>(&[]), Locale::En);
}

#[test]
fn detect_skips_an_unsupported_entry_and_matches_a_later_one() {
    assert_eq!(detect_preferred_locale(&["de-DE", "en-GB"]), Locale::En);
}

#[test]
fn boot_locale_reads_stored_settings_then_the_os() {
    let stored = json!({ "language": "ru" });
    assert_eq!(boot_locale(Some(&stored), &["fr-FR"]), Locale::Ru);
    assert_eq!(
        boot_locale(Some(&json!({ "language": "xx" })), &["fr-FR"]),
        Locale::En
    );
    assert_eq!(boot_locale(None, &["fr-FR"]), Locale::Fr);
    assert_eq!(boot_locale(Some(&json!({})), &["zh-CN"]), Locale::Zh);
}

// ------------------------------------------------------------ messages

fn placeholders(value: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = value;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let len = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(after.len());
        if len > 0 && after[len..].starts_with('}') {
            found.push(format!("{{{}}}", &after[..len]));
        }
        rest = after;
    }
    found.sort();
    found
}

fn keys(locale: Locale) -> BTreeSet<&'static str> {
    dictionary(locale).keys().map(String::as_str).collect()
}

#[test]
fn has_a_dictionary_for_every_shipped_locale() {
    let shipped: BTreeSet<&str> = LANGUAGE_OPTIONS
        .iter()
        .map(|option| option.code.code())
        .collect();
    let registered: BTreeSet<&str> = LOCALES.iter().map(|locale| locale.code()).collect();
    assert_eq!(registered, shipped);
    assert_eq!(DEFAULT_LOCALE, Locale::En);
}

#[test]
fn every_locale_covers_exactly_the_en_key_set() {
    let en = keys(Locale::En);
    for locale in LOCALES {
        assert_eq!(keys(locale), en, "{}", locale.code());
    }
}

#[test]
fn every_locale_keeps_the_en_placeholders() {
    let en = dictionary(Locale::En);
    for locale in LOCALES {
        let dict = dictionary(locale);
        for (key, value) in en {
            assert_eq!(
                placeholders(dict.get(key).map_or("", String::as_str)),
                placeholders(value),
                "placeholders in {}:{key}",
                locale.code()
            );
        }
    }
}

#[test]
fn no_locale_has_an_em_or_en_dash() {
    for locale in LOCALES {
        for (key, value) in dictionary(locale) {
            assert!(
                !value.contains(['\u{2013}', '\u{2014}']),
                "dash in {}:{key}",
                locale.code()
            );
        }
    }
}

#[test]
fn no_locale_has_an_empty_string() {
    for locale in LOCALES {
        for (key, value) in dictionary(locale) {
            assert!(
                !value.trim().is_empty(),
                "empty message for {}:{key}",
                locale.code()
            );
        }
    }
}

// ------------------------------------------------------------ export check

fn ts_file(locale: Locale) -> &'static str {
    match locale {
        Locale::En => "messages.ts",
        Locale::Es => "messages.es.ts",
        Locale::Fr => "messages.fr.ts",
        Locale::Pt => "messages.pt.ts",
        Locale::PtBr => "messages.pt-br.ts",
        Locale::Ru => "messages.ru.ts",
        Locale::Zh => "messages.zh.ts",
    }
}

/// The keys a TS message file declares: every entry line is two spaces, a
/// quoted key and a colon.
fn ts_keys(source: &str) -> BTreeSet<String> {
    source
        .lines()
        .filter_map(|line| line.strip_prefix("  \""))
        .filter_map(|rest| rest.split_once("\":"))
        .map(|(key, _)| key.to_string())
        .collect()
}

#[test]
fn every_ts_key_is_in_the_exported_json() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src/lib/i18n");
    let mut counts = Vec::new();
    for locale in LOCALES {
        let path = dir.join(ts_file(locale));
        let source = std::fs::read_to_string(&path).expect("TS message file");
        let declared = ts_keys(&source);
        assert!(
            !declared.is_empty(),
            "no key parsed from {}",
            path.display()
        );
        let exported = keys(locale);
        let missing: Vec<&String> = declared
            .iter()
            .filter(|key| !exported.contains(key.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "{}: keys missing from i18n/{}.json, rerun tools/i18n.dump.ts: {missing:?}",
            locale.code(),
            locale.code()
        );
        counts.push(format!("{} {}", locale.code(), exported.len()));
    }
    eprintln!("i18n keys: {}", counts.join(", "));
}
