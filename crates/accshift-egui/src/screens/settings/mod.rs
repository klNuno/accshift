//! The settings panel in the shell's panel slot.
//!
//! ```text
//! content area   padding 10 16 16, a stable 10 px scrollbar gutter
//!   panel        row, gap 14 (10 under 720 px)
//!     rail       168 px (40 under 720 px), a border on the right
//!       title    14 px bold, padding 4 10 10
//!       tabs     core tabs, the "Platforms" group, one tab per platform
//!       footer   Help, Logs
//!     main       column, gap 10
//!       header   title, close button, a border at the bottom
//!       cards    scrolls, 4 px of padding on the right, 8 at the bottom
//!         grid   as many columns of at least 300 px as fit, gap 12
//! ```
//!
//! A tab is a list of cards, a card a list of [`Item`]s: heights come first
//! (rows of the grid take their tallest card), painting second.
//!
//! - [`nav`]: the rail and the header.
//! - [`general_tab`], [`privacy_tab`], [`platforms_tab`], [`platform_tab`]
//!   and [`steam_tab`] build each tab's cards; a platform without settings
//!   of its own (a user descriptor) shows an empty grid.
//! - [`measure`] sizes the items and places the cards in the grid.
//! - [`rows`] paints one item, [`theme_grid`] the theme picker.

mod general_tab;
mod measure;
mod nav;
mod platform_tab;
mod platforms_tab;
mod privacy_tab;
mod rows;
mod steam_tab;
mod theme_grid;

use accshift_app::i18n::{translate as tr, Locale};
use accshift_theme::{AppTheme, ColorScheme};
use egui::{pos2, vec2, Id, Key, Rect, Ui, UiBuilder};

use crate::app::platforms::{PlatformDef, RuntimeOs};
use crate::app::settings_panel::{
    visible_tabs, NumericField, SettingsAction, SettingsPanel, TabDef, PIN_CODE_LENGTH,
};
use crate::app::vars::Vars;
use crate::app::{Action, Booted};
use crate::theme::Rgba;
use crate::ui::components::controls::card::{self, CardKind};
use crate::ui::components::controls::text_input::{self, InputKind};
use crate::ui::components::controls::toggle::{ToggleProps, NEUTRAL_ACCENT};
use crate::ui::components::controls::{scroll, FocusCtx};
use crate::ui::text::{LineHeight, TextStyle};

use measure::wrapped_height;

/// The gap inside a field: between its label, its control and the hint
/// that closes it.
const FIELD_GAP: f32 = 8.0;

/// What the panel reads from the booted app, owned so the panel state can
/// be borrowed mutably next to it.
pub struct View {
    pub vars: Vars,
    pub themes: Vec<AppTheme>,
    /// The applied theme is dark (form controls follow it).
    pub dark: bool,
    pub platforms: Vec<PlatformDef>,
    pub runtime_os: RuntimeOs,
    /// The viewport width in CSS px: it picks the narrow layout and whether
    /// a wide card spans two columns.
    pub viewport_width: f32,
}

impl View {
    pub fn new(booted: &Booted, viewport_width: f32) -> View {
        View {
            vars: booted.theme.vars.clone(),
            themes: booted.registry.all_themes(),
            dark: booted.theme.theme.color_scheme == ColorScheme::Dark,
            platforms: booted.platforms.clone(),
            runtime_os: booted.runtime_os,
            viewport_width,
        }
    }

    /// The narrow layout: a viewport of 720 px or less.
    fn narrow(&self) -> bool {
        self.viewport_width <= 720.0
    }
}

/// One child of a card, in drawing order.
enum Item {
    /// A card title.
    Title(String),
    /// A card title with a "?" help button on its right.
    TitleRow(String, String),
    /// A label (with an optional inline hint), then a control under it.
    Field {
        label: String,
        hint: Option<String>,
        control: Control,
    },
    Toggle {
        label: String,
        description: String,
        enabled: bool,
        disabled: bool,
        action: SettingsAction,
    },
    /// The theme picker.
    ThemeGrid,
    /// A button as wide as its label.
    Button {
        label: String,
        enabled: bool,
        action: SettingsAction,
    },
    /// A text link.
    Link(String, SettingsAction),
    /// A small paragraph, in the danger colour for an error.
    Hint(String, bool),
    /// A hint closing the field before it: the field's 8 px gap after the
    /// control instead of the card's 12.
    FieldHint(String, bool),
    /// Children with a gap of their own (the platform chips, the descriptor
    /// head and list).
    Stack(f32, Vec<Item>),
    /// One of the Platforms tab's own items.
    Platforms(platforms_tab::Item),
    /// One of a per-platform tab's own items.
    Platform(platform_tab::Item),
    /// One of the Steam tab's own items.
    Steam(steam_tab::Item),
}

