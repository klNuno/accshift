//! The account card: one account in the grid, its avatar, name and meta
//! lines, with the extension panel under it.
//!
//! - The box: the grid card width, at least its min height (it grows with
//!   its meta lines), the grid card padding, a medium radius, the card
//!   background under the theme's card image. Background, transform and
//!   shadow ease over 180 ms; the card fades and scales in on mount.
//! - Hover, when not active: the hover background, 2 px up, a `0 12px 24px`
//!   shadow at 18 % black. A press scales it to 0.985.
//! - A custom colour: 24 % of it over the card background, 32 % over the
//!   hover one.
//! - Active: the hover background and two rings, 2 px of the foreground at
//!   62 % and 4 px of the solid background at 45 %; with a custom colour,
//!   2 px of the foreground at 72 % and 5 px of the colour over the solid
//!   background.
//! - A ban warning: a 2 px red or yellow outline at 60 %, opaque on a
//!   custom colour.
//! - The avatar: centred, 8 px over the name, the avatar radius, the muted
//!   background. On hover its box turns elevated and lifts 1 px, the
//!   picture zooms to 1.04; an active card outlines it 2 px. The picture
//!   blurs and dims to half over 300 ms while armed, switching or
//!   refreshing; the initials while armed or switching.
//! - The name: 12 px 500 on a 16 px body line. It ends in an ellipsis; once
//!   a hover found it overflowing, it scrolls at 42 px/s with 2 s pauses.
//! - The meta stack, 4 px under the name, its lines 2 px apart: the
//!   username row (note icon, username, badge), the inline note and the
//!   last login.
//! - Armed (the first press): a play triangle over the avatar and a "click
//!   again" hint at the bottom, both fading in over 150 ms.
//!
//! The extension panel: [`super::card_extension`], under the face.
//!
//! One frame runs, in order: [`measure`] (line boxes, meta lines, height),
//! [`input`] (sensing, hover, arming), [`marquee`] (the name's fit and
//! scroll), [`extension`] (the panel's side and surface), [`look`] (the
//! eased values and the box), [`avatar`], [`labels`] (name and meta stack),
//! [`confirm`] (the armed overlays) and the outlines of [`look`].

mod avatar;
mod confirm;
mod extension;
mod input;
mod labels;
mod look;
mod marquee;
mod measure;

use egui::{vec2, Context, Id, Painter, Pos2, Rect, TextureId, Ui};

use accshift_app::display::AccountCardItem;
use accshift_app::i18n::Locale;
use accshift_app::platforms::card::{CardSection, UsernameBadge};
use accshift_app::time::TimeContext;

use super::card_extension::ChipHit;
use super::folder_card::{map_rect, Placement};
use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim;
use crate::ui::paint::{Gradient, Transform};

pub(crate) use super::folder_card::GHOST_SHADOW;
pub(crate) use avatar::{cover_uv, paint_ring_loader};
pub use measure::height;

use measure::{Lines, Meta};

/// The decoded avatar, as the workspace's texture cache hands it over.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AvatarPicture {
    pub texture: TextureId,
    /// The texture's size in device pixels, to cover the avatar box.
    pub size: [usize; 2],
    /// The same picture covered into the avatar box, then blurred 6 px and
    /// dimmed to half, square: present while [`wants_blur`] holds.
    pub blurred: Option<TextureId>,
}

/// What the avatar box shows.
#[derive(Clone, Debug, PartialEq)]
pub enum AvatarContent {
    /// The account has a picture and it is decoded.
    Picture(AvatarPicture),
    /// The account has a picture, still decoding: nothing shows yet, the
    /// fade in waits for it.
    Decoding,
    /// No picture: the initials on the account's seeded background colour
    /// and gradient.
    Initials {
        background: Rgba,
        gradient: Option<Gradient>,
    },
    /// The picture is loading and none is known: the box and its loader.
    Loading,
}

