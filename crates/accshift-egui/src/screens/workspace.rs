//! `src/lib/app/AppWorkspace.svelte` in grid mode: the `main.content`
//! scroll box with its toolbar row (breadcrumb, search, view toggle), the
//! error banner, the centre messages and the card grid, flat or in folder
//! sections.
//!
//! - `.content`: `padding: 10px 16px 16px`, `overflow-y: auto`,
//!   `scrollbar-gutter: stable` (the 10 px `::-webkit-scrollbar` of
//!   `app.css`, thumb `--bg-muted`, hover `--bg-elevated`, radius 3).
//! - `.toolbar-row`: flex, `align-items: center`, gap 8, `padding-bottom:
//!   8px`; the breadcrumb takes the free width.
//! - `.search-input`: `min(240px, 38vw)` by 30, 1 px `--border`, radius 8,
//!   `--bg-card`, 12 px, `padding: 0 10px`; focus border `color-mix(--fg
//!   35%, --border)`.
//! - `.grid-container`: flex wrap, gap 10, `padding-left` from
//!   `useGridLayout`. `.sections-wrapper`: column, gap 22; `.section`:
//!   column, gap 8; a section grid enters and leaves with `transition:slide
//!   {duration: 220}`.
//!
//! The Svelte grid is not virtualised: every card is in the DOM. Here every
//! card is laid out and sensed (so Tab walks them all in DOM order), but
//! only the cards that cross the viewport are painted.
//!
//! Cards are keyed like the Svelte `{#each ... (item.id)}` blocks: a key
//! seen for the first time is a new mount, which restarts `card-entrance`
//! and the avatar fade. A key that disappears for a frame is unmounted.
//!
//! The list view is not ported: `viewMode === "list"` draws a placeholder.

use std::collections::{HashMap, HashSet};

use accshift_app::actions::avatar_fallback::{avatar_gradient, avatar_seed};
use accshift_app::actions::extension::{note_section, warning_section};
use accshift_app::display::{
    AccountCardItem, DisplayBody, DisplayItem, FolderCardItem, SectionHeaderItem, SectionLabel,
};
use accshift_app::folders::{ItemKind, ItemRef};
use accshift_app::i18n::{translate, Locale};
use accshift_app::jobs::Jobs;
use accshift_app::stores::{ClientStores, ViewMode};
use accshift_theme::ResolvedTheme;
use egui::layers::ShapeIdx;
use egui::{pos2, vec2, Context, Event, Id, Key, LayerId, Rect, Sense, Ui, WidgetInfo, WidgetType};

use crate::app::accounts::Accounts;
use crate::app::avatars::AvatarTexture;
use crate::app::platforms::PlatformDef;
use crate::app::vars::{rgba, Vars};
use crate::app::{Action, MenuTarget};
use crate::theme::Rgba;
use crate::ui::anim::Motion;
use crate::ui::components::account_card::{
    self, AccountCardProps, AvatarContent, AvatarPicture, CardEvent,
};
use crate::ui::components::avatar_fade_in;
use crate::ui::components::back_card::{self, BackCardEvent, BackCardProps};
use crate::ui::components::breadcrumb::{self, BreadcrumbProps, Crumb};
use crate::ui::components::context_menu;
use crate::ui::components::controls::scroll;
use crate::ui::components::folder_card::{self, entrance, FolderCardEvent, FolderCardProps};
use crate::ui::components::list_view::{self, ListEvent, ListHost, ListProps, ListState};
use crate::ui::components::onboarding;
use crate::ui::components::search_cancel;
use crate::ui::components::section_header::{self, SectionHeaderEvent, SectionHeaderProps};
use crate::ui::components::use_grid_layout::{grid_layout, GAP};
use crate::ui::components::view_toggle::{self, ViewToggleProps};
use crate::ui::css;
use crate::ui::paint::{Border, BoxStyle, Corners, Gradient};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextAlign, TextStyle};

/// `.content { padding: 10px 16px 16px }`.
const PAD_TOP: f32 = 10.0;
const PAD_X: f32 = 16.0;
const PAD_BOTTOM: f32 = 16.0;
/// `::-webkit-scrollbar { width: 10px }`, kept by `scrollbar-gutter: stable`.
pub const SCROLLBAR: f32 = 10.0;
/// `.toolbar-row { gap: 8px; padding-bottom: 8px }`.
const TOOLBAR_GAP: f32 = 8.0;
/// `.search-input { width: min(240px, 38vw); height: 30px }`.
const SEARCH_W: f32 = 240.0;
const SEARCH_H: f32 = 30.0;
/// `.sections-wrapper { gap: 22px }`, `.section { gap: 8px }`.
const SECTIONS_GAP: f32 = 22.0;
const SECTION_GAP: f32 = 8.0;
/// `transition:slide={{ duration: 220 }}` (Svelte's `cubicOut`).
const SLIDE: f64 = 0.22;
/// Chromium's caret blink half period.
const CARET_BLINK: f64 = 0.5;

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
    /// `activeTabUsable`.
    pub usable: bool,
    /// `compatiblePlatformCount`.
    pub compatible_platforms: usize,
    /// `panelKey`: a new key remounts everything.
    pub panel_key: &'a str,
    pub focus_visible: bool,
    /// `color-scheme: dark`: picks Chromium's dark form-control bitmaps.
    pub dark: bool,
    /// `html[data-glass="1"]`.
    pub glass: bool,
    /// `bulkEditMode`: the grid and the list keep 52 px free at the bottom
    /// for the bar.
    pub bulk_edit: bool,
    pub now: f64,
}

/// `padding-bottom: 52px` on the grid container and the list wrapper in
/// bulk edit.
const BULK_PAD: f32 = 52.0;

/// One account card's shapes in the workspace's layer.
struct Stack {
    range: std::ops::Range<usize>,
    surface: Option<std::ops::Range<usize>>,
    raised: bool,
}

fn shape_count(ctx: &Context, layer: LayerId) -> usize {
    ctx.graphics(|g| g.get(layer).map_or(0, |l| l.next_idx().0))
}

/// Reorders the cards' shapes in `span` by z-index, as the grid stacks
/// them: the surfaces of hidden extensions (`--z-card-base`) under every
/// face, the card whose extension shows (`--z-card-extension`) over all.
/// The order within each group stays the paint order.
fn restack(ctx: &Context, layer: LayerId, span: std::ops::Range<usize>, cards: &[Stack]) {
    if !cards.iter().any(|c| c.surface.is_some()) {
        return;
    }
    let lo = span.start;
    let mut group = vec![1u8; span.len()];
    for c in cards {
        if c.raised {
            for i in c.range.clone() {
                group[i - lo] = 2;
            }
        } else if let Some(s) = &c.surface {
            for i in s.clone() {
                group[i - lo] = 0;
            }
        }
    }
    ctx.graphics_mut(|g| {
        let list = g.entry(layer);
        let mut shapes: Vec<Option<egui::epaint::ClippedShape>> = list
            .all_entries()
            .skip(lo)
            .take(span.len())
            .cloned()
            .map(Some)
            .collect();
        let mut at = lo;
        for z in 0..3u8 {
            for (i, shape) in shapes.iter_mut().enumerate() {
                if group[i] == z {
                    if let Some(s) = shape.take() {
                        list.set(ShapeIdx(at), s.clip_rect, s.shape);
                    }
                    at += 1;
                }
            }
        }
    });
}

/// A section grid entering or leaving.
struct Slide {
    start: f64,
    intro: bool,
    /// The cards of a leaving section, drawn until it is gone.
    items: Vec<DisplayItem>,
}

/// The search field's editing state (the value lives in `Navigation`).
#[derive(Default)]
struct Search {
    /// When the caret blink restarted (focus or edit).
    blink_from: f64,
    focused: bool,
}

/// State kept across frames.
#[derive(Default)]
pub struct Workspace {
    scroll: f32,
    thumb_drag: Option<(f32, f32)>,
    search: Search,
    /// Key to (mount generation, frame last seen).
    mounts: HashMap<String, (u64, u64)>,
    generation: u64,
    last_frame: Option<u64>,
    /// When the latest card mounted.
    last_mount: f64,
    /// When the latest late avatar was revealed.
    last_reveal: f64,
    /// Visible cards whose avatar is still decoding.
    decoding: HashSet<String>,
    visible_decoding: usize,
    collapsed: HashMap<String, bool>,
    slides: HashMap<String, Slide>,
    gradients: HashMap<(u16, u16), (Rgba, Option<Gradient>)>,
    focused: Option<Id>,
    /// The pointer at the end of the last frame.
    pointer: Option<egui::Pos2>,
    /// The last frame's boxes (cards, headers) with their labels, in layout
    /// px relative to the content box top left, unscrolled: the golden test
    /// reads them.
    pub layout: Vec<(String, Rect)>,
    /// The scroll offset in layout px (`scrollTop`).
    pub content_height: f32,
    /// The system time zone, read once (relative and absolute dates).
    zone: Option<accshift_app::time::TimeZone>,
    /// The list view's selection and scroll.
    list: ListState,
    /// `cardFocus.focusedId`: the card the arrow keys moved to, painted
    /// with the `[data-kb-focus]` ring.
    pub kb_focus: Option<String>,
    /// `syncDom`'s `scrollIntoView({ block: "nearest" })` is due.
    pub kb_reveal: bool,
    /// The last frame's folder and account cards (list rows in the list
    /// view) in DOM order, on screen.
    pub cards: Vec<(ItemRef, Rect)>,
    /// The grid offset `cards` were drawn at; `None` for list rows.
    cards_scroll: Option<f32>,
    /// `measureColumns()` on the last frame's grid.
    pub columns: usize,
    /// The back card (the back row in the list view) last frame, on screen.
    pub back: Option<Rect>,
    /// The `.section` boxes last frame, on screen, with their folder
    /// (`None` for the root section).
    pub sections: Vec<(Option<String>, Rect)>,
    /// `[data-sections-mode]` last frame.
    pub sections_mode: bool,
    /// `.list-container` last frame.
    pub list_mode: bool,
    /// The scroll box last frame: what it clips is not under the pointer.
    pub viewport: Option<Rect>,
    /// The cards' mount ids last frame: their transitions give
    /// `getBoundingClientRect`.
    card_ids: HashMap<ItemRef, Id>,
    /// `ghostEl`, while a card is dragged.
    pub ghost: Option<Ghost>,
    /// `animate:flip` per card key: the offset it slides back from and
    /// when it started.
    flips: HashMap<String, (egui::Vec2, f64)>,
    /// Each card's layout and painted top left last frame.
    last_boxes: HashMap<String, (egui::Pos2, egui::Pos2)>,
    /// `scroll` came from `scrollIntoView` and sits on a device pixel; the
    /// wheel and the thumb put it back on a CSS pixel.
    scroll_revealed: bool,
}

/// `ghostEl`: a clone of the dragged card following the pointer.
#[derive(Clone, Debug)]
pub struct Ghost {
    pub item: ItemRef,
    /// `sourceEl.getBoundingClientRect()` when the drag started.
    pub rect: Rect,
    /// The pointer when the drag started.
    pub anchor: egui::Pos2,
    /// A fresh mount: the clone replays `card-entrance`.
    pub id: Id,
}

/// `animate:flip`'s duration while a card is dragged.
const FLIP: f64 = 0.2;

impl Workspace {
    pub fn new() -> Self {
        Self::default()
    }

    /// The workspace was drawn last frame: its boxes are on screen.
    pub fn drawn_last_frame(&self, ctx: &Context) -> bool {
        self.last_frame
            .is_some_and(|last| last + 1 == ctx.cumulative_frame_nr())
    }

    /// `getBoundingClientRect()` of a card: its box under the transform
    /// its transitions are at.
    pub fn bounding_rect(&self, ctx: &Context, item: &ItemRef) -> Option<Rect> {
        let r = self.card_rect(item)?;
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

    /// `scrollTop` of the content box.
    pub fn scroll(&self) -> f32 {
        self.scroll
    }

    /// Nothing animates and no visible picture is decoding.
    pub fn settled(&self, vars: &Vars, motion: Motion, now: f64) -> bool {
        let full = motion == Motion::Full;
        self.slides.is_empty()
            && self.visible_decoding == 0
            && (!full || now - self.last_mount >= vars.motion_card_entrance)
            && (!full || now - self.last_reveal >= vars.motion_avatar_fade)
    }

    /// Whether `id` is a box the workspace mounted (a card, a header, a
    /// list row): `wrapper.contains(target)` of the card focus.
    pub fn owns(&self, id: Id) -> bool {
        self.mounts
            .iter()
            .any(|(key, (generation, _))| Id::new(("ws-card", key.as_str(), *generation)) == id)
    }

    /// The card `item` on screen, as `getBoundingClientRect` reads it.
    pub fn card_rect(&self, item: &ItemRef) -> Option<Rect> {
        self.cards
            .iter()
            .find(|(card, _)| card == item)
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
                self.generation
            }
        };
        Id::new(("ws-card", key, generation))
    }

    /// `getAvatarGradientStyle(seed)`: background colour and gradient.
    fn initials(&mut self, resolved: &ResolvedTheme, item: &AccountCardItem) -> AvatarContent {
        let account = &item.account;
        let g = avatar_gradient(&avatar_seed(
            &account.display_name,
            &account.username,
            &account.id,
        ));
        let (background, gradient) = self
            .gradients
            .entry((g.hue, g.hue_fade))
            .or_insert_with(|| {
                let hsl = |h: u16, s: u8, l: u8| {
                    resolved
                        .eval_color(&format!("hsl({h} {s}% {l}%)"))
                        .map(rgba)
                        .unwrap_or(Rgba::TRANSPARENT)
                };
                let css_rgba = |c: Rgba| format!("rgba({}, {}, {}, {})", c.r, c.g, c.b, c.a);
                let gradient = css::gradient(&format!(
                    "linear-gradient(145deg, {}, {})",
                    css_rgba(hsl(g.hue, 80, 56)),
                    css_rgba(hsl(g.hue_fade, 66, 38)),
                ));
                (hsl(g.hue, 72, 43), gradient)
            })
            .clone();
        AvatarContent::Initials {
            background,
            gradient,
        }
    }
}

