//! `SettingsPlatformsTab.svelte`: the platform chips and their search, the
//! startup card (default platform, personas) and the custom platforms card
//! of the descriptor folder.
//!
//! ```text
//! .settings-grid
//!   section.card.card-wide  .card-title-row (h3, search pill), .platforms
//!   section.card            h3, .field (startup select), the personas chip
//!   section.card.card-wide  h3, .descriptor-head, .descriptor-actions,
//!                           notice or error, loaded list, problems
//! ```
//!
//! Every platform of the registry is `implemented`: a chip is locked when
//! its platform does not run on this system and is not already on.

use accshift_app::i18n::{translate, Locale};
use egui::accesskit::{Role, Toggled};
use egui::{pos2, vec2, Context, Id, Rect, Sense, Ui, WidgetInfo, WidgetType};

use super::{CardSpec, Control, Ctx, Item as CardItem, View};
use crate::app::settings_panel::{SettingFlag, SettingsAction, SettingsPanel};
use crate::theme::Rgba;
use crate::ui::anim;
use crate::ui::components::controls::button::{self, ButtonProps, ButtonStyle, Transitions};
use crate::ui::components::controls::text_input::{self, FieldStyle, InputKind, TextInputProps};
use crate::ui::components::controls::{card, focus_ring, take_focus, T120, WHITE};
use crate::ui::components::icon;
use crate::ui::paint::{Border, BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextStyle, MONO_STACK};

/// The search field's id, for mod+f (`focusPlatformSearch`).
pub fn search_id() -> Id {
    Id::new("settings-platform-search")
}

/// `.platforms { gap: 8px }`.
const CHIP_GAP: f32 = 8.0;
/// `.descriptor-head`, `.descriptor-list { gap: 4px }`.
const LIST_GAP: f32 = 4.0;
/// `.descriptor-actions`, `.descriptor-row { gap: 8px }`.
const ROW_GAP: f32 = 8.0;
/// `.card-title-row { gap: 12px }`.
const TITLE_GAP: f32 = 12.0;
/// `.platform-search { width: 200px }`, border box.
const SEARCH_W: f32 = 200.0;
/// `.toggle`: 36x20.
const SWITCH: egui::Vec2 = vec2(36.0, 20.0);
/// `.platform-main { gap: 3px }`.
const MAIN_GAP: f32 = 3.0;
/// `#a855f7`, the personas chip's accent.
const PERSONAS_ACCENT: Rgba = Rgba::hex(0xa855f7, 1.0);

const ICON_SEARCH: &[u8] = concat!(
    "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"12\" height=\"12\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"white\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\">",
    "<circle cx=\"11\" cy=\"11\" r=\"8\"/><line x1=\"21\" y1=\"21\" x2=\"16.65\" y2=\"16.65\"/>",
    "</svg>"
)
.as_bytes();

/// The tab's own children of a card.
pub enum Item {
    /// `.card-title-row`: the h3 and the search pill.
    SearchRow(String),
    Chip(Chip),
    Para(String, Para),
    /// `.descriptor-actions`.
    Buttons(Vec<RowButton>),
    /// A loaded descriptor: its name, its id, the remove button.
    Descriptor {
        name: String,
        id: String,
        busy: bool,
    },
    /// A refused or skipped file and why.
    Problem {
        name: String,
        detail: String,
    },
}

/// `.platform-chip`: a `role="switch"` button.
pub struct Chip {
    /// The platform id (the `{#each}` key).
    pub key: String,
    pub label: String,
    /// `.platform-status`, empty for none.
    pub status: String,
    pub enabled: bool,
    /// `disabled` (`.disabled`): not selectable and not on.
    pub locked: bool,
    /// `--chip-accent`.
    pub accent: Rgba,
    pub action: SettingsAction,
}

/// The paragraph styles of the tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Para {
    /// `.no-results`: 12 px, padding 6 2.
    NoResults,
    /// `.descriptor-hint`: 12 px.
    Hint,
    /// `.descriptor-dir`: 11 px monospace, wraps anywhere.
    Dir,
    /// `.descriptor-detail`: 11 px.
    Detail,
    /// `.descriptor-detail.descriptor-failed`: 11 px in `--danger`.
    Failed,
    /// `h4.descriptor-subtitle`: 12 px bold, 4 px above.
    Subtitle,
}

