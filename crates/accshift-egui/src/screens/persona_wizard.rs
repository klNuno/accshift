//! `PersonaWizard.svelte`'s markup and styles, in place of the personas grid
//! inside `.personas-panel` (padding 18 22 24, its own scroll).
//!
//! - `.wizard`: a column, gap 14, `page-entrance` (200 ms ease-out).
//! - `.head`: a row, centred, gap 14: the back button (32 px, radius 8,
//!   `--fg-muted`; hover `--bg-muted` and `--fg`; armed `--danger` and white),
//!   the 72 px cover, the fields (the name input over the image buttons,
//!   gap 7), a spacer, the finish button.
//! - `.name-input`: `min(320px, 100%)`, `--bg`, 1 px `--border`, radius 8,
//!   15 px 600, padding 9 11; focus tints the border to
//!   `color-mix(--fg 35%, --border)`. Its column is as wide as the input's
//!   intrinsic width (`size=20`) or the image buttons, whichever is wider.
//! - `.mini-btn`: 1 px `--border`, radius 6, 11 px 600, padding 4 9,
//!   `--fg-muted`; hover `--fg`, `color-mix(--fg 30%, --border)` and
//!   `--bg-card`, all over 120 ms.
//! - `.finish-btn`: `color-mix(--accent 88%, #000)`, `--accent-fg`, padding
//!   9 18, 13 px 700, radius 8; disabled at 0.45 opacity (120 ms).
//! - `.hint`: 12 px `--fg-subtle`; the intro is 2 px in.
//! - `.platform-grid`: `repeat(auto-fill, minmax(150px, 1fr))`, gap 10. A
//!   card is a column, gap 8, 1 px `--border`, radius 12, `--bg-card`,
//!   `--fg-muted`, padding 14: the 26 px logo, the 12 px 700 name, the 11 px
//!   account (or a subtle dash). Hover and assigned take the border
//!   `color-mix(accent 55%, --border)` and `--fg`; assigned also tints the
//!   background at 8 % and the logo in the accent.
//! - `.picker`: a column, gap 12: the head (13 px 700, a 16 px logo in the
//!   accent, the 26 px unassign button), then a hint or `.account-grid`
//!   (`minmax(190px, 1fr)`, gap 10) of neutral cards (row, gap 10, padding
//!   10 12, radius 12, a 34 px avatar, 13 px 600 name over an 11 px
//!   username); hover `color-mix(--fg 30%, --border)`, selected `--accent`
//!   with a 1 px inset ring.

use accshift_app::i18n::{translate, translate_with, TranslationValue};
use accshift_app::platforms::PlatformAccount;
use egui::accesskit::Role;
use egui::{pos2, vec2, Color32, CursorIcon, Id, Rect, Response, Sense, Ui, UiBuilder, Vec2};

use super::personas::{paint_cover, PersonasInput};
use crate::app::avatars::AvatarTexture;
use crate::app::persona_wizard::PersonaWizard;
use crate::app::vars::{rgba, Vars};
use crate::icons;
use crate::theme::Rgba;
use crate::ui::anim::{self, Animation, Motion, Timing, Transition};
use crate::ui::backdrop_filter::Mark;
use crate::ui::components::controls::scroll;
use crate::ui::components::controls::text_input::{self, FieldStyle, InputKind, TextInputProps};
use crate::ui::components::{icon, platform_icons};
use crate::ui::paint::{self, Border, BoxShadow, BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextStyle};

/// `transition: ... 120ms ease-out` on every control of the wizard.
const T120: Transition = Transition::new(0.12, Timing::EASE_OUT);

const PAD_TOP: f32 = 18.0;
const PAD_X: f32 = 22.0;
const PAD_BOTTOM: f32 = 24.0;
/// `.wizard` and `.head` gap.
const GAP: f32 = 14.0;
const BACK: f32 = 32.0;
const COVER: f32 = 72.0;
const FIELDS_GAP: f32 = 7.0;
const NAME_MAX: f32 = 320.0;
const MINI_GAP: f32 = 8.0;
/// 12 px at line-height 1.5.
const HINT_H: f32 = 18.0;
const GRID_GAP: f32 = 10.0;
const PLATFORM_MIN: f32 = 150.0;
const ACCOUNT_MIN: f32 = 190.0;
const CARD_RADIUS: f32 = 12.0;
const PLATFORM_PAD: f32 = 14.0;
const PLATFORM_ICON: f32 = 26.0;
const PLATFORM_GAP: f32 = 8.0;
/// 11 px at line-height 1.5.
const SMALL_H: f32 = 16.5;
const PICKER_GAP: f32 = 12.0;
const UNASSIGN: f32 = 26.0;
const AVATAR: f32 = 34.0;