/// What a laid out box holds.
enum Node<'a> {
    Back,
    EmptyFolder,
    Header(&'a SectionHeaderItem),
    Folder(&'a FolderCardItem),
    Account(&'a AccountCardItem),
}

impl Node<'_> {
    /// The card's name, as `DisplayItem::label` gives it.
    fn label(&self) -> String {
        match self {
            Node::Back => "Back".into(),
            Node::EmptyFolder => "empty".into(),
            Node::Header(h) => match &h.label {
                SectionLabel::Folder(f) => f.name.clone(),
                SectionLabel::Root => "Root".into(),
            },
            Node::Folder(f) => f.folder.name.clone(),
            Node::Account(a) => a.name.clone(),
        }
    }
}

struct Placed<'a> {
    key: String,
    /// Layout box relative to the content box top left, unscrolled.
    rect: Rect,
    /// Clip of a sliding section grid, same space.
    clip: Option<Rect>,
    opacity: f32,
    node: Node<'a>,
    /// Cards per row of its grid (1 off the grids).
    per_row: usize,
}

/// One section of the sections layout.
struct Section<'a> {
    key: String,
    header: Option<&'a SectionHeaderItem>,
    items: Vec<&'a DisplayItem>,
}

fn cubic_out(t: f64) -> f64 {
    let f = t - 1.0;
    f * f * f + 1.0
}

/// `cardColor` of the view (`""`, `"accent"` in bulk edit, or a CSS colour).
fn card_color(resolved: &ResolvedTheme, vars: &Vars, value: &str) -> Option<Rgba> {
    match value.trim() {
        "" => None,
        "accent" => Some(vars.accent),
        css => resolved.eval_color(css).map(rgba),
    }
}

