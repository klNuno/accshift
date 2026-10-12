//! `src/lib/shared/components/SectionHeader.svelte` (re-exported by
//! `features/folders/SectionHeader.svelte`): the header of a section in the
//! expanded folders layout.
//!
//! `.section-header`: flex, `align-items: center`, `gap: 10px`, `padding:
//! 4px`, 12 px 500, `line-height: 1`, `--fg`; when clickable its `:hover`
//! colour is `color-mix(--section-accent 65%, --fg)` (120 ms ease-out).
//! `--section-accent` is the card colour, else the accent. Children: a
//! dashed rule, the content (`gap: 8px`: chevron, folder icon, name, count),
//! a second rule; the rules split the free width.
//!
//! - chevron: 12 px, stroke 2.4, `color-mix(accent 60%, --fg-muted)`,
//!   `rotate(180deg)` while expanded, 220 ms `cubic-bezier(.4, 0, .2, 1)`;
//! - folder icon: 16 px, stroke 1.6, `color-mix(accent 70%, --fg-muted)`;
//! - name: at most 280 px with an ellipsis;
//! - count: `padding: 2px 7px`, pill, 11 px 600, `line-height: 1`,
//!   `color-mix(--bg-muted 75%, transparent)` on `--fg-muted`; with a card
//!   colour `color-mix(accent 22%, --bg-muted)` on `color-mix(accent 65%,
//!   --fg)`;
//! - rule: 2 px, `repeating-linear-gradient(to right, c 0 6px, transparent
//!   6px 12px)`, c `color-mix(accent 55%, --border)` (70 % with a card
//!   colour) at opacity .85; at the root `color-mix(--border 90%,
//!   transparent)` at .7.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use egui::accesskit::Role;
use egui::{pos2, vec2, Id, Pos2, Rect, Sense, Ui, WidgetInfo, WidgetType};

use super::icon;
use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim::{self, Timing, Transition};
use crate::ui::paint::{BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextStyle};

/// `transition: color 120ms ease-out`.
const T_COLOR: Transition = Transition::new(0.12, Timing::EASE_OUT);
/// `.chevron { transition: transform 220ms cubic-bezier(0.4, 0, 0.2, 1) }`.
const T_CHEVRON: Transition = Transition::new(0.22, Timing::cubic_bezier(0.4, 0.0, 0.2, 1.0));
/// `.section-header { padding: 4px }`.
const PAD: f32 = 4.0;
/// `.section-header { gap: 10px }`.
const GAP: f32 = 10.0;
/// `.content { gap: 8px }`.
const CONTENT_GAP: f32 = 8.0;
/// `.name { max-width: 280px }`.
const NAME_MAX: f32 = 280.0;

/// The folder icon at stroke 1.6.
const FOLDER_ICON: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"white\" stroke-width=\"1.6\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><path d=\"M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z\"/></svg>";

/// What the header reports.
#[derive(Clone, Debug, PartialEq)]
pub enum SectionHeaderEvent {
    /// Click: `onToggle`.
    Toggle,
    /// Double click on a folder header: `onNavigate(folder.id)`.
    Navigate(String),
    /// Right click on a folder header: `onContextMenu`.
    ContextMenu(Pos2),
}

#[derive(Clone, Debug)]
pub struct SectionHeaderProps<'a> {
    pub vars: &'a Vars,
    /// Stable per section.
    pub id: Id,
    /// The folder id, `None` for the root section (`is-root`).
    pub folder: Option<&'a str>,
    pub label: &'a str,
    pub count: usize,
    /// `cardColor` resolved, `None` for `""`.
    pub card_color: Option<Rgba>,
    /// `accentColor`.
    pub accent: Rgba,
    pub collapsed: bool,
    pub focus_visible: bool,
}

/// `.section-header`'s height: the tallest child plus the padding. The
/// folder icon (16) sets it for a folder, the count pill (15) at the root.
pub fn height(props: &SectionHeaderProps) -> f32 {
    let content = if props.folder.is_some() { 16.0 } else { 15.0 };
    content + 2.0 * PAD
}

fn name_style() -> TextStyle {
    TextStyle::new(12.0, 500)
        .line_height(LineHeight::Number(1.0))
        .nowrap(true)
}