/// What the wizard asks of the panel this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    /// `onCancel`: the grid comes back.
    Close,
    /// `save()` with a valid form.
    Save,
}

fn name_style(v: &Vars) -> FieldStyle {
    FieldStyle {
        font: TextStyle::new(15.0, 600).line_height(LineHeight::Px(22.5)),
        line: 22.5,
        padding: vec2(11.0, 9.0),
        border: 1.0,
        radius: 8.0,
        background: v.bg,
        border_color: v.border,
        border_focus: v.fg.mix(0.35, v.border),
        placeholder: v.fg_subtle.mix(0.55, Rgba::TRANSPARENT),
        // Ascent and descent of the 15 px face, each rounded.
        caret_height: 20.0,
        caret_scaled: true,
        border_transition: false,
    }
}

fn mini_style() -> TextStyle {
    TextStyle::new(11.0, 600).line_height(LineHeight::Number(1.5))
}

fn finish_style() -> TextStyle {
    TextStyle::new(13.0, 700).line_height(LineHeight::Number(1.5))
}

fn hint_style(v: &Vars) -> TextStyle {
    TextStyle::new(12.0, 400)
        .line_height(LineHeight::Number(1.5))
        .color(v.fg_subtle.to_color32())
}

/// `repeat(auto-fill, minmax(min, 1fr))` with `gap`: the count and width of
/// the columns.
fn columns(width: f32, min: f32, gap: f32) -> (usize, f32) {
    let count = (((width + gap) / (min + gap)).floor() as usize).max(1);
    let col = (width - gap * (count as f32 - 1.0)) / count as f32;
    (count, col.max(0.0))
}

/// The head's boxes, relative to the wizard's top-left.
struct Head {
    height: f32,
    back: Rect,
    cover: Rect,
    name: Rect,
    choose: Rect,
    remove: Option<Rect>,
    finish: Rect,
}

fn head_layout(
    ui: &Ui,
    width: f32,
    has_image: bool,
    v: &Vars,
    locale: accshift_app::i18n::Locale,
) -> Head {
    let ctx = ui.ctx();
    let ppp = ctx.pixels_per_point();
    let bw = snap::border_width(1.0, ppp);
    let style = name_style(v);
    let name_h = style.height(ppp);
    let mini_h = SMALL_H + 8.0 + 2.0 * bw;
    let mini_w = |key: &str| {
        text::measure(ctx, &translate(locale, key), &mini_style()).unwrap_or(0.0) + 18.0 + 2.0 * bw
    };
    let choose_w = mini_w("personas.customImage");
    let remove_w = has_image.then(|| mini_w("personas.removeImage"));
    let actions_w = choose_w + remove_w.map_or(0.0, |w| MINI_GAP + w);
    let finish_w = text::measure(ctx, &translate(locale, "personas.finish"), &finish_style())
        .unwrap_or(0.0)
        + 36.0;
    let finish_h = 13.0 * 1.5 + 18.0;
    // `size=20`: the input's own width, the column's width unless the
    // buttons under it are wider. `min-width: 0` lets it give way.
    let intrinsic = style.font.text_field_width(ctx).unwrap_or(191.0) + 22.0 + 2.0 * bw;
    let room = (width - BACK - COVER - finish_w - 4.0 * GAP).max(0.0);
    let fields_w = intrinsic.max(actions_w).min(room);
    let fields_h = name_h + FIELDS_GAP + mini_h;
    let height = BACK.max(COVER).max(fields_h).max(finish_h);
    let at = |x: f32, w: f32, h: f32| {
        Rect::from_min_size(pos2(x, snap::centre(height, h, ppp)), vec2(w, h))
    };
    let fields_x = BACK + GAP + COVER + GAP;
    let fields = at(fields_x, fields_w, fields_h);
    let name = Rect::from_min_size(fields.min, vec2(fields_w.min(NAME_MAX), name_h));
    let mini_top = name.bottom() + FIELDS_GAP;
    let choose = Rect::from_min_size(pos2(fields.left(), mini_top), vec2(choose_w, mini_h));
    let remove = remove_w
        .map(|w| Rect::from_min_size(pos2(choose.right() + MINI_GAP, mini_top), vec2(w, mini_h)));
    Head {
        height,
        back: at(0.0, BACK, BACK),
        cover: at(BACK + GAP, COVER, COVER),
        name,
        choose,
        remove,
        finish: at(width - finish_w, finish_w, finish_h),
    }
}

