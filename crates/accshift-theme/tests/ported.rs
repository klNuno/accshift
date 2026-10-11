//! The TypeScript tests of `src/lib/theme/*.test.ts` for the logic this crate
//! ports: schema, surface opacities and themes. Contrast, safety and token
//! tests live next to their modules.

use std::collections::BTreeMap;

use accshift_theme::schema::{IssueLevel, ThemeIssueCode};
use accshift_theme::tokens::is_essential_token;
use accshift_theme::{
    duplicate_theme_document, parse_theme_json, parse_theme_value, resolve_theme_surface_opacities,
    resolve_theme_tokens, root_tokens, serialize_theme_document, theme_application,
    validate_theme_document, ColorScheme, CustomThemePayload, RootTokens, ThemeDocument,
    ThemeParseError, ThemeRegistry, ThemeTokens, TokenKey, MAX_THEME_CSS_LENGTH,
    THEME_CONTRACT_VERSION, THEME_TOKEN_SPECS,
};
use serde_json::json;

fn labelled_root(scheme: &str) -> ThemeTokens {
    ThemeTokens::from_fn(|key| format!("{scheme}-{}", key.name()))
}

fn roots() -> RootTokens {
    RootTokens {
        dark: labelled_root("dark"),
        light: labelled_root("light"),
    }
}

fn doc(id: &str) -> ThemeDocument {
    ThemeDocument {
        schema_version: THEME_CONTRACT_VERSION,
        id: id.to_owned(),
        name: "Test".to_owned(),
        author: None,
        version: None,
        color_scheme: ColorScheme::Dark,
        extends: None,
        glass: None,
        tokens: BTreeMap::new(),
        css: None,
    }
}

fn tokens(pairs: &[(TokenKey, &str)]) -> BTreeMap<TokenKey, String> {
    pairs
        .iter()
        .map(|(key, value)| (*key, (*value).to_owned()))
        .collect()
}

fn registry_of<'a>(
    documents: &'a [ThemeDocument],
) -> impl Fn(&str) -> Option<&'a ThemeDocument> + 'a {
    move |id| documents.iter().find(|document| document.id == id)
}

mod parse {
    use super::*;

    #[test]
    fn refuses_what_is_not_a_theme_file() {
        assert_eq!(
            parse_theme_json("{nope").error,
            Some(ThemeParseError::InvalidJson)
        );
        assert_eq!(
            parse_theme_json("[]").error,
            Some(ThemeParseError::InvalidShape)
        );
        assert_eq!(
            parse_theme_value(&json!({ "name": "No id", "tokens": {} })).error,
            Some(ThemeParseError::InvalidId)
        );
        assert_eq!(
            parse_theme_value(&json!({ "id": "../etc", "name": "Escape", "tokens": {} })).error,
            Some(ThemeParseError::InvalidId)
        );
        assert_eq!(
            parse_theme_value(&json!({ "id": "ok", "tokens": {} })).error,
            Some(ThemeParseError::InvalidName)
        );
    }

    #[test]
    fn refuses_a_file_written_for_a_contract_it_cannot_understand() {
        let result = parse_theme_value(&json!({
            "schemaVersion": THEME_CONTRACT_VERSION + 1,
            "id": "future",
            "name": "Future",
            "colorScheme": "dark",
            "tokens": { "bgCard": "#000000" },
        }));
        assert_eq!(result.error, Some(ThemeParseError::UnsupportedVersion));
        assert!(result.document.is_none());
        assert_eq!(
            result.declared_version,
            i64::from(THEME_CONTRACT_VERSION + 1)
        );
    }

    #[test]
    fn migrates_a_version_1_file_by_pointing_it_at_the_built_in_of_its_scheme() {
        let result = parse_theme_value(&json!({
            "id": "legacy",
            "name": "Legacy",
            "colorScheme": "light",
            "tokens": { "bgCard": "#ffffff", "fg": "#000000" },
        }));
        assert_eq!(result.error, None);
        assert_eq!(result.migrated_from, Some(1));
        let document = result.document.unwrap();
        assert_eq!(document.extends.as_deref(), Some("light"));
        assert_eq!(document.schema_version, THEME_CONTRACT_VERSION);
        assert_eq!(document.tokens[&TokenKey::BgCard], "#ffffff");
    }