fn count_style() -> TextStyle {
    TextStyle::new(11.0, 600).line_height(LineHeight::Number(1.0))
}

/// The chevron rotated by `degrees` about its centre, rasterized once per
/// 6 degree step (the transition runs 220 ms).
fn chevron(degrees: f32) -> &'static [u8] {
    static CACHE: OnceLock<Mutex<HashMap<i32, &'static [u8]>>> = OnceLock::new();
    let step = ((degrees / 6.0).round() as i32).clamp(0, 30) * 6;
    let mut map = CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    map.entry(step).or_insert_with(|| {
        let svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"white\" stroke-width=\"2.4\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><g transform=\"rotate({step} 12 12)\"><polyline points=\"6 9 12 15 18 9\"/></g></svg>"
        );
        Box::leak(svg.into_bytes().into_boxed_slice())
    })
}

/// Draws the header with its border box at `rect`.
pub fn section_header(
    ui: &mut Ui,
    rect: Rect,
    props: &SectionHeaderProps,
) -> Vec<SectionHeaderEvent> {
    let v = props.vars;
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let accent = props.card_color.unwrap_or(props.accent);
    let has_color = props.card_color.is_some();
    let is_root = props.folder.is_none();

    let response = ui.interact(rect, props.id, Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| {
        WidgetInfo::selected(WidgetType::Button, true, !props.collapsed, props.label)
    });
    ctx.accesskit_node_builder(props.id, |node| {
        node.set_role(Role::Button);
        node.set_expanded(!props.collapsed);
    });
    let color = anim::transition(
        &ctx,
        props.id.with("color"),
        if response.hovered() {
            accent.mix(0.65, v.fg)
        } else {
            v.fg
        },
        &T_COLOR,
    );
    let angle = anim::transition(
        &ctx,
        props.id.with("chevron"),
        if props.collapsed { 0.0f32 } else { 180.0 },
        &T_CHEVRON,
    );

    // Content widths.
    let name_w = text::measure(&ctx, props.label, &name_style())
        .unwrap_or(0.0)
        .min(NAME_MAX);
    let count_text = props.count.to_string();
    let count_w = text::measure(&ctx, &count_text, &count_style()).unwrap_or(0.0) + 14.0;
    let mut content_w = 12.0 + CONTENT_GAP + name_w + CONTENT_GAP + count_w;
    if !is_root {
        content_w += 16.0 + CONTENT_GAP;
    }
    let inner = rect.shrink(PAD);
    let rule_w = ((inner.width() - content_w - 2.0 * GAP) / 2.0).max(0.0);
    let content_h = inner.height();
    let mid = |h: f32| inner.top() + (content_h - h) / 2.0;

    // Rules.
    let (rule_color, rule_opacity) = if is_root {
        (v.border.mix(0.9, Rgba::TRANSPARENT), 0.7)
    } else if has_color {
        (accent.mix(0.70, v.border), 0.85)
    } else {
        (accent.mix(0.55, v.border), 0.85)
    };
    let rule_y = mid(2.0);
    let left_rule = Rect::from_min_size(pos2(inner.left(), rule_y), vec2(rule_w, 2.0));
    let right_rule = Rect::from_min_size(pos2(inner.right() - rule_w, rule_y), vec2(rule_w, 2.0));
    for rule in [left_rule, right_rule] {
        dashes(ui, rule, rule_color.alpha(rule_color.a * rule_opacity));
    }

    // Content.
    let mut x = inner.left() + rule_w + GAP;
    let chevron_rect = Rect::from_min_size(pos2(x, mid(12.0)), vec2(12.0, 12.0));
    icon::paint(
        ui.painter(),
        chevron(angle),
        snap_icon(chevron_rect, ppp),
        accent.mix(0.60, v.fg_muted),
        1.0,
    );
    x += 12.0 + CONTENT_GAP;
    if !is_root {
        let icon_rect = Rect::from_min_size(pos2(x, mid(16.0)), vec2(16.0, 16.0));
        icon::paint(
            ui.painter(),
            FOLDER_ICON,
            snap_icon(icon_rect, ppp),
            accent.mix(0.70, v.fg_muted),
            1.0,
        );
        x += 16.0 + CONTENT_GAP;
    }
    let name_rect = Rect::from_min_size(pos2(x, mid(12.0)), vec2(name_w, 12.0));
    text::paint(
        ui.painter(),
        name_rect,
        props.label,
        &name_style().nowrap(true).color(color.to_color32()),
    );
    x += name_w + CONTENT_GAP;
    let pill = Rect::from_min_size(pos2(x, mid(15.0)), vec2(count_w, 15.0));
    let (pill_bg, pill_fg) = if has_color {
        (accent.mix(0.22, v.bg_muted), accent.mix(0.65, v.fg))
    } else {
        (v.bg_muted.mix(0.75, Rgba::TRANSPARENT), v.fg_muted)
    };
    BoxStyle {
        background: pill_bg,
        radius: Corners::all(999.0),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), pill);
    text::paint(
        ui.painter(),
        Rect::from_min_size(pill.min + vec2(7.0, 2.0), vec2(count_w - 14.0, 11.0)),
        &count_text,
        &count_style().color(pill_fg.to_color32()),
    );

    if props.focus_visible && response.has_focus() {
        super::focus::ua_ring(ui, rect, 0.0, v);
    }
    let mut events = Vec::new();
    if response.clicked() {
        events.push(SectionHeaderEvent::Toggle);
    }
    if let Some(folder) = props.folder {
        if response.double_clicked() {
            events.push(SectionHeaderEvent::Navigate(folder.to_string()));
        }
        if response.secondary_clicked() {
            let at = super::context_click_pos(ui).unwrap_or(rect.center());
            events.push(SectionHeaderEvent::ContextMenu(at));
        }
    }
    events
}