/// One `.descriptor-button`.
pub struct RowButton {
    pub label: String,
    pub primary: bool,
    pub enabled: bool,
    pub action: SettingsAction,
}

fn para_style(v: &crate::app::vars::Vars, kind: Para) -> TextStyle {
    let style = match kind {
        Para::NoResults | Para::Hint => TextStyle::new(12.0, 400).color(v.fg_subtle.to_color32()),
        Para::Dir => TextStyle::new(11.0, 400)
            .family(MONO_STACK)
            .overflow_anywhere()
            .color(v.fg_muted.to_color32()),
        Para::Detail => TextStyle::new(11.0, 400).color(v.fg_subtle.to_color32()),
        Para::Failed => TextStyle::new(11.0, 400).color(v.danger.to_color32()),
        Para::Subtitle => TextStyle::new(12.0, 700).color(v.fg_muted.to_color32()),
    };
    style.line_height(LineHeight::Number(1.5))
}

/// Space a paragraph takes outside its text: padding or margin, as
/// (left and right, top, bottom).
fn para_box(kind: Para) -> (f32, f32, f32) {
    match kind {
        Para::NoResults => (2.0, 6.0, 6.0),
        Para::Subtitle => (0.0, 4.0, 0.0),
        _ => (0.0, 0.0, 0.0),
    }
}

/// `.descriptor-button`, or its `.primary` variant (the app's primary
/// button; hover is `filter: brightness(0.9)`, which no transition eases).
fn row_button_style(v: &crate::app::vars::Vars, primary: bool) -> ButtonStyle {
    let base = ButtonStyle {
        font: TextStyle::new(11.0, 400).line_height(LineHeight::Number(1.5)),
        padding: vec2(10.0, 5.0),
        border: 1.0,
        radius: 6.0,
        background: Rgba::TRANSPARENT,
        background_hover: v.bg_muted,
        border_color: v.border,
        border_hover: v.border,
        color: v.fg_muted,
        color_hover: v.fg,
        underline_hover: false,
        disabled_opacity: 0.5,
        transitions: Transitions {
            background: true,
            border: true,
            color: true,
        },
    };
    if !primary {
        return base;
    }
    let dim = |c: Rgba| Rgba::new(c.r * 0.9, c.g * 0.9, c.b * 0.9, c.a);
    ButtonStyle {
        font: TextStyle::new(11.0, 600).line_height(LineHeight::Number(1.5)),
        background: v.fg,
        background_hover: dim(v.fg),
        border_color: Rgba::TRANSPARENT,
        border_hover: Rgba::TRANSPARENT,
        color: v.bg_solid,
        color_hover: dim(v.bg_solid),
        transitions: Transitions::default(),
        ..base
    }
}

fn chip_label_style(v: &crate::app::vars::Vars) -> TextStyle {
    TextStyle::new(16.0, 400)
        .line_height(LineHeight::Number(1.5))
        .color(v.fg.to_color32())
}

fn chip_status_style(v: &crate::app::vars::Vars) -> TextStyle {
    TextStyle::new(10.0, 400)
        .line_height(LineHeight::Number(1.5))
        .color(v.fg_subtle.to_color32())
}

/// `.platform-chip`'s horizontal inset: border plus `padding: 10px 12px`.
fn chip_pad_x(ppp: f32) -> f32 {
    12.0 + snap::border_width(1.0, ppp)
}

fn chip_pad_y(ppp: f32) -> f32 {
    10.0 + snap::border_width(1.0, ppp)
}

/// The search pill's border-box height: an 18 px line, 6 px padding, the
/// 1 px border.
fn search_h(ppp: f32) -> f32 {
    30.0 + 2.0 * snap::border_width(1.0, ppp)
}

