use accshift_theme::{ThemeRegistry, TokenGroup, TokenKey, TokenKind};
use egui::Vec2;

use super::super::*;
use super::{
    Styles, BETA_GAP, BODY_GAP, CSS_PAD, CSS_ROWS, FIELD_GAP, LABEL_MIN, LABEL_SHARE, META_GAP,
    META_MIN, ROW_GAP, ROW_PAD, UNDER,
};
use crate::app::theme_editor::{self as model, ThemeEditor};

/// One token row as the editor shows it.
pub(super) struct Row {
    pub(super) key: TokenKey,
    pub(super) kind: TokenKind,
    pub(super) label: String,
    pub(super) overridden: bool,
    pub(super) value: String,
    /// The row's issue and its colour.
    pub(super) issue: Option<(String, Rgba)>,
}

/// A block of the body, laid out at a `y` from the content top.
pub(super) enum Block {
    Beta,
    Meta,
    Head(String),
    Token(usize),
    /// The row's issue under it, full width.
    TokenIssue(usize),
    Help,
    Css,
    Issue(String, Rgba),
}

pub(super) struct Layout {
    pub(super) blocks: Vec<(f32, f32, Block)>,
    pub(super) height: f32,
}

/// What the editor's body shows, computed once per frame.
pub(super) struct Model {
    pub(super) rows: Vec<Row>,
    pub(super) groups: Vec<(String, Vec<usize>)>,
    pub(super) issues: Vec<(String, Rgba)>,
    pub(super) blocked: bool,
    pub(super) base_ids: Vec<String>,
    pub(super) base_labels: Vec<String>,
    /// The save failed: the alert line under the issues, in `--danger`.
    pub(super) save_error: Option<(String, Rgba)>,
}

pub(super) fn view_model(
    editor: &ThemeEditor,
    registry: &ThemeRegistry,
    locale: Locale,
    v: &Vars,
) -> Model {
    let draft = editor.draft();
    let resolved = editor.resolved(registry, &draft);
    let issues = editor.issues(registry, &draft, &resolved);
    let colour = |issue: &accshift_theme::ThemeIssue| {
        if model::is_error(issue) {
            v.danger
        } else {
            v.warning
        }
    };
    let mut rows = Vec::new();
    let mut groups = Vec::new();
    for group in TokenGroup::ALL {
        let mut members = Vec::new();
        for spec in accshift_theme::THEME_TOKEN_SPECS
            .iter()
            .filter(|s| s.group == group)
        {
            let overridden = editor.tokens.contains_key(&spec.key);
            let value = editor
                .tokens
                .get(&spec.key)
                .cloned()
                .unwrap_or_else(|| resolved.tokens.get(spec.key).to_owned());
            members.push(rows.len());
            rows.push(Row {
                key: spec.key,
                kind: spec.kind,
                label: model::token_label(locale, spec.key),
                overridden,
                value,
                issue: model::issue_for(&issues, spec.key)
                    .map(|i| (model::issue_text(locale, i), colour(i))),
            });
        }
        groups.push((i18n::translate(locale, model::group_label(group)), members));
    }
    let options = editor.base_options(registry, locale);
    let mut base_ids = vec![String::new()];
    let mut base_labels = vec![i18n::translate(locale, "common.none")];
    for o in options {
        base_ids.push(o.id);
        base_labels.push(o.label);
    }
    Model {
        blocked: ThemeEditor::blocked(&issues),
        issues: issues
            .iter()
            .map(|i| (model::issue_text(locale, i), colour(i)))
            .collect(),
        rows,
        groups,
        base_ids,
        base_labels,
        save_error: editor
            .save_error
            .then(|| (i18n::translate(locale, "themeEditor.saveFailed"), v.danger)),
    }
}

/// `.meta-grid`'s column count at `width`.
fn meta_columns(width: f32) -> usize {
    (((width + META_GAP) / (META_MIN + META_GAP)).floor() as usize).max(1)
}

/// `.meta-grid`'s tracks in points, offset and width: at 150 % the last of
/// three gets the spare `LayoutUnit` (326.34375 device px after two of
/// 326.328125), at 100 % the float share loses it.
pub(super) fn meta_tracks(width: f32, ppp: f32) -> Vec<(f32, f32)> {
    snap::fr_tracks(width, meta_columns(width), META_GAP, ppp)
}

/// The label column of a token row (`minmax(140px, 40%)`).
pub(super) fn label_width(width: f32) -> f32 {
    (width * LABEL_SHARE).max(LABEL_MIN).min(width)
}

pub(super) fn line_h(ctx: &Context, style: &TextStyle) -> f32 {
    style.line_box(ctx).map_or(style.size * 1.5, |l| l.height)
}

