//! `src/lib/shared/components/AccountCard.svelte`: the account card face.
//!
//! - `.card`: `--grid-card-width` wide, at least `--grid-card-min-height`
//!   tall (it grows with its meta lines), padding `--grid-card-padding`,
//!   radius `--grid-card-radius`, `--bg-card` under `--card-bg-image`;
//!   background, transform and shadow transition 180 ms ease-out;
//!   `card-entrance` on mount.
//! - `:not(.active):hover`: `--bg-card-hover`, `translateY(-2px)`, `0 12px
//!   24px rgba(0,0,0,.18)`; `:active` `translateY(0) scale(.985)`.
//! - `.custom-color`: `color-mix(c 24%, --bg-card)`, hover `color-mix(c 32%,
//!   --bg-card-hover)`.
//! - `.active`: rings `0 0 0 2px color-mix(--fg 62%, transparent), 0 0 0 4px
//!   color-mix(--bg-solid 45%, transparent)` on `--bg-card-hover`; with a
//!   custom colour `2px color-mix(--fg 72%)`, `5px color-mix(c 50%,
//!   color-mix(--bg-solid 62%, transparent))`.
//! - `.ban-red` / `.ban-yellow`: `outline: 2px solid` red or yellow at 0.6,
//!   opaque on a custom colour.
//! - `.avatar`: `--grid-card-avatar-size`, `margin: 0 auto 8px`, radius
//!   `--avatar-radius`, `--bg-muted`; on card hover `--bg-elevated` and
//!   `translateY(-1px)`, the picture `scale(1.04)`; `.avatar.active` outline
//!   2 px `color-mix(--fg 20%, transparent)`. The picture blurs (`blur(6px)
//!   brightness(.5)`, 300 ms) while armed, switching or refreshing; the
//!   initials (`blur(4px) brightness(.5)`, 300 ms) while armed or switching.
//! - `.name`: a block line of the body font (16 px, `--line-height`) holding
//!   the inline-block `.name-inner` (12 px 500, `line-height: 1.2`). With
//!   `overflow: hidden` the inline-block sits on the line's baseline by its
//!   bottom edge; once a hover found the name overflowing (`.marquee`) it
//!   sits on its text baseline and scrolls at 42 px/s with 2 s pauses.
//! - `.meta-stack`: 4 px under the name, 2 px apart, 10 px `line-height:
//!   1.2`: the username row (note icon, username, badge), the inline note
//!   (9 px 500) and the last login (10 px 500).
//! - Armed (first click): the play triangle over the avatar and the
//!   `.confirm-hint` scrim at the bottom, both `fadeIn` 150 ms.
//!
//! - The extension panel: [`super::card_extension`], under the face.

use egui::accesskit::Role;
use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{
    pos2, vec2, Color32, Context, Id, Key, Pos2, Rect, Sense, Shape, TextureId, Ui, Vec2,
    WidgetInfo,
};

use accshift_app::actions::avatar_fallback::avatar_initials;
use accshift_app::display::AccountCardItem;
use accshift_app::i18n::{translate, Locale};
use accshift_app::platforms::card::{
    has_card_extension_content, CardSection, ChipTone, UsernameBadge,
};
use accshift_app::platforms::warnings::WarningTone;
use accshift_app::time::{format_relative_time_compact, TimeContext};

use super::avatar_fade_in;
use super::card_extension::{self, ChipHit, Surface};
use super::folder_card::{entrance, map_rect};
use super::icon;
use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim::{self, Animation, Fill, Keyframes, Timing, Transition};
use crate::ui::paint::{self, Border, BoxShadow, BoxStyle, Corners, Gradient, Transform};
use crate::ui::snap;
use crate::ui::text::{self, LineBox, LineHeight, TextAlign, TextStyle};
use crate::ui::text_shadow::{self, TextShadow};

/// `transition: background 180ms ease-out, transform 180ms ease-out,
/// box-shadow 180ms ease-out`.
const T180: Transition = Transition::new(0.18, Timing::EASE_OUT);
/// `.avatar { transition: background 150ms }` (`ease`).
const T150: Transition = Transition::new(0.15, Timing::EASE);
/// `img { transition: transform 220ms ease-out }`.
const T220: Transition = Transition::new(0.22, Timing::EASE_OUT);
/// `img { transition: filter 300ms ease-out }`.
const T300: Transition = Transition::new(0.3, Timing::EASE_OUT);
/// `@keyframes fadeIn` over `150ms ease-out`.
const FADE_IN: Animation = Animation::new(0.15, Timing::EASE_OUT);
/// `.avatar { margin-bottom: 8px }`.
const AVATAR_GAP: f32 = 8.0;
/// `.meta-stack { margin-top: 4px; gap: 2px }`.
const META_TOP: f32 = 4.0;
const META_GAP: f32 = 2.0;
/// `.username { gap: 4px }`.
const ROW_GAP: f32 = 4.0;
/// `.note-info-icon`: 12x12 with its border (`box-sizing: border-box`).
const NOTE_ICON: f32 = 12.0;
/// The badge `<svg width="11" height="11">`.
const BADGE: f32 = 11.0;
/// `.loader { width: 20px; border: 2px }`, `spin 0.7s linear infinite`.
const LOADER: f32 = 20.0;
const LOADER_SPIN: f64 = 0.7;
/// `MARQUEE_SPEED_PX_PER_SEC`, `MARQUEE_PAUSE_MS`.
const MARQUEE_SPEED: f32 = 42.0;
const MARQUEE_PAUSE: f64 = 2.0;
/// `.confirm-hint { bottom: 4px; padding: 2px 3px }`.
const HINT_BOTTOM: f32 = 4.0;
const HINT_PAD: egui::Vec2 = egui::Vec2::new(3.0, 2.0);
/// `rgba(239, 68, 68, .6)`, `rgba(234, 179, 8, .6)`.
const BAN_RED: Rgba = Rgba::new(239.0, 68.0, 68.0, 0.6);
const BAN_YELLOW: Rgba = Rgba::new(234.0, 179.0, 8.0, 0.6);
/// `.username-badge.tone-green { color: #4ade80 }`.
const BADGE_GREEN: Rgba = Rgba::new(74.0, 222.0, 128.0, 1.0);

const PLAY_SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="#fff"><path d="M8 5v14l11-7z"/></svg>"##;
const BADGE_SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="#fff" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="8" width="18" height="12" rx="2"/><path d="M8 8V6a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/><path d="M3 13h18"/></svg>"##;

/// The decoded avatar, as the workspace's texture cache hands it over.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AvatarPicture {
    pub texture: TextureId,
    /// The texture's size in device pixels (for `object-fit: cover`).
    pub size: [usize; 2],
    /// The same picture covered into the avatar box, then `blur(6px)
    /// brightness(.5)`, square: present while [`wants_blur`] holds.
    pub blurred: Option<TextureId>,
}

