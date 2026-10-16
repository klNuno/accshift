//! `ListView.svelte` with its `ListRow` and `PreviewPanel`: the rows on the
//! left, the selected account's preview in a 200 px panel on the right.
//!
//! ```text
//! .list-layout      flex, gap 1px on --border, radius 8, overflow hidden
//!   .list-panel     flex 1, padding 4, column gap 2, --bg, overflow-y auto
//!     ListRow       flex, gap 10, padding 6 10, border --border-width
//!                   transparent, radius 6
//!   .preview-panel  200 px, --bg-card
//!     .preview      column centred both ways, padding 24 16
//! ```
//!
//! Not ported yet: the drag states.

use std::collections::HashMap;

use accshift_app::display::{AccountCardItem, DisplayItem, SectionHeaderItem, SectionLabel};
use accshift_app::folders::{FolderInfo, ItemRef};
use accshift_app::i18n::{translate, Locale};
use accshift_app::platforms::warnings::WarningTone;
use accshift_app::platforms::PlatformAccount;
use accshift_app::time::{format_relative_time_compact, TimeContext};
use egui::accesskit::Role;
use egui::{pos2, vec2, Color32, Event, Id, Key, Pos2, Rect, Sense, TextureId, Ui};

use super::account_card::{cover_uv, paint_ring_loader, AvatarContent};
use super::avatar_fade_in;
use super::dialog::auto_ring;
use super::icon;
use super::section_header::{self, SectionHeaderEvent, SectionHeaderProps};
use crate::app::vars::Vars;
use crate::icons;
use crate::theme::Rgba;
use crate::ui::anim::{self, Animation, Fill, Keyframes, Motion, Timing, Transition};
use crate::ui::css;
use crate::ui::paint::{self, Border, BoxShadow, BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextAlign, TextStyle, TextTransform};

/// `.preview-panel { width: 200px }`.
pub const PREVIEW_W: f32 = 200.0;
/// `.list-layout { border-radius: 8px }`.
const RADIUS: f32 = 8.0;
/// `.list-panel { padding: 4px; gap: 2px }`.
const PAD: f32 = 4.0;
const GAP: f32 = 2.0;
/// `.avatar` and `.icon`: 32 px.
const ICON: f32 = 32.0;
/// `.row { padding: 6px 10px; gap: 10px }`.
const ROW_PAD_X: f32 = 10.0;
const ROW_PAD_Y: f32 = 6.0;
const ROW_GAP: f32 = 10.0;
/// `::-webkit-scrollbar { width: 10px }`.
const SCROLLBAR: f32 = 10.0;
/// `.row { transition: background 100ms }`.
const T_ROW: Transition = Transition::new(0.1, Timing::EASE);
/// `.switch-btn { transition: filter 150ms }`.
const T_BUTTON: Transition = Transition::new(0.15, Timing::EASE);
/// `animation: spin 0.75s linear infinite`.
const SPIN: f64 = 0.75;
/// `transition:slide={{ duration: 220 }}`.
const SLIDE: f64 = 0.22;
/// `animate:flip`'s duration while a row is dragged.
const FLIP: f64 = 0.2;
const RED: Rgba = Rgba::new(239.0, 68.0, 68.0, 1.0);
const YELLOW: Rgba = Rgba::new(234.0, 179.0, 8.0, 1.0);
const ORANGE: Rgba = Rgba::new(251.0, 146.0, 60.0, 1.0);

/// State kept across frames.
#[derive(Default)]
pub struct ListState {
    /// `selectedAccountId`.
    pub selected: Option<String>,
    /// The panel's `scrollTop`.
    pub scroll: f32,
    thumb_drag: Option<(f32, f32)>,
    pointer: Option<Pos2>,
    focused: Option<Id>,
    /// The last frame's rows and headers with their labels, on screen,
    /// scrolled: the behaviour tests read them.
    pub layout: Vec<(String, Rect)>,
    /// The last frame's folder and account rows on screen, in DOM order.
    pub cards: Vec<(ItemRef, Rect)>,
    /// The last frame's back row on screen (`[data-back-card]`).
    pub back: Option<Rect>,
    /// The last frame's `.section` wrappers on screen, with their folder
    /// (none for the root's).
    pub section_boxes: Vec<(Option<String>, Rect)>,
    /// `animate:flip`: per row key, the offset it slides from and since when.
    flips: HashMap<String, (f32, f64)>,
    /// Per row key, its laid out and its shown top last frame.
    last_tops: HashMap<String, (f32, f32)>,
    /// Per section key: folded last frame, and its rows while unfolded.
    sections: HashMap<String, (bool, Vec<DisplayItem>)>,
    slides: HashMap<String, Slide>,
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
    /// `cardColor` resolved: `None` for `""`.
    fn card_color(&self, value: &str) -> Option<Rgba>;
}

pub struct ListProps<'a> {
    pub vars: &'a Vars,
    pub locale: Locale,
    /// `accentColor`.
    pub accent: Rgba,
    pub focus_visible: bool,
    /// `color-scheme: dark`, for the focus ring.
    pub dark: bool,
    pub time: &'a TimeContext,
    /// `cardFocus.focusedId`: the row painted with `[data-kb-focus]`.
    pub kb_focus: Option<&'a str>,
    /// Scroll the focused row into view (`syncDom`).
    pub kb_reveal: bool,
    /// `dragOverFolderId`: the folder row painted `.drag-over`.
    pub drag_over_folder: Option<&'a str>,
    /// `dragOverBack`.
    pub drag_over_back: bool,
    /// `dragItem` is set: the flat rows slide to their preview slots.
    pub dragging: bool,
    /// The dragged row's `ghostEl`.
    pub ghost: Option<RowGhost<'a>>,
}

/// `ghostEl` over a list: the dragged row cloned at the drag start.
pub struct RowGhost<'a> {
    pub item: &'a ItemRef,
    /// `sourceEl.getBoundingClientRect()` when the drag started.
    pub rect: Rect,
    /// How far the pointer went since.
    pub offset: egui::Vec2,
    /// The clone's own mount.
    pub id: Id,
}

/// What the list reports.
#[derive(Clone, Debug, PartialEq)]
pub enum ListEvent {
    /// The back row: `onGoBack`.
    Back,
    /// A folder row or a header's double click: `onNavigate`.
    Navigate(String),
    FolderMenu(FolderInfo, Pos2),
    /// A row click, before the selection: `onAccountActivate`.
    Activate(String),
    /// A double click, the preview's button, or a click in bulk edit.
    Switch(String),
    AccountMenu(Box<PlatformAccount>, Pos2),
    /// A header click: `onToggleCollapse`.
    ToggleSection(String),
}

fn name_style(v: &Vars) -> TextStyle {
    TextStyle::new(12.0, 500)
        .line_height(LineHeight::Number(v.line_height))
        .nowrap(true)
        .color(v.fg.to_color32())
}

/// `.meta-stack` lines: `line-height: 1.2`.
fn meta_style(size: f32, weight: u16, color: Rgba) -> TextStyle {
    TextStyle::new(size, weight)
        .line_height(LineHeight::Number(1.2))
        .nowrap(true)
        .color(color.to_color32())
}

/// `color-mix(in srgb, var(--fg-subtle) 40%, var(--fg) 60%)`.
fn last_login_color(v: &Vars) -> Rgba {
    v.fg_subtle.mix(0.4, v.fg)
}

fn has_username(a: &AccountCardItem) -> bool {
    a.show_username && !a.account.username.trim().is_empty()
}

/// `.info`'s height: the name, then the meta lines.
fn info_height(v: &Vars, a: Option<&AccountCardItem>) -> f32 {
    let name = 12.0 * v.line_height;
    let lines = a.map_or(0, |a| has_username(a) as usize + a.show_last_login as usize);
    name + lines as f32 * 12.0
}

/// A row's border box height.
fn row_height(v: &Vars, a: Option<&AccountCardItem>, ppp: f32) -> f32 {
    let bw = snap::border_width(v.border_px(), ppp);
    ICON.max(info_height(v, a)) + 2.0 * ROW_PAD_Y + 2.0 * bw
}

fn ceil_unit(w: f32, ppp: f32) -> f32 {
    (w * ppp * 64.0).ceil() / 64.0 / ppp
}

fn header_label(locale: Locale, h: &SectionHeaderItem) -> String {
    match &h.label {
        SectionLabel::Folder(f) => f.name.clone(),
        SectionLabel::Root => translate(locale, "list.rootSection"),
    }
}

fn key_of(item: &DisplayItem) -> String {
    match item {
        DisplayItem::Back => "list/back".into(),
        DisplayItem::SectionHeader(h) => format!("list/h/{}", h.collapse_key),
        DisplayItem::Folder(f) => format!("list/f/{}", f.folder.id),
        DisplayItem::Account(a) => format!("list/a/{}", a.account.id),
    }
}

