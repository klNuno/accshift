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
use accshift_app::display::{
    AccountCardItem, DisplayBody, DisplayItem, FolderCardItem, SectionHeaderItem, SectionLabel,
};
use accshift_app::i18n::{translate, Locale};
use accshift_app::jobs::Jobs;
use accshift_app::stores::{ClientStores, ViewMode};
use accshift_theme::ResolvedTheme;
use egui::{pos2, vec2, Event, Id, Key, Rect, Sense, Ui, WidgetInfo, WidgetType};

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
use crate::ui::components::folder_card::{self, entrance, FolderCardEvent, FolderCardProps};
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
    pub now: f64,
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
}

impl Workspace {
    pub fn new() -> Self {
        Self::default()
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

    let mut slides = std::mem::take(&mut ws.slides);
    let mut placed: Vec<Placed> = Vec::new();
    // Everything above the cards, in content coordinates.
    let mut y = 0.0;
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
        ..
    } = &mut *input.accounts;
    let tab = input.platform.map_or("", |p| p.id.as_str());
    let adapter_ready = registry.adapter(tab).is_some();
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
            let search_w = SEARCH_W.min(0.38 * ctx.content_rect().width());
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
                DisplayBody::Items { .. } if view_mode == ViewMode::List => {
                    // The list view is a later port.
                    let lines = vec![(translate(input.locale, "view.list"), true)];
                    y += center_message(ui, v, accent, to_screen, y, inner_w, false, &lines, now);
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
                            });
                            y = top + 20.0;
                        } else {
                            y += h;
                        }
                    }
                }
            }
        }
    }
    let content_h = PAD_TOP + y + PAD_BOTTOM;
    ws.content_height = content_h;

    // Draw the cards.
    let view_rect = Rect::from_min_size(pos2(0.0, scroll - PAD_TOP), viewport.size());
    let avatar_px = (v.grid_card_avatar_size() * ppp).round() as u32;
    ws.visible_decoding = 0;
    ws.layout.clear();
    let mut focused_rect = None;
    let focused_now = ctx.memory(|m| m.focused());
    let time = accshift_app::time::TimeContext {
        now_ms: clock.now_ms(),
        locale: input.locale,
        zone: ws
            .zone
            .get_or_insert_with(accshift_app::time::system_zone)
            .clone(),
    };
    for p in &placed {
        ws.layout.push((p.node.label(), p.rect));
        let id = ws.mount(&p.key, frame, now);
        let visible = p
            .clip
            .map_or(p.rect, |c| p.rect.intersect(c))
            .expand(32.0)
            .intersects(view_rect)
            && p.clip.is_none_or(|c| c.height() > 0.0);
        let screen = to_screen(p.rect);
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
                        drag_over: false,
                        focus_visible: input.focus_visible,
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
                        drag_over: false,
                        dragged: f.dragged,
                        focus_visible: input.focus_visible,
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
                for event in account_card::account_card(
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
                        badge: None,
                    },
                ) {
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
    }
    if focused_now != ws.focused {
        ws.focused = focused_now;
        if let Some(r) = focused_rect {
            let top = r.top() + PAD_TOP - 2.0;
            let bottom = r.bottom() + PAD_TOP + 2.0;
            if top < ws.scroll {
                ws.scroll = top;
            } else if bottom > ws.scroll + viewport.height() {
                ws.scroll = bottom - viewport.height();
            }
        }
    }
    scrollbar(ui, rect, ws, v, content_h, max_scroll);
    ws.scroll = ws.scroll.clamp(0.0, max_scroll).round();
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
    }
    y - top
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
    let id = Id::new("workspace-search");
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
}
