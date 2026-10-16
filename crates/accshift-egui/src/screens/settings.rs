//! `Settings.svelte`: the settings panel in the shell's panel slot, with its
//! General and Privacy tabs (`SettingsGeneralTab.svelte`,
//! `SettingsPrivacyTab.svelte`).
//!
//! ```text
//! .content                padding 10 16 16, a stable 10 px scrollbar gutter
//!   .settings-panel       row, gap 14 (10 under 720 px)
//!     aside.sidebar       168 px (40 under 720 px), border-right
//!       .sidebar-title    14 px bold, padding 4 10 10
//!       .nav-list         core tabs, "Platforms" group, one tab per platform
//!       .sidebar-footer   Help, Logs
//!     .settings-main      column, gap 10
//!       .main-header      title, close button, border-bottom
//!       .settings-content overflow-y auto, padding-right 4, padding-bottom 8
//!         .settings-grid  repeat(auto-fit, minmax(300px, 1fr)), gap 12
//! ```
//!
//! A tab is a list of cards, a card a list of [`Item`]s: heights come first
//! (rows of the grid take their tallest card), painting second. The
//! Platforms tab builds its cards in [`platforms_tab`], the per-platform
//! tabs in [`platform_tab`] and [`steam_tab`]; a platform without a settings
//! component (a user descriptor) shows an empty grid.

mod platform_tab;
mod platforms_tab;
mod steam_tab;

use accshift_app::i18n::{translate, translate_with, Locale, TranslationValue, LANGUAGE_OPTIONS};
use accshift_app::settings::{AnimationsMode, StreamerMode};
use accshift_theme::{is_built_in_theme, AppTheme, ColorScheme, TokenKey};
use egui::accesskit::Role;
use egui::{pos2, vec2, Context, Id, Key, Rect, Sense, Ui, WidgetInfo, WidgetType};

use crate::app::platforms::{PlatformDef, RuntimeOs};
use crate::app::settings_panel::{
    visible_tabs, NumericField, SettingFlag, SettingsAction, SettingsPanel, TabDef, PIN_CODE_LENGTH,
};
use crate::app::vars::Vars;
use crate::app::{Action, Booted};
use crate::theme::Rgba;
use crate::ui::anim;
use crate::ui::components::controls::button::{self, ButtonProps};
use crate::ui::components::controls::card::{self, CardKind};
use crate::ui::components::controls::select::{self, SelectProps};
use crate::ui::components::controls::slider::{self, SliderProps};
use crate::ui::components::controls::text_input::{self, InputKind, TextInputProps};
use crate::ui::components::controls::toggle::{self, ToggleProps, NEUTRAL_ACCENT};
use crate::ui::components::controls::{scroll, take_focus, FocusCtx, T120, WHITE};
use crate::ui::components::icon;
use crate::ui::css;
use crate::ui::paint::{Border, BoxShadow, BoxStyle, Corners, Gradient, Sides};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextAlign, TextStyle, TextTransform};

/// `.field { gap: 8px }`.
const FIELD_GAP: f32 = 8.0;
/// `.settings-grid { gap: 12px }`.
const GRID_GAP: f32 = 12.0;
/// `minmax(300px, 1fr)`.
const GRID_MIN: f32 = 300.0;
/// `.nav-item`: an 18 px line and 8 px padding above and below.
const NAV_ITEM_H: f32 = 34.0;

macro_rules! svg14 {
    ($body:expr) => {
        concat!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"14\" height=\"14\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"white\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\">",
            $body,
            "</svg>"
        )
        .as_bytes()
    };
}

const ICON_GENERAL: &[u8] = svg14!(
    "<path d=\"M4 21v-7 M4 10V3 M12 21v-9 M12 8V3 M20 21v-5 M20 12V3 M1 14h6 M9 8h6 M17 16h6\"/>"
);
const ICON_PLATFORMS: &[u8] =
    svg14!("<path d=\"M3 3h7v7H3z M14 3h7v7h-7z M14 14h7v7h-7z M3 14h7v7H3z\"/>");
const ICON_PRIVACY: &[u8] = svg14!("<path d=\"M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z\"/>");
const ICON_HELP: &[u8] = svg14!(
    "<circle cx=\"12\" cy=\"12\" r=\"10\"/><path d=\"M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3\"/><line x1=\"12\" y1=\"17\" x2=\"12.01\" y2=\"17\"/>"
);
const ICON_LOGS: &[u8] = svg14!(
    "<path d=\"M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z\"/>"
);
const ICON_CLOSE: &[u8] = svg14!(
    "<line x1=\"18\" y1=\"6\" x2=\"6\" y2=\"18\"/><line x1=\"6\" y1=\"6\" x2=\"18\" y2=\"18\"/>"
);

/// What the panel reads from the booted app, owned so the panel state can
/// be borrowed mutably next to it.
pub struct View {
    pub vars: Vars,
    pub themes: Vec<AppTheme>,
    /// `color-scheme` of the applied theme (form controls follow it).
    pub dark: bool,
    pub platforms: Vec<PlatformDef>,
    pub runtime_os: RuntimeOs,
    /// The CSS viewport width (the media queries).
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

    /// `@media (max-width: 720px)`.
    fn narrow(&self) -> bool {
        self.viewport_width <= 720.0
    }
}

fn tr(locale: Locale, key: &str) -> String {
    translate(locale, key)
}

/// One child of a card, in DOM order.
enum Item {
    /// `<h3>`.
    Title(String),
    /// `.card-title-row`: the h3 and the wiki button.
    TitleRow(String, String),
    /// `.field`: label (and an inline hint), then a control.
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
    ThemeGrid,
    /// `.btn-export`.
    Button {
        label: String,
        enabled: bool,
        action: SettingsAction,
    },
    /// `.inline-link-btn`.
    Link(String, SettingsAction),
    /// `p.hint`, in `--danger` for an error.
    Hint(String, bool),
    /// A `p.hint` closing the `.field` before it: the field's 8 px gap
    /// after the control instead of the card's 12.
    FieldHint(String, bool),
    /// Children with a gap of their own (`.platforms`, `.descriptor-head`,
    /// `.descriptor-list`).
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
        /// `<option disabled>`; empty when none is.
        disabled: Vec<bool>,
    },
    Slider {
        min: f64,
        max: f64,
        step: f64,
        field: NumericField,
        /// `bgOpacity` commits on every `input`; `uiScale` on `change`.
        commit_on_input: bool,
    },
    Input {
        kind: InputKind,
        field: InputField,
        placeholder: String,
        enabled: bool,
    },
}

#[derive(Clone, Copy)]
enum InputField {
    Numeric(NumericField),
    Pin,
    CurrentPin,
}