/// Draws the list layout over `rect` (the `.list-wrapper` box).
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
    let mut events = Vec::new();
    let account = |id: &str| {
        items.iter().find_map(|item| match item {
            DisplayItem::Account(a) if a.account.id == id => Some(&**a),
            _ => None,
        })
    };
    // A stale selection goes: bulk edit took the clicks, or the account
    // left the list.
    let bulk = items
        .iter()
        .any(|item| matches!(item, DisplayItem::Account(a) if a.single_click_switch));
    if bulk || state.selected.as_deref().and_then(account).is_none() {
        state.selected = None;
    }

    // `.list-layout`: `--border` shows through the 1 px gap.
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

    // The column: every box 2 px apart. In the sections layout each
    // `.section` is one box: its header, 2 px, its rows (`.section-rows`,
    // there unless collapsed, possibly empty, sliding 220 ms when the
    // section folds or unfolds). The root section is the last one and is
    // there even with nothing in it (no header then).
    let box_h = |item: &DisplayItem| match item {
        // `section_header::height`: the folder icon (16) or the count pill
        // (15), plus 4 px above and below.
        DisplayItem::SectionHeader(h) if matches!(h.label, SectionLabel::Folder(_)) => 24.0,
        DisplayItem::SectionHeader(_) => 23.0,
        DisplayItem::Account(a) => row_height(v, Some(a), ppp),
        _ => row_height(v, None, ppp),
    };
    let mut blocks: Vec<Block> = Vec::new();
    for item in items {
        match (item, blocks.last_mut()) {
            (DisplayItem::SectionHeader(h), _) => blocks.push(Block::Section {
                key: h.collapse_key.clone(),
                header: Some(item),
                collapsed: h.collapsed,
                rows: Vec::new(),
            }),
            (_, Some(Block::Section { rows, .. })) => rows.push(item),
            _ => blocks.push(Block::Row(item)),
        }
    }
    let root_header = items.iter().any(|item| {
        matches!(item, DisplayItem::SectionHeader(h) if matches!(h.label, SectionLabel::Root))
    });
    if blocks.iter().any(|b| matches!(b, Block::Section { .. })) && !root_header {
        blocks.push(Block::Section {
            key: String::new(),
            header: None,
            collapsed: false,
            rows: Vec::new(),
        });
    }

    // Folding and unfolding start slides.
    let now = ctx.input(|i| i.time);
    let mut slides = std::mem::take(&mut state.slides);
    let mut seen = HashMap::new();
    for block in &blocks {
        let Block::Section {
            key,
            header: Some(_),
            collapsed,
            rows,
        } = block
        else {
            continue;
        };
        let was = state.sections.get(key);
        if anim::motion(&ctx) == Motion::Full {
            match was {
                Some((false, cached)) if *collapsed => {
                    let rows = cached.clone();
                    slides.insert(
                        key.clone(),
                        Slide {
                            start: now,
                            intro: false,
                            rows,
                        },
                    );
                }
                Some((true, _)) if !*collapsed => {
                    let rows = Vec::new();
                    slides.insert(
                        key.clone(),
                        Slide {
                            start: now,
                            intro: true,
                            rows,
                        },
                    );
                }
                _ => {}
            }
        }
        let cached = if *collapsed {
            Vec::new()
        } else {
            rows.iter().map(|item| (**item).clone()).collect()
        };
        seen.insert(key.clone(), (*collapsed, cached));
    }
    state.sections = seen;
    slides.retain(|key, slide| now - slide.start < SLIDE && state.sections.contains_key(key));
    if !slides.is_empty() {
        ctx.request_repaint();
    }

    // Every box, in content coordinates.
    let mut places: Vec<Place> = Vec::new();
    let mut wrappers: Vec<(Option<String>, f32, f32)> = Vec::new();
    let mut y = PAD;
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            y += GAP;
        }
        match block {
            Block::Row(item) => {
                let h = box_h(item);
                places.push(Place::new(item, y, h));
                y += h;
            }
            Block::Section {
                key,
                header,
                collapsed,
                rows,
            } => {
                let folder = match header {
                    Some(DisplayItem::SectionHeader(h)) => match &h.label {
                        SectionLabel::Folder(f) => Some(f.id.clone()),
                        SectionLabel::Root => None,
                    },
                    _ => None,
                };
                let wrapper = wrappers.len();
                wrappers.push((folder, y, y));
                if let Some(item) = header {
                    let h = box_h(item);
                    places.push(Place::new(item, y, h));
                    y += h;
                }
                let slide = slides.get(key);
                let leaving: Vec<&DisplayItem>;
                let group: &[&DisplayItem] = match slide {
                    Some(s) if !s.intro => {
                        leaving = s.rows.iter().collect();
                        &leaving
                    }
                    _ if *collapsed => {
                        wrappers[wrapper].2 = y;
                        continue;
                    }
                    _ => rows,
                };
                if header.is_some() {
                    y += GAP;
                }
                let factor = slide.map_or(1.0, |s| {
                    let e = cubic_out(((now - s.start) / SLIDE).clamp(0.0, 1.0)) as f32;
                    if s.intro {
                        e
                    } else {
                        1.0 - e
                    }
                });
                let top = y;
                let mut full = 0.0;
                for (k, item) in group.iter().enumerate() {
                    if k > 0 {
                        full += GAP;
                    }
                    let h = box_h(item);
                    let mut place = Place::new(item, top + full, h);
                    if slide.is_some() {
                        // `overflow: hidden` on the sliding height, and
                        // `opacity: min(t * 20, 1)`.
                        place.clip = Some((top, top + full_h(group, &box_h) * factor));
                        place.opacity = (factor * 20.0).min(1.0);
                    }
                    places.push(place);
                    full += h;
                }
                y += full * factor;
                wrappers[wrapper].2 = y;
            }
        }
    }

    // `animate:flip` in the flat layout, 200 ms while a row is dragged and
    // at most 100 render: a row the preview moves slides from where it
    // showed. The dragged row jumps.
    if wrappers.is_empty() {
        let rendered = places
            .iter()
            .filter(|p| matches!(p.item, DisplayItem::Folder(_) | DisplayItem::Account(_)))
            .count();
        let flip = props.dragging && rendered <= 100;
        let mut tops = HashMap::new();
        for place in &mut places {
            let dragged = match place.item {
                DisplayItem::Folder(f) => f.dragged,
                DisplayItem::Account(a) => a.dragged,
                _ => continue,
            };
            let key = key_of(place.item);
            let layout = place.top;
            if let Some((was, shown)) = state.last_tops.get(&key).copied() {
                if was != layout {
                    if flip && !dragged {
                        state.flips.insert(key.clone(), (shown - layout, now));
                    } else {
                        state.flips.remove(&key);
                    }
                }
            }
            if let Some((from, start)) = state.flips.get(&key).copied() {
                let t = ((now - start) / FLIP).clamp(0.0, 1.0);
                if t >= 1.0 {
                    state.flips.remove(&key);
                } else {
                    place.top += from * (1.0 - cubic_out(t) as f32);
                    ctx.request_repaint();
                }
            }
            tops.insert(key, (layout, place.top));
        }
        state.last_tops = tops;
    } else {
        state.flips.clear();
        state.last_tops.clear();
    }
    let content_h = y + PAD;
    let max_scroll = (content_h - panel.height()).max(0.0);
    let scrolls = max_scroll > 0.0;
    let row_w = panel.width() - 2.0 * PAD - if scrolls { SCROLLBAR } else { 0.0 };

    // The wheel over the panel, applied at once like Chromium's.
    let target = panel.intersect(ui.clip_rect());
    let dy: f32 = ctx.input(|i| {
        let mut at = state.pointer;
        let mut dy = 0.0;
        for event in &i.events {
            match event {
                Event::PointerMoved(p) => at = Some(*p),
                Event::PointerGone => at = None,
                Event::MouseWheel { unit, delta, .. } if at.is_some_and(|p| target.contains(p)) => {
                    dy += match unit {
                        egui::MouseWheelUnit::Point => delta.y,
                        egui::MouseWheelUnit::Line => delta.y * 100.0,
                        egui::MouseWheelUnit::Page => delta.y * panel.height(),
                    };
                }
                _ => {}
            }
        }
        dy
    });
    state.pointer = ctx.input(|i| i.pointer.latest_pos());
    let scroll_before = state.scroll;
    state.scroll = (state.scroll - dy).clamp(0.0, max_scroll).round();
    // `syncDom`: the row the arrows moved to scrolls into view.
    let kb_target = places
        .iter()
        .find(|place| props.kb_focus.is_some() && item_id(place.item) == props.kb_focus);
    if let Some(place) = kb_target.filter(|_| props.kb_reveal) {
        let (top, bottom) = (place.top, place.top + place.h);
        state.scroll = super::controls::scroll::nearest(state.scroll, panel.height(), top, bottom)
            .clamp(0.0, max_scroll)
            .round();
    }

    let clip = panel.intersect(ui.clip_rect());
    let focused_now = ctx.memory(|m| m.focused());
    let mut focused_box = None;
    state.layout.clear();
    state.cards.clear();
    state.back = None;
    let scrolled = panel.min.to_vec2() - vec2(0.0, state.scroll);
    state.section_boxes = wrappers
        .into_iter()
        .map(|(folder, top, bottom)| {
            let local = Rect::from_x_y_ranges(PAD..=PAD + row_w, top..=bottom);
            (folder, local.translate(scrolled))
        })
        .collect();
    let mut ghost = None;
    for place in &places {
        let (item, h) = (place.item, place.h);
        let local = Rect::from_min_size(pos2(PAD, place.top), vec2(row_w, h));
        let row = local.translate(scrolled);
        let clip = match place.clip {
            Some((top, bottom)) => {
                let group = Rect::from_x_y_ranges(panel.x_range(), top..=bottom.max(top));
                clip.intersect(group.translate(scrolled))
            }
            None => clip,
        };
        let key = key_of(item);
        let id = host.mount(&key);
        if focused_now == Some(id) {
            focused_box = Some(local);
        }
        let visible = row.intersects(clip) && clip.height() > 0.0;
        let label = match item {
            DisplayItem::Back => "Back".to_string(),
            DisplayItem::SectionHeader(h) => header_label(props.locale, h),
            DisplayItem::Folder(f) => f.folder.name.clone(),
            DisplayItem::Account(a) => a.name.clone(),
        };
        state.layout.push((label, row));
        let kb_focus = props.kb_focus.is_some() && item_id(item) == props.kb_focus;
        match item {
            DisplayItem::Folder(f) => state.cards.push((ItemRef::folder(&f.folder.id), row)),
            DisplayItem::Account(a) => state.cards.push((ItemRef::account(&a.account.id), row)),
            _ => {}
        }
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(row));
        child.set_clip_rect(clip);
        child.multiply_opacity(place.opacity);
        let (item_ref, dragged) = match item {
            DisplayItem::Folder(f) => (Some(ItemRef::folder(&f.folder.id)), f.dragged),
            DisplayItem::Account(a) => (Some(ItemRef::account(&a.account.id)), a.dragged),
            _ => (None, false),
        };
        // `.row.dragging { opacity: .4 }`: one layer, as the cards'.
        let group = dragged && visible && !cfg!(feature = "glow");
        if group {
            super::folder_card::drag_group(&child, row);
        } else if dragged {
            child.multiply_opacity(0.4);
        }
        if let (Some(g), Some(item_ref)) = (&props.ghost, &item_ref) {
            if g.item == item_ref {
                ghost = Some((item, h, id, key.clone(), kb_focus));
            }
        }
        match item {
            DisplayItem::SectionHeader(h) => {
                let label = header_label(props.locale, h);
                let folder = match &h.label {
                    SectionLabel::Folder(f) => Some(f.id.as_str()),
                    SectionLabel::Root => None,
                };
                let header = SectionHeaderProps {
                    vars: v,
                    id,
                    folder,
                    label: &label,
                    count: h.count,
                    card_color: host.card_color(&h.card_color),
                    accent: props.accent,
                    collapsed: h.collapsed,
                    focus_visible: props.focus_visible,
                };
                for event in section_header::section_header(&mut child, row, &header) {
                    match event {
                        SectionHeaderEvent::Toggle => {
                            events.push(ListEvent::ToggleSection(h.collapse_key.clone()))
                        }
                        SectionHeaderEvent::Navigate(id) => events.push(ListEvent::Navigate(id)),
                        SectionHeaderEvent::ContextMenu(at) => {
                            if let SectionLabel::Folder(f) = &h.label {
                                events.push(ListEvent::FolderMenu(f.clone(), at));
                            }
                        }
                    }
                }
            }
            DisplayItem::Back => {
                state.back = Some(row);
                let label = translate(props.locale, "common.back");
                let look = RowLook {
                    drag_over: props.drag_over_back,
                    ..RowLook::default()
                };
                let r = list_row(&mut child, row, id, &label, look, props, visible);
                if visible {
                    icon_box(&child, &r, icons::BACK, v);
                    plain_name(&child, &r, &label, v);
                }
                if r.clicked {
                    events.push(ListEvent::Back);
                }
            }
            DisplayItem::Folder(f) => {
                let look = RowLook {
                    drag_over: props.drag_over_folder == Some(f.folder.id.as_str()),
                    kb_focus,
                    ..RowLook::default()
                };
                let r = list_row(&mut child, row, id, &f.folder.name, look, props, visible);
                if visible {
                    icon_box(&child, &r, icons::FOLDER, v);
                    plain_name(&child, &r, &f.folder.name, v);
                }
                if r.clicked {
                    events.push(ListEvent::Navigate(f.folder.id.clone()));
                }
                if let Some(at) = r.menu {
                    events.push(ListEvent::FolderMenu(f.folder.clone(), at));
                }
            }
            DisplayItem::Account(a) => {
                let look = account_look(a, state, kb_focus, host);
                let r = list_row(&mut child, row, id, &a.name, look, props, visible);
                let px = (ICON * ppp).round() as u32;
                let mut content = host.avatar(&key, a, px, visible);
                if let AvatarContent::Picture(picture) = &mut content {
                    if a.switching && visible {
                        picture.blurred = host.blurred(a, px, 3.0 * ppp);
                    }
                }
                if visible {
                    avatar_rings(&child, r.icon, look, v);
                    paint_avatar(&child, id, r.icon, &content, a, v, AvatarSize::ROW);
                    account_info(&child, &r, a, look, props);
                } else {
                    let revealed = !matches!(content, AvatarContent::Decoding);
                    avatar_fade_in::avatar_opacity(&ctx, id.with("avatar"), revealed, v);
                }
                if r.clicked {
                    events.push(ListEvent::Activate(a.account.id.clone()));
                    if a.single_click_switch {
                        if !a.pending_setup {
                            events.push(ListEvent::Switch(a.account.id.clone()));
                        }
                    } else {
                        state.selected = Some(a.account.id.clone());
                    }
                }
                if r.double_clicked && !a.pending_setup && !a.single_click_switch {
                    events.push(ListEvent::Switch(a.account.id.clone()));
                }
                if let Some(at) = r.menu {
                    events.push(ListEvent::AccountMenu(Box::new(a.account.clone()), at));
                }
            }
        }
        if group {
            crate::ui::backdrop_filter::Mark::End.paint(child.painter(), row);
        }
    }

    if let (Some(g), Some((item, h, source, key, kb_focus))) = (&props.ghost, ghost) {
        ghost_row(
            &ctx,
            g,
            item,
            h,
            (source, &key),
            kb_focus,
            state,
            props,
            host,
        );
    }
    drop(places);
    state.slides = slides;

    // Focus scrolls its row into view (`nearest`).
    if focused_now != state.focused {
        state.focused = focused_now;
        if let Some(r) = focused_box {
            if r.top() < state.scroll {
                state.scroll = r.top();
            } else if r.bottom() > state.scroll + panel.height() {
                state.scroll = r.bottom() - panel.height();
            }
        }
    }
    if scrolls {
        scrollbar(ui, panel, state, v, content_h, max_scroll);
    } else {
        state.thumb_drag = None;
    }
    state.scroll = state.scroll.clamp(0.0, max_scroll).round();
    if state.scroll != scroll_before {
        ctx.request_repaint();
    }

    // The preview of the selected account, or the hint.
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(preview));
    child.set_clip_rect(preview.intersect(ui.clip_rect()));
    match state.selected.as_deref().and_then(account) {
        Some(a) => {
            let key = format!("list/preview/{}", a.account.id);
            let id = host.mount(&key);
            let px = (120.0 * ppp).round() as u32;
            let content = host.avatar(&key, a, px, true);
            let color = host.card_color(&a.card_color);
            let pressed = preview_panel(&mut child, preview, id, a, &content, color, props);
            if pressed && !a.pending_setup {
                events.push(ListEvent::Switch(a.account.id.clone()));
            }
        }
        None => {
            // `.no-selection`: padding 16, the text centred both ways.
            let style = TextStyle::new(11.0, 400)
                .line_height(LineHeight::Number(v.line_height))
                .align(TextAlign::Center)
                .color(v.fg_muted.to_color32());
            let inner = preview.shrink(16.0);
            let text = translate(props.locale, "list.selectAccountPreview");
            let probe = Rect::from_min_size(inner.min, vec2(inner.width(), 0.0));
            let h =
                text::layout(&ctx, &text, &style, probe).map_or(11.0 * v.line_height, |l| l.height);
            let top = inner.top() + snap::centre(inner.height(), h, ppp);
            text::paint(
                child.painter(),
                Rect::from_min_size(pos2(inner.left(), top), vec2(inner.width(), h)),
                &text,
                &style,
            );
        }
    }
    events
}

