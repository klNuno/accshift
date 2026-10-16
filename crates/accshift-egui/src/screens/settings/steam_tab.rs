//! The Steam tab: `SteamSettingsTab.svelte`, `SteamSettingsSection.svelte`
//! and `Cs2BridgeSettings.svelte`.
//!
//! ```text
//! .settings-grid
//!   .card  h3, ToggleSetting (usernames), ToggleSetting (last login)
//!   .card  h3, ToggleSetting (admin), ToggleSetting (shutdown), launch options
//!   .card  h3, .field (folder, .input-row, p.error-hint)
//!   .card  h3, API key (.field-label-row, .input-row), avatar and ban delays
//!   .card  .title-row (h3, status pill, "?"), ToggleSetting, link, token
//! ```
//!
//! A `.field-label` holds its text, then inline a `span.hint` and a
//! `.key-badge` (`vertical-align: middle`) on one line box: the row grows
//! past 18 px to fit them, like Blink's ([`label_line`]).

use accshift_app::accounts::refresh::RefreshKind;
use accshift_app::i18n::{translate, Locale};
use egui::{pos2, vec2, Context, Id, Rect, Ui};

use super::platform_tab::{self, input_row, ERROR};
use super::{CardSpec, Ctx, Item as CardItem, View};
use crate::app::platforms::{PlatformDef, RuntimeOs};
use crate::app::settings_panel::{
    Cs2Form, Cs2Status, NumericField, SettingFlag, SettingsAction, SettingsPanel,
};
use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::components::controls::button::{self, ButtonProps, ButtonStyle, Transitions};
use crate::ui::components::controls::text_input::{self, FieldStyle, InputKind, TextInputProps};
use crate::ui::components::controls::toggle::NEUTRAL_ACCENT;
use crate::ui::components::controls::{card, label_target};
use crate::ui::paint::{Border, BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextStyle};

/// `.field { gap: 8px }`.
const FIELD_GAP: f32 = 8.0;
/// `.key-badge { margin-left: 6px }`.
const BADGE_MARGIN: f32 = 6.0;
/// `.title-side { gap: 8px }`.
const SIDE_GAP: f32 = 8.0;
/// `.title-row { gap: 10px }`.
const TITLE_GAP: f32 = 10.0;
/// The wiki button: 20x20.
const WIKI: f32 = 20.0;
const GREEN: Rgba = Rgba::hex(0x22c55e, 1.0);
const GREEN_TEXT: Rgba = Rgba::hex(0x4ade80, 1.0);
const RED: Rgba = Rgba::hex(0xef4444, 1.0);
const YELLOW: Rgba = Rgba::hex(0xeab308, 1.0);
const YELLOW_TEXT: Rgba = Rgba::hex(0xfde047, 1.0);

/// The Steam tab's own children of a card.
pub enum Item {
    /// `label.field`: launch options.
    LaunchOptions { label: String },
    /// The API key `.field`: label row (badge, "Get key"), password field,
    /// "Clear key", the error hint.
    ApiKey {
        label: String,
        badge: Option<String>,
        get_key: String,
        placeholder: String,
        clear: Option<String>,
        error: Option<String>,
    },
    /// The avatar and ban delays: label and hint, number field, "Refresh
    /// now".
    Refresh {
        label: String,
        hint: String,
        field: NumericField,
        button: String,
        kind: RefreshKind,
        loading: bool,
    },
    /// `.title-row`: the h3, the status pill and the wiki button.
    Cs2Title {
        title: String,
        status: String,
        tone: Cs2Status,
        help: String,
    },
    /// The bridge link: field, "Test", the test error.
    Cs2Link {
        label: String,
        placeholder: String,
        test: String,
        testing: bool,
        invalid: bool,
        error: Option<String>,
    },
    /// The bridge token: label, hint and badge, password field, clear.
    Cs2Token {
        label: String,
        hint: String,
        badge: Option<String>,
        placeholder: String,
        clear: Option<String>,
    },
}

/// `resolvePathPlaceholder` over the registry's Steam placeholders.
fn path_placeholder(os: RuntimeOs) -> &'static str {
    match os {
        RuntimeOs::Linux => "~/.local/share/Steam",
        RuntimeOs::Macos => "~/Library/Application Support/Steam",
        RuntimeOs::Windows | RuntimeOs::Unknown => "C:\\Program Files (x86)\\Steam",
    }
}

