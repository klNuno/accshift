//! The rows pass: every placed box drawn in column order, and what the
//! clicks on them report. The drag ghosts draw their clones with the same
//! per-item painter.

use accshift_app::display::{AccountCardItem, DisplayItem, SectionHeaderItem, SectionLabel};
use accshift_app::folders::FolderInfo;
use accshift_app::i18n::{translate, Locale};
use egui::{pos2, vec2, Id, Rect, Ui, Vec2};

use super::avatar::{paint_avatar, AvatarSize};
use super::ghost::GhostPart;
use super::layout::Place;
use super::row::RowLook;
use super::row::{account_info, account_look, avatar_rings, icon_box, list_row, plain_name, Row};
use super::{dragged, item_id, item_ref, key_of, ListEvent, ListHost, ListProps, ListState};
use super::{ICON, PAD};
use crate::icons;
use crate::ui::components::account_card::AvatarContent;
use crate::ui::components::section_header::{self, SectionHeaderEvent, SectionHeaderProps};
use crate::ui::components::{avatar_fade_in, folder_card};

/// Where the rows show this frame.
pub(super) struct Viewport {
    pub(super) panel: Rect,
    /// The panel inside the parent's clip.
    pub(super) clip: Rect,
    /// From content to screen coordinates: the panel's corner, scrolled.
    pub(super) scrolled: Vec2,
    pub(super) row_w: f32,
}

impl Viewport {
    /// The clip of a box: the panel's, cut to the sliding rows box it
    /// shows through.
    fn clip_of(&self, place: &Place) -> Rect {
        let Some((top, bottom)) = place.clip else {
            return self.clip;
        };
        let group = Rect::from_x_y_ranges(self.panel.x_range(), top..=bottom.max(top));
        self.clip.intersect(group.translate(self.scrolled))
    }
}

/// What the rows pass reports.
pub(super) struct RowsDrawn<'a> {
    pub(super) events: Vec<ListEvent>,
    /// The focused widget when the pass started.
    pub(super) focused: Option<Id>,
    /// Its row, in content coordinates.
    pub(super) focused_box: Option<Rect>,
    /// The dragged row and its height, for its ghost.
    pub(super) ghost: Option<(GhostPart<'a>, f32)>,
    /// The rows inside the dragged section's box, for its ghost.
    pub(super) section_parts: Vec<GhostPart<'a>>,
}

/// Draws every placed box, records where it went, and reports the clicks.
/// `section_ghost` is the dragged section's box on screen.
pub(super) fn draw_rows<'a>(
    ui: &mut Ui,
    view: &Viewport,
    places: &[Place<'a>],
    section_ghost: Option<Rect>,
    state: &mut ListState,
    props: &ListProps,
    host: &mut dyn ListHost,
) -> RowsDrawn<'a> {
    let mut out = RowsDrawn {
        events: Vec::new(),
        focused: ui.ctx().memory(|m| m.focused()),
        focused_box: None,
        ghost: None,
        section_parts: Vec::new(),
    };
    state.layout.clear();
    state.cards.clear();
    state.back = None;
    for place in places {
        let item = place.item;
        let local = Rect::from_min_size(pos2(PAD, place.top), vec2(view.row_w, place.h));
        let row = local.translate(view.scrolled);
        let clip = view.clip_of(place);
        let key = key_of(item);
        let id = host.mount(&key);
        if out.focused == Some(id) {
            out.focused_box = Some(local);
        }
        let visible = row.intersects(clip) && clip.height() > 0.0;
        state.layout.push((item_label(props.locale, item), row));
        let kb_focus = props.kb_focus.is_some() && item_id(item) == props.kb_focus;
        let item_ref = item_ref(item);
        if let Some(r) = &item_ref {
            state.cards.push((r.clone(), row));
        }
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(row));
        child.set_clip_rect(clip);
        child.multiply_opacity(place.opacity);
        // The dragged row shows at 40 % opacity, as one layer like the cards.
        let dragged = dragged(item) == Some(true);
        let group = dragged && visible;
        if group {
            folder_card::drag_group(&child, row);
        } else if dragged {
            child.multiply_opacity(0.4);
        }
        let part = GhostPart {
            item,
            rect: row,
            source: id,
            key,
            kb_focus,
        };
        let is_ghost = matches!((&props.ghost, &item_ref), (Some(g), Some(r)) if g.item == r);
        if is_ghost {
            out.ghost = Some((part.clone(), place.h));
        }
        let in_section_ghost = section_ghost
            .is_some_and(|b| b.top() <= row.top() + 0.5 && row.bottom() <= b.bottom() + 0.5);
        if in_section_ghost {
            out.section_parts.push(part.clone());
        }
        if matches!(item, DisplayItem::Back) {
            state.back = Some(row);
        }
        let draw = ItemDraw {
            item,
            rect: row,
            id,
            avatar_id: id,
            key: &part.key,
            kb_focus,
            selected: state.selected.as_deref(),
            pass: Pass::List { visible },
        };
        let drawn = draw_item(&mut child, &draw, props, host);
        item_events(item, drawn, state, &mut out.events);
        if group {
            crate::ui::backdrop_filter::Mark::End.paint(child.painter(), row);
        }
    }
    out
}

