#[cfg(windows)]
use egui::FontFamily;
use egui::{pos2, vec2, Color32, Context, Rect};

use super::book::book;
#[cfg(windows)]
use super::book::{resolve, resolve_fallback};
use super::faces::FaceMetrics;
use super::fallback::{fallback_kinds, needs_cjk_faces, Fallback};
use super::fontconfig;
use super::layout::char_offsets;
use super::style::text_field_device;
use super::*;

// The widths are Segoe UI's, a face only Windows ships.
#[cfg(windows)]
#[test]
fn tabular_nums_give_blinks_widths() {
    // Widths from a probe of `font-variant-numeric: tabular-nums` in
    // WebView2, 9px 600 in the UI stack: the figures share one advance.
    let ctx = Context::default();
    install(&ctx, false);
    let plain = TextStyle::new(9.0, 600);
    let tabular = plain.clone().tabular_nums();
    let mut got = Vec::new();
    for t in 0..2 {
        let input = egui::RawInput {
            time: Some(f64::from(t)),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            got = [
                (&plain, "3120/5000", 42.328_125),
                (&plain, "1111", 14.484_375),
                (&tabular, "3120/5000", 43.703_125),
                (&tabular, "4999/5000", 43.703_125),
                (&tabular, "1111", 20.0),
                (&tabular, "0000", 20.0),
            ]
            .iter()
            .map(|(style, s, want)| (*s, measure(ui.ctx(), s, style), *want))
            .collect();
        });
        out.textures_delta.clear();
    }
    for (s, w, want) in got {
        let w = w.expect("faces loaded on the second frame");
        assert!((w - want).abs() < 0.02, "{s}: {w} against {want}");
    }
}

#[test]
fn text_fields_match_blinks_intrinsic_width() {
    // A `size` 20 field in Segoe UI ("x" 940, "0" 1171, glyph box 5267
    // per 2048 em), measured on WebView2 per device font size.
    for (ppem, want) in [
        (12.0, 145.0),
        (13.0, 147.0),
        (14.0, 159.0),
        (15.0, 172.0),
        (16.0, 181.0),
        (17.0, 196.0),
        (18.0, 203.0),
        (19.0, 220.0),
        (20.0, 226.0),
        (21.0, 244.0),
        (24.0, 272.0),
    ] {
        let em = |u: f32| u / 2048.0 * ppem;
        let got = text_field_device(em(940.0), em(1171.0), em(5267.0));
        assert_eq!(got, want, "{ppem} px");
    }
}

fn segoe() -> FaceMetrics {
    FaceMetrics {
        units_per_em: 2048.0,
        ascent: 2210.0,
        descent: 514.0,
        line_gap: 0.0,
        glyph_box: 5267.0,
        x_height: 1062.0,
        gasp: Default::default(),
        gasp_len: 0,
    }
}

#[test]
fn line_boxes_match_the_probe() {
    // From typography-{100,125,150}/probe.json, WebView2 at a real device
    // scale factor: (scale, size, line-height, baseline in device px).
    for (scale, size, lh, baseline) in [
        (1.0, 8.0, LineHeight::Number(1.0), 7.0),
        (1.0, 9.0, LineHeight::Number(1.5), 10.0),
        (1.0, 9.0, LineHeight::Number(1.2), 9.0),
        (1.0, 9.0, LineHeight::Number(1.0), 8.0),
        (1.0, 10.0, LineHeight::Px(13.0), 10.0),
        (1.0, 11.0, LineHeight::Number(1.45), 12.0),
        (1.0, 12.0, LineHeight::Number(1.5), 14.0),
        (1.0, 12.0, LineHeight::Number(1.55), 14.0),
        (1.0, 12.5, LineHeight::Number(1.6), 15.0),
        (1.0, 14.0, LineHeight::Number(1.0), 12.0),
        (1.0, 16.0, LineHeight::Number(1.5), 18.0),
        (1.25, 8.0, LineHeight::Number(1.0), 9.0),
        (1.25, 9.0, LineHeight::Number(1.2), 11.0),
        (1.25, 10.0, LineHeight::Px(13.0), 13.0),
        (1.25, 11.0, LineHeight::Number(1.45), 15.0),
        (1.25, 12.0, LineHeight::Number(1.0), 13.0),
        (1.25, 16.0, LineHeight::Number(1.5), 23.0),
        (1.5, 8.0, LineHeight::Number(1.0), 11.0),
        (1.5, 9.0, LineHeight::Number(1.5), 16.0),
        (1.5, 9.0, LineHeight::Number(1.2), 14.0),
        (1.5, 10.0, LineHeight::Px(13.0), 15.0),
        (1.5, 11.0, LineHeight::Number(1.45), 18.0),
        (1.5, 12.0, LineHeight::Number(1.5), 20.0),
    ] {
        let lb = LineBox::new(&segoe(), size, lh, scale);
        assert_eq!(lb.baseline * scale, baseline, "{size}px {lh:?} at {scale}");
    }
    assert_eq!(
        LineBox::new(&segoe(), 12.0, LineHeight::Normal, 1.0).height,
        16.0
    );
}