    #[test]
    fn drops_values_it_cannot_use_instead_of_refusing_the_file() {
        let result = parse_theme_value(&json!({
            "schemaVersion": 2,
            "id": "sloppy",
            "name": "Sloppy",
            "colorScheme": "dark",
            "tokens": {
                "bgCard": "#101010",
                "bgMuted": "not-a-colour",
                "density": "roomy",
                "elevationLow": "url(https://example.com/x.png)",
                "somethingElse": "#ffffff",
            },
        }));
        assert_eq!(
            result.document.unwrap().tokens,
            tokens(&[(TokenKey::BgCard, "#101010")])
        );
        let mut rejected = result.rejected_tokens;
        rejected.sort();
        assert_eq!(
            rejected,
            ["bgMuted", "density", "elevationLow", "somethingElse"]
        );
    }

    #[test]
    fn takes_the_version_3_structure_tokens_and_refuses_what_they_must_not_carry() {
        let result = parse_theme_value(&json!({
            "schemaVersion": 3,
            "id": "structured",
            "name": "Structured",
            "colorScheme": "light",
            "tokens": {
                "bgImage": "linear-gradient(180deg, #ffffff 0%, #c0c0c0 100%)",
                "borderStyle": "ridge",
                "avatarShape": "square",
                "letterSpacing": "-0.02em",
                "lineHeight": "1.2",
                "motionScale": "0",
                "cardBgImage": "linear-gradient(#fff, url(https://example.com/x.png))",
                "fontSmoothing": "subpixel",
                "focusRing": "ridge",
            },
        }));
        assert_eq!(
            result.document.unwrap().tokens,
            tokens(&[
                (
                    TokenKey::BgImage,
                    "linear-gradient(180deg, #ffffff 0%, #c0c0c0 100%)"
                ),
                (TokenKey::BorderStyle, "ridge"),
                (TokenKey::AvatarShape, "square"),
                (TokenKey::LetterSpacing, "-0.02em"),
                (TokenKey::LineHeight, "1.2"),
                (TokenKey::MotionScale, "0"),
            ])
        );
        let mut rejected = result.rejected_tokens;
        rejected.sort();
        assert_eq!(rejected, ["cardBgImage", "focusRing", "fontSmoothing"]);
    }

    #[test]
    fn keeps_custom_css_but_drops_what_would_let_a_theme_reach_out() {
        let clean = parse_theme_value(&json!({
            "schemaVersion": 2, "id": "styled", "name": "Styled", "colorScheme": "dark",
            "tokens": {}, "css": ".account-card { letter-spacing: 0.02em; }",
        }));
        assert!(clean
            .document
            .unwrap()
            .css
            .unwrap()
            .contains("letter-spacing"));
        assert_eq!(clean.rejected_css, None);

        for css in [
            "@import \"https://example.com/x.css\";",
            ".card { background: url(https://example.com/x.png); }",
            ".card { color: red; }</style><script>alert(1)</script>",
            ".card { background: \\75\\72\\6c(https://example.com/x.png); }",
            ".card { background: u/**/rl(https://example.com/x.png); }",
            "@\\69mport url(https://example.com/x.css);",
            ".card { background: u\\rl(https://example.com/x.png); }",
            ".card { background: \\u\\r\\l(https://example.com/x.png); }",
            "@imp\\ort 'https://example.com/x.css';",
            "@\\IMP\\ORT 'https://example.com/x.css';",
            ".card { background: U\\RL(https://example.com/x.png); }",
            ".card { background: u/**/\\rl(https://example.com/x.png); }",
            ".card { background: \\75 \\r\\6C(https://example.com/x.png); }",
            ".card { background: image-set(\"https://example.com/x.png\" 1x); }",
            ".card { background: -webkit-image-set(\"https://example.com/x.png\" 1x); }",
            ".card { background: IMAGE-SET(\"https://example.com/x.png\" 1x); }",
            ".card { background: image-\\73 et(\"https://example.com/x.png\" 1x); }",
            ".card { background: ima\\ge-s/**/et(\"https://example.com/x.png\" 1x); }",
            ".card { background: image(\"https://example.com/x.png\"); }",
            ".card { background: src(\"https://example.com/x.png\"); }",
        ] {
            let result = parse_theme_value(&json!({
                "schemaVersion": 2, "id": "hostile", "name": "Hostile", "colorScheme": "dark",
                "tokens": { "accent": "#123456" }, "css": css,
            }));
            let document = result.document.unwrap();
            assert_eq!(document.css, None, "{css}");
            assert_eq!(document.tokens[&TokenKey::Accent], "#123456", "{css}");
            assert!(result.rejected_css.is_some(), "{css}");
        }

        let escaped = parse_theme_value(&json!({
            "schemaVersion": 2, "id": "escaped", "name": "Escaped", "colorScheme": "dark",
            "tokens": {},
            "css": ".w-1\\/2::before { content: \"\\201C\"; background-image: linear-gradient(red, blue); }",
        }));
        assert_eq!(escaped.rejected_css, None);
    }