fn header_label(locale: Locale, h: &SectionHeaderItem) -> String {
    match &h.label {
        SectionLabel::Folder(f) => f.name.clone(),
        SectionLabel::Root => translate(locale, "list.rootSection"),
    }
}

/// The label a box is recorded under, as it shows.
fn item_label(locale: Locale, item: &DisplayItem) -> String {
    match item {
        DisplayItem::Back => translate(locale, "common.back"),
        DisplayItem::SectionHeader(h) => header_label(locale, h),
        DisplayItem::Folder(f) => f.folder.name.clone(),
        DisplayItem::Account(a) => a.name.clone(),
    }
}

/// Where an item is drawn: in the list, or in a drag ghost's clone.
#[derive(Clone, Copy)]
pub(super) enum Pass {
    /// In the list: painted only when `visible`, with its drop target
    /// state, a switching picture blurred.
    List { visible: bool },
    /// In a clone: always painted, never a drop target, a header without
    /// its focus ring, the back row not at all. `shadow` for a lone row,
    /// which casts its own shadow and ignores the hover.
    Ghost { shadow: bool },
}

impl Pass {
    fn visible(self) -> bool {
        match self {
            Pass::List { visible } => visible,
            Pass::Ghost { .. } => true,
        }
    }

    fn in_list(self) -> bool {
        matches!(self, Pass::List { .. })
    }

    fn shadow(self) -> bool {
        matches!(self, Pass::Ghost { shadow: true })
    }
}

/// One item to draw.
pub(super) struct ItemDraw<'a> {
    pub(super) item: &'a DisplayItem,
    pub(super) rect: Rect,
    /// The row's widget id.
    pub(super) id: Id,
    /// The id its avatar animates under: the source row's, in a clone too.
    pub(super) avatar_id: Id,
    /// The host's key for its avatar.
    pub(super) key: &'a str,
    pub(super) kb_focus: bool,
    /// The selected account, for the selected look.
    pub(super) selected: Option<&'a str>,
    pub(super) pass: Pass,
}

/// What drawing an item got.
pub(super) enum Drawn {
    Header(Vec<SectionHeaderEvent>),
    Row(Row),
    Nothing,
}

/// Draws one header or row.
pub(super) fn draw_item(
    ui: &mut Ui,
    d: &ItemDraw,
    props: &ListProps,
    host: &mut dyn ListHost,
) -> Drawn {
    match d.item {
        DisplayItem::SectionHeader(h) => Drawn::Header(draw_header(ui, d, h, props, host)),
        DisplayItem::Back if !d.pass.in_list() => Drawn::Nothing,
        DisplayItem::Back => {
            let look = RowLook {
                drag_over: props.drag_over_back,
                ..RowLook::default()
            };
            let label = translate(props.locale, "common.back");
            Drawn::Row(plain_row(ui, d, &label, icons::BACK, look, props))
        }
        DisplayItem::Folder(f) => {
            let id = f.folder.id.as_str();
            let look = RowLook {
                drag_over: d.pass.in_list() && props.drag_over_folder == Some(id),
                kb_focus: d.kb_focus,
                ghost: d.pass.shadow(),
                ..RowLook::default()
            };
            Drawn::Row(plain_row(ui, d, &f.folder.name, icons::FOLDER, look, props))
        }
        DisplayItem::Account(a) => Drawn::Row(draw_account(ui, d, a, props, host)),
    }
}

fn draw_header(
    ui: &mut Ui,
    d: &ItemDraw,
    h: &SectionHeaderItem,
    props: &ListProps,
    host: &dyn ListHost,
) -> Vec<SectionHeaderEvent> {
    let label = header_label(props.locale, h);
    let folder = match &h.label {
        SectionLabel::Folder(f) => Some(f.id.as_str()),
        SectionLabel::Root => None,
    };
    let header = SectionHeaderProps {
        vars: props.vars,
        id: d.id,
        folder,
        label: &label,
        count: h.count,
        card_color: host.card_color(&h.card_color),
        accent: props.accent,
        collapsed: h.collapsed,
        focus_visible: props.focus_visible && d.pass.in_list(),
    };
    section_header::section_header(ui, d.rect, &header)
}

