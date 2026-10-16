//! The personas grid: `PersonasPanel.svelte`'s markup and styles, and
//! `PersonaCover.svelte`.
//!
//! - `.personas-panel`: padding 18 22 24, `overflow-y: auto`, the page
//!   entrance (the shell's panel layer plays it).
//! - `.grid`: `repeat(auto-fill, minmax(140px, 1fr))`, gap 14.
//! - `.card`: a column flex box, gap 8, 1 px `--border`, radius 14,
//!   `--bg-card`, padding 10. Hover (not disabled): the border
//!   `color-mix(--fg 30%, --border)`, `translateY(-1px)`. Tinted: the border
//!   at 45 % of the persona colour (70 % on hover), the background at 8 %.
//!   Border, background and transform ease 120 ms. Disabled while a switch
//!   runs, at 0.6 opacity except the busy card, which shows `.switching`.
//! - `.card-name`: 13 px 700, ellipsis, padding 0 2 2.
//! - `.new-card`: dashed, transparent, `--fg-muted`, a 26 px plus and a
//!   12 px 600 label centred; hover `--fg` on `--bg-card`. `.card:hover`
//!   outranks `.new-card:hover`, so it lifts and takes the hover border too.
//! - `.empty-hint`: 12 px `--fg-subtle`, margin 16 2 0.
//! - `.cover`: square, radius 10, `--bg-muted`, a mosaic of up to four
//!   tiles (1 fills, 2 split, 3 = one tall and two stacked, 4 = 2x2). A tile
//!   is the account's avatar (`object-fit: cover`) or the platform logo at
//!   45 % in `color-mix(accent 75%, --fg-muted)` on `color-mix(accent 16%,
//!   --bg-muted)`.

use accshift_app::i18n::{translate, Locale};
use accshift_app::jobs::Jobs;
use accshift_app::personas::{Persona, PersonaAssignment};
use accshift_theme::ResolvedTheme;
use egui::accesskit::Role;
use egui::{pos2, vec2, Color32, Id, Rect, Sense, Ui, Vec2};

use crate::app::avatars::{AvatarTexture, AvatarTextures};
use crate::app::personas::{PersonasEvent, PersonasPanel};
use crate::app::vars::{rgba, Vars};
use crate::app::Action;
use crate::icons;
use crate::screens::persona_wizard::{self, Outcome};
use crate::theme::Rgba;
use crate::ui::anim::{self, Timing, Transition};
use crate::ui::backdrop_filter::Mark;
use crate::ui::components::controls::scroll;
use crate::ui::components::{context_click_pos, icon, platform_icons};
use crate::ui::paint::{self, Border, BoxStyle, Corners, Transform};
use crate::ui::text::{self, LineHeight, TextStyle};
use crate::ui::{dashed_border, snap};

/// `transition: border-color 120ms ease-out, background 120ms ease-out,
/// transform 120ms ease-out`.
const T120: Transition = Transition::new(0.12, Timing::EASE_OUT);

const PAD_TOP: f32 = 18.0;
const PAD_X: f32 = 22.0;
const PAD_BOTTOM: f32 = 24.0;
const GAP: f32 = 14.0;
const MIN_COLUMN: f32 = 140.0;
/// `.card { padding: 10px }`.
const PADDING: f32 = 10.0;
const RADIUS: f32 = 14.0;
const COVER_RADIUS: f32 = 10.0;
const NAME_GAP: f32 = 8.0;
/// 13 px at line-height 1.5, plus the 2 px bottom padding.
const NAME_H: f32 = 13.0 * 1.5 + 2.0;
const NEW_MIN_H: f32 = 150.0;
const PLUS: f32 = 26.0;
/// 12 px at line-height 1.5.
const NEW_LABEL_H: f32 = 18.0;
const HINT_TOP: f32 = 16.0;

/// What the grid reads.
pub struct PersonasInput<'a> {
    pub vars: &'a Vars,
    pub resolved: &'a ResolvedTheme,
    pub locale: Locale,
    pub personas: &'a [Persona],
    /// `switchingPersonaId`.
    pub switching_id: Option<&'a str>,
    pub panel: &'a mut PersonasPanel,
    pub avatars: &'a mut AvatarTextures,
    pub jobs: &'a Jobs,
}

/// The grid's columns for a content width: their count and width.
fn columns(width: f32) -> (usize, f32) {
    let count = (((width + GAP) / (MIN_COLUMN + GAP)).floor() as usize).max(1);
    let col = (width - GAP * (count as f32 - 1.0)) / count as f32;
    (count, col.max(0.0))
}

