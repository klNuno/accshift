//! `src/lib/shared/components/ContextMenu.svelte`: the menu a right click
//! opens, drawn by `AppDialogs.svelte` above everything (`--z-menu`).
//!
//! `.context-menu` is fixed at the click point, clamped 4 px inside the
//! window (`EDGE_GAP`), `min-width: 220px` on its border box and as wide as
//! its widest entry otherwise, `padding: 4px`, `--bg-overlay` under
//! `--card-bg-image`, the theme border, `border-radius: 6px` and
//! `box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4)`.
//!
//! - `.menu-item`: a `<button>`, `padding: 6px 10px`, 12 px text on the
//!   body's line height (18 px), `border-radius: 4px`. `:hover` and
//!   `:focus-visible` paint `--bg-muted`, `:active` `--bg-elevated`, through
//!   `transition: background 80ms`. A submenu parent ends with a `›` in
//!   `--fg-subtle`, 8 px after its label.
//! - `.separator`: 1 px of `--border`, `margin: 4px 6px`.
//! - `.swatch-group`: `padding: 6px 8px`, a 10 px uppercase label in
//!   `--fg-muted` (`letter-spacing: 0.06em`), 6 px, then a row of 18 px
//!   swatches 6 px apart, each a 12 px fill in a 1 px `--border` box on
//!   `--bg-solid`. The active one has an `--fg` border and a 1 px `--fg`
//!   ring; hovering or keyboard focus rings the others at 35 %.
//! - `.submenu`: the same box, `min-width: 240px`, at most
//!   `min(320px, 100vw - 8px)` wide and `100vh - 8px` high, 4 px to the right
//!   of the menu (or to its left when it does not fit), level with its
//!   parent entry. A loader fills it on open; until then it says
//!   "Loading...".
//!
//! The first entry takes focus on open; its ring shows only under keyboard
//! modality (`:focus-visible` after a programmatic focus). Arrows, Home and
//! End move focus, ArrowRight and ArrowLeft enter and leave a submenu, Enter
//! and Space click, Escape closes. Hovering an entry swaps the submenu after
//! 150 ms while one is open (`SUBMENU_SWITCH_DELAY`), at once otherwise.
//!
//! A press outside closes the menu; so does a scroll outside it, which the
//! caller reports. The menu reports [`MenuEvent`]s; [`AppDialogs`] runs them.
//!
//! [`AppDialogs`]: accshift_app::actions::dialogs::AppDialogs

use accshift_app::actions::dialogs::{AppMenuCommand, AppMenuEntry, AppMenuSwatch};
use accshift_app::i18n::{translate, Locale};
use accshift_app::menu::SubmenuLoader;
use egui::accesskit::Role;
use egui::{pos2, vec2, Context, Event, Id, Key, Pos2, Rect, Sense, Ui, Vec2};

use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim::{self, Timing, Transition};
use crate::ui::css;
use crate::ui::paint::{
    Border, BoxShadow, BoxStyle, ColorStop, Corners, Gradient, Length, LinearDirection,
    LinearGradient,
};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextStyle, TextTransform};

/// `min-width: 220px` (`.context-menu`) and `240px` (`.submenu`).
const MIN_W: f32 = 220.0;
const SUB_MIN_W: f32 = 240.0;
/// `.submenu { max-width: min(320px, calc(100vw - 8px)) }`.
const SUB_MAX_W: f32 = 320.0;
/// `padding: 4px`, `border-radius: 6px`.
const PAD: f32 = 4.0;
const RADIUS: f32 = 6.0;
/// `EDGE_GAP`, and the 4 px between the menu and its submenu.
const EDGE_GAP: f32 = 4.0;
/// How far in from the box's top and left edges the pointer counts as over
/// it: one layout unit.
const EDGE: f32 = 1.0 / 64.0;
const SUB_GAP: f32 = 4.0;
/// `.menu-item { padding: 6px 10px; border-radius: 4px; font-size: 12px }`.
const ITEM_PAD_X: f32 = 10.0;
const ITEM_PAD_Y: f32 = 6.0;
const ITEM_RADIUS: f32 = 4.0;
const ITEM_SIZE: f32 = 12.0;
/// `.submenu-arrow { margin-left: 8px }`.
const ARROW_GAP: f32 = 8.0;
const ARROW: &str = "\u{203a}";
/// `.separator { margin: 4px 6px }`.
const SEP_Y: f32 = 4.0;
const SEP_X: f32 = 6.0;
/// `.swatch-group { padding: 6px 8px; gap: 6px }`, `padding-top: 4px` in a
/// submenu.
const GROUP_PAD_Y: f32 = 6.0;
const GROUP_PAD_X: f32 = 8.0;
const GROUP_GAP: f32 = 6.0;
const SUB_GROUP_PAD_TOP: f32 = 4.0;
const LABEL_SIZE: f32 = 10.0;
/// `.swatch`: 18 px, 6 px apart, radius 4; `.swatch-fill`: 12 px, radius 3.
const SWATCH: f32 = 18.0;
const SWATCH_GAP: f32 = 6.0;
const SWATCH_RADIUS: f32 = 4.0;
const FILL: f32 = 12.0;
const FILL_RADIUS: f32 = 3.0;
/// `.submenu-state { padding: 8px }`, `.error { color: #f87171 }`.
const STATE_PAD: f32 = 8.0;
const STATE_ERROR: Rgba = Rgba::hex(0xf87171, 1.0);
/// `SUBMENU_SWITCH_DELAY`.
const SWITCH_DELAY: f64 = 0.15;
/// `transition: background 80ms`.
const T80: Transition = Transition::new(0.08, Timing::EASE);

