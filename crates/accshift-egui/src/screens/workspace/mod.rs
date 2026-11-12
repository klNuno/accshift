//! The accounts workspace: the scroll box under the title bar with its
//! toolbar row (breadcrumb, search, view toggle), the error banner, the
//! centre messages and the cards, as a grid (flat or in folder sections)
//! or as the list view.
//!
//! Every card is laid out and sensed (so Tab walks them all in order), but
//! only the cards that cross the viewport are painted.
//!
//! Cards are keyed by their place and item: a key seen for the first time
//! is a new mount, which restarts the card entrance and the avatar fade. A
//! key that disappears for a frame is unmounted.
//!
//! A frame runs in phases: the toolbar and the body (`body`), the grid
//! layout and the drag slides (`layout`), the card pass (`cards`), the drag
//! ghosts (`ghost`), then scrolling and focus (`scroll`).

mod body;
mod cards;
mod ghost;
mod layout;
mod scroll;
mod search;

#[cfg(test)]
mod scene_tests;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};

use accshift_app::display::DisplayBody;
use accshift_app::folders::{ItemKind, ItemRef};
use accshift_app::i18n::Locale;
use accshift_app::jobs::Jobs;
use accshift_app::stores::{ClientStores, ViewMode};
use accshift_app::time::TimeContext;
use accshift_theme::ResolvedTheme;
use egui::{pos2, vec2, Context, Id, Rect, Ui};

use crate::app::accounts::Accounts;
use crate::app::platforms::PlatformDef;
use crate::app::vars::Vars;
use crate::app::{Action, MenuTarget};
use crate::theme::Rgba;
use crate::ui::anim::Motion;
use crate::ui::components::breadcrumb::{BreadcrumbProps, Crumb};
use crate::ui::paint::Gradient;
use body::{Body, GridItems, Message, Toolbar};
use cards::{AddFlowCards, CardsDrawn, CardsInput};
use layout::{Folds, Laid};
use search::Search;

/// The content box's padding.
const PAD_TOP: f32 = 10.0;
const PAD_X: f32 = 16.0;
const PAD_BOTTOM: f32 = 16.0;
/// The scrollbar's width, its gutter kept even when nothing scrolls.
pub const SCROLLBAR: f32 = 10.0;

/// What the workspace reads this frame. Built by the shell.
pub struct WorkspaceInput<'a> {
    pub vars: &'a Vars,
    pub resolved: &'a ResolvedTheme,
    pub motion: Motion,
    pub locale: Locale,
    pub stores: &'a ClientStores,
    pub accounts: &'a mut Accounts,
    pub jobs: &'a Jobs,
    /// The active platform's definition.
    pub platform: Option<&'a PlatformDef>,
    /// The active platform runs on this OS.
    pub usable: bool,
    /// How many platforms run on this OS.
    pub compatible_platforms: usize,
    /// A new key remounts every card.
    pub panel_key: &'a str,
    pub focus_visible: bool,
    /// Dark form controls (the search field's clear button).
    pub dark: bool,
    /// The glass theme is on.
    pub glass: bool,
    /// Bulk edit is open: the grid and the list keep 52 px free at the
    /// bottom for its bar.
    pub bulk_edit: bool,
    pub now: f64,
}