/// The platform card's border-box height.
fn platform_card_h(ppp: f32) -> f32 {
    2.0 * snap::border_width(1.0, ppp)
        + 2.0 * PLATFORM_PAD
        + PLATFORM_ICON
        + PLATFORM_GAP
        + HINT_H
        + PLATFORM_GAP
        + SMALL_H
}

/// An account card's content height: the avatar or the names column.
fn account_content_h(account: &PlatformAccount) -> f32 {
    let names = if has_sub(account) {
        13.0 * 1.5 + 1.0 + SMALL_H
    } else {
        13.0 * 1.5
    };
    AVATAR.max(names)
}

fn has_sub(account: &PlatformAccount) -> bool {
    !account.username.is_empty() && account.username != account.display_name
}

fn account_name(account: &PlatformAccount) -> &str {
    [&account.display_name, &account.username, &account.id]
        .into_iter()
        .find(|s| !s.is_empty())
        .map_or("", String::as_str)
}

/// The body under the head: its height for a content `width`.
fn body_h(wizard: &PersonaWizard, input: &PersonasInput, width: f32, ppp: f32) -> f32 {
    let bw = snap::border_width(1.0, ppp);
    match wizard.picking.as_deref() {
        None => {
            let (count, _) = columns(width, PLATFORM_MIN, GRID_GAP);
            let rows = input.panel.platforms.len().div_ceil(count);
            let grid = if rows == 0 {
                0.0
            } else {
                rows as f32 * platform_card_h(ppp) + (rows as f32 - 1.0) * GRID_GAP
            };
            HINT_H + GAP + grid
        }
        Some(platform) => {
            let head = if wizard
                .selection
                .get(platform)
                .is_some_and(|s| !s.is_empty())
            {
                UNASSIGN
            } else {
                13.0 * 1.5
            };
            let accounts = input
                .panel
                .accounts_by_platform
                .get(platform)
                .map_or(&[][..], Vec::as_slice);
            let list = if input.panel.accounts_loading || accounts.is_empty() {
                HINT_H
            } else {
                let (count, _) = columns(width, ACCOUNT_MIN, GRID_GAP);
                let rows: Vec<f32> = accounts
                    .chunks(count)
                    .map(|row| {
                        row.iter().map(account_content_h).fold(0.0, f32::max) + 20.0 + 2.0 * bw
                    })
                    .collect();
                rows.iter().sum::<f32>() + (rows.len().saturating_sub(1)) as f32 * GRID_GAP
            };
            head + PICKER_GAP + list
        }
    }
}

/// Draws the wizard into `rect` (the stage under the title bar).
pub fn draw(
    ui: &mut Ui,
    rect: Rect,
    wizard: &mut PersonaWizard,
    input: &mut PersonasInput,
) -> Outcome {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let now = ui.input(|i| i.time);
    let v = input.vars;
    let has_image = wizard.image.as_deref().is_some_and(|s| !s.is_empty());

    let content_h = |width: f32| {
        let head = head_layout(ui, width, has_image, v, input.locale);
        PAD_TOP + head.height + GAP + body_h(wizard, input, width, ppp) + PAD_BOTTOM
    };
    let mut width = rect.width() - 2.0 * PAD_X;
    if scroll::overflows(rect.height(), content_h(width)) {
        width -= scroll::WIDTH;
    }
    let total = content_h(width);
    let mut offset = input.panel.scroll;
    scroll::scroll(ui, rect, Id::new("personas-scroll"), &mut offset, total, v);
    input.panel.scroll = offset;

    // `page-entrance`: from the first frame the wizard shows.
    let opened = *wizard.opened_at.get_or_insert(now);
    let entrance = Animation::new(v.motion_page_entrance, Timing::EASE_OUT);
    let (opacity, rise) = match (anim::motion(&ctx), entrance.progress(now - opened)) {
        (Motion::Full, Some(p)) => {
            ctx.request_repaint();
            let e = entrance.timing.eval(p) as f32;
            (e, 4.0 * (1.0 - e))
        }
        _ => (1.0, 0.0),
    };
    let transform = egui::emath::TSTransform::from_translation(vec2(0.0, rise));
    ui.with_visual_transform(transform, |ui| {
        let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
        child.set_clip_rect(rect.intersect(ui.clip_rect()));
        child.multiply_opacity(opacity);
        let origin = pos2(rect.left() + PAD_X, rect.top() + PAD_TOP - offset);
        contents(&mut child, origin, width, wizard, input, now)
    })
    .inner
}