/// Runs `frames` frames, `f` drawing in each; what the last returned.
fn frames<T>(ctx: &Context, frames: u32, mut f: impl FnMut(&Context) -> T) -> T {
    let mut last = None;
    for t in 0..frames {
        let input = egui::RawInput {
            time: Some(f64::from(t)),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| last = Some(f(ui.ctx())));
        out.textures_delta.clear();
    }
    last.expect("at least one frame")
}

fn family_count(ctx: &Context) -> usize {
    book(ctx).lock().unwrap().families.len()
}

fn at(x: f32) -> Rect {
    Rect::from_min_size(pos2(x, 0.0), vec2(f32::INFINITY, 0.0))
}

#[test]
fn kana_loads_the_cjk_faces() {
    let ctx = Context::default();
    install(&ctx, false);
    let first = frames(&ctx, 1, |ctx| {
        layout(
            ctx,
            "\u{3042}\u{3044}\u{3046}",
            &TextStyle::new(13.0, 400),
            at(0.0),
        )
    });
    assert!(first.is_none(), "no tofu frame while the faces bind");
    assert!(book(&ctx).lock().unwrap().cjk);
    let second = frames(&ctx, 1, |ctx| {
        layout(ctx, "\u{30AB}\u{30BF}", &TextStyle::new(13.0, 400), at(0.0))
    });
    assert!(second.is_some());
}

#[test]
fn chinese_install_lays_han_out_without_new_families() {
    let ctx = Context::default();
    install(&ctx, true);
    let before = family_count(&ctx);
    let laid = frames(&ctx, 1, |ctx| {
        [13.0, 22.0].iter().all(|&size| {
            [400, 600, 700].iter().all(|&weight| {
                layout(
                    ctx,
                    "\u{8D26}\u{53F7}",
                    &TextStyle::new(size, weight),
                    at(0.0),
                )
                .is_some()
            })
        })
    });
    assert!(laid, "Han text lays out on the first frame");
    assert_eq!(family_count(&ctx), before, "no family added mid frame");
}

#[test]
fn han_on_demand_keeps_the_warmed_sizes() {
    let ctx = Context::default();
    install(&ctx, false);
    let han = "\u{8BBE}\u{7F6E}";
    frames(&ctx, 1, |ctx| {
        layout(ctx, han, &TextStyle::new(13.0, 400), at(0.0))
    });
    let warmed = family_count(&ctx);
    let laid = frames(&ctx, 1, |ctx| {
        [9.0, 13.0, 22.0, 30.0].iter().all(|&size| {
            [400, 600, 700]
                .iter()
                .all(|&weight| layout(ctx, han, &TextStyle::new(size, weight), at(0.0)).is_some())
        })
    });
    assert!(laid, "every UI size is bound the frame after");
    assert_eq!(family_count(&ctx), warmed, "no size adds a family");
}

#[test]
fn emoji_sequences_and_hangul_get_their_own_runs() {
    use Fallback::*;
    assert_eq!(
        fallback_kinds("a\u{1F600}\u{200D}\u{1F525}\u{FE0F}\u{AC00}\u{3042}\u{2192}"),
        vec![None, Emoji, Emoji, Emoji, Emoji, Hangul, None, Math]
    );
    assert_eq!(fallback_kinds("\u{FE0F}a"), vec![None, None]);
    assert!(needs_cjk_faces('\u{3042}') && needs_cjk_faces('\u{30AB}'));
    assert!(needs_cjk_faces('\u{20000}') && needs_cjk_faces('\u{8D26}'));
    assert!(!needs_cjk_faces('\u{AC00}') && !needs_cjk_faces('a'));
}

/// On Linux the sans fontconfig answers may hold no arrow, dingbat or
/// Hangul (Noto Sans, Ubuntu's default, has none of them) and egui's own
/// fonts have no arrow either: each of those kinds names a face
/// fontconfig is asked for.
#[test]
fn symbol_kinds_name_a_face_fontconfig_finds() {
    for kind in [Fallback::Math, Fallback::Symbol, Fallback::Hangul] {
        let asked = kind
            .faces()
            .iter()
            .any(|face| fontconfig::QUERIES.iter().any(|q| q.names.contains(face)));
        assert!(asked, "{kind:?} has no face on Linux");
    }
}

