//! What the content box shows above and instead of the cards: the centre
//! messages, the toolbar row and the error banner; then the list view, or
//! the grid laid out for the card pass.

use accshift_app::display::{AccountCardItem, DisplayBody, DisplayItem};
use accshift_app::folders::FolderSection;
use accshift_app::i18n::{translate, Locale};
use accshift_app::stores::ViewMode;
use egui::{pos2, vec2, Id, Rect, Ui, Vec2};

use super::cards::{card_action, card_color};
use super::layout::{flow, lay_sections, Folds, Grid, Laid, Node, Placed};
use super::search::{search_field, Search, SearchProps, SEARCH_H};
use super::{Frame, Workspace, PAD_BOTTOM, PAD_TOP};
use crate::app::avatars::AvatarTextures;
use crate::app::Action;
use crate::theme::Rgba;
use crate::ui::components::account_card::{self, AvatarContent};
use crate::ui::components::breadcrumb::{self, BreadcrumbProps};
use crate::ui::components::list_view::{self, ListHost, ListProps};
use crate::ui::components::use_grid_layout::{grid_layout, GAP};
use crate::ui::components::view_toggle::{self, ViewToggleProps};
use crate::ui::paint::{BoxStyle, Corners};
use crate::ui::text::{self, LineHeight, TextAlign, TextStyle};

/// Gap between the toolbar controls, and under the toolbar row.
const TOOLBAR_GAP: f32 = 8.0;
/// The search field's widest width; it also takes at most 38 % of the
/// window.
const SEARCH_W: f32 = 240.0;
/// Room kept free at the bottom of the grid and the list for the bulk edit
/// bar.
const BULK_PAD: f32 = 52.0;

/// A centre message: a spinner or not, and its lines `(text, small)`.
pub(super) struct Message {
    spinner: bool,
    lines: Vec<(String, bool)>,
}

impl Message {
    fn loading(locale: Locale) -> Message {
        Message {
            spinner: true,
            lines: vec![(translate(locale, "app.loading"), true)],
        }
    }

    /// The message that stands for a view with no cards; none for cards.
    pub(super) fn for_view(body: &DisplayBody, locale: Locale) -> Option<Message> {
        let lines = match body {
            DisplayBody::Loading => return Some(Message::loading(locale)),
            DisplayBody::NoAccounts { title, hint } => {
                vec![(title.resolve(locale), false), (hint.resolve(locale), true)]
            }
            DisplayBody::NoSearchResults(text) => vec![(text.resolve(locale), false)],
            DisplayBody::Items { .. } => return None,
        };
        Some(Message {
            spinner: false,
            lines,
        })
    }
}

/// What the content box shows under the title bar.
pub(super) enum Body {
    /// A centre message alone: no compatible platform, or the platform's
    /// adapter still loading.
    Message(Message),
    /// The breadcrumb over a message: the platform does not run on this OS.
    Unsupported(Message),
    /// The toolbar row over the view.
    Toolbar,
}

impl Body {
    pub(super) fn pick(
        compatible_platforms: usize,
        usable: bool,
        adapter_ready: bool,
        locale: Locale,
        platform_name: &str,
    ) -> Body {
        if compatible_platforms == 0 {
            return Body::Message(Message {
                spinner: false,
                lines: vec![
                    (translate(locale, "app.noCompatiblePlatforms"), false),
                    (translate(locale, "app.noCompatiblePlatformsHint"), true),
                ],
            });
        }
        if !usable {
            let message = accshift_app::text::Text::with(
                "app.platformUnsupportedOs",
                [("platform", platform_name.to_string())],
            )
            .resolve(locale);
            return Body::Unsupported(Message {
                spinner: false,
                lines: vec![(message, true)],
            });
        }
        if !adapter_ready {
            return Body::Message(Message::loading(locale));
        }
        Body::Toolbar
    }

    /// A right click on the box's background opens the background menu.
    pub(super) fn has_background_menu(&self) -> bool {
        !matches!(self, Body::Message(_))
    }
}