/// State kept across frames.
#[derive(Default)]
pub struct Workspace {
    scroll: f32,
    /// While the scrollbar thumb is dragged: the pointer's y and the scroll
    /// offset when the drag started.
    thumb_drag: Option<(f32, f32)>,
    search: Search,
    /// Key to (mount generation, frame last seen).
    mounts: HashMap<String, (u64, u64)>,
    /// The ids of `mounts`, for [`Workspace::owns`].
    mount_ids: HashSet<Id>,
    generation: u64,
    last_frame: Option<u64>,
    /// When the latest card mounted.
    last_mount: f64,
    /// When the latest late avatar was revealed.
    last_reveal: f64,
    /// The mounted cards whose avatar is still decoding.
    decoding: HashSet<String>,
    /// How many cards on screen this frame have their avatar decoding.
    visible_decoding: usize,
    /// The folder sections' collapse state and slides.
    folds: Folds,
    /// The initials avatars' colours, per hue pair.
    gradients: HashMap<(u16, u16), (Rgba, Option<Gradient>)>,
    /// The widget that had keyboard focus last frame.
    focused: Option<Id>,
    /// The pointer at the end of the last frame.
    pointer: Option<egui::Pos2>,
    /// The last frame's boxes (cards, headers) with their labels, in layout
    /// px relative to the content box top left, unscrolled: the golden test
    /// reads them.
    #[cfg(test)]
    pub layout: Vec<(String, Rect)>,
    /// The scrollable height of the content in layout px.
    pub content_height: f32,
    /// The system time zone, read once (relative and absolute dates).
    zone: Option<accshift_app::time::TimeZone>,
    /// The list view's selection and scroll.
    list: list_view_state::ListState,
    /// The card the arrow keys moved to, painted with the keyboard focus
    /// ring.
    pub kb_focus: Option<String>,
    /// `kb_focus` scrolls into view (to its nearest edge) next frame.
    pub kb_reveal: bool,
    /// The last frame's folder and account cards (list rows in the list
    /// view) in order, on screen.
    pub cards: Vec<(ItemRef, Rect)>,
    /// The grid offset `cards` were drawn at; `None` for list rows.
    cards_scroll: Option<f32>,
    /// Cards per row of the focused card's grid last frame.
    pub columns: usize,
    /// The back card (the back row in the list view) last frame, on screen.
    pub back: Option<Rect>,
    /// The folder sections' boxes last frame, on screen, with their folder
    /// (`None` for the root section).
    pub sections: Vec<(Option<String>, Rect)>,
    /// The cards showed in folder sections last frame.
    pub sections_mode: bool,
    /// The list view showed last frame.
    pub list_mode: bool,
    /// The scroll box last frame: what it clips is not under the pointer.
    pub viewport: Option<Rect>,
    /// The cards' mount ids last frame: their transitions give their box
    /// on screen.
    card_ids: HashMap<ItemRef, Id>,
    /// The clone of the dragged card, while a card is dragged.
    pub ghost: Option<Ghost>,
    /// Per card key, a slide to a new place: the offset it slides back from
    /// and when it started.
    flips: HashMap<String, (egui::Vec2, f64)>,
    /// Each card's layout and painted top left last frame.
    last_boxes: HashMap<String, (egui::Pos2, egui::Pos2)>,
    /// `scroll` came from a keyboard reveal and sits on a device pixel; the
    /// wheel and the thumb put it back on a whole layout pixel.
    scroll_revealed: bool,
}

mod list_view_state {
    pub(super) use crate::ui::components::list_view::ListState;
}

/// The clone of the dragged card that follows the pointer.
#[derive(Clone, Debug)]
pub struct Ghost {
    pub item: ItemRef,
    /// The source card's box on screen when the drag started.
    pub rect: Rect,
    /// The pointer when the drag started.
    pub anchor: egui::Pos2,
    /// A fresh mount: the clone replays the card entrance.
    pub id: Id,
}

impl Workspace {
    pub fn new() -> Self {
        Self::default()
    }

    /// The workspace was drawn last frame: its boxes are on screen.
    pub fn drawn_last_frame(&self, ctx: &Context) -> bool {
        self.last_frame
            .is_some_and(|last| last + 1 == ctx.cumulative_frame_nr())
    }

    /// A card's box on screen, under the transform its transitions are at.
    pub fn bounding_rect(&self, ctx: &Context, item: &ItemRef) -> Option<Rect> {
        let Some(r) = self.card_rect(item) else {
            // A folder section (sections mode), at rest when the drag
            // starts.
            return self.section_rect(item);
        };
        let Some(id) = self.card_ids.get(item) else {
            return Some(r);
        };
        let ty = crate::ui::anim::peek::<f32>(ctx, id.with("ty")).unwrap_or(0.0);
        let scale = crate::ui::anim::peek::<f32>(ctx, id.with("scale")).unwrap_or(1.0);
        Some(Rect::from_center_size(
            r.center() + vec2(0.0, ty),
            r.size() * scale,
        ))
    }

    /// The content box's scroll offset.
    pub fn scroll(&self) -> f32 {
        self.scroll
    }

    /// Nothing animates and no visible picture is decoding.
    pub fn settled(&self, vars: &Vars, motion: Motion, now: f64) -> bool {
        let full = motion == Motion::Full;
        self.folds.slides.is_empty()
            && self.visible_decoding == 0
            && (!full || now - self.last_mount >= vars.motion_card_entrance)
            && (!full || now - self.last_reveal >= vars.motion_avatar_fade)
    }

    /// Whether `id` is a box the workspace mounted (a card, a header, a
    /// list row).
    pub fn owns(&self, id: Id) -> bool {
        self.mount_ids.contains(&id)
    }