/// The cards of the tab.
pub(super) fn cards(panel: &SettingsPanel, view: &View, l: Locale) -> Vec<CardSpec> {
    let t = |k: &str| translate(l, k);
    let d = &panel.draft;
    let os = view.runtime_os;
    let query = panel.platform_search.trim().to_lowercase();
    let chips: Vec<CardItem> = view
        .platforms
        .iter()
        .filter(|p| {
            query.is_empty()
                || p.name.to_lowercase().contains(&query)
                || p.id.to_lowercase().contains(&query)
        })
        .map(|p| {
            let enabled = d.enabled_platforms.contains(&p.id);
            let selectable = p.supports(os);
            // Where a platform came from is worth saying: a descriptor the
            // user wrote is theirs to fix.
            let status = if !selectable {
                t("settings.platformUnsupportedOs")
            } else if p.user_provided {
                t("settings.platformUserProvided")
            } else {
                String::new()
            };
            CardItem::Platforms(Item::Chip(Chip {
                key: p.id.clone(),
                label: p.name.clone(),
                status,
                enabled,
                locked: !selectable && !enabled,
                accent: p.accent,
                action: SettingsAction::TogglePlatform(p.id.clone()),
            }))
        })
        .collect();
    let list = if chips.is_empty() {
        CardItem::Platforms(Item::Para(
            t("settings.platformSearchNoResults"),
            Para::NoResults,
        ))
    } else {
        CardItem::Stack(CHIP_GAP, chips)
    };
    let mut cards = vec![CardSpec {
        wide: true,
        items: vec![
            CardItem::Platforms(Item::SearchRow(t("settings.platforms"))),
            list,
        ],
    }];

    let suffix = t("settings.platformDisabledSuffix");
    let mut options = Vec::new();
    let mut disabled = Vec::new();
    let mut actions = Vec::new();
    for p in &view.platforms {
        let off = !d.enabled_platforms.contains(&p.id) || !p.supports(os);
        options.push(if off {
            format!("{} {suffix}", p.name)
        } else {
            p.name.clone()
        });
        disabled.push(off);
        actions.push(SettingsAction::DefaultPlatform(p.id.clone()));
    }
    let selected = view
        .platforms
        .iter()
        .position(|p| p.id == d.default_platform_id)
        .unwrap_or(0);
    cards.push(CardSpec {
        wide: false,
        items: vec![
            CardItem::Title(t("settings.startupAndExtras")),
            CardItem::Field {
                label: t("settings.defaultOnStartup"),
                hint: None,
                control: Control::Select {
                    options,
                    selected,
                    actions,
                    disabled,
                },
            },
            CardItem::Platforms(Item::Chip(Chip {
                key: "personas".into(),
                label: t("personas.title"),
                status: t("settings.personasHint"),
                enabled: d.personas_enabled,
                locked: false,
                accent: PERSONAS_ACCENT,
                action: SettingsAction::Toggle(SettingFlag::Personas),
            })),
        ],
    });

    let Some(report) = panel.user_platforms.as_ref().filter(|r| !r.dir.is_empty()) else {
        return cards;
    };
    let busy = panel.descriptor_busy;
    let mut items = vec![
        CardItem::Title(t("settings.customPlatforms")),
        CardItem::Stack(
            LIST_GAP,
            vec![
                CardItem::Platforms(Item::Para(t("settings.customPlatformsHint"), Para::Hint)),
                CardItem::Platforms(Item::Para(report.dir.clone(), Para::Dir)),
            ],
        ),
        CardItem::Platforms(Item::Buttons(vec![
            RowButton {
                label: t("descriptor.addFromFile"),
                primary: true,
                enabled: !busy,
                action: SettingsAction::PickDescriptor,
            },
            RowButton {
                label: t("descriptor.reload"),
                primary: false,
                enabled: !busy,
                action: SettingsAction::ReloadDescriptors,
            },
            RowButton {
                label: t("descriptor.openFolder"),
                primary: false,
                enabled: !busy,
                action: SettingsAction::OpenDescriptorsFolder,
            },
        ])),
    ];
    if !panel.descriptor_error.is_empty() {
        items.push(CardItem::Platforms(Item::Para(
            panel.descriptor_error.clone(),
            Para::Failed,
        )));
    } else if !panel.descriptor_notice.is_empty() {
        items.push(CardItem::Platforms(Item::Para(
            panel.descriptor_notice.clone(),
            Para::Detail,
        )));
    }
    if report.loaded.is_empty() {
        items.push(CardItem::Platforms(Item::Para(
            t("settings.customPlatformsNone"),
            Para::Hint,
        )));
    } else {
        items.push(CardItem::Stack(
            LIST_GAP,
            report
                .loaded
                .iter()
                .map(|descriptor| {
                    CardItem::Platforms(Item::Descriptor {
                        name: descriptor.name.clone(),
                        id: descriptor.id.clone(),
                        busy,
                    })
                })
                .collect(),
        ));
    }
    let problems: Vec<CardItem> = report
        .rejected
        .iter()
        .map(|entry| {
            let detail = if entry.field.is_empty() {
                entry.problem.clone()
            } else {
                format!("{}: {}", entry.field, entry.problem)
            };
            (entry.source.clone(), detail)
        })
        .chain(
            report
                .skipped
                .iter()
                .map(|entry| (entry.id.clone(), entry.reason.clone())),
        )
        .map(|(name, detail)| CardItem::Platforms(Item::Problem { name, detail }))
        .collect();
    if !problems.is_empty() {
        items.push(CardItem::Platforms(Item::Para(
            t("settings.customPlatformsProblems"),
            Para::Subtitle,
        )));
        items.push(CardItem::Stack(LIST_GAP, problems));
    }
    cards.push(CardSpec { wide: true, items });
    cards
}

