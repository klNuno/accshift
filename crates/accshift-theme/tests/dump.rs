//! Compares the Rust engine with what the real TypeScript wrote on `<html>`
//! for every case in `tests/data` (see `tools/variables.dump.ts`).

use std::fs;
use std::path::PathBuf;

use accshift_theme::{
    theme_application, CustomThemePayload, ResolvedTheme, ThemeRegistry, TokenKey, UnsupportedCss,
};
use serde_json::Value;

struct Dump {
    file: String,
    value: Value,
}

fn dumps() -> Vec<Dump> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    let mut dumps: Vec<Dump> = fs::read_dir(&dir)
        .expect("tests/data exists; run the dump script")
        .map(|entry| entry.expect("readable entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .map(|path| Dump {
            file: path.file_name().unwrap().to_string_lossy().into_owned(),
            value: serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap(),
        })
        .collect();
    dumps.sort_by(|a, b| a.file.cmp(&b.file));
    dumps
}

/// Every custom payload of the dump, registered together as the script did.
fn registry(dumps: &[Dump]) -> ThemeRegistry {
    let payloads: Vec<CustomThemePayload> = dumps
        .iter()
        .filter(|dump| !dump.value["payload"].is_null())
        .map(|dump| serde_json::from_value(dump.value["payload"].clone()).unwrap())
        .collect();
    let mut registry = ThemeRegistry::new();
    registry.apply_custom_theme_payloads(&payloads);
    registry
}

/// Variables whose value is a colour once resolved.
const COLOR_VARS: &[&str] = &[
    "--bg-solid",
    "--bg-input",
    "--bg-card",
    "--bg-card-hover",
    "--bg-muted",
    "--bg-elevated",
    "--bg-overlay",
    "--fg",
    "--fg-muted",
    "--fg-subtle",
    "--border",
    "--danger",
    "--afk-text",
    "--accent",
    "--accent-fg",
    "--success",
    "--warning",
    "--bg",
    "--accent-text",
];
const LENGTH_VARS: &[&str] = &[
    "--radius-sm",
    "--radius-md",
    "--radius-lg",
    "--border-width",
    "--overlay-blur",
    "--letter-spacing",
    "--avatar-radius",
    "--grid-card-width",
    "--grid-card-min-height",
    "--grid-card-padding",
    "--grid-card-radius",
    "--grid-card-avatar-size",
];
const NUMBER_VARS: &[&str] = &[
    "--bg-opacity",
    "--density-scale",
    "--line-height",
    "--motion-scale",
    "--z-menu",
];
const DURATION_VARS: &[&str] = &[
    "--motion-card-entrance",
    "--motion-page-entrance",
    "--motion-avatar-fade",
    "--motion-avatar-defer",
];
const SHADOW_VARS: &[&str] = &["--elevation-low", "--elevation-medium", "--elevation-high"];