/// Border plus padding: the border is laid out in whole device pixels.
fn inset(ppp: f32) -> f32 {
    snap::border_width(1.0, ppp) + PADDING
}

fn card_height(col: f32) -> f32 {
    // Cover (square, the content width), the gap, the name: the insets
    // cancel out.
    col + NAME_GAP + NAME_H
}

fn hint_style(v: &Vars) -> TextStyle {
    TextStyle::new(12.0, 400)
        .line_height(LineHeight::Number(1.5))
        .color(v.fg_subtle.to_color32())
}

struct Layout {
    count: usize,
    col: f32,
    /// Row tops, relative to the grid, and each row's height.
    rows: Vec<(f32, f32)>,
    grid_h: f32,
    hint_h: f32,
}

fn layout(ui: &Ui, width: f32, personas: usize, v: &Vars, locale: Locale) -> Layout {
    let (count, col) = columns(width);
    let items = personas + 1;
    let mut rows = Vec::new();
    let mut top = 0.0;
    for row in 0..items.div_ceil(count) {
        let first = row * count;
        // A row holding a persona card is as tall as one; the new card
        // alone keeps its own minimum.
        let h = if first < personas {
            card_height(col)
        } else {
            NEW_MIN_H.max(2.0 * inset(ui.ctx().pixels_per_point()) + PLUS + NAME_GAP + NEW_LABEL_H)
        };
        rows.push((top, h));
        top += h + GAP;
    }
    let grid_h = (top - GAP).max(0.0);
    let hint_h = if personas == 0 {
        let style = hint_style(v);
        let w = width - 4.0;
        let text = translate(locale, "personas.emptyHint");
        text::layout(
            ui.ctx(),
            &text,
            &style,
            Rect::from_min_size(pos2(0.0, 0.0), vec2(w, 1e6)),
        )
        .map_or(18.0, |l| l.height)
    } else {
        0.0
    };
    Layout {
        count,
        col,
        rows,
        grid_h,
        hint_h,
    }
}

impl Layout {
    fn content_h(&self) -> f32 {
        let hint = if self.hint_h > 0.0 {
            HINT_TOP + self.hint_h
        } else {
            0.0
        };
        PAD_TOP + self.grid_h + hint + PAD_BOTTOM
    }

    fn item(&self, origin: egui::Pos2, index: usize) -> Rect {
        let (row, column) = (index / self.count, index % self.count);
        let (top, h) = self.rows[row];
        Rect::from_min_size(
            origin + vec2(column as f32 * (self.col + GAP), top),
            vec2(self.col, h),
        )
    }
}

/// Draws the panel into `rect` (the stage under the title bar).
pub fn draw(ui: &mut Ui, rect: Rect, mut input: PersonasInput) -> Vec<Action> {
    let mut out = Vec::new();
    // `{#if editing}`: the wizard in place of the grid.
    if let Some(mut wizard) = input.panel.editing.take() {
        match persona_wizard::draw(ui, rect, &mut wizard, &mut input) {
            Outcome::Close => {}
            outcome => {
                input.panel.editing = Some(wizard);
                if outcome == Outcome::Save {
                    out.push(Action::Personas(PersonasEvent::Save));
                }
            }
        }
        return out;
    }
    let v = input.vars;
    let full = rect.width() - 2.0 * PAD_X;
    let mut lay = layout(ui, full, input.personas.len(), v, input.locale);
    if scroll::overflows(rect.height(), lay.content_h()) {
        lay = layout(
            ui,
            full - scroll::WIDTH,
            input.personas.len(),
            v,
            input.locale,
        );
    }
    let mut offset = input.panel.scroll;
    scroll::scroll(
        ui,
        rect,
        Id::new("personas-scroll"),
        &mut offset,
        lay.content_h(),
        v,
    );
    input.panel.scroll = offset;
    let origin = pos2(rect.left() + PAD_X, rect.top() + PAD_TOP - offset);

    let disabled = input.switching_id.is_some();
    let personas = input.personas;
    for (i, persona) in personas.iter().enumerate() {
        let card = lay.item(origin, i);
        if !card.intersects(rect) {
            continue;
        }
        let busy = input.switching_id == Some(persona.id.as_str());
        if let Some(event) = persona_card(ui, card, persona, disabled, busy, &mut input) {
            out.push(Action::Personas(event));
        }
    }
    let new = lay.item(origin, input.personas.len());
    if new_card(ui, new, v, input.locale) {
        out.push(Action::Personas(PersonasEvent::New));
    }
    if lay.hint_h > 0.0 {
        let top = origin.y + lay.grid_h + HINT_TOP;
        let text = translate(input.locale, "personas.emptyHint");
        text::paint(
            ui.painter(),
            Rect::from_min_size(
                pos2(origin.x + 2.0, top),
                vec2(lay.col.max(full) - 4.0, lay.hint_h),
            ),
            &text,
            &hint_style(v),
        );
    }
    out
}

