//! The avatar: its box, the picture or the initials, the loaders, the play
//! overlay of an armed card and the active outline.

use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{pos2, vec2, Painter, Pos2, Rect, Shape, Vec2};

use accshift_app::actions::avatar_fallback::avatar_initials;

use super::look::Look;
use super::measure::line_box;
use super::{confirm, AvatarContent, AvatarPicture, Card, Loader, State};
use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim::{self, Animation, Keyframes, Timing};
use crate::ui::components::avatar_fade_in;
use crate::ui::components::folder_card::{map_rect, ring};
use crate::ui::components::shared;
use crate::ui::paint::{self, BoxStyle, Corners, Transform};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextAlign, TextStyle};
use crate::ui::text_shadow::{self, TextShadow};

/// The loader: 20 px across, one turn every 0.7 s.
const LOADER: f32 = 20.0;
const LOADER_SPIN: f64 = 0.7;

/// The avatar box and where it paints under the card's transform.
struct AvatarBox {
    /// Untransformed, in layout units.
    rect: Rect,
    /// The box's transform: the card's, plus the hover lift.
    transform: Transform,
    /// The box as it paints.
    mapped: Rect,
}

impl AvatarBox {
    fn new(card: &Card, lift: f32) -> Self {
        let layout = card.layout();
        let size = card.v().grid_card_avatar_size();
        let rect = Rect::from_min_size(
            pos2(
                card.content_left() + snap::centre(card.content_w(), size, card.ppp),
                layout.top() + card.pad(),
            ),
            vec2(size, size),
        );
        let transform = card.transform();
        let mapped = map_rect(layout, transform, rect.translate(vec2(0.0, lift)));
        Self {
            rect,
            transform: Transform {
                translate: mapped.center() - rect.center(),
                scale: transform.scale,
            },
            mapped,
        }
    }

    /// Where the picture paints, its transform and its radius. A lifted
    /// picture keeps the fractional offset its box paints at; under a
    /// scaled card it starts from whole device pixels.
    fn picture_frame(&self, card: &Card) -> (Rect, Transform, Corners) {
        let radius = card.v().avatar_radius;
        let scale = card.transform().scale;
        if scale == 1.0 {
            (self.rect, self.transform, Corners::all(radius))
        } else {
            (
                snap::rect(self.mapped, card.ppp),
                Transform::NONE,
                Corners::all(radius * scale),
            )
        }
    }
}

/// Paints the avatar layers, in order: the box, its content, the loader,
/// the play overlay (`shown` is the armed overlays' fade in) and the active
/// outline.
pub(super) fn paint(
    card: &Card,
    painter: &Painter,
    look: &Look,
    st: &mut State,
    armed: bool,
    shown: f32,
) {
    let v = card.v();
    let avatar = AvatarBox::new(card, look.avatar_lift);
    BoxStyle {
        background: look.avatar_bg,
        radius: Corners::all(v.avatar_radius),
        ..BoxStyle::default()
    }
    .paint_transformed(painter, avatar.rect, avatar.transform);
    paint_content(card, painter, &avatar, look);
    paint_loaders(card, painter, avatar.mapped.center(), st);
    if armed {
        confirm::paint_play(painter, avatar.mapped.center(), shown, v);
    }
    if card.item().active {
        BoxStyle {
            radius: Corners::all(v.avatar_radius),
            shadows: vec![ring(
                snap::border_width(2.0, card.ppp),
                v.fg.mix(0.2, Rgba::TRANSPARENT),
            )],
            ..BoxStyle::default()
        }
        .paint_transformed(painter, avatar.rect, avatar.transform);
    }
}

