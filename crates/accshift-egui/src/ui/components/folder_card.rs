//! A folder in the grid, and the card box it shares with the back card and
//! the account card.
//!
//! The back card draws the same box (as a button): [`frame`] paints it and
//! reports the state its children need. The account card shares the pose
//! ([`Pose`]), where the box sits this frame ([`Placement`]), the button
//! sensing, the shadows and the outline painter.
//!
//! - The box: the grid card width by its min height, the grid card
//!   padding, a medium radius, transparent, a transparent 1 px outline; it
//!   fades and scales in on mount.
//! - Hover: the hover background, the outline in the subtle foreground at
//!   45 %, 2 px up, a `0 12px 24px` shadow at 18 % black. Background,
//!   transform and shadow ease over 180 ms, the outline colour over 120 ms.
//!   The shadow switches between an inset and an outer one, which cannot be
//!   interpolated, so it changes at once.
//! - Press: back in place, scaled to 0.985.
//! - The icon wrap: the avatar size, radius 6, the muted background at
//!   52 %, the muted foreground; on hover the elevated background, the
//!   foreground and 1 px up.
//! - The name: 12 px 500, one line with an ellipsis, the content width.

use egui::accesskit::Role;
use egui::{pos2, vec2, Id, Pos2, Rect, Response, Sense, Ui, WidgetInfo};

use super::icon;
use crate::app::vars::Vars;
use crate::icons;
use crate::theme::Rgba;
use crate::ui::anim::{self, Animation, Keyframes, Timing, Transition};
use crate::ui::paint::{BoxShadow, BoxStyle, Corners, Transform};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextAlign, TextStyle};

/// Background, transform and shadow ease over 180 ms.
const T180: Transition = Transition::new(0.18, Timing::EASE_OUT);
/// The outline colour eases over 120 ms.
const T120: Transition = Transition::new(0.12, Timing::EASE_OUT);
/// The icon wrap's background eases over 150 ms.
const T150_EASE: Transition = Transition::new(0.15, Timing::EASE);
/// The icon's colour eases over 150 ms.
const T150: Transition = Transition::new(0.15, Timing::EASE_OUT);

/// The shadow of a hovered card: `0 12px 24px`, 18 % black.
pub(crate) const HOVER_SHADOW: BoxShadow = BoxShadow {
    offset: vec2(0.0, 12.0),
    blur: 24.0,
    spread: 0.0,
    color: Rgba::new(0.0, 0.0, 0.0, 0.18),
    inset: false,
};

/// The drag ghost's shadow, in place of the card's own: `0 8px 24px`, 50 %
/// black.
pub(crate) const GHOST_SHADOW: BoxShadow = BoxShadow {
    offset: vec2(0.0, 8.0),
    blur: 24.0,
    spread: 0.0,
    color: Rgba::new(0.0, 0.0, 0.0, 0.5),
    inset: false,
};

/// A solid ring `spread` px wide around a box.
pub(crate) fn ring(spread: f32, color: Rgba) -> BoxShadow {
    BoxShadow {
        offset: egui::Vec2::ZERO,
        blur: 0.0,
        spread,
        color,
        inset: false,
    }
}

/// What the folder card reports.
#[derive(Clone, Debug, PartialEq)]
pub enum FolderCardEvent {
    /// Click, Enter or Space: open the folder.
    Open,
    /// Right click at this point: open the context menu.
    ContextMenu(Pos2),
}

/// What the workspace hands a folder card.
#[derive(Clone, Debug)]
pub struct FolderCardProps<'a> {
    pub vars: &'a Vars,
    /// Stable for one mount of the card: a new id replays the entrance.
    pub id: Id,
    pub name: &'a str,
    /// The folder's colour, `None` when it has none.
    pub card_color: Option<Rgba>,
    /// The accent colour, for the drag-over state.
    pub accent: Rgba,
    pub drag_over: bool,
    pub dragged: bool,
    /// Keyboard focus is showing.
    pub focus_visible: bool,
    /// The card the arrow keys moved to.
    pub kb_focus: bool,
    /// A drag runs on the grid: a hovered card stays in place.
    pub grid_dragging: bool,
    /// The drag ghost: its own shadow replaces the card's.
    pub ghost: bool,
}

