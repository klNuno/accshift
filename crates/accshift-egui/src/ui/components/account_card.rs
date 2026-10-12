//! `src/lib/shared/components/AccountCard.svelte`, outer box only.
//!
//! This is the stub the workspace grid lays out with: the card box, its
//! hover lift and shadow, the active rings, the custom colour, the avatar
//! box with its picture (or initials) and the name line are drawn with the
//! component's CSS values; the username line, notes, badges, last login,
//! warnings, the confirm overlay, the loaders, the marquee and the extension
//! panel are not. The full port replaces this file behind the same
//! signature.
//!
//! - `.card`: `--grid-card-width` by `--grid-card-min-height`, padding
//!   `--grid-card-padding`, radius `--grid-card-radius`, `--bg-card` under
//!   `--card-bg-image`; background, transform and shadow transition 180 ms
//!   ease-out; `card-entrance` on mount.
//! - `:not(.active):hover`: `--bg-card-hover`, `translateY(-2px)`, `0 12px
//!   24px rgba(0,0,0,.18)`; `:active` `translateY(0) scale(.985)`.
//! - `.custom-color`: `color-mix(c 24%, --bg-card)`, hover `color-mix(c 32%,
//!   --bg-card-hover)`.
//! - `.active`: rings `0 0 0 2px color-mix(--fg 62%, transparent), 0 0 0 4px
//!   color-mix(--bg-solid 45%, transparent)` on `--bg-card-hover`; with a
//!   custom colour `2px color-mix(--fg 72%)`, `5px color-mix(c 50%,
//!   color-mix(--bg-solid 62%, transparent))`.
//! - `.avatar`: `--grid-card-avatar-size`, `margin: 0 auto 8px`, radius
//!   `--avatar-radius`, `--bg-muted`; on card hover `--bg-elevated` and
//!   `translateY(-1px)`, the picture `scale(1.04)`; `.avatar.active` outline
//!   2 px `color-mix(--fg 20%, transparent)`.
//! - `.name-inner`: 12 px 500 `--fg`, `line-height: 1.2`, centred.

use egui::accesskit::Role;
use egui::{pos2, vec2, Color32, Id, Pos2, Rect, Sense, TextureId, Ui, WidgetInfo, WidgetType};

use accshift_app::actions::avatar_fallback::avatar_initials;
use accshift_app::display::AccountCardItem;

use super::avatar_fade_in;
use super::folder_card::{entrance, map_rect};
use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim::{self, Timing, Transition};
use crate::ui::paint::{self, BoxShadow, BoxStyle, Corners, Gradient, Transform};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextAlign, TextStyle};

/// `transition: background 180ms ease-out, transform 180ms ease-out,
/// box-shadow 180ms ease-out`.
const T180: Transition = Transition::new(0.18, Timing::EASE_OUT);
/// `.avatar { transition: background 150ms }` (`ease`).
const T150: Transition = Transition::new(0.15, Timing::EASE);
/// `img { transition: transform 220ms ease-out }`.
const T220: Transition = Transition::new(0.22, Timing::EASE_OUT);
/// Where `.name-inner`'s line box starts under the name row's top, measured
/// in WebView2 (the `.name` line box around the inline-block); the full port
/// lays the name section out instead.
const NAME_INNER_TOP: f32 = 3.609;

/// The decoded avatar, as the workspace's texture cache hands it over.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AvatarPicture {
    pub texture: TextureId,
    /// The texture's size in device pixels (for `object-fit: cover`).
    pub size: [usize; 2],
}

/// What the avatar box shows.
#[derive(Clone, Debug, PartialEq)]
pub enum AvatarContent {
    /// `avatarUrl` set and decoded.
    Picture(AvatarPicture),
    /// `avatarUrl` set, still decoding: the `<img>` is mounted at opacity 0.
    Decoding,
    /// No picture: the initials on `getAvatarGradientStyle(seed)`
    /// (`!isLoadingAvatar`): its `background-color` and gradient.
    Initials {
        background: Rgba,
        gradient: Option<Gradient>,
    },
    /// `isLoadingAvatar`: the box alone (the loader is not ported).
    Loading,
}

/// `AccountCard.svelte`'s props, minus the callbacks. The display
/// pipeline's [`AccountCardItem`] carries every per-account prop; the rest
/// are the workspace's.
#[derive(Clone, Debug)]
pub struct AccountCardProps<'a> {
    pub vars: &'a Vars,
    /// Stable for one mount of the card.
    pub id: Id,
    pub item: &'a AccountCardItem,
    /// `cardColor` resolved, `None` for `""`.
    pub card_color: Option<Rgba>,
    pub avatar: AvatarContent,
    pub focus_visible: bool,
}