fn contents(
    ui: &mut Ui,
    origin: egui::Pos2,
    width: f32,
    wizard: &mut PersonaWizard,
    input: &mut PersonasInput,
    now: f64,
) -> Outcome {
    let ctx = ui.ctx().clone();
    let v = input.vars;
    let locale = input.locale;
    let has_image = wizard.image.as_deref().is_some_and(|s| !s.is_empty());
    let head = head_layout(ui, width, has_image, v, locale);
    let at = |r: Rect| r.translate(origin.to_vec2());
    let mut outcome = Outcome::Stay;

    // Back: closes the picker, else `requestCancel`.
    let armed = wizard.picking.is_none() && wizard.armed(now);
    if let Some(at) = wizard.armed_at.filter(|_| armed) {
        let left = crate::app::persona_wizard::DISARM_AFTER - (now - at);
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(left.max(0.01)));
    }
    let back = icon_button(
        ui,
        at(head.back),
        Id::new(("persona-wizard", "back")),
        &translate(locale, "common.back"),
        icons::BACK,
        16.0,
        armed.then_some(v.danger),
        v,
    );
    if back.clicked() && wizard.picking.take().is_none() && wizard.request_cancel(input.panel, now)
    {
        outcome = Outcome::Close;
    }

    let assignments = wizard.assignments(input.panel);
    let image = wizard.image.clone();
    paint_cover(
        ui.painter(),
        at(head.cover),
        image.as_deref(),
        &assignments,
        0.0,
        input,
    );

    // The name field: a new persona's takes the focus on mount.
    let name_id = Id::new(("persona-wizard", "name"));
    if wizard.focus_name {
        wizard.focus_name = false;
        ui.memory_mut(|m| m.request_focus(name_id));
    }
    let placeholder = translate(locale, "personas.namePlaceholder");
    text_input::text_input(
        ui,
        at(head.name),
        name_id,
        &mut wizard.name,
        &TextInputProps {
            style: &name_style(v),
            kind: InputKind::Text,
            placeholder: &placeholder,
            label: &placeholder,
            max_len: Some(40),
            enabled: true,
            vars: v,
            dark: v.dark,
        },
    );
    if mini_button(
        ui,
        at(head.choose),
        "choose",
        &translate(locale, "personas.customImage"),
        v,
    ) {
        wizard.choose_image(input.jobs);
    }
    if let Some(remove) = head.remove {
        if mini_button(
            ui,
            at(remove),
            "remove",
            &translate(locale, "personas.removeImage"),
            v,
        ) {
            wizard.image = None;
        }
    }
    let can_save = wizard.can_save(input.panel);
    if finish_button(ui, at(head.finish), can_save, input) && can_save {
        outcome = Outcome::Save;
    }

    let body = origin + vec2(0.0, head.height + GAP);
    match wizard.picking.clone() {
        None => platform_grid(ui, body, width, wizard, input),
        Some(platform) => picker(ui, body, width, &platform, wizard, input),
    }
    outcome
}

/// `.icon-btn`: a 32 or 26 px square, `danger` filling it when armed.
#[allow(clippy::too_many_arguments)]
fn icon_button(
    ui: &mut Ui,
    layout: Rect,
    id: Id,
    label: &str,
    svg: &'static [u8],
    size: f32,
    armed: Option<Rgba>,
    v: &Vars,
) -> Response {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let rect = snap::rect(layout, ppp);
    let response = ui
        .interact(rect, id, Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    ctx.accesskit_node_builder(id, |node| node.set_role(Role::Button));
    let hovered = response.hovered();
    let (bg, fg) = match (armed, hovered) {
        (Some(danger), _) => (danger, Rgba::hex(0xffffff, 1.0)),
        (None, true) => (v.bg_muted, v.fg),
        (None, false) => (Rgba::TRANSPARENT, v.fg_muted),
    };
    let bg = anim::transition(&ctx, id.with("bg"), bg, &T120);
    let fg = anim::transition(&ctx, id.with("fg"), fg, &T120);
    BoxStyle {
        background: bg,
        radius: Corners::all(8.0),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), rect);
    let icon_rect = Rect::from_min_size(
        layout.min
            + vec2(
                snap::centre(layout.width(), size, ppp),
                snap::centre(layout.height(), size, ppp),
            ),
        Vec2::splat(size),
    );
    icon::paint(ui.painter(), svg, icon_rect, fg, 1.0);
    response
}

