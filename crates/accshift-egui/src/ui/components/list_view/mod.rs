//! The list layout: folder and account rows on the left, the selected
//! account's preview in a 200 px panel on the right.
//!
//! ```text
//! layout        the border colour in a 1 px gap between the panels, radius 8
//!   rows panel  padding 4, boxes 2 px apart, scrolls vertically
//!     row       padding 6 by 10, icon and text 10 px apart, radius 6
//!   preview     200 px on the card colour, a centred column, padding 24 by 16
//! ```
//!
//! A frame runs in this order: `layout` stacks the boxes, `motion` slides
//! folding sections and moved rows, `scroll` applies the wheel and brings
//! the keyboard's row into view, `rows` draws every box (each with `row`
//! and `avatar`), `ghost` draws the dragged clone, `scroll` settles the
//! focus and the scrollbar, and `preview` draws the right panel.

use std::collections::HashMap;

use accshift_app::display::{AccountCardItem, DisplayItem};
use accshift_app::folders::{FolderInfo, ItemRef};
use accshift_app::i18n::Locale;
use accshift_app::platforms::PlatformAccount;
use accshift_app::time::{format_relative_time_compact, TimeContext};
use egui::{pos2, vec2, Id, Pos2, Rect, TextureId, Ui};

use super::account_card::AvatarContent;
use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim::{self, Timing, Transition};
use crate::ui::paint::{BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{LineHeight, TextStyle};

mod avatar;
mod ghost;
mod layout;
mod motion;
mod preview;
mod row;
mod rows;
mod scroll;
#[cfg(test)]
mod tests;

/// Width of the preview panel.
pub const PREVIEW_W: f32 = 200.0;
/// Corner radius of the whole layout.
const RADIUS: f32 = 8.0;
/// Padding of the rows panel, and the gap between its boxes.
const PAD: f32 = 4.0;
const GAP: f32 = 2.0;
/// Side of a row's avatar or icon box.
const ICON: f32 = 32.0;
/// A row's padding, and the gap between its icon and its text.
const ROW_PAD_X: f32 = 10.0;
const ROW_PAD_Y: f32 = 6.0;
const ROW_GAP: f32 = 10.0;
/// Width of the rows panel's scrollbar.
const SCROLLBAR: f32 = 10.0;
/// A row's background transition.
const T_ROW: Transition = Transition::new(0.1, Timing::EASE);
/// The preview's switch button brightness transition.
const T_BUTTON: Transition = Transition::new(0.15, Timing::EASE);
/// One turn of the avatar spinner, in seconds.
const SPIN: f64 = 0.75;
/// How long a section's rows take to fold or unfold, in seconds.
const SLIDE: f64 = 0.22;
/// How long a row moved by a drag takes to reach its new place, in seconds.
const FLIP: f64 = 0.2;
const RED: Rgba = Rgba::new(239.0, 68.0, 68.0, 1.0);
const YELLOW: Rgba = Rgba::new(234.0, 179.0, 8.0, 1.0);
const ORANGE: Rgba = Rgba::new(251.0, 146.0, 60.0, 1.0);

/// State kept across frames.
#[derive(Default)]
pub struct ListState {
    /// The account whose preview shows.
    pub selected: Option<String>,
    /// How far the rows panel is scrolled.
    pub scroll: f32,
    thumb_drag: Option<(f32, f32)>,
    pointer: Option<Pos2>,
    focused: Option<Id>,
    /// The last frame's rows and headers with their labels, on screen,
    /// scrolled: the behaviour tests read them.
    pub layout: Vec<(String, Rect)>,
    /// The last frame's folder and account rows on screen, in drawing order.
    pub cards: Vec<(ItemRef, Rect)>,
    /// The last frame's back row on screen.
    pub back: Option<Rect>,
    /// The last frame's section boxes (header and rows) on screen, with their
    /// folder (none for the root's).
    pub section_boxes: Vec<(Option<String>, Rect)>,
    /// Per row key, the offset a moved row slides back from, and since when.
    flips: HashMap<String, (f32, f64)>,
    /// Per row key, its laid out and its shown top last frame.
    last_tops: HashMap<String, (f32, f32)>,
    /// Per section key: folded last frame, and its rows while unfolded.
    sections: HashMap<String, (bool, Vec<DisplayItem>)>,
    /// The sections folding or unfolding, by key.
    slides: HashMap<String, motion::Slide>,
}

/// What the list needs from the workspace.
pub trait ListHost {
    /// The mount id of the box keyed `key`: a new one after a remount.
    fn mount(&mut self, key: &str) -> Id;
    /// The avatar of `item` at `px` device pixels, its decode started.
    fn avatar(
        &mut self,
        key: &str,
        item: &AccountCardItem,
        px: u32,
        visible: bool,
    ) -> AvatarContent;
    /// The picture of `item` at `px`, blurred by `sigma` device pixels.
    fn blurred(&mut self, item: &AccountCardItem, px: u32, sigma: f32) -> Option<TextureId>;
    /// A card colour setting resolved: `None` for an empty one.
    fn card_color(&self, value: &str) -> Option<Rgba>;
}

pub struct ListProps<'a> {
    pub vars: &'a Vars,
    pub locale: Locale,
    /// The accent colour.
    pub accent: Rgba,
    pub focus_visible: bool,
    /// A dark colour scheme, for the focus ring.
    pub dark: bool,
    pub time: &'a TimeContext,
    /// The row the arrow keys focused, painted with the keyboard focus ring.
    pub kb_focus: Option<&'a str>,
    /// Scroll that row into view this frame.
    pub kb_reveal: bool,
    /// The folder row a drag hovers, painted as a drop target.
    pub drag_over_folder: Option<&'a str>,
    /// A drag hovers the back row.
    pub drag_over_back: bool,
    /// A drag is under way: the flat rows slide to their preview places.
    pub dragging: bool,
    /// The dragged row's clone that follows the pointer.
    pub ghost: Option<RowGhost<'a>>,
}