/// What the avatar box shows.
#[derive(Clone, Debug, PartialEq)]
pub enum AvatarContent {
    /// `avatarUrl` set and decoded.
    Picture(AvatarPicture),
    /// `avatarUrl` set, still decoding: the `<img>` is mounted at opacity 0.
    Decoding,
    /// No picture: the initials on `getAvatarGradientStyle(seed)`
    /// (`!isLoadingAvatar`): its `background-color` and gradient.
    Initials {
        background: Rgba,
        gradient: Option<Gradient>,
    },
    /// `isLoadingAvatar` with no picture: the box and its loader.
    Loading,
}

/// `AccountCard.svelte`'s props, minus the callbacks. The display
/// pipeline's [`AccountCardItem`] carries every per-account prop; the rest
/// are the workspace's.
#[derive(Clone, Debug)]
pub struct AccountCardProps<'a> {
    pub vars: &'a Vars,
    /// Stable for one mount of the card.
    pub id: Id,
    pub item: &'a AccountCardItem,
    /// `cardColor` resolved, `None` for `""`.
    pub card_color: Option<Rgba>,
    pub avatar: AvatarContent,
    pub focus_visible: bool,
    pub locale: Locale,
    /// The clock and zone the last login reads against.
    pub time: &'a TimeContext,
    /// `usernameBadge`.
    pub badge: Option<&'a UsernameBadge>,
    /// `extensionContent`'s sections.
    pub extension: &'a [CardSection],
    /// `disableExtension`: bulk edit or a drag in progress.
    pub extension_disabled: bool,
    /// `forceExtensionOpen`: the add flow's setup card stays expanded.
    pub extension_forced: bool,
    /// `disableHoverExtension`: another card's setup is open on the tab.
    pub extension_hover_disabled: bool,
    /// The extension's chips that have an `onClick`, as `(section, chip)`.
    pub clickable_chips: &'a [(usize, usize)],
    /// `html[data-glass="1"]`.
    pub glass: bool,
    /// `[data-kb-focus]`: the card the arrow keys moved to.
    pub kb_focus: bool,
    /// `.is-dragging` on the grid: a hovered card stays in place.
    pub grid_dragging: bool,
    /// The drag ghost: its inline `box-shadow` replaces the card's.
    pub ghost: bool,
}

/// The ghost's inline `box-shadow: 0 8px 24px rgba(0,0,0,0.5)`.
pub(crate) const GHOST_SHADOW: BoxShadow = BoxShadow {
    offset: vec2(0.0, 8.0),
    blur: 24.0,
    spread: 0.0,
    color: Rgba::new(0.0, 0.0, 0.0, 0.5),
    inset: false,
};

/// What the card painted and what the user did.
#[derive(Clone, Debug, Default)]
pub struct CardOutput {
    pub events: Vec<CardEvent>,
    /// The extension surface's shapes in the ui's layer, when it showed.
    pub surface: Option<std::ops::Range<usize>>,
    /// `.card-shell.extension-visible`: the card paints over every other.
    pub raised: bool,
    /// The clickable chips the extension showed, for the workspace to sense
    /// over every card.
    pub chips: Vec<ChipHit>,
}

/// What the card reports.
#[derive(Clone, Debug, PartialEq)]
pub enum CardEvent {
    /// Click, key or right click: `onActivate` (selection and focus).
    Activate,
    /// `onSwitch`: the second click of an armed card, or a click with
    /// `singleClickSwitch` (bulk edit).
    Switch,
    /// Right click, the context menu key or Shift+F10: `onContextMenu`.
    ContextMenu(Pos2),
}

/// Per mount state, kept in egui's memory under the card id.
#[derive(Clone, Default)]
struct State {
    /// `showConfirm`.
    confirm: bool,
    /// `isHovered`.
    hovered: bool,
    /// `lastOverflowName` and `cachedOverflowPx`.
    measured: Option<(String, f32)>,
    /// `isOverflowing`: set by a hover, kept until the name changes.
    overflowing: bool,
    /// When the marquee started (the hover).
    marquee_from: f64,
    /// Which loader is mounted: 0 none, 1 deferred, 2 plain.
    loader: u8,
    /// Whether the deferred initials are mounted.
    deferred_fallback: bool,
    /// The blur is on or still fading out.
    blurring: bool,
    /// `panelSide === "left"`, picked on hover.
    opens_left: bool,
    /// The extension was forced open last frame.
    forced: bool,
    /// The clickable chips it showed last frame: inside `.card-shell`, they
    /// keep the card hovered.
    chips: Vec<Id>,
}

fn state(ctx: &Context, id: Id) -> State {
    ctx.data(|d| d.get_temp::<State>(id.with("card-state")))
        .unwrap_or_default()
}

fn set_state(ctx: &Context, id: Id, s: State) {
    ctx.data_mut(|d| d.insert_temp(id.with("card-state"), s));
}

/// Whether the card wants [`AvatarPicture::blurred`] this frame: armed,
/// switching, refreshing, or fading back from one of those.
pub fn wants_blur(ctx: &Context, id: Id, item: &AccountCardItem) -> bool {
    item.switching || item.avatar.refreshing || {
        let s = state(ctx, id);
        s.confirm || s.blurring
    }
}

/// The line boxes the card stacks.
#[derive(Clone, Copy, Debug)]
struct Lines {
    /// The body line of `.name` (16 px, `--line-height`).
    strut: LineBox,
    name: LineBox,
    /// `.meta-stack` lines (10 px, 1.2).
    meta: LineBox,
    note: LineBox,
}

fn line_box(ctx: &Context, style: &TextStyle) -> LineBox {
    style.line_box(ctx).unwrap_or_else(|| {
        let height = match style.line_height {
            LineHeight::Number(n) => n * style.size,
            LineHeight::Px(px) => px,
            LineHeight::Normal => 1.2 * style.size,
        };
        LineBox {
            height,
            baseline: (height + 0.7 * style.size) / 2.0,
        }
    })
}

fn strut_style(v: &Vars) -> TextStyle {
    TextStyle::new(16.0, 400).line_height(LineHeight::Number(v.line_height))
}

fn name_style(v: &Vars) -> TextStyle {
    TextStyle::new(12.0, 500)
        .line_height(LineHeight::Number(1.2))
        .color(v.fg.to_color32())
}

fn meta_style(weight: u16, size: f32, color: Rgba) -> TextStyle {
    TextStyle::new(size, weight)
        .line_height(LineHeight::Number(1.2))
        .color(color.to_color32())
}