/// A box of the panel's column.
enum Block<'a> {
    /// A row of the flat layout.
    Row(&'a DisplayItem),
    /// A `.section`: its header (none for an empty root), its rows.
    Section {
        key: String,
        header: Option<&'a DisplayItem>,
        collapsed: bool,
        rows: Vec<&'a DisplayItem>,
    },
}

/// A header or row placed in the column, in content coordinates.
struct Place<'a> {
    item: &'a DisplayItem,
    top: f32,
    h: f32,
    /// The sliding `.section-rows` box it shows through, top to bottom.
    clip: Option<(f32, f32)>,
    opacity: f32,
}

impl<'a> Place<'a> {
    fn new(item: &'a DisplayItem, top: f32, h: f32) -> Self {
        Place {
            item,
            top,
            h,
            clip: None,
            opacity: 1.0,
        }
    }
}

/// The rows of `group` stacked 2 px apart.
fn full_h(group: &[&DisplayItem], box_h: &dyn Fn(&DisplayItem) -> f32) -> f32 {
    let rows: f32 = group.iter().map(|item| box_h(item)).sum();
    rows + GAP * group.len().saturating_sub(1) as f32
}

/// A section's rows folding or unfolding: `transition:slide`, 220 ms
/// `cubicOut`.
struct Slide {
    start: f64,
    intro: bool,
    /// The rows of a folding section, drawn until it is gone.
    rows: Vec<DisplayItem>,
}