/// The Steam tab's cards.
pub(super) fn cards(
    panel: &SettingsPanel,
    view: &View,
    l: Locale,
    platform: &PlatformDef,
) -> Vec<CardSpec> {
    let t = |k: &str| translate(l, k);
    let d = &panel.draft;
    let accent = platform.accent;
    let toggle = |label: &str, enabled: bool, on: &str, off: &str, accent, action| {
        CardItem::Platform(platform_tab::Item::Toggle {
            label: t(label),
            description: String::new(),
            enabled,
            on: t(on),
            off: t(off),
            accent,
            action,
        })
    };
    let days_hint = format!(
        "({}, {})",
        t("settings.unitDays"),
        t("settings.zeroEachLaunch")
    );
    let fallback = Cs2Form::default();
    let cs2 = panel.cs2.as_ref().unwrap_or(&fallback);
    vec![
        CardSpec {
            wide: false,
            items: vec![
                CardItem::Title(t("settings.accountDisplay")),
                toggle(
                    "settings.showUsernames",
                    d.account_display.show_usernames,
                    "common.visible",
                    "common.hidden",
                    accent,
                    SettingsAction::Toggle(SettingFlag::ShowUsernames),
                ),
                toggle(
                    "settings.showSteamLastLogin",
                    d.account_display
                        .show_last_login_per_platform
                        .get("steam")
                        .copied()
                        .unwrap_or(false),
                    "common.on",
                    "common.off",
                    accent,
                    SettingsAction::ToggleLastLogin("steam".to_string()),
                ),
            ],
        },
        CardSpec {
            wide: false,
            items: vec![
                CardItem::Title(t("settings.steamLaunch")),
                toggle(
                    "settings.runSteamAsAdmin",
                    d.platform_settings.steam.run_as_admin,
                    "common.enabled",
                    "common.disabled",
                    NEUTRAL_ACCENT,
                    SettingsAction::Toggle(SettingFlag::SteamRunAsAdmin),
                ),
                toggle(
                    "settings.shutdownMode",
                    d.platform_settings.steam.shutdown_mode
                        == accshift_app::settings::SteamShutdownMode::Force,
                    "settings.shutdownModeForce",
                    "settings.shutdownModeGraceful",
                    NEUTRAL_ACCENT,
                    SettingsAction::Toggle(SettingFlag::SteamForceShutdown),
                ),
                CardItem::Steam(Item::LaunchOptions {
                    label: t("settings.launchOptions"),
                }),
            ],
        },
        CardSpec {
            wide: false,
            items: vec![
                CardItem::Title(t("settings.steamInstallation")),
                CardItem::Platform(platform_tab::Item::Path {
                    label: t("settings.steamFolder"),
                    placeholder: path_placeholder(view.runtime_os).to_string(),
                    platform: "steam".to_string(),
                    choose: t("common.choose"),
                    error: panel
                        .platform_path_errors
                        .get("steam")
                        .copied()
                        .unwrap_or(false)
                        .then(|| t("settings.pathInvalidHint")),
                }),
            ],
        },
        CardSpec {
            wide: false,
            items: vec![
                CardItem::Title(t("settings.steamApiAndData")),
                CardItem::Steam(Item::ApiKey {
                    label: t("settings.steamWebApiKey"),
                    badge: panel.api_key_configured.then(|| t("common.configured")),
                    get_key: t("settings.getKey"),
                    placeholder: t("settings.pasteApiKey"),
                    clear: panel.api_key_configured.then(|| t("settings.clearApiKey")),
                    error: panel.api_key_error.then(|| t("settings.apiKeyInvalidHint")),
                }),
                CardItem::Steam(Item::Refresh {
                    label: t("settings.avatarRefresh"),
                    hint: days_hint.clone(),
                    field: NumericField::AvatarCacheDays,
                    button: t("settings.refreshNow"),
                    kind: RefreshKind::Avatars,
                    loading: panel.refresh_loading(RefreshKind::Avatars),
                }),
                CardItem::Steam(Item::Refresh {
                    label: t("settings.banCheckDelay"),
                    hint: days_hint,
                    field: NumericField::BanCheckDays,
                    button: t("settings.refreshNow"),
                    kind: RefreshKind::Bans,
                    loading: panel.refresh_loading(RefreshKind::Bans),
                }),
            ],
        },
        CardSpec {
            wide: false,
            items: vec![
                CardItem::Steam(Item::Cs2Title {
                    title: t("settings.cs2Bridge"),
                    status: match cs2.status {
                        Cs2Status::Testing => t("settings.cs2BridgeStatusTesting"),
                        Cs2Status::Unknown => t("settings.cs2BridgeStatusUnknown"),
                        Cs2Status::Ok | Cs2Status::Fail => cs2.status_detail.clone(),
                    },
                    tone: cs2.status,
                    help: t("settings.help"),
                }),
                toggle(
                    "settings.cs2Bridge",
                    cs2.enabled,
                    "common.enabled",
                    "common.disabled",
                    accent,
                    SettingsAction::Cs2Toggle,
                ),
                CardItem::Steam(Item::Cs2Link {
                    label: t("settings.cs2BridgeLink"),
                    placeholder: t("settings.cs2BridgeLinkPlaceholder"),
                    test: t("settings.cs2BridgeTest"),
                    testing: cs2.status == Cs2Status::Testing,
                    invalid: cs2.url_error,
                    error: (cs2.status == Cs2Status::Fail && !cs2.status_error.is_empty())
                        .then(|| cs2.status_error.clone()),
                }),
                CardItem::Steam(Item::Cs2Token {
                    label: t("settings.cs2BridgeToken"),
                    hint: format!("({})", t("settings.cs2BridgeTokenOptional")),
                    badge: cs2.token_configured.then(|| t("common.configured")),
                    placeholder: t("settings.cs2BridgeTokenPlaceholder"),
                    clear: cs2
                        .token_configured
                        .then(|| t("settings.cs2BridgeClearToken")),
                }),
            ],
        },
    ]
}