/// A loader's result, as the caller holds it.
#[derive(Clone, Debug, PartialEq)]
pub enum SubmenuLoad {
    Loading,
    Ready(Vec<AppMenuEntry>),
    /// `context.submenuError`.
    Failed,
}

/// What the menu shows.
pub struct ContextMenuProps<'a> {
    pub vars: &'a Vars,
    pub locale: Locale,
    /// Changes on every open: `{#if contextMenu}` mounts a fresh menu.
    pub open_id: u64,
    pub items: &'a [AppMenuEntry],
    /// `contextMenu.x` and `.y`.
    pub at: Pos2,
    /// The window: what the menu stays inside.
    pub viewport: Rect,
    /// Chromium's keyboard modality: the last input was a key press.
    pub keyboard: bool,
    /// The loaders asked for with [`MenuEvent::Load`] and their state.
    pub loads: &'a [(SubmenuLoader, SubmenuLoad)],
    pub now: f64,
}

/// What the user did.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuEvent {
    /// An entry or swatch: run it, the menu closes.
    Activate(AppMenuCommand),
    /// `onClose`: Escape, a press outside, an entry with nothing to run.
    Close,
    /// A submenu opened on a loader: run it.
    Load(SubmenuLoader),
}

/// What a pending submenu switch does.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Switch {
    Open(usize),
    Clear,
}

/// The element the pointer is over, for `mouseenter`.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Over {
    Item(usize),
    Swatch(usize, usize),
}

/// One focusable element (`[data-menu-item]`): an entry, or one swatch of it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Target {
    entry: usize,
    swatch: Option<usize>,
}

#[derive(Clone, Debug, Default)]
struct State {
    open_id: u64,
    /// `hoveredSubmenuIndex`.
    submenu: Option<usize>,
    /// `submenuSwitchTimer`: when it fires and what it does.
    timer: Option<(f64, Switch)>,
    /// The element under the pointer last frame, outside the submenu.
    over: Option<Over>,
    /// The pointer was over the submenu box last frame.
    over_submenu: bool,
    /// `openSubmenuForKeyboard`: focus the submenu's first entry once its
    /// entries are in.
    focus_submenu: Option<usize>,
    /// The first entry still has to take focus.
    mount_focus: bool,
    /// Opened by a key (the context menu key, Shift+F10, Delete), not at
    /// the pointer.
    from_key: bool,
    /// Last drawn boxes, for [`contains`].
    menu_rect: Option<Rect>,
    submenu_rect: Option<Rect>,
}

fn state_id() -> Id {
    Id::unique("accshift::context-menu")
}

/// Whether `pos` falls on the open menu or its submenu (as last drawn).
pub fn contains(ctx: &Context, pos: Pos2) -> bool {
    hit_test(ctx)(pos)
}

/// `contains` with the rects read now, for use inside `ctx.input`, where
/// reading the context again would deadlock.
pub fn hit_test(ctx: &Context) -> impl Fn(Pos2) -> bool {
    let st: Option<State> = ctx.data(|d| d.get_temp(state_id()));
    let rects = st.map_or([None, None], |st| [st.menu_rect, st.submenu_rect]);
    move |pos| rects.iter().flatten().any(|r| r.contains(pos))
}

/// Forgets the menu: the next open starts fresh.
pub fn reset(ctx: &Context) {
    ctx.data_mut(|d| d.remove::<State>(state_id()));
}

fn item_style(v: &Vars) -> TextStyle {
    TextStyle::new(ITEM_SIZE, 400)
        .line_height(LineHeight::Number(v.line_height))
        .color(v.fg.to_color32())
}

fn arrow_style(v: &Vars) -> TextStyle {
    item_style(v).nowrap(false).color(v.fg_subtle.to_color32())
}

fn label_style(v: &Vars) -> TextStyle {
    TextStyle::new(LABEL_SIZE, 400)
        .line_height(LineHeight::Number(v.line_height))
        .letter_spacing_em(0.06)
        .transform(TextTransform::Uppercase)
        .nowrap(false)
        .color(v.fg_muted.to_color32())
}

fn state_style(v: &Vars, error: bool) -> TextStyle {
    TextStyle::new(ITEM_SIZE, 400)
        .line_height(LineHeight::Number(v.line_height))
        .color(if error { STATE_ERROR } else { v.fg_muted }.to_color32())
}

/// A width rounded up to a LayoutUnit, as inline boxes are.
fn ceil_unit(w: f32, ppp: f32) -> f32 {
    (w * ppp * 64.0).ceil() / 64.0 / ppp
}

fn line(ctx: &Context, style: &TextStyle, size: f32) -> f32 {
    style.line_box(ctx).map_or(size * 1.5, |l| l.height)
}

fn has_submenu(entry: &AppMenuEntry) -> bool {
    matches!(
        entry,
        AppMenuEntry::Item { submenu, loader, .. } if submenu.is_some() || loader.is_some()
    )
}