/// Lays out `items` as a wrapping row grid from `(left, top)`, appending
/// to `out`; `size` gives each card's box. Returns the grid height. Like
/// `.grid-container { display: flex; flex-wrap: wrap }`, a line is as tall
/// as its tallest card and each card keeps its own height.
#[allow(clippy::too_many_arguments)]
fn flow<'a>(
    out: &mut Vec<Placed<'a>>,
    items: &[&'a DisplayItem],
    prefix: &str,
    left: f32,
    top: f32,
    width: f32,
    size: &dyn Fn(&DisplayItem) -> egui::Vec2,
    clip: Option<Rect>,
    opacity: f32,
) -> f32 {
    let cards: Vec<(&'a DisplayItem, egui::Vec2)> = items
        .iter()
        .filter(|item| !matches!(item, DisplayItem::SectionHeader(_)))
        .map(|item| (*item, size(item)))
        .collect();
    let Some(card_w) = cards.first().map(|(_, s)| s.x) else {
        return 0.0;
    };
    let per_row = (((width + GAP) / (card_w + GAP)).floor() as usize).max(1);
    let mut y = top;
    for row in cards.chunks(per_row) {
        let line = row.iter().map(|(_, s)| s.y).fold(0.0, f32::max);
        for (col, (item, card)) in row.iter().enumerate() {
            let node = match item {
                DisplayItem::Back => Node::Back,
                DisplayItem::Folder(f) => Node::Folder(f),
                DisplayItem::Account(a) => Node::Account(a),
                DisplayItem::SectionHeader(_) => unreachable!(),
            };
            let key = match &node {
                Node::Back => format!("{prefix}/back"),
                Node::Folder(f) => format!("{prefix}/f/{}", f.folder.id),
                Node::Account(a) => format!("{prefix}/a/{}", a.account.id),
                _ => unreachable!(),
            };
            out.push(Placed {
                key,
                rect: Rect::from_min_size(pos2(left + col as f32 * (card.x + GAP), y), *card),
                clip,
                opacity,
                node,
                per_row,
            });
        }
        y += line + GAP;
    }
    y - GAP - top
}

/// Draws the workspace into `rect` (the panel slot below the title bar).
pub fn draw(ui: &mut Ui, rect: Rect, ws: &mut Workspace, input: WorkspaceInput) -> Vec<Action> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = input.vars;
    let now = input.now;
    let frame = ctx.cumulative_frame_nr();
    if ws.last_frame.is_none_or(|last| last + 1 != frame) {
        // Not drawn last frame: everything unmounted.
        ws.mounts.clear();
        ws.slides.clear();
        ws.collapsed.clear();
        ws.decoding.clear();
    }
    ws.last_frame = Some(frame);
    let mut actions = Vec::new();
    let accent = input.platform.map_or(v.accent, |p| p.accent);
    let platform_name = input.platform.map_or("", |p| p.name.as_str());

    // `.content`: the scroll box, its gutter on the right.
    let viewport = Rect::from_min_max(rect.min, pos2(rect.right() - SCROLLBAR, rect.bottom()));
    let inner_w = viewport.width() - 2.0 * PAD_X;
    let clip = viewport.intersect(ui.clip_rect());
    let origin = pos2(viewport.left() + PAD_X, viewport.top() + PAD_TOP);
    // `syncDom`: the card the arrows moved to scrolls into view, from the
    // boxes the last frame drew, before anything draws. The list view
    // scrolls its own panel.
    if let Some(drawn_at) = ws.cards_scroll.filter(|_| ws.kb_reveal) {
        ws.kb_reveal = false;
        let target = ws.kb_focus.as_ref().and_then(|id| {
            ws.cards
                .iter()
                .find(|(card, _)| &card.id == id)
                .map(|(_, r)| *r)
        });
        if let Some(r) = target {
            // Back from the screen to the scroll box: the padding included.
            // On Blink's 1/64 px layout unit, or float noise from a
            // fractional `drawn_at` turns 220.5 device px into 220.4999.
            let unit = |x: f32| (x * 64.0).round() / 64.0;
            let top = unit(r.top() - viewport.top() + drawn_at);
            let bottom = unit(r.bottom() - viewport.top() + drawn_at);
            // The scroll offset lands on a whole device pixel.
            let to = scroll::nearest(ws.scroll, viewport.height(), top, bottom);
            ws.scroll = (to * ppp).round() / ppp;
            ws.scroll_revealed = true;
        }
    }

    let mut slides = std::mem::take(&mut ws.slides);
    let mut placed: Vec<Placed> = Vec::new();
    // Everything above the cards, in content coordinates.
    let mut y = 0.0;
    let mut list_mode = false;
    let path_owned: Vec<(String, String)> = input
        .accounts
        .navigation
        .folder_path(input.stores)
        .into_iter()
        .map(|f| (f.id.clone(), f.name.clone()))
        .collect();
    let path: Vec<Crumb> = path_owned
        .iter()
        .map(|(id, name)| Crumb { id, name })
        .collect();
    let crumbs = BreadcrumbProps {
        vars: v,
        platform_name,
        path: &path,
        accent,
        focus_visible: input.focus_visible,
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
        ..
    } = &mut *input.accounts;
    let grid_dragging = drag.is_dragging();
    let drag_over_folder = drag.drag_over_folder_id().map(str::to_owned);
    let drag_over_back = drag.drag_over_back();
    let pointer = {
        let (x, y) = drag.pointer();
        pos2(x, y)
    };
    let mut section_boxes: Vec<(Option<String>, Rect)> = Vec::new();
    let tab = input.platform.map_or("", |p| p.id.as_str());
    let adapter_ready = registry.adapter(tab).is_some();
    let time = accshift_app::time::TimeContext {
        now_ms: clock.now_ms(),
        locale: input.locale,
        zone: ws
            .zone
            .get_or_insert_with(accshift_app::time::system_zone)
            .clone(),
    };
    let scroll = ws.scroll;
    let to_screen = move |r: Rect| r.translate(origin.to_vec2() - vec2(0.0, scroll));

    enum Body {
        Message {
            spinner: bool,
            lines: Vec<(String, bool)>,
        },
        Toolbar,
    }
    let body = if input.compatible_platforms == 0 {
        Body::Message {
            spinner: false,
            lines: vec![
                (translate(input.locale, "app.noCompatiblePlatforms"), false),
                (
                    translate(input.locale, "app.noCompatiblePlatformsHint"),
                    true,
                ),
            ],
        }
    } else if input.usable && !adapter_ready {
        Body::Message {
            spinner: true,
            lines: vec![(translate(input.locale, "app.loading"), true)],
        }
    } else if input.usable {
        Body::Toolbar
    } else {
        // Breadcrumb, then the unsupported message.
        let h = breadcrumb::height(ui, &crumbs, inner_w);
        let r = to_screen(Rect::from_min_size(pos2(0.0, y), vec2(inner_w, h)));
        if let Some(target) = breadcrumb::breadcrumb(ui, r, &crumbs) {
            actions.push(Action::NavigateToFolder(target));
        }
        y += h;
        let message = accshift_app::text::Text::with(
            "app.platformUnsupportedOs",
            [("platform", platform_name.to_string())],
        )
        .resolve(input.locale);
        Body::Message {
            spinner: false,
            lines: vec![(message, true)],
        }
    };

    // `<main class="content" oncontextmenu>`: the toolbar and the
    // unsupported-platform branches.
    let background_menu =
        matches!(body, Body::Toolbar) || (!input.usable && input.compatible_platforms > 0);
    match body {
        Body::Message { spinner, lines } => {
            y += center_message(ui, v, accent, to_screen, y, inner_w, spinner, &lines, now);
        }
        Body::Toolbar => {
            // `.toolbar-row`.
            let search_w = SEARCH_W.min(0.38 * crate::zoom::viewport(&ctx).x);
            let crumb_w = (inner_w - search_w - view_toggle::WIDTH - 2.0 * TOOLBAR_GAP).max(0.0);
            let crumb_h = breadcrumb::height(ui, &crumbs, crumb_w);
            let row_h = crumb_h.max(SEARCH_H);
            let crumb_rect = to_screen(Rect::from_min_size(
                pos2(0.0, y + (row_h - crumb_h) / 2.0),
                vec2(crumb_w, crumb_h),
            ));
            if let Some(target) = breadcrumb::breadcrumb(ui, crumb_rect, &crumbs) {
                actions.push(Action::NavigateToFolder(target));
            }
            let search_rect = to_screen(Rect::from_min_size(
                pos2(crumb_w + TOOLBAR_GAP, y + (row_h - SEARCH_H) / 2.0),
                vec2(search_w, SEARCH_H),
            ));
            if let Some(query) = search_field(
                ui,
                search_rect,
                &mut ws.search,
                v,
                SearchProps {
                    value: navigation.search_query(),
                    placeholder: &translate(input.locale, "app.searchPlaceholder"),
                    dark: input.dark,
                    now,
                },
            ) {
                actions.push(Action::SearchChanged(query));
            }
            let toggle_rect = to_screen(Rect::from_min_size(
                pos2(
                    inner_w - view_toggle::WIDTH,
                    y + (row_h - view_toggle::HEIGHT) / 2.0,
                ),
                vec2(view_toggle::WIDTH, view_toggle::HEIGHT),
            ));
            let view_mode = input.stores.view_mode();
            if let Some(mode) = view_toggle::view_toggle(
                ui,
                toggle_rect,
                &ViewToggleProps {
                    vars: v,
                    mode: view_mode,
                    locale: input.locale,
                    focus_visible: input.focus_visible,
                },
            ) {
                actions.push(Action::SetViewMode(mode));
            }
            y += row_h + TOOLBAR_GAP;

            if let Some(error) = &view.error {
                y += error_banner(ui, to_screen, y, inner_w, &error.resolve(input.locale));
            }

            match &view.body {
                DisplayBody::Loading => {
                    let lines = vec![(translate(input.locale, "app.loading"), true)];
                    y += center_message(ui, v, accent, to_screen, y, inner_w, true, &lines, now);
                }
                DisplayBody::NoAccounts { title, hint } => {
                    let lines = vec![
                        (title.resolve(input.locale), false),
                        (hint.resolve(input.locale), true),
                    ];
                    y += center_message(ui, v, accent, to_screen, y, inner_w, false, &lines, now);
                }
                DisplayBody::NoSearchResults(text) => {
                    let lines = vec![(text.resolve(input.locale), false)];
                    y += center_message(ui, v, accent, to_screen, y, inner_w, false, &lines, now);
                }
                DisplayBody::Items { items, .. } if view_mode == ViewMode::List => {
                    // `.list-wrapper { flex: 1; min-height: 0 }`: the rest
                    // of the box, so `.content` never scrolls.
                    let h = (viewport.height() - PAD_TOP - PAD_BOTTOM - y).max(0.0);
                    let list_h = if input.bulk_edit {
                        (h - BULK_PAD).max(0.0)
                    } else {
                        h
                    };
                    let r = to_screen(Rect::from_min_size(pos2(0.0, y), vec2(inner_w, list_h)));
                    let mut list = std::mem::take(&mut ws.list);
                    let list_ghost = ws.ghost.clone().filter(|_| grid_dragging);
                    let kb_focus = ws.kb_focus.clone();
                    let kb_reveal = std::mem::take(&mut ws.kb_reveal);
                    let mut host = Host {
                        ws: &mut *ws,
                        avatars: &mut *avatars,
                        jobs: input.jobs,
                        resolved: input.resolved,
                        vars: v,
                        ctx: ctx.clone(),
                        prefix: format!("{}/", input.panel_key),
                        frame,
                        now,
                    };
                    // `.list-layout { animation: page-entrance }` from its
                    // mount.
                    let mount = host.mount("list");
                    let (opacity, rise) = page_entrance(&ctx, mount, v);
                    let props = ListProps {
                        vars: v,
                        locale: input.locale,
                        accent,
                        focus_visible: input.focus_visible,
                        dark: input.dark,
                        time: &time,
                        kb_focus: kb_focus.as_deref(),
                        kb_reveal,
                        drag_over_folder: drag_over_folder.as_deref(),
                        drag_over_back,
                        dragging: grid_dragging,
                        ghost: list_ghost.as_ref().map(|g| list_view::RowGhost {
                            item: &g.item,
                            rect: g.rect,
                            offset: pointer - g.anchor,
                            id: g.id,
                        }),
                    };
                    let transform = egui::emath::TSTransform::from_translation(vec2(0.0, rise));
                    let events = ui
                        .with_visual_transform(transform, |ui| {
                            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r));
                            child.set_clip_rect(clip);
                            child.multiply_opacity(opacity);
                            list_view::list_view(&mut child, r, &mut list, items, &props, &mut host)
                        })
                        .inner;
                    ws.list = list;
                    ws.cards = ws.list.cards.clone();
                    ws.cards_scroll = None;
                    list_mode = true;
                    for event in events {
                        actions.push(match event {
                            ListEvent::Back => Action::NavigateBack,
                            ListEvent::Navigate(id) => Action::NavigateToFolder(Some(id)),
                            ListEvent::FolderMenu(f, at) => {
                                Action::ContextMenu(MenuTarget::Folder(f), at)
                            }
                            ListEvent::Activate(id) => Action::AccountActivate(id),
                            ListEvent::Switch(id) => Action::AccountSwitch(id),
                            ListEvent::AccountMenu(a, at) => {
                                Action::ContextMenu(MenuTarget::Account(*a), at)
                            }
                            ListEvent::ToggleSection(key) => Action::ToggleSection(key),
                        });
                    }
                    y += h;
                }
                DisplayBody::Items {
                    items,
                    folder_empty,
                } => {
                    let layout = grid_layout(inner_w, v.density_scale);
                    let pad = layout.map_or(0.0, |l| l.padding_left);
                    let card = vec2(v.grid_card_width(), v.grid_card_min_height());
                    let size = |item: &DisplayItem| match item {
                        DisplayItem::Account(a) => {
                            vec2(card.x, account_card::height(&ctx, v, a, false))
                        }
                        _ => card,
                    };
                    let prefix = format!("{}/", input.panel_key);
                    if let Some(sections) = &lists.sections {
                        y += lay_sections(
                            &mut slides,
                            &mut ws.collapsed,
                            &mut placed,
                            &mut section_boxes,
                            sections,
                            items,
                            prev_view.items(),
                            &prefix,
                            pad,
                            y,
                            inner_w - pad,
                            &size,
                            input.motion,
                            now,
                        );
                    } else {
                        let refs: Vec<&DisplayItem> = items.iter().collect();
                        let prefix = format!("{prefix}flat");
                        let h = flow(
                            &mut placed,
                            &refs,
                            &prefix,
                            pad,
                            y,
                            inner_w - pad,
                            &size,
                            None,
                            1.0,
                        );
                        if *folder_empty {
                            // `.empty-folder-msg`: its own row under the
                            // back card, `padding: 2px 2px 0`, 12 px.
                            let top = y + card.y + GAP;
                            placed.push(Placed {
                                key: format!("{prefix}/empty"),
                                rect: Rect::from_min_size(
                                    pos2(pad, top),
                                    vec2(inner_w - pad, 20.0),
                                ),
                                clip: None,
                                opacity: 1.0,
                                node: Node::EmptyFolder,
                                per_row: 1,
                            });
                            y = top + 20.0;
                        } else {
                            y += h;
                        }
                    }
                    if input.bulk_edit {
                        y += BULK_PAD;
                    }
                }
            }
        }
    }
    let content_h = PAD_TOP + y + PAD_BOTTOM;
    ws.content_height = content_h;

    // `animate:flip`, 200 ms while a card is dragged and at most 100
    // render: a card the preview moves slides from where it showed. The
    // dragged card jumps.
    let rendered: usize = match &lists.sections {
        Some(sections) => sections
            .iter()
            .map(|s| s.folder_items.len() + s.account_items.len())
            .sum(),
        None => lists.folder_items.len() + lists.account_items.len(),
    };
    let flip = grid_dragging && rendered <= 100;
    let mut boxes = HashMap::new();
    for p in &mut placed {
        let dragged = match &p.node {
            Node::Folder(f) => f.dragged,
            Node::Account(a) => a.dragged,
            _ => continue,
        };
        let layout = p.rect.min;
        if let Some((was, shown)) = ws.last_boxes.get(&p.key).copied() {
            if was != layout {
                if flip && !dragged {
                    ws.flips.insert(p.key.clone(), (shown - layout, now));
                } else {
                    ws.flips.remove(&p.key);
                }
            }
        }
        if let Some((from, start)) = ws.flips.get(&p.key).copied() {
            let t = ((now - start) / FLIP).clamp(0.0, 1.0);
            if t >= 1.0 {
                ws.flips.remove(&p.key);
            } else {
                p.rect = p.rect.translate(from * (1.0 - cubic_out(t) as f32));
                ctx.request_repaint();
            }
        }
        boxes.insert(p.key.clone(), (layout, p.rect.min));
    }
    ws.last_boxes = boxes;

    // Draw the cards.
    let view_rect = Rect::from_min_size(pos2(0.0, scroll - PAD_TOP), viewport.size());
    let avatar_px = (v.grid_card_avatar_size() * ppp).round() as u32;
    ws.visible_decoding = 0;
    ws.layout.clear();
    let mut focused_rect = None;
    let focused_now = ctx.memory(|m| m.focused());
    let layer = ui.layer_id();
    let cards_from = shape_count(&ctx, layer);
    let mut stacks = Vec::new();
    let kb_focus = ws.kb_focus.clone();
    let kb = |id: &str| kb_focus.as_deref() == Some(id);
    let mut card_ids = HashMap::new();
    let mut back_rect = None;
    let mut ghost_avatar = None;
    for p in &placed {
        ws.layout.push((p.node.label(), p.rect));
        let id = ws.mount(&p.key, frame, now);
        match &p.node {
            Node::Folder(f) => {
                card_ids.insert(ItemRef::folder(&f.folder.id), id);
            }
            Node::Account(a) => {
                card_ids.insert(ItemRef::account(&a.account.id), id);
            }
            Node::Back => back_rect = Some(to_screen(p.rect)),
            _ => {}
        }
        let visible = p
            .clip
            .map_or(p.rect, |c| p.rect.intersect(c))
            .expand(32.0)
            .intersects(view_rect)
            && p.clip.is_none_or(|c| c.height() > 0.0);
        let screen = to_screen(p.rect);
        if matches!(p.node, Node::Account(_)) {
            // `data-tour="account-card"` on every card: the first wins.
            onboarding::record_target(&ctx, |t| {
                t.account_card.get_or_insert(screen);
            });
        }
        if focused_now == Some(id) {
            focused_rect = Some(p.rect);
        }
        let avatar = match &p.node {
            Node::Account(a) => {
                let mut content = account_avatar(
                    ws,
                    avatars,
                    input.jobs,
                    input.resolved,
                    &p.key,
                    a,
                    avatar_px,
                    visible,
                    now,
                );
                if let (AvatarContent::Picture(picture), Some(url)) =
                    (&mut content, a.avatar.url.as_deref())
                {
                    if visible && account_card::wants_blur(&ctx, id, a) {
                        // `blur(6px)` in device pixels.
                        picture.blurred = avatars.blurred(&ctx, url, avatar_px, 6.0 * ppp);
                    }
                }
                Some(content)
            }
            _ => None,
        };
        if let (Node::Account(a), Some(g)) = (&p.node, &ws.ghost) {
            if g.item == ItemRef::account(&a.account.id) {
                ghost_avatar = avatar.clone();
            }
        }
        if !visible {
            // Laid out and sensed, not painted; its entrance and fade clocks
            // run as if it were on screen.
            let label = match &p.node {
                Node::Back => translate(input.locale, "common.back"),
                Node::EmptyFolder => continue,
                Node::Header(h) => match &h.label {
                    SectionLabel::Folder(f) => f.name.clone(),
                    SectionLabel::Root => translate(input.locale, "list.rootSection"),
                },
                Node::Folder(f) => f.folder.name.clone(),
                Node::Account(a) => a.name.clone(),
            };
            sense_only(ui, screen, id, &label);
            if !matches!(p.node, Node::Header(_)) {
                entrance(ui, id, v);
            }
            if let Some(avatar) = &avatar {
                let revealed = !matches!(avatar, AvatarContent::Decoding);
                avatar_fade_in::avatar_opacity(&ctx, id, revealed, v);
            }
            continue;
        }
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(screen));
        let child_clip = match p.clip {
            Some(c) => to_screen(c).intersect(clip),
            None => clip,
        };
        child.set_clip_rect(child_clip);
        child.multiply_opacity(p.opacity);
        match &p.node {
            Node::Back => {
                for event in back_card::back_card(
                    &mut child,
                    screen,
                    &BackCardProps {
                        vars: v,
                        id,
                        locale: input.locale,
                        accent,
                        drag_over: drag_over_back,
                        focus_visible: input.focus_visible,
                        grid_dragging,
                    },
                ) {
                    match event {
                        BackCardEvent::Back => actions.push(Action::NavigateBack),
                    }
                }
            }
            Node::EmptyFolder => {
                let (o, _) = entrance(&child, id, v);
                let style = TextStyle::new(12.0, 400)
                    .line_height(LineHeight::Number(1.5))
                    .color(v.fg_muted.alpha(v.fg_muted.a * o).to_color32());
                let r = screen.translate(vec2(2.0, 2.0));
                text::paint(
                    child.painter(),
                    r,
                    &translate(input.locale, "app.folderEmpty"),
                    &style,
                );
            }
            Node::Header(h) => {
                let (folder, label) = match &h.label {
                    SectionLabel::Folder(f) => (Some(f.id.as_str()), f.name.clone()),
                    SectionLabel::Root => (None, translate(input.locale, "list.rootSection")),
                };
                for event in section_header::section_header(
                    &mut child,
                    screen,
                    &SectionHeaderProps {
                        vars: v,
                        id,
                        folder,
                        label: &label,
                        count: h.count,
                        card_color: card_color(input.resolved, v, &h.card_color),
                        accent,
                        collapsed: h.collapsed,
                        focus_visible: input.focus_visible,
                    },
                ) {
                    match event {
                        SectionHeaderEvent::Toggle => {
                            actions.push(Action::ToggleSection(h.collapse_key.clone()))
                        }
                        SectionHeaderEvent::Navigate(id) => {
                            actions.push(Action::NavigateToFolder(Some(id)))
                        }
                        SectionHeaderEvent::ContextMenu(at) => {
                            if let SectionLabel::Folder(f) = &h.label {
                                actions
                                    .push(Action::ContextMenu(MenuTarget::Folder(f.clone()), at));
                            }
                        }
                    }
                }
            }
            Node::Folder(f) => {
                for event in folder_card::folder_card(
                    &mut child,
                    screen,
                    &FolderCardProps {
                        vars: v,
                        id,
                        name: &f.folder.name,
                        card_color: card_color(input.resolved, v, &f.card_color),
                        accent,
                        drag_over: drag_over_folder.as_deref() == Some(f.folder.id.as_str()),
                        dragged: f.dragged,
                        focus_visible: input.focus_visible,
                        kb_focus: kb(&f.folder.id),
                        grid_dragging,
                        ghost: false,
                    },
                ) {
                    match event {
                        FolderCardEvent::Open => {
                            actions.push(Action::NavigateToFolder(Some(f.folder.id.clone())))
                        }
                        FolderCardEvent::ContextMenu(at) => actions.push(Action::ContextMenu(
                            MenuTarget::Folder(f.folder.clone()),
                            at,
                        )),
                    }
                }
            }
            Node::Account(a) => {
                // `createWarningExtensionSection`, `createNoteExtensionSection`,
                // then the extra sections (Steam's CS2 bridge row).
                let cs2 = cs2.card(&a.account.id).cloned();
                let sections: Vec<_> = warning_section(a.warning.as_ref(), input.locale)
                    .into_iter()
                    .chain(note_section(&a.note, a.show_note_inline))
                    .chain(cs2.iter().flat_map(|c| c.sections.iter().cloned()))
                    .collect();
                // `bulkEditMode ? null : getUsernameBadge(account.id)`.
                let badge = cs2
                    .as_ref()
                    .and_then(|c| c.badge.as_ref())
                    .filter(|_| !a.single_click_switch);
                let start = shape_count(&ctx, layer);
                let out = account_card::account_card(
                    &mut child,
                    screen,
                    &AccountCardProps {
                        vars: v,
                        id,
                        item: a,
                        card_color: card_color(input.resolved, v, &a.card_color),
                        avatar: avatar.clone().unwrap_or(AvatarContent::Loading),
                        focus_visible: input.focus_visible,
                        locale: input.locale,
                        time: &time,
                        badge,
                        extension: &sections,
                        extension_disabled: a.single_click_switch || grid_dragging,
                        glass: input.glass,
                        kb_focus: kb(&a.account.id),
                        grid_dragging,
                        ghost: false,
                    },
                );
                stacks.push(Stack {
                    range: start..shape_count(&ctx, layer),
                    surface: out.surface,
                    raised: out.raised,
                });
                for event in out.events {
                    match event {
                        CardEvent::Activate => {
                            actions.push(Action::AccountActivate(a.account.id.clone()))
                        }
                        CardEvent::Switch => {
                            actions.push(Action::AccountSwitch(a.account.id.clone()))
                        }
                        CardEvent::ContextMenu(at) => actions.push(Action::ContextMenu(
                            MenuTarget::Account(a.account.clone()),
                            at,
                        )),
                    }
                }
            }
        }
    }
    restack(&ctx, layer, cards_from..shape_count(&ctx, layer), &stacks);
    ws.card_ids = card_ids;
    if list_mode {
        ws.back = ws.list.back;
        ws.sections = ws.list.section_boxes.clone();
    } else {
        ws.back = back_rect;
        ws.sections = section_boxes
            .into_iter()
            .map(|(folder, r)| (folder, to_screen(r)))
            .collect();
    }
    ws.sections_mode = lists.sections.is_some() && (list_mode || !placed.is_empty());
    ws.list_mode = list_mode;
    ws.viewport = Some(clip);
    if !grid_dragging {
        ws.ghost = None;
    }
    // `ghostEl`: `position: fixed` over everything, `pointer-events: none`.
    let ghost_node = ws.ghost.clone().and_then(|g| {
        let node = placed.iter().find_map(|p| match &p.node {
            Node::Account(a) if g.item == ItemRef::account(&a.account.id) => Some(&p.node),
            Node::Folder(f) if g.item == ItemRef::folder(&f.folder.id) => Some(&p.node),
            _ => None,
        })?;
        Some((g, node))
    });
    if let Some((g, node)) = ghost_node {
        draw_ghost(
            ui,
            &g,
            node,
            pointer,
            &GhostInput {
                vars: v,
                resolved: input.resolved,
                accent,
                locale: input.locale,
                time: &time,
                glass: input.glass,
                cs2,
                avatar: ghost_avatar,
            },
        );
    }
    if !list_mode {
        ws.cards_scroll = Some(scroll);
        ws.cards = placed
            .iter()
            .filter_map(|p| {
                let item = match &p.node {
                    Node::Folder(f) => ItemRef {
                        kind: ItemKind::Folder,
                        id: f.folder.id.clone(),
                    },
                    Node::Account(a) => ItemRef {
                        kind: ItemKind::Account,
                        id: a.account.id.clone(),
                    },
                    _ => return None,
                };
                Some((item, to_screen(p.rect)))
            })
            .collect();
    }
    // `measureColumns(item)`: cards per row of the focused card's grid, so
    // each section of the sections layout counts its own.
    fn card_id<'p>(p: &'p Placed) -> Option<&'p str> {
        match &p.node {
            Node::Folder(f) => Some(f.folder.id.as_str()),
            Node::Account(a) => Some(a.account.id.as_str()),
            _ => None,
        }
    }
    let focused = ws.kb_focus.as_deref();
    ws.columns = placed
        .iter()
        .find(|p| focused.is_some() && card_id(p) == focused)
        .or_else(|| placed.iter().find(|p| card_id(p).is_some()))
        .map_or(1, |p| p.per_row);
    drop(placed);
    let sliding = !slides.is_empty();
    ws.slides = slides;
    if sliding {
        ctx.request_repaint();
    }
    // Unmount what was not drawn.
    ws.mounts.retain(|_, (_, seen)| *seen == frame);
    let mounted: HashSet<&String> = ws.mounts.keys().collect();
    ws.decoding.retain(|k| mounted.contains(k));

    // Scrolling: the wheel over the box, the thumb, and keyboard focus.
    let max_scroll = (content_h - viewport.height()).max(0.0);
    // The wheel events themselves, not egui's smoothed delta: Chromium
    // applies a point delta at once, at the pointer position of the event
    // (a scene can move the pointer again in the same frame). A line is
    // Windows' 100 px notch (3 lines of 100/3 px).
    let page = viewport.height();
    let target = rect.intersect(ui.clip_rect());
    let on_menu = context_menu::hit_test(&ctx);
    let dy: f32 = ctx.input(|i| {
        let mut at = ws.pointer;
        let mut dy = 0.0;
        for event in &i.events {
            match event {
                Event::PointerMoved(p) => at = Some(*p),
                Event::PointerGone => at = None,
                // The menu is no descendant of `.content`: a wheel over it
                // scrolls nothing here.
                Event::MouseWheel { unit, delta, .. }
                    if at.is_some_and(|p| target.contains(p) && !on_menu(p)) =>
                {
                    dy += match unit {
                        egui::MouseWheelUnit::Point => delta.y,
                        egui::MouseWheelUnit::Line => delta.y * 100.0,
                        egui::MouseWheelUnit::Page => delta.y * page,
                    };
                }
                _ => {}
            }
        }
        dy
    });
    ws.pointer = ctx.input(|i| i.pointer.latest_pos());
    if dy != 0.0 {
        ws.scroll -= dy;
        ws.scroll_revealed = false;
    }
    if focused_now != ws.focused {
        ws.focused = focused_now;
        if let Some(r) = focused_rect {
            let top = r.top() + PAD_TOP - 2.0;
            let bottom = r.bottom() + PAD_TOP + 2.0;
            if top < ws.scroll {
                ws.scroll = top;
                ws.scroll_revealed = false;
            } else if bottom > ws.scroll + viewport.height() {
                ws.scroll = bottom - viewport.height();
                ws.scroll_revealed = false;
            }
        }
    }
    scrollbar(ui, rect, ws, v, content_h, max_scroll);
    let clamped = ws.scroll.clamp(0.0, max_scroll);
    ws.scroll = if ws.scroll_revealed {
        (clamped * ppp).round() / ppp
    } else {
        clamped.round()
    };
    if ws.scroll != scroll {
        ctx.request_repaint();
        // `scroll` (capture) on the document: an open menu closes.
        actions.push(Action::ContentScrolled);
    }
    // A right click no card or folder header took bubbles up to `.content`.
    let opened = actions.iter().any(|a| matches!(a, Action::ContextMenu(..)));
    if background_menu && !opened {
        let at = crate::ui::components::context_click_pos(ui);
        if let Some(at) = at.filter(|p| rect.intersect(ui.clip_rect()).contains(*p)) {
            actions.push(Action::ContextMenu(MenuTarget::Background, at));
        }
    }
    actions
}