impl Lines {
    fn new(ctx: &Context, v: &Vars) -> Self {
        Self {
            strut: line_box(ctx, &strut_style(v)),
            name: line_box(ctx, &name_style(v)),
            meta: line_box(ctx, &meta_style(400, 10.0, v.fg_muted)),
            note: line_box(ctx, &meta_style(500, 9.0, v.fg)),
        }
    }
}

/// What the meta stack holds.
#[derive(Clone, Debug, Default)]
struct Meta {
    /// `.note-info-icon`: a note shown as an icon, not inline.
    icon: bool,
    /// `.username-text` and its max-content width.
    username: Option<f32>,
    badge: bool,
    note: bool,
    last_login: bool,
}

impl Meta {
    fn new(ctx: &Context, v: &Vars, item: &AccountCardItem, badge: bool) -> Self {
        let note = item.note.trim();
        // `isPendingSetupAccount(account.id) ? false : showUsernames`, and
        // the same for the last login.
        let has_username =
            item.show_username && !item.pending_setup && !item.username.trim().is_empty();
        let row = has_username || badge;
        let ppp = ctx.pixels_per_point();
        Self {
            icon: row && !note.is_empty() && !item.show_note_inline,
            username: has_username.then(|| {
                let w = text::measure(ctx, &item.username, &meta_style(400, 10.0, v.fg_muted))
                    .unwrap_or(0.0);
                ceil_unit(w, ppp)
            }),
            badge,
            note: item.show_note_inline && !note.is_empty(),
            last_login: item.show_last_login && !item.pending_setup,
        }
    }

    fn row(&self) -> bool {
        self.username.is_some() || self.badge
    }

    fn any(&self) -> bool {
        self.row() || self.note || self.last_login
    }

    fn row_height(&self, lines: &Lines) -> f32 {
        let mut h: f32 = 0.0;
        if self.icon {
            h = h.max(NOTE_ICON);
        }
        if self.username.is_some() {
            h = h.max(lines.meta.height);
        }
        if self.badge {
            h = h.max(BADGE);
        }
        h
    }

    fn height(&self, lines: &Lines) -> f32 {
        let rows = [
            self.row().then(|| self.row_height(lines)),
            self.note.then_some(lines.note.height),
            self.last_login.then_some(lines.meta.height),
        ];
        let present: Vec<f32> = rows.into_iter().flatten().collect();
        present.iter().sum::<f32>() + META_GAP * present.len().saturating_sub(1) as f32
    }
}

/// A width rounded up to a layout unit (1/64 device px), as Blink sizes an
/// inline box around its text.
fn ceil_unit(w: f32, ppp: f32) -> f32 {
    (w * ppp * 64.0 - 1e-3).ceil() / 64.0 / ppp
}

/// The card's height for `item`: the min height, or its content when the
/// meta lines need more.
pub fn height(ctx: &Context, v: &Vars, item: &AccountCardItem, badge: bool) -> f32 {
    let lines = Lines::new(ctx, v);
    let meta = Meta::new(ctx, v, item, badge);
    content_height(v, &lines, &meta).max(v.grid_card_min_height())
}

fn content_height(v: &Vars, lines: &Lines, meta: &Meta) -> f32 {
    let pad = v.grid_card_padding();
    let mut h = pad + v.grid_card_avatar_size() + AVATAR_GAP + lines.strut.height;
    if meta.any() {
        h += META_TOP + meta.height(lines);
    }
    h + pad
}

/// `stepMarquee` over time: to the end, pause, back, pause. Reduced motion
/// cuts each move to the 0.01 ms transition of `html[data-motion="reduced"]`:
/// the name jumps, the pauses stay.
fn marquee_offset(elapsed: f64, shift: f32, reduced: bool) -> f32 {
    let d = if reduced {
        1e-5
    } else {
        (f64::from(shift) / f64::from(MARQUEE_SPEED) * 1000.0).round() / 1000.0
    };
    if d <= 0.0 || elapsed <= 0.0 {
        return 0.0;
    }
    let m = elapsed % (2.0 * (d + MARQUEE_PAUSE));
    let t = if m < d {
        m / d
    } else if m < d + MARQUEE_PAUSE {
        1.0
    } else if m < 2.0 * d + MARQUEE_PAUSE {
        1.0 - (m - d - MARQUEE_PAUSE) / d
    } else {
        0.0
    };
    -shift * t as f32
}