    /// The card `item` on screen, as last drawn.
    pub fn card_rect(&self, item: &ItemRef) -> Option<Rect> {
        self.cards
            .iter()
            .find(|(card, _)| card == item)
            .map(|(_, r)| *r)
    }

    /// The box of a folder's section, in screen points.
    fn section_rect(&self, item: &ItemRef) -> Option<Rect> {
        if item.kind != ItemKind::Folder {
            return None;
        }
        self.sections
            .iter()
            .find(|(folder, _)| folder.as_deref() == Some(item.id.as_str()))
            .map(|(_, r)| *r)
    }

    /// The mount id of `key` this frame.
    fn mount(&mut self, key: &str, frame: u64, now: f64) -> Id {
        let generation = match self.mounts.get_mut(key) {
            Some(entry) => {
                entry.1 = frame;
                entry.0
            }
            None => {
                self.generation += 1;
                self.last_mount = now;
                self.mounts
                    .insert(key.to_string(), (self.generation, frame));
                self.mount_ids.insert(mount_id(key, self.generation));
                self.generation
            }
        };
        mount_id(key, generation)
    }

    /// Drops the mounts `keep` refuses, with the temp state they wrote.
    fn unmount(&mut self, ctx: &Context, mut keep: impl FnMut(&str, u64) -> bool) {
        let ids = &mut self.mount_ids;
        self.mounts.retain(|key, (generation, seen)| {
            let kept = keep(key, *seen);
            if !kept {
                let id = mount_id(key, *generation);
                ids.remove(&id);
                crate::ui::anim::forget(ctx, id);
            }
            kept
        });
    }

    /// Starts frame `frame`: a workspace not drawn last frame starts over,
    /// and the nodes [`remount`] named mount anew.
    fn begin_frame(&mut self, ctx: &Context, frame: u64) {
        if self.last_frame.is_none_or(|last| last + 1 != frame) {
            self.unmount(ctx, |_, _| false);
            self.folds.clear();
            self.decoding.clear();
        }
        self.last_frame = Some(frame);
        let moved = ctx
            .data_mut(|d| d.remove_temp::<Vec<Moved>>(moved_id()))
            .unwrap_or_default();
        if moved.is_empty() {
            return;
        }
        self.unmount(ctx, |key, _| !moved.iter().any(|m| m.owns(key)));
        // A section inserted again has no drag transition to start from.
        ctx.data_mut(|d| {
            for m in &moved {
                if let Moved::Section(id) = m {
                    let sid = Id::unique(("section-dragged", id));
                    d.remove::<crate::ui::anim::TransitionState<f32>>(sid.with("o"));
                    d.remove::<crate::ui::anim::TransitionState<f32>>(sid.with("s"));
                }
            }
        });
    }

    /// Keeps what the frame drew for the next frame's keys, drag and
    /// focus.
    fn publish(
        &mut self,
        f: &Frame,
        laid: &Laid,
        card_ids: HashMap<ItemRef, Id>,
        back: Option<Rect>,
        list_mode: bool,
    ) {
        self.card_ids = card_ids;
        self.list_mode = list_mode;
        self.viewport = Some(f.clip);
        if !f.drag.dragging {
            self.ghost = None;
        }
        // Cards per row of the focused card's grid (each section counts its
        // own), else of the first card's.
        let focused = self.kb_focus.as_deref();
        self.columns = laid
            .placed
            .iter()
            .find(|p| focused.is_some() && p.node.card_id() == focused)
            .or_else(|| laid.placed.iter().find(|p| p.node.card_id().is_some()))
            .map_or(1, |p| p.per_row);
        if list_mode {
            self.back = self.list.back;
            self.sections = self.list.section_boxes.clone();
            return;
        }
        self.back = back;
        self.sections = laid
            .sections
            .iter()
            .map(|(folder, r)| (folder.clone(), f.to_screen(*r)))
            .collect();
        self.cards_scroll = Some(f.scroll);
        self.cards = laid
            .placed
            .iter()
            .filter_map(|p| Some((p.node.item_ref()?, f.to_screen(p.rect))))
            .collect();
    }

    /// Ends the frame: the slides go back, and what was not drawn unmounts.
    fn end_frame(&mut self, ctx: &Context, frame: u64, folds: Folds) {
        let sliding = !folds.slides.is_empty();
        self.folds = folds;
        if sliding {
            ctx.request_repaint();
        }
        self.unmount(ctx, |_, seen| seen == frame);
        let mounted: HashSet<&String> = self.mounts.keys().collect();
        self.decoding.retain(|k| mounted.contains(k));
    }
}