/// The list view's way into the workspace: mounts, avatars, colours.
struct Host<'a> {
    ws: &'a mut Workspace,
    avatars: &'a mut crate::app::avatars::AvatarTextures,
    jobs: &'a Jobs,
    resolved: &'a ResolvedTheme,
    vars: &'a Vars,
    ctx: egui::Context,
    /// The panel key and a slash.
    prefix: String,
    frame: u64,
    now: f64,
}

impl ListHost for Host<'_> {
    fn mount(&mut self, key: &str) -> Id {
        let key = format!("{}{key}", self.prefix);
        self.ws.mount(&key, self.frame, self.now)
    }

    fn avatar(
        &mut self,
        key: &str,
        item: &AccountCardItem,
        px: u32,
        visible: bool,
    ) -> AvatarContent {
        let key = format!("{}{key}", self.prefix);
        account_avatar(
            self.ws,
            self.avatars,
            self.jobs,
            self.resolved,
            &key,
            item,
            px,
            visible,
            self.now,
        )
    }

    fn blurred(&mut self, item: &AccountCardItem, px: u32, sigma: f32) -> Option<egui::TextureId> {
        let url = item.avatar.url.as_deref()?;
        self.avatars.blurred(&self.ctx, url, px, sigma)
    }

    fn card_color(&self, value: &str) -> Option<Rgba> {
        card_color(self.resolved, self.vars, value)
    }
}

/// `animation: page-entrance var(--motion-page-entrance) ease-out` (opacity
/// 0 to 1, `translateY` 4 px to 0) from the first call for `id`: `(opacity,
/// rise)`.
fn page_entrance(ctx: &egui::Context, id: Id, v: &Vars) -> (f32, f32) {
    let p = crate::ui::anim::animate(
        ctx,
        id.with("page-entrance"),
        &crate::ui::anim::Keyframes::from_to(0.0f32, 1.0),
        &crate::ui::anim::Animation::new(v.motion_page_entrance, crate::ui::anim::Timing::EASE_OUT),
        1.0,
    );
    (p, 4.0 * (1.0 - p))
}

/// The avatar box content of an account card, starting its decode.
#[allow(clippy::too_many_arguments)]
fn account_avatar(
    ws: &mut Workspace,
    avatars: &mut crate::app::avatars::AvatarTextures,
    jobs: &Jobs,
    resolved: &ResolvedTheme,
    key: &str,
    a: &AccountCardItem,
    px: u32,
    visible: bool,
    now: f64,
) -> AvatarContent {
    let picture = a
        .avatar
        .url
        .as_deref()
        .map(|url| avatars.get(jobs, url, px));
    match picture {
        Some(AvatarTexture::Ready { texture, size }) => {
            if ws.decoding.remove(key) {
                ws.last_reveal = now;
            }
            AvatarContent::Picture(AvatarPicture {
                texture,
                size,
                blurred: None,
            })
        }
        Some(AvatarTexture::Pending) => {
            ws.decoding.insert(key.to_string());
            if visible {
                ws.visible_decoding += 1;
            }
            AvatarContent::Decoding
        }
        Some(AvatarTexture::Failed) | None => {
            ws.decoding.remove(key);
            if a.avatar.loading {
                AvatarContent::Loading
            } else {
                ws.initials(resolved, a)
            }
        }
    }
}

/// Senses a card that is not painted, so Tab still walks it.
fn sense_only(ui: &mut Ui, rect: Rect, id: Id, label: &str) {
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    ui.ctx()
        .accesskit_node_builder(id, |node| node.set_role(egui::accesskit::Role::Button));
}

