//! The bar's and the dialog's buttons: their CSS box, the look of each
//! state, and the painter they share.

use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim;
use crate::ui::backdrop_filter::Mark;
use crate::ui::components::controls::{focus_ring, take_focus, FocusCtx, BLACK};
use crate::ui::paint::{Border, BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextAlign, TextStyle};
use accshift_app::bulk_edit::TriState;
use egui::{pos2, vec2, Context, Id, Rect, Sense, Ui, Vec2};

use super::{BulkProps, T100, TRI_MIN_W};

/// One button's CSS.
pub(super) struct Btn {
    font: TextStyle,
    padding: Vec2,
    radius: f32,
    min_width: f32,
}

/// How a button paints in a state.
#[derive(Clone, Copy)]
pub(super) struct Look {
    background: Rgba,
    color: Rgba,
    border: Rgba,
}

impl Btn {
    /// `.tool-btn`: 11 px, `padding: 4px 10px`.
    pub(super) fn tool() -> Self {
        Btn {
            font: TextStyle::new(11.0, 400).line_height(LineHeight::Number(1.5)),
            padding: vec2(10.0, 4.0),
            radius: 6.0,
            min_width: 0.0,
        }
    }

    /// `.btn-secondary` and `.btn-primary`: 12 px, `padding: 5px 12px`.
    pub(super) fn action(weight: u16) -> Self {
        Btn {
            font: TextStyle::new(12.0, weight).line_height(LineHeight::Number(1.5)),
            padding: vec2(12.0, 5.0),
            radius: 6.0,
            min_width: 0.0,
        }
    }

    /// `.tri-state-btn`: 11 px, `padding: 4px 12px`, `min-width: 86px`.
    pub(super) fn tri() -> Self {
        Btn {
            font: TextStyle::new(11.0, 400).line_height(LineHeight::Number(1.5)),
            padding: vec2(12.0, 4.0),
            radius: 6.0,
            min_width: TRI_MIN_W,
        }
    }

    fn line(&self, ctx: &Context) -> f32 {
        self.font
            .line_box(ctx)
            .map_or(self.font.size * 1.5, |b| b.height)
    }

    pub(super) fn width(&self, ctx: &Context, label: &str, bw: f32) -> f32 {
        let ppp = ctx.pixels_per_point();
        let text = text::measure(ctx, label, &self.font).unwrap_or(0.0);
        (snap::content_width(text, ppp) + 2.0 * self.padding.x + 2.0 * bw).max(self.min_width)
    }

    pub(super) fn height(&self, ctx: &Context, bw: f32) -> f32 {
        self.line(ctx) + 2.0 * self.padding.y + 2.0 * bw
    }
}

/// What a button reported.
pub(super) struct Pressed {
    pub(super) clicked: bool,
}

/// Draws a button; `eased` is `transition: all 100ms`, which also eases the
/// disabled opacity.
#[allow(clippy::too_many_arguments)]
pub(super) fn button(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    b: &Btn,
    label: &str,
    enabled: bool,
    look: Look,
    eased: bool,
    props: &BulkProps,
) -> Pressed {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let bw = snap::border_width(v.border_px(), ppp);
    let boxed = snap::rect(rect, ppp);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui.interact(boxed, id, sense);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    let focused = enabled && take_focus(ui, &response);
    let opacity = if enabled { 1.0 } else { 0.5 };
    let (bg, fg, border, opacity) = if eased {
        (
            anim::transition(&ctx, id.with("bg"), look.background, &T100),
            anim::transition(&ctx, id.with("fg"), look.color, &T100),
            anim::transition(&ctx, id.with("border"), look.border, &T100),
            anim::transition(&ctx, id.with("opacity"), opacity, &T100),
        )
    } else {
        (look.background, look.color, look.border, opacity)
    };
    // `opacity` fades the button as one group: a layer where the renderer
    // has layers, each shape where it has not.
    let group = opacity < 1.0;
    if group {
        Mark::Begin {
            sigma: 0.0,
            opacity,
            saturate: 1.0,
            radius: Some(b.radius),
            ppp,
        }
        .paint(ui.painter(), boxed);
    }
    let mut painter = ui.painter().clone();
    painter.multiply_opacity(if group { 1.0 } else { opacity });
    BoxStyle {
        background: bg,
        radius: Corners::all(b.radius),
        border: Border::all(bw, border),
        ..BoxStyle::default()
    }
    .paint(&painter, boxed);
    let line = b.line(&ctx);
    let content_h = rect.height() - 2.0 * bw;
    let top = rect.top() + bw + snap::centre(content_h, line, ppp);
    let text_rect = Rect::from_min_max(
        pos2(rect.left() + bw + b.padding.x, top),
        pos2(rect.right() - bw - b.padding.x, top + line),
    );
    let font = b
        .font
        .clone()
        .color(fg.to_color32())
        .align(TextAlign::Center)
        .nowrap(false);
    text::paint(&painter, text_rect, label, &font);
    if group {
        Mark::End.paint(ui.painter(), boxed);
    }
    focus_ring(
        ui,
        boxed,
        b.radius,
        v.border_px(),
        v,
        FocusCtx {
            visible: props.focus_visible,
        },
        focused,
    );
    Pressed {
        clicked: enabled && response.clicked(),
    }
}

/// Whether the pointer is over `rect` (hover state without a widget).
pub(super) fn hovered(ui: &Ui, id: Id) -> bool {
    ui.ctx().read_response(id).is_some_and(|r| r.hovered())
}

/// `.tool-btn`: `--bg-card`, `--fg-muted`; hover (enabled) and `.active`
/// take `--bg-card-hover` and `--fg`. No transition.
pub(super) fn tool_look(v: &Vars, lit: bool) -> Look {
    Look {
        background: if lit { v.bg_card_hover } else { v.bg_card },
        color: if lit { v.fg } else { v.fg_muted },
        border: v.border,
    }
}

/// `.btn-secondary`: transparent, `--fg-muted`; hover `--bg-muted`, `--fg`.
pub(super) fn secondary_look(v: &Vars, hover: bool) -> Look {
    Look {
        background: if hover { v.bg_muted } else { Rgba::TRANSPARENT },
        color: if hover { v.fg } else { v.fg_muted },
        border: v.border,
    }
}

/// `.btn-primary`: `--accent` under `--accent-fg`, a transparent border;
/// hover (enabled) mixes 12 % black in.
pub(super) fn primary_look(v: &Vars, accent_fg: Rgba, hover: bool) -> Look {
    Look {
        background: if hover {
            v.accent.mix(0.88, BLACK)
        } else {
            v.accent
        },
        color: accent_fg,
        border: Rgba::TRANSPARENT,
    }
}

/// `.tri-state-btn`: `--bg-card`, `--fg-muted`; hover `--bg-card-hover`;
/// `.active` (changed) borders in `--accent` at 50 % and turns `--fg`.
pub(super) fn tri_look(v: &Vars, state: TriState, hover: bool) -> Look {
    let active = state.is_changed();
    Look {
        background: if hover { v.bg_card_hover } else { v.bg_card },
        color: if active { v.fg } else { v.fg_muted },
        border: if active {
            v.accent.mix(0.5, v.border)
        } else {
            v.border
        },
    }
}