struct CardSpec {
    items: Vec<Item>,
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

fn general_cards(panel: &SettingsPanel, view: &View, l: Locale) -> Vec<CardSpec> {
    let d = &panel.draft;
    let t = |k: &str| tr(l, k);
    let languages: Vec<String> = LANGUAGE_OPTIONS.iter().map(|o| t(o.label_key)).collect();
    let language = LANGUAGE_OPTIONS
        .iter()
        .position(|o| o.code == d.language)
        .unwrap_or(0);
    let animations = match d.animations {
        AnimationsMode::System => 0,
        AnimationsMode::On => 1,
        AnimationsMode::Off => 2,
    };
    let glass = view
        .themes
        .iter()
        .find(|th| th.id == d.theme_id)
        .is_some_and(|th| th.glass);
    let toggle = |key: &str, desc: &str, on: bool, flag: SettingFlag| Item::Toggle {
        label: t(key),
        description: if desc.is_empty() {
            String::new()
        } else {
            t(desc)
        },
        enabled: on,
        disabled: false,
        action: SettingsAction::Toggle(flag),
    };
    let mut theme = vec![Item::Title(t("settings.theme")), Item::ThemeGrid];
    if !glass {
        theme.push(Item::Field {
            label: format!(
                "{} - {}%",
                t("settings.backgroundOpacity"),
                d.background_opacity
            ),
            hint: None,
            control: Control::Slider {
                min: 0.0,
                max: 100.0,
                step: 5.0,
                field: NumericField::BgOpacity,
                commit_on_input: true,
            },
        });
    }
    vec![
        CardSpec {
            wide: false,
            items: vec![
                Item::Title(t("settings.appearance")),
                Item::Field {
                    label: t("settings.language"),
                    hint: None,
                    control: Control::Select {
                        options: languages,
                        selected: language,
                        actions: LANGUAGE_OPTIONS
                            .iter()
                            .map(|o| SettingsAction::Language(o.code))
                            .collect(),
                        disabled: Vec::new(),
                    },
                },
                Item::Field {
                    label: format!("{} - {}%", t("settings.uiZoom"), panel.ui_scale.input),
                    hint: None,
                    control: Control::Slider {
                        min: 75.0,
                        max: 150.0,
                        step: 5.0,
                        field: NumericField::UiScale,
                        commit_on_input: false,
                    },
                },
                Item::Field {
                    label: t("settings.animations"),
                    hint: None,
                    control: Control::Select {
                        options: vec![
                            t("settings.animationsSystem"),
                            t("settings.animationsOn"),
                            t("settings.animationsOff"),
                        ],
                        selected: animations,
                        actions: [
                            AnimationsMode::System,
                            AnimationsMode::On,
                            AnimationsMode::Off,
                        ]
                        .into_iter()
                        .map(SettingsAction::Animations)
                        .collect(),
                        disabled: Vec::new(),
                    },
                },
            ],
        },
        CardSpec {
            wide: false,
            items: theme,
        },
        CardSpec {
            wide: false,
            items: vec![
                Item::Title(t("settings.accountDisplay")),
                Item::Toggle {
                    label: t("settings.showNotesUnderCards"),
                    description: String::new(),
                    enabled: d.account_display.show_card_notes_inline,
                    disabled: false,
                    action: SettingsAction::Toggle(SettingFlag::ShowCardNotesInline),
                },
                toggle(
                    "settings.expandedFolders",
                    "",
                    d.account_display.expanded_folders,
                    SettingFlag::ExpandedFolders,
                ),
            ],
        },
        CardSpec {
            wide: false,
            items: vec![
                Item::Title(t("settings.performance")),
                toggle(
                    "settings.suspendGraphicsWhenMinimized",
                    "",
                    d.suspend_graphics_when_minimized,
                    SettingFlag::SuspendGraphicsWhenMinimized,
                ),
                toggle(
                    "settings.minimizeOnAccountSwitch",
                    "",
                    d.minimize_on_account_switch,
                    SettingFlag::MinimizeOnAccountSwitch,
                ),
            ],
        },
        CardSpec {
            wide: false,
            items: vec![
                Item::TitleRow(t("settings.integrations"), t("settings.help")),
                toggle(
                    "settings.deepLinks",
                    "settings.deepLinksHint",
                    d.deep_links_enabled,
                    SettingFlag::DeepLinks,
                ),
                toggle(
                    "settings.cliEnabled",
                    "settings.cliEnabledHint",
                    d.cli_enabled,
                    SettingFlag::Cli,
                ),
            ],
        },
    ]
}

fn privacy_cards(panel: &SettingsPanel, l: Locale) -> Vec<CardSpec> {
    let d = &panel.draft;
    let t = |k: &str| tr(l, k);
    let mut security = vec![
        Item::Title(t("settings.security")),
        Item::Toggle {
            label: t("settings.pinLockOnAfk"),
            description: String::new(),
            enabled: panel.pin_toggle_on(),
            disabled: false,
            action: SettingsAction::TogglePin,
        },
        Item::Hint(t("settings.pinScreenLockOnly"), false),
    ];
    if panel.pin_guarded() {
        security.push(Item::Field {
            label: t("settings.pinCurrentCode"),
            hint: None,
            control: Control::Input {
                kind: InputKind::Password,
                field: InputField::CurrentPin,
                placeholder: t("settings.pinPlaceholder"),
                enabled: !panel.current_pin_busy(),
            },
        });
        if panel.current_pin_error.is_empty() {
            security.push(Item::FieldHint(t("settings.pinCurrentHint"), false));
        } else {
            security.push(Item::FieldHint(panel.current_pin_error.clone(), true));
        }
    } else if d.pin_enabled || panel.pin_setup_pending {
        security.push(Item::Field {
            label: t("settings.pinCode"),
            hint: None,
            control: Control::Input {
                kind: InputKind::Password,
                field: InputField::Pin,
                placeholder: t("settings.pinPlaceholder"),
                enabled: true,
            },
        });
        security.push(Item::FieldHint(
            t("settings.pinRequiredAfterInactivity"),
            false,
        ));
    }
    let mut cards = vec![
        CardSpec {
            wide: false,
            items: vec![
                Item::Title(t("settings.privacy")),
                Item::Field {
                    label: t("settings.inactivityTimeout"),
                    hint: Some(format!(
                        "({}, {})",
                        t("settings.unitSeconds"),
                        t("settings.zeroDisabled")
                    )),
                    control: Control::Input {
                        kind: InputKind::Number,
                        field: InputField::Numeric(NumericField::InactivityBlur),
                        placeholder: String::new(),
                        enabled: true,
                    },
                },
                Item::Toggle {
                    label: t("settings.streamerMode"),
                    description: t("settings.streamerModeHint"),
                    enabled: d.streamer_mode == StreamerMode::Auto,
                    disabled: false,
                    action: SettingsAction::Toggle(SettingFlag::StreamerMode),
                },
                Item::Button {
                    label: t("settings.replayOnboarding"),
                    enabled: true,
                    action: SettingsAction::ReplayOnboarding,
                },
                Item::Hint(t("settings.replayOnboardingHint"), false),
            ],
        },
        CardSpec {
            wide: false,
            items: security,
        },
    ];
    if let Some(tel) = &panel.telemetry {
        let mut items = vec![
            Item::Title(t("settings.telemetry")),
            Item::Link(
                t("settings.telemetryLearnMore"),
                SettingsAction::OpenTelemetryDoc,
            ),
            Item::Toggle {
                label: t("settings.telemetryModeA"),
                description: t("settings.telemetryModeAHint"),
                enabled: tel.mode_a_enabled,
                disabled: false,
                action: SettingsAction::ToggleTelemetryA,
            },
            Item::Toggle {
                label: t("settings.telemetryModeB"),
                description: t("settings.telemetryModeBHint"),
                enabled: tel.mode_b_enabled,
                disabled: panel.mode_b_busy,
                action: SettingsAction::ToggleTelemetryB,
            },
        ];
        if tel.forget_pending {
            items.push(Item::Hint(t("settings.telemetryDeletePendingHint"), false));
            items.push(Item::Button {
                label: t("settings.telemetryRetryDelete"),
                enabled: !panel.mode_b_busy,
                action: SettingsAction::RetryTelemetryDeletion,
            });
        }
        if tel.install_id_set {
            items.push(Item::Button {
                label: t("settings.telemetryExport"),
                enabled: !panel.export_busy,
                action: SettingsAction::ExportTelemetry,
            });
        }
        cards.push(CardSpec { wide: true, items });
    } else if panel.telemetry_error {
        cards.push(CardSpec {
            wide: true,
            items: vec![
                Item::Title(t("settings.telemetry")),
                Item::Hint(t("settings.telemetryLoadFailed"), false),
                Item::Button {
                    label: t("common.retry"),
                    enabled: true,
                    action: SettingsAction::RetryTelemetryLoad,
                },
            ],
        });
    }
    cards
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

/// `.theme-grid`: `repeat(auto-fill, minmax(86px, 1fr))`, gap 8.
fn theme_columns(width: f32) -> (usize, f32) {
    let cols = (((width + 8.0) / 94.0).floor() as usize).max(1);
    (cols, (width - 8.0 * (cols - 1) as f32) / cols as f32)
}

/// `.theme-swatch`: a 2 px border, 8 px padding on top and 6 px at the
/// bottom, the preview (23 px inside its 1 px border), a 6 px gap and the
/// 12 px label. Borders are laid out floored to device pixels.
fn swatch_h(ppp: f32) -> f32 {
    55.0 + 2.0 * snap::border_width(2.0, ppp) + 2.0 * snap::border_width(1.0, ppp)
}

/// The theme action buttons: a 26.5 px box inside a 1 px border.
fn theme_action_h(ppp: f32) -> f32 {
    26.5 + 2.0 * snap::border_width(1.0, ppp)
}

fn wrapped_height(ctx: &Context, text: &str, style: &TextStyle, width: f32) -> f32 {
    text::layout(
        ctx,
        text,
        style,
        Rect::from_min_size(pos2(0.0, 0.0), vec2(width.max(1.0), 0.0)),
    )
    .map_or(0.0, |l| l.height)
}

fn item_height(ctx: &Context, c: &Ctx, item: &Item, width: f32) -> f32 {
    let v = &c.view.vars;
    match item {
        Item::Title(_) => card::title_height(ctx.pixels_per_point()),
        Item::TitleRow(..) => 28.0 + snap::border_width(1.0, ctx.pixels_per_point()),
        Item::Field { control, .. } => {
            18.0 + 8.0
                + match control {
                    Control::Select { .. } | Control::Input { .. } => {
                        text_input::height(ctx.pixels_per_point())
                    }
                    Control::Slider { .. } => slider::HEIGHT,
                }
        }
        Item::Toggle {
            label,
            description,
            enabled,
            disabled,
            ..
        } => {
            let on_off = c.on_off();
            toggle::height(
                ctx,
                width,
                &c.toggle_props(label, description, *enabled, *disabled, &on_off),
            )
        }
        Item::ThemeGrid => {
            let (cols, _) = theme_columns(width);
            let rows = c.view.themes.len().div_ceil(cols);
            // `margin-top: 4px` on the grid and on the actions row, the
            // field's 8 px gap between them.
            let ppp = ctx.pixels_per_point();
            4.0 + rows as f32 * swatch_h(ppp)
                + rows.saturating_sub(1) as f32 * 8.0
                + 8.0
                + 4.0
                + theme_action_h(ppp)
        }
        Item::Button { label, .. } => button::size(ctx, label, &button::export_style(v)).y,
        Item::Link(..) => 18.0,
        Item::Hint(text, _) | Item::FieldHint(text, _) => {
            wrapped_height(ctx, text, &hint_style(v), width)
        }
        Item::Stack(gap, items) => {
            items
                .iter()
                .map(|i| item_height(ctx, c, i, width))
                .sum::<f32>()
                + gap * items.len().saturating_sub(1) as f32
        }
        Item::Platforms(item) => platforms_tab::height(ctx, c, item, width),
        Item::Platform(item) => platform_tab::height(ctx, c, item, width),
        Item::Steam(item) => steam_tab::height(ctx, c, item, width),
    }
}

fn card_height(ctx: &Context, c: &Ctx, spec: &CardSpec, width: f32) -> f32 {
    let inset = card::inset(ctx.pixels_per_point());
    let inner = width - 2.0 * inset;
    let items: f32 = spec
        .items
        .iter()
        .map(|i| item_height(ctx, c, i, inner))
        .sum();
    let gaps: f32 = spec.items.iter().skip(1).map(gap_before).sum();
    items + gaps + 2.0 * inset
}

/// Grid placement: `(column, row)` of each card, columns spanned, row count.
fn place(cards: &[CardSpec], cols: usize, wide_spans: bool) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    let (mut col, mut row) = (0, 0);
    for spec in cards {
        let span = if spec.wide && wide_spans {
            cols.min(2)
        } else {
            1
        };
        if col + span > cols {
            col = 0;
            row += 1;
        }
        out.push((col, row, span));
        col += span;
        if col >= cols {
            col = 0;
            row += 1;
        }
    }
    out
}

struct GridLayout {
    rects: Vec<Rect>,
    height: f32,
}

fn grid_layout(
    ctx: &Context,
    c: &Ctx,
    cards: &[CardSpec],
    origin: egui::Pos2,
    width: f32,
) -> GridLayout {
    let cols = if c.view.narrow() {
        1
    } else {
        (((width + GRID_GAP) / (GRID_MIN + GRID_GAP)).floor() as usize).max(1)
    };
    let tracks = snap::fr_tracks(width, cols, GRID_GAP, ctx.pixels_per_point());
    let placed = place(cards, cols, c.view.viewport_width > 980.0);
    let rows = placed.iter().map(|p| p.1 + 1).max().unwrap_or(0);
    let heights: Vec<f32> = cards
        .iter()
        .zip(&placed)
        .map(|(spec, (col, _, span))| {
            let (_, w) = snap::track_span(&tracks, *col, *span);
            card_height(ctx, c, spec, w)
        })
        .collect();
    let mut row_h = vec![0.0f32; rows];
    for ((_, row, _), h) in placed.iter().zip(&heights) {
        row_h[*row] = row_h[*row].max(*h);
    }
    let mut row_y = Vec::with_capacity(rows);
    let mut y = origin.y;
    for h in &row_h {
        row_y.push(y);
        y += h + GRID_GAP;
    }
    let rects = placed
        .iter()
        .zip(&heights)
        .map(|((col, row, span), h)| {
            let (x, w) = snap::track_span(&tracks, *col, *span);
            Rect::from_min_size(pos2(origin.x + x, row_y[*row]), vec2(w, *h))
        })
        .collect();
    let height = if rows == 0 {
        0.0
    } else {
        row_h.iter().sum::<f32>() + GRID_GAP * (rows - 1) as f32
    };
    GridLayout { rects, height }
}

/// Draws the panel into the shell's panel slot.
pub fn draw(
    ui: &mut Ui,
    slot: Rect,
    panel: &mut SettingsPanel,
    view: &View,
    focus_visible: bool,
) -> Vec<Action> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = &view.vars;
    let locale = panel.draft.language;
    let focus = FocusCtx {
        visible: focus_visible,
    };
    let mut out: Vec<SettingsAction> = Vec::new();
    let narrow = view.narrow();
    // `.content`: padding 10 16 16 and the 10 px scrollbar gutter.
    let content = Rect::from_min_max(
        pos2(slot.left() + 16.0, slot.top() + 10.0),
        pos2(slot.right() - 16.0 - scroll::WIDTH, slot.bottom() - 16.0),
    );
    let sidebar_w = if narrow { 40.0 } else { 168.0 };
    let gap = if narrow { 10.0 } else { 14.0 };
    let sidebar = Rect::from_min_size(content.min, vec2(sidebar_w, content.height()));
    let main = Rect::from_min_max(pos2(sidebar.right() + gap, content.top()), content.max);
    let divider = v.border.mix(0.75, Rgba::TRANSPARENT);