/// The sections layout: per section its header, then (unless collapsed)
/// its grid 8 px under it; sections 22 px apart. Returns the height.
#[allow(clippy::too_many_arguments)]
fn lay_sections<'a>(
    slides: &'a mut HashMap<String, Slide>,
    collapsed: &mut HashMap<String, bool>,
    out: &mut Vec<Placed<'a>>,
    boxes: &mut Vec<(Option<String>, Rect)>,
    sections: &[accshift_app::folders::FolderSection],
    items: &'a [DisplayItem],
    previous: &[DisplayItem],
    prefix: &str,
    left: f32,
    top: f32,
    width: f32,
    card: &dyn Fn(&DisplayItem) -> egui::Vec2,
    motion: Motion,
    now: f64,
) -> f32 {
    // Split the view's items on their headers, following the pipeline's
    // sections (a root section with nothing in it has no header).
    let mut parts: Vec<Section<'a>> = Vec::new();
    let mut it = items.iter().peekable();
    for section in sections {
        let count = section.folder_items.len() + section.account_items.len();
        let key = accshift_app::display::section_collapse_key(section).to_string();
        let header = if section.folder.is_some() || count > 0 {
            match it.next() {
                Some(DisplayItem::SectionHeader(h)) => Some(h),
                _ => None,
            }
        } else {
            None
        };
        let mut cards = Vec::new();
        while let Some(item) = it.peek() {
            if matches!(item, DisplayItem::SectionHeader(_)) {
                break;
            }
            cards.push(*item);
            it.next();
        }
        parts.push(Section {
            key,
            header,
            items: cards,
        });
    }

    // Collapse changes start slides.
    let mut seen = HashMap::new();
    for part in &parts {
        let Some(h) = part.header else { continue };
        let was = collapsed.get(&part.key).copied();
        if motion == Motion::Full {
            match (was, h.collapsed) {
                (Some(false), true) => {
                    let cached = section_items(previous, &part.key);
                    if !cached.is_empty() {
                        slides.insert(
                            part.key.clone(),
                            Slide {
                                start: now,
                                intro: false,
                                items: cached,
                            },
                        );
                    }
                }
                (Some(true), false) => {
                    slides.insert(
                        part.key.clone(),
                        Slide {
                            start: now,
                            intro: true,
                            items: Vec::new(),
                        },
                    );
                }
                _ => {}
            }
        }
        seen.insert(part.key.clone(), h.collapsed);
    }
    *collapsed = seen;
    slides.retain(|key, slide| now - slide.start < SLIDE && parts.iter().any(|p| &p.key == key));
    let slides: &'a HashMap<String, Slide> = slides;

    let mut y = top;
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            y += SECTIONS_GAP;
        }
        let section_prefix = format!("{prefix}s/{}", part.key);
        let section_top = y;
        let folder = part.header.and_then(|h| match &h.label {
            SectionLabel::Folder(f) => Some(f.id.clone()),
            SectionLabel::Root => None,
        });
        if let Some(h) = part.header {
            let height = if h.label == SectionLabel::Root {
                23.0
            } else {
                24.0
            };
            out.push(Placed {
                key: format!("{section_prefix}/header"),
                rect: Rect::from_min_size(pos2(left, y), vec2(width, height)),
                clip: None,
                opacity: 1.0,
                node: Node::Header(h),
                per_row: 1,
            });
            y += height;
        }
        let slide = slides.get(&part.key);
        let leaving: Vec<&'a DisplayItem>;
        let cards: &[&'a DisplayItem] = match slide {
            Some(s) if !s.intro => {
                leaving = s.items.iter().collect();
                &leaving
            }
            _ => &part.items,
        };
        if cards.is_empty() {
            boxes.push((
                folder,
                Rect::from_min_size(pos2(left, section_top), vec2(width, y - section_top)),
            ));
            continue;
        }
        if part.header.is_some() {
            y += SECTION_GAP;
        }
        // Lay out at full height, then clip to the slide's height.
        let start = out.len();
        let full = flow(out, cards, &section_prefix, left, y, width, card, None, 1.0);
        let factor = slide.map_or(1.0, |s| {
            let e = cubic_out(((now - s.start) / SLIDE).clamp(0.0, 1.0));
            if s.intro {
                e
            } else {
                1.0 - e
            }
        }) as f32;
        if slide.is_some() {
            let clip =
                Rect::from_min_size(pos2(left - 64.0, y), vec2(width + 128.0, full * factor));
            for p in &mut out[start..] {
                p.clip = Some(clip);
                p.opacity = (factor * 20.0).min(1.0);
            }
        }
        y += full * factor;
        boxes.push((
            folder,
            Rect::from_min_size(pos2(left, section_top), vec2(width, y - section_top)),
        ));
    }
    y - top
}

/// What [`draw_ghost`] reads besides the card.
struct GhostInput<'a> {
    vars: &'a Vars,
    resolved: &'a ResolvedTheme,
    accent: Rgba,
    locale: Locale,
    time: &'a accshift_app::time::TimeContext,
    glass: bool,
    cs2: &'a crate::app::accounts::Cs2Cards,
    avatar: Option<AvatarContent>,
}

/// `ghostEl`: the source card cloned at the drag start, `position: fixed`
/// at the pointer, `opacity: 0.85`, `transform: scale(1.05)` and its own
/// shadow, over everything.
fn draw_ghost(ui: &Ui, g: &Ghost, node: &Node, pointer: egui::Pos2, input: &GhostInput) {
    let ctx = ui.ctx().clone();
    let v = input.vars;
    let cs2 = match node {
        Node::Account(a) => input.cs2.card(&a.account.id).cloned(),
        _ => None,
    };
    let badge = match node {
        Node::Account(a) => cs2
            .as_ref()
            .and_then(|c| c.badge.as_ref())
            .filter(|_| !a.single_click_switch),
        _ => None,
    };
    let height = match node {
        Node::Account(a) => account_card::height(&ctx, v, a, badge.is_some()),
        _ => v.grid_card_min_height(),
    };
    // `left: clientX - ghostOffsetX`, at the source's width.
    let r = Rect::from_min_size(
        g.rect.min + (pointer - g.anchor),
        vec2(g.rect.width(), height),
    );
    // The cloned `.entrance` replays `card-entrance`, which overrides the
    // inline opacity and transform while it runs.
    let (enter_opacity, _) = entrance(ui, g.id, v);
    let entering = enter_opacity < 1.0;
    let (scale, opacity) = if entering { (1.0, 1.0) } else { (1.05, 0.85) };
    crate::ui::components::drag_ghost(&ctx, r, scale, opacity, |gui| {
        ghost_card(gui, g, node, r, badge, input);
    });
}

/// The clone inside the ghost: the source card at rest.
fn ghost_card(
    gui: &mut Ui,
    g: &Ghost,
    node: &Node,
    r: Rect,
    badge: Option<&accshift_app::platforms::card::UsernameBadge>,
    input: &GhostInput,
) {
    let v = input.vars;
    match node {
        Node::Folder(f) => {
            folder_card::folder_card(
                gui,
                r,
                &FolderCardProps {
                    vars: v,
                    id: g.id,
                    name: &f.folder.name,
                    card_color: card_color(input.resolved, v, &f.card_color),
                    accent: input.accent,
                    drag_over: false,
                    dragged: false,
                    focus_visible: false,
                    kb_focus: false,
                    grid_dragging: false,
                    ghost: true,
                },
            );
        }
        Node::Account(a) => {
            // Cloned before the source took `.dragging`.
            let mut item = AccountCardItem::clone(a);
            item.dragged = false;
            account_card::account_card(
                gui,
                r,
                &AccountCardProps {
                    vars: v,
                    id: g.id,
                    item: &item,
                    card_color: card_color(input.resolved, v, &a.card_color),
                    avatar: input.avatar.clone().unwrap_or(AvatarContent::Loading),
                    focus_visible: false,
                    locale: input.locale,
                    time: input.time,
                    badge,
                    extension: &[],
                    extension_disabled: true,
                    glass: input.glass,
                    kb_focus: false,
                    grid_dragging: false,
                    ghost: true,
                },
            );
        }
        _ => {}
    }
}

/// The cards of section `key` in a previous frame's items.
fn section_items(items: &[DisplayItem], key: &str) -> Vec<DisplayItem> {
    let mut out = Vec::new();
    let mut inside = false;
    for item in items {
        match item {
            DisplayItem::SectionHeader(h) => {
                if inside {
                    break;
                }
                inside = h.collapse_key == key;
            }
            other if inside => out.push(other.clone()),
            _ => {}
        }
    }
    out
}

/// `.center-msg`: `padding: 48px 0`, centred column, `--fg-muted`. Lines
/// are `(text, small)`: a plain `<p>` (16 px, line-height 1.5) or `.text-sm
/// mt-1 opacity-70` (14 px over 20 px). Returns the height.
#[allow(clippy::too_many_arguments)]
fn center_message(
    ui: &mut Ui,
    v: &Vars,
    accent: Rgba,
    to_screen: impl Fn(Rect) -> Rect,
    top: f32,
    width: f32,
    spinner: bool,
    lines: &[(String, bool)],
    now: f64,
) -> f32 {
    let mut y = top + 48.0;
    if spinner {
        // `.spinner`: 20 px ring, 2 px `--border`, top `accent`, 0.7 s turn.
        let r = to_screen(Rect::from_min_size(
            pos2((width - 20.0) / 2.0, y),
            vec2(20.0, 20.0),
        ));
        let painter = ui.painter();
        let c = r.center();
        painter.circle_stroke(c, 9.0, egui::Stroke::new(2.0, v.border.to_color32()));
        let turn = (now / 0.7).fract() as f32 * std::f32::consts::TAU;
        let start = -3.0 * std::f32::consts::FRAC_PI_4 + turn;
        let points: Vec<egui::Pos2> = (0..=12)
            .map(|k| {
                let a = start + k as f32 / 12.0 * std::f32::consts::FRAC_PI_2;
                c + vec2(a.cos(), a.sin()) * 9.0
            })
            .collect();
        painter.add(egui::Shape::line(
            points,
            egui::Stroke::new(2.0, accent.to_color32()),
        ));
        ui.ctx().request_repaint();
        y += 20.0;
    }
    for (i, (line, small)) in lines.iter().enumerate() {
        let (size, line_h, color) = if *small {
            (
                14.0,
                20.0,
                v.fg_muted
                    .alpha(v.fg_muted.a * if spinner { 1.0 } else { 0.7 }),
            )
        } else {
            (16.0, 24.0, v.fg_muted)
        };
        if *small && i > 0 {
            y += 4.0;
        }
        let style = TextStyle::new(size, 400)
            .line_height(LineHeight::Px(line_h))
            .align(TextAlign::Center)
            .color(color.to_color32());
        let r = to_screen(Rect::from_min_size(pos2(0.0, y), vec2(width, line_h)));
        let laid = text::paint(ui.painter(), r, line, &style);
        y += laid.map_or(line_h, |l| l.height.max(line_h));
    }
    y + 48.0 - top
}

/// `.error-banner`: `margin-bottom: 16px; padding: 12px; radius 8; 13px;
/// rgba(239,68,68,.1)` on `#f87171`. Returns its height with the margin.
fn error_banner(
    ui: &mut Ui,
    to_screen: impl Fn(Rect) -> Rect,
    top: f32,
    width: f32,
    message: &str,
) -> f32 {
    let style = TextStyle::new(13.0, 400)
        .line_height(LineHeight::Number(1.5))
        .color(Rgba::hex(0xf87171, 1.0).to_color32());
    let inner = Rect::from_min_size(pos2(12.0, top + 12.0), vec2(width - 24.0, 19.5));
    let text_h =
        text::layout(ui.ctx(), message, &style, to_screen(inner)).map_or(19.5, |l| l.height);
    let r = to_screen(Rect::from_min_size(
        pos2(0.0, top),
        vec2(width, text_h + 24.0),
    ));
    BoxStyle {
        background: Rgba::new(239.0, 68.0, 68.0, 0.1),
        radius: Corners::all(8.0),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), r);
    text::paint(ui.painter(), to_screen(inner), message, &style);
    text_h + 24.0 + 16.0
}

/// What the search field shows this frame.
struct SearchProps<'a> {
    value: &'a str,
    placeholder: &'a str,
    /// `color-scheme: dark`, for the clear button's bitmap.
    dark: bool,
    now: f64,
}