    #[test]
    fn round_trips_through_the_exported_file_byte_for_byte() {
        let source = ThemeDocument {
            id: "nord".into(),
            name: "Nord".into(),
            author: Some("someone".into()),
            version: Some("1.2.0".into()),
            extends: Some("dark".into()),
            glass: Some(true),
            tokens: tokens(&[(TokenKey::Accent, "#88c0d0"), (TokenKey::RadiusMd, "10px")]),
            css: Some(".account-card { letter-spacing: 0.02em; }".into()),
            ..doc("nord")
        };
        let json = serialize_theme_document(&source);
        let parsed = parse_theme_json(&json).document.unwrap();
        assert_eq!(parsed, source);
        assert_eq!(serialize_theme_document(&parsed), json);
        assert!(json.starts_with("{\n  \"schemaVersion\": 3,\n  \"id\": \"nord\""));
    }
}

mod resolve {
    use super::*;

    #[test]
    fn lets_the_nearest_document_win_and_fills_the_rest_from_the_root() {
        let parent = ThemeDocument {
            tokens: tokens(&[(TokenKey::Accent, "#111111"), (TokenKey::Fg, "#eeeeee")]),
            ..doc("parent")
        };
        let child = ThemeDocument {
            extends: Some("parent".into()),
            tokens: tokens(&[(TokenKey::Accent, "#222222")]),
            ..doc("child")
        };
        let documents = [parent, child.clone()];
        let resolved = resolve_theme_tokens(&child, registry_of(&documents), &roots());
        assert_eq!(resolved.tokens[TokenKey::Accent], *"#222222");
        assert_eq!(resolved.tokens[TokenKey::Fg], *"#eeeeee");
        assert_eq!(resolved.tokens[TokenKey::BgCard], *"dark-bgCard");
        assert_eq!(resolved.chain, ["child", "parent"]);
        assert_eq!(resolved.broken_extends, None);
    }

    #[test]
    fn still_produces_a_complete_theme_when_the_file_is_empty() {
        let empty = doc("test");
        let resolved = resolve_theme_tokens(&empty, registry_of(&[]), &roots());
        for key in TokenKey::ALL {
            assert_eq!(resolved.tokens.get(key), format!("dark-{}", key.name()));
        }
        assert_eq!(resolved.filled_from_root.len(), TokenKey::ALL.len());
    }

    #[test]
    fn survives_a_base_that_does_not_exist() {
        let orphan = ThemeDocument {
            extends: Some("ghost".into()),
            tokens: tokens(&[(TokenKey::Fg, "#abcdef")]),
            ..doc("orphan")
        };
        let documents = [orphan.clone()];
        let resolved = resolve_theme_tokens(&orphan, registry_of(&documents), &roots());
        assert_eq!(resolved.broken_extends.as_deref(), Some("ghost"));
        assert_eq!(resolved.tokens[TokenKey::Fg], *"#abcdef");
        assert_eq!(resolved.tokens[TokenKey::BgCard], *"dark-bgCard");
    }

    #[test]
    fn survives_a_cycle_rather_than_spinning_on_it() {
        let a = ThemeDocument {
            extends: Some("b".into()),
            tokens: tokens(&[(TokenKey::Fg, "#a0a0a0")]),
            ..doc("a")
        };
        let b = ThemeDocument {
            extends: Some("a".into()),
            tokens: tokens(&[(TokenKey::BgCard, "#0b0b0b")]),
            ..doc("b")
        };
        let documents = [a.clone(), b];
        let resolved = resolve_theme_tokens(&a, registry_of(&documents), &roots());
        assert_eq!(resolved.broken_extends.as_deref(), Some("a"));
        assert_eq!(resolved.chain, ["a", "b"]);
        assert_eq!(resolved.tokens[TokenKey::Fg], *"#a0a0a0");
        assert_eq!(resolved.tokens[TokenKey::BgCard], *"#0b0b0b");
    }
}

mod validate {
    use super::*;