    // ------------------------------------------------------------ sidebar
    let sidebar_pad = if narrow { 8.0 } else { 12.0 };
    let line_w = snap::border_width(1.0, ppp);
    let inner_w = sidebar_w - sidebar_pad - line_w;
    BoxStyle {
        border: Border {
            width: Sides::new(0.0, line_w, 0.0, 0.0),
            color: Sides::all(divider),
        },
        ..BoxStyle::default()
    }
    .paint(ui.painter(), snap::rect(sidebar, ppp));
    let mut y = sidebar.top();
    if !narrow {
        let title = TextStyle::new(14.0, 700)
            .line_height(LineHeight::Number(1.5))
            .nowrap(false)
            .color(v.fg.to_color32());
        text::paint(
            ui.painter(),
            Rect::from_min_size(
                pos2(sidebar.left() + 10.0, y + 4.0),
                vec2(inner_w - 20.0, 21.0),
            ),
            &tr(locale, "settings.title"),
            &title,
        );
        y += 35.0 + 6.0;
    }
    let tabs = visible_tabs(&view.platforms, &panel.draft.enabled_platforms);
    let active = panel.tab_bar.active_tab().to_string();
    let footer_h = if narrow {
        NAV_ITEM_H * 2.0 + 4.0
    } else {
        NAV_ITEM_H
    };
    let footer_top = sidebar.bottom() - footer_h;
    let list = Rect::from_min_max(
        pos2(sidebar.left(), y),
        pos2(sidebar.left() + inner_w, footer_top - 6.0),
    );
    let list_painter_clip = list.intersect(ui.clip_rect());
    let mut list_ui = ui.new_child(egui::UiBuilder::new().max_rect(list));
    list_ui.set_clip_rect(list_painter_clip);
    let mut first_platform = true;
    for tab in &tabs {
        if tab.accent.is_some() && first_platform {
            first_platform = false;
            if !narrow {
                // `.nav-group-label`: margin-top 12, padding 0 10 4, 9 px
                // bold uppercase with 0.08em tracking.
                y += 12.0;
                let style = TextStyle::new(9.0, 700)
                    .line_height(LineHeight::Number(1.5))
                    .letter_spacing_em(0.08)
                    .transform(TextTransform::Uppercase)
                    .nowrap(false)
                    .color(v.fg_subtle.to_color32());
                text::paint(
                    list_ui.painter(),
                    Rect::from_min_size(pos2(list.left() + 10.0, y), vec2(inner_w - 20.0, 13.5)),
                    &tr(locale, "settings.platforms"),
                    &style,
                );
                y += 17.5 + 2.0;
            }
        }
        let rect = Rect::from_min_size(pos2(list.left(), y), vec2(inner_w, NAV_ITEM_H));
        if nav_item(
            &mut list_ui,
            rect,
            tab,
            tab.id == active,
            locale,
            v,
            focus,
            narrow,
        ) {
            out.push(SettingsAction::SelectTab(tab.id.clone()));
        }
        y += NAV_ITEM_H + 2.0;
    }
    // `.sidebar-footer`: Help and Logs side by side (stacked under 720 px).
    let footer: [(&[u8], &str, &str, SettingsAction); 2] = [
        (
            ICON_HELP,
            "settings.helpShort",
            "settings.help",
            SettingsAction::OpenHelp,
        ),
        (
            ICON_LOGS,
            "settings.logs",
            "settings.openLogs",
            SettingsAction::OpenLogs,
        ),
    ];
    for (i, (svg, label, aria, action)) in footer.into_iter().enumerate() {
        let rect = if narrow {
            Rect::from_min_size(
                pos2(sidebar.left(), footer_top + (NAV_ITEM_H + 4.0) * i as f32),
                vec2(inner_w, NAV_ITEM_H),
            )
        } else {
            let w = (inner_w - 4.0) / 2.0;
            Rect::from_min_size(
                pos2(sidebar.left() + (w + 4.0) * i as f32, footer_top),
                vec2(w, NAV_ITEM_H),
            )
        };
        let tab = TabDef {
            id: format!("footer:{i}"),
            label_key: label.to_string(),
            accent: None,
        };
        if footer_button(
            ui,
            rect,
            &tab,
            svg,
            &tr(locale, aria),
            locale,
            v,
            focus,
            narrow,
        ) {
            out.push(action);
        }
    }