/// What the workspace hands an account card. The display pipeline's
/// [`AccountCardItem`] carries every per-account value; the rest are the
/// workspace's.
#[derive(Clone, Debug)]
pub struct AccountCardProps<'a> {
    pub vars: &'a Vars,
    /// Stable for one mount of the card.
    pub id: Id,
    pub item: &'a AccountCardItem,
    /// The account's colour, `None` when it has none.
    pub card_color: Option<Rgba>,
    pub avatar: AvatarContent,
    pub focus_visible: bool,
    pub locale: Locale,
    /// The clock and zone the last login reads against.
    pub time: &'a TimeContext,
    /// The badge after the username, if the platform shows one.
    pub badge: Option<&'a UsernameBadge>,
    /// The extension panel's sections.
    pub extension: &'a [CardSection],
    /// No extension panel: bulk edit or a drag in progress.
    pub extension_disabled: bool,
    /// The add flow's setup card keeps its panel open.
    pub extension_forced: bool,
    /// Another card's setup is open on the tab: hovering opens no panel.
    pub extension_hover_disabled: bool,
    /// The extension's chips that react to a click, as `(section, chip)`.
    pub clickable_chips: &'a [(usize, usize)],
    /// The glass theme is on.
    pub glass: bool,
    /// The card the arrow keys moved to.
    pub kb_focus: bool,
    /// A drag runs on the grid: a hovered card stays in place.
    pub grid_dragging: bool,
    /// The drag ghost: its own shadow replaces the card's.
    pub ghost: bool,
}

/// What the card painted and what the user did.
#[derive(Clone, Debug, Default)]
pub struct CardOutput {
    pub events: Vec<CardEvent>,
    /// The extension surface's shapes in the ui's layer, when it showed.
    pub surface: Option<std::ops::Range<usize>>,
    /// The extension panel shows: the card paints over every other.
    pub raised: bool,
    /// The clickable chips the extension showed, for the workspace to sense
    /// over every card.
    pub chips: Vec<ChipHit>,
}

/// What the card reports.
#[derive(Clone, Debug, PartialEq)]
pub enum CardEvent {
    /// Click, key or right click: select and focus the account.
    Activate,
    /// Switch to the account: the second press of an armed card, or any
    /// press when one press switches (bulk edit).
    Switch,
    /// Right click, the context menu key or Shift+F10: open the menu here.
    ContextMenu(Pos2),
}

/// Which loader the avatar shows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Loader {
    #[default]
    None,
    /// The picture is loading: the loader waits, then fades in.
    Deferred,
    /// Refreshing or switching: the loader shows at once.
    Plain,
}

/// Per mount state, kept in egui's memory under the card id.
#[derive(Clone, Default)]
struct State {
    /// Armed: the next press switches.
    confirm: bool,
    /// The card had the keyboard focus last frame.
    had_focus: bool,
    /// The card was hovered last frame.
    hovered: bool,
    /// The name and username the overflow was measured for, and the
    /// overflow in px.
    measured: Option<(String, f32)>,
    /// The name overflows: set by a hover, kept until the name changes.
    overflowing: bool,
    /// When the marquee started (the hover).
    marquee_from: f64,
    /// The loader shown last frame.
    loader: Loader,
    /// The blur is on or still fading out.
    blurring: bool,
    /// The extension panel opens to the left, picked on hover.
    opens_left: bool,
    /// The extension was forced open last frame.
    forced: bool,
    /// The clickable chips it showed last frame: they belong to the card
    /// and keep it hovered.
    chips: Vec<Id>,
}

fn state(ctx: &Context, id: Id) -> State {
    ctx.data(|d| d.get_temp::<State>(id.with("card-state")))
        .unwrap_or_default()
}

fn set_state(ctx: &Context, id: Id, s: State) {
    let key = id.with("card-state");
    ctx.data_mut(|d| {
        anim::own::<State>(d, key);
        d.insert_temp(key, s);
    });
}

/// Whether the card wants [`AvatarPicture::blurred`] this frame: armed,
/// switching, refreshing, or fading back from one of those.
pub fn wants_blur(ctx: &Context, id: Id, item: &AccountCardItem) -> bool {
    item.switching || item.avatar.refreshing || {
        let s = state(ctx, id);
        s.confirm || s.blurring
    }
}

/// What every phase of a frame reads: the props, the measured lines and
/// where the box sits.
struct Card<'a> {
    ctx: Context,
    props: &'a AccountCardProps<'a>,
    ppp: f32,
    lines: Lines,
    meta: Meta,
    /// At rest until the frame's pose places the box.
    place: Placement,
}

