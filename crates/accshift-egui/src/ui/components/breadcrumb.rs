//! `src/lib/features/folders/Breadcrumb.svelte`: the platform name and the
//! folder path, each a button.
//!
//! `.breadcrumb`: flex, `align-items: center`, `gap: 4px`, wraps,
//! `min-height: 18px`, 12 px. `.crumb`: `padding: 2px 4px`, radius 3,
//! `--fg-subtle`, the inherited 1.5 line height (a 22 px box);
//! `:hover` `--bg-muted` and `--fg`; `.current` 600 in the accent colour
//! (inline style) and no hover background. `.sep`: "/" at 11 px in
//! `--bg-elevated` (a 16.5 px line box). `transition: all 100ms` (`ease`).
//! In the toolbar row the breadcrumb loses its bottom padding and takes the
//! free width (`flex: 1; min-width: 0`).

use egui::accesskit::Role;
use egui::{pos2, vec2, Id, Rect, Sense, Ui, WidgetInfo};

use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim::{self, Timing, Transition};
use crate::ui::paint::{BoxStyle, Corners};
use crate::ui::text::{self, LineHeight, TextStyle};

/// `transition: all 100ms`.
const T100: Transition = Transition::new(0.1, Timing::EASE);
/// `.breadcrumb { gap: 4px }`.
const GAP: f32 = 4.0;
/// `.crumb { padding: 2px 4px }`.
const PAD_X: f32 = 4.0;
const PAD_Y: f32 = 2.0;
/// The crumb's line box: 12 px at the inherited `line-height: 1.5`.
const CRUMB_LINE: f32 = 18.0;
/// The separator's line box: 11 px at 1.5.
const SEP_LINE: f32 = 16.5;
/// A crumb's border box height.
pub const CRUMB_H: f32 = CRUMB_LINE + 2.0 * PAD_Y;

/// One folder of the path.
#[derive(Clone, Copy, Debug)]
pub struct Crumb<'a> {
    pub id: &'a str,
    pub name: &'a str,
}

#[derive(Clone, Debug)]
pub struct BreadcrumbProps<'a> {
    pub vars: &'a Vars,
    pub platform_name: &'a str,
    pub path: &'a [Crumb<'a>],
    /// `accentColor`: the active platform's accent.
    pub accent: Rgba,
    pub focus_visible: bool,
}

fn crumb_style(current: bool) -> TextStyle {
    TextStyle::new(12.0, if current { 600 } else { 400 })
        .line_height(LineHeight::Number(1.5))
        .nowrap(false)
}

fn sep_style(vars: &Vars) -> TextStyle {
    TextStyle::new(11.0, 400)
        .line_height(LineHeight::Number(1.5))
        .nowrap(false)
        .color(vars.bg_elevated.to_color32())
}

/// The breadcrumb's height for `width`: one 22 px line per wrapped row.
pub fn height(ui: &Ui, props: &BreadcrumbProps, width: f32) -> f32 {
    let rows = layout(
        ui,
        props,
        Rect::from_min_size(pos2(0.0, 0.0), vec2(width, 0.0)),
    )
    .iter()
    .map(|p| p.row)
    .max()
    .unwrap_or(0);
    (rows + 1) as f32 * CRUMB_H
}

/// A placed crumb or separator.
struct Placed {
    rect: Rect,
    row: usize,
    /// `None` for a separator; else the crumb index (0 is the platform).
    crumb: Option<usize>,
}

/// Flex-wrap layout: items of one row share its 22 px line, each centred
/// in it (`align-items: center`).
fn layout(ui: &Ui, props: &BreadcrumbProps, rect: Rect) -> Vec<Placed> {
    let ctx = ui.ctx();
    let mut out = Vec::new();
    let mut x = rect.left();
    let mut row = 0usize;
    let mut first_in_row = true;
    let mut place = |w: f32, h: f32, crumb: Option<usize>, out: &mut Vec<Placed>| {
        if !first_in_row && x + GAP + w > rect.right() {
            row += 1;
            x = rect.left();
            first_in_row = true;
        }
        if !first_in_row {
            x += GAP;
        }
        let top = rect.top() + row as f32 * CRUMB_H + (CRUMB_H - h) / 2.0;
        out.push(Placed {
            rect: Rect::from_min_size(pos2(x, top), vec2(w, h)),
            row,
            crumb,
        });
        x += w;
        first_in_row = false;
    };
    let count = props.path.len() + 1;
    for index in 0..count {
        let name = if index == 0 {
            props.platform_name
        } else {
            props.path[index - 1].name
        };
        if index > 0 {
            let w = text::measure(ctx, "/", &sep_style(props.vars)).unwrap_or(0.0);
            place(w, SEP_LINE, None, &mut out);
        }
        let current = index + 1 == count;
        let w = text::measure(ctx, name, &crumb_style(current)).unwrap_or(0.0);
        place(w + 2.0 * PAD_X, CRUMB_H, Some(index), &mut out);
    }
    out
}

/// Draws the breadcrumb with its box at `rect` (its height from
/// [`height`]) and returns the folder clicked (`onNavigate`): `Some(None)`
/// for the platform root.
pub fn breadcrumb(ui: &mut Ui, rect: Rect, props: &BreadcrumbProps) -> Option<Option<String>> {
    let v = props.vars;
    let ctx = ui.ctx().clone();
    let count = props.path.len() + 1;
    let mut clicked = None;
    for placed in layout(ui, props, rect) {
        let Some(index) = placed.crumb else {
            text::paint(ui.painter(), placed.rect, "/", &sep_style(v));
            continue;
        };
        let current = index + 1 == count;
        let (name, folder) = if index == 0 {
            (props.platform_name, None)
        } else {
            let crumb = props.path[index - 1];
            (crumb.name, Some(crumb.id.to_string()))
        };
        let id = Id::unique(("breadcrumb", index));
        let response = ui.interact(placed.rect, id, Sense::click());
        response.widget_info(|| WidgetInfo::selected(Role::Button, true, current, name));
        ctx.accesskit_node_builder(id, |node| node.set_role(Role::Button));
        let hovered = response.hovered();
        let response = if current {
            response
        } else {
            response.on_hover_cursor(egui::CursorIcon::PointingHand)
        };
        // `.crumb.current:hover { background: transparent }`; the colour
        // stays the inline accent.
        let bg = if hovered && !current {
            v.bg_muted
        } else {
            Rgba::TRANSPARENT
        };
        let fg = if current {
            props.accent
        } else if hovered {
            v.fg
        } else {
            v.fg_subtle
        };
        let bg = anim::transition(&ctx, id.with("bg"), bg, &T100);
        let fg = anim::transition(&ctx, id.with("fg"), fg, &T100);
        BoxStyle {
            background: bg,
            radius: Corners::all(3.0),
            ..BoxStyle::default()
        }
        .paint(ui.painter(), placed.rect);
        let text_rect = Rect::from_min_size(
            placed.rect.min + vec2(PAD_X, PAD_Y),
            vec2(placed.rect.width() - 2.0 * PAD_X, CRUMB_LINE),
        );
        text::paint(
            ui.painter(),
            text_rect,
            name,
            &crumb_style(current).color(fg.to_color32()),
        );
        if props.focus_visible && response.has_focus() {
            super::focus::ua_ring(ui, placed.rect, 3.0, v);
        }
        if response.clicked() {
            clicked = Some(folder);
        }
    }
    clicked
}