/// The chip's text column: label, then the status under it.
struct ChipLayout {
    main_w: f32,
    label_h: f32,
    main_h: f32,
}

fn chip_layout(ctx: &Context, v: &crate::app::vars::Vars, chip: &Chip, width: f32) -> ChipLayout {
    let avail = (width - 2.0 * chip_pad_x(ctx.pixels_per_point()) - SWITCH.x).max(1.0);
    let wrap = |s: &str, style: &TextStyle| {
        text::layout(
            ctx,
            s,
            style,
            Rect::from_min_size(pos2(0.0, 0.0), vec2(avail, 0.0)),
        )
        .map_or(0.0, |l| l.height)
    };
    let measure = |s: &str, style: &TextStyle| text::measure(ctx, s, style).unwrap_or(0.0);
    let mut main_w = measure(&chip.label, &chip_label_style(v));
    let label_h = wrap(&chip.label, &chip_label_style(v));
    let mut main_h = label_h;
    if !chip.status.is_empty() {
        main_w = main_w.max(measure(&chip.status, &chip_status_style(v)));
        main_h += MAIN_GAP + wrap(&chip.status, &chip_status_style(v));
    }
    ChipLayout {
        main_w: main_w.min(avail),
        label_h,
        main_h,
    }
}

/// The wrapped rows of `.descriptor-actions`: each button's rect.
fn button_rects(ctx: &Context, c: &Ctx, buttons: &[RowButton], r: Rect) -> Vec<Rect> {
    let v = &c.view.vars;
    let mut out = Vec::with_capacity(buttons.len());
    let (mut x, mut y) = (r.left(), r.top());
    for b in buttons {
        let size = button::size(ctx, &b.label, &row_button_style(v, b.primary));
        if x > r.left() && x + size.x > r.right() + 0.01 {
            x = r.left();
            y += size.y + ROW_GAP;
        }
        out.push(Rect::from_min_size(pos2(x, y), size));
        x += size.x + ROW_GAP;
    }
    out
}

/// The border-box height of one of the tab's items `width` wide.
pub(super) fn height(ctx: &Context, c: &Ctx, item: &Item, width: f32) -> f32 {
    let ppp = ctx.pixels_per_point();
    let v = &c.view.vars;
    match item {
        Item::SearchRow(_) => search_h(ppp) + 8.0 + snap::border_width(1.0, ppp),
        Item::Chip(chip) => {
            chip_layout(ctx, v, chip, width).main_h.max(SWITCH.y) + 2.0 * chip_pad_y(ppp)
        }
        Item::Para(s, kind) => {
            let (x, top, bottom) = para_box(*kind);
            let style = para_style(v, *kind);
            let text_h = text::layout(
                ctx,
                s,
                &style,
                Rect::from_min_size(pos2(0.0, 0.0), vec2((width - 2.0 * x).max(1.0), 0.0)),
            )
            .map_or(0.0, |l| l.height);
            top + text_h + bottom
        }
        Item::Buttons(buttons) => {
            let rects = button_rects(
                ctx,
                c,
                buttons,
                Rect::from_min_size(pos2(0.0, 0.0), vec2(width, 0.0)),
            );
            rects.iter().map(|r| r.bottom()).fold(0.0, f32::max)
        }
        Item::Descriptor { .. } => {
            let style = row_button_style(v, false);
            button::size(ctx, "x", &style).y.max(18.0)
        }
        Item::Problem { .. } => 18.0,
    }
}

