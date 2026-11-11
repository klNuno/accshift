//! Registers and paints what one box lists: entries, separators and swatch
//! groups, and the message of a submenu with nothing to list.

use accshift_app::actions::dialogs::{AppMenuEntry, AppMenuSwatch};
use accshift_app::i18n::Locale;
use accshift_app::text::Text;
use egui::accesskit::{self, Role};
use egui::{pos2, vec2, Id, Painter, Pos2, Rect, Response, Sense, Ui, Vec2};

use super::layout::{
    arrow_style, arrow_width, item_style, label_style, line, state_style, Layout, Row, ARROW,
    ARROW_GAP, FILL, FILL_RADIUS, GROUP_GAP, GROUP_PAD_X, GROUP_PAD_Y, ITEM_PAD_X, ITEM_PAD_Y,
    ITEM_RADIUS, ITEM_SIZE, PAD, RADIUS, SEP_X, SEP_Y, STATE_PAD, SUB_GROUP_PAD_TOP, SWATCH,
    SWATCH_GAP, SWATCH_RADIUS,
};
use super::{has_submenu, target_id, Target};
use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim::{self, Timing, Transition};
use crate::ui::css;
use crate::ui::paint::{
    Border, BoxShadow, BoxStyle, ColorStop, Corners, Gradient, Length, LinearDirection,
    LinearGradient,
};
use crate::ui::snap;
use crate::ui::text;

/// An entry's background easing between rest, hover and press.
const T80: Transition = Transition::new(0.08, Timing::EASE);

/// The box of a menu or submenu, its shadow included.
pub(super) fn paint_box(ui: &Ui, rect: Rect, v: &Vars, border: f32) {
    BoxStyle {
        background: v.bg_overlay,
        images: v.card_bg_image.clone(),
        radius: Corners::all(RADIUS),
        border: Border::all(border, v.border),
        shadows: vec![BoxShadow {
            offset: vec2(0.0, 8.0),
            blur: 24.0,
            spread: 0.0,
            color: Rgba::new(0.0, 0.0, 0.0, 0.4),
            inset: false,
        }],
        ..BoxStyle::default()
    }
    .paint(ui.painter(), snap::rect(rect, ui.ctx().pixels_per_point()));
}

/// The fill of a swatch without a colour: the muted background over the
/// top-left half, the elevated one over the bottom-right half.
fn default_fill(v: &Vars) -> Gradient {
    let stop = |color, p| ColorStop {
        color,
        position: Some(Length::Percent(p)),
    };
    Gradient::Linear(LinearGradient {
        direction: LinearDirection::Angle(135.0),
        stops: vec![
            stop(v.bg_muted, 0.0),
            stop(v.bg_muted, 50.0),
            stop(v.bg_elevated, 50.0),
            stop(v.bg_elevated, 100.0),
        ],
    })
}

/// What the pointer did in one box: the last target it is over, clicked
/// and pressed.
#[derive(Default)]
pub(super) struct BoxInput {
    pub(super) over: Option<Target>,
    pub(super) clicked: Option<Target>,
    pub(super) pressed: Option<Target>,
}

/// A box to draw: where, what it lists, its ids and its focus.
pub(super) struct Frame<'a> {
    pub(super) rect: Rect,
    pub(super) layout: &'a Layout,
    pub(super) entries: &'a [AppMenuEntry],
    pub(super) submenu: bool,
    pub(super) id: Id,
    pub(super) focused: Option<Target>,
    /// The entry whose submenu is open.
    pub(super) expanded: Option<usize>,
    pub(super) vars: &'a Vars,
    pub(super) locale: Locale,
    /// Keyboard modality: focus shows.
    pub(super) keyboard: bool,
}

/// Registers and paints the entries of one box.
pub(super) fn draw_entries(ui: &mut Ui, f: &Frame) -> BoxInput {
    let ppp = ui.ctx().pixels_per_point();
    let mut painter = ui.painter().clone();
    painter.set_clip_rect(
        painter
            .clip_rect()
            .intersect(f.rect.shrink(f.layout.border)),
    );
    let mut pen = Pen {
        f,
        painter,
        ppp,
        origin: f.rect.min + Vec2::splat(f.layout.border + PAD),
        inner_w: f.rect.width() - 2.0 * (f.layout.border + PAD),
        out: BoxInput::default(),
    };
    for ((entry, e), row) in f.entries.iter().enumerate().zip(&f.layout.rows) {
        match e {
            AppMenuEntry::Separator => pen.separator(*row),
            AppMenuEntry::Item { label, .. } => {
                let parent = !f.submenu && has_submenu(e);
                pen.item(ui, entry, label, parent, *row);
            }
            AppMenuEntry::Swatches {
                label, swatches, ..
            } => pen.swatches(ui, entry, label, swatches, *row),
        }
    }
    pen.out
}