/// The `<input type="search">`: returns the new value when it changed.
fn search_field(
    ui: &mut Ui,
    rect: Rect,
    state: &mut Search,
    v: &Vars,
    props: SearchProps,
) -> Option<String> {
    let SearchProps {
        value,
        placeholder,
        dark,
        now,
    } = props;
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let layout = rect;
    let rect = snap::rect(rect, ppp);
    let id = Id::new(crate::app::keys::SEARCH_ID);
    let response = ui.interact(rect, id, Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::Text);
    response.widget_info(|| WidgetInfo::text_edit(true, value, value, placeholder));
    ctx.accesskit_node_builder(id, |node| {
        node.set_role(egui::accesskit::Role::SearchInput);
        node.set_label(placeholder);
    });
    if response.clicked() {
        response.request_focus();
    }
    let bw = snap::border_width(1.0, ppp);
    // `:hover` holds over the clear button too, which takes the pointer.
    let hovered = ui.rect_contains_pointer(rect);
    let cancel_rect = search_cancel::rect(rect, bw, 10.0, 12.0, ppp);
    let mut cleared = false;
    if !value.is_empty() && (response.has_focus() || hovered) {
        let cancel = ui.interact(cancel_rect, id.with("cancel"), Sense::click());
        if cancel.clicked() {
            cleared = true;
            response.request_focus();
        }
    }
    let focused = response.has_focus();
    if focused {
        crate::ui::components::focus::mark_text_field(&ctx, id);
    }
    if focused && !state.focused {
        state.blink_from = now;
    }
    state.focused = focused;

    let mut next = if cleared {
        String::new()
    } else {
        value.to_string()
    };
    if focused {
        ctx.input(|i| {
            for event in &i.events {
                match event {
                    Event::Text(t) => next.push_str(t),
                    Event::Key {
                        key: Key::Backspace,
                        pressed: true,
                        ..
                    } => {
                        next.pop();
                    }
                    // `type="search"`: Escape clears the field.
                    Event::Key {
                        key: Key::Escape,
                        pressed: true,
                        ..
                    } => next.clear(),
                    _ => {}
                }
            }
        });
        if next != value {
            state.blink_from = now;
        }
    }

    let border = if focused {
        v.fg.mix(0.35, v.border)
    } else {
        v.border
    };
    BoxStyle {
        background: v.bg_card,
        radius: Corners::all(8.0),
        border: Border::all(snap::border_width(1.0, ppp), border),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), rect);
    // The inner editor: 12 px on a `normal` line (the UA `font:` shorthand
    // resets the inherited 1.5), centred in the content box in LayoutUnits,
    // unsnapped (`padding: 0 10px` inside a 1 px border floored to device
    // pixels).
    let style = TextStyle::new(12.0, 400)
        .line_height(LineHeight::Normal)
        .nowrap(false);
    let line = style.line_box(&ctx).map_or(16.0, |l| l.height);
    let inset = bw + 10.0;
    let top = layout.top() + bw + snap::centre(SEARCH_H - 2.0 * bw, line, ppp);
    let text_rect = Rect::from_min_max(
        pos2(layout.left() + inset, top),
        pos2(layout.right() - inset, top + line),
    );
    // The editor ends at the clear button and its 1 px start margin, which
    // keep their room while hidden.
    let mut painter = ui.painter().clone();
    painter.set_clip_rect(ui.clip_rect().intersect(Rect::from_min_max(
        pos2(text_rect.left(), rect.top()),
        pos2(cancel_rect.left() - 1.0, rect.bottom()),
    )));
    let shown = next.as_str();
    if shown.is_empty() {
        let color = v.fg_subtle.mix(0.55, Rgba::TRANSPARENT);
        text::paint(
            &painter,
            text_rect,
            placeholder,
            &style.clone().color(color.to_color32()),
        );
    } else {
        text::paint(
            &painter,
            text_rect,
            shown,
            &style.clone().color(v.fg.to_color32()),
        );
    }
    if !next.is_empty() && (focused || hovered) {
        search_cancel::paint(&ctx, ui.painter(), cancel_rect, dark);
    }
    if focused {
        let phase = ((now - state.blink_from) / CARET_BLINK).floor() as i64;
        if phase % 2 == 0 {
            let x = text_rect.left() + text::measure(&ctx, shown, &style).unwrap_or(0.0);
            let x = snap::coord(x, ppp);
            let caret = Rect::from_min_max(
                pos2(x, snap::coord(text_rect.top(), ppp)),
                pos2(x + 1.0, snap::coord(text_rect.bottom(), ppp)),
            );
            painter.rect_filled(caret, 0.0, v.fg.to_color32());
        }
        let next_flip = state.blink_from + (phase + 1) as f64 * CARET_BLINK;
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(
            (next_flip - now).max(0.0),
        ));
    }
    (next != value).then_some(next)
}