/// The back or a folder row: its icon box and its name.
fn plain_row(
    ui: &mut Ui,
    d: &ItemDraw,
    label: &str,
    svg: &'static [u8],
    look: RowLook,
    props: &ListProps,
) -> Row {
    let visible = d.pass.visible();
    let r = list_row(ui, d.rect, d.id, label, look, props, visible);
    if visible {
        icon_box(ui, &r, svg, props.vars);
        plain_name(ui, &r, label, props.vars);
    }
    r
}

/// An account row: its avatar with its rings, its text, dots and badge.
/// Off screen, only the avatar's fade in keeps time.
fn draw_account(
    ui: &mut Ui,
    d: &ItemDraw,
    a: &AccountCardItem,
    props: &ListProps,
    host: &mut dyn ListHost,
) -> Row {
    let ppp = ui.ctx().pixels_per_point();
    let v = props.vars;
    let visible = d.pass.visible();
    let look = RowLook {
        ghost: d.pass.shadow(),
        ..account_look(a, d.selected, d.kb_focus, host)
    };
    let r = list_row(ui, d.rect, d.id, &a.name, look, props, visible);
    let px = (ICON * ppp).round() as u32;
    let mut content = host.avatar(d.key, a, px, visible);
    if let AvatarContent::Picture(picture) = &mut content {
        if a.switching && visible && d.pass.in_list() {
            picture.blurred = host.blurred(a, px, 3.0 * ppp);
        }
    }
    if visible {
        avatar_rings(ui, r.icon, look, v);
        paint_avatar(ui, d.avatar_id, r.icon, &content, a, v, AvatarSize::ROW);
        account_info(ui, &r, a, look, props);
    } else {
        let revealed = !matches!(content, AvatarContent::Decoding);
        avatar_fade_in::avatar_opacity(ui.ctx(), d.avatar_id.with("avatar"), revealed, v);
    }
    r
}

/// Turns what a drawn item got into list events.
fn item_events(
    item: &DisplayItem,
    drawn: Drawn,
    state: &mut ListState,
    events: &mut Vec<ListEvent>,
) {
    match (item, drawn) {
        (DisplayItem::SectionHeader(h), Drawn::Header(got)) => {
            for event in got {
                header_event(h, event, events);
            }
        }
        (DisplayItem::Back, Drawn::Row(r)) => {
            if r.clicked {
                events.push(ListEvent::Back);
            }
        }
        (DisplayItem::Folder(f), Drawn::Row(r)) => folder_events(&f.folder, &r, events),
        (DisplayItem::Account(a), Drawn::Row(r)) => account_events(a, &r, state, events),
        _ => {}
    }
}

fn header_event(h: &SectionHeaderItem, event: SectionHeaderEvent, events: &mut Vec<ListEvent>) {
    match event {
        SectionHeaderEvent::Toggle => events.push(ListEvent::ToggleSection(h.collapse_key.clone())),
        SectionHeaderEvent::Navigate(id) => events.push(ListEvent::Navigate(id)),
        SectionHeaderEvent::ContextMenu(at) => {
            if let SectionLabel::Folder(f) = &h.label {
                events.push(ListEvent::FolderMenu(f.clone(), at));
            }
        }
    }
}

fn folder_events(folder: &FolderInfo, r: &Row, events: &mut Vec<ListEvent>) {
    if r.clicked {
        events.push(ListEvent::Navigate(folder.id.clone()));
    }
    if let Some(at) = r.menu {
        events.push(ListEvent::FolderMenu(folder.clone(), at));
    }
}

/// A click activates and selects, or in bulk edit switches at once; a
/// double click switches. An account still in setup never switches.
fn account_events(
    a: &AccountCardItem,
    r: &Row,
    state: &mut ListState,
    events: &mut Vec<ListEvent>,
) {
    let id = &a.account.id;
    if r.clicked {
        events.push(ListEvent::Activate(id.clone()));
        if !a.single_click_switch {
            state.selected = Some(id.clone());
        }
    }
    let switch = if a.single_click_switch {
        r.clicked
    } else {
        r.double_clicked
    };
    if switch && !a.pending_setup {
        events.push(ListEvent::Switch(id.clone()));
    }
    if let Some(at) = r.menu {
        events.push(ListEvent::AccountMenu(Box::new(a.account.clone()), at));
    }
}