/// What the entries of one box draw with, and what the pointer did so far.
struct Pen<'a> {
    f: &'a Frame<'a>,
    /// Clipped to the box's padding box.
    painter: Painter,
    ppp: f32,
    /// The content box's top left and width.
    origin: Pos2,
    inner_w: f32,
    out: BoxInput,
}

impl Pen<'_> {
    fn separator(&self, row: Row) {
        let top = self.origin.y + row.top;
        let r = Rect::from_min_size(
            pos2(self.origin.x + SEP_X, top + SEP_Y),
            vec2(self.inner_w - 2.0 * SEP_X, 1.0),
        );
        self.painter.rect_filled(
            snap::rect(r, self.ppp),
            0.0,
            self.f.vars.border.to_color32(),
        );
    }

    /// One entry: its background, label and, for a submenu parent, the `›`.
    fn item(&mut self, ui: &mut Ui, entry: usize, label: &Text, parent: bool, row: Row) {
        let (v, ppp) = (self.f.vars, self.ppp);
        let t = Target::item(entry);
        let top = self.origin.y + row.top;
        let rect = Rect::from_min_size(pos2(self.origin.x, top), vec2(self.inner_w, row.height));
        let label = label.resolve(self.f.locale);
        let expanded = parent.then_some(self.f.expanded == Some(entry));
        let response = self.register(ui, t, rect, &label, expanded);
        let target = if response.is_pointer_button_down_on() {
            v.bg_elevated
        } else if self.lit(t, &response) {
            v.bg_muted
        } else {
            Rgba::TRANSPARENT
        };
        let bg = anim::transition(ui.ctx(), response.id.with("bg"), target, &T80);
        if bg.a > 0.0 {
            BoxStyle {
                background: bg,
                radius: Corners::all(ITEM_RADIUS),
                ..BoxStyle::default()
            }
            .paint(&self.painter, snap::rect(rect, ppp));
        }
        let arrow_w = if parent {
            arrow_width(ui.ctx(), v)
        } else {
            0.0
        };
        let text_rect = Rect::from_min_size(
            pos2(rect.left() + ITEM_PAD_X, top + ITEM_PAD_Y),
            vec2(
                rect.width() - 2.0 * ITEM_PAD_X - if parent { ARROW_GAP + arrow_w } else { 0.0 },
                row.label,
            ),
        );
        text::paint(&self.painter, text_rect, &label, &item_style(v));
        if parent {
            let line = line(ui.ctx(), &arrow_style(v), ITEM_SIZE);
            let r = Rect::from_min_size(
                pos2(rect.right() - ITEM_PAD_X - arrow_w, top + ITEM_PAD_Y),
                vec2(arrow_w, line),
            );
            text::paint(&self.painter, r, ARROW, &arrow_style(v));
        }
    }

    /// A swatch group: its label, then one focusable swatch per colour.
    fn swatches(
        &mut self,
        ui: &mut Ui,
        entry: usize,
        label: &Text,
        swatches: &[AppMenuSwatch],
        row: Row,
    ) {
        let (v, ppp) = (self.f.vars, self.ppp);
        let top = self.origin.y + row.top;
        let pad_top = if self.f.submenu {
            SUB_GROUP_PAD_TOP
        } else {
            GROUP_PAD_Y
        };
        let x0 = self.origin.x + GROUP_PAD_X;
        let label_rect = Rect::from_min_size(
            pos2(x0, top + pad_top),
            vec2(self.inner_w - 2.0 * GROUP_PAD_X, row.label),
        );
        text::paint(
            &self.painter,
            label_rect,
            &label.resolve(self.f.locale),
            &label_style(v),
        );
        let y = top + pad_top + row.label + GROUP_GAP;
        for (k, sw) in swatches.iter().enumerate() {
            let t = Target {
                entry,
                swatch: Some(k),
            };
            let rect = Rect::from_min_size(
                pos2(x0 + k as f32 * (SWATCH + SWATCH_GAP), y),
                Vec2::splat(SWATCH),
            );
            let name = sw.label.resolve(self.f.locale);
            let response = self.register(ui, t, rect, &name, None);
            let ring = self.lit(t, &response);
            paint_swatch(&self.painter, rect, sw, v, ring, ppp);
        }
    }

    /// Registers one focusable target with the pointer and assistive
    /// technology, and notes what the pointer did to it. `expanded` is set
    /// for a submenu parent: whether its submenu is open.
    fn register(
        &mut self,
        ui: &mut Ui,
        t: Target,
        rect: Rect,
        name: &str,
        expanded: Option<bool>,
    ) -> Response {
        let id = target_id(self.f.id, t);
        let response = ui.interact(snap::rect(rect, self.ppp), id, Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        ui.ctx().accesskit_node_builder(id, |node| {
            node.set_role(Role::MenuItem);
            node.set_label(name);
            if let Some(expanded) = expanded {
                node.set_has_popup(accesskit::HasPopup::Menu);
                node.set_expanded(expanded);
            }
        });
        if response.hovered() {
            self.out.over = Some(t);
        }
        if pointer_click(ui, &response) {
            self.out.clicked = Some(t);
        }
        if ui.input(|i| i.pointer.primary_pressed()) && response.hovered() {
            self.out.pressed = Some(t);
        }
        response
    }

    /// Whether a target shows as hovered: under the pointer, or focused
    /// under keyboard modality.
    fn lit(&self, t: Target, response: &Response) -> bool {
        response.hovered() || (self.f.focused == Some(t) && self.f.keyboard)
    }
}