/// The breadcrumb alone on its row. Returns its height.
pub(super) fn breadcrumb_row(
    ui: &mut Ui,
    f: &Frame,
    crumbs: &BreadcrumbProps,
    top: f32,
    actions: &mut Vec<Action>,
) -> f32 {
    let h = breadcrumb::height(ui, crumbs, f.inner_w);
    let r = f.to_screen(Rect::from_min_size(pos2(0.0, top), vec2(f.inner_w, h)));
    if let Some(target) = breadcrumb::breadcrumb(ui, r, crumbs) {
        actions.push(Action::NavigateToFolder(target));
    }
    h
}

/// The toolbar row's content.
pub(super) struct Toolbar<'a> {
    pub crumbs: &'a BreadcrumbProps<'a>,
    pub query: &'a str,
    pub mode: ViewMode,
}

impl Toolbar<'_> {
    /// The toolbar row: the breadcrumb takes the free width, then the
    /// search field and the view toggle, each centred on the row. Returns
    /// its height with the gap under it.
    pub(super) fn draw(
        &self,
        ui: &mut Ui,
        search: &mut Search,
        f: &Frame,
        top: f32,
        actions: &mut Vec<Action>,
    ) -> f32 {
        let search_w = SEARCH_W.min(0.38 * crate::zoom::viewport(&f.ctx).x);
        let crumb_w = (f.inner_w - search_w - view_toggle::WIDTH - 2.0 * TOOLBAR_GAP).max(0.0);
        let crumb_h = breadcrumb::height(ui, self.crumbs, crumb_w);
        let row_h = crumb_h.max(SEARCH_H);
        let centred = |x: f32, size: Vec2| {
            f.to_screen(Rect::from_min_size(
                pos2(x, top + (row_h - size.y) / 2.0),
                size,
            ))
        };
        let crumb_rect = centred(0.0, vec2(crumb_w, crumb_h));
        if let Some(target) = breadcrumb::breadcrumb(ui, crumb_rect, self.crumbs) {
            actions.push(Action::NavigateToFolder(target));
        }
        let search_rect = centred(crumb_w + TOOLBAR_GAP, vec2(search_w, SEARCH_H));
        let props = SearchProps {
            value: self.query,
            placeholder: &translate(f.locale, "app.searchPlaceholder"),
            dark: f.dark,
            now: f.now,
        };
        if let Some(query) = search_field(ui, search_rect, search, f.v, props) {
            actions.push(Action::SearchChanged(query));
        }
        let toggle_rect = centred(
            f.inner_w - view_toggle::WIDTH,
            vec2(view_toggle::WIDTH, view_toggle::HEIGHT),
        );
        let toggle = ViewToggleProps {
            vars: f.v,
            mode: self.mode,
            locale: f.locale,
            focus_visible: f.focus_visible,
        };
        if let Some(mode) = view_toggle::view_toggle(ui, toggle_rect, &toggle) {
            actions.push(Action::SetViewMode(mode));
        }
        row_h + TOOLBAR_GAP
    }
}

/// A centred column of lines in the muted colour, 48 px of room above and
/// below, under an optional spinner. A plain line is 16 px on 24; a small
/// one is 14 px on 20, 4 px under the line above it, at 70 % opacity
/// unless under the spinner. Returns the height.
pub(super) fn center_message(ui: &mut Ui, f: &Frame, top: f32, message: &Message) -> f32 {
    let v = f.v;
    let width = f.inner_w;
    let mut y = top + 48.0;
    if message.spinner {
        let r = f.to_screen(Rect::from_min_size(
            pos2((width - 20.0) / 2.0, y),
            vec2(20.0, 20.0),
        ));
        paint_spinner(ui, r, f);
        y += 20.0;
    }
    for (i, (line, small)) in message.lines.iter().enumerate() {
        let (size, line_h, color) = if *small {
            let opacity = if message.spinner { 1.0 } else { 0.7 };
            (14.0, 20.0, v.fg_muted.alpha(v.fg_muted.a * opacity))
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
        let r = f.to_screen(Rect::from_min_size(pos2(0.0, y), vec2(width, line_h)));
        let laid = text::paint(ui.painter(), r, line, &style);
        y += laid.map_or(line_h, |l| l.height.max(line_h));
    }
    y + 48.0 - top
}

/// A 20 px ring of 2 px in the border colour with a quarter arc in the
/// accent, one turn every 0.7 s.
fn paint_spinner(ui: &Ui, r: Rect, f: &Frame) {
    let painter = ui.painter();
    let c = r.center();
    painter.circle_stroke(c, 9.0, egui::Stroke::new(2.0, f.v.border.to_color32()));
    let turn = (f.now / 0.7).fract() as f32 * std::f32::consts::TAU;
    let start = -3.0 * std::f32::consts::FRAC_PI_4 + turn;
    let points: Vec<egui::Pos2> = (0..=12)
        .map(|k| {
            let a = start + k as f32 / 12.0 * std::f32::consts::FRAC_PI_2;
            c + vec2(a.cos(), a.sin()) * 9.0
        })
        .collect();
    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(2.0, f.accent.to_color32()),
    ));
    ui.ctx().request_repaint();
}