/// Paints one of the tab's items into `r`.
pub(super) fn paint(ui: &mut Ui, r: Rect, id: Id, c: &Ctx, item: &Item) -> Vec<SettingsAction> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = &c.view.vars;
    let mut out = Vec::new();
    match item {
        Item::SearchRow(title) => {
            card::paint_divider(ui, r, v);
            let style = card::title_style(v);
            let title_w = text::measure(&ctx, title, &style).unwrap_or(0.0);
            let pill_h = search_h(ppp);
            text::paint(
                ui.painter(),
                Rect::from_min_size(
                    pos2(r.left(), r.top() + snap::centre(pill_h, 18.0, ppp)),
                    vec2(title_w + 1.0, 18.0),
                ),
                title,
                &style.nowrap(false),
            );
            // `justify-content: space-between`; past the row's width the
            // pill keeps its min-content width (padding, border, the 12 px
            // icon, the 7 px gap, the field's intrinsic width) and sits after
            // the h3.
            let field = TextStyle::new(12.0, 400)
                .text_field_width(&ctx)
                .unwrap_or(145.0);
            let min_w = 22.0 + 2.0 * snap::border_width(1.0, ppp) + 19.0 + field;
            let pill_w = (r.width() - title_w - TITLE_GAP).clamp(min_w.min(SEARCH_W), SEARCH_W);
            let left = r.left() + (title_w + TITLE_GAP).max(r.width() - pill_w);
            let pill = Rect::from_min_size(pos2(left, r.top()), vec2(pill_w, pill_h));
            if let Some(text) = search_pill(ui, pill, c) {
                out.push(SettingsAction::PlatformSearch(text));
            }
        }
        Item::Chip(chip) => {
            if platform_chip(ui, r, c.scope.with(("chip", chip.key.as_str())), c, chip) {
                out.push(chip.action.clone());
            }
        }
        Item::Para(s, kind) => {
            let (x, top, _) = para_box(*kind);
            let rect = Rect::from_min_size(
                pos2(r.left() + x, r.top() + top),
                vec2(r.width() - 2.0 * x, 0.0),
            );
            text::paint(ui.painter(), rect, s, &para_style(v, *kind));
        }
        Item::Buttons(buttons) => {
            for (k, (b, rect)) in buttons
                .iter()
                .zip(button_rects(&ctx, c, buttons, r))
                .enumerate()
            {
                let style = row_button_style(v, b.primary);
                let props = ButtonProps {
                    label: &b.label,
                    aria_label: None,
                    style: &style,
                    enabled: b.enabled,
                    vars: v,
                    focus: c.focus,
                };
                if button::button(ui, rect, id.with(("button", k)), &props) {
                    out.push(b.action.clone());
                }
            }
        }
        Item::Descriptor {
            name,
            id: descriptor_id,
            busy,
        } => {
            let name_style = TextStyle::new(12.0, 400)
                .line_height(LineHeight::Number(1.5))
                .nowrap(false)
                .color(v.fg.to_color32());
            let code_style = TextStyle::new(11.0, 400)
                .family("monospace")
                .line_height(LineHeight::Number(1.5))
                .nowrap(false)
                .color(v.fg_subtle.to_color32());
            let name_w = text::measure(&ctx, name, &name_style).unwrap_or(0.0);
            text::paint(
                ui.painter(),
                Rect::from_min_size(
                    pos2(r.left(), r.top() + snap::centre(r.height(), 18.0, ppp)),
                    vec2(name_w + 1.0, 18.0),
                ),
                name,
                &name_style,
            );
            let code_w = text::measure(&ctx, descriptor_id, &code_style).unwrap_or(0.0);
            text::paint(
                ui.painter(),
                Rect::from_min_size(
                    pos2(
                        r.left() + name_w + ROW_GAP,
                        r.top() + snap::centre(r.height(), 16.5, ppp),
                    ),
                    vec2(code_w + 1.0, 16.5),
                ),
                descriptor_id,
                &code_style,
            );
            let label = translate(c.locale, "descriptor.remove");
            let style = row_button_style(v, false);
            let size = button::size(&ctx, &label, &style);
            let rect = Rect::from_min_size(pos2(r.right() - size.x, r.top()), size);
            let props = ButtonProps {
                label: &label,
                aria_label: None,
                style: &style,
                enabled: !busy,
                vars: v,
                focus: c.focus,
            };
            if button::button(ui, rect, id.with("remove"), &props) {
                out.push(SettingsAction::RemoveDescriptor {
                    id: descriptor_id.clone(),
                    name: name.clone(),
                });
            }
        }
        Item::Problem { name, detail } => {
            let name_style = TextStyle::new(12.0, 400)
                .line_height(LineHeight::Px(18.0))
                .nowrap(false)
                .color(v.fg.to_color32());
            let detail_style = TextStyle::new(11.0, 400)
                .line_height(LineHeight::Px(18.0))
                .nowrap(true)
                .color(v.fg_subtle.to_color32());
            let lead = text::measure(&ctx, &format!("{name} "), &name_style).unwrap_or(0.0);
            text::paint(ui.painter(), r, name, &name_style);
            text::paint(
                ui.painter(),
                Rect::from_min_max(pos2(r.left() + lead, r.top()), r.max),
                detail,
                &detail_style.nowrap(true),
            );
        }
    }
    out
}