/// `.mini-btn`: true on a click.
fn mini_button(ui: &mut Ui, layout: Rect, key: &str, label: &str, v: &Vars) -> bool {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let id = Id::new(("persona-wizard", key));
    let rect = snap::rect(layout, ppp);
    let response = ui
        .interact(rect, id, Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    ctx.accesskit_node_builder(id, |node| node.set_role(Role::Button));
    let hovered = response.hovered();
    let pick = |on: Rgba, off: Rgba| if hovered { on } else { off };
    let fg = anim::transition(&ctx, id.with("fg"), pick(v.fg, v.fg_muted), &T120);
    let border = anim::transition(
        &ctx,
        id.with("border"),
        pick(v.fg.mix(0.30, v.border), v.border),
        &T120,
    );
    let bg = anim::transition(
        &ctx,
        id.with("bg"),
        pick(v.bg_card, Rgba::TRANSPARENT),
        &T120,
    );
    let bw = snap::border_width(1.0, ppp);
    BoxStyle {
        background: bg,
        radius: Corners::all(6.0),
        border: Border::all(bw, border),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), rect);
    let style = mini_style().color(fg.to_color32());
    text::paint(
        ui.painter(),
        Rect::from_min_size(
            layout.min + vec2(bw + 9.0, bw + 4.0),
            vec2(layout.width() - 2.0 * bw - 18.0 + 1.0, SMALL_H),
        ),
        label,
        &style,
    );
    response.clicked()
}

/// `.finish-btn`: true on a click, enabled or not.
fn finish_button(ui: &mut Ui, layout: Rect, enabled: bool, input: &PersonasInput) -> bool {
    let v = input.vars;
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let id = Id::new(("persona-wizard", "finish"));
    let rect = snap::rect(layout, ppp);
    let label = translate(input.locale, "personas.finish");
    let response = ui.interact(
        rect,
        id,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let response = response.on_hover_cursor(if enabled {
        CursorIcon::PointingHand
    } else {
        CursorIcon::NotAllowed
    });
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &label));
    ctx.accesskit_node_builder(id, |node| node.set_role(Role::Button));
    let opacity = anim::transition(
        &ctx,
        id.with("opacity"),
        if enabled { 1.0f32 } else { 0.45 },
        &T120,
    );
    let fg = input
        .resolved
        .eval_color("var(--accent-fg)")
        .map(rgba)
        .unwrap_or(Rgba::hex(0xffffff, 1.0));
    // `opacity` fades the button as one group: a layer where the renderer
    // has layers, each shape where it has not.
    let group = opacity < 1.0 && !cfg!(feature = "glow");
    if group {
        Mark::Begin {
            sigma: 0.0,
            opacity,
            saturate: 1.0,
            radius: Some(8.0),
            ppp,
        }
        .paint(ui.painter(), rect);
    }
    let shape_opacity = if group { 1.0 } else { opacity };
    BoxStyle {
        background: v.accent.mix(0.88, Rgba::hex(0x000000, 1.0)),
        radius: Corners::all(8.0),
        opacity: shape_opacity,
        ..BoxStyle::default()
    }
    .paint(ui.painter(), rect);
    let mut painter = ui.painter().clone();
    painter.multiply_opacity(shape_opacity);
    text::paint(
        &painter,
        Rect::from_min_size(
            layout.min + vec2(18.0, 9.0),
            vec2(layout.width() - 36.0 + 1.0, 13.0 * 1.5),
        ),
        &label,
        &finish_style().color(fg.to_color32()),
    );
    if group {
        Mark::End.paint(ui.painter(), rect);
    }
    response.clicked()
}

fn accent_of(input: &PersonasInput, platform_id: &str) -> Rgba {
    input
        .panel
        .accent(platform_id)
        .and_then(|a| input.resolved.eval_color(a).map(rgba))
        .unwrap_or(input.vars.fg_subtle)
}