/// A node the user moved: it mounts anew, which restarts its entrance.
#[derive(Clone)]
pub enum Moved {
    /// A folder section by folder id, with its header and cards.
    Section(String),
    /// A card by kind and id.
    Card(ItemKind, String),
}

impl Moved {
    /// Whether the mount `key` belongs to this node.
    fn owns(&self, key: &str) -> bool {
        match self {
            Moved::Section(id) => key.contains(&format!("/s/{id}/")),
            Moved::Card(kind, id) => {
                let tag = if *kind == ItemKind::Folder { "f" } else { "a" };
                key.ends_with(&format!("/{tag}/{id}"))
            }
        }
    }
}

/// The id of the mount `key` in its `generation`: a remount is a fresh id.
fn mount_id(key: &str, generation: u64) -> Id {
    Id::unique(("ws-card", key, generation))
}

fn moved_id() -> Id {
    Id::unique("ws-moved")
}

/// Mounts `nodes` anew at the next draw.
pub fn remount(ctx: &Context, nodes: Vec<Moved>) {
    if nodes.is_empty() {
        return;
    }
    ctx.data_mut(|d| {
        let mut all = d.get_temp::<Vec<Moved>>(moved_id()).unwrap_or_default();
        all.extend(nodes);
        d.insert_temp(moved_id(), all);
    });
}

/// The card drag as this frame sees it.
struct DragState {
    dragging: bool,
    /// The folder card the dragged card is over.
    over_folder: Option<String>,
    over_back: bool,
    pointer: egui::Pos2,
}

/// What every phase of one frame reads.
struct Frame<'a> {
    ctx: Context,
    ppp: f32,
    v: &'a Vars,
    resolved: &'a ResolvedTheme,
    jobs: &'a Jobs,
    locale: Locale,
    motion: Motion,
    /// The active platform's accent.
    accent: Rgba,
    focus_visible: bool,
    dark: bool,
    glass: bool,
    bulk_edit: bool,
    now: f64,
    frame: u64,
    panel_key: &'a str,
    /// The scroll box: the panel less the scrollbar gutter.
    viewport: Rect,
    /// The content width inside the padding.
    inner_w: f32,
    /// The scroll box cut to the ui's clip.
    clip: Rect,
    /// The content box's top left on screen, unscrolled.
    origin: egui::Pos2,
    /// The scroll offset the frame draws at.
    scroll: f32,
    time: TimeContext,
    drag: DragState,
}

impl Frame<'_> {
    /// A box in content coordinates, on screen.
    fn to_screen(&self, r: Rect) -> Rect {
        r.translate(self.origin.to_vec2() - vec2(0.0, self.scroll))
    }
}