/// What the card reports.
#[derive(Clone, Debug, PartialEq)]
pub enum CardEvent {
    /// Click: `onActivate` (selection and keyboard focus).
    Activate,
    /// `onSwitch`: a click with `singleClickSwitch` (bulk edit). The double
    /// click confirm flow belongs to the full port.
    Switch,
    /// Right click: `onContextMenu`.
    ContextMenu(Pos2),
}

/// Draws the card over `rect` (its layout box) and returns what the user did.
pub fn account_card(ui: &mut Ui, rect: Rect, props: &AccountCardProps) -> Vec<CardEvent> {
    let v = props.vars;
    let item = props.item;
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let rect = snap::rect(rect, ppp);
    let id = props.id;
    let disabled = item.pending_setup;
    let response = ui.interact(
        rect,
        id,
        if disabled {
            Sense::hover()
        } else {
            Sense::click()
        },
    );
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| {
        WidgetInfo::selected(WidgetType::Button, !disabled, item.active, &item.name)
    });
    ctx.accesskit_node_builder(id, |node| node.set_role(Role::Button));

    let hovered = response.hovered();
    let pressed = response.is_pointer_button_down_on();
    let active = item.active;
    let dragged = item.dragged;
    let lift = hovered && !active && !dragged;
    let transparent = Rgba::TRANSPARENT;
    let background = match (props.card_color, active, lift) {
        (Some(c), false, true) => c.mix(0.32, v.bg_card_hover),
        (Some(c), _, _) => c.mix(0.24, v.bg_card),
        (None, true, _) | (None, false, true) => v.bg_card_hover,
        (None, false, false) => v.bg_card,
    };
    let (ty, scale) = if dragged {
        (0.0, 0.95)
    } else if pressed && !active {
        (0.0, 0.985)
    } else if lift {
        (-2.0, 1.0)
    } else {
        (0.0, 1.0)
    };
    let background = anim::transition(&ctx, id.with("bg"), background, &T180);
    let ty = anim::transition(&ctx, id.with("ty"), ty, &T180);
    let scale = anim::transition(&ctx, id.with("scale"), scale, &T180);
    let avatar_bg = anim::transition(
        &ctx,
        id.with("avatar-bg"),
        if lift { v.bg_elevated } else { v.bg_muted },
        &T150,
    );
    let avatar_lift = anim::transition(
        &ctx,
        id.with("avatar-ty"),
        if lift { -1.0f32 } else { 0.0 },
        &T180,
    );
    let zoom = anim::transition(
        &ctx,
        id.with("img-scale"),
        if lift { 1.04f32 } else { 1.0 },
        &T220,
    );

    let (enter_opacity, enter_scale) = entrance(ui, id, v);
    let transform = if enter_opacity < 1.0 {
        Transform::scale(enter_scale)
    } else {
        Transform {
            translate: vec2(0.0, ty),
            scale,
        }
    };
    let opacity = enter_opacity * if dragged { 0.4 } else { 1.0 };

    // Shadows: the active rings, else the hover shadow. Ring lists and the
    // drop shadow interpolate as lists, padded with transparent shadows.
    let shadows = if active {
        let (halo, band, band_w) = match props.card_color {
            Some(c) => (
                v.fg.mix(0.72, transparent),
                c.mix(0.5, v.bg_solid.mix(0.62, transparent)),
                5.0,
            ),
            None => (
                v.fg.mix(0.62, transparent),
                v.bg_solid.mix(0.45, transparent),
                4.0,
            ),
        };
        vec![ring(2.0, halo), ring(band_w, band)]
    } else if lift {
        vec![BoxShadow {
            offset: vec2(0.0, 12.0),
            blur: 24.0,
            spread: 0.0,
            color: Rgba::new(0.0, 0.0, 0.0, 0.18),
            inset: false,
        }]
    } else {
        Vec::new()
    };
    BoxStyle {
        background,
        images: v.card_bg_image.clone(),
        radius: Corners::all(v.radius_md),
        shadows,
        opacity,
        ..BoxStyle::default()
    }
    .paint_transformed(ui.painter(), rect, transform);

    let mut painter = ui.painter().clone();
    painter.multiply_opacity(opacity);

    // Avatar box.
    let pad = v.grid_card_padding();
    let size = v.grid_card_avatar_size();
    let content_w = rect.width() - 2.0 * pad;
    let avatar = Rect::from_min_size(
        pos2(
            rect.left() + pad + snap::centre(content_w, size, ppp),
            rect.top() + pad,
        ),
        vec2(size, size),
    );
    let lifted = avatar.translate(vec2(0.0, avatar_lift));
    let mapped = map_rect(rect, transform, lifted);
    let avatar_transform = Transform {
        translate: mapped.center() - avatar.center(),
        scale: transform.scale,
    };
    BoxStyle {
        background: avatar_bg,
        radius: Corners::all(v.avatar_radius),
        ..BoxStyle::default()
    }
    .paint_transformed(&painter, avatar, avatar_transform);
    let radius = Corners::all(v.avatar_radius * transform.scale);
    match &props.avatar {
        AvatarContent::Picture(picture) => {
            let reveal = avatar_fade_in::avatar_opacity(&ctx, id, true, v);
            if reveal > 0.0 {
                let alpha = (reveal * 255.0).round() as u8;
                paint::paint_image(
                    &painter,
                    picture.texture,
                    snap::rect(mapped, ppp),
                    cover_uv(picture.size, zoom),
                    radius,
                    Color32::from_white_alpha(alpha),
                );
            }
        }
        AvatarContent::Decoding => {
            avatar_fade_in::avatar_opacity(&ctx, id, false, v);
        }
        AvatarContent::Initials {
            background,
            gradient,
        } => {
            BoxStyle {
                background: *background,
                images: gradient.iter().cloned().collect(),
                radius: Corners::all(v.avatar_radius),
                ..BoxStyle::default()
            }
            .paint_transformed(&painter, avatar, avatar_transform);
            let style = TextStyle::new(20.0, 600)
                .line_height(LineHeight::Number(1.5))
                .nowrap(false)
                .align(TextAlign::Center)
                .color(v.fg.to_color32());
            let line = 30.0;
            let text_rect = Rect::from_min_size(
                pos2(mapped.left(), mapped.center().y - line / 2.0),
                vec2(mapped.width(), line),
            );
            text::paint(&painter, text_rect, &avatar_initials(&item.name), &style);
        }
        AvatarContent::Loading => {}
    }
    if active {
        // `.avatar.active { outline: 2px solid color-mix(--fg 20%) }`.
        BoxStyle {
            radius: Corners::all(v.avatar_radius),
            shadows: vec![ring(
                snap::border_width(2.0, ppp),
                v.fg.mix(0.2, transparent),
            )],
            ..BoxStyle::default()
        }
        .paint_transformed(&painter, avatar, avatar_transform);
    }

    // Name.
    let name_top = rect.top() + pad + size + 8.0 + NAME_INNER_TOP;
    let name_box = Rect::from_min_size(pos2(rect.left() + pad, name_top), vec2(content_w, 14.4));
    let name_mapped = map_rect(rect, transform, name_box);
    let name_rect = Rect::from_center_size(name_mapped.center(), name_box.size());
    let style = TextStyle::new(12.0, 500)
        .line_height(LineHeight::Number(1.2))
        .nowrap(false)
        .align(TextAlign::Center)
        .color(v.fg.to_color32());
    let mut name_painter = painter.clone();
    name_painter.set_clip_rect(painter.clip_rect().intersect(Rect::from_min_max(
        pos2(name_rect.left(), name_rect.top() - 4.0),
        pos2(name_rect.right(), name_rect.bottom() + 6.0),
    )));
    let mut nowrap = style.clone();
    nowrap.nowrap = true;
    text::paint(&name_painter, name_rect, &item.name, &nowrap);

    if props.focus_visible && response.has_focus() {
        super::focus::card_ring(ui, rect, v.radius_md, v);
    }
    let mut events = Vec::new();
    if !disabled && !dragged && response.clicked() {
        events.push(CardEvent::Activate);
        if item.single_click_switch {
            events.push(CardEvent::Switch);
        }
    }
    if response.secondary_clicked() {
        let at = response.interact_pointer_pos().unwrap_or(rect.center());
        events.push(CardEvent::ContextMenu(at));
    }
    events
}

/// `0 0 0 <spread> color`.
fn ring(spread: f32, color: Rgba) -> BoxShadow {
    BoxShadow {
        offset: egui::Vec2::ZERO,
        blur: 0.0,
        spread,
        color,
        inset: false,
    }
}

/// `object-fit: cover` of a `size` picture in a square box, then `scale()`
/// about the centre (the box clips it).
fn cover_uv(size: [usize; 2], zoom: f32) -> Rect {
    let (w, h) = (size[0].max(1) as f32, size[1].max(1) as f32);
    let (uw, uh) = if w > h { (h / w, 1.0) } else { (1.0, w / h) };
    let (uw, uh) = (uw / zoom, uh / zoom);
    Rect::from_center_size(pos2(0.5, 0.5), vec2(uw, uh))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_crops_the_long_side() {
        let square = cover_uv([64, 64], 1.0);
        assert_eq!(square, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)));
        let wide = cover_uv([200, 100], 1.0);
        assert!((wide.width() - 0.5).abs() < 1e-6 && (wide.height() - 1.0).abs() < 1e-6);
        let zoomed = cover_uv([64, 64], 2.0);
        assert!((zoomed.width() - 0.5).abs() < 1e-6);
    }
}