/// The `::-webkit-scrollbar` of the content box, with a draggable thumb.
fn scrollbar(
    ui: &mut Ui,
    rect: Rect,
    ws: &mut Workspace,
    v: &Vars,
    content_h: f32,
    max_scroll: f32,
) {
    if max_scroll <= 0.0 {
        ws.thumb_drag = None;
        return;
    }
    let track = Rect::from_min_max(pos2(rect.right() - SCROLLBAR, rect.top()), rect.max);
    let track_h = track.height();
    let thumb_h = (track_h * track_h / content_h).round().max(SCROLLBAR);
    let travel = track_h - thumb_h;
    let top = (travel * ws.scroll.clamp(0.0, max_scroll) / max_scroll).round();
    let thumb = Rect::from_min_size(
        pos2(track.left(), track.top() + top),
        vec2(SCROLLBAR, thumb_h),
    );
    let id = Id::new("workspace-scrollbar");
    let response = ui.interact(thumb, id, Sense::drag());
    if response.drag_started() {
        if let Some(p) = response.interact_pointer_pos() {
            ws.thumb_drag = Some((p.y, ws.scroll));
        }
    }
    if response.dragged() {
        if let (Some((from, start)), Some(p)) = (ws.thumb_drag, response.interact_pointer_pos()) {
            if travel > 0.0 {
                ws.scroll = start + (p.y - from) * max_scroll / travel;
                ws.scroll_revealed = false;
            }
        }
    } else {
        ws.thumb_drag = None;
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

    #[test]
    fn restack_lifts_the_open_extension_and_sinks_fading_surfaces() {
        let ctx = Context::default();
        let mut order = Vec::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let ctx = ui.ctx().clone();
            let layer = ui.layer_id();
            let count = || shape_count(&ctx, layer);
            let mark = |n: u8| {
                ui.painter().rect_filled(
                    Rect::from_min_size(pos2(f32::from(n), 0.0), vec2(1.0, 1.0)),
                    0.0,
                    egui::Color32::from_gray(n),
                );
            };
            let from = count();
            let mut stacks = Vec::new();
            // A face; a fading surface and its face; an open extension's
            // surface and its face; a face.
            let a = count();
            mark(1);
            stacks.push(Stack {
                range: a..count(),
                surface: None,
                raised: false,
            });
            let b = count();
            mark(2);
            let fading = b..count();
            mark(3);
            stacks.push(Stack {
                range: b..count(),
                surface: Some(fading),
                raised: false,
            });
            let c = count();
            mark(4);
            let open = c..count();
            mark(5);
            stacks.push(Stack {
                range: c..count(),
                surface: Some(open),
                raised: true,
            });
            let d = count();
            mark(6);
            stacks.push(Stack {
                range: d..count(),
                surface: None,
                raised: false,
            });
            restack(&ctx, layer, from..count(), &stacks);
            order = ctx.graphics(|g| {
                g.get(layer)
                    .map(|l| {
                        l.all_entries()
                            .skip(from)
                            .map(|c| match &c.shape {
                                egui::Shape::Rect(r) => r.fill.r(),
                                _ => 0,
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            });
        });
        output.textures_delta.clear();
        assert_eq!(order, vec![2, 1, 3, 6, 4, 5]);
    }

    #[test]
    fn slide_easing_ends_where_it_should() {
        assert_eq!(cubic_out(0.0), 0.0);
        assert_eq!(cubic_out(1.0), 1.0);
        assert!(cubic_out(0.5) > 0.8);
    }

    #[test]
    fn section_items_takes_one_section() {
        use accshift_app::display::SectionHeaderItem;
        let header = |key: &str| {
            DisplayItem::SectionHeader(SectionHeaderItem {
                label: SectionLabel::Root,
                collapse_key: key.into(),
                count: 1,
                card_color: String::new(),
                collapsed: false,
                dragged: false,
            })
        };
        let items = vec![
            header("a"),
            DisplayItem::Back,
            header("b"),
            DisplayItem::Back,
        ];
        assert_eq!(section_items(&items, "a"), vec![DisplayItem::Back]);
        assert_eq!(section_items(&items, "c"), Vec::<DisplayItem>::new());
    }
}

/// The workspace driven like a scene run, without a window: the app on the
/// mock, frames on the wall clock, pointer events by hand.
#[cfg(test)]
mod scene_tests {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use egui::{pos2, vec2, Context, Event, Key, Modifiers, PointerButton, Pos2, RawInput};

    use crate::harness;
    use crate::screens::Screen;

    type Box4 = (String, f32, f32, f32, f32);

    pub(super) struct Run {
        pub ctx: Context,
        pub screen: Screen,
        start: Instant,
        events: Vec<Event>,
        scale: f32,
    }

    impl Run {
        pub fn new(scene: &str, scale: f32) -> Run {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("parity/scenes")
                .join(format!("{scene}.json"));
            let scene = harness::load(&path).unwrap();
            let ctx = Context::default();
            let screen = Screen::for_scene(&ctx, &scene).unwrap();
            let mut run = Run {
                ctx,
                screen,
                start: Instant::now(),
                events: Vec::new(),
                scale,
            };
            run.frame();
            run
        }

        pub fn input(&mut self) -> RawInput {
            let mut input = RawInput {
                screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(860.0, 440.0))),
                time: Some(self.start.elapsed().as_secs_f64()),
                events: std::mem::take(&mut self.events),
                ..Default::default()
            };
            input
                .viewports
                .entry(egui::ViewportId::ROOT)
                .or_default()
                .native_pixels_per_point = Some(self.scale);
            input
        }

        pub fn frame(&mut self) -> egui::FullOutput {
            let input = self.input();
            let screen = &mut self.screen;
            let mut out = self.ctx.run_ui(input, |ui| screen.ui(ui));
            out.textures_delta.clear();
            out
        }

        /// Frames until the screen is ready three frames in a row.
        pub fn settle(&mut self) {
            let deadline = Instant::now() + Duration::from_secs(20);
            let mut streak = 0;
            while streak < 3 {
                assert!(Instant::now() < deadline, "never settled");
                self.frame();
                streak = if self.screen.ready() { streak + 1 } else { 0 };
                std::thread::sleep(Duration::from_millis(4));
            }
        }

        pub fn send(&mut self, event: Event) {
            self.events.push(event);
        }

        pub fn click(&mut self, at: Pos2) {
            self.send(Event::PointerMoved(at));
            self.frame();
            for pressed in [true, false] {
                self.send(Event::PointerButton {
                    pos: at,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                });
                self.frame();
                std::thread::sleep(Duration::from_millis(40));
            }
            self.send(Event::PointerMoved(pos2(250.0, 18.0)));
            self.frame();
            self.settle();
        }

        /// A right click: `contextmenu` fires on the release.
        pub fn rclick(&mut self, at: Pos2) {
            self.send(Event::PointerMoved(at));
            self.frame();
            for pressed in [true, false] {
                self.send(Event::PointerButton {
                    pos: at,
                    button: PointerButton::Secondary,
                    pressed,
                    modifiers: Modifiers::NONE,
                });
                self.frame();
            }
            self.frame();
        }

        pub fn key(&mut self, key: Key) {
            for pressed in [true, false] {
                self.send(Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                });
                self.frame();
            }
        }

        pub fn workspace(&self) -> &super::Workspace {
            match &self.screen {
                Screen::App(app) => &app.booted().unwrap().workspace,
                _ => panic!("not the app"),
            }
        }

        /// `(label, x, y, w, h)` of every box on screen, scrolled.
        pub fn boxes(&self) -> Vec<Box4> {
            let ws = self.workspace();
            // `.content` at (0, 36), padding 10 16.
            ws.layout
                .iter()
                .map(|(label, r)| {
                    (
                        label.clone(),
                        r.left() + super::PAD_X,
                        r.top() + 36.0 + super::PAD_TOP - ws.scroll(),
                        r.width(),
                        r.height(),
                    )
                })
                .collect()
        }
    }

    /// Rows of 7 cards from `(45, first_y)`, 110 by 146 apart, all 100 by
    /// 136: what the WebView2 DOM dump gives for these scenes.
    fn cards(labels: &[&str], first_y: f32) -> Vec<Box4> {
        labels
            .iter()
            .enumerate()
            .map(|(i, l)| {
                (
                    l.to_string(),
                    45.0 + 110.0 * (i % 7) as f32,
                    first_y + 146.0 * (i / 7) as f32,
                    100.0,
                    136.0,
                )
            })
            .collect()
    }

    fn only_cards(boxes: Vec<Box4>) -> Vec<Box4> {
        boxes.into_iter().filter(|b| b.4 == 136.0).collect()
    }

    /// Card order and positions equal the reference's DOM rects
    /// (`getBoundingClientRect` in the `workspace-*` reference scenes).
    #[test]
    fn golden_card_positions() {
        let mut run = Run::new("workspace-glass-dark-100", 1.0);
        run.settle();
        let root = [
            "Smurfs",
            "main",
            "bro\u{27}s account",
            "alt",
            "trading",
            "faceit",
            "ranked grind",
            "chill",
            "lan party",
            "streams",
            "esea",
            "tournaments",
            "family",
            "practice",
        ];
        assert_eq!(only_cards(run.boxes()), cards(&root, 84.0));
        run.click(pos2(95.0, 152.0));
        let smurfs = ["Back", "smurf 1", "smurf 2", "smurf 3", "smurf 4"];
        assert_eq!(only_cards(run.boxes()), cards(&smurfs, 84.0));
        run.click(pos2(95.0, 152.0));
        run.click(pos2(639.0, 60.0));
        run.send(Event::Text("r".into()));
        run.frame();
        run.send(Event::Text("a".into()));
        run.frame();
        run.settle();
        let search = ["trading", "ranked grind", "practice"];
        assert_eq!(only_cards(run.boxes()), cards(&search, 84.0));

        let mut run = Run::new("workspace-sections-glass-dark-100", 1.0);
        run.settle();
        let boxes = run.boxes();
        let headers: Vec<Box4> = boxes.iter().filter(|b| b.4 != 136.0).cloned().collect();
        assert_eq!(
            headers,
            vec![
                ("Smurfs".to_string(), 45.0, 84.0, 789.0, 24.0),
                ("Root".to_string(), 45.0, 274.0, 789.0, 23.0),
            ]
        );
        let all = only_cards(boxes);
        assert_eq!(
            all[..3],
            cards(&["Ranked", "smurf 1", "smurf 2"], 116.0)[..]
        );
        let root13 = &root[1..];
        assert_eq!(all[3..], cards(root13, 305.0)[..]);
        // `scrollHeight` 567.
        assert_eq!(run.workspace().content_height, 567.0);

        let mut run = Run::new("workspace-deep-glass-dark-100", 1.0);
        run.settle();
        run.click(pos2(95.0, 152.0));
        run.click(pos2(205.0, 152.0));
        run.click(pos2(205.0, 152.0));
        assert_eq!(only_cards(run.boxes()), cards(&["Back", "smurf 4"], 84.0));

        let mut run = Run::new("workspace-huge-100", 1.0);
        run.settle();
        assert_eq!(run.workspace().content_height, 4288.0);
        run.send(Event::PointerMoved(pos2(430.0, 260.0)));
        run.send(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -1942.0),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
        run.frame();
        run.settle();
        assert_eq!(run.workspace().scroll(), 1942.0);
        let boxes = only_cards(run.boxes());
        assert_eq!(boxes.len(), 200);
        assert_eq!(
            boxes[0],
            ("account 1".to_string(), 45.0, -1858.0, 100.0, 136.0)
        );
        assert_eq!(boxes[7].2, -1712.0);
    }

    /// The "Settings saved" toast of an inactivity timeout edit, as in the
    /// `pin-setup` shot: hovering it pauses its countdown, leaving resumes
    /// it, its cross removes it.
    #[test]
    fn toast_pauses_on_hover_and_closes_on_its_cross() {
        // Frames on the wall clock, like a scene's `wait`.
        fn wait(run: &mut Run, ms: u64) {
            let until = Instant::now() + Duration::from_millis(ms);
            while Instant::now() < until {
                run.frame();
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let mut run = Run::new("settings-glass-dark-100", 1.0);
        run.settle();
        wait(&mut run, 600);
        // The steps and waits of `settings-glass-dark-100`.
        run.click(pos2(87.0, 18.0));
        wait(&mut run, 700);
        run.click(pos2(93.0, 176.0));
        run.click(pos2(350.0, 192.0));
        run.send(Event::Text("45".into()));
        run.frame();
        // The PIN toggle blurs the field: `change` commits it, and the save
        // toast follows `queueSave`'s 220 ms debounce.
        run.click(pos2(766.0, 167.0));
        wait(&mut run, 400);
        let toasts = |run: &Run| match &run.screen {
            Screen::App(app) => app.toasts().toasts().to_vec(),
            _ => panic!("not the app"),
        };
        let shown = toasts(&run);
        assert_eq!(shown.len(), 1);
        assert!(shown[0].deadline().is_some());
        // The reference draws the toast at 694..850 x 386..424.
        run.send(Event::PointerMoved(pos2(760.0, 405.0)));
        run.frame();
        run.frame();
        assert!(toasts(&run)[0].hovered());
        assert_eq!(toasts(&run)[0].deadline(), None);
        run.send(Event::PointerMoved(pos2(430.0, 5.0)));
        run.frame();
        run.frame();
        assert!(toasts(&run)[0].deadline().is_some());
        run.click(pos2(821.0, 405.0));
        assert!(toasts(&run).is_empty());
    }

    /// `streamer-*` scenes: the mock reports OBS running, the overlay
    /// covers the app and swallows its input; "Disable" hides it for the
    /// session, "Disable permanently" also saves `streamerMode: "off"`.
    #[test]
    fn streamer_mode_covers_the_app_until_disabled() {
        use accshift_app::settings::StreamerMode;
        let state = |run: &Run| match &run.screen {
            Screen::App(app) => {
                let b = app.booted().unwrap();
                (
                    b.streamer.active(),
                    b.stores.settings().streamer_mode,
                    b.nav.show_settings,
                )
            }
            _ => panic!("not the app"),
        };
        let wait_active = |run: &mut Run| {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !state(run).0 {
                assert!(Instant::now() < deadline, "streamer mode never came up");
                run.frame();
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        let mut run = Run::new("streamer-glass-dark-100", 1.0);
        run.settle();
        wait_active(&mut run);
        // The settings gear under the overlay gets nothing.
        run.click(pos2(87.0, 18.0));
        assert_eq!(state(&run), (true, StreamerMode::Auto, false));
        run.click(pos2(351.0, 268.0));
        assert_eq!(state(&run), (false, StreamerMode::Auto, false));

        let mut run = Run::new("streamer-glass-dark-100", 1.0);
        run.settle();
        wait_active(&mut run);
        run.click(pos2(472.0, 268.0));
        assert_eq!(state(&run), (false, StreamerMode::Off, false));
    }

    /// The account menu of the `menu-*` scenes: Escape and a press outside
    /// close it, hovering "Copy..." opens its submenu at once, and an entry
    /// there copies with a toast (the demo usernames are empty, so SteamID64).
    #[test]
    fn context_menu_opens_closes_and_copies() {
        use crate::ui::components::context_menu;
        let open = |run: &Run| match &run.screen {
            Screen::App(app) => app.dialogs().context_menu().is_some(),
            _ => panic!("not the app"),
        };
        let mut run = Run::new("menu-glass-dark-100", 1.0);
        run.settle();
        run.rclick(pos2(205.0, 152.0));
        assert!(open(&run));
        run.key(Key::Escape);
        assert!(!open(&run));
        run.rclick(pos2(205.0, 152.0));
        assert!(open(&run));
        // The reference draws the menu at 205..425 x 111..436.
        assert!(context_menu::contains(&run.ctx, pos2(300.0, 300.0)));
        run.click(pos2(845.0, 425.0));
        assert!(!open(&run));
        // "Copy..." at 210..420 x 254..284, its submenu right of the menu.
        run.rclick(pos2(205.0, 152.0));
        run.send(Event::PointerMoved(pos2(260.0, 269.0)));
        run.frame();
        run.frame();
        assert!(context_menu::contains(&run.ctx, pos2(500.0, 270.0)));
        run.send(Event::PointerMoved(pos2(500.0, 305.0)));
        run.frame();
        let mut copied = Vec::new();
        for pressed in [true, false] {
            run.send(Event::PointerButton {
                pos: pos2(500.0, 305.0),
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            for _ in 0..2 {
                let out = run.frame();
                copied.extend(
                    out.platform_output
                        .commands
                        .into_iter()
                        .filter_map(|c| match c {
                            egui::OutputCommand::CopyText(text) => Some(text),
                            _ => None,
                        }),
                );
            }
        }
        assert_eq!(copied, ["76561198000000001"]);
        assert!(!open(&run));
        let Screen::App(app) = &run.screen else {
            panic!("not the app")
        };
        assert_eq!(app.toasts().toasts().len(), 1);
        // The same entry by keyboard: ArrowRight opens "Copy..." and focuses
        // its first entry.
        run.rclick(pos2(205.0, 152.0));
        let mut copied = Vec::new();
        for key in
            [Key::ArrowDown; 4]
                .into_iter()
                .chain([Key::ArrowRight, Key::ArrowDown, Key::Enter])
        {
            for pressed in [true, false] {
                run.send(Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                });
                let out = run.frame();
                copied.extend(
                    out.platform_output
                        .commands
                        .into_iter()
                        .filter_map(|c| match c {
                            egui::OutputCommand::CopyText(text) => Some(text),
                            _ => None,
                        }),
                );
            }
        }
        assert_eq!(copied, ["76561198000000001"]);
        assert!(!open(&run));
    }

    /// The input and confirm dialogs of the `dialog-*` scenes, driven like a
    /// user: Escape and the overlay cancel, Tab cycles inside, Enter submits
    /// the new folder, and the delete confirm opens on Cancel.
    #[test]
    fn dialogs_cancel_cycle_and_submit() {
        use crate::ui::components::dialog;
        use egui::Id;

        let app = |run: &Run| match &run.screen {
            Screen::App(app) => (
                app.dialogs().input_dialog().map(|d| d.value().to_string()),
                app.dialogs().confirm_dialog().is_some(),
                app.dialog_mounts(),
            ),
            _ => panic!("not the app"),
        };
        let focused = |run: &Run| run.ctx.memory(|m| m.focused());
        let labels = |run: &Run| -> Vec<String> { run.boxes().into_iter().map(|b| b.0).collect() };
        let tab = |run: &mut Run, shift: bool| {
            for pressed in [true, false] {
                run.send(Event::Key {
                    key: Key::Tab,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: if shift {
                        Modifiers::SHIFT
                    } else {
                        Modifiers::NONE
                    },
                });
                run.frame();
            }
        };
        // The scene's "New folder": the empty grid's menu, its only entry.
        let new_folder = |run: &mut Run| {
            run.rclick(pos2(430.0, 400.0));
            run.click(pos2(474.0, 416.0));
        };

        let mut run = Run::new("dialog-light-100", 1.0);
        run.settle();
        new_folder(&mut run);
        let (value, _, (Some(mount), _)) = app(&run) else {
            panic!("no input dialog")
        };
        assert_eq!(value.as_deref(), Some(""));
        let ids = Id::new(dialog::INPUT_ID).with(mount);
        assert_eq!(focused(&run), Some(ids.with("input")));
        run.send(Event::Text("Ranked".into()));
        run.frame();
        assert_eq!(app(&run).0.as_deref(), Some("Ranked"));
        // The field, Cancel, OK, and around.
        tab(&mut run, false);
        assert_eq!(focused(&run), Some(ids.with("cancel")));
        tab(&mut run, false);
        assert_eq!(focused(&run), Some(ids.with("ok")));
        tab(&mut run, false);
        assert_eq!(focused(&run), Some(ids.with("input")));
        tab(&mut run, true);
        assert_eq!(focused(&run), Some(ids.with("ok")));
        run.key(Key::Escape);
        assert_eq!(app(&run).0, None);
        assert!(!labels(&run).iter().any(|l| l == "Ranked"));

        // A press on the overlay, outside the 290..570 x 146..294 box.
        new_folder(&mut run);
        assert!(app(&run).0.is_some());
        run.click(pos2(40.0, 120.0));
        assert_eq!(app(&run).0, None);

        new_folder(&mut run);
        run.send(Event::Text("Ranked".into()));
        run.frame();
        run.key(Key::Enter);
        run.settle();
        assert_eq!(app(&run).0, None);
        assert!(labels(&run).iter().any(|l| l == "Ranked"));

        // "Delete folder", the folder menu's last entry, opens on Cancel:
        // Enter there keeps the folder, Tab then Enter deletes it.
        let delete = |run: &mut Run| {
            run.rclick(pos2(95.0, 152.0));
            run.key(Key::End);
            run.key(Key::Enter);
            run.frame();
        };
        let smurfs = |run: &Run| labels(run).iter().any(|l| l == "Smurfs");
        assert!(smurfs(&run));
        delete(&mut run);
        let (_, true, (_, Some(mount))) = app(&run) else {
            panic!("no confirm dialog")
        };
        let ids = Id::new(dialog::CONFIRM_ID).with(mount);
        assert_eq!(focused(&run), Some(ids.with("cancel")));
        run.key(Key::Enter);
        run.settle();
        assert!(!app(&run).1);
        assert!(smurfs(&run));
        delete(&mut run);
        tab(&mut run, false);
        let mount = app(&run).2 .1.unwrap();
        let ids = Id::new(dialog::CONFIRM_ID).with(mount);
        assert_eq!(focused(&run), Some(ids.with("confirm")));
        run.key(Key::Enter);
        run.settle();
        assert!(!app(&run).1);
        assert!(!smurfs(&run));
    }

    /// "Forget" on the `main` card, confirmed with its button: the task runs
    /// on the pool and its failure becomes a toast. The mock refuses
    /// `platform_forget_account` like `mockInvoke` does, so the card stays
    /// (TS prints `Error: ` before it, `String()` of a thrown `Error`; the
    /// real backend rejects with the bare message).
    #[test]
    fn forget_confirm_runs_the_task() {
        use accshift_app::text::Text;
        let labels = |run: &Run| -> Vec<String> { run.boxes().into_iter().map(|b| b.0).collect() };
        let state = |run: &Run| match &run.screen {
            Screen::App(app) => (
                app.dialogs().confirm_dialog().is_some(),
                app.toasts()
                    .toasts()
                    .iter()
                    .map(|t| t.text.clone())
                    .collect::<Vec<_>>(),
            ),
            _ => panic!("not the app"),
        };
        let mut run = Run::new("dialog-light-100", 1.0);
        run.settle();
        // The scene's "Forget" entry, then the dialog's red button at the
        // right end of its actions row (250..610 x 242..274).
        run.rclick(pos2(205.0, 152.0));
        run.click(pos2(240.0, 377.0));
        assert!(state(&run).0);
        run.click(pos2(575.0, 258.0));
        let deadline = Instant::now() + Duration::from_secs(5);
        while state(&run).1.is_empty() {
            assert!(Instant::now() < deadline, "the task never landed");
            run.frame();
            std::thread::sleep(Duration::from_millis(10));
        }
        let (open, toasts) = state(&run);
        assert!(!open);
        assert_eq!(
            toasts,
            [Text::raw(
                "mock mode: platform_forget_account is not available"
            )]
        );
        assert!(labels(&run).iter().any(|l| l == "main"));

        // What a successful forget lands: `removeAccount` takes the card out.
        let Screen::App(app) = &mut run.screen else {
            panic!("not the app")
        };
        let b = app.booted_mut().unwrap();
        b.accounts
            .remove_account(&mut b.stores, &b.nav.active_tab, "76561198000000001");
        run.frame();
        run.settle();
        let left = labels(&run);
        assert!(!left.iter().any(|l| l == "main"));
        assert!(left.iter().any(|l| l == "alt"));
    }

    /// The streamer mode row of the privacy tab answers the pointer: the
    /// telemetry card under it once took its id, and its hit box with it.
    #[test]
    fn privacy_toggle_keeps_its_hit_box() {
        let mut run = Run::new("settings-glass-dark-100", 1.0);
        run.settle();
        run.click(pos2(87.0, 18.0));
        run.click(pos2(93.0, 176.0));
        run.send(Event::PointerMoved(pos2(448.0, 267.0)));
        run.frame();
        run.frame();
        let hovered = run.ctx.viewport(|vp| {
            vp.interact_widgets.hovered.iter().any(|id| {
                vp.prev_pass
                    .widgets
                    .get(*id)
                    .is_some_and(|w| w.interact_rect.contains(pos2(448.0, 267.0)))
            })
        });
        assert!(hovered);
    }

    /// The huge scenario scrolled down and back with the CPU renderer:
    /// median and p99 of the whole frame (ui, tessellation, raster) over 300
    /// frames. `cargo test --release --lib huge_scroll -- --ignored
    /// --nocapture`.
    #[cfg(feature = "software")]
    #[test]
    #[ignore]
    fn huge_scroll_frame_time() {
        use egui_software_backend::{BufferMutRef, ColorFieldOrder, EguiSoftwareRender};
        for scale in [1.0f32, 1.5] {
            let mut run = Run::new("workspace-huge-100", scale);
            run.settle();
            let (w, h) = ((860.0 * scale) as usize, (440.0 * scale) as usize);
            let mut pixels = vec![[0u8; 4]; w * h];
            let mut render = EguiSoftwareRender::new(ColorFieldOrder::Bgra)
                .with_glyph_coverage(crate::ui::text::coverage_tables());
            let mut times = Vec::new();
            let mut textures = egui::TexturesDelta::default();
            for i in 0..330 {
                // 24 px a frame down, then back up.
                let dy = if (i / 165) % 2 == 0 { 24.0 } else { -24.0 };
                run.send(Event::PointerMoved(pos2(430.0, 260.0)));
                run.send(Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: vec2(0.0, -dy),
                    phase: egui::TouchPhase::Move,
                    modifiers: Modifiers::NONE,
                });
                let started = Instant::now();
                let input = run.input();
                let screen = &mut run.screen;
                let mut out = run.ctx.run_ui(input, |ui| screen.ui(ui));
                textures.append(std::mem::take(&mut out.textures_delta));
                let prims = run.ctx.tessellate(out.shapes, out.pixels_per_point);
                let mut buffer = BufferMutRef::new(&mut pixels, w, h);
                render.render(&mut buffer, &prims, &textures, out.pixels_per_point);
                textures.clear();
                if i >= 30 {
                    times.push(started.elapsed().as_secs_f64() * 1000.0);
                }
            }
            times.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let median = times[times.len() / 2];
            let p99 = times[(times.len() * 99).div_ceil(100) - 1];
            println!(
                "huge scroll @{scale}: {} frames, median {median:.2} ms, p99 {p99:.2} ms, max {:.2} ms",
                times.len(),
                times[times.len() - 1]
            );
        }
    }

    /// The `list-*` scenes: the toggle shows the rows where the reference
    /// put them, a click previews, a right click opens the account menu, a
    /// folder row goes in (dropping the preview) and the back row comes out.
    #[test]
    fn list_view_previews_opens_menus_and_navigates() {
        let open = |run: &Run| match &run.screen {
            Screen::App(app) => app.dialogs().context_menu().is_some(),
            _ => panic!("not the app"),
        };
        let labels = |run: &Run| -> Vec<String> {
            run.workspace()
                .list
                .layout
                .iter()
                .map(|(l, _)| l.clone())
                .collect()
        };
        let mut run = Run::new("list-glass-dark-100", 1.0);
        run.settle();
        run.click(pos2(816.0, 60.0));
        assert_eq!(
            labels(&run)[..4],
            ["Smurfs", "main", "bro\u{27}s account", "alt"]
        );
        // `.list-wrapper` at (16, 84), 818 by 340: rows of 599 by 46 from
        // (20, 88), 48 apart.
        let rows: Vec<egui::Rect> = run
            .workspace()
            .list
            .layout
            .iter()
            .map(|(_, r)| *r)
            .collect();
        assert_eq!(
            rows[1],
            egui::Rect::from_min_size(pos2(20.0, 136.0), vec2(599.0, 46.0))
        );
        assert_eq!(rows[2].top(), 184.0);

        run.click(pos2(300.0, 159.0));
        assert_eq!(
            run.workspace().list.selected.as_deref(),
            Some("76561198000000001")
        );
        run.rclick(pos2(300.0, 207.0));
        assert!(open(&run));
        run.key(Key::Escape);
        assert!(!open(&run));

        run.click(pos2(300.0, 111.0));
        let inside = labels(&run);
        assert_eq!(inside[0], "Back");
        assert!(inside.iter().any(|l| l == "smurf 1"));
        assert_eq!(run.workspace().list.selected, None);
        run.click(pos2(300.0, 111.0));
        assert_eq!(labels(&run)[0], "Smurfs");
    }

    /// The app's binding table and the command palette: Ctrl+K opens it on
    /// its focused field, typing filters, Enter runs, Escape and a second
    /// Ctrl+K close, the arrows move the highlight; the tab, view and
    /// search shortcuts reach the app.
    #[test]
    fn card_focus_follows_the_arrows() {
        fn app(run: &Run) -> &crate::app::AppModel {
            match &run.screen {
                Screen::App(app) => app,
                _ => panic!("not the app"),
            }
        }
        let focus = |run: &Run| app(run).booted().unwrap().workspace.kb_focus.clone();
        let label = |run: &Run| {
            let b = app(run).booted().unwrap();
            let id = b.workspace.kb_focus.clone()?;
            b.accounts.view.items().iter().find_map(|item| match item {
                accshift_app::display::DisplayItem::Folder(f) if f.folder.id == id => {
                    Some(f.folder.name.clone())
                }
                accshift_app::display::DisplayItem::Account(a) if a.account.id == id => {
                    Some(a.name.clone())
                }
                _ => None,
            })
        };
        let mut run = Run::new("workspace-glass-dark-100", 1.0);
        run.settle();
        assert_eq!(focus(&run), None);
        // The first arrow lands on the first card, the next ones move by
        // one or by a row of seven (the folder takes the first slot).
        run.key(Key::ArrowRight);
        assert_eq!(label(&run).as_deref(), Some("Smurfs"));
        run.key(Key::ArrowRight);
        assert_eq!(label(&run).as_deref(), Some("main"));
        run.key(Key::ArrowDown);
        assert_eq!(label(&run).as_deref(), Some("lan party"));
        // Past the last row it stays.
        run.key(Key::ArrowDown);
        assert_eq!(label(&run).as_deref(), Some("lan party"));
        run.key(Key::ArrowUp);
        assert_eq!(label(&run).as_deref(), Some("main"));
        // Escape clears, then a pointer press does.
        run.key(Key::Escape);
        assert_eq!(focus(&run), None);
        run.key(Key::ArrowRight);
        assert!(focus(&run).is_some());
        run.click(pos2(840.0, 420.0));
        assert_eq!(focus(&run), None);
        // Delete opens the menu of the focused card at its centre, F2 the
        // rename dialog of a folder.
        run.key(Key::ArrowRight);
        run.key(Key::Delete);
        assert!(
            app(&run).dialogs().context_menu().is_some(),
            "Delete opens the menu"
        );
        run.key(Key::Escape);
        assert!(app(&run).dialogs().context_menu().is_none());
        assert_eq!(
            label(&run).as_deref(),
            Some("Smurfs"),
            "the menu keeps the focus"
        );
        run.key(Key::F2);
        assert!(
            app(&run).dialogs().input_dialog().is_some(),
            "F2 renames the folder"
        );
        run.key(Key::Escape);
        // Enter opens the focused folder, and the new folder clears it.
        run.key(Key::Enter);
        let b = app(&run).booted().unwrap();
        assert!(b.accounts.navigation.current_folder_id().is_some());
        assert_eq!(focus(&run), None);
    }

    #[test]
    fn keyboard_shortcuts_and_the_palette() {
        use crate::ui::components::command_palette;
        use accshift_app::stores::ViewMode;

        let chord = |run: &mut Run, key: Key, modifiers: Modifiers| {
            for pressed in [true, false] {
                run.send(Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed,
                    repeat: false,
                    modifiers,
                });
                run.frame();
            }
        };
        fn app(run: &Run) -> &crate::app::AppModel {
            match &run.screen {
                Screen::App(app) => app,
                _ => panic!("not the app"),
            }
        }
        let palette = |run: &Run| app(run).palette().map(|(m, q, a)| (m, q.to_string(), a));
        let nav = |run: &Run| app(run).booted().unwrap().nav.clone();
        let view_mode = |run: &Run| app(run).booted().unwrap().stores.view_mode();
        let ctrl = Modifiers::CTRL;
        let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;

        let mut run = Run::new("workspace-glass-dark-100", 1.0);
        run.settle();
        assert_eq!(palette(&run), None);
        chord(&mut run, Key::K, ctrl);
        let (mount, query, active) = palette(&run).expect("Ctrl+K opens the palette");
        assert_eq!((query.as_str(), active), ("", 0));
        let focused = run.ctx.memory(|m| m.focused());
        assert_eq!(focused, Some(command_palette::input_id(mount)));
        // The arrows wrap; End and Home go to the ends of an idle list.
        run.key(Key::ArrowUp);
        let last = palette(&run).unwrap().2;
        assert!(last > 5, "the idle list holds accounts, actions and tabs");
        run.key(Key::ArrowDown);
        assert_eq!(palette(&run).unwrap().2, 0);
        run.key(Key::End);
        assert_eq!(palette(&run).unwrap().2, last);
        run.key(Key::Home);
        assert_eq!(palette(&run).unwrap().2, 0);
        // A second Ctrl+K closes, Escape too.
        chord(&mut run, Key::K, ctrl);
        assert_eq!(palette(&run), None);
        chord(&mut run, Key::K, ctrl);
        assert!(palette(&run).is_some_and(|p| p.0 > mount), "a fresh mount");
        run.key(Key::Escape);
        assert_eq!(palette(&run), None);
        // Typing filters and resets the highlight; Enter runs the first
        // match, here the settings, and closes.
        chord(&mut run, Key::K, ctrl);
        run.key(Key::ArrowDown);
        run.send(Event::Text("settings".into()));
        run.frame();
        assert_eq!(
            palette(&run).map(|p| (p.1, p.2)),
            Some(("settings".into(), 0))
        );
        run.key(Key::Enter);
        assert_eq!(palette(&run), None);
        assert!(nav(&run).show_settings);
        chord(&mut run, Key::Comma, ctrl);
        assert!(!nav(&run).show_settings);
        // A press on the overlay outside the box closes.
        chord(&mut run, Key::K, ctrl);
        run.click(pos2(30.0, 400.0));
        assert_eq!(palette(&run), None);

        // Tabs: Ctrl+Tab cycles steam, riot, roblox; Ctrl+3 jumps.
        assert_eq!(nav(&run).active_tab, "steam");
        chord(&mut run, Key::Tab, ctrl);
        assert_eq!(nav(&run).active_tab, "riot");
        chord(&mut run, Key::Tab, ctrl_shift);
        assert_eq!(nav(&run).active_tab, "steam");
        chord(&mut run, Key::Num3, ctrl);
        assert_eq!(nav(&run).active_tab, "roblox");
        chord(&mut run, Key::Num1, ctrl);
        assert_eq!(nav(&run).active_tab, "steam");
        run.settle();

        // Ctrl+Shift+L flips the view.
        assert_eq!(view_mode(&run), ViewMode::Grid);
        chord(&mut run, Key::L, ctrl_shift);
        assert_eq!(view_mode(&run), ViewMode::List);
        chord(&mut run, Key::L, ctrl_shift);
        assert_eq!(view_mode(&run), ViewMode::Grid);

        // Ctrl+F focuses the search; Escape clears and blurs it.
        let search = egui::Id::new(crate::app::keys::SEARCH_ID);
        chord(&mut run, Key::F, ctrl);
        assert!(run.ctx.memory(|m| m.has_focus(search)));
        run.send(Event::Text("zz".into()));
        run.frame();
        let query = |run: &Run| {
            app(run)
                .booted()
                .unwrap()
                .accounts
                .navigation
                .search_query()
                .to_string()
        };
        assert_eq!(query(&run), "zz");
        run.key(Key::Escape);
        assert_eq!(query(&run), "");
        assert!(!run.ctx.memory(|m| m.has_focus(search)));
    }
}