/// Which card [`frame`] paints and its states.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CardState {
    /// The card's own colour (folder cards only).
    pub custom: Option<Rgba>,
    /// The accent colour of the drag-over state.
    pub accent: Rgba,
    /// A dragged item hovers this card.
    pub drag_over: bool,
    /// The card itself is dragged (folder cards only).
    pub dragged: bool,
    /// The arrow keys moved here (folder cards only).
    pub kb_focus: bool,
    /// A drag runs on the grid.
    pub grid_dragging: bool,
    /// The drag ghost.
    pub ghost: bool,
}

impl CardState {
    /// The box's background. A hovered custom colour beats the drag-over
    /// tint, which beats the plain hover and the resting custom colour.
    fn background(&self, hovered: bool, vars: &Vars) -> Rgba {
        let transparent = Rgba::TRANSPARENT;
        match (self.custom, hovered, self.drag_over) {
            (Some(c), true, _) => c.mix(0.32, vars.bg_card_hover),
            (_, _, true) => self.accent.mix(0.10, transparent),
            (Some(c), false, false) => c.mix(0.24, transparent),
            (None, true, false) => vars.bg_card_hover,
            (None, false, false) => transparent,
        }
    }

    /// The outline's colour: the arrow-key focus first, then the drag-over
    /// accent, then the hover.
    fn outline(&self, hovered: bool, vars: &Vars) -> Rgba {
        let transparent = Rgba::TRANSPARENT;
        if self.kb_focus {
            vars.fg.mix(0.65, transparent)
        } else if self.drag_over {
            self.accent.mix(0.55, transparent)
        } else if hovered {
            vars.fg_subtle.mix(0.45, transparent)
        } else {
            transparent
        }
    }

    /// Dragged beats pressed, which beats the hover lift; a hovered card
    /// stays in place while another one is dragged.
    fn pose(&self, hovered: bool, pressed: bool) -> Pose {
        if self.dragged {
            return Pose::DRAGGED;
        }
        if self.grid_dragging && hovered {
            return Pose::REST;
        }
        if pressed {
            Pose::PRESSED
        } else if hovered {
            Pose::LIFTED
        } else {
            Pose::REST
        }
    }

    /// The box's shadows: the 1 px outline (drawn outside the border box,
    /// its width snapped like a border), then the drag-over inset or the
    /// hover shadow. The arrow-key focus draws its own wider outline.
    fn shadows(&self, hovered: bool, outline: Rgba, ppp: f32) -> Vec<BoxShadow> {
        if self.ghost {
            return vec![GHOST_SHADOW];
        }
        let mut shadows = Vec::new();
        if !self.kb_focus {
            shadows.push(ring(snap::border_width(1.0, ppp), outline));
        }
        if self.drag_over {
            shadows.push(BoxShadow {
                offset: egui::Vec2::ZERO,
                blur: 0.0,
                spread: 1.0,
                color: self.accent,
                inset: true,
            });
        } else if hovered {
            shadows.push(HOVER_SHADOW);
        }
        shadows
    }
}

/// A card's vertical offset and scale, before the entrance overrides them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Pose {
    pub ty: f32,
    pub scale: f32,
}

impl Pose {
    pub const REST: Pose = Pose::new(0.0, 1.0);
    /// Hovered: 2 px up.
    pub const LIFTED: Pose = Pose::new(-2.0, 1.0);
    pub const PRESSED: Pose = Pose::new(0.0, 0.985);
    pub const DRAGGED: Pose = Pose::new(0.0, 0.95);

    const fn new(ty: f32, scale: f32) -> Self {
        Self { ty, scale }
    }

    /// The pose eased toward `self` over 180 ms.
    pub fn transition(self, ctx: &egui::Context, id: Id) -> Pose {
        Pose {
            ty: anim::transition(ctx, id.with("ty"), self.ty, &T180),
            scale: anim::transition(ctx, id.with("scale"), self.scale, &T180),
        }
    }