/// The loader's error: 13 px red text on a faint red box with 8 px corners
/// and 12 px of padding, 16 px over what follows. Returns its height with
/// that margin.
pub(super) fn error_banner(ui: &mut Ui, f: &Frame, top: f32, message: &str) -> f32 {
    let width = f.inner_w;
    let style = TextStyle::new(13.0, 400)
        .line_height(LineHeight::Number(1.5))
        .color(Rgba::hex(0xf87171, 1.0).to_color32());
    let inner = Rect::from_min_size(pos2(12.0, top + 12.0), vec2(width - 24.0, 19.5));
    let text_h =
        text::layout(ui.ctx(), message, &style, f.to_screen(inner)).map_or(19.5, |l| l.height);
    let r = f.to_screen(Rect::from_min_size(
        pos2(0.0, top),
        vec2(width, text_h + 24.0),
    ));
    BoxStyle {
        background: Rgba::new(239.0, 68.0, 68.0, 0.1),
        radius: Corners::all(8.0),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), r);
    text::paint(ui.painter(), f.to_screen(inner), message, &style);
    text_h + 24.0 + 16.0
}

/// The list view in the rest of the box: it scrolls its own panel, so the
/// content box never scrolls. Returns the height it takes.
pub(super) fn draw_list(
    ui: &mut Ui,
    ws: &mut Workspace,
    f: &Frame,
    avatars: &mut AvatarTextures,
    items: &[DisplayItem],
    top: f32,
    actions: &mut Vec<Action>,
) -> f32 {
    let h = (f.viewport.height() - PAD_TOP - PAD_BOTTOM - top).max(0.0);
    let list_h = if f.bulk_edit {
        (h - BULK_PAD).max(0.0)
    } else {
        h
    };
    let r = f.to_screen(Rect::from_min_size(pos2(0.0, top), vec2(f.inner_w, list_h)));
    let mut list = std::mem::take(&mut ws.list);
    let list_ghost = ws.ghost.clone().filter(|_| f.drag.dragging);
    let kb_focus = ws.kb_focus.clone();
    let kb_reveal = std::mem::take(&mut ws.kb_reveal);
    let mut host = Host {
        ws: &mut *ws,
        avatars,
        f,
        prefix: format!("{}/", f.panel_key),
    };
    // The list fades and rises in from its mount.
    let mount = host.mount("list");
    let (opacity, rise) = page_entrance(&f.ctx, mount, f.v);
    let props = ListProps {
        vars: f.v,
        locale: f.locale,
        accent: f.accent,
        focus_visible: f.focus_visible,
        dark: f.dark,
        time: &f.time,
        kb_focus: kb_focus.as_deref(),
        kb_reveal,
        drag_over_folder: f.drag.over_folder.as_deref(),
        drag_over_back: f.drag.over_back,
        dragging: f.drag.dragging,
        ghost: list_ghost.as_ref().map(|g| list_view::RowGhost {
            item: &g.item,
            rect: g.rect,
            offset: f.drag.pointer - g.anchor,
            id: g.id,
        }),
    };
    let transform = egui::emath::TSTransform::from_translation(vec2(0.0, rise));
    let events = ui
        .with_visual_transform(transform, |ui| {
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r));
            child.set_clip_rect(f.clip);
            child.multiply_opacity(opacity);
            list_view::list_view(&mut child, r, &mut list, items, &props, &mut host)
        })
        .inner;
    crate::ui::anim::release(&f.ctx);
    ws.list = list;
    ws.cards = ws.list.cards.clone();
    ws.cards_scroll = None;
    actions.extend(events.into_iter().map(card_action));
    h
}