/// The space above `item` when it follows another one in a card.
fn gap_before(item: &Item) -> f32 {
    match item {
        Item::FieldHint(..) => FIELD_GAP,
        _ => card::GAP,
    }
}

enum Control {
    Select {
        options: Vec<String>,
        selected: usize,
        /// What picking each option does.
        actions: Vec<SettingsAction>,
        /// Which options are disabled; empty when none is.
        disabled: Vec<bool>,
    },
    Slider {
        min: f64,
        max: f64,
        step: f64,
        field: NumericField,
        /// The background opacity commits on every move; the UI scale on
        /// release.
        commit_on_input: bool,
    },
    Input {
        kind: InputKind,
        field: InputField,
        placeholder: String,
        enabled: bool,
    },
}

impl Control {
    /// The salt of the control's widget id under its field's id; the
    /// field's label focuses that id.
    fn salt(&self) -> &'static str {
        match self {
            Control::Select { .. } => "select",
            Control::Slider { .. } => "slider",
            Control::Input { .. } => "input",
        }
    }
}

/// The value a text field edits.
#[derive(Clone, Copy)]
enum InputField {
    Numeric(NumericField),
    Pin,
    CurrentPin,
}

impl InputField {
    fn value(self, panel: &SettingsPanel) -> String {
        match self {
            InputField::Numeric(f) => panel.numeric(f).input.clone(),
            InputField::Pin => panel.pin_code_input.clone(),
            InputField::CurrentPin => panel.current_pin_input.clone(),
        }
    }

    /// A PIN is capped at its length; a number is not.
    fn max_len(self) -> Option<usize> {
        match self {
            InputField::Numeric(_) => None,
            _ => Some(PIN_CODE_LENGTH),
        }
    }

    /// What typing `value` into the field does.
    fn input_action(self, value: String) -> SettingsAction {
        match self {
            InputField::Numeric(f) => SettingsAction::NumericInput(f, value),
            InputField::Pin => SettingsAction::PinInput(value),
            InputField::CurrentPin => SettingsAction::CurrentPinInput(value),
        }
    }
}

struct CardSpec {
    items: Vec<Item>,
    /// Spans two columns on a viewport wider than 980 px.
    wide: bool,
}

fn field_label_style(v: &Vars) -> TextStyle {
    TextStyle::new(12.0, 400)
        .line_height(LineHeight::Px(18.0))
        .color(v.fg_muted.to_color32())
}

fn hint_style(v: &Vars) -> TextStyle {
    TextStyle::new(11.0, 400)
        .line_height(LineHeight::Number(1.4))
        .color(v.fg_subtle.to_color32())
}

/// Everything an item needs to size and paint itself.
struct Ctx<'a> {
    panel: &'a SettingsPanel,
    view: &'a View,
    locale: Locale,
    focus: FocusCtx,
    scope: Id,
}

impl Ctx<'_> {
    fn toggle_props<'b>(
        &'b self,
        label: &'b str,
        description: &'b str,
        enabled: bool,
        disabled: bool,
        on_off: &'b (String, String),
    ) -> ToggleProps<'b> {
        ToggleProps {
            label,
            description,
            enabled,
            accent: NEUTRAL_ACCENT,
            on_label: &on_off.0,
            off_label: &on_off.1,
            disabled,
            vars: &self.view.vars,
            focus: self.focus,
        }
    }

    fn on_off(&self) -> (String, String) {
        (
            tr(self.locale, "common.enabled"),
            tr(self.locale, "common.disabled"),
        )
    }
}

/// What the rail and the header read this frame.
struct Frame<'a> {
    vars: &'a Vars,
    locale: Locale,
    focus: FocusCtx,
    narrow: bool,
    /// A 1 px border, floored to device pixels.
    line_w: f32,
    /// The colour of the rail's and the header's borders.
    divider: Rgba,
    tabs: &'a [TabDef],
    active: &'a str,
}