    // ------------------------------------------------------------ header
    // `.main-header`: 34 px of content, then its bottom border.
    let header = Rect::from_min_size(main.min, vec2(main.width(), 34.0 + line_w));
    BoxStyle {
        border: Border {
            width: Sides::new(0.0, 0.0, line_w, 0.0),
            color: Sides::all(divider),
        },
        ..BoxStyle::default()
    }
    .paint(ui.painter(), snap::rect(header, ppp));
    let title_key = tabs
        .iter()
        .find(|t| t.id == active)
        .map_or("settings.title".to_string(), |t| t.label_key.clone());
    let title = TextStyle::new(14.0, 700)
        .line_height(LineHeight::Number(1.5))
        .nowrap(true)
        .color(v.fg.to_color32());
    text::paint(
        ui.painter(),
        Rect::from_min_size(
            pos2(main.left(), main.top() + snap::centre(26.0, 21.0, ppp)),
            vec2(main.width() - 38.0, 21.0),
        ),
        &tr(locale, &title_key),
        &title,
    );
    let close = Rect::from_min_size(pos2(main.right() - 26.0, main.top()), vec2(26.0, 26.0));
    if close_button(ui, close, &tr(locale, "common.close"), v, focus) {
        out.push(SettingsAction::Close);
    }

    // ------------------------------------------------------------ content
    let viewport = Rect::from_min_max(pos2(main.left(), header.bottom() + 10.0), main.max);
    // mod+f: the search field takes focus with its text selected.
    if std::mem::take(&mut panel.focus_platform_search) && active == "platforms" {
        ui.memory_mut(|m| m.request_focus(platforms_tab::search_id()));
        text_input::select_all(&ctx, platforms_tab::search_id());
    }
    let platform = active
        .strip_prefix("platform:")
        .and_then(|id| view.platforms.iter().find(|p| p.id == id));
    let c = Ctx {
        panel,
        view,
        locale,
        focus,
        scope: Id::new("settings"),
    };
    let cards = match (active.as_str(), platform) {
        ("general", _) => general_cards(c.panel, view, locale),
        ("platforms", _) => platforms_tab::cards(c.panel, view, locale),
        ("privacy", _) => privacy_cards(c.panel, locale),
        (_, Some(p)) => platform_tab::cards(c.panel, view, locale, p),
        _ => Vec::new(),
    };
    // A platform tab's cards carry its accent; Steam's own cards
    // (`.platform-display-card`) lose the border colour to the panel.
    let kind = platform.map_or(CardKind::Panel, |p| CardKind::Platform {
        accent: p.accent,
        tint_border: p.id != "steam",
    });
    // Chromium lays out without the bar, then with it when that overflows.
    let origin = pos2(viewport.left(), viewport.top() - c.panel.scroll);
    let mut width = viewport.width() - 4.0;
    let mut grid = grid_layout(&ctx, &c, &cards, origin, width);
    if scroll::overflows(viewport.height(), grid.height + 8.0) {
        width -= scroll::WIDTH;
        grid = grid_layout(&ctx, &c, &cards, origin, width);
    }
    let mut content_ui = ui.new_child(egui::UiBuilder::new().max_rect(viewport));
    content_ui.set_clip_rect(viewport.intersect(ui.clip_rect()));
    let content_h = grid.height;
    for (k, (spec, rect)) in cards.iter().zip(&grid.rects).enumerate() {
        card::paint_frame(&content_ui, *rect, v, kind);
        let inner = card::content(*rect, ppp);
        let mut y = inner.top();
        for (i, item) in spec.items.iter().enumerate() {
            if i > 0 {
                y += gap_before(item);
            }
            let h = item_height(&ctx, &c, item, inner.width());
            let r = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), h));
            // By card and item: two cards share a left edge when one spans
            // the row (telemetry under privacy), and an id clash leaves the
            // first widget without a hit box.
            let id = c.scope.with((active.as_str(), k, i));
            out.extend(paint_item(&mut content_ui, r, id, &c, item));
            y += h;
        }
    }
    let mut offset = c.panel.scroll;
    scroll::scroll(
        ui,
        viewport,
        Id::new("settings-scroll"),
        &mut offset,
        content_h + 8.0,
        v,
    );
    panel.scroll = offset;

    let escape = ui.input(|i| i.key_pressed(Key::Escape));
    if escape {
        out.push(SettingsAction::Close);
    }
    let _ = anim::motion(&ctx);
    out.into_iter().map(Action::Settings).collect()
}