fn label_style(v: &Vars) -> TextStyle {
    super::field_label_style(v)
}

/// `.hint` of the Steam tab: 11 px on the body's 1.5 line height.
fn hint_style(v: &Vars) -> TextStyle {
    TextStyle::new(11.0, 400)
        .line_height(LineHeight::Number(1.5))
        .nowrap(false)
        .color(v.fg_subtle.to_color32())
}

/// `.key-badge` and `.status-pill`: 10 px semibold on a 15 px line.
fn badge_font() -> TextStyle {
    TextStyle::new(10.0, 600)
        .line_height(LineHeight::Number(1.5))
        .nowrap(true)
}

/// The badge's border box: the 15 px line, `padding: 1px 7px` and the 1 px
/// border laid out floored to device pixels.
fn badge_size(ctx: &Context, label: &str) -> egui::Vec2 {
    let ppp = ctx.pixels_per_point();
    let b = snap::border_width(1.0, ppp);
    let w = snap::content_width(text::measure(ctx, label, &badge_font()).unwrap_or(0.0), ppp);
    vec2(w + 14.0 + 2.0 * b, 15.0 + 2.0 + 2.0 * b)
}

/// `p.error-hint`: 11 px, `#fca5a5`, wrapping; the bridge's breaks inside
/// a word too (`word-break: break-word`).
fn error_style(break_word: bool) -> TextStyle {
    let style = TextStyle::new(11.0, 400)
        .line_height(LineHeight::Number(1.5))
        .color(ERROR.to_color32());
    if break_word {
        style.overflow_anywhere()
    } else {
        style
    }
}

/// The line box of a `.field-label` with its inline children.
struct LabelLine {
    height: f32,
    /// The shared baseline, from the top of the line.
    baseline: f32,
}

