//! `src/lib/features/folders/FolderCard.svelte`: a folder in the grid.
//!
//! The card box is shared with `BackCard.svelte` (same CSS, a `<button>`
//! instead of a `div role="button"`): [`frame`] paints it and reports the
//! state its children need.
//!
//! - `.card`: `--grid-card-width` by `--grid-card-min-height`, padding
//!   `--grid-card-padding`, radius `--grid-card-radius`, transparent, a
//!   transparent 1 px outline and an inset 1 px transparent shadow;
//!   `card-entrance` on mount.
//! - `:hover`: `--bg-card-hover`, outline `color-mix(--fg-subtle 45%,
//!   transparent)`, `translateY(-2px)`, `0 12px 24px rgba(0,0,0,.18)`.
//!   Background, transform and shadow transition 180 ms ease-out, the
//!   outline colour 120 ms. The shadow switches between an inset and an
//!   outer one, which CSS cannot interpolate, so it changes at once.
//! - `:active`: `translateY(0) scale(.985)`.
//! - `.icon-wrap`: 68 px, radius 6, `color-mix(--bg-muted 52%, transparent)`,
//!   `--fg-muted`; on card hover `--bg-elevated`, `--fg`, `translateY(-1px)`.
//! - `.name`: 12 px 500 `--fg`, nowrap with an ellipsis, the content width.

use egui::accesskit::Role;
use egui::{pos2, vec2, Id, Pos2, Rect, Response, Sense, Ui};

use super::icon;
use crate::app::vars::Vars;
use crate::icons;
use crate::theme::Rgba;
use crate::ui::anim::{self, Animation, Keyframes, Timing, Transition};
use crate::ui::paint::{BoxShadow, BoxStyle, Corners, Transform};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextAlign, TextStyle};

/// `transition: background 180ms ease-out, transform 180ms ease-out`.
const T180: Transition = Transition::new(0.18, Timing::EASE_OUT);
/// `outline-color 120ms ease-out`.
const T120: Transition = Transition::new(0.12, Timing::EASE_OUT);
/// `.icon-wrap { transition: background 150ms }` (CSS default `ease`).
const T150_EASE: Transition = Transition::new(0.15, Timing::EASE);
/// `.icon-wrap { transition: color 150ms ease-out }`.
const T150: Transition = Transition::new(0.15, Timing::EASE_OUT);

/// What the folder card reports.
#[derive(Clone, Debug, PartialEq)]
pub enum FolderCardEvent {
    /// Click, Enter or Space: `onOpen`.
    Open,
    /// Right click at this point: `onContextMenu`.
    ContextMenu(Pos2),
}

/// `FolderCard.svelte`'s props, minus the callbacks.
#[derive(Clone, Debug)]
pub struct FolderCardProps<'a> {
    pub vars: &'a Vars,
    /// Stable for one mount of the card: a new id replays `card-entrance`.
    pub id: Id,
    pub name: &'a str,
    /// `cardColor` resolved, `None` for `""`.
    pub card_color: Option<Rgba>,
    /// `accentColor`, for the drag-over state.
    pub accent: Rgba,
    pub drag_over: bool,
    pub dragged: bool,
    /// Keyboard focus is showing (`:focus-visible`).
    pub focus_visible: bool,
    /// `[data-kb-focus]`: the card the arrow keys moved to.
    pub kb_focus: bool,
}

/// Which card [`frame`] paints and its state classes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CardState {
    /// `.custom-color` (folder cards only).
    pub custom: Option<Rgba>,
    /// `--drag-accent`.
    pub accent: Rgba,
    /// `.drag-over`.
    pub drag_over: bool,
    /// `.dragging` (folder cards only).
    pub dragged: bool,
    /// `.card[data-kb-focus]` (folder cards only): the arrow keys moved
    /// here.
    pub kb_focus: bool,
}

/// The card box drawn, for its children.
pub(crate) struct Frame {
    pub response: Response,
    /// The untransformed border box, snapped.
    pub rect: Rect,
    /// The same box in layout units: the content lays out from it.
    pub layout: Rect,
    pub transform: Transform,
    /// `.icon-wrap` hover progress (its own transitions applied).
    pub icon_bg: Rgba,
    pub icon_fg: Rgba,
    pub icon_lift: f32,
    pub opacity: f32,
}

/// `card-entrance`: `(opacity, scale)` at this frame, `(1, 1)` once over.
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