/// Draws the panel into the shell's panel slot.
pub fn draw(
    ui: &mut Ui,
    slot: Rect,
    panel: &mut SettingsPanel,
    view: &View,
    focus_visible: bool,
    modal: bool,
) -> Vec<Action> {
    let ppp = ui.ctx().pixels_per_point();
    let narrow = view.narrow();
    // Padding 10 16 16 and the 10 px scrollbar gutter.
    let content = Rect::from_min_max(
        pos2(slot.left() + 16.0, slot.top() + 10.0),
        pos2(slot.right() - 16.0 - scroll::WIDTH, slot.bottom() - 16.0),
    );
    let gap = if narrow { 10.0 } else { 14.0 };
    let rail = Rect::from_min_size(content.min, vec2(nav::rail_width(narrow), content.height()));
    let main = Rect::from_min_max(pos2(rail.right() + gap, content.top()), content.max);
    let line_w = crate::ui::snap::border_width(1.0, ppp);
    let tabs = visible_tabs(&view.platforms, &panel.draft.enabled_platforms);
    let active = panel.tab_bar.active_tab().to_string();
    let frame = Frame {
        vars: &view.vars,
        locale: panel.draft.language,
        focus: FocusCtx {
            visible: focus_visible,
        },
        narrow,
        line_w,
        divider: view.vars.border.mix(0.75, Rgba::TRANSPARENT),
        tabs: &tabs,
        active: &active,
    };

    let mut out = nav::rail(ui, rail, &frame);
    // 34 px of content, then the bottom border.
    let header = Rect::from_min_size(main.min, vec2(main.width(), 34.0 + line_w));
    if nav::header(ui, header, main, &frame) {
        out.push(SettingsAction::Close);
    }
    let viewport = Rect::from_min_max(pos2(main.left(), header.bottom() + 10.0), main.max);
    out.extend(tab_content(ui, viewport, panel, view, &frame));

    // An overlay drawn after the panel (theme editor, descriptor preview,
    // a dialog, the palette) owns Escape: it closes that overlay only.
    if !modal && ui.input(|i| i.key_pressed(Key::Escape)) {
        out.push(SettingsAction::Close);
    }
    out.into_iter().map(Action::Settings).collect()
}

/// The active tab's cards in the grid under the header, and its scrollbar.
fn tab_content(
    ui: &mut Ui,
    viewport: Rect,
    panel: &mut SettingsPanel,
    view: &View,
    f: &Frame,
) -> Vec<SettingsAction> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    // The find shortcut: the search field takes focus with its text
    // selected.
    if std::mem::take(&mut panel.focus_platform_search) && f.active == "platforms" {
        ui.memory_mut(|m| m.request_focus(platforms_tab::search_id()));
        text_input::select_all(&ctx, platforms_tab::search_id());
    }
    let platform = f
        .active
        .strip_prefix("platform:")
        .and_then(|id| view.platforms.iter().find(|p| p.id == id));
    let c = Ctx {
        panel,
        view,
        locale: f.locale,
        focus: f.focus,
        scope: Id::unique("settings"),
    };
    let cards = tab_cards(&c, f.active, platform);
    // A platform tab's cards carry its accent; Steam's own cards leave the
    // border colour to the panel.
    let kind = platform.map_or(CardKind::Panel, |p| CardKind::Platform {
        accent: p.accent,
        tint_border: p.id != "steam",
    });
    let grid = measure::fit_grid(&ctx, &c, &cards, viewport);
    let mut content_ui = ui.new_child(UiBuilder::new().max_rect(viewport));
    content_ui.set_clip_rect(viewport.intersect(ui.clip_rect()));
    let mut out = Vec::new();
    for (k, ((spec, rect), measured)) in cards.iter().zip(&grid.rects).zip(&grid.items).enumerate()
    {
        card::paint_frame(&content_ui, *rect, &view.vars, kind);
        let inner = card::content(*rect, ppp);
        // The layout's heights hold when the content width is the one they
        // were measured at; a float rounding step apart, measure again.
        let measured = (inner.width() == measured.inner).then_some(&measured.heights);
        let mut y = inner.top();
        for (i, item) in spec.items.iter().enumerate() {
            if i > 0 {
                y += gap_before(item);
            }
            let h = measured.map_or_else(
                || measure::item_height(&ctx, &c, item, inner.width()),
                |m| m[i],
            );
            let r = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), h));
            // By card and item: two cards share a left edge when one spans
            // the row (telemetry under privacy), and an id clash leaves the
            // first widget without a hit box.
            let id = c.scope.with((f.active, k, i));
            out.extend(rows::paint_item(&mut content_ui, r, id, &c, item));
            y += h;
        }
    }
    let mut offset = c.panel.scroll;
    scroll::scroll(
        ui,
        viewport,
        Id::unique("settings-scroll"),
        &mut offset,
        grid.height + measure::PADDING_BOTTOM,
        &view.vars,
    );
    panel.scroll = offset;
    out
}

/// The cards of tab `tab`; `platform` is the platform a per-platform tab
/// stands for.
fn tab_cards(c: &Ctx, tab: &str, platform: Option<&PlatformDef>) -> Vec<CardSpec> {
    match (tab, platform) {
        ("general", _) => general_tab::cards(c.panel, c.view, c.locale),
        ("platforms", _) => platforms_tab::cards(c.panel, c.view, c.locale),
        ("privacy", _) => privacy_tab::cards(c.panel, c.locale),
        (_, Some(p)) => platform_tab::cards(c.panel, c.view, c.locale, p),
        _ => Vec::new(),
    }
}