/// What a box lists.
enum Content<'a> {
    Entries(&'a [AppMenuEntry]),
    /// `.submenu-state`: loading, failed or empty.
    State {
        text: String,
        error: bool,
        loading: bool,
    },
}

/// One row of a box, relative to its content box.
#[derive(Clone, Copy, Debug)]
struct Row {
    top: f32,
    height: f32,
    /// The label's height (more than a line when it wraps).
    label: f32,
}

struct Layout {
    size: Vec2,
    border: f32,
    rows: Vec<Row>,
}

fn row_width(ctx: &Context, v: &Vars, locale: Locale, entry: &AppMenuEntry) -> f32 {
    let ppp = ctx.pixels_per_point();
    let measure =
        |s: &str, style: &TextStyle| ceil_unit(text::measure(ctx, s, style).unwrap_or(0.0), ppp);
    match entry {
        AppMenuEntry::Separator => 2.0 * SEP_X,
        AppMenuEntry::Item { label, .. } => {
            let style = item_style(v);
            let arrow = if has_submenu(entry) {
                ARROW_GAP + measure(ARROW, &arrow_style(v))
            } else {
                0.0
            };
            2.0 * ITEM_PAD_X + measure(&label.resolve(locale), &style) + arrow
        }
        AppMenuEntry::Swatches {
            label, swatches, ..
        } => {
            let n = swatches.len() as f32;
            let row = (n * SWATCH + (n - 1.0).max(0.0) * SWATCH_GAP).max(0.0);
            2.0 * GROUP_PAD_X + measure(&label.resolve(locale), &label_style(v)).max(row)
        }
    }
}

/// Sizes a box: `min_w` and `max_w` bound its border box, `max_h` too.
#[allow(clippy::too_many_arguments)]
fn layout(
    ctx: &Context,
    v: &Vars,
    locale: Locale,
    content: &Content,
    submenu: bool,
    min_w: f32,
    max_w: f32,
    max_h: f32,
) -> Layout {
    let ppp = ctx.pixels_per_point();
    let border = snap::border_width(v.border_px(), ppp);
    let chrome = 2.0 * (PAD + border);
    let natural = match content {
        Content::Entries(entries) => entries
            .iter()
            .map(|e| row_width(ctx, v, locale, e))
            .fold(0.0, f32::max),
        Content::State { text, error, .. } => {
            2.0 * STATE_PAD
                + ceil_unit(
                    text::measure(ctx, text, &state_style(v, *error)).unwrap_or(0.0),
                    ppp,
                )
        }
    };
    let width = (natural + chrome).min(max_w).max(min_w);
    let inner = width - chrome;
    let item_line = line(ctx, &item_style(v), ITEM_SIZE);
    let mut rows = Vec::new();
    let mut y = 0.0;
    match content {
        Content::Entries(entries) => {
            for entry in *entries {
                let (height, label) = match entry {
                    AppMenuEntry::Separator => (1.0 + 2.0 * SEP_Y, 0.0),
                    AppMenuEntry::Item { label, .. } => {
                        // A long label wraps inside a narrow submenu.
                        let avail = inner - 2.0 * ITEM_PAD_X;
                        let h = text::layout(
                            ctx,
                            &label.resolve(locale),
                            &item_style(v),
                            Rect::from_min_size(Pos2::ZERO, vec2(avail, item_line)),
                        )
                        .map_or(item_line, |l| l.height.max(item_line));
                        (h + 2.0 * ITEM_PAD_Y, h)
                    }
                    AppMenuEntry::Swatches { .. } => {
                        let top = if submenu {
                            SUB_GROUP_PAD_TOP
                        } else {
                            GROUP_PAD_Y
                        };
                        let label = line(ctx, &label_style(v), LABEL_SIZE);
                        (top + label + GROUP_GAP + SWATCH + GROUP_PAD_Y, label)
                    }
                };
                rows.push(Row {
                    top: y,
                    height,
                    label,
                });
                y += height;
            }
        }
        Content::State { .. } => {
            rows.push(Row {
                top: 0.0,
                height: item_line + 2.0 * STATE_PAD,
                label: item_line,
            });
            y = item_line + 2.0 * STATE_PAD;
        }
    }
    Layout {
        size: vec2(width, (y + chrome).min(max_h)),
        border,
        rows,
    }
}

/// `positionMenu`, the click point `at` and the result relative to the
/// window. `offsetWidth` and `offsetHeight` are whole pixels, and the
/// local scale is the rendered width over `offsetWidth`, so a fractional
/// box lands where the TS arithmetic puts it, not on the exact clamp.
fn place_menu(at: Vec2, size: Vec2, window: Vec2) -> Vec2 {
    let offset = vec2(size.x.round().max(1.0), size.y.round());
    let scale = size.x / offset.x;
    let clamp = |v: f32, extent: f32, own: f32| {
        (v / scale)
            .min(extent / scale - own - EDGE_GAP)
            .max(EDGE_GAP)
    };
    vec2(
        clamp(at.x, window.x, offset.x),
        clamp(at.y, window.y, offset.y),
    )
}