/// A mouse or assistive click. egui also clicks a focused widget on Enter
/// and Space, but the menu's key path already runs those.
fn pointer_click(ui: &Ui, response: &Response) -> bool {
    response.clicked_by(egui::PointerButton::Primary)
        || ui.input(|i| i.has_accesskit_action_request(response.id, accesskit::Action::Click))
}

/// One swatch: its box, ringed when active or `ring`, and its fill centred
/// in it.
fn paint_swatch(painter: &Painter, rect: Rect, sw: &AppMenuSwatch, v: &Vars, ring: bool, ppp: f32) {
    let bw = snap::border_width(1.0, ppp);
    // The active ring wins over the hover ring.
    let shadow = |color: Rgba| BoxShadow {
        offset: Vec2::ZERO,
        blur: 0.0,
        spread: 1.0,
        color,
        inset: false,
    };
    let shadows = if sw.active {
        vec![shadow(v.fg)]
    } else if ring {
        vec![shadow(v.fg.mix(0.35, Rgba::TRANSPARENT))]
    } else {
        Vec::new()
    };
    let boxed = snap::rect(rect, ppp);
    BoxStyle {
        background: v.bg_solid,
        radius: Corners::all(SWATCH_RADIUS),
        border: Border::all(bw, if sw.active { v.fg } else { v.border }),
        shadows,
        ..BoxStyle::default()
    }
    .paint(painter, boxed);
    // The fill, centred in the content box.
    let off = bw + (SWATCH - 2.0 * bw - FILL) / 2.0;
    let fill = Rect::from_min_size(rect.min + Vec2::splat(off), Vec2::splat(FILL));
    let (background, images) = match css::color(&sw.color).filter(|_| !sw.color.is_empty()) {
        Some(color) => (color, Vec::new()),
        None => (Rgba::TRANSPARENT, vec![default_fill(v)]),
    };
    BoxStyle {
        background,
        images,
        radius: Corners::all(FILL_RADIUS),
        ..BoxStyle::default()
    }
    .paint(painter, snap::rect(fill, ppp));
}

/// A submenu with nothing to list: its loading, failed or empty message.
pub(super) fn paint_state(ui: &Ui, rect: Rect, layout: &Layout, text: &str, error: bool, v: &Vars) {
    let origin = rect.min + Vec2::splat(layout.border + PAD);
    let r = Rect::from_min_size(
        origin + Vec2::splat(STATE_PAD),
        vec2(
            rect.width() - 2.0 * (layout.border + PAD + STATE_PAD),
            layout.rows[0].label,
        ),
    );
    text::paint(ui.painter(), r, text, &state_style(v, error));
}
