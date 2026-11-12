//! The card's paint layers: the eased values of a frame, the box with its
//! shadows, and the outlines drawn over the content.

use egui::{Response, Ui};

use accshift_app::platforms::warnings::WarningTone;

use super::input::Hover;
use super::{AccountCardProps, Card, State};
use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim::{self, Timing, Transition};
use crate::ui::components::folder_card::{ring, Pose, GHOST_SHADOW, HOVER_SHADOW};
use crate::ui::paint::{BoxShadow, BoxStyle, Corners};

/// Background, transform and shadow ease over 180 ms.
const T180: Transition = Transition::new(0.18, Timing::EASE_OUT);
/// The avatar box's background eases over 150 ms.
const T150: Transition = Transition::new(0.15, Timing::EASE);
/// The picture's hover zoom eases over 220 ms.
const T220: Transition = Transition::new(0.22, Timing::EASE_OUT);
/// The avatar's blur and dimming ease over 300 ms.
const T300: Transition = Transition::new(0.3, Timing::EASE_OUT);
/// A ban outline: red or yellow at 60 %.
const BAN_RED: Rgba = Rgba::new(239.0, 68.0, 68.0, 0.6);
const BAN_YELLOW: Rgba = Rgba::new(234.0, 179.0, 8.0, 0.6);

/// The values the card eases toward this frame, eased.
pub(super) struct Look {
    pub background: Rgba,
    pub pose: Pose,
    pub avatar_bg: Rgba,
    /// The avatar box's hover lift.
    pub avatar_lift: f32,
    /// The picture's hover zoom.
    pub zoom: f32,
    /// The picture's blur, 0 to 1.
    pub blur: f32,
    /// The initials' blur, 0 to 1.
    pub text_blur: f32,
}

impl Look {
    pub fn transition(card: &Card, hover: Hover, st: &mut State) -> Self {
        let ctx = &card.ctx;
        let id = card.id();
        let v = card.v();
        let item = card.item();
        let lift = hover.lift;
        let background = background(card.props, lift);
        let background = anim::transition(ctx, id.with("bg"), background, &T180);
        let pose = pose(card.props, hover).transition(ctx, id);
        let avatar_bg = anim::transition(
            ctx,
            id.with("avatar-bg"),
            if lift { v.bg_elevated } else { v.bg_muted },
            &T150,
        );
        let avatar_lift = anim::transition(
            ctx,
            id.with("avatar-ty"),
            if lift { -1.0f32 } else { 0.0 },
            &T180,
        );
        let zoom = anim::transition(
            ctx,
            id.with("img-scale"),
            if lift { 1.04f32 } else { 1.0 },
            &T220,
        );
        let blur_on = item.avatar.refreshing || st.confirm || item.switching;
        let blur = anim::transition_bool(ctx, id.with("blur"), blur_on, &T300);
        st.blurring = blur_on || blur > 0.0;
        // The initials blur while armed or switching, not while refreshing.
        let text_blur = anim::transition_bool(
            ctx,
            id.with("text-blur"),
            st.confirm || item.switching,
            &T300,
        );
        Self {
            background,
            pose,
            avatar_bg,
            avatar_lift,
            zoom,
            blur,
            text_blur,
        }
    }
}

/// The box's background: an active or lifted card takes the hover one; a
/// custom colour mixes into it only when lifted.
fn background(props: &AccountCardProps, lift: bool) -> Rgba {
    let v = props.vars;
    match (props.card_color, props.item.active, lift) {
        (Some(c), false, true) => c.mix(0.32, v.bg_card_hover),
        (Some(c), _, _) => c.mix(0.24, v.bg_card),
        (None, true, _) | (None, false, true) => v.bg_card_hover,
        (None, false, false) => v.bg_card,
    }
}

/// A dragged card shrinks, less while pressed. Otherwise a hovered card
/// stays in place while another one is dragged, and the hover lift beats a
/// press. An active card never shrinks under a press.
fn pose(props: &AccountCardProps, hover: Hover) -> Pose {
    let item = props.item;
    let pressing = hover.pressed && !item.active;
    if item.dragged {
        return if pressing {
            Pose::PRESSED
        } else {
            Pose::DRAGGED
        };
    }
    if props.grid_dragging && hover.hovered {
        return Pose::REST;
    }
    if hover.lift {
        return Pose::LIFTED;
    }
    if pressing {
        Pose::PRESSED
    } else {
        Pose::REST
    }
}

/// The active card's two rings: a halo of the foreground and a band of the
/// solid background, or of the custom colour over it.
fn active_rings(v: &Vars, card_color: Option<Rgba>) -> Vec<BoxShadow> {
    let transparent = Rgba::TRANSPARENT;
    let (halo, band, band_w) = match card_color {
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
}

/// The ghost's shadow, else the active rings (the surface's own while the
/// extension shows), else the hover shadow. Ring lists and the drop shadow
/// interpolate as lists, padded with transparent shadows.
fn shadows(props: &AccountCardProps, extension_visible: bool, lift: bool) -> Vec<BoxShadow> {
    let active = props.item.active;
    if props.ghost {
        vec![GHOST_SHADOW]
    } else if active && extension_visible {
        Vec::new()
    } else if active {
        active_rings(props.vars, props.card_color)
    } else if lift {
        vec![HOVER_SHADOW]
    } else {
        Vec::new()
    }
}

/// The card box: background, the theme's card image, shadows.
pub(super) fn paint_box(ui: &Ui, card: &Card, look: &Look, extension_visible: bool, lift: bool) {
    let v = card.v();
    BoxStyle {
        background: look.background,
        images: v.card_bg_image.clone(),
        radius: Corners::all(v.radius_md),
        shadows: shadows(card.props, extension_visible, lift),
        opacity: card.place.opacity,
        ..BoxStyle::default()
    }
    .paint_transformed(ui.painter(), card.rect(), card.transform());
}

/// The ban outline's colour: opaque on a custom colour.
fn ban_color(tone: WarningTone, custom: bool) -> Rgba {
    let base = match tone {
        WarningTone::Red => BAN_RED,
        WarningTone::Orange => BAN_YELLOW,
    };
    if custom {
        base.alpha(1.0)
    } else {
        base
    }
}

/// The outlines, after the content like any outline: the ban outline (the
/// surface's while the extension shows), else the focus ring of the arrow
/// keys or of a visible keyboard focus. The ban outline takes the focus
/// ring's 2 px offset when both apply.
pub(super) fn paint_outlines(ui: &Ui, card: &Card, response: &Response, extension_visible: bool) {
    let props = card.props;
    let v = card.v();
    let ring_on = props.kb_focus || (props.focus_visible && response.has_focus());
    let offset = if ring_on { 2.0 } else { 0.0 };
    let banned = props
        .item
        .warning
        .as_ref()
        .and_then(|w| w.card_outline_tone);
    if let Some(tone) = banned.filter(|_| !extension_visible) {
        let color = ban_color(tone, props.card_color.is_some());
        card.place.paint_outline(ui, offset, v.radius_md, color);
    }
    if ring_on && banned.is_none() {
        let color = v.fg.mix(0.65, Rgba::TRANSPARENT);
        card.place.paint_outline(ui, offset, v.radius_md, color);
    }
}