/// Blink's line box: each inline box sits on the baseline with its own
/// half-leading (floored to device px), the badge centred on the label's
/// x-height (`vertical-align: middle`); the line spans the highest top and
/// the lowest bottom.
fn label_line(ctx: &Context, v: &Vars, hint: bool, badge: bool) -> LabelLine {
    let label = label_style(v);
    let strut = label
        .line_box(ctx)
        .map_or((14.0, 4.0), |b| (b.baseline, b.height - b.baseline));
    let (mut above, mut below) = strut;
    if hint {
        if let Some(b) = hint_style(v).line_box(ctx) {
            above = above.max(b.baseline);
            below = below.max(b.height - b.baseline);
        }
    }
    if badge {
        let h = badge_size(ctx, "").y;
        let mid = label.x_height(ctx).unwrap_or(6.0) / 2.0;
        above = above.max(mid + h / 2.0);
        below = below.max(h / 2.0 - mid);
    }
    LabelLine {
        height: above + below,
        baseline: above,
    }
}

/// Paints a `.field-label` line at the top of `r`: the text, the hint and
/// the badge after it, each on the shared baseline. Returns its width.
fn paint_label_line(
    ui: &Ui,
    r: Rect,
    v: &Vars,
    label: &str,
    hint: Option<&str>,
    badge: Option<&str>,
) -> f32 {
    let ctx = ui.ctx().clone();
    let line = label_line(&ctx, v, hint.is_some(), badge.is_some());
    let style = label_style(v);
    let label_base = style.line_box(&ctx).map_or(14.0, |b| b.baseline);
    let top = r.top() + line.baseline - label_base;
    text::paint(
        ui.painter(),
        Rect::from_min_size(pos2(r.left(), top), vec2(r.width(), 18.0)),
        label,
        &style,
    );
    let measure = |s: &str, st: &TextStyle| text::measure(&ctx, s, st).unwrap_or(0.0);
    let space = measure(&format!("{label} "), &style) - measure(label, &style);
    let mut x = r.left() + measure(label, &style);
    if let Some(hint) = hint {
        x += space;
        let hs = hint_style(v);
        let base = hs.line_box(&ctx).map_or(12.0, |b| b.baseline);
        text::paint(
            ui.painter(),
            Rect::from_min_size(
                pos2(x, r.top() + line.baseline - base),
                vec2((r.right() - x).max(0.0), 16.5),
            ),
            hint,
            &hs,
        );
        x += measure(hint, &hs);
    }
    if let Some(badge) = badge {
        x += space + BADGE_MARGIN;
        let size = badge_size(&ctx, badge);
        let mid = style.x_height(&ctx).unwrap_or(6.0) / 2.0;
        let top = r.top() + line.baseline - mid - size.y / 2.0;
        paint_pill(
            ui,
            Rect::from_min_size(pos2(x, top), size),
            badge,
            pill_colors(Cs2Status::Ok, v),
        );
        x += size.x;
    }
    x - r.left()
}

/// Border, background and text of a pill in `tone` (`.key-badge` is the
/// `ok` one).
fn pill_colors(tone: Cs2Status, v: &Vars) -> (Rgba, Rgba, Rgba) {
    let tinted =
        |c: Rgba, text: Rgba| (c.mix(0.45, v.border), c.mix(0.14, Rgba::TRANSPARENT), text);
    match tone {
        Cs2Status::Unknown => (
            v.fg_subtle.mix(0.45, v.border),
            v.fg_subtle.mix(0.10, Rgba::TRANSPARENT),
            v.fg_muted,
        ),
        Cs2Status::Ok => tinted(GREEN, GREEN_TEXT),
        Cs2Status::Fail => tinted(RED, ERROR),
        Cs2Status::Testing => tinted(YELLOW, YELLOW_TEXT),
    }
}

/// A round pill: border, background, its label clipped with an ellipsis.
fn paint_pill(ui: &Ui, rect: Rect, label: &str, (border, bg, fg): (Rgba, Rgba, Rgba)) {
    let ppp = ui.ctx().pixels_per_point();
    let b = snap::border_width(1.0, ppp);
    let boxed = snap::rect(rect, ppp);
    BoxStyle {
        background: bg,
        radius: Corners::all(boxed.height() / 2.0),
        border: Border::all(b, border),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), boxed);
    let inner = Rect::from_min_max(
        pos2(rect.left() + b + 7.0, rect.top() + b + 1.0),
        pos2(rect.right() - b - 7.0, rect.top() + b + 16.0),
    );
    text::paint(
        &ui.painter().with_clip_rect(inner.intersect(ui.clip_rect())),
        inner,
        label,
        &badge_font().color(fg.to_color32()),
    );
}