#[test]
fn every_dumped_case_matches_the_typescript() {
    let dumps = dumps();
    assert!(
        dumps.len() >= 11,
        "expected the 6 built-ins and 5 custom themes"
    );
    let registry = registry(&dumps);

    let mut cases = 0;
    let mut matched = 0;
    let mut unsupported: Vec<UnsupportedCss> = Vec::new();
    for dump in &dumps {
        let id = dump.value["id"].as_str().unwrap();
        let theme = registry.theme(Some(id));
        assert_eq!(theme.id, id, "{}", dump.file);
        assert_eq!(
            theme.is_custom,
            dump.value["isCustom"].as_bool().unwrap(),
            "{id}"
        );
        assert_eq!(
            theme.liquid,
            dump.value["liquid"].as_bool().unwrap(),
            "{id}"
        );
        for (name, expected) in dump.value["tokens"].as_object().unwrap() {
            let key = TokenKey::from_name(name).unwrap();
            assert_eq!(
                theme.tokens.get(key),
                expected.as_str().unwrap(),
                "{id} token {name}"
            );
        }

        for case in dump.value["cases"].as_array().unwrap() {
            let opacity = case["opacity"].as_f64().unwrap();
            let backdrop = case["backdropAvailable"].as_bool().unwrap();
            let label = format!("{id} {opacity} {backdrop}");
            let applied = theme_application(&theme, opacity, backdrop);
            cases += 1;

            assert_eq!(
                applied.data_theme,
                case["dataTheme"].as_str().unwrap(),
                "{label}"
            );
            assert_eq!(
                applied.data_glass,
                case["dataGlass"].as_str().unwrap(),
                "{label}"
            );
            assert_eq!(
                applied.color_scheme.as_str(),
                case["colorScheme"].as_str().unwrap(),
                "{label}"
            );
            assert_eq!(applied.css.as_deref(), case["css"].as_str(), "{label}");

            let surfaces = &case["surfaces"];
            let s = applied.surfaces;
            for (field, actual) in [
                ("windowOpacity", s.window_opacity),
                ("cardOpacity", s.card_opacity),
                ("hoverOpacity", s.hover_opacity),
                ("mutedOpacity", s.muted_opacity),
                ("elevatedOpacity", s.elevated_opacity),
                ("overlayOpacity", s.overlay_opacity),
            ] {
                assert_eq!(actual, surfaces[field].as_f64().unwrap(), "{label} {field}");
            }
            assert_eq!(
                s.is_liquid,
                surfaces["isLiquid"].as_bool().unwrap(),
                "{label}"
            );

            let expected_vars = case["vars"].as_object().unwrap();
            let expected_names: Vec<&str> = expected_vars.keys().map(String::as_str).collect();
            let actual_names: Vec<&str> = applied.vars.iter().map(|(name, _)| *name).collect();
            assert_eq!(actual_names, expected_names, "{label}: names and order");
            for (name, value) in &applied.vars {
                assert_eq!(
                    value,
                    expected_vars[*name].as_str().unwrap(),
                    "{label} {name}"
                );
                matched += 1;
            }

            // Everything the app paints with resolves to a concrete value.
            let resolved = ResolvedTheme::new(&applied);
            for name in COLOR_VARS {
                assert!(
                    resolved.color(name).is_some(),
                    "{label} colour {name}: {:?}",
                    resolved.substitute(name)
                );
            }
            for name in LENGTH_VARS {
                assert!(
                    resolved.length(name).is_some(),
                    "{label} length {name}: {:?}",
                    resolved.substitute(name)
                );
            }
            for name in NUMBER_VARS {
                assert!(
                    resolved.number(name).is_some(),
                    "{label} number {name}: {:?}",
                    resolved.substitute(name)
                );
            }
            for name in DURATION_VARS {
                assert!(
                    resolved.duration_ms(name).is_some(),
                    "{label} duration {name}"
                );
            }
            for name in SHADOW_VARS {
                assert!(resolved.shadow(name).is_some(), "{label} shadow {name}");
            }
            for item in resolved.unsupported_css() {
                if !unsupported.contains(item) {
                    unsupported.push(item.clone());
                }
            }
        }
    }
    println!(
        "dump: {matched} variables matched across {cases} cases, {} unsupported constructs: {unsupported:?}",
        unsupported.len()
    );
    assert_eq!(cases, dumps.len() * 8);
}

#[test]
fn custom_css_overrides_follow_the_cascade() {
    let dumps = dumps();
    let registry = registry(&dumps);
    let theme = registry.theme(Some("with-css"));
    let resolved = ResolvedTheme::new(&theme_application(&theme, 100.0, true));
    // The inline style beats the theme's own `--accent: #00ff00`.
    assert_eq!(resolved.raw("--accent"), Some("rgb(200 40 120)"));
    assert_eq!(
        resolved.color("--accent").map(|c| c.to_u8()),
        Some([200, 40, 120, 255])
    );
    // `!important` in the theme CSS beats app.css and the inline style alike.
    assert_eq!(
        resolved.color("--accent-text").map(|c| c.to_u8()),
        Some([255, 0, 0, 255])
    );
    // A plain override of a variable no theme writes wins over app.css.
    assert_eq!(resolved.number("--z-menu"), Some(5.0));
    assert_eq!(
        resolved.color("--border").map(|c| c.to_u8()),
        Some([102, 51, 153, 255])
    );
    assert_eq!(
        resolved.unsupported_css(),
        &[UnsupportedCss::Rule(".card".into())]
    );
}

#[test]
fn resolved_values_follow_the_theme() {
    let registry = ThemeRegistry::new();
    let midnight = registry.theme(Some("midnight"));
    let resolved = ResolvedTheme::new(&theme_application(&midnight, 50.0, true));
    // rgb(20 28 48 / 0.66)
    let card = resolved.color("--bg-card").unwrap();
    assert_eq!(card.to_u8(), [20, 28, 48, 168]);
    assert!((card.a - 0.66).abs() < 1e-6);
    // rgb(var(--bg-rgb) / var(--bg-opacity)) from app.css, fed by the theme.
    let bg = resolved.color("--bg").unwrap();
    assert_eq!(bg.to_u8(), [10, 14, 28, 128]);

    let liquid = registry.theme(Some("liquid-glass"));
    let resolved = ResolvedTheme::new(&theme_application(&liquid, 100.0, true));
    assert_eq!(
        resolved.color("--bg-overlay").map(|c| c.to_u8()),
        Some([22, 24, 30, 240])
    );
    assert_eq!(
        resolved.color("--bg-card").map(|c| c.to_u8()),
        Some([255, 255, 255, 33])
    );
}