/// Draws the card over `rect` (its layout box, top left and width; the
/// height follows [`height`]) and returns what the user did.
pub fn account_card(ui: &mut Ui, rect: Rect, props: &AccountCardProps) -> CardOutput {
    let v = props.vars;
    let item = props.item;
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let now = ctx.input(|i| i.time);
    let id = props.id;
    let lines = Lines::new(&ctx, v);
    let meta = Meta::new(&ctx, v, item, props.badge.is_some());
    let card_h = content_height(v, &lines, &meta).max(v.grid_card_min_height());
    // `layout` places the content (layout units); the box paints and senses
    // on its pixel-snapped edges.
    let layout = Rect::from_min_size(rect.min, vec2(rect.width(), card_h));
    let rect = snap::rect(layout, ppp);
    // `interactionDisabled`: the card still takes its clicks and right
    // clicks, so none reaches the workspace, and does nothing with them.
    let disabled = item.pending_setup;
    let response = ui.interact(rect, id, Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| WidgetInfo::selected(Role::Button, !disabled, item.active, &item.name));
    ctx.accesskit_node_builder(id, |node| node.set_role(Role::Button));

    let mut st = state(&ctx, id);
    let hovered = super::css_hovered(&response)
        || st
            .chips
            .iter()
            .any(|chip| ctx.read_response(*chip).is_some_and(|r| r.hovered()));
    let pressed = super::held(&response);
    let active = item.active;
    let dragged = item.dragged;
    let lift = hovered && !active && !dragged;
    let transparent = Rgba::TRANSPARENT;

    // Events first: the armed state they change shows this frame.
    let mut events = Vec::new();
    if dragged {
        st.confirm = false;
    }
    if st.confirm
        && ctx.input(|i| {
            i.pointer.any_pressed() && i.pointer.press_origin().is_some_and(|p| !rect.contains(p))
        })
    {
        st.confirm = false;
    }
    // A press focuses the card like any `<button>`: its keys then reach it.
    let keys = !disabled && super::controls::take_focus(ui, &response);
    let key_click = keys && ctx.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Space));
    let key_menu = keys
        && ctx.input(|i| {
            (i.modifiers.shift && i.key_pressed(Key::F10)) || i.key_pressed(crate::CONTEXT_MENU_KEY)
        });
    if !disabled && !dragged && (super::clicked(&response) || key_click) {
        events.push(CardEvent::Activate);
        if item.single_click_switch {
            events.push(CardEvent::Switch);
        } else if st.confirm {
            st.confirm = false;
            events.push(CardEvent::Switch);
        } else {
            st.confirm = true;
        }
    }
    if !disabled && (response.secondary_clicked() || key_menu) {
        // `getBoundingClientRect()`: the layout box, not its snapped edges.
        let at = if key_menu {
            layout.center()
        } else {
            super::context_click_pos(ui).unwrap_or(rect.center())
        };
        events.push(CardEvent::Activate);
        st.confirm = false;
        events.push(CardEvent::ContextMenu(at));
    }
    let armed = st.confirm && !dragged;

    // The marquee: measured on hover, kept until the name changes.
    let pad = v.grid_card_padding();
    let content_w = layout.width() - 2.0 * pad;
    let name_key = format!("{}|{}", item.name, item.username);
    let style = name_style(v);
    let name_w = ceil_unit(text::measure(&ctx, &item.name, &style).unwrap_or(0.0), ppp);
    if st
        .measured
        .as_ref()
        .is_some_and(|(key, _)| *key != name_key)
    {
        st.measured = None;
        st.overflowing = false;
    }
    let has_extension = has_card_extension_content(props.extension);
    // `updatePanelSide` on hover, and once the panel is forced open.
    let forced = props.extension_forced && has_extension;
    if has_extension && ((hovered && !st.hovered) || (forced && !st.forced)) {
        // `innerWidth - rect.right` in window px: the stage's zoom included,
        // the panel's own width not.
        let zoom = crate::zoom::frame(&ctx);
        st.opens_left =
            !card_extension::opens_right(ctx.content_rect().width() * zoom, rect.right() * zoom);
    }
    st.forced = forced;
    let extension_visible = has_extension
        && !dragged
        && !props.extension_disabled
        && (props.extension_forced || (!props.extension_hover_disabled && hovered));
    if hovered && !st.hovered {
        let overflow = match &st.measured {
            Some((_, px)) => *px,
            None => {
                let px = (name_w.round() - content_w.round()).max(0.0);
                st.measured = Some((name_key.clone(), px));
                px
            }
        };
        st.overflowing = overflow > 2.0;
        st.marquee_from = now;
    }
    st.hovered = hovered;
    let marquee = hovered && st.overflowing;
    let shift = st.measured.as_ref().map_or(0.0, |(_, px)| *px);
    let offset = if marquee {
        let reduced = anim::motion(&ctx) == anim::Motion::Reduced;
        let elapsed = now - st.marquee_from;
        if reduced && elapsed > 0.0 {
            // The name only moves when a pause ends.
            let left = MARQUEE_PAUSE - elapsed.rem_euclid(MARQUEE_PAUSE);
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(left));
        } else {
            ctx.request_repaint();
        }
        marquee_offset(elapsed, shift, reduced)
    } else {
        0.0
    };

    let background = match (props.card_color, active, lift) {
        (Some(c), false, true) => c.mix(0.32, v.bg_card_hover),
        (Some(c), _, _) => c.mix(0.24, v.bg_card),
        (None, true, _) | (None, false, true) => v.bg_card_hover,
        (None, false, false) => v.bg_card,
    };
    // `.card:not(.active):active` (0,3,0) beats `.card.dragging` (0,2,0)
    // and loses to the shell's hover lift (0,5,0); `.is-dragging
    // .card:not(.dragging):hover { transform: none !important }`.
    let (ty, scale) = if dragged && pressed && !active {
        (0.0, 0.985)
    } else if dragged {
        (0.0, 0.95)
    } else if props.grid_dragging && hovered {
        (0.0, 1.0)
    } else if lift {
        (-2.0, 1.0)
    } else if pressed && !active {
        (0.0, 0.985)
    } else {
        (0.0, 1.0)
    };
    let background = anim::transition(&ctx, id.with("bg"), background, &T180);
    let ty = anim::transition(&ctx, id.with("ty"), ty, &T180);
    let scale = anim::transition(&ctx, id.with("scale"), scale, &T180);
    let avatar_bg = anim::transition(
        &ctx,
        id.with("avatar-bg"),
        if lift { v.bg_elevated } else { v.bg_muted },
        &T150,
    );
    let avatar_lift = anim::transition(
        &ctx,
        id.with("avatar-ty"),
        if lift { -1.0f32 } else { 0.0 },
        &T180,
    );
    let zoom = anim::transition(
        &ctx,
        id.with("img-scale"),
        if lift { 1.04f32 } else { 1.0 },
        &T220,
    );
    let blur_on = item.avatar.refreshing || st.confirm || item.switching;
    let blur = anim::transition_bool(&ctx, id.with("blur"), blur_on, &T300);
    st.blurring = blur_on || blur > 0.0;
    // `.initials.blurred-text`: armed or switching, not refreshing.
    let text_blur = anim::transition_bool(
        &ctx,
        id.with("text-blur"),
        st.confirm || item.switching,
        &T300,
    );

    let (enter_opacity, enter_scale) = entrance(ui, id, v);
    let entering = enter_opacity < 1.0;
    let transform = if entering {
        Transform::scale(enter_scale)
    } else {
        Transform {
            translate: vec2(0.0, ty),
            scale,
        }
    };
    // A scaled box sits on whole device pixels and lays its content out
    // from there.
    let (layout, rect) = if transform.scale != 1.0 {
        let layout = layout.translate(super::snap_offset(&ctx, layout.min));
        (layout, snap::rect(layout, ppp))
    } else {
        (layout, rect)
    };
    // `.dragging { opacity: .4 }`: one layer, as the folder card's.
    // `animation` overrides `opacity` too: a dragged card moved by the
    // preview replays its entrance from 0 to 1, then drops back to .4.
    let group = dragged && !entering && !cfg!(feature = "glow");
    if group {
        super::folder_card::drag_group(ui, rect);
    }
    let opacity = enter_opacity
        * if dragged && !group && !entering {
            0.4
        } else {
            1.0
        };

    // The extension surface, under the face.
    let mut surface = None;
    let mut chips = Vec::new();
    if has_extension {
        let layer = ui.layer_id();
        let next = || ctx.graphics(|g| g.get(layer).map_or(0, |l| l.next_idx().0));
        let start = next();
        let painted = card_extension::paint(
            ui,
            layout,
            &Surface {
                vars: v,
                id,
                sections: props.extension,
                clickable: props.clickable_chips,
                locale: props.locale,
                card_color: props.card_color,
                active,
                visible: extension_visible,
                lift: hovered && !active,
                right: !st.opens_left,
                ban: item.warning.as_ref().and_then(|w| w.card_outline_tone),
                glass: props.glass,
            },
        );
        if painted.shown {
            surface = Some(start..next());
        }
        chips = painted.chips;
    }
    st.chips = chips.iter().map(|hit| hit.id).collect();

    // Shadows: the active rings, else the hover shadow. Ring lists and the
    // drop shadow interpolate as lists, padded with transparent shadows.
    // While the extension shows, the rings are the surface's.
    let shadows = if props.ghost {
        vec![GHOST_SHADOW]
    } else if active && extension_visible {
        Vec::new()
    } else if active {
        let (halo, band, band_w) = match props.card_color {
            Some(c) => (
                v.fg.mix(0.72, transparent),
                c.mix(0.5, v.bg_solid.mix(0.62, transparent)),
                5.0,
            ),
            None => (
                v.fg.mix(0.62, transparent),
                v.bg_solid.mix(0.45, transparent),
                4.0,
            ),
        };
        vec![ring(2.0, halo), ring(band_w, band)]
    } else if lift {
        vec![BoxShadow {
            offset: vec2(0.0, 12.0),
            blur: 24.0,
            spread: 0.0,
            color: Rgba::new(0.0, 0.0, 0.0, 0.18),
            inset: false,
        }]
    } else {
        Vec::new()
    };
    BoxStyle {
        background,
        images: v.card_bg_image.clone(),
        radius: Corners::all(v.radius_md),
        shadows,
        opacity,
        ..BoxStyle::default()
    }
    .paint_transformed(ui.painter(), rect, transform);

    let mut painter = ui.painter().clone();
    painter.multiply_opacity(opacity);

    // Avatar box.
    let size = v.grid_card_avatar_size();
    let avatar = Rect::from_min_size(
        pos2(
            layout.left() + pad + snap::centre(content_w, size, ppp),
            layout.top() + pad,
        ),
        vec2(size, size),
    );
    let lifted = avatar.translate(vec2(0.0, avatar_lift));
    let mapped = map_rect(layout, transform, lifted);
    let avatar_transform = Transform {
        translate: mapped.center() - avatar.center(),
        scale: transform.scale,
    };
    BoxStyle {
        background: avatar_bg,
        radius: Corners::all(v.avatar_radius),
        ..BoxStyle::default()
    }
    .paint_transformed(&painter, avatar, avatar_transform);
    // A lifted picture keeps the fractional `translateY` its frame paints
    // at; under a scaled card it starts from whole device pixels.
    let (frame, frame_transform, radius) = if transform.scale == 1.0 {
        (avatar, avatar_transform, Corners::all(v.avatar_radius))
    } else {
        (
            snap::rect(mapped, ppp),
            Transform::NONE,
            Corners::all(v.avatar_radius * transform.scale),
        )
    };
    let mut fallback_deferred = false;
    match &props.avatar {
        AvatarContent::Picture(picture) => {
            let reveal = avatar_fade_in::avatar_opacity(&ctx, id, true, v);
            if reveal > 0.0 {
                paint_picture(
                    &painter,
                    picture,
                    (frame, frame_transform),
                    zoom,
                    radius,
                    reveal,
                    blur,
                );
            }
        }
        AvatarContent::Decoding => {
            avatar_fade_in::avatar_opacity(&ctx, id, false, v);
        }
        AvatarContent::Initials {
            background,
            gradient,
        } => {
            // `.avatar-fallback.deferred`: held back while no avatar state
            // is known, so a fast profile never flashes a placeholder.
            fallback_deferred = item.avatar.pending;
            let fade = if fallback_deferred {
                placeholder_in(&ctx, id.with("fallback-in"), v)
            } else {
                1.0
            };
            let mut layer = painter.clone();
            layer.multiply_opacity(fade);
            BoxStyle {
                background: *background,
                images: gradient.iter().cloned().collect(),
                radius: Corners::all(v.avatar_radius),
                ..BoxStyle::default()
            }
            .paint_transformed(&layer, avatar, avatar_transform);
            // `.initials.blurred-text`: `blur(4px) brightness(.5)`, the
            // filter interpolated as one. The blur is the glyph coverage
            // through a gaussian of sigma 4 px, as a `text-shadow` of blur
            // radius 8 px in place of the text.
            let dim = 1.0 - 0.5 * text_blur;
            let fg = v.fg;
            let color = Rgba::new(fg.r * dim, fg.g * dim, fg.b * dim, fg.a);
            let style = TextStyle::new(20.0, 600)
                .line_height(LineHeight::Number(1.5))
                .nowrap(false)
                .align(TextAlign::Center)
                .color(color.to_color32());
            let line = line_box(&ctx, &style).height;
            let text_rect = Rect::from_min_size(
                pos2(mapped.left(), mapped.top() + snap::centre(size, line, ppp)),
                vec2(mapped.width(), line),
            );
            let initials = avatar_initials(&item.name);
            if text_blur > 0.0 {
                if let Some(layout) = text::layout(&ctx, &initials, &style, text_rect) {
                    let shadow = TextShadow {
                        offset: Vec2::ZERO,
                        blur: 8.0 * text_blur,
                        color,
                    };
                    text_shadow::paint(&layer, &layout, &[shadow], None, 1.0);
                }
            } else {
                text::paint(&layer, text_rect, &initials, &style);
            }
        }
        AvatarContent::Loading => {}
    }
    if fallback_deferred != st.deferred_fallback || !fallback_deferred {
        if !fallback_deferred {
            anim::restart(&ctx, id.with("fallback-in"));
        }
        st.deferred_fallback = fallback_deferred;
    }

    // Loaders: `isLoadingAvatar` (deferred), else refreshing or switching.
    let loader = if item.avatar.loading {
        1
    } else if item.avatar.refreshing || item.switching {
        2
    } else {
        0
    };
    if loader != st.loader {
        anim::restart(&ctx, id.with("loader-in"));
        anim::restart(&ctx, id.with("loader-spin"));
        st.loader = loader;
    }
    if loader != 0 {
        let fade = if loader == 1 {
            placeholder_in(&ctx, id.with("loader-in"), v)
        } else {
            1.0
        };
        let turn = anim::animate(
            &ctx,
            id.with("loader-spin"),
            &Keyframes::from_to(0.0f32, 1.0),
            &Animation::new(LOADER_SPIN, Timing::LINEAR).infinite(),
            0.0,
        );
        paint_loader(&painter, mapped.center(), turn, fade, v);
        ctx.request_repaint();
    }

    // Play overlay over the avatar while armed.
    if !armed {
        anim::restart(&ctx, id.with("confirm-in"));
    }
    let shown = if armed {
        anim::animate(
            &ctx,
            id.with("confirm-in"),
            &Keyframes::from_to(0.0f32, 1.0),
            &FADE_IN,
            1.0,
        )
    } else {
        0.0
    };
    if armed {
        let play = Rect::from_center_size(mapped.center(), vec2(24.0, 24.0));
        icon::paint(
            &painter,
            PLAY_SVG,
            play,
            v.fg.alpha(v.fg.a * shown),
            0.8 + 0.2 * shown,
        );
    }
    if active {
        // `.avatar.active { outline: 2px solid color-mix(--fg 20%) }`.
        BoxStyle {
            radius: Corners::all(v.avatar_radius),
            shadows: vec![ring(
                snap::border_width(2.0, ppp),
                v.fg.mix(0.2, transparent),
            )],
            ..BoxStyle::default()
        }
        .paint_transformed(&painter, avatar, avatar_transform);
    }

    // Name.
    let content_left = layout.left() + pad;
    let name_top = layout.top() + pad + size + AVATAR_GAP;
    let name_box = Rect::from_min_size(
        pos2(content_left, name_top),
        vec2(content_w, lines.strut.height),
    );
    let (inner, ellipsis) = if st.overflowing {
        // `.name.marquee`: the inline-block is as wide as its text, aligned
        // on its text baseline, and starts at the line's start.
        let top = name_top + lines.strut.baseline - lines.name.baseline;
        (
            Rect::from_min_size(
                pos2(content_left + offset, top),
                vec2(name_w, lines.name.height),
            ),
            false,
        )
    } else {
        let w = name_w.min(content_w);
        let top = name_top + lines.strut.baseline - lines.name.height;
        (
            Rect::from_min_size(
                pos2(content_left + snap::centre(content_w, w, ppp), top),
                vec2(w, lines.name.height),
            ),
            name_w > content_w,
        )
    };
    let clip = if st.overflowing {
        name_box
    } else {
        inner.intersect(name_box)
    };
    // Under a scale (dragged, entering) the text is laid out at its
    // transformed size and its boxes mapped; the hover lift only moves it.
    let scaled = transform.scale != 1.0;
    let scope = scaled.then(|| {
        let s = transform.scale;
        crate::ui::components::TransformScope::begin(
            &ctx,
            painter.layer_id(),
            egui::emath::TSTransform::new(
                layout.center().to_vec2() * (1.0 - s) + transform.translate,
                s,
            ),
        )
    });
    let shift_by = |r: Rect| {
        if scaled {
            return r;
        }
        let m = map_rect(layout, transform, r);
        Rect::from_center_size(m.center(), r.size())
    };
    let mut name_painter = painter.clone();
    name_painter.set_clip_rect(painter.clip_rect().intersect(shift_by(clip)));
    text::paint(
        &name_painter,
        shift_by(inner),
        &item.name,
        &style.clone().nowrap(ellipsis).align(TextAlign::Left),
    );

    // Meta stack.
    if meta.any() {
        let mut y = name_top + lines.strut.height + META_TOP;
        if meta.row() {
            let row_h = meta.row_height(&lines);
            paint_username_row(
                &painter,
                &|r| shift_by(r),
                Rect::from_min_size(pos2(content_left, y), vec2(content_w, row_h)),
                &meta,
                &lines,
                props,
            );
            y += row_h + META_GAP;
        }
        if meta.note {
            let r = Rect::from_min_size(pos2(content_left, y), vec2(content_w, lines.note.height));
            let style = meta_style(500, 9.0, v.fg)
                .nowrap(true)
                .align(TextAlign::Center);
            text::paint(&painter, shift_by(r), item.note.trim(), &style);
            y += lines.note.height + META_GAP;
        }
        if meta.last_login {
            let r = Rect::from_min_size(pos2(content_left, y), vec2(content_w, lines.meta.height));
            let label = format_relative_time_compact(
                item.last_login_at_sec,
                props.time,
                item.last_login_unknown_key,
            );
            let style = meta_style(500, 10.0, v.fg_subtle.mix(0.4, v.fg))
                .nowrap(true)
                .align(TextAlign::Center);
            text::paint(&painter, shift_by(r), &label, &style);
        }
    }
    if let Some(scope) = scope {
        scope.end(&ctx, Some(painter.clip_rect()));
    }

    // `.confirm-hint`, positioned at the card's bottom.
    if armed {
        paint_confirm_hint(&painter, layout, transform, shown, props);
    }

    // Ban outlines, after the content like any outline; the surface's
    // while the extension shows. They replace the focus ring's colour and
    // width but keep its `outline-offset: 2px`.
    let ring_on = props.kb_focus || (props.focus_visible && response.has_focus());
    let (ring_rect, ring_radius) = if ring_on {
        (rect.expand(2.0), v.radius_md + 2.0)
    } else {
        (rect, v.radius_md)
    };
    let banned = item.warning.as_ref().and_then(|w| w.card_outline_tone);
    let tone = banned.filter(|_| !extension_visible);
    if let Some(tone) = tone {
        let base = match tone {
            WarningTone::Red => BAN_RED,
            WarningTone::Orange => BAN_YELLOW,
        };
        let color = if props.card_color.is_some() {
            base.alpha(1.0)
        } else {
            base
        };
        BoxStyle {
            radius: Corners::all(ring_radius),
            shadows: vec![ring(snap::border_width(2.0, ppp), color)],
            opacity,
            ..BoxStyle::default()
        }
        .paint_transformed(ui.painter(), ring_rect, transform);
    }

    // `[data-kb-focus]` and `:focus-visible`, under the card's transform.
    if ring_on && banned.is_none() {
        BoxStyle {
            radius: Corners::all(ring_radius),
            shadows: vec![ring(
                snap::border_width(2.0, ppp),
                v.fg.mix(0.65, Rgba::TRANSPARENT),
            )],
            opacity,
            ..BoxStyle::default()
        }
        .paint_transformed(ui.painter(), ring_rect, transform);
    }
    if group {
        crate::ui::backdrop_filter::Mark::End.paint(ui.painter(), rect);
    }
    set_state(&ctx, id, st);
    CardOutput {
        events,
        surface,
        raised: extension_visible,
        chips,
    }
}