    fn readable_tokens() -> BTreeMap<TokenKey, String> {
        tokens(&[
            (TokenKey::BgRgb, "9 9 11"),
            (TokenKey::BgCard, "#1c1c1f"),
            (TokenKey::Fg, "#fafafa"),
            (TokenKey::FgMuted, "#a1a1aa"),
            (TokenKey::FgSubtle, "#71717a"),
            (TokenKey::AfkText, "#ffffff"),
            (TokenKey::Accent, "#2563eb"),
            (TokenKey::AccentFg, "#ffffff"),
            (TokenKey::Success, "#22c55e"),
            (TokenKey::Warning, "#eab308"),
            (TokenKey::Danger, "#dc2626"),
        ])
    }

    fn with_tokens(map: BTreeMap<TokenKey, String>) -> ThemeDocument {
        ThemeDocument {
            tokens: map,
            ..doc("test")
        }
    }

    fn issues_for(document: &ThemeDocument) -> Vec<accshift_theme::ThemeIssue> {
        let mut dark = labelled_root("dark");
        for (key, value) in readable_tokens() {
            dark.set(key, value);
        }
        let roots = RootTokens {
            dark,
            light: labelled_root("light"),
        };
        let documents = [document.clone()];
        let resolved = resolve_theme_tokens(document, registry_of(&documents), &roots);
        validate_theme_document(document, &resolved, None)
    }

    #[test]
    fn says_nothing_about_a_readable_palette() {
        let issues = issues_for(&with_tokens(readable_tokens()));
        assert!(issues
            .iter()
            .all(|issue| issue.code != ThemeIssueCode::Contrast));
    }

    #[test]
    fn reports_text_that_cannot_be_read_on_its_own_background() {
        let mut map = readable_tokens();
        map.insert(TokenKey::Fg, "#1d1d20".into());
        let issues = issues_for(&with_tokens(map));
        let issue = issues
            .iter()
            .find(|issue| {
                issue.code == ThemeIssueCode::Contrast
                    && issue.token == Some(TokenKey::Fg)
                    && issue.against == Some(TokenKey::BgCard)
            })
            .unwrap();
        assert_eq!(issue.level, IssueLevel::Error);
        assert!(issue.ratio.unwrap() < 3.0);
    }

    #[test]
    fn measures_a_translucent_card_as_it_is_actually_painted() {
        let mut map = readable_tokens();
        map.insert(TokenKey::BgRgb, "22 24 30".into());
        map.insert(TokenKey::BgCard, "#ffffff".into());
        map.insert(TokenKey::Fg, "#ffffff".into());
        let glass = with_tokens(map);
        let documents = [glass.clone()];
        let resolved = resolve_theme_tokens(&glass, registry_of(&documents), &roots());
        let fg_contrast = |alpha| {
            validate_theme_document(&glass, &resolved, alpha)
                .into_iter()
                .filter(|issue| {
                    issue.code == ThemeIssueCode::Contrast && issue.token == Some(TokenKey::Fg)
                })
                .count()
        };
        assert_ne!(fg_contrast(None), 0);
        assert_eq!(fg_contrast(Some(0.13)), 0);
    }

    #[test]
    fn flags_a_hole_only_when_the_theme_inherits_from_nothing() {
        let standalone = with_tokens(tokens(&[(TokenKey::Fg, "#fafafa")]));
        let derived = ThemeDocument {
            extends: Some("dark".into()),
            ..standalone.clone()
        };
        let has_missing = |document| {
            issues_for(document)
                .iter()
                .any(|issue| issue.code == ThemeIssueCode::MissingToken)
        };
        assert!(has_missing(&standalone));
        assert!(!has_missing(&derived));
    }

    #[test]
    fn does_not_call_a_theme_incomplete_for_leaving_the_structure_alone() {
        let palette = with_tokens(
            THEME_TOKEN_SPECS
                .iter()
                .filter(|spec| is_essential_token(spec.key.name()))
                .map(|spec| (spec.key, format!("dark-{}", spec.key.name())))
                .collect(),
        );
        let missing: Vec<_> = issues_for(&palette)
            .into_iter()
            .filter(|issue| issue.code == ThemeIssueCode::MissingToken)
            .collect();
        assert!(missing.is_empty(), "{missing:?}");
    }