/// Whether a face of `family` maps `c`.
#[cfg(windows)]
fn covers(ctx: &Context, family: &FontFamily, c: char) -> bool {
    use skrifa::MetadataProvider as _;
    let FontFamily::Name(name) = family else {
        return false;
    };
    let book = book(ctx);
    let b = book.lock().unwrap();
    b.families.get(&**name).into_iter().flatten().any(|key| {
        b.faces.get(key).is_some_and(|d| {
            skrifa::FontRef::from_index((*d.font).as_ref(), d.index)
                .is_ok_and(|f| f.charmap().map(c).is_some())
        })
    })
}

#[cfg(windows)]
#[test]
fn emoji_hangul_and_kana_have_a_face_on_windows() {
    let ctx = Context::default();
    install(&ctx, false);
    let style = TextStyle::new(13.0, 400);
    let laid = frames(&ctx, 4, |ctx| {
        layout(ctx, "a\u{1F600}\u{AC00}\u{3042}", &style, at(0.0)).is_some()
    });
    assert!(laid);
    let ppem = 13.0 * ctx.pixels_per_point();
    let primary = resolve(&ctx, &style.family, 400, ppem).unwrap().family;
    for (c, kind) in [
        ('a', Fallback::None),
        ('\u{3042}', Fallback::None),
        ('\u{1F600}', Fallback::Emoji),
        ('\u{AC00}', Fallback::Hangul),
    ] {
        let family = match kind {
            Fallback::None => primary.clone(),
            kind => resolve_fallback(&ctx, &style.family, 400, ppem, kind).unwrap(),
        };
        assert!(covers(&ctx, &family, c), "U+{:04X} has no face", c as u32);
    }
}

#[test]
fn caret_offsets_pair_glyphs_and_characters() {
    // One glyph per character.
    assert_eq!(
        char_offsets("ab", &[('a', 0.0), ('b', 5.0)], 9.0),
        vec![0.0, 5.0, 9.0]
    );
    // A dropped combining mark sits where the next glyph starts.
    assert_eq!(
        char_offsets("e\u{1AB0}x", &[('e', 0.0), ('x', 6.0)], 11.0),
        vec![0.0, 6.0, 6.0, 11.0]
    );
    // Two glyphs for one character, and a case mapping.
    assert_eq!(
        char_offsets("ka", &[('k', 0.0), ('k', 2.0), ('A', 5.0)], 9.0),
        vec![0.0, 5.0, 9.0]
    );
    assert_eq!(
        char_offsets("\u{DF}b", &[('S', 0.0), ('S', 4.0), ('B', 8.0)], 12.0),
        vec![0.0, 8.0, 12.0]
    );
}

#[test]
fn caret_offsets_of_a_large_paste_are_linear() {
    // Upper case turns every sharp s into "SS": more glyphs than
    // characters, the case that measured every prefix.
    let ctx = Context::default();
    install(&ctx, false);
    let style = TextStyle::new(13.0, 400).transform(TextTransform::Uppercase);
    let text = "a\u{DF}".repeat(1500);
    let n = text.chars().count();
    frames(&ctx, 2, |ctx| measure(ctx, "a", &style));
    let started = std::time::Instant::now();
    let got = frames(&ctx, 1, |ctx| offsets(ctx, &text, &style)).expect("laid out");
    let took = started.elapsed();
    assert_eq!(got.len(), n + 1);
    assert!(got.windows(2).all(|w| w[0] <= w[1]), "offsets only grow");
    assert!(took.as_secs_f32() < 5.0, "{took:?} for {n} characters");
}

#[test]
fn css_styles() {
    let s = TextStyle::from_css(
        "left: 3px; font-size: 9px; font-weight: 700; line-height: 1.2; letter-spacing: 0.08em; text-transform: uppercase; color: #7c7c88",
    )
    .unwrap();
    assert_eq!(s.size, 9.0);
    assert_eq!(s.weight, 700);
    assert_eq!(s.line_height, LineHeight::Number(1.2));
    assert!((s.letter_spacing - 0.72).abs() < 1e-6);
    assert_eq!(s.transform, TextTransform::Uppercase);
    assert_eq!(s.color, Color32::from_rgb(0x7c, 0x7c, 0x88));
    assert_eq!(s.transformed("Startup"), "STARTUP");
}
