//! `DescriptorPreviewDialog.svelte`: what a descriptor file would add, before
//! it adds it, on `BaseDialog` at `min(560px, 100vw - 24px)`.
//!
//! ```text
//! .body      a column, 12 px gaps, max-height min(60vh, 460px), overflow-y
//!            auto, padding-right 4
//!   .head    flex, wrap, baseline, 8 px gaps: the name (13 px 600), the id in
//!            `<code>` (16 px, the preflight mono stack), the picked file
//!            (11 px --fg-subtle, margin-left auto)
//!   p.line   12 px --fg-muted: the systems; --fg for the replace warning
//!   .blocked why it would add nothing, on --danger at 14 %
//!   section  h4 (11 px 700 uppercase, .04em, --fg-subtle) over a list, 5 px
//!            apart; items 3 px apart, 11 px, overflow-wrap anywhere
//!   p.quiet  nothing is written yet, --fg-subtle
//! .actions   Cancel, then Add or Replace on --fg (off while blocked or busy)
//! ```
//!
//! A step is one inline formatting context: the action (inline-block,
//! min-width 58), the target and the snapshot in the mono stack, the note.
//! Its line boxes hold both faces, so they are taller than either alone:
//! 17.5 px at 11 px, Segoe UI's ascent over Cascadia Mono's floored leading
//! and descent.
//!
//! Keys: Escape cancels, Tab cycles over the buttons, Enter presses the
//! focused one. Nothing takes focus at mount (`BaseDialog` focuses nothing).

use accshift_app::types::DescriptorPreview;
use accshift_core::platforms::descriptor::plan::PlanAction;
use egui::Pos2;

use super::*;
use crate::ui::components::controls::scroll;
use crate::ui::text::{LineBox, TextTransform, MONO_STACK};

/// The root id of the dialog, `.with(mount)`.
pub const PREVIEW_ID: &str = "accshift-descriptor-preview";
/// `.body { gap: 12px }`.
const BODY_GAP: f32 = 12.0;
/// `.body { padding-right: 4px }`.
const BODY_PAD_RIGHT: f32 = 4.0;
/// `.head { gap: 8px }`.
const HEAD_GAP: f32 = 8.0;
/// `section { gap: 5px }`.
const SECTION_GAP: f32 = 5.0;
/// `ul { gap: 3px }`.
const LIST_GAP: f32 = 3.0;
/// `.action { min-width: 58px }`.
const ACTION_MIN: f32 = 58.0;
/// `<code>` under the preflight: `font-family` only, the size inherited.
const CODE_STACK: &str = "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, \
                          \"Liberation Mono\", \"Courier New\", monospace";

/// What the dialog shows and whether it can add.
pub struct PreviewState<'a> {
    pub preview: &'a DescriptorPreview,
    /// A call is running: Add waits.
    pub busy: bool,
}

/// `ACTION_KEYS`: the label of a plan step's action.
fn action_key(action: PlanAction) -> &'static str {
    match action {
        PlanAction::Read => "descriptor.actionRead",
        PlanAction::Capture => "descriptor.actionCapture",
        PlanAction::Restore => "descriptor.actionRestore",
        PlanAction::Delete => "descriptor.actionDelete",
        PlanAction::Close => "descriptor.actionClose",
        PlanAction::Launch => "descriptor.actionLaunch",
    }
}