/// An avatar picture in `rect` with `object-fit: cover`.
fn paint_avatar(ui: &Ui, rect: Rect, url: &str, radius: f32, input: &mut PersonasInput) -> bool {
    let ppp = ui.ctx().pixels_per_point();
    let px = (rect.width().max(rect.height()) * ppp).round().max(1.0) as u32;
    if let AvatarTexture::Ready { texture, .. } = input.avatars.get(input.jobs, url, px) {
        paint::paint_image(
            ui.painter(),
            texture,
            snap::rect(rect, ppp),
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Corners::all(radius),
            Color32::WHITE,
        );
        true
    } else {
        false
    }
}

fn platform_grid(
    ui: &mut Ui,
    top: egui::Pos2,
    width: f32,
    wizard: &mut PersonaWizard,
    input: &mut PersonasInput,
) {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = input.vars;
    let intro = translate(input.locale, "personas.choosePlatforms");
    text::paint(
        ui.painter(),
        Rect::from_min_size(top + vec2(2.0, 0.0), vec2(width - 2.0, HINT_H)),
        &intro,
        &hint_style(v),
    );
    let grid = top + vec2(0.0, HINT_H + GAP);
    let (count, _) = columns(width, PLATFORM_MIN, GRID_GAP);
    let tracks = snap::fr_tracks(width, count, GRID_GAP, ppp);
    let card_h = platform_card_h(ppp);
    let bw = snap::border_width(1.0, ppp);
    let platforms = input.panel.platforms.clone();
    let mut open = None;
    for (i, platform) in platforms.iter().enumerate() {
        let (row, column) = (i / count, i % count);
        let (left, col) = tracks[column];
        let layout = Rect::from_min_size(
            grid + vec2(left, row as f32 * (card_h + GRID_GAP)),
            vec2(col, card_h),
        );
        let id = Id::new(("persona-wizard-platform", platform.id.as_str()));
        let rect = snap::rect(layout, ppp);
        let response = ui
            .interact(rect, id, Sense::click())
            .on_hover_cursor(CursorIcon::PointingHand);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &platform.name)
        });
        ctx.accesskit_node_builder(id, |node| node.set_role(Role::Button));
        let hovered = response.hovered();
        let assigned = wizard.is_valid_assignment(input.panel, &platform.id);
        let accent = accent_of(input, &platform.id);
        let lit = hovered || assigned;
        let border = anim::transition(
            &ctx,
            id.with("border"),
            if lit {
                accent.mix(0.55, v.border)
            } else {
                v.border
            },
            &T120,
        );
        let background = anim::transition(
            &ctx,
            id.with("bg"),
            if assigned {
                accent.mix(0.08, v.bg_card)
            } else {
                v.bg_card
            },
            &T120,
        );
        let fg = anim::transition(
            &ctx,
            id.with("fg"),
            if lit { v.fg } else { v.fg_muted },
            &T120,
        );
        BoxStyle {
            background,
            radius: Corners::all(CARD_RADIUS),
            border: Border::all(bw, border),
            ..BoxStyle::default()
        }
        .paint(ui.painter(), rect);
        let inner = layout.min + vec2(bw + PLATFORM_PAD, bw + PLATFORM_PAD);
        let content_w = layout.width() - 2.0 * (bw + PLATFORM_PAD);
        let svg = platform_icons::platform(&platform.id).unwrap_or(platform_icons::PERSONAS);
        icon::paint(
            ui.painter(),
            svg,
            Rect::from_min_size(inner, Vec2::splat(PLATFORM_ICON)),
            if assigned { accent } else { fg },
            1.0,
        );
        let name_top = inner.y + PLATFORM_ICON + PLATFORM_GAP;
        text::paint(
            ui.painter(),
            Rect::from_min_size(pos2(inner.x, name_top), vec2(content_w, HINT_H)),
            &platform.name,
            &TextStyle::new(12.0, 700)
                .line_height(LineHeight::Number(1.5))
                .nowrap(false)
                .color(fg.to_color32()),
        );
        let line = Rect::from_min_size(
            pos2(inner.x, name_top + HINT_H + PLATFORM_GAP),
            vec2(content_w, SMALL_H),
        );
        let small = TextStyle::new(11.0, 400).line_height(LineHeight::Number(1.5));
        if assigned {
            let account = wizard.assigned_account(input.panel, &platform.id).cloned();
            let mut text_rect = line;
            let label = match &account {
                Some(account) => {
                    if let Some(url) = input
                        .panel
                        .avatar_for(&platform.id, &account.id)
                        .map(str::to_owned)
                    {
                        let img = Rect::from_min_size(
                            pos2(line.left(), line.top() + snap::centre(SMALL_H, 16.0, ppp)),
                            Vec2::splat(16.0),
                        );
                        paint_avatar(ui, img, &url, 4.0, input);
                        text_rect.min.x += 16.0 + 6.0;
                    }
                    account_name(account).to_string()
                }
                None => wizard
                    .selection
                    .get(&platform.id)
                    .cloned()
                    .unwrap_or_default(),
            };
            text::paint(
                ui.painter(),
                text_rect,
                &label,
                &small.nowrap(true).color(v.fg_muted.to_color32()),
            );
        } else {
            text::paint(
                ui.painter(),
                line,
                "\u{2014}",
                &small.color(v.fg_subtle.to_color32()),
            );
        }
        if response.clicked() {
            open = Some(platform.id.clone());
        }
    }
    if open.is_some() {
        wizard.picking = open;
    }
}