/// The picture, the initials or nothing, as [`AvatarContent`] says.
fn paint_content(card: &Card, painter: &Painter, avatar: &AvatarBox, look: &Look) {
    let ctx = &card.ctx;
    let id = card.id();
    let v = card.v();
    let mut deferred = false;
    match &card.props.avatar {
        AvatarContent::Picture(picture) => {
            let reveal = avatar_fade_in::avatar_opacity(ctx, id, true, v);
            if reveal > 0.0 {
                let frame = avatar.picture_frame(card);
                paint_picture(painter, picture, frame, look.zoom, reveal, look.blur);
            }
        }
        AvatarContent::Decoding => {
            avatar_fade_in::avatar_opacity(ctx, id, false, v);
        }
        AvatarContent::Initials {
            background,
            gradient,
        } => {
            // Held back while no avatar state is known, so a fast profile
            // never flashes a placeholder.
            deferred = card.item().avatar.pending;
            let fade = if deferred {
                shared::avatar_placeholder_in(ctx, id.with("fallback-in"), v)
            } else {
                1.0
            };
            let mut layer = painter.clone();
            layer.multiply_opacity(fade);
            BoxStyle {
                background: *background,
                images: gradient.iter().cloned().collect(),
                radius: Corners::all(v.avatar_radius),
                ..BoxStyle::default()
            }
            .paint_transformed(&layer, avatar.rect, avatar.transform);
            paint_initials(card, &layer, avatar.mapped, look.text_blur);
        }
        AvatarContent::Loading => {}
    }
    // Every frame without the hold resets the deferred fade in, so it runs
    // from the first held frame.
    if !deferred {
        anim::restart(ctx, id.with("fallback-in"));
    }
}

/// The initials, centred in the avatar box. Armed or switching, they blur
/// 4 px and dim to half, the filter interpolated as one: the glyph coverage
/// through a gaussian of sigma 4 px, as a text shadow of blur radius 8 px
/// in place of the text.
fn paint_initials(card: &Card, layer: &Painter, mapped: Rect, text_blur: f32) {
    let ctx = &card.ctx;
    let v = card.v();
    let size = v.grid_card_avatar_size();
    let dim = 1.0 - 0.5 * text_blur;
    let fg = v.fg;
    let color = Rgba::new(fg.r * dim, fg.g * dim, fg.b * dim, fg.a);
    let style = TextStyle::new(20.0, 600)
        .line_height(LineHeight::Number(1.5))
        .nowrap(false)
        .align(TextAlign::Center)
        .color(color.to_color32());
    let line = line_box(ctx, &style).height;
    let text_rect = Rect::from_min_size(
        pos2(
            mapped.left(),
            mapped.top() + snap::centre(size, line, card.ppp),
        ),
        vec2(mapped.width(), line),
    );
    let initials = avatar_initials(&card.item().name);
    if text_blur <= 0.0 {
        text::paint(layer, text_rect, &initials, &style);
        return;
    }
    if let Some(layout) = text::layout(ctx, &initials, &style, text_rect) {
        let shadow = TextShadow {
            offset: Vec2::ZERO,
            blur: 8.0 * text_blur,
            color,
        };
        text_shadow::paint(layer, &layout, &[shadow], None, 1.0);
    }
}

/// The loader over the avatar: deferred while the picture loads, at once
/// while refreshing or switching. A new loader restarts its fade and spin.
fn paint_loaders(card: &Card, painter: &Painter, centre: Pos2, st: &mut State) {
    let ctx = &card.ctx;
    let id = card.id();
    let v = card.v();
    let item = card.item();
    let loader = if item.avatar.loading {
        Loader::Deferred
    } else if item.avatar.refreshing || item.switching {
        Loader::Plain
    } else {
        Loader::None
    };
    if loader != st.loader {
        anim::restart(ctx, id.with("loader-in"));
        anim::restart(ctx, id.with("loader-spin"));
        st.loader = loader;
    }
    if loader == Loader::None {
        return;
    }
    let fade = if loader == Loader::Deferred {
        shared::avatar_placeholder_in(ctx, id.with("loader-in"), v)
    } else {
        1.0
    };
    let turn = anim::animate(
        ctx,
        id.with("loader-spin"),
        &Keyframes::from_to(0.0f32, 1.0),
        &Animation::new(LOADER_SPIN, Timing::LINEAR).infinite(),
        0.0,
    );
    paint_loader(painter, centre, turn, fade, v);
    ctx.request_repaint();
}