#[allow(clippy::too_many_arguments)]
fn nav_item(
    ui: &mut Ui,
    rect: Rect,
    tab: &TabDef,
    active: bool,
    locale: Locale,
    v: &Vars,
    focus: FocusCtx,
    narrow: bool,
) -> bool {
    let label = tr(locale, &tab.label_key);
    let id = Id::new(("settings-tab", tab.id.as_str()));
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, active, &label));
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(Role::Tab);
        node.set_selected(active);
    });
    let focused = take_focus(ui, &response);
    let hovered = response.hovered();
    let (bg, fg) = if active {
        (v.bg_card.mix(0.74, WHITE), v.fg)
    } else if hovered {
        (v.bg_card.mix(0.82, WHITE), v.fg)
    } else {
        (Rgba::TRANSPARENT, v.fg_muted)
    };
    let ctx = ui.ctx().clone();
    let bg = anim::transition(&ctx, id.with("bg"), bg, &T120);
    let fg = anim::transition(&ctx, id.with("fg"), fg, &T120);
    paint_nav(ui, rect, bg, fg, &label, narrow);
    let ppp = ctx.pixels_per_point();
    let icon_x = if narrow {
        rect.left() + (rect.width() - 14.0) / 2.0
    } else {
        rect.left() + 10.0
    };
    let icon_rect = Rect::from_min_size(pos2(icon_x, rect.top() + 10.0), vec2(14.0, 14.0));
    match tab.accent {
        None => {
            let svg = match tab.id.as_str() {
                "platforms" => ICON_PLATFORMS,
                "privacy" => ICON_PRIVACY,
                _ => ICON_GENERAL,
            };
            let a = if active { 1.0 } else { 0.75 };
            icon::paint(
                ui.painter(),
                svg,
                Rect::from_min_size(icon::snapped_origin(icon_rect.min, ppp), icon_rect.size()),
                fg.alpha(fg.a * a),
                1.0,
            );
        }
        Some(accent) => {
            let a = if active || hovered { 1.0 } else { 0.7 };
            let dot = Rect::from_min_size(icon_rect.min + vec2(3.0, 3.0), vec2(8.0, 8.0));
            BoxStyle {
                background: accent,
                radius: Corners::all(4.0),
                opacity: a,
                ..BoxStyle::default()
            }
            .paint(ui.painter(), snap::rect(dot, ppp));
        }
    }
    crate::ui::components::controls::focus_ring(
        ui,
        snap::rect(rect, ppp),
        8.0,
        0.0,
        v,
        focus,
        focused,
    );
    response.clicked()
        || (focused && ui.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Space)))
}

