//! The per-platform tabs: `PlatformPathSettingsTab.svelte` (Riot,
//! Battle.net, Ubisoft, Epic, GOG, Jagex, Discord) and
//! `RobloxSettingsTab.svelte`.
//!
//! ```text
//! .settings-grid
//!   SettingsCard  h3, ToggleSetting (last login) [, health check]
//!   SettingsCard  h3, .field (label, .input-row: path, "Choose...")
//!                 or the cookie paste: p.hint, .input-row, p.status.error
//! ```
//!
//! Inside the panel a `SettingsCard` keeps the panel's card box and only
//! adds its accent ([`card::CardKind::Platform`]). The tab shows nothing
//! until the panel has read the platform paths (`platformId in
//! platformPaths`).

use accshift_app::i18n::{translate, Locale};
use egui::{pos2, vec2, Context, Id, Rect, Ui};

use super::{CardSpec, Ctx, Item as CardItem, View};
use crate::app::platforms::{PlatformDef, RuntimeOs};
use crate::app::settings_panel::{SettingsAction, SettingsPanel};
use crate::theme::Rgba;
use crate::ui::components::controls::button::{self, ButtonProps};
use crate::ui::components::controls::text_input::{self, InputKind, TextInputProps};
use crate::ui::components::controls::toggle::{self, ToggleProps};
use crate::ui::text::{self, LineHeight, TextStyle};

/// `.input-row { gap: 8px }`.
pub(super) const ROW_GAP: f32 = 8.0;
/// `.field { gap: 8px }` under its 18 px label.
const LABEL_H: f32 = 18.0 + 8.0;
/// `.status.error { color: #fca5a5 }`.
pub(super) const ERROR: Rgba = Rgba::hex(0xfca5a5, 1.0);

/// The tab's own children of a card.
pub enum Item {
    /// `ToggleSetting.svelte`, in the platform's accent or the neutral one.
    Toggle {
        label: String,
        description: String,
        enabled: bool,
        on: String,
        off: String,
        accent: Rgba,
        action: SettingsAction,
    },
    /// `.field`: the label, then the path field and "Choose...", and on the
    /// Steam tab the save error under them (`.invalid`, `p.error-hint`).
    Path {
        label: String,
        placeholder: String,
        platform: String,
        choose: String,
        error: Option<String>,
    },
    /// The cookie paste row: a password field and the add button.
    Cookie { add: String },
    /// `p.status.error`.
    Error(String),
}