/// `.clear-key-btn`: 11 px muted text, red on hover.
fn clear_style(v: &Vars) -> ButtonStyle {
    ButtonStyle {
        font: TextStyle::new(11.0, 400).line_height(LineHeight::Number(1.5)),
        padding: vec2(10.0, 0.0),
        border: 1.0,
        radius: 8.0,
        background: v.bg_card,
        background_hover: v.bg_card,
        border_color: v.border,
        border_hover: RED.mix(0.45, v.border),
        color: v.fg_muted,
        color_hover: ERROR,
        underline_hover: false,
        disabled_opacity: 1.0,
        transitions: Transitions {
            background: false,
            border: true,
            color: true,
        },
    }
}

/// `.text-input.invalid`: a red border, focused or not.
pub(super) fn invalid_style(style: FieldStyle) -> FieldStyle {
    FieldStyle {
        border_color: RED,
        border_focus: RED,
        ..style
    }
}

fn error_height(ctx: &Context, error: &Option<String>, width: f32, break_word: bool) -> f32 {
    error.as_ref().map_or(0.0, |e| {
        FIELD_GAP + super::wrapped_height(ctx, e, &error_style(break_word), width)
    })
}

fn paint_error(ui: &Ui, top: f32, r: Rect, error: &Option<String>, break_word: bool) {
    if let Some(e) = error {
        text::paint(
            ui.painter(),
            Rect::from_min_size(pos2(r.left(), top + FIELD_GAP), vec2(r.width(), 0.0)),
            e,
            &error_style(break_word),
        );
    }
}

pub(super) fn height(ctx: &Context, c: &Ctx, item: &Item, width: f32) -> f32 {
    let ppp = ctx.pixels_per_point();
    let v = &c.view.vars;
    let input = text_input::height(ppp);
    match item {
        Item::LaunchOptions { .. } => 18.0 + FIELD_GAP + input,
        Item::ApiKey { badge, error, .. } => {
            label_line(ctx, v, false, badge.is_some()).height
                + FIELD_GAP
                + input
                + error_height(ctx, error, width, false)
        }
        Item::Refresh { .. } => label_line(ctx, v, true, false).height + FIELD_GAP + input,
        Item::Cs2Title { .. } => card::title_height(ppp) + 8.0 + snap::border_width(1.0, ppp),
        Item::Cs2Link { error, .. } => {
            18.0 + FIELD_GAP + input + error_height(ctx, error, width, true)
        }
        Item::Cs2Token { badge, .. } => {
            label_line(ctx, v, true, badge.is_some()).height + FIELD_GAP + input
        }
    }
}

/// One `.text-input` of the tab.
struct Field<'a> {
    kind: InputKind,
    placeholder: &'a str,
    label: &'a str,
    invalid: bool,
}

fn field(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    c: &Ctx,
    value: &mut String,
    f: &Field,
) -> text_input::TextInputEvents {
    let v = &c.view.vars;
    let mut style = text_input::settings_style(v);
    if f.invalid {
        style = invalid_style(style);
    }
    let props = TextInputProps {
        style: &style,
        kind: f.kind,
        placeholder: f.placeholder,
        label: f.label,
        max_len: None,
        enabled: true,
        vars: v,
        dark: c.view.dark,
    };
    text_input::text_input(ui, rect, id, value, &props)
}

fn push_button(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    c: &Ctx,
    label: &str,
    style: &ButtonStyle,
    enabled: bool,
) -> bool {
    let props = ButtonProps {
        label,
        aria_label: None,
        style,
        enabled,
        vars: &c.view.vars,
        focus: c.focus,
    };
    button::button(ui, rect, id, &props)
}