fn cubic_out(t: f64) -> f64 {
    let f = t - 1.0;
    f * f * f + 1.0
}

/// The card focus id of a folder or account row.
fn item_id(item: &DisplayItem) -> Option<&str> {
    match item {
        DisplayItem::Folder(f) => Some(&f.folder.id),
        DisplayItem::Account(a) => Some(&a.account.id),
        _ => None,
    }
}

/// The states a row draws.
#[derive(Clone, Copy, Default)]
struct RowLook {
    selected: bool,
    active: bool,
    custom: Option<Rgba>,
    red: bool,
    orange: bool,
    /// `.drag-over`.
    drag_over: bool,
    /// `[data-kb-focus]`.
    kb_focus: bool,
    /// The drag's clone: never hovered, its own shadow.
    ghost: bool,
}

/// An account row's states.
fn account_look(
    a: &AccountCardItem,
    state: &ListState,
    kb_focus: bool,
    host: &dyn ListHost,
) -> RowLook {
    let warning = a.warning.as_ref();
    RowLook {
        selected: state.selected.as_deref() == Some(a.account.id.as_str()),
        active: a.active,
        custom: host.card_color(&a.card_color),
        red: warning.is_some_and(|w| w.list_has_red),
        orange: warning.is_some_and(|w| w.list_has_orange),
        kb_focus,
        ..RowLook::default()
    }
}

/// `ghostEl`: the source row cloned at the drag start, `position: fixed` at
/// the pointer, `opacity: 0.85`, `transform: scale(1.05)` and its own
/// shadow, over everything. A `.row` has no entrance to replay.
#[allow(clippy::too_many_arguments)]
fn ghost_row(
    ctx: &egui::Context,
    g: &RowGhost,
    item: &DisplayItem,
    h: f32,
    (source, key): (Id, &str),
    kb_focus: bool,
    state: &ListState,
    props: &ListProps,
    host: &mut dyn ListHost,
) {
    let v = props.vars;
    let ppp = ctx.pixels_per_point();
    let r = Rect::from_min_size(g.rect.min + g.offset, vec2(g.rect.width(), h));
    super::drag_ghost(ctx, r, 1.05, 0.85, |gui| match item {
        DisplayItem::Folder(f) => {
            let look = RowLook {
                kb_focus,
                ghost: true,
                ..RowLook::default()
            };
            let row = list_row(gui, r, g.id, &f.folder.name, look, props, true);
            icon_box(gui, &row, icons::FOLDER, v);
            plain_name(gui, &row, &f.folder.name, v);
        }
        DisplayItem::Account(a) => {
            let look = RowLook {
                ghost: true,
                ..account_look(a, state, kb_focus, host)
            };
            let row = list_row(gui, r, g.id, &a.name, look, props, true);
            let px = (ICON * ppp).round() as u32;
            let content = host.avatar(key, a, px, true);
            avatar_rings(gui, row.icon, look, v);
            paint_avatar(gui, source, row.icon, &content, a, v, AvatarSize::ROW);
            account_info(gui, &row, a, look, props);
        }
        _ => {}
    });
}

/// A row drawn: where its parts go and what it got.
struct Row {
    /// The content box (inside the border and the padding).
    content: Rect,
    /// `.icon` or `.avatar`.
    icon: Rect,
    clicked: bool,
    double_clicked: bool,
    menu: Option<Pos2>,
}

/// The highest specificity rule that applies, the later one on a tie.
fn cascade(rules: &[(u8, bool, Rgba)]) -> Rgba {
    rules
        .iter()
        .enumerate()
        .filter(|(_, (_, on, _))| *on)
        .max_by_key(|(order, (spec, _, _))| (*spec, *order))
        .map_or(Rgba::TRANSPARENT, |(_, (_, _, color))| *color)
}

/// The `.row` box: its background, border and focus ring, and its events.
fn list_row(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    label: &str,
    look: RowLook,
    props: &ListProps,
    visible: bool,
) -> Row {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let painted = snap::rect(rect, ppp);
    let response = ui
        .interact(painted, id, Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    ctx.accesskit_node_builder(id, |node| node.set_role(Role::Button));
    let RowLook {
        selected,
        active,
        custom,
        red,
        orange,
        drag_over,
        kb_focus,
        ghost,
    } = look;
    let hovered = super::css_hovered(&response) && !ghost;
    let c = custom.is_some();
    let cc = custom.unwrap_or(Rgba::TRANSPARENT);
    let background = cascade(&[
        (2, hovered, v.bg_card_hover),
        (2, c, cc.mix(0.18, v.bg_card)),
        (3, c && hovered, cc.mix(0.24, v.bg_card_hover)),
        (2, selected, v.bg_card),
        (3, selected && c, cc.mix(0.22, v.bg_card)),
        (2, active, v.bg_card_hover),
        (3, active && c, cc.mix(0.24, v.bg_card_hover)),
        (2, drag_over, props.accent.mix(0.10, Rgba::TRANSPARENT)),
    ]);
    let border = cascade(&[
        (2, selected, Rgba::new(255.0, 255.0, 255.0, 0.1)),
        (2, drag_over, props.accent),
        (2, red, RED.alpha(0.45)),
        (3, orange && !red, YELLOW.alpha(0.45)),
        (3, c && red, RED),
        (4, c && orange && !red, YELLOW),
    ]);
    let background = anim::transition(&ctx, id.with("row-bg"), background, &T_ROW);
    let bw = snap::border_width(v.border_px(), ppp);
    if visible {
        BoxStyle {
            background,
            radius: Corners::all(6.0),
            border: Border::all(bw, border),
            shadows: if ghost {
                vec![super::account_card::GHOST_SHADOW]
            } else {
                Vec::new()
            },
            ..BoxStyle::default()
        }
        .paint(ui.painter(), painted);
    }
    let focused = response.has_focus();
    if visible && focused && props.focus_visible {
        auto_ring(ui.painter(), painted, 6.0, props.dark, ppp);
    } else if visible && kb_focus {
        super::focus::card_ring(ui, painted, 6.0, v);
    }
    let keys = focused && ctx.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Space));
    let inset = vec2(bw + ROW_PAD_X, bw + ROW_PAD_Y);
    let content = Rect::from_min_max(rect.min + inset, rect.max - inset);
    let icon = Rect::from_min_size(
        pos2(
            content.left(),
            content.top() + snap::centre(content.height(), ICON, ppp),
        ),
        vec2(ICON, ICON),
    );
    Row {
        content,
        icon,
        clicked: super::clicked(&response) || keys,
        double_clicked: response.double_clicked(),
        menu: response
            .secondary_clicked()
            .then(|| super::context_click_pos(ui).unwrap_or(rect.center())),
    }
}

/// `.icon`: 32 px of `--bg-muted`, radius 4, its 18 px icon centred.
fn icon_box(ui: &Ui, r: &Row, svg: &'static [u8], v: &Vars) {
    let ppp = ui.ctx().pixels_per_point();
    BoxStyle {
        background: v.bg_muted,
        radius: Corners::all(4.0),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), snap::rect(r.icon, ppp));
    let inner = Rect::from_min_size(r.icon.min + vec2(7.0, 7.0), vec2(18.0, 18.0));
    icon::paint(ui.painter(), svg, inner, v.fg_muted, 1.0);
}

/// The back and folder rows' `.info`: the name alone.
fn plain_name(ui: &Ui, r: &Row, label: &str, v: &Vars) {
    let ctx = ui.ctx();
    let ppp = ctx.pixels_per_point();
    let style = name_style(v);
    let left = r.icon.right() + ROW_GAP;
    let room = (r.content.right() - left).max(0.0);
    let w = ceil_unit(text::measure(ctx, label, &style).unwrap_or(0.0), ppp).min(room);
    let h = 12.0 * v.line_height;
    let top = r.content.top() + snap::centre(r.content.height(), h, ppp);
    text::paint(
        ui.painter(),
        Rect::from_min_size(pos2(left, top), vec2(w, h)),
        label,
        &style,
    );
}