/// The pill's border box: its text, `padding: 1px 6px` and the border
/// (floored to device px).
pub(super) fn pill_size(ctx: &Context, s: &Styles, text: &str) -> Vec2 {
    let ppp = ctx.pixels_per_point();
    let w = snap::content_width(text::measure(ctx, text, &s.pill).unwrap_or(0.0), ppp);
    let b = snap::border_width(1.0, ppp);
    vec2(w + 12.0 + 2.0 * b, line_h(ctx, &s.pill) + 2.0 + 2.0 * b)
}

/// The textarea's border box: six rows, the padding, the floored border.
fn css_height(ctx: &Context, s: &Styles) -> f32 {
    let b = snap::border_width(1.0, ctx.pixels_per_point());
    CSS_ROWS * line_h(ctx, &s.mono) + 2.0 * CSS_PAD + 2.0 * b
}

/// The textarea is inline-block on its baseline (its bottom edge) in the
/// section's 16 px line at 24 px: the strut's part under the baseline
/// follows it.
fn strut_descent(ctx: &Context) -> f32 {
    TextStyle::new(16.0, 400)
        .line_height(LineHeight::Px(24.0))
        .line_box(ctx)
        .map_or(6.0, |l| l.height - l.baseline)
}

pub(super) fn layout(
    ctx: &Context,
    s: &Styles,
    m: &Model,
    locale: Locale,
    width: f32,
    field_h: f32,
) -> Layout {
    let mut blocks = Vec::new();
    let mut y = 0.0;
    let push = |blocks: &mut Vec<(f32, f32, Block)>, y: &mut f32, h: f32, b: Block| {
        blocks.push((*y, h, b));
        *y += h;
    };
    let beta_label = i18n::translate(locale, "themeEditor.beta");
    let pill = pill_size(ctx, s, &beta_label);
    let note = i18n::translate(locale, "themeEditor.betaNote");
    let note_w = width - pill.x - BETA_GAP;
    let note_h = text_height(ctx, &note, &s.beta, note_w);
    push(&mut blocks, &mut y, pill.y.max(note_h), Block::Beta);
    y += BODY_GAP;
    let rows = 6usize.div_ceil(meta_columns(width)) as f32;
    let cell = line_h(ctx, &s.label) + FIELD_GAP + field_h;
    push(
        &mut blocks,
        &mut y,
        rows * cell + (rows - 1.0) * META_GAP,
        Block::Meta,
    );
    let h4 = line_h(ctx, &s.h4);
    let issue_style = TextStyle::new(11.0, 400);
    for (title, members) in &m.groups {
        y += BODY_GAP;
        push(&mut blocks, &mut y, h4, Block::Head(title.clone()));
        y += UNDER;
        for &i in members {
            push(&mut blocks, &mut y, ROW_PAD + field_h, Block::Token(i));
            if let Some((text, _)) = &m.rows[i].issue {
                y += ROW_GAP;
                let h = text_height(ctx, text, &issue_style, width);
                push(&mut blocks, &mut y, h, Block::TokenIssue(i));
            }
            y += ROW_PAD;
        }
    }
    y += BODY_GAP;
    push(
        &mut blocks,
        &mut y,
        h4,
        Block::Head(i18n::translate(locale, "themeEditor.customCss")),
    );
    y += UNDER;
    let help = i18n::translate(locale, "themeEditor.customCssHelp");
    push(
        &mut blocks,
        &mut y,
        text_height(ctx, &help, &s.help, width),
        Block::Help,
    );
    y += UNDER;
    push(&mut blocks, &mut y, css_height(ctx, s), Block::Css);
    y += strut_descent(ctx);
    y += BODY_GAP;
    push(
        &mut blocks,
        &mut y,
        h4,
        Block::Head(i18n::translate(locale, "themeEditor.issues")),
    );
    y += UNDER;
    let mut lines: Vec<(String, Rgba)> = if m.issues.is_empty() {
        vec![(
            i18n::translate(locale, "themeEditor.noIssues"),
            Rgba::TRANSPARENT,
        )]
    } else {
        m.issues.clone()
    };
    lines.extend(m.save_error.clone());
    for (text, colour) in lines {
        let h = text_height(ctx, &text, &issue_style, width);
        push(&mut blocks, &mut y, h, Block::Issue(text, colour));
    }
    Layout { blocks, height: y }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_meta_columns_and_a_forty_percent_label_at_full_width() {
        assert_eq!(meta_tracks(672.0, 1.0).len(), 3);
        assert!((label_width(672.0) - 268.8).abs() < 0.01);
        assert_eq!(label_width(300.0), 140.0);
    }
}