fn persona_color(resolved: &ResolvedTheme, value: &str) -> Option<Rgba> {
    match value.trim() {
        "" => None,
        css => resolved.eval_color(css).map(rgba),
    }
}

fn persona_card(
    ui: &mut Ui,
    layout: Rect,
    persona: &Persona,
    disabled: bool,
    busy: bool,
    input: &mut PersonasInput,
) -> Option<PersonasEvent> {
    let v = input.vars;
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let id = Id::new(("persona-card", persona.id.as_str()));
    let rect = snap::rect(layout, ppp);
    let response = ui.interact(
        rect,
        id,
        if disabled {
            Sense::hover()
        } else {
            Sense::click()
        },
    );
    let response = if disabled {
        response
    } else {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    };
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, !disabled, &persona.name)
    });
    ctx.accesskit_node_builder(id, |node| node.set_role(Role::Button));
    let hovered = response.hovered() && !disabled;

    let tint = persona_color(input.resolved, &persona.color);
    let border = match (tint, hovered) {
        (Some(c), true) => c.mix(0.70, v.border),
        (Some(c), false) => c.mix(0.45, v.border),
        (None, true) => v.fg.mix(0.30, v.border),
        (None, false) => v.border,
    };
    let background = tint.map_or(v.bg_card, |c| c.mix(0.08, v.bg_card));
    let border = anim::transition(&ctx, id.with("border"), border, &T120);
    let background = anim::transition(&ctx, id.with("bg"), background, &T120);
    let ty = anim::transition(
        &ctx,
        id.with("ty"),
        if hovered { -1.0f32 } else { 0.0 },
        &T120,
    );
    // `opacity: .6` fades the card as one group: a layer that lands at 0.6
    // where the renderer has layers, each shape at 0.6 where it has not.
    let faded = disabled && !busy;
    let group = faded && !cfg!(feature = "glow");
    if group {
        Mark::Begin {
            sigma: 0.0,
            opacity: 0.6,
            saturate: 1.0,
            radius: Some(RADIUS),
            ppp,
        }
        .paint(ui.painter(), rect);
    }
    let opacity = if faded && !group { 0.6 } else { 1.0 };
    let transform = Transform {
        translate: vec2(0.0, ty),
        scale: 1.0,
    };
    BoxStyle {
        background,
        radius: Corners::all(RADIUS),
        border: Border::all(snap::border_width(1.0, ppp), border),
        opacity,
        ..BoxStyle::default()
    }
    .paint_transformed(ui.painter(), rect, transform);

    let mut painter = ui.painter().clone();
    painter.multiply_opacity(opacity);
    let inset = inset(ppp);
    let content_w = layout.width() - 2.0 * inset;
    let cover = Rect::from_min_size(layout.min + vec2(inset, inset), vec2(content_w, content_w));
    paint_cover(
        &painter,
        cover,
        persona.image.as_deref(),
        &persona.assignments,
        ty,
        input,
    );
    let name = Rect::from_min_size(
        pos2(cover.left() + 2.0, cover.bottom() + NAME_GAP + ty),
        vec2(content_w - 4.0, NAME_H - 2.0),
    );
    text::paint(
        &painter,
        name,
        &persona.name,
        &TextStyle::new(13.0, 700)
            .line_height(LineHeight::Number(1.5))
            .nowrap(true)
            .color(v.fg.to_color32()),
    );
    if busy {
        // `.switching`: over the cover, `color-mix(--bg 65%, transparent)`.
        // `backdrop-filter: blur(2px)` on the veil's own rounded box.
        let veil = snap::rect(cover, ppp).translate(vec2(0.0, ty));
        Mark::Begin {
            sigma: 2.0 * ppp,
            opacity: 1.0,
            saturate: 1.0,
            radius: Some(COVER_RADIUS),
            ppp,
        }
        .paint(&painter, veil);
        BoxStyle {
            background: v.bg.mix(0.65, Rgba::TRANSPARENT),
            radius: Corners::all(COVER_RADIUS),
            ..BoxStyle::default()
        }
        .paint(&painter, veil);
        let label = translate(input.locale, "personas.switching");
        let style = TextStyle::new(11.0, 700)
            .line_height(LineHeight::Number(1.5))
            .color(v.fg.to_color32());
        let w = text::measure(&ctx, &label, &style).unwrap_or(0.0);
        let h = 11.0 * 1.5;
        text::paint(
            &painter,
            Rect::from_min_size(
                cover.min
                    + vec2(
                        snap::centre(cover.width(), w, ppp),
                        snap::centre(cover.height(), h, ppp) + ty,
                    ),
                vec2(w + 1.0, h),
            ),
            &label,
            &style,
        );
        Mark::End.paint(&painter, veil);
    }
    if group {
        Mark::End.paint(&painter, rect);
    }

    if response.clicked() {
        return Some(PersonasEvent::Switch(persona.id.clone()));
    }
    if !disabled && response.secondary_clicked() {
        let at = context_click_pos(ui).unwrap_or(rect.center());
        return Some(PersonasEvent::ContextMenu(persona.id.clone(), at));
    }
    None
}