/// `avatar-placeholder-in var(--motion-avatar-fade) ease-out
/// var(--motion-avatar-defer) backwards`, from its mount.
fn placeholder_in(ctx: &Context, id: Id, v: &Vars) -> f32 {
    anim::animate(
        ctx,
        id,
        &Keyframes::from_to(0.0f32, 1.0),
        &Animation::new(v.motion_avatar_fade, Timing::EASE_OUT)
            .with_delay(v.motion_avatar_defer)
            .with_fill(Fill::Backwards),
        1.0,
    )
}

/// The `<img>`: its fade in, its hover zoom and its filter, the blurred copy
/// cross-faded over the 300 ms transition.
fn paint_picture(
    painter: &egui::Painter,
    picture: &AvatarPicture,
    (rect, transform): (Rect, Transform),
    zoom: f32,
    radius: Corners,
    reveal: f32,
    blur: f32,
) {
    let tint = |alpha: f32, dim: f32| {
        let a = (alpha * 255.0).round().clamp(0.0, 255.0) as u8;
        let g = (alpha * dim * 255.0).round().clamp(0.0, 255.0) as u8;
        Color32::from_rgba_premultiplied(g, g, g, a)
    };
    match picture.blurred {
        Some(blurred) if blur > 0.0 => {
            if blur < 1.0 {
                let uv = cover_uv(picture.size, zoom);
                paint::paint_image_transformed(
                    painter,
                    picture.texture,
                    rect,
                    transform,
                    uv,
                    radius,
                    tint(reveal * (1.0 - blur), 1.0),
                );
            }
            paint::paint_image_transformed(
                painter,
                blurred,
                rect,
                transform,
                cover_uv([1, 1], zoom),
                radius,
                tint(reveal * blur, 1.0),
            );
        }
        _ => {
            paint::paint_image_transformed(
                painter,
                picture.texture,
                rect,
                transform,
                cover_uv(picture.size, zoom),
                radius,
                tint(reveal, 1.0 - 0.5 * blur),
            );
        }
    }
}