/// Paints the `.card` box of a folder or back card over `rect` and senses
/// it; `label` goes to AccessKit with the button role.
pub(crate) fn frame(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    label: &str,
    state: CardState,
    vars: &Vars,
) -> Frame {
    let CardState {
        custom,
        accent,
        drag_over,
        dragged,
        kb_focus,
    } = state;
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let layout = rect;
    let rect = snap::rect(rect, ppp);
    let response = ui.interact(rect, id, Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    ctx.accesskit_node_builder(id, |node| node.set_role(Role::Button));
    let hovered = response.hovered();
    let pressed = response.is_pointer_button_down_on();

    let transparent = Rgba::TRANSPARENT;
    // Later rules win at equal specificity: `.custom-color:hover` (0,3,0)
    // beats `.drag-over` (0,2,0), which beats `:hover` and `.custom-color`.
    let background = match (custom, hovered, drag_over) {
        (Some(c), true, _) => c.mix(0.32, vars.bg_card_hover),
        (_, _, true) => accent.mix(0.10, transparent),
        (Some(c), false, false) => c.mix(0.24, transparent),
        (None, true, false) => vars.bg_card_hover,
        (None, false, false) => transparent,
    };
    // `.card[data-kb-focus]` comes last and wins over both.
    let outline = if kb_focus {
        vars.fg.mix(0.65, transparent)
    } else if drag_over {
        accent.mix(0.55, transparent)
    } else if hovered {
        vars.fg_subtle.mix(0.45, transparent)
    } else {
        transparent
    };
    // `.dragging` (folder only) comes last and wins over `:active`.
    let (ty, scale) = if dragged {
        (0.0, 0.95)
    } else if pressed {
        (0.0, 0.985)
    } else if hovered {
        (-2.0, 1.0)
    } else {
        (0.0, 1.0)
    };
    let background = anim::transition(&ctx, id.with("bg"), background, &T180);
    let outline = anim::transition(&ctx, id.with("outline"), outline, &T120);
    let ty = anim::transition(&ctx, id.with("ty"), ty, &T180);
    let scale = anim::transition(&ctx, id.with("scale"), scale, &T180);
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

    // `animation` overrides `transform` while it runs.
    let (enter_opacity, enter_scale) = entrance(ui, id, vars);
    let entering = enter_opacity < 1.0;
    let transform = if entering {
        Transform::scale(enter_scale)
    } else {
        Transform {
            translate: vec2(0.0, ty),
            scale,
        }
    };
    let opacity = enter_opacity * if dragged { 0.4 } else { 1.0 };

    let mut shadows = Vec::new();
    // The outline is drawn outside the border box, above the shadows; its
    // width snaps like a border. Only its colour eases: the arrow-key
    // focus widens it and moves it out at once.
    if !kb_focus {
        shadows.push(BoxShadow {
            offset: egui::Vec2::ZERO,
            blur: 0.0,
            spread: snap::border_width(1.0, ppp),
            color: outline,
            inset: false,
        });
    }
    if drag_over {
        shadows.push(BoxShadow {
            offset: egui::Vec2::ZERO,
            blur: 0.0,
            spread: 1.0,
            color: accent,
            inset: true,
        });
    } else if hovered {
        shadows.push(BoxShadow {
            offset: vec2(0.0, 12.0),
            blur: 24.0,
            spread: 0.0,
            color: Rgba::new(0.0, 0.0, 0.0, 0.18),
            inset: false,
        });
    }
    BoxStyle {
        background,
        radius: Corners::all(vars.radius_md),
        shadows,
        opacity,
        ..BoxStyle::default()
    }
    .paint_transformed(ui.painter(), rect, transform);
    if kb_focus {
        // `outline: 2px; outline-offset: 2px`, its radius grown by the
        // offset.
        BoxStyle {
            radius: Corners::all(vars.radius_md + 2.0),
            shadows: vec![BoxShadow {
                offset: egui::Vec2::ZERO,
                blur: 0.0,
                spread: snap::border_width(2.0, ppp),
                color: outline,
                inset: false,
            }],
            opacity,
            ..BoxStyle::default()
        }
        .paint_transformed(ui.painter(), rect.expand(2.0), transform);
    }

    Frame {
        response,
        rect,
        layout,
        transform,
        icon_bg,
        icon_fg,
        icon_lift,
        opacity,
    }
}

/// The `.icon-wrap` box and the icon centred in it, under the card's
/// transform. `svg_size` is the inline `<svg>` size.
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

/// The card's `.name` line under the icon: `rect` is the name box
/// (untransformed), the text follows the card's translation.
pub(crate) fn name(ui: &Ui, f: &Frame, rect: Rect, label: &str, style: &TextStyle) {
    let mut painter = ui.painter().clone();
    painter.multiply_opacity(f.opacity);
    let mapped = map_rect(f.layout, f.transform, rect);
    let moved = Rect::from_center_size(mapped.center(), rect.size());
    painter.set_clip_rect(painter.clip_rect().intersect(mapped.expand(1.0)));
    text::paint(&painter, moved, label, style);
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

/// `.name` of the folder card.
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
        },
        v,
    );
    icon_wrap(ui, &f, v, icons::FOLDER, 30.0);
    let style = name_style(v);
    name(ui, &f, name_rect(f.layout, v, 18.0), props.name, &style);
    if props.focus_visible && f.response.has_focus() && !props.kb_focus {
        super::focus::card_ring(ui, f.rect, v.radius_md, v);
    }
    let mut events = Vec::new();
    if f.response.clicked() {
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