/// The radius of each corner a tile touches.
fn corners(tl: bool, tr: bool, br: bool, bl: bool) -> Corners {
    let r = |on: bool| {
        if on {
            Vec2::splat(COVER_RADIUS)
        } else {
            Vec2::ZERO
        }
    };
    Corners {
        top_left: r(tl),
        top_right: r(tr),
        bottom_right: r(br),
        bottom_left: r(bl),
    }
}

/// `PersonaCover.svelte` in `cover` (layout units), moved by `ty`: the
/// custom `image`, else a mosaic of the first four `assignments`.
pub(super) fn paint_cover(
    painter: &egui::Painter,
    cover: Rect,
    image: Option<&str>,
    assignments: &[PersonaAssignment],
    ty: f32,
    input: &mut PersonasInput,
) {
    let v = input.vars;
    let ppp = painter.ctx().pixels_per_point();
    let shift = vec2(0.0, ty);
    let place = |r: Rect| snap::rect(r, ppp).translate(shift);
    BoxStyle {
        background: v.bg_muted,
        radius: Corners::all(COVER_RADIUS),
        ..BoxStyle::default()
    }
    .paint(painter, place(cover));
    // A custom cover wins over the mosaic.
    if let Some(image) = image.filter(|s| !s.is_empty()) {
        let px = (cover.width() * ppp).round().max(1.0) as u32;
        if let AvatarTexture::Ready { texture, .. } = input.avatars.get(input.jobs, image, px) {
            paint::paint_image(
                painter,
                texture,
                place(cover),
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Corners::all(COVER_RADIUS),
                Color32::WHITE,
            );
        }
        return;
    }
    let tiles: Vec<_> = assignments.iter().take(4).collect();
    if tiles.is_empty() {
        // `.tile.placeholder`: `--bg-muted` again, over the cover's.
        BoxStyle {
            background: v.bg_muted,
            radius: Corners::all(COVER_RADIUS),
            ..BoxStyle::default()
        }
        .paint(painter, place(cover));
        let icon_rect = Rect::from_min_size(
            cover.min
                + vec2(
                    snap::centre(cover.width(), PLUS, ppp),
                    snap::centre(cover.height(), PLUS, ppp),
                ),
            vec2(PLUS, PLUS),
        );
        icon::paint_shifted(
            painter,
            platform_icons::PERSONAS,
            icon_rect,
            v.fg_subtle,
            1.0,
            shift,
        );
        return;
    }
    let (x0, y0) = (cover.left(), cover.top());
    let (half_w, half_h) = (cover.width() / 2.0, cover.height() / 2.0);
    let (x1, y1) = (x0 + half_w, y0 + half_h);
    let (x2, y2) = (cover.right(), cover.bottom());
    let r = |a: f32, b: f32, c: f32, d: f32| Rect::from_min_max(pos2(a, b), pos2(c, d));
    let slots: Vec<(Rect, Corners)> = match tiles.len() {
        1 => vec![(cover, Corners::all(COVER_RADIUS))],
        2 => vec![
            (r(x0, y0, x1, y2), corners(true, false, false, true)),
            (r(x1, y0, x2, y2), corners(false, true, true, false)),
        ],
        3 => vec![
            (r(x0, y0, x1, y2), corners(true, false, false, true)),
            (r(x1, y0, x2, y1), corners(false, true, false, false)),
            (r(x1, y1, x2, y2), corners(false, false, true, false)),
        ],
        _ => vec![
            (r(x0, y0, x1, y1), corners(true, false, false, false)),
            (r(x1, y0, x2, y1), corners(false, true, false, false)),
            (r(x0, y1, x1, y2), corners(false, false, false, true)),
            (r(x1, y1, x2, y2), corners(false, false, true, false)),
        ],
    };
    for (assignment, (tile, radius)) in tiles.iter().zip(slots) {
        let accent = input
            .panel
            .accent(&assignment.platform_id)
            .and_then(|a| input.resolved.eval_color(a).map(rgba))
            .unwrap_or(v.fg_subtle);
        BoxStyle {
            background: accent.mix(0.16, v.bg_muted),
            radius,
            ..BoxStyle::default()
        }
        .paint(painter, place(tile));
        match input
            .panel
            .avatar_for(&assignment.platform_id, &assignment.account_id)
        {
            Some(url) => {
                // `object-fit: cover` of a square picture.
                let side = tile.width().max(tile.height());
                let px = (side * ppp).round().max(1.0) as u32;
                if let AvatarTexture::Ready { texture, .. } = input.avatars.get(input.jobs, url, px)
                {
                    let (u, w) = (tile.width() / side, tile.height() / side);
                    let uv = Rect::from_min_max(
                        pos2(0.5 - u / 2.0, 0.5 - w / 2.0),
                        pos2(0.5 + u / 2.0, 0.5 + w / 2.0),
                    );
                    paint::paint_image(painter, texture, place(tile), uv, radius, Color32::WHITE);
                }
            }
            None => {
                let svg = platform_icons::platform(&assignment.platform_id)
                    .unwrap_or(platform_icons::PERSONAS);
                // `width="45%" height="45%"`: the box, then the square
                // drawing centred in it (`xMidYMid meet`).
                let (bw, bh) = (tile.width() * 0.45, tile.height() * 0.45);
                let s = bw.min(bh);
                let at = tile.min
                    + vec2(
                        snap::centre(tile.width(), bw, ppp),
                        snap::centre(tile.height(), bh, ppp),
                    );
                let inner = vec2((bw - s) / 2.0, (bh - s) / 2.0);
                icon::paint_shifted(
                    painter,
                    svg,
                    Rect::from_min_size(at, vec2(s, s)),
                    accent.mix(0.75, v.fg_muted),
                    1.0,
                    inner + shift,
                );
            }
        }
    }
}