pub(super) fn paint(ui: &mut Ui, r: Rect, id: Id, c: &Ctx, item: &Item) -> Vec<SettingsAction> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = &c.view.vars;
    let input_h = text_input::height(ppp);
    let mut out = Vec::new();
    match item {
        Item::LaunchOptions { label } => {
            label_target(ui, r, id.with("label"), id.with("launch"));
            paint_label_line(ui, r, v, label, None, None);
            let rect = Rect::from_min_size(
                pos2(r.left(), r.top() + 18.0 + FIELD_GAP),
                vec2(r.width(), input_h),
            );
            let mut value = c.panel.draft.platform_settings.steam.launch_options.clone();
            let f = Field {
                kind: InputKind::Text,
                placeholder: "-silent -vgui",
                label,
                invalid: false,
            };
            if field(ui, rect, id.with("launch"), c, &mut value, &f).changed {
                out.push(SettingsAction::SteamLaunchOptions(value));
            }
        }
        Item::ApiKey {
            label,
            badge,
            get_key,
            placeholder,
            clear,
            error,
        } => {
            let line = label_line(&ctx, v, false, badge.is_some());
            paint_label_line(ui, r, v, label, None, badge.as_deref());
            // `.field-label-row`: the link at the end, centred on the line.
            let link = button::link_style(v);
            let w = button::size(&ctx, get_key, &link).x;
            let link_rect = Rect::from_min_size(
                pos2(r.right() - w, r.top() + (line.height - 18.0) / 2.0),
                vec2(w, 18.0),
            );
            if push_button(ui, link_rect, id.with("get-key"), c, get_key, &link, true) {
                out.push(SettingsAction::OpenSteamApiKeyPage);
            }
            let row = Rect::from_min_size(
                pos2(r.left(), r.top() + line.height + FIELD_GAP),
                vec2(r.width(), input_h),
            );
            let clear_style = clear_style(v);
            let (field_rect, clear_rect) = match clear {
                Some(label) => {
                    let (f, b) = input_row(&ctx, row, label, &clear_style);
                    (f, Some(b))
                }
                None => (row, None),
            };
            let mut value = c.panel.api_key.clone();
            let f = Field {
                kind: InputKind::Password,
                placeholder,
                label,
                invalid: error.is_some(),
            };
            if field(ui, field_rect, id.with("api-key"), c, &mut value, &f).changed {
                out.push(SettingsAction::ApiKeyInput(value));
            }
            if let (Some(label), Some(rect)) = (clear, clear_rect) {
                if push_button(ui, rect, id.with("clear"), c, label, &clear_style, true) {
                    out.push(SettingsAction::ClearApiKey);
                }
            }
            paint_error(ui, row.bottom(), r, error, false);
        }
        Item::Refresh {
            label,
            hint,
            field: numeric,
            button: refresh,
            kind,
            loading,
        } => {
            let line = label_line(&ctx, v, true, false);
            paint_label_line(ui, r, v, label, Some(hint), None);
            let row = Rect::from_min_size(
                pos2(r.left(), r.top() + line.height + FIELD_GAP),
                vec2(r.width(), input_h),
            );
            // No `:disabled` rule: a running refresh looks the same.
            let browse = ButtonStyle {
                disabled_opacity: 1.0,
                ..button::browse_style(v)
            };
            let (field_rect, button_rect) = input_row(&ctx, row, refresh, &browse);
            let mut value = c.panel.numeric(*numeric).input.clone();
            let input_id = id.with("days");
            let f = Field {
                kind: InputKind::Number,
                placeholder: "",
                label,
                invalid: false,
            };
            let ev = field(ui, field_rect, input_id, c, &mut value, &f);
            if ev.changed {
                out.push(SettingsAction::NumericInput(*numeric, value));
            }
            if ev.enter {
                out.push(SettingsAction::NumericCommit(*numeric));
                ui.memory_mut(|m| m.surrender_focus(input_id));
            } else if ev.blurred {
                out.push(SettingsAction::NumericCommit(*numeric));
            }
            if push_button(
                ui,
                button_rect,
                id.with("refresh"),
                c,
                refresh,
                &browse,
                !loading,
            ) {
                out.push(SettingsAction::RefreshNow(*kind));
            }
        }
        Item::Cs2Title {
            title,
            status,
            tone,
            help,
        } => {
            let h3_h = card::title_height(ppp);
            let style = card::title_style(v);
            // The h3 keeps its own divider, as wide as its text.
            let h3_w = snap::content_width(text::measure(&ctx, title, &style).unwrap_or(0.0), ppp)
                .min(r.width());
            card::paint_title(ui, Rect::from_min_size(r.min, vec2(h3_w, h3_h)), title, v);
            card::paint_divider(ui, r, v);
            // `.title-side`, at the end and centred on the h3; the pill
            // gives way first.
            let pill = badge_size(&ctx, status);
            let room = (r.width() - h3_w - TITLE_GAP - SIDE_GAP - WIKI).max(0.0);
            let pill_w = pill.x.min(room);
            let side_h = WIKI.max(pill.y);
            let side_top = r.top() + (h3_h - side_h) / 2.0;
            let wiki = Rect::from_min_size(
                pos2(r.right() - WIKI, side_top + (side_h - WIKI) / 2.0),
                vec2(WIKI, WIKI),
            );
            let pill_rect = Rect::from_min_size(
                pos2(
                    wiki.left() - SIDE_GAP - pill_w,
                    side_top + (side_h - pill.y) / 2.0,
                ),
                vec2(pill_w, pill.y),
            );
            paint_pill(ui, pill_rect, status, pill_colors(*tone, v));
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
                out.push(SettingsAction::OpenCs2Wiki);
            }
        }
        Item::Cs2Link {
            label,
            placeholder,
            test,
            testing,
            invalid,
            error,
        } => {
            label_target(ui, r, id.with("label"), id.with("url"));
            paint_label_line(ui, r, v, label, None, None);
            let row = Rect::from_min_size(
                pos2(r.left(), r.top() + 18.0 + FIELD_GAP),
                vec2(r.width(), input_h),
            );
            let browse = ButtonStyle {
                disabled_opacity: 0.6,
                ..button::browse_style(v)
            };
            let (field_rect, button_rect) = input_row(&ctx, row, test, &browse);
            let fallback = Cs2Form::default();
            let mut value = c.panel.cs2.as_ref().unwrap_or(&fallback).url.clone();
            let f = Field {
                kind: InputKind::Text,
                placeholder,
                label,
                invalid: *invalid,
            };
            let ev = field(ui, field_rect, id.with("url"), c, &mut value, &f);
            if ev.changed {
                out.push(SettingsAction::Cs2Url(value));
            }
            if ev.blurred {
                out.push(SettingsAction::Cs2UrlBlur);
            }
            if push_button(ui, button_rect, id.with("test"), c, test, &browse, !testing) {
                out.push(SettingsAction::Cs2Test);
            }
            paint_error(ui, row.bottom(), r, error, true);
        }
        Item::Cs2Token {
            label,
            hint,
            badge,
            placeholder,
            clear,
        } => {
            let line = label_line(&ctx, v, true, badge.is_some());
            paint_label_line(ui, r, v, label, Some(hint), badge.as_deref());
            let row = Rect::from_min_size(
                pos2(r.left(), r.top() + line.height + FIELD_GAP),
                vec2(r.width(), input_h),
            );
            let clear_style = clear_style(v);
            let (field_rect, clear_rect) = match clear {
                Some(label) => {
                    let (f, b) = input_row(&ctx, row, label, &clear_style);
                    (f, Some(b))
                }
                None => (row, None),
            };
            let fallback = Cs2Form::default();
            let mut value = c
                .panel
                .cs2
                .as_ref()
                .unwrap_or(&fallback)
                .token_input
                .clone();
            let f = Field {
                kind: InputKind::Password,
                placeholder,
                label,
                invalid: false,
            };
            let ev = field(ui, field_rect, id.with("token"), c, &mut value, &f);
            if ev.changed {
                out.push(SettingsAction::Cs2Token(value));
            }
            if ev.blurred {
                out.push(SettingsAction::Cs2TokenBlur);
            }
            if let (Some(label), Some(rect)) = (clear, clear_rect) {
                if push_button(ui, rect, id.with("clear"), c, label, &clear_style, true) {
                    out.push(SettingsAction::Cs2ClearToken);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_placeholders_follow_the_registry() {
        assert_eq!(
            path_placeholder(RuntimeOs::Windows),
            "C:\\Program Files (x86)\\Steam"
        );
        assert_eq!(path_placeholder(RuntimeOs::Linux), "~/.local/share/Steam");
        assert_eq!(
            path_placeholder(RuntimeOs::Macos),
            "~/Library/Application Support/Steam"
        );
    }
}