/// Draws the workspace into `rect` (the panel slot below the title bar).
pub fn draw(ui: &mut Ui, rect: Rect, ws: &mut Workspace, input: WorkspaceInput) -> Vec<Action> {
    let _span = crate::bench::span("ws");
    let pre_span = crate::bench::span("ws_pre");
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let frame = ctx.cumulative_frame_nr();
    ws.begin_frame(&ctx, frame);
    let WorkspaceInput {
        vars: v,
        resolved,
        motion,
        locale,
        stores,
        accounts,
        jobs,
        platform,
        usable,
        compatible_platforms,
        panel_key,
        focus_visible,
        dark,
        glass,
        bulk_edit,
        now,
    } = input;
    let accent = platform.map_or(v.accent, |p| p.accent);
    let platform_name = platform.map_or("", |p| p.name.as_str());
    let tab = platform.map_or("", |p| p.id.as_str());

    // The scroll box, its scrollbar gutter on the right.
    let viewport = Rect::from_min_max(rect.min, pos2(rect.right() - SCROLLBAR, rect.bottom()));
    let clip = viewport.intersect(ui.clip_rect());
    ws.reveal_kb_focus(viewport, ppp);

    let mut folds = std::mem::take(&mut ws.folds);
    let path: Vec<(String, String)> = accounts
        .navigation
        .folder_path(stores)
        .into_iter()
        .map(|f| (f.id.clone(), f.name.clone()))
        .collect();
    let path: Vec<Crumb> = path.iter().map(|(id, name)| Crumb { id, name }).collect();
    let crumbs = BreadcrumbProps {
        vars: v,
        platform_name,
        path: &path,
        accent,
        focus_visible,
    };
    let Accounts {
        view,
        prev_view,
        lists,
        avatars,
        navigation,
        registry,
        clock,
        cs2,
        drag,
        add_flow,
        setup,
        ..
    } = accounts;
    let (x, y) = drag.pointer();
    let f = Frame {
        ctx: ctx.clone(),
        ppp,
        v,
        resolved,
        jobs,
        locale,
        motion,
        accent,
        focus_visible,
        dark,
        glass,
        bulk_edit,
        now,
        frame,
        panel_key,
        viewport,
        inner_w: viewport.width() - 2.0 * PAD_X,
        clip,
        origin: pos2(viewport.left() + PAD_X, viewport.top() + PAD_TOP),
        scroll: ws.scroll,
        time: TimeContext {
            now_ms: clock.now_ms(),
            locale,
            zone: ws
                .zone
                .get_or_insert_with(accshift_app::time::system_zone)
                .clone(),
        },
        drag: DragState {
            dragging: drag.is_dragging(),
            over_folder: drag.drag_over_folder_id().map(str::to_owned),
            over_back: drag.drag_over_back(),
            pointer: pos2(x, y),
        },
    };

    // Everything above the cards, in content coordinates.
    let mut actions = Vec::new();
    let mut laid = Laid::default();
    let mut list_mode = false;
    let mut y = 0.0;
    let adapter_ready = registry.adapter(tab).is_some();
    let body = Body::pick(
        compatible_platforms,
        usable,
        adapter_ready,
        locale,
        platform_name,
    );
    match &body {
        Body::Message(message) => y += body::center_message(ui, &f, y, message),
        Body::Unsupported(message) => {
            y += body::breadcrumb_row(ui, &f, &crumbs, y, &mut actions);
            y += body::center_message(ui, &f, y, message);
        }
        Body::Toolbar => {
            let mode = stores.view_mode();
            let toolbar = Toolbar {
                crumbs: &crumbs,
                query: navigation.search_query(),
                mode,
            };
            y += toolbar.draw(ui, &mut ws.search, &f, y, &mut actions);
            if let Some(error) = &view.error {
                y += body::error_banner(ui, &f, y, &error.resolve(locale));
            }
            if let Some(message) = Message::for_view(&view.body, locale) {
                y += body::center_message(ui, &f, y, &message);
            } else if let DisplayBody::Items {
                items,
                folder_empty,
            } = &view.body
            {
                if mode == ViewMode::List {
                    y += body::draw_list(ui, ws, &f, avatars, items, y, &mut actions);
                    list_mode = true;
                } else {
                    let cards = GridItems {
                        items,
                        folder_empty: *folder_empty,
                        sections: lists.sections.as_deref(),
                        previous: prev_view.items(),
                    };
                    y = body::lay_grid(&f, &mut laid, &mut folds, cards, y);
                }
            }
        }
    }
    let content_h = PAD_TOP + y + PAD_BOTTOM;
    ws.content_height = content_h;
    ws.animate_flips(
        &ctx,
        &mut laid.placed,
        layout::flips_on(lists, f.drag.dragging),
        now,
    );

    let cards_input = CardsInput {
        avatars,
        cs2,
        add_flow: AddFlowCards::new(add_flow, tab, setup),
    };
    drop(pre_span);
    let CardsDrawn {
        focus,
        card_ids,
        back,
        ghosts,
    } = cards::draw_cards(ui, ws, &f, cards_input, &laid, &mut actions);
    ws.publish(&f, &laid, card_ids, back, list_mode);
    ws.sections_mode = lists.sections.is_some() && (list_mode || !laid.placed.is_empty());
    ghost::draw_ghosts(ui, ws, &f, &laid.placed, cs2, ghosts);
    drop(laid);
    ws.end_frame(&ctx, frame, folds);

    if ws.scroll_frame(ui, rect, &f, content_h, focus) {
        // Scrolling closes an open menu.
        actions.push(Action::ContentScrolled);
    }
    // A right click no card or folder header took reaches the box.
    let opened = actions.iter().any(|a| matches!(a, Action::ContextMenu(..)));
    if body.has_background_menu() && !opened {
        let at = crate::ui::components::context_click_pos(ui);
        if let Some(at) = at.filter(|p| rect.intersect(ui.clip_rect()).contains(*p)) {
            actions.push(Action::ContextMenu(MenuTarget::Background, at));
        }
    }
    actions
}