/// What a path tab differs in: `PlatformPathSettingsTab`'s props and the
/// registry's `pathLabelKey` and `pathPlaceholder`.
struct PathTab {
    title_key: &'static str,
    last_login_key: &'static str,
    last_login_default: bool,
    path_label_key: &'static str,
    /// `pathPlaceholder` per system: Windows, Linux, macOS.
    placeholder: [Option<&'static str>; 3],
}

const fn path_tab(
    title_key: &'static str,
    last_login_key: &'static str,
    last_login_default: bool,
    path_label_key: &'static str,
    placeholder: [Option<&'static str>; 3],
) -> PathTab {
    PathTab {
        title_key,
        last_login_key,
        last_login_default,
        path_label_key,
        placeholder,
    }
}

/// The descriptor platforms' placeholder: the first `path` candidate of the
/// shipped descriptor, rendered for Windows (`renderTemplate`), the only
/// system they list.
fn path_tab_for(id: &str) -> Option<PathTab> {
    Some(match id {
        "riot" => path_tab(
            "settings.riot",
            "settings.showRiotLastLogin",
            false,
            "settings.riotClientPath",
            [
                Some("C:\\Riot Games\\Riot Client\\RiotClientServices.exe"),
                None,
                None,
            ],
        ),
        "battle-net" => path_tab(
            "settings.battleNet",
            "settings.showBattleNetLastLogin",
            false,
            "settings.battleNetPath",
            [
                Some("C:\\Program Files (x86)\\Battle.net\\Battle.net Launcher.exe"),
                None,
                Some("/Applications/Battle.net.app"),
            ],
        ),
        "ubisoft" => path_tab(
            "settings.ubisoft",
            "settings.showUbisoftLastLogin",
            true,
            "settings.ubisoftPath",
            [
                Some("%ProgramFiles(x86)%\\Ubisoft\\Ubisoft Game Launcher"),
                None,
                None,
            ],
        ),
        "epic" => path_tab(
            "settings.epic",
            "settings.showEpicLastLogin",
            true,
            "settings.epicPath",
            [
                Some("%ProgramFiles(x86)%\\Epic Games\\Launcher\\Portal\\Binaries\\Win64\\EpicGamesLauncher.exe"),
                None,
                None,
            ],
        ),
        "gog" => path_tab(
            "settings.gog",
            "settings.showGogLastLogin",
            true,
            "settings.gogPath",
            [
                Some("%ProgramFiles(x86)%\\GOG Galaxy\\GalaxyClient.exe"),
                None,
                None,
            ],
        ),
        "jagex" => path_tab(
            "settings.jagex",
            "settings.showJagexLastLogin",
            true,
            "settings.jagexPath",
            [
                Some("%ProgramFiles(x86)%\\Jagex Launcher\\JagexLauncher.exe"),
                None,
                None,
            ],
        ),
        "discord" => path_tab(
            "settings.discord",
            "settings.showDiscordLastLogin",
            true,
            "settings.discordPath",
            [Some("%LOCALAPPDATA%\\Discord"), None, None],
        ),
        _ => return None,
    })
}

/// `resolvePathPlaceholder`: the system's own, else Windows, Linux, macOS.
fn resolve_placeholder(placeholder: &[Option<&'static str>; 3], os: RuntimeOs) -> &'static str {
    let own = match os {
        RuntimeOs::Windows => placeholder[0],
        RuntimeOs::Linux => placeholder[1],
        RuntimeOs::Macos => placeholder[2],
        RuntimeOs::Unknown => None,
    };
    own.or(placeholder[0])
        .or(placeholder[1])
        .or(placeholder[2])
        .unwrap_or("")
}

/// The last login toggle every platform tab opens with.
fn last_login(
    panel: &SettingsPanel,
    l: Locale,
    platform: &PlatformDef,
    key: &str,
    default: bool,
) -> CardItem {
    let t = |k: &str| translate(l, k);
    let id = platform.id.as_str();
    CardItem::Platform(Item::Toggle {
        label: t(key),
        description: String::new(),
        enabled: panel
            .draft
            .account_display
            .show_last_login_per_platform
            .get(id)
            .copied()
            .unwrap_or(default),
        on: t("common.on"),
        off: t("common.off"),
        accent: platform.accent,
        action: SettingsAction::ToggleLastLogin(id.to_string()),
    })
}

/// The cards of the tab `platform:<id>`; none for a tab without a port yet
/// or before the paths are read.
pub(super) fn cards(
    panel: &SettingsPanel,
    view: &View,
    l: Locale,
    platform: &PlatformDef,
) -> Vec<CardSpec> {
    let t = |k: &str| translate(l, k);
    let id = platform.id.as_str();
    if !panel.platform_paths.contains_key(id) {
        return Vec::new();
    }
    if id == "steam" {
        return super::steam_tab::cards(panel, view, l, platform);
    }
    if id == "roblox" {
        let mut cookie = vec![
            CardItem::Title(t("roblox.cookiePasteTitle")),
            CardItem::Hint(t("roblox.cookiePasteHint"), false),
            CardItem::Platform(Item::Cookie {
                add: if panel.roblox_adding {
                    "...".to_string()
                } else {
                    t("common.add")
                },
            }),
        ];
        if !panel.roblox_error.is_empty() {
            cookie.push(CardItem::Platform(Item::Error(panel.roblox_error.clone())));
        }
        return vec![
            CardSpec {
                wide: false,
                items: vec![
                    CardItem::Title(t("settings.accountDisplay")),
                    last_login(panel, l, platform, "settings.showRobloxLastLogin", true),
                    CardItem::Platform(Item::Toggle {
                        label: t("settings.robloxHealthCheck"),
                        description: t("settings.robloxHealthCheckHint"),
                        enabled: panel.draft.health_check_per_platform.get(id) != Some(&false),
                        on: t("common.enabled"),
                        off: t("common.disabled"),
                        accent: platform.accent,
                        action: SettingsAction::ToggleHealthCheck(id.to_string()),
                    }),
                ],
            },
            CardSpec {
                wide: false,
                items: cookie,
            },
        ];
    }
    let Some(tab) = path_tab_for(id) else {
        return Vec::new();
    };
    vec![
        CardSpec {
            wide: false,
            items: vec![
                CardItem::Title(t("settings.accountDisplay")),
                last_login(
                    panel,
                    l,
                    platform,
                    tab.last_login_key,
                    tab.last_login_default,
                ),
            ],
        },
        CardSpec {
            wide: false,
            items: vec![
                CardItem::Title(t(tab.title_key)),
                CardItem::Platform(Item::Path {
                    label: t(tab.path_label_key),
                    placeholder: resolve_placeholder(&tab.placeholder, view.runtime_os).to_string(),
                    platform: id.to_string(),
                    choose: t("common.choose"),
                    error: None,
                }),
            ],
        },
    ]
}

pub(super) fn error_style() -> TextStyle {
    TextStyle::new(11.0, 400)
        .line_height(LineHeight::Number(1.5))
        .color(ERROR.to_color32())
}

fn toggle_props<'a>(
    c: &'a Ctx,
    accent: Rgba,
    label: &'a str,
    description: &'a str,
    enabled: bool,
    on: &'a str,
    off: &'a str,
) -> ToggleProps<'a> {
    ToggleProps {
        label,
        description,
        enabled,
        accent,
        on_label: on,
        off_label: off,
        disabled: false,
        vars: &c.view.vars,
        focus: c.focus,
    }
}

pub(super) fn height(ctx: &Context, c: &Ctx, item: &Item, width: f32) -> f32 {
    let ppp = ctx.pixels_per_point();
    match item {
        Item::Toggle {
            label,
            description,
            enabled,
            on,
            off,
            accent,
            ..
        } => toggle::height(
            ctx,
            width,
            &toggle_props(c, *accent, label, description, *enabled, on, off),
        ),
        Item::Path { error, .. } => {
            LABEL_H
                + text_input::height(ppp)
                + error.as_ref().map_or(0.0, |e| {
                    ROW_GAP + super::wrapped_height(ctx, e, &error_style(), width)
                })
        }
        Item::Cookie { .. } => text_input::height(ppp),
        Item::Error(message) => super::wrapped_height(ctx, message, &error_style(), width),
    }
}

/// `.input-row`: the field takes what the button leaves.
pub(super) fn input_row(
    ctx: &Context,
    row: Rect,
    label: &str,
    style: &button::ButtonStyle,
) -> (Rect, Rect) {
    let w = button::size(ctx, label, style).x.min(row.width());
    let button = Rect::from_min_max(pos2(row.right() - w, row.top()), row.max);
    let field = Rect::from_min_max(row.min, pos2(button.left() - ROW_GAP, row.bottom()));
    (field, button)
}

pub(super) fn paint(ui: &mut Ui, r: Rect, id: Id, c: &Ctx, item: &Item) -> Vec<SettingsAction> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = &c.view.vars;
    let mut out = Vec::new();
    match item {
        Item::Toggle {
            label,
            description,
            enabled,
            on,
            off,
            accent,
            action,
        } => {
            let props = toggle_props(c, *accent, label, description, *enabled, on, off);
            if toggle::toggle(ui, r, id.with("toggle"), &props) {
                out.push(action.clone());
            }
        }
        Item::Path {
            label,
            placeholder,
            platform,
            choose,
            error,
        } => {
            let style = super::field_label_style(v);
            text::paint(
                ui.painter(),
                Rect::from_min_size(r.min, vec2(r.width(), 18.0)),
                label,
                &style,
            );
            let row = Rect::from_min_size(
                pos2(r.left(), r.top() + LABEL_H),
                vec2(r.width(), text_input::height(ppp)),
            );
            let browse = button::browse_style(v);
            let (field, choose_rect) = input_row(&ctx, row, choose, &browse);
            let mut value = c
                .panel
                .platform_paths
                .get(platform)
                .cloned()
                .unwrap_or_default();
            let mut field_style = text_input::settings_style(v);
            if error.is_some() {
                field_style = super::steam_tab::invalid_style(field_style);
            }
            let props = TextInputProps {
                style: &field_style,
                kind: InputKind::Text,
                placeholder,
                label,
                max_len: None,
                enabled: true,
                vars: v,
                dark: c.view.dark,
            };
            let ev = text_input::text_input(
                ui,
                field,
                id.with(("path", platform.as_str())),
                &mut value,
                &props,
            );
            if ev.changed {
                out.push(SettingsAction::PlatformPath(platform.clone(), value));
            }
            let props = ButtonProps {
                label: choose,
                aria_label: None,
                style: &browse,
                enabled: true,
                vars: v,
                focus: c.focus,
            };
            if button::button(ui, choose_rect, id.with("choose"), &props) {
                out.push(SettingsAction::ChoosePlatformPath(platform.clone()));
            }
            if let Some(e) = error {
                text::paint(
                    ui.painter(),
                    Rect::from_min_size(
                        pos2(r.left(), row.bottom() + ROW_GAP),
                        vec2(r.width(), 0.0),
                    ),
                    e,
                    &error_style(),
                );
            }
        }
        Item::Cookie { add } => {
            let browse = button::browse_style(v);
            let (field, add_rect) = input_row(&ctx, r, add, &browse);
            let mut value = c.panel.roblox_cookie.clone();
            let field_style = text_input::settings_style(v);
            let busy = c.panel.roblox_adding;
            let props = TextInputProps {
                style: &field_style,
                kind: InputKind::Password,
                placeholder: ".ROBLOSECURITY",
                label: ".ROBLOSECURITY",
                max_len: None,
                enabled: !busy,
                vars: v,
                dark: c.view.dark,
            };
            let ev = text_input::text_input(ui, field, id.with("cookie"), &mut value, &props);
            if ev.changed {
                out.push(SettingsAction::RobloxCookie(value));
            }
            let props = ButtonProps {
                label: add,
                aria_label: None,
                style: &browse,
                enabled: !busy && !c.panel.roblox_cookie.trim().is_empty(),
                vars: v,
                focus: c.focus,
            };
            if button::button(ui, add_rect, id.with("add"), &props) {
                out.push(SettingsAction::AddRobloxCookie);
            }
        }
        Item::Error(message) => {
            text::paint(
                ui.painter(),
                Rect::from_min_size(r.min, vec2(r.width(), 0.0)),
                message,
                &error_style(),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `renderTemplate` of `registry.ts`, Windows side.
    fn render_windows(template: &str) -> String {
        let mut out = String::new();
        let mut rest = template;
        while let Some(start) = rest.find("${") {
            out.push_str(&rest[..start]);
            let end = rest[start..].find('}').unwrap() + start;
            out.push('%');
            out.push_str(&rest[start + 2..end]);
            out.push('%');
            rest = &rest[end + 1..];
        }
        out.push_str(rest);
        out.replace('/', "\\")
    }

    #[test]
    fn descriptor_placeholders_follow_the_shipped_descriptors() {
        let shipped = [
            (
                "ubisoft",
                include_str!(
                    "../../../../accshift-core/src/platforms/descriptor/descriptors/ubisoft.json"
                ),
            ),
            (
                "epic",
                include_str!(
                    "../../../../accshift-core/src/platforms/descriptor/descriptors/epic.json"
                ),
            ),
            (
                "gog",
                include_str!(
                    "../../../../accshift-core/src/platforms/descriptor/descriptors/gog.json"
                ),
            ),
            (
                "jagex",
                include_str!(
                    "../../../../accshift-core/src/platforms/descriptor/descriptors/jagex.json"
                ),
            ),
            (
                "discord",
                include_str!(
                    "../../../../accshift-core/src/platforms/descriptor/descriptors/discord.json"
                ),
            ),
        ];
        for (id, json) in shipped {
            let descriptor: serde_json::Value = serde_json::from_str(json).unwrap();
            let os = descriptor["os"].as_object().unwrap();
            assert_eq!(os.keys().collect::<Vec<_>>(), ["windows"], "{id}");
            let template = os["windows"]["executable"]["candidates"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["kind"] == "path" && c["template"].is_string())
                .unwrap()["template"]
                .as_str()
                .unwrap();
            let tab = path_tab_for(id).unwrap();
            for system in [RuntimeOs::Windows, RuntimeOs::Linux, RuntimeOs::Macos] {
                assert_eq!(
                    resolve_placeholder(&tab.placeholder, system),
                    render_windows(template),
                    "{id}"
                );
            }
        }
    }

    #[test]
    fn placeholders_fall_back_like_resolve_path_placeholder() {
        let battle = path_tab_for("battle-net").unwrap();
        assert_eq!(
            resolve_placeholder(&battle.placeholder, RuntimeOs::Macos),
            "/Applications/Battle.net.app"
        );
        assert_eq!(
            resolve_placeholder(&battle.placeholder, RuntimeOs::Linux),
            "C:\\Program Files (x86)\\Battle.net\\Battle.net Launcher.exe"
        );
        assert!(path_tab_for("roblox").is_none());
        assert!(path_tab_for("steam").is_none());
    }
}