/// `.loader`: a 20 px ring, `2px solid --bg-elevated` with an `--fg` top
/// border, turned by `turn` (0 to 1).
fn paint_loader(painter: &egui::Painter, centre: Pos2, turn: f32, fade: f32, v: &Vars) {
    if fade <= 0.0 {
        return;
    }
    let rest = v.bg_elevated.alpha(v.bg_elevated.a * fade);
    let top = v.fg.alpha(v.fg.a * fade);
    paint_ring_loader(painter, centre, LOADER, 2.0, turn, rest, top);
}

/// A spinner ring: `size` px across its border box (preflight's
/// `box-sizing: border-box`), a `border` px border in `rest` with a `top`
/// coloured top border, turned by `turn` (0 to 1).
pub(crate) fn paint_ring_loader(
    painter: &egui::Painter,
    centre: Pos2,
    size: f32,
    border: f32,
    turn: f32,
    rest: Rgba,
    top: Rgba,
) {
    let ppp = painter.ctx().pixels_per_point();
    let bw = snap::border_width(border, ppp);
    let outer = size / 2.0;
    let inner = outer - bw;
    let rest = rest.to_color32();
    let top = top.to_color32();
    let aa = 0.5 / ppp;
    let mut mesh = Mesh::default();
    let segments = 96;
    let rotation = turn * std::f32::consts::TAU;
    // Screen angles (y down): the top border spans -135 to -45 degrees.
    let q = std::f32::consts::FRAC_PI_4;
    for k in 0..segments {
        let a0 = k as f32 / segments as f32 * std::f32::consts::TAU;
        let a1 = (k + 1) as f32 / segments as f32 * std::f32::consts::TAU;
        let mid = (a0 + a1) / 2.0;
        // Unrotated angle relative to straight up.
        let rel = (mid + std::f32::consts::FRAC_PI_2).rem_euclid(std::f32::consts::TAU);
        let color = if !(q..=std::f32::consts::TAU - q).contains(&rel) {
            top
        } else {
            rest
        };
        let radii = [inner - aa, inner + aa, outer - aa, outer + aa];
        let alphas = [0.0, 1.0, 1.0, 0.0];
        let base = mesh.vertices.len() as u32;
        for a in [a0 + rotation, a1 + rotation] {
            let dir = vec2(a.cos(), a.sin());
            for (r, alpha) in radii.iter().zip(alphas) {
                mesh.vertices.push(Vertex {
                    pos: centre + dir * *r,
                    uv: WHITE_UV,
                    color: color.gamma_multiply(alpha),
                });
            }
        }
        for j in 0..3u32 {
            let (a, b) = (base + j, base + j + 1);
            let (c, d) = (base + 4 + j, base + 5 + j);
            mesh.add_triangle(a, b, d);
            mesh.add_triangle(a, d, c);
        }
    }
    painter.add(Shape::mesh(mesh));
}