/// An inline `<svg>` box: its origin snaps, its size stays.
fn snap_icon(rect: Rect, ppp: f32) -> Rect {
    Rect::from_min_size(snap::point(rect.min, ppp), rect.size())
}

/// The repeating gradient: 6 px of `color` every 12 px from the rule's
/// left edge, clipped to the rule.
fn dashes(ui: &Ui, rule: Rect, color: Rgba) {
    if color.a <= 0.0 || rule.width() <= 0.0 {
        return;
    }
    // The background box snaps to device pixels, then each pixel takes the
    // gradient's colour at its centre: the hard stops come out crisp, their
    // phase measured from the snapped left edge.
    let ppp = ui.ctx().pixels_per_point();
    let snapped = snap::rect(rule, ppp);
    let left = (snapped.left() * ppp).round() as i32;
    let right = (snapped.right() * ppp).round() as i32;
    let on = |i: i32| ((i as f32 + 0.5) / ppp - snapped.left()).rem_euclid(12.0) < 6.0;
    let painter = ui.painter();
    let c = color.to_color32();
    let mut i = left;
    while i < right {
        let start = i;
        let dash = on(i);
        while i < right && on(i) == dash {
            i += 1;
        }
        if dash {
            painter.rect_filled(
                Rect::from_min_max(
                    pos2(start as f32 / ppp, snapped.top()),
                    pos2(i as f32 / ppp, snapped.bottom()),
                ),
                0.0,
                c,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heights_match_the_reference() {
        let vars = crate::app::vars::Vars::new(&accshift_theme::ResolvedTheme::new(
            &accshift_theme::theme_application(
                &accshift_theme::ThemeRegistry::new().theme(Some("glass-dark")),
                85.0,
                true,
            ),
        ));
        let mut props = SectionHeaderProps {
            vars: &vars,
            id: Id::new("h"),
            folder: Some("f"),
            label: "Smurfs",
            count: 3,
            card_color: None,
            accent: vars.accent,
            collapsed: false,
            focus_visible: false,
        };
        assert_eq!(height(&props), 24.0);
        props.folder = None;
        assert_eq!(height(&props), 23.0);
    }

    #[test]
    fn chevron_steps_are_cached() {
        assert!(std::ptr::eq(chevron(180.0), chevron(179.0)));
        assert!(!std::ptr::eq(chevron(0.0), chevron(180.0)));
    }
}