impl<'a> Card<'a> {
    /// The card over `rect` (top left and width): its height follows its
    /// meta lines.
    fn measure(ctx: &Context, props: &'a AccountCardProps<'a>, rect: Rect) -> Self {
        let v = props.vars;
        let ppp = ctx.pixels_per_point();
        let lines = Lines::new(ctx, v);
        let meta = Meta::new(ctx, v, props.item, props.badge.is_some());
        let card_h = measure::content_height(v, &lines, &meta).max(v.grid_card_min_height());
        // The layout box places the content (layout units); the box paints
        // and senses on its pixel-snapped edges.
        let layout = Rect::from_min_size(rect.min, vec2(rect.width(), card_h));
        Self {
            ctx: ctx.clone(),
            props,
            ppp,
            lines,
            meta,
            place: Placement::rest(layout, ppp),
        }
    }

    fn v(&self) -> &'a Vars {
        self.props.vars
    }

    fn item(&self) -> &'a AccountCardItem {
        self.props.item
    }

    fn id(&self) -> Id {
        self.props.id
    }

    fn layout(&self) -> Rect {
        self.place.layout
    }

    fn rect(&self) -> Rect {
        self.place.rect
    }

    fn transform(&self) -> Transform {
        self.place.transform
    }

    fn pad(&self) -> f32 {
        self.v().grid_card_padding()
    }

    fn content_left(&self) -> f32 {
        self.layout().left() + self.pad()
    }

    fn content_w(&self) -> f32 {
        self.layout().width() - 2.0 * self.pad()
    }

    /// The top of the name line, under the avatar and its gap.
    fn name_top(&self) -> f32 {
        self.layout().top() + self.pad() + self.v().grid_card_avatar_size() + measure::AVATAR_GAP
    }

    /// Under a scale (dragged, entering) the text is laid out at its
    /// transformed size inside a transform scope.
    fn scaled(&self) -> bool {
        self.transform().scale != 1.0
    }

    /// Where a text box paints: under a scale the scope maps it, otherwise
    /// the hover lift only moves it.
    fn follow(&self, r: Rect) -> Rect {
        if self.scaled() {
            return r;
        }
        let m = map_rect(self.layout(), self.transform(), r);
        Rect::from_center_size(m.center(), r.size())
    }

    /// The ui's painter at the card's opacity.
    fn painter(&self, ui: &Ui) -> Painter {
        let mut painter = ui.painter().clone();
        painter.multiply_opacity(self.place.opacity);
        painter
    }
}

/// Draws the card over `rect` (its layout box, top left and width; the
/// height follows [`height`]) and returns what the user did.
pub fn account_card(ui: &mut Ui, rect: Rect, props: &AccountCardProps) -> CardOutput {
    let item = props.item;
    let mut card = Card::measure(ui.ctx(), props, rect);
    let ctx = card.ctx.clone();
    let response = input::sense(ui, &card);

    let mut st = state(&ctx, props.id);
    let hover = input::Hover::read(&ctx, &response, &st, item);
    // Events first: the armed state they change shows this frame.
    let events = input::arm(ui, &response, &mut st, &card);
    let armed = st.confirm && !item.dragged;

    let name = marquee::fit_name(&card, &mut st, hover);
    let extension = extension::Extension::update(&card, &mut st, hover);
    st.hovered = hover.hovered;

    let look = look::Look::transition(&card, hover, &mut st);
    card.place = look
        .pose
        .place(ui, props.id, props.vars, card.layout(), item.dragged);

    let (surface, chips) = extension.paint(ui, &card, &st, hover.hovered);
    st.chips = chips.iter().map(|hit| hit.id).collect();
    look::paint_box(ui, &card, &look, extension.visible, hover.lift);

    let painter = card.painter(ui);
    let shown = confirm::fade_in(&ctx, props.id, armed);
    avatar::paint(&card, &painter, &look, &mut st, armed, shown);
    labels::paint(&card, &painter, &name);
    if armed {
        confirm::paint_hint(&painter, &card, shown);
    }
    look::paint_outlines(ui, &card, &response, extension.visible);
    card.place.end_group(ui);

    set_state(&ctx, props.id, st);
    CardOutput {
        events,
        surface,
        raised: extension.visible,
        chips,
    }
}