/// The account row's `.info`, then its `.warnings` (`margin-left: auto`)
/// and its `.active-badge` (`margin-left: 6px`), 10 px apart.
fn account_info(ui: &Ui, r: &Row, a: &AccountCardItem, look: RowLook, props: &ListProps) {
    let ctx = ui.ctx();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let painter = ui.painter();
    let content = r.content;

    let dots: Vec<WarningTone> = [
        (look.red, WarningTone::Red),
        (look.orange, WarningTone::Orange),
    ]
    .into_iter()
    .filter_map(|(on, tone)| on.then_some(tone))
    .collect();
    let dots_w = match dots.len() {
        0 => 0.0,
        n => n as f32 * 14.0 + (n - 1) as f32 * 4.0,
    };
    let badge_style = TextStyle::new(9.0, 600)
        .line_height(LineHeight::Number(v.line_height))
        .letter_spacing_px(0.5)
        .transform(TextTransform::Uppercase)
        .color(Rgba::new(255.0, 255.0, 255.0, 0.5).to_color32());
    let badge_text = translate(props.locale, "common.active");
    let badge_w = if a.active {
        ceil_unit(
            text::measure(ctx, &badge_text, &badge_style).unwrap_or(0.0),
            ppp,
        )
    } else {
        0.0
    };
    let mut fixed = 0.0;
    if !dots.is_empty() {
        fixed += ROW_GAP + dots_w;
    }
    if a.active {
        fixed += ROW_GAP + 6.0 + badge_w;
    }
    let left = r.icon.right() + ROW_GAP;
    let room = (content.right() - left - fixed).max(0.0);

    let name = name_style(v);
    let username = meta_style(10.0, 400, v.fg_muted);
    let meta = meta_style(10.0, 500, last_login_color(v));
    let show_username = has_username(a);
    let last_login = a.show_last_login.then(|| {
        format_relative_time_compact(a.last_login_at_sec, props.time, a.last_login_unknown_key)
    });
    let width = |s: &str, style: &TextStyle| text::measure(ctx, s, style).unwrap_or(0.0);
    let mut max_content = width(&a.name, &name);
    if show_username {
        max_content = max_content.max(width(&a.account.username, &username));
    }
    if let Some(label) = &last_login {
        max_content = max_content.max(width(label, &meta));
    }
    let info_w = ceil_unit(max_content, ppp).min(room);
    let info_h = info_height(v, Some(a));
    let name_h = 12.0 * v.line_height;
    let mut top = content.top() + snap::centre(content.height(), info_h, ppp);
    text::paint(
        painter,
        Rect::from_min_size(pos2(left, top), vec2(info_w, name_h)),
        &a.name,
        &name,
    );
    top += name_h;
    if show_username {
        text::paint(
            painter,
            Rect::from_min_size(pos2(left, top), vec2(info_w, 12.0)),
            &a.account.username,
            &username,
        );
        top += 12.0;
    }
    if let Some(label) = &last_login {
        text::paint(
            painter,
            Rect::from_min_size(pos2(left, top), vec2(info_w, 12.0)),
            label,
            &meta,
        );
    }

    // Right to left: the badge, then the dots.
    let mut right = content.right();
    if a.active {
        let x = if dots.is_empty() {
            left + info_w + ROW_GAP + 6.0
        } else {
            right - badge_w
        };
        let h = 9.0 * v.line_height;
        let y = content.top() + snap::centre(content.height(), h, ppp);
        text::paint(
            painter,
            Rect::from_min_size(pos2(x, y), vec2(badge_w, h)),
            &badge_text,
            &badge_style,
        );
        right = x - 6.0 - ROW_GAP;
    }
    let bw = snap::border_width(1.0, ppp);
    let mut x = right - dots_w;
    for tone in dots {
        let (base, glyph) = match tone {
            WarningTone::Red => (RED, Rgba::hex(0xfca5a5, 1.0)),
            WarningTone::Orange => (ORANGE, Rgba::hex(0xfdba74, 1.0)),
        };
        let dot = Rect::from_min_size(
            pos2(x, content.top() + snap::centre(content.height(), 14.0, ppp)),
            vec2(14.0, 14.0),
        );
        BoxStyle {
            background: base.alpha(0.24),
            radius: Corners::all(7.0),
            border: Border::all(bw, base.alpha(0.55)),
            ..BoxStyle::default()
        }
        .paint(painter, snap::rect(dot, ppp));
        let style = TextStyle::new(10.0, 800)
            .line_height(LineHeight::Number(1.0))
            .color(glyph.to_color32());
        let inner = dot.shrink(bw);
        let w = ceil_unit(text::measure(ctx, "!", &style).unwrap_or(0.0), ppp);
        let at = pos2(
            inner.left() + snap::centre(inner.width(), w, ppp),
            inner.top() + snap::centre(inner.height(), 10.0, ppp),
        );
        text::paint(painter, Rect::from_min_size(at, vec2(w, 10.0)), "!", &style);
        x += 18.0;
    }
}

/// `.avatar.ban-red { outline: 2px }` and `.ban-orange { box-shadow }`.
fn avatar_rings(ui: &Ui, rect: Rect, look: RowLook, v: &Vars) {
    let ppp = ui.ctx().pixels_per_point();
    let snapped = snap::rect(rect, ppp);
    let shadow = match (look.red, look.orange) {
        (true, true) => Some((4.0, YELLOW.alpha(0.45))),
        (false, true) => Some((2.0, YELLOW.alpha(0.7))),
        _ => None,
    };
    if let Some((spread, color)) = shadow {
        BoxStyle {
            radius: Corners::all(v.avatar_radius),
            shadows: vec![BoxShadow {
                offset: egui::Vec2::ZERO,
                blur: 0.0,
                spread,
                color,
                inset: false,
            }],
            ..BoxStyle::default()
        }
        .paint(ui.painter(), snapped);
    }
    if look.red {
        let w = snap::border_width(2.0, ppp);
        BoxStyle {
            radius: Corners::all(v.avatar_radius + w),
            border: Border::all(w, RED.alpha(0.65)),
            ..BoxStyle::default()
        }
        .paint(ui.painter(), snapped.expand(w));
    }
}

/// Blink's flex shrink of the preview column, for its two items that can
/// shrink (bases `a` and `b`, minimum 0): each gives way in proportion to its
/// base, `LayoutUnit::FromFloatRound` on the device length.
fn flex_shrink(overflow: f32, a: f32, b: f32, ppp: f32) -> (f32, f32) {
    let sum = a + b;
    if overflow <= 0.0 || sum <= 0.0 {
        return (a, b);
    }
    let give = |base: f32| {
        let shrunk = base - overflow.min(sum) * base / sum;
        snap::layout_unit(shrunk * ppp) / ppp
    };
    (give(a), give(b))
}

/// The row's and the preview's avatar boxes.
#[derive(Clone, Copy)]
struct AvatarSize {
    initials: f32,
    loader: f32,
    loader_border: f32,
}

impl AvatarSize {
    const ROW: AvatarSize = AvatarSize {
        initials: 12.0,
        loader: 15.0,
        loader_border: 2.0,
    };
    const PREVIEW: AvatarSize = AvatarSize {
        initials: 36.0,
        loader: 42.0,
        loader_border: 4.0,
    };
}

/// `avatar-placeholder-in var(--motion-avatar-fade) ease-out
/// var(--motion-avatar-defer) backwards`, from its mount.
fn deferred(ui: &Ui, id: Id, v: &Vars) -> f32 {
    anim::animate(
        ui.ctx(),
        id,
        &Keyframes::from_to(0.0f32, 1.0),
        &Animation::new(v.motion_avatar_fade, Timing::EASE_OUT)
            .with_delay(v.motion_avatar_defer)
            .with_fill(Fill::Backwards),
        1.0,
    )
}

/// A spinner of the given size, turning from its mount.
fn loader(ui: &Ui, id: Id, centre: Pos2, size: AvatarSize, fade: f32, v: &Vars) {
    let turn = anim::animate(
        ui.ctx(),
        id.with("spin"),
        &Keyframes::from_to(0.0f32, 1.0),
        &Animation::new(SPIN, Timing::LINEAR).infinite(),
        0.0,
    );
    let rest = v.fg.mix(0.24, Rgba::TRANSPARENT);
    paint_ring_loader(
        ui.painter(),
        centre,
        size.loader,
        size.loader_border,
        turn,
        rest.alpha(rest.a * fade),
        v.fg.alpha(v.fg.a * fade),
    );
}

/// `object-fit: cover` of a square crop in a box that flex shrink made
/// shorter than wide: Blink pixel-snaps the whole picture, centred on the
/// box, then clips it to the box, so `uv` keeps the band the box shows.
fn box_uv(uv: Rect, rect: Rect, ppp: f32) -> Rect {
    if rect.height() >= rect.width() {
        return uv;
    }
    let image = snap::rect(
        Rect::from_center_size(rect.center(), vec2(rect.width(), rect.width())),
        ppp,
    );
    let shown = snap::rect(rect, ppp);
    let at = |y: f32| uv.top() + uv.height() * (y - image.top()) / image.height();
    Rect::from_min_max(
        pos2(uv.left(), at(shown.top())),
        pos2(uv.right(), at(shown.bottom())),
    )
}