/// `positionSubmenu`: `left` and `top` from the menu's padding edge, for a
/// menu at `menu` (relative to the window) and an entry `anchor` px down.
fn place_submenu(menu: Vec2, size: Vec2, sub: Vec2, anchor: f32, window: Vec2) -> Vec2 {
    let menu_w = size.x.round().max(1.0);
    let scale = size.x / menu_w;
    let (sub_w, sub_h) = (sub.x.round(), sub.y.round());
    let local_top = -menu.y / scale;
    let local_right = (window.x - menu.x) / scale;
    let local_bottom = (window.y - menu.y) / scale;
    let left = if menu_w + SUB_GAP + sub_w <= local_right - EDGE_GAP {
        menu_w + SUB_GAP
    } else {
        -(sub_w + SUB_GAP)
    };
    let mut top = anchor;
    if sub_h > 0.0 && top + sub_h > local_bottom - EDGE_GAP {
        top = local_bottom - sub_h - EDGE_GAP;
    }
    if top < local_top + EDGE_GAP {
        top = local_top + EDGE_GAP;
    }
    vec2(left, top)
}

fn targets(entries: &[AppMenuEntry]) -> Vec<Target> {
    let mut out = Vec::new();
    for (entry, e) in entries.iter().enumerate() {
        match e {
            AppMenuEntry::Separator => {}
            AppMenuEntry::Item { .. } => out.push(Target {
                entry,
                swatch: None,
            }),
            AppMenuEntry::Swatches { swatches, .. } => {
                out.extend((0..swatches.len()).map(|k| Target {
                    entry,
                    swatch: Some(k),
                }));
            }
        }
    }
    out
}

/// The box of a menu or submenu, its shadow included.
fn paint_box(ui: &Ui, rect: Rect, v: &Vars, border: f32) {
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

/// `.swatch-fill.default`: `linear-gradient(135deg, --bg-muted 0 50%,
/// --bg-elevated 50% 100%)`.
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

/// What one box reported.
#[derive(Default)]
struct BoxInput {
    over: Option<Over>,
    clicked: Option<Target>,
    pressed: Option<Target>,
}

/// Where the menu or submenu draws: its entries, ids and focus.
struct Frame<'a> {
    rect: Rect,
    layout: &'a Layout,
    entries: &'a [AppMenuEntry],
    submenu: bool,
    id: Id,
    focused: Option<Target>,
    /// The entry whose submenu is open (`aria-expanded`).
    expanded: Option<usize>,
}

fn target_id(base: Id, t: Target) -> Id {
    base.with((t.entry, t.swatch))
}

/// Registers and paints the entries of one box; `over` is relative to it.
fn draw_entries(ui: &mut Ui, f: &Frame, v: &Vars, locale: Locale, keyboard: bool) -> BoxInput {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let mut out = BoxInput::default();
    let origin = f.rect.min + Vec2::splat(f.layout.border + PAD);
    let inner_w = f.rect.width() - 2.0 * (f.layout.border + PAD);
    let mut painter = ui.painter().clone();
    painter.set_clip_rect(
        painter
            .clip_rect()
            .intersect(f.rect.shrink(f.layout.border)),
    );
    for ((entry, e), row) in f.entries.iter().enumerate().zip(&f.layout.rows) {
        let top = origin.y + row.top;
        match e {
            AppMenuEntry::Separator => {
                let r = Rect::from_min_size(
                    pos2(origin.x + SEP_X, top + SEP_Y),
                    vec2(inner_w - 2.0 * SEP_X, 1.0),
                );
                painter.rect_filled(snap::rect(r, ppp), 0.0, v.border.to_color32());
            }
            AppMenuEntry::Item { label, .. } => {
                let t = Target {
                    entry,
                    swatch: None,
                };
                let rect = Rect::from_min_size(pos2(origin.x, top), vec2(inner_w, row.height));
                let id = target_id(f.id, t);
                let label = label.resolve(locale);
                let response = ui.interact(snap::rect(rect, ppp), id, Sense::click());
                let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                let parent = !f.submenu && has_submenu(e);
                ctx.accesskit_node_builder(id, |node| {
                    node.set_role(Role::MenuItem);
                    node.set_label(label.as_str());
                    if parent {
                        node.set_has_popup(egui::accesskit::HasPopup::Menu);
                        node.set_expanded(f.expanded == Some(entry));
                    }
                });
                if response.hovered() {
                    out.over = Some(Over::Item(entry));
                }
                if response.clicked() {
                    out.clicked = Some(t);
                }
                if ui.input(|i| i.pointer.primary_pressed()) && response.hovered() {
                    out.pressed = Some(t);
                }
                let focused = f.focused == Some(t);
                let target = if response.is_pointer_button_down_on() {
                    v.bg_elevated
                } else if response.hovered() || (focused && keyboard) {
                    v.bg_muted
                } else {
                    Rgba::TRANSPARENT
                };
                let bg = anim::transition(&ctx, id.with("bg"), target, &T80);
                if bg.a > 0.0 {
                    BoxStyle {
                        background: bg,
                        radius: Corners::all(ITEM_RADIUS),
                        ..BoxStyle::default()
                    }
                    .paint(&painter, snap::rect(rect, ppp));
                }
                let arrow_w = if parent {
                    ceil_unit(
                        text::measure(&ctx, ARROW, &arrow_style(v)).unwrap_or(0.0),
                        ppp,
                    )
                } else {
                    0.0
                };
                let text_rect = Rect::from_min_size(
                    pos2(rect.left() + ITEM_PAD_X, top + ITEM_PAD_Y),
                    vec2(
                        rect.width()
                            - 2.0 * ITEM_PAD_X
                            - if parent { ARROW_GAP + arrow_w } else { 0.0 },
                        row.label,
                    ),
                );
                text::paint(&painter, text_rect, &label, &item_style(v));
                if parent {
                    let line = line(&ctx, &arrow_style(v), ITEM_SIZE);
                    let r = Rect::from_min_size(
                        pos2(rect.right() - ITEM_PAD_X - arrow_w, top + ITEM_PAD_Y),
                        vec2(arrow_w, line),
                    );
                    text::paint(&painter, r, ARROW, &arrow_style(v));
                }
            }
            AppMenuEntry::Swatches {
                label, swatches, ..
            } => {
                let pad_top = if f.submenu {
                    SUB_GROUP_PAD_TOP
                } else {
                    GROUP_PAD_Y
                };
                let x0 = origin.x + GROUP_PAD_X;
                let label_rect = Rect::from_min_size(
                    pos2(x0, top + pad_top),
                    vec2(inner_w - 2.0 * GROUP_PAD_X, row.label),
                );
                text::paint(
                    &painter,
                    label_rect,
                    &label.resolve(locale),
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
                    let id = target_id(f.id, t);
                    let response = ui.interact(snap::rect(rect, ppp), id, Sense::click());
                    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                    let name = sw.label.resolve(locale);
                    ctx.accesskit_node_builder(id, |node| {
                        node.set_role(Role::MenuItem);
                        node.set_label(name.as_str());
                    });
                    if response.hovered() {
                        out.over = Some(Over::Swatch(entry, k));
                    }
                    if response.clicked() {
                        out.clicked = Some(t);
                    }
                    if ui.input(|i| i.pointer.primary_pressed()) && response.hovered() {
                        out.pressed = Some(t);
                    }
                    let ring = response.hovered() || (f.focused == Some(t) && keyboard);
                    paint_swatch(&painter, rect, sw, v, ring, ppp);
                }
            }
        }
    }
    out
}