/// The username row: the note icon, the username (shrinking, with an
/// ellipsis) and the badge, centred, 4 px apart.
fn paint_username_row(
    painter: &egui::Painter,
    place: &dyn Fn(Rect) -> Rect,
    row: Rect,
    meta: &Meta,
    lines: &Lines,
    props: &AccountCardProps,
) {
    let v = props.vars;
    let ctx = painter.ctx();
    let ppp = ctx.pixels_per_point();
    let icon_w = NOTE_ICON;
    let fixed: f32 = [meta.icon.then_some(icon_w), meta.badge.then_some(BADGE)]
        .into_iter()
        .flatten()
        .sum();
    let count =
        usize::from(meta.icon) + usize::from(meta.username.is_some()) + usize::from(meta.badge);
    let gaps = ROW_GAP * count.saturating_sub(1) as f32;
    let text_w = meta
        .username
        .map(|w| w.min((row.width() - fixed - gaps).max(0.0)));
    let total = fixed + gaps + text_w.unwrap_or(0.0);
    let mut x = row.left() + snap::centre(row.width(), total, ppp);
    if meta.icon {
        let r = Rect::from_min_size(
            pos2(x, row.top() + snap::centre(row.height(), icon_w, ppp)),
            vec2(icon_w, icon_w),
        );
        let placed = place(r);
        let bw = snap::border_width(1.0, ppp);
        BoxStyle {
            radius: Corners::all(icon_w / 2.0),
            border: Border::all(bw, v.fg.mix(0.92, Rgba::TRANSPARENT)),
            ..BoxStyle::default()
        }
        .paint(painter, snap::rect(placed, ppp));
        let style = TextStyle::new(8.0, 700)
            .line_height(LineHeight::Number(1.0))
            .nowrap(false)
            .align(TextAlign::Center)
            .color(v.fg.to_color32());
        let lb = line_box(ctx, &style).height;
        let content = NOTE_ICON - 2.0 * bw;
        let inner = Rect::from_min_size(
            pos2(
                placed.left() + bw,
                placed.top() + bw + snap::centre(content, lb, ppp),
            ),
            vec2(content, lb),
        );
        text::paint(painter, inner, "i", &style);
        x += icon_w + ROW_GAP;
    }
    if let (Some(w), Some(full)) = (text_w, meta.username) {
        let r = Rect::from_min_size(
            pos2(
                x,
                row.top() + snap::centre(row.height(), lines.meta.height, ppp),
            ),
            vec2(w, lines.meta.height),
        );
        let style = meta_style(400, 10.0, v.fg_muted)
            .nowrap(full > w)
            .align(TextAlign::Center);
        text::paint(painter, place(r), &props.item.username, &style);
        x += w + ROW_GAP;
    }
    if let Some(badge) = props.badge.filter(|_| meta.badge) {
        let color = match badge.tone {
            ChipTone::Green => BADGE_GREEN,
            _ => v.fg_subtle.mix(0.75, Rgba::TRANSPARENT),
        };
        let r = Rect::from_min_size(
            pos2(x, row.top() + snap::centre(row.height(), BADGE, ppp)),
            vec2(BADGE, BADGE),
        );
        icon::paint(painter, BADGE_SVG, place(r), color, 1.0);
    }
}