/// `.avatar`: `--bg-muted` under the picture (faded in on decode), or the
/// initials on their gradient, or the loader.
fn paint_avatar(
    ui: &Ui,
    id: Id,
    rect: Rect,
    content: &AvatarContent,
    a: &AccountCardItem,
    v: &Vars,
    size: AvatarSize,
) {
    let ctx = ui.ctx();
    let ppp = ctx.pixels_per_point();
    let painter = ui.painter();
    let snapped = snap::rect(rect, ppp);
    let radius = Corners::all(v.avatar_radius);
    BoxStyle {
        background: v.bg_muted,
        radius,
        ..BoxStyle::default()
    }
    .paint(painter, snapped);
    let revealed = !matches!(content, AvatarContent::Decoding);
    let reveal = avatar_fade_in::avatar_opacity(ctx, id.with("avatar"), revealed, v);
    match content {
        AvatarContent::Picture(picture) => {
            let tint = |alpha: f32, dim: f32| {
                let a8 = (alpha * 255.0).round().clamp(0.0, 255.0) as u8;
                let g = (alpha * dim * 255.0).round().clamp(0.0, 255.0) as u8;
                Color32::from_rgba_premultiplied(g, g, g, a8)
            };
            match (a.switching, picture.blurred) {
                // `filter: blur(3px) brightness(.5)`.
                (true, Some(blurred)) => paint::paint_image(
                    painter,
                    blurred,
                    snapped,
                    box_uv(cover_uv([1, 1], 1.0), rect, ppp),
                    radius,
                    tint(reveal, 0.5),
                ),
                (switching, _) => paint::paint_image(
                    painter,
                    picture.texture,
                    snapped,
                    box_uv(cover_uv(picture.size, 1.0), rect, ppp),
                    radius,
                    tint(reveal, if switching { 0.5 } else { 1.0 }),
                ),
            }
            if a.switching {
                loader(ui, id, rect.center(), size, 1.0, v);
            }
        }
        AvatarContent::Decoding => {}
        AvatarContent::Initials {
            background,
            gradient,
        } => {
            let fade = if a.avatar.pending {
                deferred(ui, id.with("initials-in"), v)
            } else {
                1.0
            };
            BoxStyle {
                background: *background,
                images: gradient.iter().cloned().collect(),
                radius,
                opacity: fade,
                ..BoxStyle::default()
            }
            .paint(painter, snapped);
            let seed = if a.account.display_name.is_empty() {
                &a.account.username
            } else {
                &a.account.display_name
            };
            let initials = accshift_app::actions::avatar_fallback::avatar_initials(seed);
            let style = TextStyle::new(size.initials, 600)
                .line_height(LineHeight::Number(v.line_height))
                .color(v.fg.alpha(v.fg.a * fade).to_color32());
            let w = ceil_unit(text::measure(ctx, &initials, &style).unwrap_or(0.0), ppp);
            let h = size.initials * v.line_height;
            let at = pos2(
                rect.left() + snap::centre(rect.width(), w, ppp),
                rect.top() + snap::centre(rect.height(), h, ppp),
            );
            text::paint(
                painter,
                Rect::from_min_size(at, vec2(w, h)),
                &initials,
                &style,
            );
            if a.switching {
                loader(ui, id, rect.center(), size, 1.0, v);
            }
        }
        AvatarContent::Loading => {
            let fade = deferred(ui, id.with("loader-in"), v);
            loader(ui, id, rect.center(), size, fade, v);
        }
    }
}