/// `.new-card`: true on a click.
fn new_card(ui: &mut Ui, layout: Rect, v: &Vars, locale: Locale) -> bool {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let id = Id::new("persona-new-card");
    let rect = snap::rect(layout, ppp);
    let label = translate(locale, "personas.create");
    let response = ui
        .interact(rect, id, Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    ctx.accesskit_node_builder(id, |node| node.set_role(Role::Button));
    let hovered = response.hovered();
    let fg = if hovered { v.fg } else { v.fg_muted };
    let border = anim::transition(
        &ctx,
        id.with("border"),
        if hovered {
            v.fg.mix(0.30, v.border)
        } else {
            v.border
        },
        &T120,
    );
    let background = anim::transition(
        &ctx,
        id.with("bg"),
        if hovered {
            v.bg_card
        } else {
            Rgba::TRANSPARENT
        },
        &T120,
    );
    let ty = anim::transition(
        &ctx,
        id.with("ty"),
        if hovered { -1.0f32 } else { 0.0 },
        &T120,
    );
    let shift = vec2(0.0, ty);
    BoxStyle {
        background,
        radius: Corners::all(RADIUS),
        ..BoxStyle::default()
    }
    .paint_transformed(
        ui.painter(),
        rect,
        Transform {
            translate: shift,
            scale: 1.0,
        },
    );
    dashed_border::paint(ui.painter(), rect, RADIUS, 1.0, border, shift);

    let inner = layout.shrink(inset(ppp));
    let content_h = PLUS + NAME_GAP + NEW_LABEL_H;
    let top = inner.top() + snap::centre(inner.height(), content_h, ppp);
    let plus = Rect::from_min_size(
        pos2(inner.left() + snap::centre(inner.width(), PLUS, ppp), top),
        vec2(PLUS, PLUS),
    );
    icon::paint_shifted(ui.painter(), icons::PLUS, plus, fg, 1.0, shift);
    let style = TextStyle::new(12.0, 600)
        .line_height(LineHeight::Number(1.5))
        .color(fg.to_color32());
    let w = text::measure(&ctx, &label, &style).unwrap_or(0.0);
    text::paint(
        ui.painter(),
        Rect::from_min_size(
            pos2(
                inner.left() + snap::centre(inner.width(), w, ppp),
                top + PLUS + NAME_GAP + ty,
            ),
            vec2(w + 1.0, NEW_LABEL_H),
        ),
        &label,
        &style,
    );
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_columns_of_152_fill_the_reference_width() {
        assert_eq!(columns(816.0), (5, 152.0));
        assert_eq!(card_height(152.0), 181.5);
        // Narrower than one track: one column.
        assert_eq!(columns(100.0).0, 1);
    }
}