fn paint_nav(ui: &Ui, rect: Rect, bg: Rgba, fg: Rgba, label: &str, narrow: bool) {
    let ppp = ui.ctx().pixels_per_point();
    if bg.a > 0.0 {
        BoxStyle {
            background: bg,
            radius: Corners::all(8.0),
            ..BoxStyle::default()
        }
        .paint(ui.painter(), snap::rect(rect, ppp));
    }
    if !narrow {
        let style = TextStyle::new(12.0, 600)
            .line_height(LineHeight::Px(18.0))
            .nowrap(true)
            .color(fg.to_color32());
        text::paint(
            ui.painter(),
            Rect::from_min_size(
                pos2(rect.left() + 33.0, rect.top() + 8.0),
                vec2(rect.width() - 43.0, 18.0),
            ),
            label,
            &style,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn footer_button(
    ui: &mut Ui,
    rect: Rect,
    tab: &TabDef,
    svg: &'static [u8],
    aria: &str,
    locale: Locale,
    v: &Vars,
    focus: FocusCtx,
    narrow: bool,
) -> bool {
    let id = Id::new(("settings-footer", tab.id.as_str()));
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, aria));
    let focused = take_focus(ui, &response);
    let hovered = response.hovered();
    let (bg, fg) = if hovered {
        (v.bg_card.mix(0.82, WHITE), v.fg)
    } else {
        (Rgba::TRANSPARENT, v.fg_subtle)
    };
    let ctx = ui.ctx().clone();
    let bg = anim::transition(&ctx, id.with("bg"), bg, &T120);
    let fg = anim::transition(&ctx, id.with("fg"), fg, &T120);
    paint_nav(ui, rect, bg, fg, &tr(locale, &tab.label_key), narrow);
    let ppp = ctx.pixels_per_point();
    let x = if narrow {
        rect.left() + (rect.width() - 14.0) / 2.0
    } else {
        rect.left() + 10.0
    };
    icon::paint(
        ui.painter(),
        svg,
        Rect::from_min_size(
            icon::snapped_origin(pos2(x, rect.top() + 10.0), ppp),
            vec2(14.0, 14.0),
        ),
        fg,
        1.0,
    );
    crate::ui::components::controls::focus_ring(
        ui,
        snap::rect(rect, ppp),
        8.0,
        0.0,
        v,
        focus,
        focused,
    );
    response.clicked()
        || (focused && ui.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Space)))
}

fn close_button(ui: &mut Ui, rect: Rect, label: &str, v: &Vars, focus: FocusCtx) -> bool {
    let id = Id::new("settings-close");
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    let focused = take_focus(ui, &response);
    let hovered = response.hovered();
    let ppp = ui.ctx().pixels_per_point();
    let (bg, fg) = if hovered {
        (v.bg_muted, v.fg)
    } else {
        (Rgba::TRANSPARENT, v.fg_muted)
    };
    if bg.a > 0.0 {
        BoxStyle {
            background: bg,
            radius: Corners::all(6.0),
            ..BoxStyle::default()
        }
        .paint(ui.painter(), snap::rect(rect, ppp));
    }
    icon::paint(
        ui.painter(),
        ICON_CLOSE,
        Rect::from_min_size(
            icon::snapped_origin(rect.min + vec2(6.0, 6.0), ppp),
            vec2(14.0, 14.0),
        ),
        fg,
        1.0,
    );
    crate::ui::components::controls::focus_ring(
        ui,
        snap::rect(rect, ppp),
        6.0,
        0.0,
        v,
        focus,
        focused,
    );
    response.clicked()
        || (focused && ui.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Space)))
}