/// `PreviewPanel`: true when its switch button was pressed.
fn preview_panel(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    a: &AccountCardItem,
    content: &AvatarContent,
    card_color: Option<Rgba>,
    props: &ListProps,
) -> bool {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let painter = ui.painter().clone();
    if let Some(c) = card_color {
        let css_rgba = |c: Rgba| format!("rgba({}, {}, {}, {})", c.r, c.g, c.b, c.a);
        let gradient = css::gradient(&format!(
            "linear-gradient(180deg, {}, {})",
            css_rgba(c.mix(0.14, v.bg_card)),
            css_rgba(c.mix(0.08, Rgba::TRANSPARENT)),
        ));
        BoxStyle {
            images: gradient.into_iter().collect(),
            radius: Corners::new(0.0, RADIUS, RADIUS, 0.0),
            ..BoxStyle::default()
        }
        .paint(&painter, snap::rect(rect, ppp));
    }

    // The column, centred in the padding box.
    let inner = Rect::from_min_max(rect.min + vec2(16.0, 24.0), rect.max - vec2(16.0, 24.0));
    let width = inner.width();
    let at = |top: f32, h: f32| Rect::from_min_size(pos2(inner.left(), top), vec2(width, h));
    let lay = |s: &str, style: &TextStyle, w: f32| {
        text::layout(
            &ctx,
            s,
            style,
            Rect::from_min_size(Pos2::ZERO, vec2(w, 0.0)),
        )
    };
    let name_style = TextStyle::new(15.0, 600)
        .line_height(LineHeight::Number(v.line_height))
        .align(TextAlign::Center)
        .color(v.fg.to_color32());
    let name_h = lay(&a.name, &name_style, width).map_or(15.0 * v.line_height, |l| l.height);
    let show_username = has_username(a);
    let meta_lines = show_username as usize + a.show_last_login as usize;
    let meta_line = 12.0 * 1.2;
    let username = meta_style(12.0, 400, v.fg_muted).align(TextAlign::Center);
    let meta = meta_style(12.0, 500, last_login_color(v)).align(TextAlign::Center);
    let last_login = a.show_last_login.then(|| {
        format_relative_time_compact(a.last_login_at_sec, props.time, a.last_login_unknown_key)
    });

    // `.ban-badges`: wrapping, 4 px apart, centred.
    let chip_style = TextStyle::new(9.0, 700)
        .line_height(LineHeight::Number(v.line_height))
        .letter_spacing_px(0.3)
        .transform(TextTransform::Uppercase);
    let chips: Vec<(WarningTone, String, f32)> = a
        .warning
        .as_ref()
        .map(|w| {
            w.chips
                .iter()
                .map(|chip| {
                    let text = chip.text.resolve(props.locale);
                    let w = text::measure(&ctx, &text, &chip_style).unwrap_or(0.0);
                    (chip.tone, text, ceil_unit(w, ppp) + 12.0)
                })
                .collect()
        })
        .unwrap_or_default();
    let chip_h = 9.0 * v.line_height + 4.0;
    let mut chip_lines: Vec<Vec<usize>> = Vec::new();
    let mut line_w = 0.0;
    for (i, (_, _, w)) in chips.iter().enumerate() {
        match chip_lines.last_mut() {
            Some(line) if line_w + 4.0 + w <= width => {
                line.push(i);
                line_w += 4.0 + w;
            }
            _ => {
                chip_lines.push(vec![i]);
                line_w = *w;
            }
        }
    }
    let chips_h = match chip_lines.len() {
        0 => 0.0,
        n => n as f32 * chip_h + (n - 1) as f32 * 4.0,
    };

    // `.note-block`: padding 8 10, the label then the note.
    let bw = snap::border_width(v.border_px(), ppp);
    let label_style = TextStyle::new(10.0, 700)
        .line_height(LineHeight::Number(v.line_height))
        .letter_spacing_em(0.06)
        .transform(TextTransform::Uppercase)
        .color(v.fg_muted.to_color32());
    let note_style = TextStyle::new(12.0, 400)
        .line_height(LineHeight::Number(1.35))
        .color(v.fg.to_color32());
    let note_w = width - 20.0 - 2.0 * bw;
    let has_note = !a.note.is_empty();
    let note_text_h = if has_note {
        lay(&a.note, &note_style, note_w).map_or(12.0 * 1.35, |l| l.height)
    } else {
        0.0
    };
    let label_h = 10.0 * v.line_height;
    let note_h = 16.0 + 2.0 * bw + label_h + 4.0 + note_text_h;

    let status_h = 11.0 * v.line_height;
    let button_h = 16.0 + 12.0 * v.line_height;
    let button = !a.pending_setup;
    let mut total = 120.0 + 16.0 + name_h;
    if meta_lines > 0 {
        total += 4.0 + meta_lines as f32 * meta_line;
    }
    if !chips.is_empty() {
        total += 6.0 + chips_h;
    }
    if has_note {
        total += 10.0 + note_h;
    }
    if a.active {
        total += 16.0 + status_h;
    }
    if button {
        total += 16.0 + button_h;
    }
    // Past the column's height every item shrinks in proportion to its
    // size, but only `.avatar-large` and a nowrap `.meta-stack` can: their
    // `overflow: hidden` drops `min-height: auto` to 0.
    let meta_h = meta_lines as f32 * meta_line;
    let meta_shrinks = !a.pending_setup;
    let (avatar_h, meta_box_h) = flex_shrink(
        total - inner.height(),
        120.0,
        if meta_shrinks { meta_h } else { 0.0 },
        ppp,
    );
    let meta_box_h = if meta_shrinks { meta_box_h } else { meta_h };
    total -= (120.0 - avatar_h) + (meta_h - meta_box_h);
    let mut y = inner.top() + snap::centre(inner.height(), total, ppp);
    let centre_x = |w: f32| inner.left() + snap::centre(width, w, ppp);

    let avatar = Rect::from_min_size(pos2(centre_x(120.0), y), vec2(120.0, avatar_h));
    paint_avatar(ui, id, avatar, content, a, v, AvatarSize::PREVIEW);
    y += avatar_h + 16.0;
    text::paint(&painter, at(y, name_h), &a.name, &name_style);
    y += name_h;
    if meta_lines > 0 {
        y += 4.0;
        let clipped = painter.with_clip_rect(
            painter
                .clip_rect()
                .intersect(snap::rect(at(y, meta_box_h), ppp)),
        );
        let mut line_y = y;
        if show_username {
            text::paint(
                &clipped,
                at(line_y, meta_line),
                &a.account.username,
                &username,
            );
            line_y += meta_line;
        }
        if let Some(label) = &last_login {
            text::paint(&clipped, at(line_y, meta_line), label, &meta);
        }
        y += meta_box_h;
    }
    if !chips.is_empty() {
        y += 6.0;
        for (n, line) in chip_lines.iter().enumerate() {
            if n > 0 {
                y += chip_h + 4.0;
            }
            let line_w =
                line.iter().map(|&i| chips[i].2).sum::<f32>() + 4.0 * (line.len() - 1) as f32;
            let mut x = centre_x(line_w);
            for &i in line {
                let (tone, text, w) = &chips[i];
                let (bg, fg) = match tone {
                    WarningTone::Red => (RED.alpha(0.2), Rgba::hex(0xf87171, 1.0)),
                    WarningTone::Orange => (ORANGE.alpha(0.2), Rgba::hex(0xfb923c, 1.0)),
                };
                let chip = Rect::from_min_size(pos2(x, y), vec2(*w, chip_h));
                BoxStyle {
                    background: bg,
                    radius: Corners::all(3.0),
                    ..BoxStyle::default()
                }
                .paint(&painter, snap::rect(chip, ppp));
                text::paint(
                    &painter,
                    Rect::from_min_size(chip.min + vec2(6.0, 2.0), vec2(w - 12.0, chip_h - 4.0)),
                    text,
                    &chip_style.clone().color(fg.to_color32()),
                );
                x += w + 4.0;
            }
        }
        y += chip_h;
    }
    if has_note {
        y += 10.0;
        let block = at(y, note_h);
        BoxStyle {
            background: v.bg_muted.mix(0.74, Rgba::TRANSPARENT),
            radius: Corners::all(6.0),
            border: Border::all(bw, v.border),
            ..BoxStyle::default()
        }
        .paint(&painter, snap::rect(block, ppp));
        let left = block.left() + bw + 10.0;
        let mut top = block.top() + bw + 8.0;
        text::paint(
            &painter,
            Rect::from_min_size(pos2(left, top), vec2(note_w, label_h)),
            &translate(props.locale, "preview.noteLabel"),
            &label_style,
        );
        top += label_h + 4.0;
        text::paint(
            &painter,
            Rect::from_min_size(pos2(left, top), vec2(note_w, note_text_h)),
            &a.note,
            &note_style,
        );
        y += note_h;
    }
    if a.active {
        y += 16.0;
        let style = TextStyle::new(11.0, 600)
            .line_height(LineHeight::Number(v.line_height))
            .letter_spacing_px(0.5)
            .transform(TextTransform::Uppercase)
            .align(TextAlign::Center)
            .color(v.fg_muted.to_color32());
        text::paint(
            &painter,
            at(y, status_h),
            &translate(props.locale, "preview.currentlyActive"),
            &style,
        );
        y += status_h;
    }
    if !button {
        return false;
    }

    // `.switch-btn`: padding 8 16, the 14 px play icon, 6 px, the label.
    y += 16.0;
    let label_style = TextStyle::new(12.0, 500)
        .line_height(LineHeight::Number(v.line_height))
        .nowrap(true);
    let label = translate(
        props.locale,
        if a.active {
            "preview.switchAgain"
        } else {
            "preview.switchAccount"
        },
    );
    let label_w = ceil_unit(
        text::measure(&ctx, &label, &label_style).unwrap_or(0.0),
        ppp,
    );
    let w = 16.0 + 14.0 + 6.0 + label_w + 16.0;
    let r = Rect::from_min_size(pos2(centre_x(w), y), vec2(w, button_h));
    let painted = snap::rect(r, ppp);
    let response = ui
        .interact(painted, id.with("switch"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    let brightness = if response.is_pointer_button_down_on() {
        0.9
    } else if response.hovered() {
        1.15
    } else {
        1.0
    };
    let brightness = anim::transition(&ctx, id.with("switch-filter"), brightness, &T_BUTTON);
    let bright = |c: Rgba| {
        let ch = |x: f32| (x * brightness).min(255.0);
        Rgba::new(ch(c.r), ch(c.g), ch(c.b), c.a)
    };
    BoxStyle {
        background: bright(props.accent),
        radius: Corners::all(6.0),
        ..BoxStyle::default()
    }
    .paint(&painter, painted);
    let content_h = 12.0 * v.line_height;
    let icon_rect = Rect::from_min_size(
        pos2(
            r.left() + 16.0,
            r.top() + 8.0 + snap::centre(content_h, 14.0, ppp),
        ),
        vec2(14.0, 14.0),
    );
    let white = bright(Rgba::new(255.0, 255.0, 255.0, 1.0));
    icon::paint(&painter, icons::PLAY, icon_rect, white, 1.0);
    text::paint(
        &painter,
        Rect::from_min_size(
            pos2(icon_rect.right() + 6.0, r.top() + 8.0),
            vec2(label_w, content_h),
        ),
        &label,
        &label_style.color(white.to_color32()),
    );
    let focused = response.has_focus();
    if focused && props.focus_visible {
        auto_ring(&painter, painted, 6.0, props.dark, ppp);
    }
    let keys = focused && ctx.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Space));
    response.clicked() || keys
}