fn paint_swatch(
    painter: &egui::Painter,
    rect: Rect,
    sw: &AppMenuSwatch,
    v: &Vars,
    ring: bool,
    ppp: f32,
) {
    let bw = snap::border_width(1.0, ppp);
    // `.swatch.active` comes after `:hover` and wins.
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
    // `display: grid; place-items: center` in the content box.
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

/// What the open submenu lists.
fn submenu_content<'a>(
    entry: &'a AppMenuEntry,
    loads: &'a [(SubmenuLoader, SubmenuLoad)],
    locale: Locale,
) -> Content<'a> {
    let AppMenuEntry::Item {
        submenu, loader, ..
    } = entry
    else {
        return Content::Entries(&[]);
    };
    let entries: &[AppMenuEntry] = match (submenu, loader) {
        (Some(items), _) => items,
        (None, Some(loader)) => match loads.iter().find(|(l, _)| l == loader).map(|(_, s)| s) {
            Some(SubmenuLoad::Ready(items)) => items,
            Some(SubmenuLoad::Failed) => {
                return Content::State {
                    text: translate(locale, "context.submenuError"),
                    error: true,
                    loading: false,
                }
            }
            Some(SubmenuLoad::Loading) | None => {
                return Content::State {
                    text: translate(locale, "context.loading"),
                    error: false,
                    loading: true,
                }
            }
        },
        (None, None) => &[],
    };
    if entries.is_empty() {
        Content::State {
            text: translate(locale, "context.noGamesFound"),
            error: false,
            loading: false,
        }
    } else {
        Content::Entries(entries)
    }
}

/// The entries of an open submenu, once they are in.
fn submenu_entries<'a>(
    items: &'a [AppMenuEntry],
    index: Option<usize>,
    loads: &'a [(SubmenuLoader, SubmenuLoad)],
    locale: Locale,
) -> Option<&'a [AppMenuEntry]> {
    let entry = items.get(index?)?;
    match submenu_content(entry, loads, locale) {
        Content::Entries(entries) => Some(entries),
        Content::State { .. } => None,
    }
}

/// `openSubmenu`: the loader runs again on every open.
fn open_submenu(st: &mut State, items: &[AppMenuEntry], index: usize, events: &mut Vec<MenuEvent>) {
    st.timer = None;
    let Some(AppMenuEntry::Item {
        submenu, loader, ..
    }) = items.get(index)
    else {
        return;
    };
    if submenu.is_none() && loader.is_none() {
        clear_submenu(st);
        return;
    }
    let reopen = st.submenu == Some(index);
    st.submenu = Some(index);
    if let (None, Some(loader), false) = (submenu, loader, reopen) {
        events.push(MenuEvent::Load(loader.clone()));
    }
}

fn clear_submenu(st: &mut State) {
    st.timer = None;
    st.submenu = None;
    st.focus_submenu = None;
}

/// `scheduleSubmenuChange`: at once without a submenu, after 150 ms with one.
fn schedule(
    st: &mut State,
    change: Switch,
    items: &[AppMenuEntry],
    now: f64,
    events: &mut Vec<MenuEvent>,
) {
    st.timer = None;
    if st.submenu.is_none() {
        apply_switch(st, change, items, events);
    } else {
        st.timer = Some((now + SWITCH_DELAY, change));
    }
}