/// The picture: its fade in, its hover zoom and its filter, the blurred copy
/// cross-faded over the 300 ms transition.
fn paint_picture(
    painter: &Painter,
    picture: &AvatarPicture,
    (rect, transform, radius): (Rect, Transform, Corners),
    zoom: f32,
    reveal: f32,
    blur: f32,
) {
    let tint = shared::image_tint;
    match picture.blurred {
        Some(blurred) if blur > 0.0 => {
            if blur < 1.0 {
                let uv = cover_uv(picture.size, zoom);
                paint::paint_image_transformed(
                    painter,
                    picture.texture,
                    rect,
                    transform,
                    uv,
                    radius,
                    tint(reveal * (1.0 - blur), 1.0),
                );
            }
            paint::paint_image_transformed(
                painter,
                blurred,
                rect,
                transform,
                cover_uv([1, 1], zoom),
                radius,
                tint(reveal * blur, 1.0),
            );
        }
        _ => {
            paint::paint_image_transformed(
                painter,
                picture.texture,
                rect,
                transform,
                cover_uv(picture.size, zoom),
                radius,
                tint(reveal, 1.0 - 0.5 * blur),
            );
        }
    }
}

/// The avatar loader: a 20 px ring, 2 px of the elevated background with a
/// foreground top border, turned by `turn` (0 to 1).
fn paint_loader(painter: &Painter, centre: Pos2, turn: f32, fade: f32, v: &Vars) {
    if fade <= 0.0 {
        return;
    }
    let rest = v.bg_elevated.alpha(v.bg_elevated.a * fade);
    let top = v.fg.alpha(v.fg.a * fade);
    paint_ring_loader(painter, centre, LOADER, 2.0, turn, rest, top);
}

/// A spinner ring: `size` px across, border included, a `border` px border
/// in `rest` with a `top` coloured top border, turned by `turn` (0 to 1).
pub(crate) fn paint_ring_loader(
    painter: &Painter,
    centre: Pos2,
    size: f32,
    border: f32,
    turn: f32,
    rest: Rgba,
    top: Rgba,
) {
    let ppp = painter.ctx().pixels_per_point();
    let bw = snap::border_width(border, ppp);
    let outer = size / 2.0;
    let inner = outer - bw;
    let rest = rest.to_color32();
    let top = top.to_color32();
    let aa = 0.5 / ppp;
    let mut mesh = Mesh::default();
    let segments = 96;
    let rotation = turn * std::f32::consts::TAU;
    // Screen angles (y down): the top border spans -135 to -45 degrees.
    let q = std::f32::consts::FRAC_PI_4;
    for k in 0..segments {
        let a0 = k as f32 / segments as f32 * std::f32::consts::TAU;
        let a1 = (k + 1) as f32 / segments as f32 * std::f32::consts::TAU;
        let mid = (a0 + a1) / 2.0;
        // Unrotated angle relative to straight up.
        let rel = (mid + std::f32::consts::FRAC_PI_2).rem_euclid(std::f32::consts::TAU);
        let color = if !(q..=std::f32::consts::TAU - q).contains(&rel) {
            top
        } else {
            rest
        };
        let radii = [inner - aa, inner + aa, outer - aa, outer + aa];
        let alphas = [0.0, 1.0, 1.0, 0.0];
        let base = mesh.vertices.len() as u32;
        for a in [a0 + rotation, a1 + rotation] {
            let dir = vec2(a.cos(), a.sin());
            for (r, alpha) in radii.iter().zip(alphas) {
                mesh.vertices.push(Vertex {
                    pos: centre + dir * *r,
                    uv: WHITE_UV,
                    color: color.gamma_multiply(alpha),
                });
            }
        }
        for j in 0..3u32 {
            let (a, b) = (base + j, base + j + 1);
            let (c, d) = (base + 4 + j, base + 5 + j);
            mesh.add_triangle(a, b, d);
            mesh.add_triangle(a, d, c);
        }
    }
    painter.add(Shape::mesh(mesh));
}

/// The UV rect that covers a square box with a `size` picture (the long
/// side cropped), then zoomed by `zoom` about the centre (the box clips
/// it).
pub(crate) fn cover_uv(size: [usize; 2], zoom: f32) -> Rect {
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
