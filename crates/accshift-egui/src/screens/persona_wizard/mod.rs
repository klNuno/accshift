//! The persona wizard, drawn in place of the personas grid inside the
//! personas panel (padding 18 22 24, its own scroll).
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

use accshift_app::i18n::translate;
use accshift_app::platforms::PlatformAccount;
use egui::{pos2, vec2, Color32, Id, Rect, Ui, UiBuilder};

use crate::app::avatars::AvatarTexture;
use crate::app::persona_wizard::PersonaWizard;
use crate::app::vars::{rgba, Vars};
use crate::icons;
use crate::screens::personas::{paint_cover, PersonasInput};
use crate::theme::Rgba;
use crate::ui::anim::{self, Animation, Motion, Timing, Transition};
use crate::ui::components::controls::scroll;
use crate::ui::components::controls::text_input::{self, FieldStyle, InputKind, TextInputProps};
use crate::ui::paint::{self, Corners};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextStyle};

mod buttons;
mod picker;
mod platforms;

use buttons::{finish_button, icon_button, mini_button};
use picker::picker;
use platforms::platform_grid;

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
    /// Cancel: the grid comes back.
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
    scroll::scroll(
        ui,
        rect,
        Id::unique("personas-scroll"),
        &mut offset,
        total,
        v,
    );
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

    // Back: closes the picker, else asks to leave the wizard.
    let armed = wizard.picking.is_none() && wizard.armed(now);
    if let Some(at) = wizard.armed_at.filter(|_| armed) {
        let left = crate::app::persona_wizard::DISARM_AFTER - (now - at);
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(left.max(0.01)));
    }
    let back = icon_button(
        ui,
        at(head.back),
        Id::unique(("persona-wizard", "back")),
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
    let name_id = Id::unique(("persona-wizard", "name"));
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