fn apply_switch(
    st: &mut State,
    change: Switch,
    items: &[AppMenuEntry],
    events: &mut Vec<MenuEvent>,
) {
    match change {
        Switch::Open(index) => open_submenu(st, items, index, events),
        Switch::Clear => clear_submenu(st),
    }
}

/// A click on an entry of the menu (`submenu: false`) or of the submenu.
fn click(
    st: &mut State,
    entries: &[AppMenuEntry],
    t: Target,
    submenu: bool,
    events: &mut Vec<MenuEvent>,
) {
    let Some(entry) = entries.get(t.entry) else {
        return;
    };
    match (entry, t.swatch) {
        (AppMenuEntry::Swatches { swatches, .. }, Some(k)) => {
            if let Some(sw) = swatches.get(k) {
                events.push(MenuEvent::Activate(sw.command.clone()));
            }
        }
        (AppMenuEntry::Item { command, .. }, None) => match command {
            Some(command) => events.push(MenuEvent::Activate(command.clone())),
            None if !submenu && has_submenu(entry) => {
                open_submenu(st, entries, t.entry, events);
            }
            None => events.push(MenuEvent::Close),
        },
        _ => {}
    }
}

/// Draws the open menu. `items` are `contextMenuItems`.
pub fn context_menu(ui: &mut Ui, props: &ContextMenuProps) -> Vec<MenuEvent> {
    let ctx = ui.ctx().clone();
    let v = props.vars;
    let now = props.now;
    let sid = state_id();
    let mut st: State = ctx
        .data(|d| d.get_temp(sid))
        .filter(|s: &State| s.open_id == props.open_id)
        .unwrap_or_else(|| State {
            open_id: props.open_id,
            mount_focus: true,
            from_key: props.keyboard,
            ..State::default()
        });
    let mut events = Vec::new();
    let base = Id::unique(("accshift::context-menu", props.open_id));
    let menu_id = base.with("menu");

    // The switch timer.
    if let Some((at, change)) = st.timer {
        if now >= at {
            st.timer = None;
            apply_switch(&mut st, change, props.items, &mut events);
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(at - now));
        }
    }

    let menu_targets = targets(props.items);
    if st.mount_focus {
        st.mount_focus = false;
        if let Some(first) = menu_targets.first() {
            ctx.memory_mut(|m| m.request_focus(target_id(menu_id, *first)));
        }
    }

    // Focus, from egui's memory.
    let focused_id = ctx.memory(|m| m.focused());
    let sub_entries = submenu_entries(props.items, st.submenu, props.loads, props.locale);
    let sub_targets = sub_entries.map(targets).unwrap_or_default();
    let sub_id = base.with(("submenu", st.submenu));
    let focus_menu = menu_targets
        .iter()
        .copied()
        .find(|t| Some(target_id(menu_id, *t)) == focused_id);
    let focus_sub = sub_targets
        .iter()
        .copied()
        .find(|t| Some(target_id(sub_id, *t)) == focused_id);
    if let Some(id) = focused_id.filter(|_| focus_menu.is_some() || focus_sub.is_some()) {
        ctx.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    tab: false,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            )
        });
    }

    // `handleKeydown` on the document.
    // Each keydown moves once: two presses in one frame move twice.
    let keys = ui.input(|i| {
        [
            Key::Escape,
            Key::ArrowDown,
            Key::ArrowUp,
            Key::Home,
            Key::End,
            Key::ArrowRight,
            Key::ArrowLeft,
            Key::Enter,
            Key::Space,
        ]
        .map(|k| i.num_presses(k))
    });
    let [esc, _, _, home, end, right, left, enter, space] = keys.map(|n| n > 0);
    let (downs, ups) = (keys[1], keys[2]);
    if downs + ups > 0 || right || left {
        // egui moves focus on arrows too, and the lock filter above only holds
        // from the second frame of a focus: a key right after a move would
        // count twice. The menu moves focus itself.
        ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
    }
    let mut focus_to: Option<Id> = None;
    if esc {
        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        events.push(MenuEvent::Close);
    } else {
        let in_sub = focus_sub.is_some();
        let (list, scope, current) = if in_sub {
            (&sub_targets, sub_id, focus_sub)
        } else {
            (&menu_targets, menu_id, focus_menu)
        };
        if downs + ups > 0 && !list.is_empty() {
            let n = list.len();
            let mut at = current.and_then(|c| list.iter().position(|t| *t == c));
            for down in std::iter::repeat_n(true, downs).chain(std::iter::repeat_n(false, ups)) {
                at = Some(match at {
                    None if down => 0,
                    None => n - 1,
                    Some(i) if down => (i + 1) % n,
                    Some(i) => (i + n - 1) % n,
                });
            }
            if let Some(next) = at {
                focus_to = Some(target_id(scope, list[next]));
            }
        } else if (home || end) && !list.is_empty() {
            let t = if home { list[0] } else { list[list.len() - 1] };
            focus_to = Some(target_id(scope, t));
        } else if right && !in_sub {
            if let Some(t) = focus_menu
                .filter(|t| t.swatch.is_none() && props.items.get(t.entry).is_some_and(has_submenu))
            {
                if st.submenu == Some(t.entry) && sub_entries.is_some() {
                    if let Some(first) = sub_targets.first() {
                        focus_to = Some(target_id(sub_id, *first));
                    }
                } else {
                    open_submenu(&mut st, props.items, t.entry, &mut events);
                    st.focus_submenu = Some(t.entry);
                }
            }
        } else if left && in_sub {
            let parent = st.submenu;
            clear_submenu(&mut st);
            if let Some(p) = parent {
                focus_to = Some(target_id(
                    menu_id,
                    Target {
                        entry: p,
                        swatch: None,
                    },
                ));
            }
        } else if enter || space {
            if let Some(t) = focus_sub {
                if let Some(entries) = sub_entries {
                    click(&mut st, entries, t, true, &mut events);
                }
            } else if let Some(t) = focus_menu {
                click(&mut st, props.items, t, false, &mut events);
            }
        }
    }

    // The menu box.
    let vp = props.viewport;
    let menu_layout = layout(
        &ctx,
        v,
        props.locale,
        &Content::Entries(props.items),
        false,
        MIN_W,
        f32::INFINITY,
        f32::INFINITY,
    );
    let size = menu_layout.size;
    let menu_rect = Rect::from_min_size(
        vp.min + place_menu(props.at - vp.min, size, vp.size()),
        size,
    );

    // The box itself takes the pointer: nothing under it reacts. Its top
    // and left pixel edges excepted when it opened at the pointer: WebView2
    // leaves the element under such a menu hovered, the pointer on the
    // menu's pixel-snapped corner at 100 and 150 %, while a clamped menu
    // that lands over the pointer takes it at once. A menu a key opened
    // hit-tests on its layout box, as Blink does: at 150 % the pointer on
    // its unsnapped left edge is over it (all measured).
    let ppp = ctx.pixels_per_point();
    let hit = if st.from_key {
        menu_rect
    } else {
        let hit = snap::rect(menu_rect, ppp);
        Rect::from_min_max(hit.min + vec2(EDGE, EDGE), hit.max)
    };
    ui.interact(hit, menu_id, Sense::click());
    ctx.accesskit_node_builder(menu_id, |node| node.set_role(Role::Menu));
    paint_box(ui, menu_rect, v, menu_layout.border);
    let focus_now = focus_to.or(focused_id);
    let menu_focus = menu_targets
        .iter()
        .copied()
        .find(|t| Some(target_id(menu_id, *t)) == focus_now);
    let input = draw_entries(
        ui,
        &Frame {
            rect: menu_rect,
            layout: &menu_layout,
            entries: props.items,
            submenu: false,
            id: menu_id,
            focused: menu_focus,
            expanded: st.submenu,
        },
        v,
        props.locale,
        props.keyboard,
    );

    // `mouseenter` on the menu's entries.
    if input.over != st.over {
        match input.over {
            Some(Over::Item(index)) => {
                if st.submenu == Some(index) {
                    st.timer = None;
                } else if props.items.get(index).is_some_and(has_submenu) {
                    schedule(&mut st, Switch::Open(index), props.items, now, &mut events);
                } else {
                    schedule(&mut st, Switch::Clear, props.items, now, &mut events);
                }
            }
            Some(Over::Swatch(..)) => {
                schedule(&mut st, Switch::Clear, props.items, now, &mut events);
            }
            None => {}
        }
        st.over = input.over;
    }
    if let Some(t) = input.pressed {
        // A press focuses the button (mouse focus, no ring).
        focus_to = Some(target_id(menu_id, t));
    }
    if let Some(t) = input.clicked {
        click(&mut st, props.items, t, false, &mut events);
    }

    // The submenu.
    let mut submenu_rect = None;
    if let Some(index) = st.submenu {
        let sub_id = base.with(("submenu", st.submenu));
        let entry = &props.items[index.min(props.items.len().saturating_sub(1))];
        let content = submenu_content(entry, props.loads, props.locale);
        // `100vw - 8px` and `100vh - 8px`; the placement clamps to the
        // window in the stage's points (`innerWidth / scale`).
        let css_vp = crate::zoom::viewport(&ctx);
        let max_w = SUB_MAX_W.min(css_vp.x - 8.0);
        let sub_layout = layout(
            &ctx,
            v,
            props.locale,
            &content,
            true,
            SUB_MIN_W,
            max_w,
            css_vp.y - 8.0,
        );
        let border = menu_layout.border;
        // `anchor.offsetTop`: from the padding edge, rounded.
        let anchor = (PAD + menu_layout.rows.get(index).map_or(0.0, |r| r.top)).round();
        let offset = place_submenu(
            menu_rect.min - vp.min,
            size,
            sub_layout.size,
            anchor,
            vp.size(),
        );
        let rect = Rect::from_min_size(
            menu_rect.min + vec2(border, border) + offset,
            sub_layout.size,
        );
        submenu_rect = Some(rect);
        let response = ui.interact(snap::rect(rect, ppp), sub_id, Sense::click());
        ctx.accesskit_node_builder(sub_id, |node| node.set_role(Role::Menu));
        // `onmouseenter={cancelSubmenuSwitch}`.
        let over = response.hovered() || ui.rect_contains_pointer(rect);
        paint_box(ui, rect, v, sub_layout.border);
        match &content {
            Content::State { text, error, .. } => {
                let origin = rect.min + Vec2::splat(sub_layout.border + PAD);
                let r = Rect::from_min_size(
                    origin + Vec2::splat(STATE_PAD),
                    vec2(
                        rect.width() - 2.0 * (sub_layout.border + PAD + STATE_PAD),
                        sub_layout.rows[0].label,
                    ),
                );
                text::paint(ui.painter(), r, text, &state_style(v, *error));
            }
            Content::Entries(entries) => {
                let sub_targets = targets(entries);
                if st.focus_submenu == Some(index) {
                    st.focus_submenu = None;
                    if let Some(first) = sub_targets.first() {
                        focus_to = Some(target_id(sub_id, *first));
                    }
                }
                let focused = sub_targets
                    .iter()
                    .copied()
                    .find(|t| Some(target_id(sub_id, *t)) == focus_to.or(focused_id));
                let input = draw_entries(
                    ui,
                    &Frame {
                        rect,
                        layout: &sub_layout,
                        entries,
                        submenu: true,
                        id: sub_id,
                        focused,
                        expanded: None,
                    },
                    v,
                    props.locale,
                    props.keyboard,
                );
                if let Some(t) = input.pressed {
                    focus_to = Some(target_id(sub_id, t));
                }
                if let Some(t) = input.clicked {
                    click(&mut st, entries, t, true, &mut events);
                }
            }
        }
        if over && !st.over_submenu {
            st.timer = None;
        }
        st.over_submenu = over;
        // A failed or empty submenu has nothing to focus; a loading one
        // keeps the keyboard request for when its entries land.
        if matches!(content, Content::State { loading: false, .. }) {
            st.focus_submenu = None;
        }
    } else {
        st.over_submenu = false;
    }

    // `mousedown` outside the menu (its submenu is inside it).
    let outside = ui.input(|i| {
        i.events.iter().any(|e| {
            matches!(e, Event::PointerButton { pos, pressed: true, .. }
                if !menu_rect.contains(*pos) && !submenu_rect.is_some_and(|r| r.contains(*pos)))
        })
    });
    if outside {
        events.push(MenuEvent::Close);
    }

    if let Some(id) = focus_to {
        ctx.memory_mut(|m| m.request_focus(id));
    }
    st.menu_rect = Some(menu_rect);
    st.submenu_rect = submenu_rect;
    ctx.data_mut(|d| d.insert_temp(sid, st));
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use accshift_app::text::Text;

    fn item(id: &str) -> AppMenuEntry {
        AppMenuEntry::item(id, Text::raw(id), AppMenuCommand::Custom(id.into()))
    }

    #[test]
    fn swatches_and_items_are_focusable_separators_not() {
        let entries = vec![
            item("a"),
            AppMenuEntry::Separator,
            AppMenuEntry::Swatches {
                id: "s".into(),
                label: Text::raw("Colour"),
                swatches: vec![
                    AppMenuSwatch {
                        id: "x".into(),
                        label: Text::raw("x"),
                        color: String::new(),
                        active: true,
                        command: AppMenuCommand::Custom("x".into()),
                    };
                    2
                ],
            },
        ];
        let t = targets(&entries);
        assert_eq!(t.len(), 3);
        assert_eq!(t[0].entry, 0);
        assert_eq!((t[1].entry, t[1].swatch), (2, Some(0)));
    }

    #[test]
    fn hovering_switches_at_once_without_a_submenu_and_late_with_one() {
        let entries = vec![item("a"), item("b")];
        let mut st = State::default();
        let mut events = Vec::new();
        schedule(&mut st, Switch::Clear, &entries, 1.0, &mut events);
        assert!(st.timer.is_none());
        st.submenu = Some(0);
        schedule(&mut st, Switch::Clear, &entries, 1.0, &mut events);
        assert_eq!(st.timer, Some((1.0 + SWITCH_DELAY, Switch::Clear)));
    }

    #[test]
    fn menus_clamp_with_whole_offset_sizes() {
        let window = vec2(860.0, 440.0);
        // The account menu at 150 %: 324.333 high, `offsetHeight` 324.
        let at = place_menu(vec2(205.0, 152.0), vec2(220.0, 324.333), window);
        assert_eq!(at, vec2(205.0, 112.0));
        // Clamped right and bottom.
        let at = place_menu(vec2(755.0, 298.0), vec2(220.0, 70.0), window);
        assert_eq!(at, vec2(636.0, 298.0));
        // A submenu that does not fit on the right opens on the left.
        let off = place_submenu(
            vec2(636.0, 298.0),
            vec2(220.0, 70.0),
            vec2(240.0, 34.0),
            4.0,
            window,
        );
        assert_eq!(off.x, -244.0);
    }

    #[test]
    fn a_plain_entry_without_command_closes() {
        let entries = vec![AppMenuEntry::Item {
            id: "x".into(),
            label: Text::raw("x"),
            command: None,
            submenu: None,
            loader: None,
        }];
        let mut st = State::default();
        let mut events = Vec::new();
        click(
            &mut st,
            &entries,
            Target {
                entry: 0,
                swatch: None,
            },
            false,
            &mut events,
        );
        assert_eq!(events, vec![MenuEvent::Close]);
    }
}