/// `label.platform-search`: the icon and a borderless search field in a
/// pill. Returns the new text when it changed.
fn search_pill(ui: &mut Ui, pill: Rect, c: &Ctx) -> Option<String> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = &c.view.vars;
    let field_id = search_id();
    let line_w = snap::border_width(1.0, ppp);
    // The label forwards a press on its padding to the field.
    let hit = ui.interact(pill, field_id.with("pill"), Sense::click());
    if hit.is_pointer_button_down_on() && ui.input(|i| i.pointer.any_pressed()) {
        ui.memory_mut(|m| m.request_focus(field_id));
    }
    let focused = ui.memory(|m| m.has_focus(field_id));
    let hovered = hit.hovered() || ui.rect_contains_pointer(pill);
    let bg = if hovered {
        v.bg_card.mix(0.84, WHITE)
    } else {
        v.bg_card.mix(0.88, WHITE)
    };
    let border = if focused {
        v.fg_muted.mix(0.45, v.border)
    } else {
        Rgba::TRANSPARENT
    };
    let fg = if focused { v.fg_muted } else { v.fg_subtle };
    let bg = anim::transition(&ctx, field_id.with("bg"), bg, &T120);
    let border = anim::transition(&ctx, field_id.with("border"), border, &T120);
    let fg = anim::transition(&ctx, field_id.with("fg"), fg, &T120);
    let boxed = snap::rect(pill, ppp);
    BoxStyle {
        background: bg,
        radius: Corners::all(pill.height() / 2.0),
        border: Border::all(line_w, border),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), boxed);
    let inner_top = pill.top() + line_w + 6.0;
    let icon_rect = Rect::from_min_size(
        pos2(
            pill.left() + line_w + 11.0,
            inner_top + snap::centre(18.0, 12.0, ppp),
        ),
        vec2(12.0, 12.0),
    );
    icon::paint(ui.painter(), ICON_SEARCH, icon_rect, fg, 1.0);
    let field = Rect::from_min_max(
        pos2(icon_rect.right() + 7.0, inner_top),
        pos2(pill.right() - line_w - 11.0, inner_top + 18.0),
    );
    let style = FieldStyle {
        font: TextStyle::new(12.0, 400).line_height(LineHeight::Px(18.0)),
        line: 18.0,
        padding: egui::Vec2::ZERO,
        border: 0.0,
        radius: 0.0,
        background: Rgba::TRANSPARENT,
        border_color: Rgba::TRANSPARENT,
        border_focus: Rgba::TRANSPARENT,
        placeholder: v.fg_subtle,
        // The field keeps `line-height: normal` inside its 18 px line: the
        // caret spans Segoe UI's ascent and descent, 16 px, 1 px wide.
        caret_height: 16.0,
        caret_scaled: true,
        border_transition: false,
    };
    let placeholder = translate(c.locale, "settings.platformSearchPlaceholder");
    let mut value = c.panel.platform_search.clone();
    let props = TextInputProps {
        style: &style,
        kind: InputKind::Search,
        placeholder: &placeholder,
        label: &placeholder,
        max_len: None,
        enabled: true,
        vars: v,
        dark: c.view.dark,
    };
    let ev = text_input::text_input(ui, field, field_id, &mut value, &props);
    ev.changed.then_some(value)
}