/// `.confirm-hint`: `left` and `right: --grid-card-padding`, `bottom: 4px`,
/// a `color-mix(--bg-solid 82%)` scrim, 9 px 600 text that wraps.
fn paint_confirm_hint(
    painter: &egui::Painter,
    card: Rect,
    transform: Transform,
    shown: f32,
    props: &AccountCardProps,
) {
    let v = props.vars;
    let ctx = painter.ctx();
    let pad = v.grid_card_padding();
    let label = translate(props.locale, "card.clickAgainToSwitch");
    // The hint wraps (`white-space: normal`).
    let style = TextStyle::new(9.0, 600)
        .line_height(LineHeight::Number(1.2))
        .align(TextAlign::Center)
        .color(v.fg.alpha(v.fg.a * shown).to_color32());
    let lb = line_box(ctx, &style);
    let w = card.width() - 2.0 * pad;
    let text_w = w - 2.0 * HINT_PAD.x;
    let lines = text::layout(
        ctx,
        &label,
        &style,
        Rect::from_min_size(Pos2::ZERO, vec2(text_w, lb.height)),
    )
    .map_or(1, |l| l.lines.max(1));
    let h = lines as f32 * lb.height + 2.0 * HINT_PAD.y;
    let hint = Rect::from_min_size(
        pos2(card.left() + pad, card.bottom() - HINT_BOTTOM - h),
        vec2(w, h),
    );
    let s = 0.8 + 0.2 * shown;
    let mapped = map_rect(card, transform, hint);
    BoxStyle {
        background: v.bg_solid.mix(0.82, Rgba::TRANSPARENT),
        radius: Corners::all(v.radius_sm),
        opacity: shown,
        ..BoxStyle::default()
    }
    .paint_transformed(
        painter,
        hint,
        Transform {
            translate: mapped.center() - hint.center(),
            scale: transform.scale * s,
        },
    );
    let scaled = TextStyle {
        size: style.size * s,
        ..style
    };
    let text_h = lines as f32 * lb.height * s;
    let text_rect = Rect::from_min_size(
        pos2(
            mapped.center().x - text_w * s / 2.0,
            mapped.center().y - text_h / 2.0,
        ),
        vec2(text_w * s, text_h),
    );
    text::paint(painter, text_rect, &label, &scaled);
}

/// `0 0 0 <spread> color`.
fn ring(spread: f32, color: Rgba) -> BoxShadow {
    BoxShadow {
        offset: egui::Vec2::ZERO,
        blur: 0.0,
        spread,
        color,
        inset: false,
    }
}

/// `object-fit: cover` of a `size` picture in a square box, then `scale()`
/// about the centre (the box clips it).
pub(crate) fn cover_uv(size: [usize; 2], zoom: f32) -> Rect {
    let (w, h) = (size[0].max(1) as f32, size[1].max(1) as f32);
    let (uw, uh) = if w > h { (h / w, 1.0) } else { (1.0, w / h) };
    let (uw, uh) = (uw / zoom, uh / zoom);
    Rect::from_center_size(pos2(0.5, 0.5), vec2(uw, uh))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_confirm_hint_wraps_as_in_blink() {
        // `.confirm-hint` on a 110 px card: 84 px wide, 78 px of text, and
        // "Click again to switch" is 83.3 px: two lines (25.58 px tall in
        // Blink, 2 x 10.8 + 4).
        let ctx = Context::default();
        text::install(&ctx, false);
        let style = TextStyle::new(9.0, 600)
            .line_height(LineHeight::Number(1.2))
            .align(TextAlign::Center);
        let mut lines = 0;
        for t in 0..2 {
            let input = egui::RawInput {
                time: Some(f64::from(t)),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                let r = Rect::from_min_size(Pos2::ZERO, vec2(78.0, 20.0));
                if let Some(l) = text::layout(ui.ctx(), "Click again to switch", &style, r) {
                    lines = l.lines;
                }
            });
            out.textures_delta.clear();
        }
        assert_eq!(lines, 2);
    }

    #[test]
    fn cover_crops_the_long_side() {
        let square = cover_uv([64, 64], 1.0);
        assert_eq!(square, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)));
        let wide = cover_uv([200, 100], 1.0);
        assert!((wide.width() - 0.5).abs() < 1e-6 && (wide.height() - 1.0).abs() < 1e-6);
        let zoomed = cover_uv([64, 64], 2.0);
        assert!((zoomed.width() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn marquee_runs_to_the_end_pauses_and_comes_back() {
        // 42 px at 42 px/s: one second each way, two seconds of pause.
        assert_eq!(marquee_offset(0.0, 42.0, false), 0.0);
        assert!((marquee_offset(0.5, 42.0, false) + 21.0).abs() < 1e-4);
        assert_eq!(marquee_offset(1.0, 42.0, false), -42.0);
        assert_eq!(marquee_offset(2.9, 42.0, false), -42.0);
        assert!((marquee_offset(3.5, 42.0, false) + 21.0).abs() < 1e-4);
        assert_eq!(marquee_offset(5.0, 42.0, false), 0.0);
        assert!((marquee_offset(6.5, 42.0, false) + 21.0).abs() < 1e-4);
    }

    #[test]
    fn reduced_marquee_jumps_and_keeps_the_pauses() {
        assert_eq!(marquee_offset(0.0, 42.0, true), 0.0);
        assert_eq!(marquee_offset(0.1, 42.0, true), -42.0);
        assert_eq!(marquee_offset(1.9, 42.0, true), -42.0);
        assert_eq!(marquee_offset(2.1, 42.0, true), 0.0);
        assert_eq!(marquee_offset(4.1, 42.0, true), -42.0);
    }

    #[test]
    fn layout_units_round_up() {
        assert_eq!(ceil_unit(28.65, 1.5), 2751.0 / 64.0 / 1.5);
        assert_eq!(ceil_unit(10.0, 1.0), 10.0);
    }
}