fn paint_item(ui: &mut Ui, r: Rect, id: Id, c: &Ctx, item: &Item) -> Vec<SettingsAction> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = &c.view.vars;
    let mut out = Vec::new();
    match item {
        Item::Title(text) => {
            card::paint_title(ui, r, text, v);
        }
        Item::TitleRow(text, help) => {
            card::paint_divider(ui, r, v);
            let h3_top = r.top() + snap::centre(20.0, 18.0, ppp);
            text::paint(
                ui.painter(),
                Rect::from_min_size(pos2(r.left(), h3_top), vec2(r.width() - 28.0, 18.0)),
                text,
                &card::title_style(v),
            );
            let wiki = Rect::from_min_size(pos2(r.right() - 20.0, r.top()), vec2(20.0, 20.0));
            let style = button::wiki_style(v);
            let props = ButtonProps {
                label: "?",
                aria_label: Some(help),
                style: &style,
                enabled: true,
                vars: v,
                focus: c.focus,
            };
            if button::button(ui, wiki, id.with("wiki"), &props) {
                out.push(SettingsAction::OpenIntegrationsWiki);
            }
        }
        Item::Field {
            label,
            hint,
            control,
        } => {
            // `label.field` (and the PIN fields' `label[for]`): a click on
            // the label focuses the control.
            let target = id.with(match control {
                Control::Select { .. } => "select",
                Control::Slider { .. } => "slider",
                Control::Input { .. } => "input",
            });
            crate::ui::components::controls::label_target(ui, r, id.with("label"), target);
            let label_rect = Rect::from_min_size(r.min, vec2(r.width(), 18.0));
            let style = field_label_style(v);
            text::paint(ui.painter(), label_rect, label, &style);
            if let Some(hint) = hint {
                // An inline `span.hint` on the label's baseline.
                let w = text::measure(&ctx, &format!("{label} "), &style).unwrap_or(0.0);
                let hs = hint_style(v).nowrap(false);
                let base = style.line_box(&ctx).map_or(0.0, |b| b.baseline);
                let hb = hs.line_box(&ctx).map_or(0.0, |b| b.baseline);
                text::paint(
                    ui.painter(),
                    Rect::from_min_size(
                        pos2(r.left() + w, r.top() + base - hb),
                        vec2(r.width() - w, 16.0),
                    ),
                    hint,
                    &hs,
                );
            }
            let control_top = r.top() + 26.0;
            match control {
                Control::Select {
                    options,
                    selected,
                    actions,
                    disabled,
                } => {
                    let height = text_input::height(ui.ctx().pixels_per_point());
                    let rect =
                        Rect::from_min_size(pos2(r.left(), control_top), vec2(r.width(), height));
                    let props = SelectProps {
                        options,
                        selected: *selected,
                        disabled,
                        label,
                        vars: v,
                    };
                    let ev = select::select(ui, rect, id.with("select"), &props);
                    if let Some(action) = ev.changed.and_then(|i| actions.get(i)) {
                        out.push(action.clone());
                    }
                }
                Control::Slider {
                    min,
                    max,
                    step,
                    field,
                    commit_on_input,
                } => {
                    let rect =
                        Rect::from_min_size(pos2(r.left(), control_top), vec2(r.width(), 16.0));
                    let value =
                        crate::app::settings_panel::js_number(&c.panel.numeric(*field).input)
                            .unwrap_or(*min);
                    let props = SliderProps {
                        min: *min,
                        max: *max,
                        step: *step,
                        value,
                        label,
                        dark: c.view.dark,
                        vars: v,
                    };
                    let ev = slider::slider(ui, rect, id.with("slider"), &props);
                    if let Some(value) = ev.input {
                        out.push(SettingsAction::NumericInput(*field, format_number(value)));
                        if *commit_on_input {
                            out.push(SettingsAction::NumericCommit(*field));
                        }
                    }
                    if let (Some(value), false) = (ev.change, *commit_on_input) {
                        out.push(SettingsAction::NumericInput(*field, format_number(value)));
                        out.push(SettingsAction::NumericCommit(*field));
                    }
                }
                Control::Input {
                    kind,
                    field,
                    placeholder,
                    enabled,
                } => {
                    let height = text_input::height(ui.ctx().pixels_per_point());
                    let rect =
                        Rect::from_min_size(pos2(r.left(), control_top), vec2(r.width(), height));
                    let mut value = match field {
                        InputField::Numeric(f) => c.panel.numeric(*f).input.clone(),
                        InputField::Pin => c.panel.pin_code_input.clone(),
                        InputField::CurrentPin => c.panel.current_pin_input.clone(),
                    };
                    let style = text_input::settings_style(v);
                    let props = TextInputProps {
                        style: &style,
                        kind: *kind,
                        placeholder,
                        label,
                        max_len: match field {
                            InputField::Numeric(_) => None,
                            _ => Some(PIN_CODE_LENGTH),
                        },
                        enabled: *enabled,
                        vars: v,
                        dark: c.view.dark,
                    };
                    let input_id = id.with("input");
                    let ev = text_input::text_input(ui, rect, input_id, &mut value, &props);
                    if ev.changed {
                        out.push(match field {
                            InputField::Numeric(f) => SettingsAction::NumericInput(*f, value),
                            InputField::Pin => SettingsAction::PinInput(value),
                            InputField::CurrentPin => SettingsAction::CurrentPinInput(value),
                        });
                    }
                    if let InputField::Numeric(f) = field {
                        if ev.enter {
                            out.push(SettingsAction::NumericCommit(*f));
                            ui.memory_mut(|m| m.surrender_focus(input_id));
                        } else if ev.blurred {
                            out.push(SettingsAction::NumericCommit(*f));
                        }
                    }
                }
            }
        }
        Item::Toggle {
            label,
            description,
            enabled,
            disabled,
            action,
        } => {
            let on_off = c.on_off();
            let props = c.toggle_props(label, description, *enabled, *disabled, &on_off);
            if toggle::toggle(ui, r, id.with("toggle"), &props) {
                out.push(action.clone());
            }
        }
        Item::ThemeGrid => out.extend(theme_grid(ui, r, id, c)),
        Item::Button {
            label,
            enabled,
            action,
        } => {
            let style = button::export_style(v);
            let size = button::size(&ctx, label, &style);
            let rect = Rect::from_min_size(r.min, vec2(size.x.min(r.width()), size.y));
            let props = ButtonProps {
                label,
                aria_label: None,
                style: &style,
                enabled: *enabled,
                vars: v,
                focus: c.focus,
            };
            if button::button(ui, rect, id.with("button"), &props) {
                out.push(action.clone());
            }
        }
        Item::Link(label, action) => {
            let style = button::link_style(v);
            let size = button::size(&ctx, label, &style);
            let rect = Rect::from_min_size(r.min, vec2(size.x.min(r.width()), 18.0));
            let props = ButtonProps {
                label,
                aria_label: None,
                style: &style,
                enabled: true,
                vars: v,
                focus: c.focus,
            };
            if button::button(ui, rect, id.with("link"), &props) {
                out.push(action.clone());
            }
        }
        Item::Hint(text, danger) | Item::FieldHint(text, danger) => {
            let mut style = hint_style(v);
            if *danger {
                style = style.color(v.danger.to_color32());
            }
            text::paint(
                ui.painter(),
                Rect::from_min_size(r.min, vec2(r.width(), 0.0)),
                text,
                &style,
            );
        }
        Item::Stack(gap, items) => {
            let mut y = r.top();
            for (i, child) in items.iter().enumerate() {
                let h = item_height(&ctx, c, child, r.width());
                let rect = Rect::from_min_size(pos2(r.left(), y), vec2(r.width(), h));
                out.extend(paint_item(ui, rect, id.with(i), c, child));
                y += h + gap;
            }
        }
        Item::Platforms(item) => out.extend(platforms_tab::paint(ui, r, id, c, item)),
        Item::Platform(item) => out.extend(platform_tab::paint(ui, r, id, c, item)),
        Item::Steam(item) => out.extend(steam_tab::paint(ui, r, id, c, item)),
    }
    out
}