/// The list view's way into the workspace: mounts, avatars, colours.
struct Host<'h> {
    ws: &'h mut Workspace,
    avatars: &'h mut AvatarTextures,
    f: &'h Frame<'h>,
    /// The panel key and a slash.
    prefix: String,
}

impl ListHost for Host<'_> {
    fn mount(&mut self, key: &str) -> Id {
        let key = format!("{}{key}", self.prefix);
        let id = self.ws.mount(&key, self.f.frame, self.f.now);
        // The row draws its state right after its mount.
        crate::ui::anim::own_from(&self.f.ctx, id);
        id
    }

    fn avatar(
        &mut self,
        key: &str,
        item: &AccountCardItem,
        px: u32,
        visible: bool,
    ) -> AvatarContent {
        let key = format!("{}{key}", self.prefix);
        self.ws
            .account_avatar(self.avatars, self.f, &key, item, px, visible)
    }

    fn blurred(&mut self, item: &AccountCardItem, px: u32, sigma: f32) -> Option<egui::TextureId> {
        let url = item.avatar.url.as_deref()?;
        self.avatars.blurred(&self.f.ctx, url, px, sigma)
    }

    fn card_color(&self, value: &str) -> Option<Rgba> {
        card_color(self.f.resolved, self.f.v, value)
    }
}

/// The page entrance (ease out over the theme's page entrance time, opacity
/// 0 to 1 while rising 4 px) from the first call for `id`: `(opacity,
/// rise)`.
fn page_entrance(ctx: &egui::Context, id: Id, v: &crate::app::vars::Vars) -> (f32, f32) {
    let p = crate::ui::anim::animate(
        ctx,
        id.with("page-entrance"),
        &crate::ui::anim::Keyframes::from_to(0.0f32, 1.0),
        &crate::ui::anim::Animation::new(v.motion_page_entrance, crate::ui::anim::Timing::EASE_OUT),
        1.0,
    );
    (p, 4.0 * (1.0 - p))
}

/// The cards the grid lays out.
pub(super) struct GridItems<'a> {
    pub items: &'a [DisplayItem],
    pub folder_empty: bool,
    /// The folder sections, in sections mode.
    pub sections: Option<&'a [FolderSection]>,
    /// Last frame's items: a collapsing section slides its cards out.
    pub previous: &'a [DisplayItem],
}

/// Lays the grid out from `top`, flat or in folder sections; nothing draws
/// yet. Returns the content's bottom.
pub(super) fn lay_grid<'a>(
    f: &Frame,
    laid: &mut Laid<'a>,
    folds: &'a mut Folds,
    cards: GridItems<'a>,
    top: f32,
) -> f32 {
    let v = f.v;
    let pad = grid_layout(f.inner_w, v.density_scale).map_or(0.0, |l| l.padding_left);
    let card = vec2(v.grid_card_width(), v.grid_card_min_height());
    let size = |item: &DisplayItem| match item {
        DisplayItem::Account(a) => vec2(card.x, account_card::height(&f.ctx, v, a, false)),
        _ => card,
    };
    let grid = Grid {
        left: pad,
        width: f.inner_w - pad,
        size: &size,
    };
    let prefix = format!("{}/", f.panel_key);
    let mut y = top;
    if let Some(sections) = cards.sections {
        y += lay_sections(
            laid,
            folds,
            sections,
            cards.items,
            cards.previous,
            &prefix,
            &grid,
            y,
            f.motion,
            f.now,
        );
    } else {
        let refs: Vec<&DisplayItem> = cards.items.iter().collect();
        let prefix = format!("{prefix}flat");
        let h = flow(&mut laid.placed, &refs, &prefix, &grid, y);
        if cards.folder_empty {
            // The empty-folder message: its own 20 px row under the back
            // card.
            let row = y + card.y + GAP;
            laid.placed.push(Placed {
                key: format!("{prefix}/empty"),
                rect: Rect::from_min_size(pos2(pad, row), vec2(f.inner_w - pad, 20.0)),
                clip: None,
                opacity: 1.0,
                node: Node::EmptyFolder,
                per_row: 1,
            });
            y = row + 20.0;
        } else {
            y += h;
        }
    }
    if f.bulk_edit {
        y += BULK_PAD;
    }
    y
}