    /// Where the card box over `layout` sits this frame. The entrance
    /// replaces the pose while it runs. A dragged card opens its fading
    /// layer here; the caller closes it with [`Placement::end_group`].
    pub fn place(self, ui: &Ui, id: Id, vars: &Vars, layout: Rect, dragged: bool) -> Placement {
        let ctx = ui.ctx();
        let (opacity, enter_scale) = entrance(ui, id, vars);
        let entering = opacity < 1.0;
        let transform = if entering {
            Transform::scale(enter_scale)
        } else {
            Transform {
                translate: vec2(0.0, self.ty),
                scale: self.scale,
            }
        };
        // A scaled box sits on whole device pixels and lays its content out
        // from there.
        let layout = if transform.scale != 1.0 {
            layout.translate(super::snap_offset(ctx, layout.min))
        } else {
            layout
        };
        let rect = snap::rect(layout, ctx.pixels_per_point());
        // A dragged card fades to 0.4 as one layer. While its entrance
        // replays (the preview moved it), the entrance's opacity rules
        // instead, from 0 to 1, then the layer drops it back to 0.4.
        let group = dragged && !entering;
        if group {
            drag_group(ui, rect);
        }
        Placement {
            layout,
            rect,
            transform,
            opacity,
            group,
        }
    }
}

/// Where a card box sits this frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Placement {
    /// The box in layout units: the content lays out from it.
    pub layout: Rect,
    /// The untransformed border box, snapped to device pixels.
    pub rect: Rect,
    pub transform: Transform,
    /// The entrance's opacity.
    pub opacity: f32,
    /// The dragged card's fading layer is open.
    pub group: bool,
}

impl Placement {
    /// The box before any pose: in place, opaque.
    pub fn rest(layout: Rect, ppp: f32) -> Self {
        Self {
            layout,
            rect: snap::rect(layout, ppp),
            transform: Transform::NONE,
            opacity: 1.0,
            group: false,
        }
    }

    /// A 2 px outline `offset` px outside the box, its radius grown by the
    /// offset, under the card's transform.
    pub fn paint_outline(&self, ui: &Ui, offset: f32, radius: f32, color: Rgba) {
        let ppp = ui.ctx().pixels_per_point();
        BoxStyle {
            radius: Corners::all(radius + offset),
            shadows: vec![ring(snap::border_width(2.0, ppp), color)],
            opacity: self.opacity,
            ..BoxStyle::default()
        }
        .paint_transformed(ui.painter(), self.rect.expand(offset), self.transform);
    }

    /// Closes the dragged card's layer opened by [`Pose::place`].
    pub fn end_group(&self, ui: &Ui) {
        if self.group {
            crate::ui::backdrop_filter::Mark::End.paint(ui.painter(), self.rect);
        }
    }
}