/// A slider value as `input.value` prints it.
fn format_number(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// `swatchStyle`: background layers, preview card, foreground and border of
/// a theme's swatch.
struct Swatch {
    bg: Rgba,
    images: Vec<Gradient>,
    card: Rgba,
    fg: Rgba,
    border: Rgba,
}

fn token_color(theme: &AppTheme, key: TokenKey) -> Rgba {
    css::color(theme.tokens.get(key)).unwrap_or(Rgba::TRANSPARENT)
}

fn swatch(theme: &AppTheme) -> Swatch {
    let rgb: Vec<f32> = theme
        .tokens
        .get(TokenKey::BgRgb)
        .split_whitespace()
        .filter_map(|n| n.parse().ok())
        .collect();
    let bg_rgb = match rgb.as_slice() {
        [r, g, b] => Rgba::new(*r, *g, *b, 1.0),
        _ => Rgba::new(0.0, 0.0, 0.0, 1.0),
    };
    let card = token_color(theme, TokenKey::BgCard);
    let fg = token_color(theme, TokenKey::Fg);
    let border = token_color(theme, TokenKey::Border);
    if !theme.glass {
        return Swatch {
            bg: bg_rgb,
            images: Vec::new(),
            card,
            fg,
            border,
        };
    }
    let liquid = theme.id == "liquid-glass";
    let veil = if liquid { 0.2 } else { 0.6 };
    let card_pct = if liquid { 0.22 } else { 0.72 };
    let veil_css = format!(
        "linear-gradient(rgb({} {} {} / {veil}), rgb({} {} {} / {veil}))",
        bg_rgb.r, bg_rgb.g, bg_rgb.b, bg_rgb.r, bg_rgb.g, bg_rgb.b
    );
    let images = [
        veil_css.as_str(),
        "linear-gradient(135deg, #8a63a8, #4f6d9e 48%, #a8825f)",
    ]
    .iter()
    .filter_map(|g| css::gradient(g))
    .collect();
    Swatch {
        bg: Rgba::TRANSPARENT,
        images,
        card: card.mix(card_pct, Rgba::TRANSPARENT),
        fg,
        border: border.mix(0.45, Rgba::TRANSPARENT),
    }
}

/// `flex: 1` items sharing `avail`: equal shares, except that an item never
/// shrinks under its min-content width (`min-width: auto`); frozen items
/// leave the rest to the others, as the flex algorithm resolves it.
fn flex_equal<const N: usize>(avail: f32, mins: &[f32; N]) -> [f32; N] {
    let mut frozen = [false; N];
    loop {
        let free: f32 = avail - (0..N).filter(|&i| frozen[i]).map(|i| mins[i]).sum::<f32>();
        let open = frozen.iter().filter(|f| !**f).count().max(1);
        let share = free / open as f32;
        let mut changed = false;
        for i in 0..N {
            if !frozen[i] && mins[i] > share {
                frozen[i] = true;
                changed = true;
            }
        }
        if !changed {
            return std::array::from_fn(|i| if frozen[i] { mins[i] } else { share });
        }
    }
}

/// `.theme-grid` and `.theme-actions-row`, the field of the Theme card.
fn theme_grid(ui: &mut Ui, r: Rect, id: Id, c: &Ctx) -> Vec<SettingsAction> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = &c.view.vars;
    let mut out = Vec::new();
    let (cols, _) = theme_columns(r.width());
    let tracks = snap::fr_tracks(r.width(), cols, 8.0, ppp);
    let top = r.top() + 4.0;
    for (i, theme) in c.view.themes.iter().enumerate() {
        let (col, row) = (i % cols, i / cols);
        let (left, col_w) = tracks[col];
        let rect = Rect::from_min_size(
            pos2(r.left() + left, top + (swatch_h(ppp) + 8.0) * row as f32),
            vec2(col_w, swatch_h(ppp)),
        );
        let name = if theme.is_custom {
            theme.display_name.clone()
        } else {
            tr(c.locale, theme.label_key)
        };
        let sid = id.with(("swatch", theme.id.as_str()));
        let response = ui.interact(rect, sid, Sense::click());
        let selected = c.panel.draft.theme_id == theme.id;
        response.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, selected, &name));
        let focused = take_focus(ui, &response);
        let hovered = response.hovered();
        let s = swatch(theme);
        let (border, shadows) = if selected {
            (
                s.fg,
                vec![BoxShadow {
                    offset: egui::Vec2::ZERO,
                    blur: 0.0,
                    spread: 1.0,
                    color: s.fg.mix(0.3, Rgba::TRANSPARENT),
                    inset: false,
                }],
            )
        } else if hovered {
            (
                s.fg.mix(0.25, Rgba::TRANSPARENT),
                vec![BoxShadow {
                    offset: vec2(0.0, 2.0),
                    blur: 8.0,
                    spread: 0.0,
                    color: Rgba::new(0.0, 0.0, 0.0, 0.18),
                    inset: false,
                }],
            )
        } else {
            (Rgba::TRANSPARENT, Vec::new())
        };
        let border = anim::transition(&ctx, sid.with("border"), border, &T120);
        let boxed = snap::rect(rect, ppp);
        BoxStyle {
            background: s.bg,
            images: s.images.clone(),
            radius: Corners::all(8.0),
            border: Border::all(snap::border_width(2.0, ppp), border),
            shadows,
            ..BoxStyle::default()
        }
        .paint(ui.painter(), boxed);
        let edge = snap::border_width(2.0, ppp);
        let inner = Rect::from_min_max(
            rect.min + vec2(8.0 + edge, 8.0 + edge),
            pos2(rect.right() - 8.0 - edge, rect.bottom() - 6.0 - edge),
        );
        let preview_border = snap::border_width(1.0, ppp);
        let preview =
            Rect::from_min_size(inner.min, vec2(inner.width(), 23.0 + 2.0 * preview_border));
        BoxStyle {
            background: s.card,
            radius: Corners::all(4.0),
            border: Border::all(snap::border_width(1.0, ppp), s.border),
            ..BoxStyle::default()
        }
        .paint(ui.painter(), snap::rect(preview, ppp));
        let bar_pad = 6.0 + preview_border;
        let bar_w = preview.width() - 2.0 * bar_pad;
        for (k, (w, a)) in [(bar_w, 0.35), (bar_w * 0.6, 0.2)].into_iter().enumerate() {
            let bar = Rect::from_min_size(
                preview.min + vec2(bar_pad, bar_pad + 7.0 * k as f32),
                vec2(w, 4.0),
            );
            BoxStyle {
                background: s.fg,
                radius: Corners::all(2.0),
                opacity: a,
                ..BoxStyle::default()
            }
            .paint(ui.painter(), snap::rect(bar, ppp));
        }
        let label_style = TextStyle::new(10.0, 600)
            .line_height(LineHeight::Number(1.2))
            .nowrap(true)
            .align(TextAlign::Center)
            .color(
                s.fg.alpha(s.fg.a * if selected { 1.0 } else { 0.7 })
                    .to_color32(),
            );
        text::paint(
            ui.painter(),
            Rect::from_min_size(
                pos2(inner.left(), preview.bottom() + 6.0),
                vec2(inner.width(), 12.0),
            ),
            &name,
            &label_style,
        );
        crate::ui::components::controls::focus_ring(ui, boxed, 8.0, 2.0, v, c.focus, focused);
        let keyed = focused && ui.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Space));
        if response.clicked() || keyed {
            out.push(SettingsAction::Theme(theme.id.clone()));
        }
    }
    let rows = c.view.themes.len().div_ceil(cols);
    let actions_top =
        top + rows as f32 * swatch_h(ppp) + rows.saturating_sub(1) as f32 * 8.0 + 8.0 + 4.0;
    let customize = if is_built_in_theme(&c.panel.draft.theme_id) {
        "settings.themeCustomize"
    } else {
        "settings.themeEdit"
    };
    let style = button::theme_action_style(v);
    let labels =
        [customize, "settings.themeImport", "settings.themeExport"].map(|key| tr(c.locale, key));
    let mins = labels.each_ref().map(|l| button::size(&ctx, l, &style).x);
    let widths = flex_equal(r.width() - 16.0, &mins);
    let mut x = r.left();
    for (k, label) in labels.iter().enumerate() {
        let rect = Rect::from_min_size(pos2(x, actions_top), vec2(widths[k], theme_action_h(ppp)));
        x += widths[k] + 8.0;
        let props = ButtonProps {
            label,
            aria_label: None,
            style: &style,
            enabled: true,
            vars: v,
            focus: c.focus,
        };
        // The clipboard import is a later port.
        if button::button(ui, rect, id.with(("theme-action", k)), &props) {
            match k {
                0 => out.push(SettingsAction::CustomizeTheme),
                2 => out.push(SettingsAction::ExportTheme),
                _ => {}
            }
        }
    }
    out
}

/// `t(key, params)` for callers outside the panel.
pub fn t_with(locale: Locale, key: &str, params: &[(&str, TranslationValue)]) -> String {
    translate_with(locale, key, params)
}

#[cfg(test)]
mod tests {
    use super::flex_equal;

    #[test]
    fn flex_equal_shares_and_freezes_wide_items() {
        assert_eq!(flex_equal(255.0, &[60.0, 60.0, 60.0]), [85.0, 85.0, 85.0]);
        // "Personnaliser" is wider than a third: it keeps its min-content
        // width and the others split the rest.
        assert_eq!(flex_equal(255.0, &[92.0, 60.0, 62.0]), [92.0, 81.5, 81.5]);
    }
}