/// The dragged row's clone over a list, taken when the drag started.
pub struct RowGhost<'a> {
    pub item: &'a ItemRef,
    /// The source row's box when the drag started.
    pub rect: Rect,
    /// How far the pointer went since.
    pub offset: egui::Vec2,
    /// The clone's own mount.
    pub id: Id,
}

/// What the list reports.
#[derive(Clone, Debug, PartialEq)]
pub enum ListEvent {
    /// The back row was clicked.
    Back,
    /// A folder row click or a header double click: open the folder.
    Navigate(String),
    FolderMenu(FolderInfo, Pos2),
    /// An account row click, reported before the selection changes.
    Activate(String),
    /// A double click, the preview's button, or a click in bulk edit.
    Switch(String),
    AccountMenu(Box<PlatformAccount>, Pos2),
    /// A header click: fold or unfold the section.
    ToggleSection(String),
}

/// A meta line under the name: line height 1.2, on one line.
fn meta_style(size: f32, weight: u16, color: Rgba) -> TextStyle {
    TextStyle::new(size, weight)
        .line_height(LineHeight::Number(1.2))
        .nowrap(true)
        .color(color.to_color32())
}

/// The last login line's colour: 40 % of the subtle text colour, 60 % of
/// the text colour.
fn last_login_color(v: &Vars) -> Rgba {
    v.fg_subtle.mix(0.4, v.fg)
}

// An account still in setup shows neither meta line, as on its grid card.
fn has_username(a: &AccountCardItem) -> bool {
    a.show_username && !a.pending_setup && !a.account.username.trim().is_empty()
}

fn shows_last_login(a: &AccountCardItem) -> bool {
    a.show_last_login && !a.pending_setup
}

/// The last login line's text, when the account shows one.
fn last_login_label(a: &AccountCardItem, time: &TimeContext) -> Option<String> {
    shows_last_login(a)
        .then(|| format_relative_time_compact(a.last_login_at_sec, time, a.last_login_unknown_key))
}

fn ceil_unit(w: f32, ppp: f32) -> f32 {
    (w * ppp * 64.0).ceil() / 64.0 / ppp
}

/// The key a box mounts under.
fn key_of(item: &DisplayItem) -> String {
    match item {
        DisplayItem::Back => "list/back".into(),
        DisplayItem::SectionHeader(h) => format!("list/h/{}", h.collapse_key),
        DisplayItem::Folder(f) => format!("list/f/{}", f.folder.id),
        DisplayItem::Account(a) => format!("list/a/{}", a.account.id),
    }
}

/// The card focus id of a folder or account row.
fn item_id(item: &DisplayItem) -> Option<&str> {
    match item {
        DisplayItem::Folder(f) => Some(&f.folder.id),
        DisplayItem::Account(a) => Some(&a.account.id),
        _ => None,
    }
}

/// The drag reference of a folder or account row.
fn item_ref(item: &DisplayItem) -> Option<ItemRef> {
    match item {
        DisplayItem::Folder(f) => Some(ItemRef::folder(&f.folder.id)),
        DisplayItem::Account(a) => Some(ItemRef::account(&a.account.id)),
        _ => None,
    }
}

/// Whether a folder or account row is the one dragged; `None` for a header
/// or the back row, which cannot be.
fn dragged(item: &DisplayItem) -> Option<bool> {
    match item {
        DisplayItem::Folder(f) => Some(f.dragged),
        DisplayItem::Account(a) => Some(a.dragged),
        _ => None,
    }
}