/// `Object.keys(descriptor.os).join(", ")`: the map is ordered like the JSON
/// the backend serialized.
fn systems(preview: &DescriptorPreview) -> String {
    preview
        .descriptor
        .os
        .keys()
        .filter_map(|os| serde_json::to_value(os).ok()?.as_str().map(str::to_owned))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The styles of the body, from the dialog's CSS.
struct Styles {
    name: TextStyle,
    code: TextStyle,
    from: TextStyle,
    line: TextStyle,
    warn: TextStyle,
    quiet: TextStyle,
    h4: TextStyle,
    /// `li`: the strut of every list line.
    li: TextStyle,
    mono: TextStyle,
    subtle: TextStyle,
    action: TextStyle,
    mono_subtle: TextStyle,
}

impl Styles {
    fn new(v: &Vars) -> Self {
        let li = TextStyle::new(11.0, 400)
            .overflow_anywhere()
            .color(v.fg_muted.to_color32());
        let mono = li.clone().family(MONO_STACK);
        Self {
            name: TextStyle::new(13.0, 600).color(v.fg.to_color32()),
            code: TextStyle::new(16.0, 400)
                .family(CODE_STACK)
                .color(v.fg.to_color32()),
            from: TextStyle::new(11.0, 400).color(v.fg_subtle.to_color32()),
            line: TextStyle::new(12.0, 400).color(v.fg_muted.to_color32()),
            warn: TextStyle::new(12.0, 400).color(v.fg.to_color32()),
            quiet: TextStyle::new(12.0, 400).color(v.fg_subtle.to_color32()),
            h4: TextStyle::new(11.0, 700)
                .letter_spacing_em(0.04)
                .transform(TextTransform::Uppercase)
                .color(v.fg_subtle.to_color32()),
            subtle: li.clone().color(v.fg_subtle.to_color32()),
            action: li.clone().color(v.fg.to_color32()),
            mono_subtle: mono.clone().color(v.fg_subtle.to_color32()),
            li,
            mono,
        }
    }
}

/// A run of one style placed on a line.
struct Placed {
    text: String,
    style: TextStyle,
    x: f32,
    line: usize,
    /// A word after the first of an inline item: see [`TextStyle::mid_item`].
    mid_item: bool,
}

/// A laid out inline formatting context: its runs and line boxes (top and
/// baseline, from the top of the block).
struct Inline {
    runs: Vec<Placed>,
    lines: Vec<(f32, f32)>,
    height: f32,
}

impl Inline {
    fn paint(&self, painter: &Painter, origin: Pos2) {
        for run in &self.runs {
            let (top, baseline) = self.lines[run.line];
            let lb = run
                .style
                .line_box(painter.ctx())
                .map_or(0.0, |l| l.baseline);
            let rect = Rect::from_min_size(
                pos2(origin.x + run.x, origin.y + top + baseline - lb),
                vec2(f32::INFINITY, 0.0),
            );
            let style = run.style.clone().nowrap(false);
            let style = if run.mid_item {
                style.mid_item()
            } else {
                style
            };
            text::paint(painter, rect, &run.text, &style);
        }
    }
}

/// A word: runs with no break opportunity between them, the collapsible
/// space before it, and a minimum width (an inline-block).
struct Word {
    runs: Vec<(String, TextStyle)>,
    space: f32,
    min_width: f32,
    /// Where the word starts a new inline item: [`Lead::Own`] when its space
    /// is an item of its own, [`Lead::Shared`] when the space ends the item
    /// before. Blink ceils every item's width to a LayoutUnit (1/64 px), so
    /// the pen snaps there; words inside one item keep the shaper's offsets.
    item: Option<Lead>,
}

#[derive(Clone, Copy)]
enum Lead {
    Own,
    Shared,
}

/// Splits `text`, one inline item, at its spaces into words of `style`; the
/// first takes `lead` as its space (the one between this run and the last).
fn words(
    out: &mut Vec<Word>,
    ctx: &Context,
    text: &str,
    style: &TextStyle,
    (lead, kind): (f32, Lead),
) {
    let space = text::measure(ctx, " ", style).unwrap_or(0.0);
    for (i, word) in text.split(' ').filter(|w| !w.is_empty()).enumerate() {
        out.push(Word {
            runs: vec![(word.to_owned(), style.clone())],
            space: if i == 0 { lead } else { space },
            min_width: 0.0,
            item: (i == 0).then_some(kind),
        });
    }
}

/// Greedy line breaking at the spaces, `overflow-wrap: anywhere` for a word
/// wider than the line; each line box holds the strut and every face on it.
fn inline(ctx: &Context, strut: &TextStyle, words: Vec<Word>, width: f32) -> Inline {
    let measure = |s: &str, style: &TextStyle| text::measure(ctx, s, style).unwrap_or(0.0);
    let lu = |x: f32| snap::content_width(x, ctx.pixels_per_point());
    let mut runs: Vec<Placed> = Vec::new();
    let (mut line, mut x, mut empty) = (0usize, 0.0f32, true);
    for word in words {
        let widths: Vec<f32> = word.runs.iter().map(|(s, st)| measure(s, st)).collect();
        let w = widths.iter().sum::<f32>().max(word.min_width);
        let start = match word.item {
            Some(Lead::Own) => lu(lu(x) + word.space),
            Some(Lead::Shared) => lu(x + word.space),
            None => x + word.space,
        };
        if !empty && start + w > width + 1e-3 {
            line += 1;
            x = 0.0;
            empty = true;
        }
        if !empty {
            x = start;
        }
        if empty && w > width + 1e-3 && word.runs.len() == 1 {
            // `overflow-wrap: anywhere`: as many characters as fit, per line.
            let Some((text, style)) = word.runs.into_iter().next() else {
                continue;
            };
            let mut piece = String::new();
            for ch in text.chars() {
                let mut next = piece.clone();
                next.push(ch);
                if !piece.is_empty() && measure(&next, &style) > width + 1e-3 {
                    runs.push(Placed {
                        text: std::mem::take(&mut piece),
                        style: style.clone(),
                        x: 0.0,
                        line,
                        mid_item: false,
                    });
                    line += 1;
                    next = ch.to_string();
                }
                piece = next;
            }
            x = measure(&piece, &style);
            runs.push(Placed {
                text: piece,
                style,
                x: 0.0,
                line,
                mid_item: false,
            });
            empty = false;
            continue;
        }
        let mut at = x;
        let mid_item = word.item.is_none() && at > 0.0;
        for (k, ((s, style), rw)) in word.runs.into_iter().zip(widths).enumerate() {
            runs.push(Placed {
                text: s,
                style,
                x: at,
                line,
                mid_item: mid_item && k == 0,
            });
            at += rw;
        }
        x += w;
        empty = false;
    }
    let count = line + 1;
    let strut = strut.line_box(ctx);
    let mut lines = Vec::with_capacity(count);
    let mut top = 0.0;
    for l in 0..count {
        let boxes: Vec<LineBox> = strut
            .into_iter()
            .chain(
                runs.iter()
                    .filter(|r| r.line == l)
                    .filter_map(|r| r.style.line_box(ctx)),
            )
            .collect();
        let baseline = boxes.iter().map(|b| b.baseline).fold(0.0, f32::max);
        let descent = boxes
            .iter()
            .map(|b| b.height - b.baseline)
            .fold(0.0, f32::max);
        lines.push((top, baseline));
        top += baseline + descent;
    }
    Inline {
        runs,
        lines,
        height: top,
    }
}

/// `.head`: a wrapping flex row aligned on baselines, the last item pushed
/// right by `margin-left: auto`. Each item is a run of its own style.
fn head(ctx: &Context, items: Vec<(String, TextStyle)>, width: f32) -> Inline {
    let mut rows: Vec<Vec<(String, TextStyle, f32)>> = vec![Vec::new()];
    let mut x = 0.0f32;
    for (s, style) in items {
        let w = snap::content_width(
            text::measure(ctx, &s, &style).unwrap_or(0.0),
            ctx.pixels_per_point(),
        )
        .min(width);
        let row = rows.last_mut().expect("a row");
        if !row.is_empty() && x + HEAD_GAP + w > width + 1e-3 {
            rows.push(vec![(s, style, w)]);
            x = w;
        } else {
            x += if row.is_empty() { w } else { HEAD_GAP + w };
            row.push((s, style, w));
        }
    }
    let mut runs = Vec::new();
    let mut lines = Vec::new();
    let mut top = 0.0;
    let last_row = rows.len() - 1;
    for (r, row) in rows.into_iter().enumerate() {
        let boxes: Vec<LineBox> = row
            .iter()
            .filter_map(|(_, st, _)| st.line_box(ctx))
            .collect();
        let baseline = boxes.iter().map(|b| b.baseline).fold(0.0, f32::max);
        let height = boxes
            .iter()
            .map(|b| baseline - b.baseline + b.height)
            .fold(0.0, f32::max);
        lines.push((top, baseline));
        let n = row.len();
        let mut x = 0.0;
        for (i, (s, style, w)) in row.into_iter().enumerate() {
            // `.from { margin-left: auto }`: the last item of the head.
            let left = if r == last_row && i + 1 == n && i > 0 {
                (width - w).max(x)
            } else {
                x
            };
            runs.push(Placed {
                text: s,
                style,
                x: left,
                line: r,
                mid_item: false,
            });
            x = left + w + HEAD_GAP;
        }
        top += height + HEAD_GAP;
    }
    Inline {
        runs,
        lines,
        height: top - HEAD_GAP,
    }
}

/// One block of the body.
enum Block {
    Inline(Inline),
    /// A paragraph of one style, `overflow-wrap` as the style says.
    Para(String, TextStyle, f32),
    /// `.blocked`: the paragraph on its danger tint.
    Blocked(String, TextStyle, f32),
}

impl Block {
    fn height(&self) -> f32 {
        match self {
            Block::Inline(i) => i.height,
            Block::Para(_, _, h) => *h,
            // `padding: 8px 10px`.
            Block::Blocked(_, _, h) => h + 16.0,
        }
    }
}

/// The body's blocks, each with its gap before it.
fn blocks(
    ctx: &Context,
    s: &Styles,
    locale: Locale,
    preview: &DescriptorPreview,
    width: f32,
) -> Vec<(f32, Block)> {
    let t = |key: &str| i18n::translate(locale, key);
    let para = |text: String, style: &TextStyle| {
        let h = text_height(ctx, &text, style, width);
        Block::Para(text, style.clone(), h)
    };
    let mut out = vec![(
        0.0,
        Block::Inline(head(
            ctx,
            vec![
                (preview.descriptor.name.clone(), s.name.clone()),
                (preview.descriptor.id.clone(), s.code.clone()),
                (preview.source.clone(), s.from.clone()),
            ],
            width,
        )),
    )];
    let os = i18n::translate_with(
        locale,
        "descriptor.previewOs",
        &[("list", i18n::TranslationValue::Str(systems(preview)))],
    );
    out.push((BODY_GAP, para(os, &s.line)));
    if !preview.blocked.is_empty() {
        let h = text_height(ctx, &preview.blocked, &s.warn, width - 20.0);
        out.push((
            BODY_GAP,
            Block::Blocked(preview.blocked.clone(), s.warn.clone(), h),
        ));
    } else if preview.replaces {
        out.push((BODY_GAP, para(t("descriptor.previewReplaces"), &s.warn)));
    }
    if let Some(plan) = &preview.plan {
        let section = |out: &mut Vec<(f32, Block)>, title: &str, items: Vec<Block>| {
            out.push((BODY_GAP, para(t(title), &s.h4)));
            for (i, item) in items.into_iter().enumerate() {
                out.push((if i == 0 { SECTION_GAP } else { LIST_GAP }, item));
            }
        };
        let roots = plan
            .roots
            .iter()
            .map(|r| para(r.clone(), &s.mono))
            .collect();
        section(&mut out, "descriptor.previewRoots", roots);
        let li_space = text::measure(ctx, " ", &s.li).unwrap_or(0.0);
        let steps = plan
            .steps
            .iter()
            .map(|step| {
                let mut w = vec![Word {
                    runs: vec![(t(action_key(step.action)), s.action.clone())],
                    space: 0.0,
                    min_width: ACTION_MIN,
                    item: Some(Lead::Own),
                }];
                let li_lead = (li_space, Lead::Own);
                words(&mut w, ctx, &step.target, &s.mono, li_lead);
                if !step.snapshot.is_empty() {
                    // `.snapshot::before { content: "< " }` in --fg-subtle,
                    // one item with its space.
                    words(&mut w, ctx, "<", &s.mono_subtle, li_lead);
                    let mono_space = text::measure(ctx, " ", &s.mono).unwrap_or(0.0);
                    let lead = (mono_space, Lead::Shared);
                    words(&mut w, ctx, &step.snapshot, &s.mono, lead);
                }
                if !step.note.is_empty() {
                    // `.note::before { content: " " }` collapses into the
                    // space before it.
                    words(&mut w, ctx, &step.note, &s.subtle, li_lead);
                }
                Block::Inline(inline(ctx, &s.li, w, width))
            })
            .collect();
        section(&mut out, "descriptor.previewSteps", steps);
        if !plan.warnings.is_empty() {
            let warnings = plan
                .warnings
                .iter()
                .map(|w| para(w.clone(), &s.subtle))
                .collect();
            section(&mut out, "descriptor.previewWarnings", warnings);
        }
    } else if !preview.plan_problem.is_empty() {
        let text = i18n::translate_with(
            locale,
            "descriptor.previewNoPlan",
            &[(
                "problem",
                i18n::TranslationValue::Str(preview.plan_problem.clone()),
            )],
        );
        out.push((BODY_GAP, para(text, &s.warn)));
    }
    out.push((
        BODY_GAP,
        para(t("descriptor.previewNothingWritten"), &s.quiet),
    ));
    out
}

fn blocks_height(blocks: &[(f32, Block)]) -> f32 {
    blocks.iter().map(|(gap, b)| gap + b.height()).sum()
}

/// `DescriptorPreviewDialog.svelte` over the window.
pub fn descriptor_preview_dialog(
    ui: &mut Ui,
    props: &DialogProps,
    state: &PreviewState,
) -> Vec<DialogEvent> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let id = Id::unique(PREVIEW_ID).with(props.mount);
    let preview = state.preview;
    let styles = Styles::new(v);
    let title = i18n::translate(props.locale, "descriptor.previewTitle");
    let cancel_label = i18n::translate(props.locale, "common.cancel");
    let confirm_label = i18n::translate(
        props.locale,
        if preview.replaces {
            "descriptor.replace"
        } else {
            "common.add"
        },
    );
    let width = 560f32.min(crate::zoom::viewport(&ctx).x - 24.0);
    let border = snap::border_width(v.border_px(), ppp);
    let inner = width - 2.0 * (border + PADDING);
    let max_h = (props.viewport.height() * 0.6).min(460.0);
    // Laid out without the bar, then with it when that overflows.
    let mut content_w = inner - BODY_PAD_RIGHT;
    let mut body = blocks(&ctx, &styles, props.locale, preview, content_w);
    let mut content_h = blocks_height(&body);
    if scroll::overflows(max_h, content_h) {
        content_w -= scroll::WIDTH;
        body = blocks(&ctx, &styles, props.locale, preview, content_w);
        content_h = blocks_height(&body);
    }
    let body_h = content_h.min(max_h);
    let mut cancel = cancel_button(id.with("cancel"), &cancel_label, v);
    cancel.eased = false;
    let confirm = Button {
        id: id.with("confirm"),
        label: &confirm_label,
        font: TextStyle::new(12.0, 600).line_height(LineHeight::Px(18.0)),
        border: 0.0,
        enabled: preview.blocked.is_empty() && !state.busy,
        eased: false,
    };
    let actions_h = 30.0 + 2.0 * snap::border_width(cancel.border, ppp);
    let mut order = vec![cancel.id];
    if confirm.enabled {
        order.push(confirm.id);
    }

    let focused = ctx.memory(|m| m.focused());
    trap_focus(&ctx, &order, focused);
    let scroll_id = id.with("scroll");
    let mut offset = ctx.data(|d| d.get_temp::<f32>(scroll_id)).unwrap_or(0.0);
    let (overlay_clicked, (cancel_hit, confirm_hit)) = base(
        ui,
        props,
        id,
        &title,
        width,
        &[(body_h, 0.0), (actions_h, 0.0)],
        |ui, column| {
            let viewport = column.children[0];
            let mut content = ui.new_child(UiBuilder::new().max_rect(viewport));
            // `overflow-y: auto` clips on the pixel-snapped padding box.
            content.set_clip_rect(snap::rect(viewport, ppp).intersect(ui.clip_rect()));
            let painter = content.painter().clone();
            let mut y = viewport.top() - offset;
            for (gap, block) in &body {
                y += gap;
                let h = block.height();
                let r = Rect::from_min_size(pos2(viewport.left(), y), vec2(content_w, h));
                match block {
                    Block::Inline(i) => i.paint(&painter, r.min),
                    Block::Para(text, style, _) => {
                        text::paint(&painter, r, text, style);
                    }
                    Block::Blocked(text, style, _) => {
                        BoxStyle {
                            background: Rgba::new(
                                v.danger.r,
                                v.danger.g,
                                v.danger.b,
                                v.danger.a * 0.14,
                            ),
                            radius: Corners::all(6.0),
                            ..BoxStyle::default()
                        }
                        .paint(&painter, snap::rect(r, ppp));
                        let text_rect = Rect::from_min_max(
                            pos2(r.left() + 10.0, r.top() + 8.0),
                            pos2(r.right() - 10.0, r.bottom() - 8.0),
                        );
                        text::paint(&painter, text_rect, text, style);
                    }
                }
                y += h;
            }
            scroll::scroll(ui, viewport, scroll_id, &mut offset, content_h, v);
            let rects = button_rects(&ctx, column.children[1], &[&cancel, &confirm]);
            let c = button(ui, rects[0], &cancel, props, cancel_look(v), false);
            // `.btn-confirm:not(:disabled):hover { filter: brightness(0.9) }`.
            let dim = |c: Rgba| Rgba::new(c.r * 0.9, c.g * 0.9, c.b * 0.9, c.a);
            let k = button(
                ui,
                rects[1],
                &confirm,
                props,
                |hovered| ButtonLook {
                    background: if hovered { dim(v.fg) } else { v.fg },
                    color: if hovered { dim(v.bg_solid) } else { v.bg_solid },
                    border_color: Rgba::TRANSPARENT,
                    opacity: if confirm.enabled { 1.0 } else { 0.5 },
                },
                false,
            );
            (c, k)
        },
    );
    ctx.data_mut(|d| d.insert_temp(scroll_id, offset));
    let enter = ctx.input(|i| i.key_pressed(Key::Enter));
    let escape = ctx.input(|i| i.key_pressed(Key::Escape));
    let mut events = Vec::new();
    if escape || overlay_clicked || cancel_hit.clicked || (enter && cancel_hit.focused) {
        events.push(DialogEvent::Cancel);
    } else if confirm_hit.clicked || (enter && confirm_hit.focused) {
        events.push(DialogEvent::Confirm);
    }
    events
}