fn picker(
    ui: &mut Ui,
    top: egui::Pos2,
    width: f32,
    platform_id: &str,
    wizard: &mut PersonaWizard,
    input: &mut PersonasInput,
) {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = input.vars;
    let Some(platform) = input
        .panel
        .platforms
        .iter()
        .find(|p| p.id == platform_id)
        .cloned()
    else {
        wizard.picking = None;
        return;
    };
    let accent = accent_of(input, platform_id);
    let selected = wizard
        .selection
        .get(platform_id)
        .cloned()
        .unwrap_or_default();
    let head_h = if selected.is_empty() {
        13.0 * 1.5
    } else {
        UNASSIGN
    };
    let svg = platform_icons::platform(platform_id).unwrap_or(platform_icons::PERSONAS);
    icon::paint(
        ui.painter(),
        svg,
        Rect::from_min_size(
            top + vec2(0.0, snap::centre(head_h, 16.0, ppp)),
            Vec2::splat(16.0),
        ),
        accent,
        1.0,
    );
    let title = translate_with(
        input.locale,
        "personas.chooseAccount",
        &[("platform", TranslationValue::Str(platform.name.clone()))],
    );
    let title_style = TextStyle::new(13.0, 700)
        .line_height(LineHeight::Number(1.5))
        .nowrap(true)
        .color(v.fg.to_color32());
    let title_w = (width
        - 16.0
        - 8.0
        - if selected.is_empty() {
            0.0
        } else {
            8.0 + UNASSIGN
        })
    .max(0.0);
    text::paint(
        ui.painter(),
        Rect::from_min_size(
            top + vec2(16.0 + 8.0, snap::centre(head_h, 13.0 * 1.5, ppp)),
            vec2(title_w, 13.0 * 1.5),
        ),
        &title,
        &title_style,
    );
    if !selected.is_empty() {
        let label = translate(input.locale, "personas.removeFromPersona");
        let button = icon_button(
            ui,
            Rect::from_min_size(top + vec2(width - UNASSIGN, 0.0), Vec2::splat(UNASSIGN)),
            Id::new(("persona-wizard", "unassign")),
            &label,
            icons::UNASSIGN,
            14.0,
            None,
            v,
        );
        if button.clicked() {
            wizard.unassign(platform_id);
            return;
        }
    }

    let list = top + vec2(0.0, head_h + PICKER_GAP);
    let accounts = input
        .panel
        .accounts_by_platform
        .get(platform_id)
        .cloned()
        .unwrap_or_default();
    let hint = if input.panel.accounts_loading {
        Some("personas.loadingAccounts")
    } else if accounts.is_empty() {
        Some("personas.noAccountsOnPlatform")
    } else {
        None
    };
    if let Some(key) = hint {
        text::paint(
            ui.painter(),
            Rect::from_min_size(list, vec2(width, HINT_H)),
            &translate(input.locale, key),
            &hint_style(v),
        );
        return;
    }
    let bw = snap::border_width(1.0, ppp);
    let (count, _) = columns(width, ACCOUNT_MIN, GRID_GAP);
    let tracks = snap::fr_tracks(width, count, GRID_GAP, ppp);
    let mut y = list.y;
    let mut picked = None;
    for row in accounts.chunks(count) {
        let content = row.iter().map(account_content_h).fold(0.0, f32::max);
        let card_h = content + 20.0 + 2.0 * bw;
        for (column, account) in row.iter().enumerate() {
            let (left, col) = tracks[column];
            let layout = Rect::from_min_size(pos2(list.x + left, y), vec2(col, card_h));
            let id = Id::new(("persona-wizard-account", platform_id, account.id.as_str()));
            let rect = snap::rect(layout, ppp);
            let name = account_name(account).to_string();
            let response = ui
                .interact(rect, id, Sense::click())
                .on_hover_cursor(CursorIcon::PointingHand);
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
            ctx.accesskit_node_builder(id, |node| node.set_role(Role::Button));
            let is_selected = selected == account.id;
            let border = if is_selected {
                v.accent
            } else if response.hovered() {
                v.fg.mix(0.30, v.border)
            } else {
                v.border
            };
            let border = anim::transition(&ctx, id.with("border"), border, &T120);
            BoxStyle {
                background: v.bg_card,
                radius: Corners::all(CARD_RADIUS),
                border: Border::all(bw, border),
                shadows: if is_selected {
                    vec![BoxShadow {
                        offset: Vec2::ZERO,
                        blur: 0.0,
                        spread: 1.0,
                        color: v.accent,
                        inset: true,
                    }]
                } else {
                    Vec::new()
                },
                ..BoxStyle::default()
            }
            .paint(ui.painter(), rect);
            let inner = Rect::from_min_max(
                layout.min + vec2(bw + 12.0, bw + 10.0),
                layout.max - vec2(bw + 12.0, bw + 10.0),
            );
            let avatar = Rect::from_min_size(
                pos2(
                    inner.left(),
                    inner.top() + snap::centre(inner.height(), AVATAR, ppp),
                ),
                Vec2::splat(AVATAR),
            );
            BoxStyle {
                background: accent.mix(0.16, v.bg_muted),
                radius: Corners::all(8.0),
                ..BoxStyle::default()
            }
            .paint(ui.painter(), snap::rect(avatar, ppp));
            let url = input
                .panel
                .avatar_for(platform_id, &account.id)
                .map(str::to_owned);
            let shown = url.is_some_and(|url| paint_avatar(ui, avatar, &url, 8.0, input));
            if !shown {
                icon::paint(
                    ui.painter(),
                    svg,
                    Rect::from_min_size(
                        avatar.min
                            + vec2(
                                snap::centre(AVATAR, 18.0, ppp),
                                snap::centre(AVATAR, 18.0, ppp),
                            ),
                        Vec2::splat(18.0),
                    ),
                    accent.mix(0.75, v.fg_muted),
                    1.0,
                );
            }
            let names_h = if has_sub(account) {
                13.0 * 1.5 + 1.0 + SMALL_H
            } else {
                13.0 * 1.5
            };
            let names_x = avatar.right() + 10.0;
            let names_w = (inner.right() - names_x).max(0.0);
            let names_top = inner.top() + snap::centre(inner.height(), names_h, ppp);
            text::paint(
                ui.painter(),
                Rect::from_min_size(pos2(names_x, names_top), vec2(names_w, 13.0 * 1.5)),
                &name,
                &TextStyle::new(13.0, 600)
                    .line_height(LineHeight::Number(1.5))
                    .nowrap(true)
                    .color(v.fg.to_color32()),
            );
            if has_sub(account) {
                text::paint(
                    ui.painter(),
                    Rect::from_min_size(
                        pos2(names_x, names_top + 13.0 * 1.5 + 1.0),
                        vec2(names_w, SMALL_H),
                    ),
                    &account.username,
                    &TextStyle::new(11.0, 400)
                        .line_height(LineHeight::Number(1.5))
                        .nowrap(true)
                        .color(v.fg_subtle.to_color32()),
                );
            }
            if response.clicked() {
                picked = Some(account.id.clone());
            }
        }
        y += card_h + GRID_GAP;
    }
    if let Some(account) = picked {
        wizard.pick_account(platform_id, &account);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_platform_columns_at_the_reference_width() {
        let (count, col) = columns(816.0, PLATFORM_MIN, GRID_GAP);
        assert_eq!(count, 5);
        assert!((col - 155.2).abs() < 1e-3);
        assert_eq!(platform_card_h(1.0), 106.5);
    }
}
