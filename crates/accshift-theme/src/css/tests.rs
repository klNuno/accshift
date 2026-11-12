use super::*;

fn theme(vars: &[(&str, &str)]) -> ResolvedTheme {
    let mut resolved = ResolvedTheme::root();
    for (name, value) in vars {
        resolved
            .declared
            .insert((*name).to_owned(), (*value).to_owned());
    }
    resolved
}

#[test]
fn reads_the_app_css_root_block() {
    let root = ResolvedTheme::root();
    assert_eq!(root.raw("--bg-rgb"), Some("9 9 11"));
    assert_eq!(root.raw("--z-menu"), Some("1200"));
    assert!(root
        .raw("--font-stack-base")
        .is_some_and(|stack| stack.ends_with("sans-serif")));
    assert_eq!(root.color("--bg").map(Rgba::to_u8), Some([9, 9, 11, 255]));
    assert_eq!(root.number("--density-scale"), Some(1.0));
    assert_eq!(root.length_px("--grid-card-width"), Some(100.0));
    assert_eq!(root.duration_ms("--motion-card-entrance"), Some(260.0));
    assert_eq!(root.duration_ms("--motion-avatar-defer"), Some(380.0));
    assert_eq!(root.length_px("--avatar-radius"), Some(6.0));
    assert_eq!(root.number("--z-wallpaper"), Some(-2.0));
}

#[test]
fn follows_var_chains_and_fallbacks() {
    let resolved = theme(&[
        ("--a", "var(--b)"),
        ("--b", "var(--c, #00ff00)"),
        ("--loop", "var(--loop)"),
        ("--uses-loop", "var(--loop, red)"),
        ("--nested", "var(--missing, var(--also-missing, 4px))"),
    ]);
    assert_eq!(resolved.substitute("--a").as_deref(), Some("#00ff00"));
    assert_eq!(resolved.substitute("--loop"), None);
    assert_eq!(
        resolved.color("--uses-loop").map(Rgba::to_u8),
        Some([255, 0, 0, 255])
    );
    assert_eq!(resolved.length_px("--nested"), Some(4.0));
    assert_eq!(resolved.substitute("--undefined"), None);
}

#[test]
fn evaluates_calc_forms_the_css_uses() {
    let resolved = theme(&[
        ("--density-scale", "0.88"),
        ("--motion-scale", "0"),
        ("--edge", "calc(100% - 26px)"),
        ("--wide", "calc(var(--grid-card-width) + 130px)"),
        ("--viewport", "calc(100vw - 24px)"),
        ("--rem", "1.5rem"),
        ("--px-times-px", "calc(2px * 3px)"),
        ("--px-plus-number", "calc(2px + 3)"),
        ("--number", "calc((1 + 2) * 2)"),
    ]);
    assert_eq!(resolved.length_px("--grid-card-padding"), Some(8.0 * 0.88));
    assert_eq!(resolved.duration_ms("--motion-page-entrance"), Some(0.0));
    let edge = resolved.length("--edge").unwrap();
    assert_eq!(edge.resolve(16.0, 200.0, (0.0, 0.0)), 174.0);
    assert_eq!(resolved.length_px("--wide"), Some(218.0));
    assert_eq!(
        resolved
            .length("--viewport")
            .map(|l| l.resolve(16.0, 0.0, (800.0, 600.0))),
        Some(776.0)
    );
    assert_eq!(resolved.length_px("--rem"), Some(24.0));
    assert_eq!(resolved.length("--px-times-px"), None);
    assert_eq!(resolved.length("--px-plus-number"), None);
    assert_eq!(resolved.number("--number"), Some(6.0));
}

#[test]
fn mixes_the_accent_text_colour() {
    let root = ResolvedTheme::root();
    // color-mix(in srgb, #2563eb 55%, #fafafa)
    let accent_text = root.color("--accent-text").unwrap();
    let expected = [
        (0x25 as f32 * 0.55 + 0xfa as f32 * 0.45) / 255.0,
        (0x63 as f32 * 0.55 + 0xfa as f32 * 0.45) / 255.0,
        (0xeb as f32 * 0.55 + 0xfa as f32 * 0.45) / 255.0,
    ];
    assert!((accent_text.r - expected[0]).abs() < 1e-5);
    assert!((accent_text.g - expected[1]).abs() < 1e-5);
    assert!((accent_text.b - expected[2]).abs() < 1e-5);
    assert_eq!(accent_text.a, 1.0);
    let ring = root
        .eval_color("color-mix(in srgb, var(--fg) 65%, transparent)")
        .unwrap();
    assert!((ring.a - 0.65).abs() < 1e-6);
    assert_eq!(ring.to_u8()[..3], [250, 250, 250]);
}

#[test]
fn parses_shadows() {
    let root = ResolvedTheme::root();
    let low = root.shadow("--elevation-low").unwrap();
    assert_eq!(low.len(), 1);
    assert_eq!(
        (low[0].offset_x, low[0].offset_y, low[0].blur, low[0].spread),
        (0.0, 2.0, 8.0, 0.0)
    );
    assert_eq!(low[0].color, Some(Rgba::new(0.0, 0.0, 0.0, 0.18)));
    let custom = theme(&[
            (
                "--glaze",
                "inset 0 1px 0 rgba(255, 255, 255, 0.34), inset 0 12px 24px -18px rgba(255, 255, 255, 0.4)",
            ),
            ("--flat", "none"),
        ]);
    let glaze = custom.shadow("--glaze").unwrap();
    assert_eq!(glaze.len(), 2);
    assert!(glaze[1].inset);
    assert_eq!(glaze[1].spread, -18.0);
    assert_eq!(custom.shadow("--flat"), Some(Vec::new()));
}

#[test]
fn reads_custom_css_and_reports_the_rest() {
    let (properties, unsupported) = parse_root_custom_properties(
            "/* note */ @import x; :root { --a: 1px; color: red; --b: rgb(1 2 3) !important; }\n.card { color: red; }\n@media (x) { :root { --c: 2 } }",
        );
    assert_eq!(
        properties,
        vec![
            CustomProperty {
                name: "--a".into(),
                value: "1px".into(),
                important: false
            },
            CustomProperty {
                name: "--b".into(),
                value: "rgb(1 2 3)".into(),
                important: true
            },
        ]
    );
    assert_eq!(
        unsupported,
        vec![
            UnsupportedCss::AtRule("@import".into()),
            UnsupportedCss::Declaration("color".into()),
            UnsupportedCss::Rule(".card".into()),
            UnsupportedCss::AtRule("@media".into()),
        ]
    );
}