/// Senses a card box as a button: a click target under the pointing hand,
/// announced to AccessKit with `info`.
pub(crate) fn sense_button(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    info: impl Fn() -> WidgetInfo,
) -> Response {
    let response = ui
        .interact(rect, id, Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(info);
    ui.ctx()
        .accesskit_node_builder(id, |node| node.set_role(Role::Button));
    response
}

/// The card box drawn, for its children.
pub(crate) struct Frame {
    pub response: Response,
    /// The untransformed border box, snapped.
    pub rect: Rect,
    /// The same box in layout units: the content lays out from it.
    pub layout: Rect,
    pub transform: Transform,
    /// The icon wrap's hover progress (its own transitions applied).
    pub icon_bg: Rgba,
    pub icon_fg: Rgba,
    pub icon_lift: f32,
    pub opacity: f32,
    /// A dragged card fades as one layer, closed by the caller.
    pub group: bool,
}

/// The entrance animation: `(opacity, scale)` at this frame, `(1, 1)` once
/// over.
pub(crate) fn entrance(ui: &Ui, id: Id, vars: &Vars) -> (f32, f32) {
    let p = anim::animate(
        ui.ctx(),
        id.with("card-entrance"),
        &Keyframes::from_to(0.0f32, 1.0),
        &Animation::new(vars.motion_card_entrance, Timing::EASE_OUT),
        1.0,
    );
    (p, 0.98 + 0.02 * p)
}

/// Where a child box `child` (untransformed, points) paints under the card's
/// `transform` (origin at the card centre).
pub(crate) fn map_rect(card: Rect, transform: Transform, child: Rect) -> Rect {
    let c = card.center();
    let centre = c + (child.center() - c) * transform.scale + transform.translate;
    Rect::from_center_size(centre, child.size() * transform.scale)
}

/// Paints the box of a folder or back card over `rect` and senses it;
/// `label` goes to AccessKit with the button role.
pub(crate) fn frame(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    label: &str,
    state: CardState,
    vars: &Vars,
) -> Frame {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let response = sense_button(ui, snap::rect(rect, ppp), id, || {
        WidgetInfo::labeled(egui::Role::Button, true, label)
    });
    let hovered = super::css_hovered(&response);
    let pressed = super::held(&response);

    let transparent = Rgba::TRANSPARENT;
    let background = state.background(hovered, vars);
    let background = anim::transition(&ctx, id.with("bg"), background, &T180);
    let outline = state.outline(hovered, vars);
    let outline = anim::transition(&ctx, id.with("outline"), outline, &T120);
    let pose = state.pose(hovered, pressed).transition(&ctx, id);
    let icon_bg = anim::transition(
        &ctx,
        id.with("icon-bg"),
        if hovered {
            vars.bg_elevated
        } else {
            vars.bg_muted.mix(0.52, transparent)
        },
        &T150_EASE,
    );
    let icon_fg = anim::transition(
        &ctx,
        id.with("icon-fg"),
        if hovered { vars.fg } else { vars.fg_muted },
        &T150,
    );
    let icon_lift = anim::transition(
        &ctx,
        id.with("icon-ty"),
        if hovered { -1.0f32 } else { 0.0 },
        &T180,
    );

    let place = pose.place(ui, id, vars, rect, state.dragged);
    BoxStyle {
        background,
        radius: Corners::all(vars.radius_md),
        shadows: state.shadows(hovered, outline, ppp),
        opacity: place.opacity,
        ..BoxStyle::default()
    }
    .paint_transformed(ui.painter(), place.rect, place.transform);
    if state.kb_focus {
        // Only the outline's colour eases: the arrow-key focus widens it
        // and moves it 2 px out at once.
        place.paint_outline(ui, 2.0, vars.radius_md, outline);
    }

    Frame {
        response,
        rect: place.rect,
        layout: place.layout,
        transform: place.transform,
        icon_bg,
        icon_fg,
        icon_lift,
        opacity: place.opacity,
        group: place.group,
    }
}

/// Opens the layer of a dragged card, landing at 0.4 on
/// [`Mark::End`](crate::ui::backdrop_filter::Mark::End).
pub(crate) fn drag_group(ui: &Ui, rect: Rect) {
    crate::ui::backdrop_filter::Mark::Group { opacity: 0.4 }.paint(ui.painter(), rect);
}

/// The icon wrap and the icon centred in it, under the card's transform.
/// `svg_size` is the icon's own size.
pub(crate) fn icon_wrap(ui: &Ui, f: &Frame, vars: &Vars, svg: &'static [u8], svg_size: f32) {
    let ppp = ui.ctx().pixels_per_point();
    let pad = vars.grid_card_padding();
    let size = vars.grid_card_avatar_size();
    let content_w = f.layout.width() - 2.0 * pad;
    let wrap = Rect::from_min_size(
        pos2(
            f.layout.left() + pad + snap::centre(content_w, size, ppp),
            f.layout.top() + pad,
        ),
        vec2(size, size),
    );
    let mut painter = ui.painter().clone();
    painter.multiply_opacity(f.opacity);
    let lifted = wrap.translate(vec2(0.0, f.icon_lift));
    BoxStyle {
        background: f.icon_bg,
        radius: Corners::all(6.0),
        ..BoxStyle::default()
    }
    .paint_transformed(
        &painter,
        wrap,
        Transform {
            translate: (map_rect(f.layout, f.transform, lifted).center() - wrap.center()),
            scale: f.transform.scale,
        },
    );
    let svg_rect = Rect::from_min_size(
        wrap.min
            + vec2(
                snap::centre(size, svg_size, ppp),
                snap::centre(size, svg_size, ppp),
            ),
        vec2(svg_size, svg_size),
    );
    // The SVG root snaps in the untransformed box; the card's and the
    // wrap's transforms then move it off the grid, as Skia draws it.
    let local = Rect::from_min_size(icon::snapped_origin(svg_rect.min, ppp), svg_rect.size());
    let mapped = map_rect(
        f.layout,
        f.transform,
        local.translate(vec2(0.0, f.icon_lift)),
    );
    icon::paint_shifted(
        &painter,
        svg,
        local,
        f.icon_fg,
        f.transform.scale,
        mapped.center() - local.center(),
    );
}

/// The card's name line under the icon: `rect` is the name box
/// (untransformed), the text follows the card's translation.
pub(crate) fn name(ui: &Ui, f: &Frame, rect: Rect, label: &str, style: &TextStyle) {
    let mut painter = ui.painter().clone();
    painter.multiply_opacity(f.opacity);
    let base = painter.clip_rect();
    let s = f.transform.scale;
    if s == 1.0 {
        let mapped = map_rect(f.layout, f.transform, rect);
        painter.set_clip_rect(base.intersect(mapped.expand(1.0)));
        let moved = Rect::from_center_size(mapped.center(), rect.size());
        text::paint(&painter, moved, label, style);
        return;
    }
    painter.set_clip_rect(base.intersect(rect.expand(1.0)));
    // Under a scale the text is laid out at its transformed size.
    let scope = super::TransformScope::begin(
        ui.ctx(),
        painter.layer_id(),
        egui::emath::TSTransform::new(
            f.layout.center().to_vec2() * (1.0 - s) + f.transform.translate,
            s,
        ),
    );
    text::paint(&painter, rect, label, style);
    scope.end(ui.ctx(), Some(base));
}

/// The name box of a card: under the icon wrap and its 8 px margin.
pub(crate) fn name_rect(card: Rect, vars: &Vars, line: f32) -> Rect {
    let pad = vars.grid_card_padding();
    Rect::from_min_size(
        pos2(
            card.left() + pad,
            card.top() + pad + vars.grid_card_avatar_size() + 8.0,
        ),
        vec2(card.width() - 2.0 * pad, line),
    )
}

/// The folder card's name: 12 px 500, one line with an ellipsis.
fn name_style(vars: &Vars) -> TextStyle {
    TextStyle::new(12.0, 500)
        .line_height(LineHeight::Number(1.5))
        .nowrap(true)
        .align(TextAlign::Center)
        .color(vars.fg.to_color32())
}

/// Draws a folder card over `rect` (its layout box) and returns what the
/// user did with it.
pub fn folder_card(ui: &mut Ui, rect: Rect, props: &FolderCardProps) -> Vec<FolderCardEvent> {
    let v = props.vars;
    let f = frame(
        ui,
        rect,
        props.id,
        props.name,
        CardState {
            custom: props.card_color,
            accent: props.accent,
            drag_over: props.drag_over,
            dragged: props.dragged,
            kb_focus: props.kb_focus,
            grid_dragging: props.grid_dragging,
            ghost: props.ghost,
        },
        v,
    );
    icon_wrap(ui, &f, v, icons::FOLDER, 30.0);
    let style = name_style(v);
    name(ui, &f, name_rect(f.layout, v, 18.0), props.name, &style);
    if props.focus_visible && f.response.has_focus() && !props.kb_focus {
        super::focus::card_ring(ui, f.rect, v.radius_md, v);
    }
    if f.group {
        crate::ui::backdrop_filter::Mark::End.paint(ui.painter(), f.rect);
    }
    let mut events = Vec::new();
    if super::clicked(&f.response) {
        events.push(FolderCardEvent::Open);
    }
    if f.response.secondary_clicked() {
        let at = super::context_click_pos(ui).unwrap_or(f.rect.center());
        events.push(FolderCardEvent::ContextMenu(at));
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn children_follow_the_card_transform() {
        let card = Rect::from_min_size(pos2(45.0, 84.0), vec2(100.0, 136.0));
        let icon = Rect::from_min_size(pos2(61.0, 92.0), vec2(68.0, 68.0));
        let lifted = map_rect(card, Transform::translate(vec2(0.0, -2.0)), icon);
        assert_eq!(lifted, icon.translate(vec2(0.0, -2.0)));
        let scaled = map_rect(card, Transform::scale(0.5), card);
        assert_eq!(scaled.center(), card.center());
        assert_eq!(scaled.size(), card.size() * 0.5);
    }
}