/// The panel's `::-webkit-scrollbar` thumb, dragged like the content's.
fn scrollbar(
    ui: &mut Ui,
    panel: Rect,
    state: &mut ListState,
    v: &Vars,
    content_h: f32,
    max_scroll: f32,
) {
    let track = Rect::from_min_max(pos2(panel.right() - SCROLLBAR, panel.top()), panel.max);
    let track_h = track.height();
    let thumb_h = (track_h * track_h / content_h).round().max(SCROLLBAR);
    let travel = track_h - thumb_h;
    let top = (travel * state.scroll.clamp(0.0, max_scroll) / max_scroll).round();
    let thumb = Rect::from_min_size(
        pos2(track.left(), track.top() + top),
        vec2(SCROLLBAR, thumb_h),
    );
    let response = ui.interact(thumb, Id::new("accshift-list-scrollbar"), Sense::drag());
    if response.drag_started() {
        if let Some(p) = response.interact_pointer_pos() {
            state.thumb_drag = Some((p.y, state.scroll));
        }
    }
    if response.dragged() {
        if let (Some((from, start)), Some(p)) = (state.thumb_drag, response.interact_pointer_pos())
        {
            if travel > 0.0 {
                state.scroll = start + (p.y - from) * max_scroll / travel;
            }
        }
    } else {
        state.thumb_drag = None;
    }
    let color = if response.hovered() || response.dragged() {
        v.bg_elevated
    } else {
        v.bg_muted
    };
    BoxStyle {
        background: color,
        radius: Corners::all(3.0),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), snap::rect(thumb, ui.ctx().pixels_per_point()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use accshift_app::display::AvatarView;
    use egui::{Context, Modifiers, PointerButton, RawInput};

    struct Fake;

    impl ListHost for Fake {
        fn mount(&mut self, key: &str) -> Id {
            Id::new(key)
        }

        fn avatar(&mut self, _: &str, _: &AccountCardItem, _: u32, _: bool) -> AvatarContent {
            AvatarContent::Loading
        }

        fn blurred(&mut self, _: &AccountCardItem, _: u32, _: f32) -> Option<TextureId> {
            None
        }

        fn card_color(&self, _: &str) -> Option<Rgba> {
            None
        }
    }

    fn account(id: &str, pending_setup: bool, single_click_switch: bool) -> DisplayItem {
        DisplayItem::Account(Box::new(AccountCardItem {
            account: PlatformAccount {
                id: id.into(),
                display_name: id.into(),
                username: id.into(),
                last_login_at_sec: None,
            },
            name: id.into(),
            username: id.into(),
            card_color: String::new(),
            note: String::new(),
            show_note_inline: false,
            show_username: false,
            show_last_login: false,
            last_login_at_sec: None,
            last_login_unknown_key: "time.unknown",
            active: false,
            switching: false,
            avatar: AvatarView::default(),
            warning: None,
            dragged: false,
            pending_setup,
            single_click_switch,
            bulk_selected: false,
        }))
    }

    /// Drives `list_view` over an 800 by 400 box at the window's top left.
    struct Bench {
        ctx: Context,
        vars: Vars,
        state: ListState,
        items: Vec<DisplayItem>,
        time: f64,
    }

    impl Bench {
        fn new(items: Vec<DisplayItem>) -> Bench {
            let vars = Vars::new(&accshift_theme::ResolvedTheme::new(
                &accshift_theme::theme_application(
                    &accshift_theme::ThemeRegistry::new().theme(Some("glass-dark")),
                    100.0,
                    true,
                ),
            ));
            Bench {
                ctx: Context::default(),
                vars,
                state: ListState::default(),
                items,
                time: 0.0,
            }
        }

        fn frame(&mut self, events: Vec<Event>) -> Vec<ListEvent> {
            self.time += 0.02;
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 400.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            let time = TimeContext {
                now_ms: 0.0,
                locale: Locale::En,
                zone: accshift_app::time::TimeZone::UTC,
            };
            let props = ListProps {
                vars: &self.vars,
                locale: Locale::En,
                accent: self.vars.accent,
                focus_visible: false,
                dark: true,
                time: &time,
                kb_focus: None,
                kb_reveal: false,
                drag_over_folder: None,
                drag_over_back: false,
                dragging: false,
                ghost: None,
            };
            let (state, items) = (&mut self.state, &self.items);
            let mut out = Vec::new();
            let mut output = self.ctx.run_ui(input, |ui| {
                let rect = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 400.0));
                out = list_view(ui, rect, state, items, &props, &mut Fake);
            });
            output.textures_delta.clear();
            out
        }

        fn press(&mut self, at: Pos2, button: PointerButton) -> Vec<ListEvent> {
            let mut out = self.frame(vec![Event::PointerMoved(at)]);
            for pressed in [true, false] {
                out.extend(self.frame(vec![Event::PointerButton {
                    pos: at,
                    button,
                    pressed,
                    modifiers: Modifiers::NONE,
                }]));
            }
            out
        }
    }

    /// The rows' centres: `.list-panel` padding 4, rows 46 tall, 2 apart.
    const ROW0: Pos2 = pos2(200.0, 27.0);
    const ROW1: Pos2 = pos2(200.0, 75.0);

    #[test]
    fn a_click_selects_and_a_double_click_switches() {
        let mut bench = Bench::new(vec![account("a", false, false), account("b", false, false)]);
        bench.frame(Vec::new());
        let boxes: Vec<Rect> = bench.state.layout.iter().map(|(_, r)| *r).collect();
        assert_eq!(
            boxes[0],
            Rect::from_min_size(pos2(4.0, 4.0), vec2(591.0, 46.0))
        );
        assert_eq!(boxes[1].top(), 52.0);

        let events = bench.press(ROW1, PointerButton::Primary);
        assert_eq!(events, [ListEvent::Activate("b".into())]);
        assert_eq!(bench.state.selected.as_deref(), Some("b"));

        let events = bench.press(ROW1, PointerButton::Primary);
        assert!(
            events.contains(&ListEvent::Switch("b".into())),
            "{events:?}"
        );

        let events = bench.press(ROW0, PointerButton::Secondary);
        assert!(matches!(&events[..], [ListEvent::AccountMenu(a, _)] if a.id == "a"));
    }

    #[test]
    fn the_preview_column_shrinks_like_blink() {
        // 30 px too tall with a 120 px avatar and one 14.4 px meta line:
        // Blink gives 26.79 and 3.21 px, rounded to 1/64.
        let (avatar, meta) = flex_shrink(30.0, 120.0, 14.4, 1.0);
        assert_eq!(avatar, 93.21875);
        assert!((meta - 11.184).abs() < 1.0 / 64.0, "{meta}");
        assert_eq!(flex_shrink(-4.0, 120.0, 14.4, 1.0), (120.0, 14.4));
        assert_eq!(flex_shrink(500.0, 120.0, 14.4, 1.5), (0.0, 0.0));
        let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let uv = box_uv(
            full,
            Rect::from_min_size(pos2(0.0, 0.0), vec2(120.0, 90.0)),
            1.0,
        );
        assert_eq!(uv, Rect::from_min_max(pos2(0.0, 0.125), pos2(1.0, 0.875)));
        // A 93.2 px box at y 108: the picture snaps to 95..215, the box to
        // 108..201, so the band starts on a whole texel.
        let rect = Rect::from_min_size(pos2(674.0, 108.0), vec2(120.0, 93.21875));
        let uv = box_uv(full, rect, 1.0);
        assert_eq!(uv.top() * 120.0, 13.0);
        assert_eq!(uv.bottom() * 120.0, 106.0);
    }

    #[test]
    fn bulk_edit_switches_on_one_click_and_never_previews() {
        let mut bench = Bench::new(vec![account("a", false, true), account("p", true, true)]);
        bench.frame(Vec::new());
        let events = bench.press(ROW0, PointerButton::Primary);
        assert_eq!(
            events,
            [
                ListEvent::Activate("a".into()),
                ListEvent::Switch("a".into())
            ]
        );
        assert_eq!(bench.state.selected, None);
        // The add flow's placeholder takes no switch.
        let events = bench.press(ROW1, PointerButton::Primary);
        assert_eq!(events, [ListEvent::Activate("p".into())]);
    }

    #[test]
    fn a_selection_that_left_the_list_goes() {
        let mut bench = Bench::new(vec![account("a", false, false)]);
        bench.frame(Vec::new());
        bench.press(ROW0, PointerButton::Primary);
        assert_eq!(bench.state.selected.as_deref(), Some("a"));
        bench.items = vec![account("z", false, false)];
        bench.frame(Vec::new());
        assert_eq!(bench.state.selected, None);
    }

    #[test]
    fn the_wheel_scrolls_a_long_panel() {
        let items = (0..20)
            .map(|i| account(&format!("a{i}"), false, false))
            .collect();
        let mut bench = Bench::new(items);
        bench.frame(Vec::new());
        // 20 rows of 46, 2 apart, padding 4: 966 px in a 400 px panel, so
        // the rows leave room for the 10 px scrollbar.
        assert_eq!(bench.state.layout[0].1.width(), 581.0);
        bench.frame(vec![
            Event::PointerMoved(ROW0),
            Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, -10_000.0),
                modifiers: Modifiers::NONE,
                phase: egui::TouchPhase::Move,
            },
        ]);
        assert_eq!(bench.state.scroll, 566.0);
        bench.frame(Vec::new());
        assert_eq!(bench.state.layout[19].1.bottom(), 396.0);
    }

    fn header(collapsed: bool) -> DisplayItem {
        DisplayItem::SectionHeader(SectionHeaderItem {
            label: SectionLabel::Folder(FolderInfo {
                id: "f".into(),
                name: "f".into(),
                parent_id: None,
                platform: "steam".into(),
            }),
            collapse_key: "f".into(),
            count: 2,
            card_color: String::new(),
            collapsed,
            dragged: false,
        })
    }

    fn top_of(bench: &Bench, label: &str) -> f32 {
        let found = bench.state.layout.iter().find(|(l, _)| l == label);
        found.expect(label).1.top()
    }

    /// A folding section slides its rows out over 220 ms `cubicOut`; the
    /// root section below follows. Box tops: folder header 4, rows 30 and
    /// 78, root header 126, its row 151.
    #[test]
    fn a_folding_section_slides_its_rows_out() {
        let root = DisplayItem::SectionHeader(SectionHeaderItem {
            label: SectionLabel::Root,
            collapse_key: "root".into(),
            count: 1,
            card_color: String::new(),
            collapsed: false,
            dragged: false,
        });
        let open = vec![
            header(false),
            account("a", false, false),
            account("b", false, false),
            root.clone(),
            account("c", false, false),
        ];
        let mut bench = Bench::new(open.clone());
        bench.frame(Vec::new());
        assert_eq!(top_of(&bench, "b"), 78.0);
        assert_eq!(top_of(&bench, "c"), 151.0);

        bench.items = vec![header(true), root, account("c", false, false)];
        bench.frame(Vec::new());
        assert_eq!(top_of(&bench, "c"), 151.0);
        for _ in 0..5 {
            bench.frame(Vec::new());
        }
        // 100 ms in: the 94 px of rows at 1 - cubicOut(0.1 / 0.22).
        let left = 1.0 - (1.0 - (1.0 - 0.1f32 / 0.22).powi(3));
        let c = top_of(&bench, "c");
        assert!(
            (c - (30.0 + 94.0 * left + 2.0 + 23.0 + 2.0)).abs() < 0.5,
            "{c}"
        );
        assert_eq!(top_of(&bench, "a"), 30.0);
        for _ in 0..10 {
            bench.frame(Vec::new());
        }
        assert_eq!(top_of(&bench, "c"), 55.0);
        assert!(bench.state.layout.iter().all(|(l, _)| l != "a"));

        // Unfolding slides them back in.
        bench.items = open;
        bench.frame(Vec::new());
        assert_eq!(top_of(&bench, "c"), 57.0);
        for _ in 0..15 {
            bench.frame(Vec::new());
        }
        assert_eq!(top_of(&bench, "c"), 151.0);
    }
}
