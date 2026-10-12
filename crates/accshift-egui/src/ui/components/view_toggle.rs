//! `src/lib/shared/components/ViewToggle.svelte`: grid or list.
//!
//! `.view-toggle`: 30 px high, `--bg-muted`, radius 8, padding 2, gap 2.
//! `.btn`: `padding: 0 8px` around a 14 px icon (30 px wide), full height
//! (26), radius 6, `--fg-muted`; `:hover` `--fg`; `.active` `--bg-card` and
//! `--fg`. `transition: all 100ms` (CSS default `ease`).

use egui::accesskit::Role;
use egui::{pos2, vec2, Id, Rect, Sense, Ui, WidgetInfo, WidgetType};

use accshift_app::i18n::{translate, Locale};
use accshift_app::stores::ViewMode;

use super::icon;
use crate::app::vars::Vars;
use crate::icons;
use crate::theme::Rgba;
use crate::ui::anim::{self, Timing, Transition};
use crate::ui::paint::{BoxStyle, Corners};
use crate::ui::snap;

/// `transition: all 100ms`.
const T100: Transition = Transition::new(0.1, Timing::EASE);
/// `.view-toggle { padding: 2px; gap: 2px }`.
const PAD: f32 = 2.0;
/// `.btn { padding: 0 8px }` around the 14 px icon.
const BUTTON_W: f32 = 8.0 + 14.0 + 8.0;
/// `.view-toggle { height: 30px }`.
pub const HEIGHT: f32 = 30.0;
/// Outer width: two buttons, the gap and the padding.
pub const WIDTH: f32 = PAD + BUTTON_W + PAD + BUTTON_W + PAD;

#[derive(Clone, Debug)]
pub struct ViewToggleProps<'a> {
    pub vars: &'a Vars,
    pub mode: ViewMode,
    pub locale: Locale,
    pub focus_visible: bool,
}

/// Draws the toggle with its border box at `rect` and returns the mode
/// clicked this frame (`onChange`), if any.
pub fn view_toggle(ui: &mut Ui, rect: Rect, props: &ViewToggleProps) -> Option<ViewMode> {
    let v = props.vars;
    let ppp = ui.ctx().pixels_per_point();
    BoxStyle {
        background: v.bg_muted,
        radius: Corners::all(8.0),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), rect);
    let inner_h = rect.height() - 2.0 * PAD;
    let mut changed = None;
    let buttons = [
        (ViewMode::Grid, icons::GRID, "view.grid"),
        (ViewMode::List, icons::LIST, "view.list"),
    ];
    for (index, (mode, svg, key)) in buttons.into_iter().enumerate() {
        let r = Rect::from_min_size(
            pos2(
                rect.left() + PAD + index as f32 * (BUTTON_W + PAD),
                rect.top() + PAD,
            ),
            vec2(BUTTON_W, inner_h),
        );
        let id = Id::new(("view-toggle", index));
        let label = translate(props.locale, key);
        let active = props.mode == mode;
        let response = ui.interact(r, id, Sense::click());
        response.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, active, &label));
        ui.ctx()
            .accesskit_node_builder(id, |node| node.set_role(Role::Button));
        let hovered = response.hovered();
        let ctx = ui.ctx().clone();
        let bg = anim::transition(
            &ctx,
            id.with("bg"),
            if active { v.bg_card } else { Rgba::TRANSPARENT },
            &T100,
        );
        let fg = anim::transition(
            &ctx,
            id.with("fg"),
            if active || hovered { v.fg } else { v.fg_muted },
            &T100,
        );
        BoxStyle {
            background: bg,
            radius: Corners::all(6.0),
            ..BoxStyle::default()
        }
        .paint(ui.painter(), r);
        let svg_rect = Rect::from_min_size(
            r.min
                + vec2(
                    snap::centre(BUTTON_W, 14.0, ppp),
                    snap::centre(inner_h, 14.0, ppp),
                ),
            vec2(14.0, 14.0),
        );
        icon::paint(ui.painter(), svg, svg_rect, fg, 1.0);
        if props.focus_visible && response.has_focus() {
            super::focus::ua_ring(ui, r, 6.0, v);
        }
        if response.clicked() {
            changed = Some(mode);
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outer_box_matches_the_reference() {
        // Measured in WebView2: 66 x 30, buttons 30 x 26 at +2 and +34.
        assert_eq!(WIDTH, 66.0);
        assert_eq!(HEIGHT, 30.0);
    }
}