    #[test]
    fn blocks_css_that_reaches_outside_the_window_and_css_past_the_cap() {
        let hostile = ThemeDocument {
            css: Some("@import url(https://example.com/x.css);".into()),
            ..with_tokens(readable_tokens())
        };
        let huge = ThemeDocument {
            css: Some(format!("/*{}*/", "x".repeat(MAX_THEME_CSS_LENGTH))),
            ..with_tokens(readable_tokens())
        };
        let unsafe_issue = issues_for(&hostile)
            .into_iter()
            .find(|issue| issue.code == ThemeIssueCode::UnsafeCss)
            .unwrap();
        let too_long = issues_for(&huge)
            .into_iter()
            .find(|issue| issue.code == ThemeIssueCode::CssTooLong)
            .unwrap();
        assert_eq!(unsafe_issue.level, IssueLevel::Error);
        assert_eq!(unsafe_issue.construct, Some("@import"));
        assert_eq!(too_long.level, IssueLevel::Error);
        assert_eq!(too_long.limit, Some(MAX_THEME_CSS_LENGTH));
    }

    #[test]
    fn reports_a_value_the_editor_refuses_to_store() {
        let mut map = readable_tokens();
        map.insert(TokenKey::RadiusMd, "12 pixels".into());
        let issue = issues_for(&with_tokens(map))
            .into_iter()
            .find(|issue| issue.code == ThemeIssueCode::InvalidValue)
            .unwrap();
        assert_eq!(issue.level, IssueLevel::Error);
        assert_eq!(issue.token, Some(TokenKey::RadiusMd));
        assert_eq!(issue.expected.map(|kind| kind.name()), Some("length"));
    }
}

#[test]
fn duplicate_extends_the_source_instead_of_copying_its_tokens() {
    let source = ThemeDocument {
        glass: Some(true),
        tokens: tokens(&[(TokenKey::Fg, "#f4f4f6")]),
        css: Some(".account-card { letter-spacing: 0.02em; }".into()),
        ..doc("glass-dark")
    };
    let copy = duplicate_theme_document(&source, "my-glass", "My Glass");
    assert_eq!(copy.extends.as_deref(), Some("glass-dark"));
    assert!(copy.tokens.is_empty());
    assert_eq!(copy.css, source.css);
    assert_eq!(copy.glass, Some(true));
    assert_eq!(copy.color_scheme, source.color_scheme);
}

/// `surfaceOpacities.test.ts`, captured from the TypeScript before the four
/// surface rules became a table.
mod surfaces {
    use super::*;

    const GOLDEN: &[(&str, f64, bool, [f64; 6], bool)] = &[
        (
            "dark",
            0.0,
            true,
            [0.0, 0.66, 0.72, 0.72, 0.78, 0.86],
            false,
        ),
        (
            "dark",
            25.0,
            true,
            [0.25, 0.66, 0.72, 0.72, 0.78, 0.86],
            false,
        ),
        (
            "dark",
            42.0,
            false,
            [0.42, 0.66, 0.72, 0.72, 0.78, 0.86],
            false,
        ),
        (
            "dark",
            55.0,
            true,
            [0.55, 0.6900000000000001, 0.75, 0.73, 0.78, 0.86],
            false,
        ),
        (
            "dark",
            80.0,
            false,
            [0.8, 0.9400000000000001, 1.0, 0.98, 1.0, 1.0],
            false,
        ),
        ("dark", 100.0, true, [1.0, 1.0, 1.0, 1.0, 1.0, 1.0], false),
        (
            "light",
            55.0,
            false,
            [0.55, 0.6900000000000001, 0.75, 0.73, 0.78, 0.86],
            false,
        ),
        (
            "midnight",
            80.0,
            true,
            [0.8, 0.9400000000000001, 1.0, 0.98, 1.0, 1.0],
            false,
        ),
        (
            "glass-dark",
            0.0,
            true,
            [0.55, 0.65, 0.73, 0.6900000000000001, 0.75, 0.86],
            false,
        ),
        (
            "glass-dark",
            42.0,
            false,
            [0.96, 0.72, 0.7999999999999999, 0.78, 0.85, 1.0],
            false,
        ),
        (
            "glass-light",
            100.0,
            true,
            [0.55, 0.65, 0.73, 0.6900000000000001, 0.75, 0.86],
            false,
        ),
        (
            "glass-light",
            25.0,
            false,
            [0.96, 0.72, 0.7999999999999999, 0.78, 0.85, 1.0],
            false,
        ),
        (
            "liquid-glass",
            55.0,
            true,
            [0.18, 0.13, 0.21, 0.16, 0.34, 0.86],
            true,
        ),
        (
            "liquid-glass",
            80.0,
            false,
            [0.96, 0.72, 0.7999999999999999, 0.78, 0.85, 1.0],
            false,
        ),
    ];