/// `.platform-chip`: name and status on the left, the switch on the right.
/// True when it was activated.
fn platform_chip(ui: &mut Ui, rect: Rect, id: Id, c: &Ctx, chip: &Chip) -> bool {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = &c.view.vars;
    let active = !chip.locked;
    let response = ui.interact(
        rect,
        id,
        if active {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    response.widget_info(|| {
        WidgetInfo::selected(WidgetType::Checkbox, active, chip.enabled, &chip.label)
    });
    ctx.accesskit_node_builder(id, |node| {
        node.set_role(Role::Switch);
        node.set_toggled(if chip.enabled {
            Toggled::True
        } else {
            Toggled::False
        });
    });
    let focused = active && take_focus(ui, &response);
    let hovered = active && response.hovered();
    let (border, bg) = if hovered {
        (chip.accent.mix(0.55, v.border), v.bg_card.mix(0.84, WHITE))
    } else {
        (v.border, v.bg_card.mix(0.88, WHITE))
    };
    let border = anim::transition(&ctx, id.with("border"), border, &T120);
    let bg = anim::transition(&ctx, id.with("bg"), bg, &T120);
    let mut painter = ui.painter().clone();
    if chip.locked {
        painter.multiply_opacity(0.55);
    }
    let boxed = snap::rect(rect, ppp);
    BoxStyle {
        background: bg,
        radius: Corners::all(10.0),
        border: Border::all(snap::border_width(1.0, ppp), border),
        ..BoxStyle::default()
    }
    .paint(&painter, boxed);

    let l = chip_layout(&ctx, v, chip, rect.width());
    let inner_h = rect.height() - 2.0 * chip_pad_y(ppp);
    let top = rect.top() + chip_pad_y(ppp);
    let main = Rect::from_min_size(
        pos2(
            rect.left() + chip_pad_x(ppp),
            top + snap::centre(inner_h, l.main_h, ppp),
        ),
        vec2(l.main_w, l.main_h),
    );
    text::paint(&painter, main, &chip.label, &chip_label_style(v));
    if !chip.status.is_empty() {
        let status = Rect::from_min_size(
            pos2(main.left(), main.top() + l.label_h + MAIN_GAP),
            vec2(l.main_w, 0.0),
        );
        text::paint(&painter, status, &chip.status, &chip_status_style(v));
    }
    // `.toggle`: `--bg-elevated` or the accent, the knob sliding 16 px.
    let switch = Rect::from_min_size(
        pos2(
            rect.right() - chip_pad_x(ppp) - SWITCH.x,
            top + snap::centre(inner_h, SWITCH.y, ppp),
        ),
        SWITCH,
    );
    let track = if chip.enabled {
        chip.accent
    } else {
        v.bg_elevated
    };
    let track = anim::transition(&ctx, id.with("track"), track, &T120);
    BoxStyle {
        background: track,
        radius: Corners::all(10.0),
        ..BoxStyle::default()
    }
    .paint(&painter, snap::rect(switch, ppp));
    let shift = anim::transition(
        &ctx,
        id.with("knob"),
        if chip.enabled { 16.0 } else { 0.0 },
        &T120,
    );
    let knob = Rect::from_min_size(switch.min + vec2(2.0, 2.0), vec2(16.0, 16.0));
    BoxStyle {
        background: WHITE,
        radius: Corners::all(8.0),
        ..BoxStyle::default()
    }
    .paint(&painter, snap::translated(knob, vec2(shift, 0.0), ppp));

    focus_ring(ui, boxed, 10.0, 1.0, v, c.focus, focused);
    let keyed =
        focused && ui.input(|i| i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter));
    active && (response.clicked() || keyed)
}