fn find_account<'a>(items: &'a [DisplayItem], id: &str) -> Option<&'a AccountCardItem> {
    items.iter().find_map(|item| match item {
        DisplayItem::Account(a) if a.account.id == id => Some(&**a),
        _ => None,
    })
}

/// A stale selection goes: bulk edit took the clicks, or the account left
/// the list.
fn drop_stale_selection(state: &mut ListState, items: &[DisplayItem]) {
    let bulk = items
        .iter()
        .any(|item| matches!(item, DisplayItem::Account(a) if a.single_click_switch));
    let gone = || {
        state
            .selected
            .as_deref()
            .and_then(|id| find_account(items, id))
            .is_none()
    };
    if bulk || gone() {
        state.selected = None;
    }
}

/// Draws the list layout over `rect`.
pub fn list_view(
    ui: &mut Ui,
    rect: Rect,
    state: &mut ListState,
    items: &[DisplayItem],
    props: &ListProps,
    host: &mut dyn ListHost,
) -> Vec<ListEvent> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    drop_stale_selection(state, items);

    // The border colour shows through the 1 px gap between the panels.
    let panel = Rect::from_min_size(
        rect.min,
        vec2(rect.width() - PREVIEW_W - 1.0, rect.height()),
    );
    let preview = Rect::from_min_max(pos2(rect.right() - PREVIEW_W, rect.top()), rect.max);
    let painter = ui.painter().clone();
    let fill = |r: Rect, color: Rgba, radius: Corners| {
        BoxStyle {
            background: color,
            radius,
            ..BoxStyle::default()
        }
        .paint(&painter, snap::rect(r, ppp));
    };
    fill(rect, v.border, Corners::all(RADIUS));
    fill(panel, v.bg, Corners::new(RADIUS, 0.0, 0.0, RADIUS));
    fill(preview, v.bg_card, Corners::new(0.0, RADIUS, RADIUS, 0.0));

    let blocks = layout::blocks(items);
    let now = ctx.input(|i| i.time);
    let slides = motion::update_slides(&ctx, state, &blocks, now);
    let layout::Column {
        mut places,
        wrappers,
        bottom,
    } = layout::column(&blocks, &slides, v, ppp, now);
    if wrappers.is_empty() {
        motion::flip(&ctx, state, &mut places, props.dragging, now);
    } else {
        state.flips.clear();
        state.last_tops.clear();
    }
    let content_h = bottom + PAD;
    let max_scroll = (content_h - panel.height()).max(0.0);
    let scrolls = max_scroll > 0.0;
    let row_w = panel.width() - 2.0 * PAD - if scrolls { SCROLLBAR } else { 0.0 };

    let scroll_before = state.scroll;
    scroll::wheel(
        &ctx,
        panel.intersect(ui.clip_rect()),
        panel,
        state,
        max_scroll,
    );
    scroll::reveal_kb_focus(state, &places, props, panel.height(), max_scroll);

    let view = rows::Viewport {
        panel,
        clip: panel.intersect(ui.clip_rect()),
        scrolled: panel.min.to_vec2() - vec2(0.0, state.scroll),
        row_w,
    };
    state.section_boxes = wrappers
        .into_iter()
        .map(|(folder, top, bottom)| {
            let local = Rect::from_x_y_ranges(PAD..=PAD + row_w, top..=bottom);
            (folder, local.translate(view.scrolled))
        })
        .collect();
    let section_ghost = ghost::dragged_section(props, &state.section_boxes);
    let drawn = rows::draw_rows(ui, &view, &places, section_ghost, state, props, host);
    // The host's mount made each row the owner of its state; the rest of
    // the list belongs to no row.
    anim::release(&ctx);

    if let Some(g) = &props.ghost {
        let selected = state.selected.as_deref();
        match (&drawn.ghost, section_ghost) {
            (Some((part, h)), _) => ghost::ghost_row(&ctx, g, part, *h, selected, props, host),
            (None, Some(at)) => {
                let parts = &drawn.section_parts;
                ghost::ghost_section(&ctx, g, parts, at.min, selected, props, host);
            }
            (None, None) => {}
        }
    }
    let rows::RowsDrawn {
        events: mut list_events,
        focused,
        focused_box,
        ..
    } = drawn;
    drop(places);
    state.slides = slides;

    scroll::reveal_focus(state, focused, focused_box, panel.height());
    if scrolls {
        scroll::scrollbar(ui, panel, state, v, content_h, max_scroll);
    } else {
        state.thumb_drag = None;
    }
    state.scroll = state.scroll.clamp(0.0, max_scroll).round();
    if state.scroll != scroll_before {
        ctx.request_repaint();
    }

    let selected = state
        .selected
        .as_deref()
        .and_then(|id| find_account(items, id));
    list_events.extend(preview::preview(ui, preview, selected, props, host));
    list_events
}
