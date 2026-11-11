//! The row's and the preview's avatar: the picture, the initials or the
//! spinner, over the muted background.

use accshift_app::display::AccountCardItem;
use egui::{pos2, vec2, Color32, Id, Pos2, Rect, Ui};

use super::{ceil_unit, SPIN};
use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim::{self, Animation, Fill, Keyframes, Timing};
use crate::ui::components::account_card::{cover_uv, paint_ring_loader, AvatarContent};
use crate::ui::components::avatar_fade_in;
use crate::ui::paint::{self, BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextStyle};

/// The row's and the preview's avatar sizes.
#[derive(Clone, Copy)]
pub(super) struct AvatarSize {
    initials: f32,
    loader: f32,
    loader_border: f32,
}

impl AvatarSize {
    pub(super) const ROW: AvatarSize = AvatarSize {
        initials: 12.0,
        loader: 15.0,
        loader_border: 2.0,
    };
    pub(super) const PREVIEW: AvatarSize = AvatarSize {
        initials: 36.0,
        loader: 42.0,
        loader_border: 4.0,
    };
}

/// The placeholder's fade in, counted from its mount: hidden for the
/// theme's avatar defer delay, then faded in over its avatar fade time,
/// easing out.
fn deferred(ui: &Ui, id: Id, v: &Vars) -> f32 {
    anim::animate(
        ui.ctx(),
        id,
        &Keyframes::from_to(0.0f32, 1.0),
        &Animation::new(v.motion_avatar_fade, Timing::EASE_OUT)
            .with_delay(v.motion_avatar_defer)
            .with_fill(Fill::Backwards),
        1.0,
    )
}

/// A spinner of the given size, turning from its mount.
fn loader(ui: &Ui, id: Id, centre: Pos2, size: AvatarSize, fade: f32, v: &Vars) {
    let turn = anim::animate(
        ui.ctx(),
        id.with("spin"),
        &Keyframes::from_to(0.0f32, 1.0),
        &Animation::new(SPIN, Timing::LINEAR).infinite(),
        0.0,
    );
    let rest = v.fg.mix(0.24, Rgba::TRANSPARENT);
    paint_ring_loader(
        ui.painter(),
        centre,
        size.loader,
        size.loader_border,
        turn,
        rest.alpha(rest.a * fade),
        v.fg.alpha(v.fg.a * fade),
    );
}

/// A square crop covering a box that flex shrink made shorter than wide:
/// Blink pixel-snaps the whole picture, centred on the box, then clips it
/// to the box, so `uv` keeps the band the box shows.
pub(super) fn box_uv(uv: Rect, rect: Rect, ppp: f32) -> Rect {
    if rect.height() >= rect.width() {
        return uv;
    }
    let image = snap::rect(
        Rect::from_center_size(rect.center(), vec2(rect.width(), rect.width())),
        ppp,
    );
    let shown = snap::rect(rect, ppp);
    let at = |y: f32| uv.top() + uv.height() * (y - image.top()) / image.height();
    Rect::from_min_max(
        pos2(uv.left(), at(shown.top())),
        pos2(uv.right(), at(shown.bottom())),
    )
}

/// The avatar: the muted background under the picture (faded in once
/// decoded), or the initials on their gradient, or the spinner. A switching
/// account's picture shows at half brightness, blurred when the host gave
/// a blurred copy, under the spinner.
pub(super) fn paint_avatar(
    ui: &Ui,
    id: Id,
    rect: Rect,
    content: &AvatarContent,
    a: &AccountCardItem,
    v: &Vars,
    size: AvatarSize,
) {
    let ctx = ui.ctx();
    let ppp = ctx.pixels_per_point();
    let painter = ui.painter();
    let snapped = snap::rect(rect, ppp);
    let radius = Corners::all(v.avatar_radius);
    BoxStyle {
        background: v.bg_muted,
        radius,
        ..BoxStyle::default()
    }
    .paint(painter, snapped);
    let revealed = !matches!(content, AvatarContent::Decoding);
    let reveal = avatar_fade_in::avatar_opacity(ctx, id.with("avatar"), revealed, v);
    match content {
        AvatarContent::Picture(picture) => {
            let tint = |alpha: f32, dim: f32| {
                let a8 = (alpha * 255.0).round().clamp(0.0, 255.0) as u8;
                let g = (alpha * dim * 255.0).round().clamp(0.0, 255.0) as u8;
                Color32::from_rgba_premultiplied(g, g, g, a8)
            };
            let (texture, uv) = match (a.switching, picture.blurred) {
                (true, Some(blurred)) => (blurred, cover_uv([1, 1], 1.0)),
                _ => (picture.texture, cover_uv(picture.size, 1.0)),
            };
            let dim = if a.switching { 0.5 } else { 1.0 };
            paint::paint_image(
                painter,
                texture,
                snapped,
                box_uv(uv, rect, ppp),
                radius,
                tint(reveal, dim),
            );
            if a.switching {
                loader(ui, id, rect.center(), size, 1.0, v);
            }
        }
        AvatarContent::Decoding => {}
        AvatarContent::Initials {
            background,
            gradient,
        } => {
            let fade = if a.avatar.pending {
                deferred(ui, id.with("initials-in"), v)
            } else {
                1.0
            };
            BoxStyle {
                background: *background,
                images: gradient.iter().cloned().collect(),
                radius,
                opacity: fade,
                ..BoxStyle::default()
            }
            .paint(painter, snapped);
            let seed = if a.account.display_name.is_empty() {
                &a.account.username
            } else {
                &a.account.display_name
            };
            let initials = accshift_app::actions::avatar_fallback::avatar_initials(seed);
            let style = TextStyle::new(size.initials, 600)
                .line_height(LineHeight::Number(v.line_height))
                .color(v.fg.alpha(v.fg.a * fade).to_color32());
            let w = ceil_unit(text::measure(ctx, &initials, &style).unwrap_or(0.0), ppp);
            let h = size.initials * v.line_height;
            let at = pos2(
                rect.left() + snap::centre(rect.width(), w, ppp),
                rect.top() + snap::centre(rect.height(), h, ppp),
            );
            text::paint(
                painter,
                Rect::from_min_size(at, vec2(w, h)),
                &initials,
                &style,
            );
            if a.switching {
                loader(ui, id, rect.center(), size, 1.0, v);
            }
        }
        AvatarContent::Loading => {
            let fade = deferred(ui, id.with("loader-in"), v);
            loader(ui, id, rect.center(), size, fade, v);
        }
    }
}