    #[test]
    fn paints_every_built_in_theme_exactly_as_before_the_table() {
        let registry = ThemeRegistry::new();
        for (id, percent, backdrop, expected, liquid) in GOLDEN {
            let s = resolve_theme_surface_opacities(&registry.theme(Some(id)), *percent, *backdrop);
            let actual = [
                s.window_opacity,
                s.card_opacity,
                s.hover_opacity,
                s.muted_opacity,
                s.elevated_opacity,
                s.overlay_opacity,
            ];
            assert_eq!(actual, *expected, "{id} {percent} {backdrop}");
            assert_eq!(s.is_liquid, *liquid, "{id} {percent} {backdrop}");
        }
    }

    #[test]
    fn clamps_a_slider_value_outside_0_to_100() {
        let dark = ThemeRegistry::new().theme(Some("dark"));
        assert_eq!(
            resolve_theme_surface_opacities(&dark, -40.0, true).window_opacity,
            0.0
        );
        assert_eq!(
            resolve_theme_surface_opacities(&dark, 400.0, true).window_opacity,
            1.0
        );
    }
}

/// `themes.test.ts`.
mod themes {
    use super::*;
    use accshift_theme::{is_valid_token_value, BuiltInOverwrite};

    fn payload(value: serde_json::Value) -> CustomThemePayload {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn liquid_glass_uses_tuned_translucent_values_with_a_backdrop() {
        let registry = ThemeRegistry::new();
        let values =
            resolve_theme_surface_opacities(&registry.theme(Some("liquid-glass")), 50.0, true);
        assert!(values.is_liquid);
        assert_eq!(values.window_opacity, 0.18);
        assert_eq!(values.card_opacity, 0.13);
        let fallback =
            resolve_theme_surface_opacities(&registry.theme(Some("liquid-glass")), 50.0, false);
        assert!(!fallback.is_liquid);
        assert_eq!(fallback.window_opacity, 0.96);
        assert_eq!(fallback.card_opacity, 0.72);
        assert_eq!(fallback.overlay_opacity, 1.0);
        let dark = resolve_theme_surface_opacities(&registry.theme(Some("dark")), 42.0, false);
        assert_eq!(dark.window_opacity, 0.42);
    }

    #[test]
    fn treats_a_theme_that_extends_liquid_glass_as_liquid() {
        let registry = ThemeRegistry::new();
        let copy = registry.theme_from_document(&ThemeDocument {
            extends: Some("liquid-glass".into()),
            glass: Some(true),
            name: "My Liquid".into(),
            ..doc("my-liquid")
        });
        assert!(copy.liquid);
        assert!(registry.uses_liquid_glass(&copy));
        let values = resolve_theme_surface_opacities(&copy, 50.0, true);
        assert!(values.is_liquid);
        assert_eq!(values.window_opacity, 0.18);
        assert_eq!(values.card_opacity, 0.13);
    }

    #[test]
    fn gives_both_roots_a_valid_value_for_every_token() {
        for (scheme, root) in [
            ("dark", &root_tokens().dark),
            ("light", &root_tokens().light),
        ] {
            for (key, value) in root.iter() {
                assert!(!value.is_empty(), "{scheme}.{key}");
                assert!(
                    is_valid_token_value(key.name(), value),
                    "{scheme}.{key} = {value}"
                );
            }
        }
    }

    #[test]
    fn ships_nothing_a_user_could_not_read() {
        let registry = ThemeRegistry::new();
        for theme in registry.built_in_themes() {
            let resolved =
                resolve_theme_tokens(&theme.document, |id| registry.document(id), root_tokens());
            let card_alpha = resolve_theme_surface_opacities(&theme, 100.0, true).card_opacity;
            let errors: Vec<_> =
                validate_theme_document(&theme.document, &resolved, Some(card_alpha))
                    .into_iter()
                    .filter(|issue| issue.level == IssueLevel::Error)
                    .collect();
            assert!(errors.is_empty(), "{}: {errors:?}", theme.id);
        }
    }

    #[test]
    fn keeps_the_two_json_built_ins_expressible_in_the_public_format() {
        let registry = ThemeRegistry::new();
        for id in ["midnight", "glass-dark"] {
            let theme = registry.theme(Some(id));
            assert_eq!(theme.id, id);
            assert_eq!(theme.document.extends.as_deref(), Some("dark"));
            assert_eq!(
                theme.tokens[TokenKey::Accent],
                root_tokens().dark[TokenKey::Accent]
            );
            assert_eq!(
                theme.tokens[TokenKey::Density],
                root_tokens().dark[TokenKey::Density]
            );
            assert_eq!(
                theme.tokens.get(TokenKey::BgCard),
                theme.document.tokens[&TokenKey::BgCard]
            );
        }
    }

    #[test]
    fn keeps_a_version_1_theme_working_and_hands_it_the_tokens_added_since() {
        let mut registry = ThemeRegistry::new();
        registry.apply_custom_theme_payloads(&[payload(json!({
            "id": "legacy", "name": "Legacy", "colorScheme": "dark",
            "tokens": { "bgCard": "#101014", "fg": "#f0f0f0" },
        }))]);
        let theme = registry.theme(Some("legacy"));
        assert!(theme.is_custom);
        assert_eq!(theme.tokens[TokenKey::BgCard], *"#101014");
        assert_eq!(theme.tokens[TokenKey::Fg], *"#f0f0f0");
        assert_eq!(
            theme.tokens[TokenKey::Accent],
            root_tokens().dark[TokenKey::Accent]
        );
        assert_eq!(
            theme.tokens[TokenKey::RadiusMd],
            root_tokens().dark[TokenKey::RadiusMd]
        );
    }

    #[test]
    fn degrades_a_broken_theme_to_its_base_instead_of_breaking_the_interface() {
        let mut registry = ThemeRegistry::new();
        registry.apply_custom_theme_payloads(&[
            payload(json!({
                "schemaVersion": 2, "id": "broken", "name": "Broken", "colorScheme": "dark",
                "extends": "does-not-exist", "tokens": { "bgCard": "javascript:alert(1)" },
            })),
            payload(json!({ "id": "", "name": "Nameless", "colorScheme": "dark", "tokens": {} })),
        ]);
        let theme = registry.theme(Some("broken"));
        assert_eq!(
            theme.tokens[TokenKey::BgCard],
            root_tokens().dark[TokenKey::BgCard]
        );
        for (key, value) in theme.tokens.iter() {
            assert!(!value.is_empty(), "{key}");
        }
        assert_eq!(registry.theme(Some("")).id, "dark");

        registry.apply_custom_theme_payloads(&[]);
        assert_eq!(registry.theme(Some("broken")).id, "dark");
    }

    #[test]
    fn writes_css_the_filter_refused_back_to_the_file_when_the_editor_saves() {
        let refused = ".card { background: image-set(\"a.png\" 1x); }";
        let older = || {
            payload(json!({
                "schemaVersion": 3, "id": "older", "name": "Older", "colorScheme": "dark",
                "tokens": {}, "css": refused,
            }))
        };
        let mut registry = ThemeRegistry::new();
        registry.apply_custom_theme_payloads(&[older()]);
        let theme = registry.theme(Some("older"));
        assert_eq!(theme.document.css, None);

        let save = |registry: &mut ThemeRegistry, keep: bool, css: Option<&str>| {
            let document = ThemeDocument {
                name: "Renamed".into(),
                css: css.map(str::to_owned),
                ..theme.document.clone()
            };
            let pending = registry.prepare_save(&document, keep).unwrap();
            let css = pending.payload.css.clone();
            registry.finish_save(pending);
            css
        };
        assert_eq!(save(&mut registry, true, None).as_deref(), Some(refused));
        assert_eq!(
            save(&mut registry, true, Some(".card { opacity: 1; }")).as_deref(),
            Some(".card { opacity: 1; }")
        );
        assert_eq!(save(&mut registry, true, None), None);

        registry.apply_custom_theme_payloads(&[older()]);
        assert_eq!(save(&mut registry, false, None), None);

        assert_eq!(
            registry.prepare_save(&doc("dark"), false),
            Err(BuiltInOverwrite("dark".into()))
        );
    }

    #[test]
    fn writes_every_token_to_the_custom_property_its_spec_declares() {
        let applied = theme_application(&ThemeRegistry::new().theme(Some("dark")), 100.0, true);
        for spec in &THEME_TOKEN_SPECS {
            assert!(applied.var(spec.css_var).is_some(), "{}", spec.css_var);
        }
        assert_eq!(
            applied.var("--accent"),
            Some(&root_tokens().dark[TokenKey::Accent])
        );
        assert_eq!(
            applied.var("--radius-md"),
            Some(&root_tokens().dark[TokenKey::RadiusMd])
        );
        assert_eq!(applied.var("--density-scale"), Some("1"));
        assert_eq!(
            applied.var("--font-ui"),
            Some("Inter, var(--font-stack-base)")
        );
    }

    #[test]
    fn turns_the_structure_tokens_into_what_css_can_actually_use() {
        let theme = ThemeRegistry::new().theme_from_document(&ThemeDocument {
            color_scheme: ColorScheme::Light,
            tokens: tokens(&[
                (TokenKey::AvatarShape, "circle"),
                (TokenKey::FontSmoothing, "none"),
                (TokenKey::BorderStyle, "ridge"),
                (TokenKey::BorderWidth, "2px"),
                (TokenKey::MotionScale, "0"),
                (TokenKey::FontDisplay, "Tahoma"),
                (
                    TokenKey::CardBgImage,
                    "linear-gradient(180deg, #ffffff, #c0c0c0)",
                ),
            ]),
            ..doc("bevelled")
        });
        let applied = theme_application(&theme, 100.0, true);
        assert_eq!(applied.var("--avatar-radius"), Some("50%"));
        assert_eq!(applied.var("--font-smoothing"), Some("none"));
        assert_eq!(applied.var("--font-smoothing-mac"), Some("auto"));
        assert_eq!(applied.var("--border-style"), Some("ridge"));
        assert_eq!(applied.var("--border-width"), Some("2px"));
        assert_eq!(applied.var("--motion-scale"), Some("0"));
        assert_eq!(
            applied.var("--font-display"),
            Some("Tahoma, var(--font-ui)")
        );
        assert_eq!(
            applied.var("--card-bg-image"),
            Some("linear-gradient(180deg, #ffffff, #c0c0c0)")
        );
    }

    #[test]
    fn keeps_a_gradient_off_the_surfaces_a_glass_theme_reads_the_desktop_through() {
        let gradient = "linear-gradient(180deg, #ffffff, #000000)";
        let theme = ThemeRegistry::new().theme_from_document(&ThemeDocument {
            glass: Some(true),
            tokens: tokens(&[
                (TokenKey::BgImage, gradient),
                (TokenKey::CardBgImage, gradient),
            ]),
            ..doc("glassy")
        });
        let applied = theme_application(&theme, 100.0, true);
        assert_eq!(applied.var("--bg-image"), Some("none"));
        assert_eq!(applied.var("--card-bg-image"), Some("none"));
    }

    #[test]
    fn paints_a_theme_it_has_never_seen_without_falling_over() {
        let theme = ThemeRegistry::new().theme_from_document(&ThemeDocument {
            schema_version: 2,
            color_scheme: ColorScheme::Light,
            tokens: tokens(&[(TokenKey::Accent, "#ff0088")]),
            ..doc("sparse")
        });
        let applied = theme_application(&theme, 100.0, true);
        assert_eq!(applied.var("--accent"), Some("#ff0088"));
        assert_eq!(
            applied.var("--fg"),
            Some(&root_tokens().light[TokenKey::Fg])
        );
    }

    #[test]
    fn carries_the_themes_own_css_and_takes_it_away_with_the_theme() {
        let registry = ThemeRegistry::new();
        let with_css = registry.theme_from_document(&ThemeDocument {
            schema_version: 2,
            css: Some(".account-card { letter-spacing: 0.02em; }".into()),
            ..doc("with-css")
        });
        assert!(theme_application(&with_css, 100.0, true)
            .css
            .unwrap()
            .contains("letter-spacing"));
        assert_eq!(
            theme_application(&registry.theme(Some("dark")), 100.0, true).css,
            None
        );
    }

    #[test]
    fn suggests_unique_ids() {
        let mut registry = ThemeRegistry::new();
        assert_eq!(registry.suggest_theme_id("  My Théme!! "), "my-th-me");
        assert_eq!(registry.suggest_theme_id("Dark"), "dark-2");
        assert_eq!(registry.suggest_theme_id("!!!"), "theme");
        registry.apply_custom_theme_payloads(&[payload(json!({
            "schemaVersion": 3, "id": "dark-2", "name": "x", "colorScheme": "dark", "tokens": {},
        }))]);
        assert_eq!(registry.suggest_theme_id("Dark"), "dark-3");
    }
}
