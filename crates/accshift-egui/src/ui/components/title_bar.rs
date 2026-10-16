//! `src/lib/shared/components/TitleBar.svelte`: the custom title bar.
//!
//! Every metric comes from the component's CSS. Tailwind's preflight makes
//! every box `border-box`, so the bar is 36 px with its 1 px bottom border
//! inside (35 px of content) on Windows and Linux, 32 with 31 of content on
//! macOS: the reference draws that border on row 35.
//!
//! Windows and Linux: action buttons on the left from x = 10, 6 apart; the
//! platform tabs centred on the bar; the 46 px caption strip flush right at
//! full content height. macOS: 72 px of padding for the traffic lights, the
//! actions on the right with 8 px of padding, an opaque `--bg-solid` fill,
//! no caption strip.
//!
//! Placement follows [`crate::ui::snap`] at a real device scale factor:
//! boxes snap edge by edge, the flex-centred buttons and icons centre in
//! layout units first. `.tabs` is a scroll container with real overflow (its
//! tooltips hang below it), which Chromium composites: the whole strip
//! offset, `translate(-50%, -50%)` included, lands on a whole device pixel.

use egui::accesskit::Role;
use egui::{
    pos2, vec2, Color32, FontId, Id, LayerId, Order, PointerButton, Rect, Response, Sense, Ui,
    WidgetInfo, WidgetType,
};

use accshift_app::i18n::{translate, Locale};

use super::icon;
use super::onboarding;
use super::platform_icons;
use crate::app::platforms::{PlatformDef, RuntimeOs};
use crate::app::vars::Vars;
use crate::app::Action;
use crate::icons;
use crate::theme::Rgba;
use crate::ui::anim::{self, Timing, Transition};
use crate::ui::paint::{Border, BoxShadow, BoxStyle, Corners, Transform};
use crate::ui::snap;

/// `.titlebar { height: 36px }`, its `border-bottom: 1px` included.
pub const HEIGHT: f32 = 36.0;
/// `.titlebar.macos { height: 32px }`, border included.
pub const HEIGHT_MACOS: f32 = 32.0;

/// The bar's outer height for an OS.
pub fn height(os: RuntimeOs) -> f32 {
    if os == RuntimeOs::Macos {
        HEIGHT_MACOS
    } else {
        HEIGHT
    }
}

/// `transition: all 120ms ease-out` (`.btn`, `.tab`, `.update-btn`).
const T120: Transition = Transition::new(0.12, Timing::EASE_OUT);
/// `.tab-wrap:hover .tab-tooltip { transition-delay: 300ms }`.
const TOOLTIP_IN: Transition = Transition::new(0.12, Timing::EASE_OUT).with_delay(0.3);

/// The update call to action (`updateCtaLabel` and friends).
#[derive(Clone, Debug)]
pub struct UpdateCta<'a> {
    pub label: &'a str,
    pub title: &'a str,
    pub disabled: bool,
}

/// `TitleBar.svelte`'s props, minus the callbacks: what a click means comes
/// back as an [`Action`].
#[derive(Clone, Debug)]
pub struct TitleBarProps<'a> {
    pub vars: &'a Vars,
    pub locale: Locale,
    pub runtime_os: RuntimeOs,
    /// Onboarding outside its demo step: drag and caption buttons only.
    pub hide_actions: bool,
    pub personas_visible: bool,
    pub personas_active: bool,
    pub active_tab: &'a str,
    pub enabled_platforms: &'a [PlatformDef],
    /// Enabled platforms this OS cannot run.
    pub unavailable: &'a [String],
    pub can_refresh: bool,
    pub can_add_account: bool,
    pub show_bulk_edit: bool,
    pub bulk_edit_active: bool,
    pub show_settings: bool,
    pub update_cta: Option<UpdateCta<'a>>,
    /// The window is maximized: the restore glyph replaces maximize.
    pub maximized: bool,
    /// Windows 11 reports the pointer over the snap-layout maximize button
    /// (`titlebar:maximize-hover`); `:hover` never fires there. Always false
    /// until the window-shell job answers `HTMAXBUTTON`.
    pub snap_hover: bool,
    /// Keyboard focus is showing (the last focus move came from Tab), so
    /// `:focus-visible` rings draw.
    pub focus_visible: bool,
}

/// Draws the bar into `rect` (the bar's border box, its top-left at the
/// window's) and returns what the user asked for this frame.
pub fn title_bar(ui: &mut Ui, rect: Rect, props: &TitleBarProps) -> Vec<Action> {
    let mut actions = Vec::new();
    let v = props.vars;
    let macos = props.runtime_os == RuntimeOs::Macos;
    // Chromium lays out in device pixels and snaps border widths down to
    // whole ones (1 CSS px is 1 device px at 150 %), so the content box
    // keeps the rest.
    let ppp = ui.ctx().pixels_per_point();
    let rect = snap::rect(rect, ppp);
    let border = snap::border_width(1.0, ppp);
    let content_h = rect.height() - border;
    let painter = ui.painter().clone();

    if macos {
        // Opaque fill under the whole border box, then the border on top.
        fill(&painter, rect, 0.0, v.bg_solid);
    }
    fill(
        &painter,
        Rect::from_min_max(pos2(rect.left(), rect.bottom() - border), rect.max),
        0.0,
        v.bg_card,
    );

    // The bar itself: mousedown outside a button drags the window, a double
    // click toggles maximize. Registered first so every button sits on top.
    let bar = ui.interact(rect, Id::new("titlebar"), Sense::click_and_drag());
    if bar.drag_started_by(PointerButton::Primary) {
        actions.push(Action::StartDrag);
    }
    if bar.double_clicked() {
        actions.push(Action::ToggleMaximize);
    }

    let show_actions = !props.hide_actions;
    if show_actions {
        let count = 3 + usize::from(props.show_bulk_edit);
        // 26 px buttons, 6 apart.
        let row_w = count as f32 * 26.0 + (count as f32 - 1.0) * 6.0;
        let x0 = if macos {
            // `.right { padding-right: 8px }`, the update button after them.
            let cta_w = props
                .update_cta
                .as_ref()
                .map(|cta| update_width(ui, cta.label) + 6.0)
                .unwrap_or(0.0);
            rect.right() - 8.0 - cta_w - row_w
        } else {
            rect.left() + 10.0
        };
        let y = rect.top() + snap::centre(content_h, 26.0, ppp);
        let buttons: [(&'static [u8], &str, bool, Action); 4] = [
            (
                icons::REFRESH,
                "titlebar.refresh",
                props.can_refresh,
                Action::Refresh,
            ),
            (
                icons::PLUS,
                "titlebar.addAccount",
                props.can_add_account,
                Action::AddAccount,
            ),
            (
                icons::SETTINGS,
                "titlebar.settings",
                true,
                Action::OpenSettings,
            ),
            (icons::BULK_EDIT, "bulkEdit.title", true, Action::BulkEdit),
        ];
        for (index, (svg, key, enabled, action)) in buttons.into_iter().take(count).enumerate() {
            let r = Rect::from_min_size(pos2(x0 + index as f32 * 32.0, y), vec2(26.0, 26.0));
            let label = translate(props.locale, key);
            let active_mode = index == 3 && props.bulk_edit_active;
            if index == 1 {
                // `data-tour="add-account"`.
                onboarding::record_target(ui.ctx(), |t| {
                    t.add_account.get_or_insert(r);
                });
            }
            if action_button(ui, r, svg, &label, enabled, active_mode, props) {
                actions.push(action);
            }
        }
    }

    if show_actions && (props.enabled_platforms.len() > 1 || props.personas_visible) {
        tabs(ui, rect, content_h, props, &mut actions);
    }

    if !macos {
        // Caption strip, flush right, full content height.
        let strip_left = rect.right() - 3.0 * 46.0;
        if let (true, Some(cta)) = (show_actions, &props.update_cta) {
            // `.right { gap: 6px }` and `.win-controls { margin-left: 4px }`.
            let w = update_width(ui, cta.label);
            let r = Rect::from_min_size(
                pos2(
                    strip_left - 4.0 - 6.0 - w,
                    rect.top() + snap::centre(content_h, 24.0, ppp),
                ),
                vec2(w, 24.0),
            );
            if update_button(ui, r, cta, props) {
                actions.push(Action::ApplyUpdate);
            }
        }
        let max_key = if props.maximized {
            "titlebar.restore"
        } else {
            "titlebar.maximize"
        };
        let caption: [(&'static [u8], &str, Action); 3] = [
            (icons::MINIMIZE, "titlebar.minimize", Action::Minimize),
            (
                if props.maximized {
                    icons::RESTORE
                } else {
                    icons::MAXIMIZE
                },
                max_key,
                Action::ToggleMaximize,
            ),
            (icons::CLOSE, "titlebar.close", Action::Close),
        ];
        for (index, (svg, key, action)) in caption.into_iter().enumerate() {
            let r = Rect::from_min_size(
                pos2(strip_left + index as f32 * 46.0, rect.top()),
                vec2(46.0, content_h),
            );
            let label = translate(props.locale, key);
            let (clicked, response) = caption_button(ui, r, svg, &label, index, props);
            if clicked {
                actions.push(action);
            }
            if index == 1 {
                // Windows 11 Snap Layouts: the window must answer HTMAXBUTTON
                // over this rect (`set_maximize_button_rect` in Tauri).
                actions.push(Action::SetMaximizeButtonRect(Some(response.rect)));
            }
        }
    } else if show_actions {
        if let Some(cta) = &props.update_cta {
            let w = update_width(ui, cta.label);
            let r = Rect::from_min_size(
                pos2(
                    rect.right() - 8.0 - w,
                    rect.top() + snap::centre(content_h, 24.0, ppp),
                ),
                vec2(w, 24.0),
            );
            if update_button(ui, r, cta, props) {
                actions.push(Action::ApplyUpdate);
            }
        }
    }
    if macos {
        actions.push(Action::SetMaximizeButtonRect(None));
    }
    actions
}

/// Chromium focuses a button on mousedown; any other press blurs it. The
/// focused button stays in egui memory under this id.
fn focus_id() -> Id {
    Id::new("titlebar::focused")
}

fn focused(ui: &Ui) -> Option<Id> {
    ui.ctx().data(|d| d.get_temp::<Id>(focus_id()))
}

/// Track mousedown focus for `response`; returns whether it holds focus.
fn track_focus(ui: &Ui, response: &Response, enabled: bool) -> bool {
    let pressed_here =
        enabled && response.is_pointer_button_down_on() && ui.input(|i| i.pointer.any_pressed());
    if pressed_here {
        ui.ctx()
            .data_mut(|d| d.insert_temp(focus_id(), response.id));
    } else if ui.input(|i| i.pointer.any_pressed())
        && focused(ui) == Some(response.id)
        && !response.contains_pointer()
    {
        ui.ctx().data_mut(|d| d.remove::<Id>(focus_id()));
    }
    response.has_focus() || focused(ui) == Some(response.id)
}

/// The UA `:focus-visible` ring of a borderless button, keyboard focus
/// only.
fn focus_ring(ui: &Ui, rect: Rect, radius: f32, props: &TitleBarProps, focused: bool) {
    if focused && props.focus_visible {
        super::controls::paint_ua_focus_ring(ui.painter(), rect, radius, 0.0, props.vars.dark);
    }
}

/// `.btn`: 26x26, radius 4. Returns whether it was clicked.
fn action_button(
    ui: &mut Ui,
    rect: Rect,
    svg: &'static [u8],
    label: &str,
    enabled: bool,
    active_mode: bool,
    props: &TitleBarProps,
) -> bool {
    let v = props.vars;
    let id = Id::new(("titlebar::btn", label));
    let response = ui.interact(
        rect,
        id,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, active_mode, label));
    let has_focus = track_focus(ui, &response, enabled);
    let hovered = enabled && response.hovered();
    let pressed = enabled && response.is_pointer_button_down_on();
    // `.btn.active-mode` comes after `:hover` and wins over it.
    let (bg, fg) = if active_mode {
        (v.accent.mix(0.24, v.bg_muted), v.accent_text)
    } else if hovered {
        (v.bg_muted, v.fg)
    } else {
        (Rgba::TRANSPARENT, v.fg_muted)
    };
    let ctx = ui.ctx().clone();
    let bg = anim::transition(&ctx, id.with("bg"), bg, &T120);
    let fg = anim::transition(&ctx, id.with("fg"), fg, &T120);
    let scale = anim::transition(
        &ctx,
        id.with("scale"),
        if pressed { 0.92 } else { 1.0 },
        &T120,
    );
    let opacity = anim::transition(&ctx, id.with("op"), if enabled { 1.0 } else { 0.45 }, &T120);
    let mut painter = ui.painter().clone();
    painter.multiply_opacity(opacity);
    // `transform: scale()` on the snapped box; the 14 px svg is flex-centred
    // in it.
    BoxStyle {
        background: bg,
        radius: Corners::all(4.0),
        ..BoxStyle::default()
    }
    .paint_transformed(&painter, rect, Transform::scale(scale));
    icon::paint(&painter, svg, centred(rect, 14.0, ui), fg, scale);
    focus_ring(ui, rect, 4.0, props, has_focus);
    enabled && response.clicked()
}

/// `.win-btn`: 46 px wide, full height, no radius. Index 2 is `.close`.
fn caption_button(
    ui: &mut Ui,
    rect: Rect,
    svg: &'static [u8],
    label: &str,
    index: usize,
    props: &TitleBarProps,
) -> (bool, Response) {
    let v = props.vars;
    let id = Id::new(("titlebar::caption", index));
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    let has_focus = track_focus(ui, &response, true);
    let hovered = response.hovered() || (index == 1 && props.snap_hover);
    let pressed = response.is_pointer_button_down_on();
    let close = index == 2;
    let white = Rgba::hex(0xffffff, 1.0);
    let (bg, fg) = match (close, hovered, pressed) {
        (true, _, true) => (v.danger.mix(0.78, Rgba::hex(0x000000, 1.0)), white),
        (true, true, false) => (v.danger, white),
        (false, _, true) => (v.bg_elevated, if hovered { v.fg } else { v.fg_muted }),
        (false, true, false) => (v.bg_muted, v.fg),
        (_, false, false) => (Rgba::TRANSPARENT, v.fg_muted),
    };
    let ctx = ui.ctx().clone();
    let bg = anim::transition(&ctx, id.with("bg"), bg, &T120);
    let fg = anim::transition(&ctx, id.with("fg"), fg, &T120);
    let scale = anim::transition(
        &ctx,
        id.with("scale"),
        if pressed { 0.9 } else { 1.0 },
        &T120,
    );
    let painter = ui.painter();
    fill(painter, rect, 0.0, bg);
    icon::paint(painter, svg, centred(rect, 12.0, ui), fg, scale);
    focus_ring(ui, rect, 0.0, props, has_focus);
    (response.clicked(), response)
}

/// `.tabs`: centred on the bar's padding box, 32x28 tabs 2 apart.
fn tabs(ui: &mut Ui, bar: Rect, content_h: f32, props: &TitleBarProps, actions: &mut Vec<Action>) {
    struct Tab<'a> {
        key: &'a str,
        name: String,
        svg: Option<&'static [u8]>,
        accent: Rgba,
        active: bool,
        unavailable: bool,
    }
    let mut list = Vec::new();
    if props.personas_visible {
        list.push(Tab {
            key: "__personas__",
            name: translate(props.locale, "titlebar.personas"),
            svg: Some(platform_icons::PERSONAS),
            accent: Rgba::hex(platform_icons::PERSONAS_ACCENT, 1.0),
            active: props.personas_active,
            unavailable: false,
        });
    }
    for platform in props.enabled_platforms {
        list.push(Tab {
            key: &platform.id,
            name: platform.name.clone(),
            svg: platform_icons::platform(&platform.id),
            accent: platform.accent,
            active: !props.show_settings
                && !props.personas_active
                && props.active_tab == platform.id,
            unavailable: props.unavailable.contains(&platform.id),
        });
    }
    let n = list.len() as f32;
    let total = n * 32.0 + (n - 1.0).max(0.0) * 2.0;
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    // `left: 50%; top: 50%; transform: translate(-50%, -50%)` in the bar's
    // padding box, then the composited layer's offset rounded.
    let laid = Rect::from_min_size(
        pos2(
            bar.left() + bar.width() / 2.0 - total / 2.0,
            bar.top() + content_h / 2.0 - 14.0,
        ),
        vec2(total, 28.0),
    );
    // `data-tour="platforms"`: the layout box, unsnapped.
    onboarding::record_target(&ctx, |t| {
        t.platforms.get_or_insert(laid);
    });
    let strip = snap::composited(laid, ppp);
    let (x0, y) = (strip.left(), strip.top());
    let v = props.vars;
    for (index, tab) in list.iter().enumerate() {
        let rect = Rect::from_min_size(pos2(x0 + index as f32 * 34.0, y), vec2(32.0, 28.0));
        let id = Id::new(("titlebar::tab", tab.key));
        let enabled = !tab.unavailable;
        let response = ui.interact(
            rect,
            id,
            if enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        response.widget_info(|| {
            WidgetInfo::selected(WidgetType::SelectableLabel, enabled, tab.active, &tab.name)
        });
        ctx.accesskit_node_builder(id, |node| {
            node.set_role(Role::Tab);
            node.set_selected(tab.active);
        });
        let has_focus = track_focus(ui, &response, enabled);
        let hovered = enabled && response.hovered();
        // `.tab.active` keeps the card fill and the inline accent colour beats
        // `:hover`'s; `.tab.disabled:hover` (more specific) clears both.
        let disabled_hover = !enabled && response.hovered();
        let bg = if (hovered || tab.active) && !disabled_hover {
            v.bg_card
        } else {
            Rgba::TRANSPARENT
        };
        let fg = if tab.active {
            tab.accent
        } else if hovered {
            v.fg_muted
        } else {
            v.fg_subtle
        };
        let bg = anim::transition(&ctx, id.with("bg"), bg, &T120);
        let fg = anim::transition(&ctx, id.with("fg"), fg, &T120);
        let opacity =
            anim::transition(&ctx, id.with("op"), if enabled { 1.0 } else { 0.45 }, &T120);
        let mut painter = ui.painter().clone();
        painter.multiply_opacity(opacity);
        BoxStyle {
            background: bg,
            radius: Corners::all(4.0),
            ..BoxStyle::default()
        }
        .paint(&painter, rect);
        match tab.svg {
            Some(svg) => icon::paint(&painter, svg, centred(rect, 16.0, ui), fg, 1.0),
            None => {
                // `.tab-text`: the first two characters, 11px 600.
                let short: String = tab.name.chars().take(2).collect();
                let galley =
                    painter.layout_no_wrap(short, FontId::new(11.0, semibold()), fg.to_color32());
                let at = rect.center() - galley.size() / 2.0;
                painter.galley(at, galley, fg.to_color32());
            }
        }
        focus_ring(ui, rect, 4.0, props, has_focus);
        if enabled && response.clicked() {
            actions.push(if tab.key == "__personas__" {
                Action::OpenPersonas
            } else {
                Action::TabChange(tab.key.to_string())
            });
        }
        let label = if tab.unavailable {
            format!(
                "{} ({})",
                tab.name,
                translate(props.locale, "settings.platformUnsupportedOs")
            )
        } else {
            tab.name.clone()
        };
        // `.tab-wrap:hover` covers the disabled button too.
        tooltip(
            ui,
            rect,
            strip,
            &label,
            response.hovered() || has_focus,
            props,
        );
    }
}

/// `.tab-tooltip`: under the tab, centred, fading in after 300 ms, clipped
/// to `strip`, the `.tabs` box. Its `overflow-x: auto` makes `overflow-y`
/// auto as well, so the tooltip below the tabs never shows: the reference
/// draws none, and neither does this until the CSS changes.
fn tooltip(ui: &Ui, tab: Rect, strip: Rect, label: &str, shown: bool, props: &TitleBarProps) {
    let ctx = ui.ctx();
    let id = Id::new(("titlebar::tooltip", label));
    let spec = if shown { TOOLTIP_IN } else { T120 };
    let opacity = anim::transition(ctx, id, if shown { 1.0f32 } else { 0.0 }, &spec);
    if opacity <= 0.0 {
        return;
    }
    let v = props.vars;
    // 11px, weight 500 (Regular until the text port lands its faces).
    let font = FontId::proportional(11.0);
    let galley = ctx.fonts_mut(|f| f.layout_no_wrap(label.to_owned(), font, v.fg.to_color32()));
    let border = v.border_px();
    // `line-height: normal` of the 11 px UI face, until `ui::text` owns it.
    let line = 15.0;
    let size = vec2(
        galley.size().x + 16.0 + 2.0 * border,
        line + 8.0 + 2.0 * border,
    );
    // `left: 50%; top: 100%; transform: translateX(-50%) translateY(4px)`.
    let rect = Rect::from_min_size(
        pos2(tab.center().x - size.x / 2.0, tab.bottom() + 4.0),
        size,
    );
    let mut painter = ctx
        .layer_painter(LayerId::new(Order::Foreground, Id::new("titlebar::tips")))
        .with_clip_rect(strip);
    painter.multiply_opacity(opacity);
    BoxStyle {
        background: v.bg_overlay,
        radius: Corners::all(4.0),
        border: Border::all(border, v.border),
        shadows: vec![BoxShadow {
            offset: vec2(0.0, 4.0),
            blur: 12.0,
            spread: 0.0,
            color: Rgba::new(0.0, 0.0, 0.0, 0.3),
            inset: false,
        }],
        ..BoxStyle::default()
    }
    .paint(&painter, rect);
    let text_top = rect.top() + border + 4.0 + (line - galley.size().y) / 2.0;
    painter.galley(
        pos2(rect.left() + border + 8.0, text_top),
        galley,
        Color32::WHITE,
    );
}

/// A plain rectangle (`background` with `border-radius`) on the device grid.
fn fill(painter: &egui::Painter, rect: Rect, radius: f32, color: Rgba) {
    BoxStyle {
        background: color,
        radius: Corners::all(radius),
        ..BoxStyle::default()
    }
    .paint(painter, rect);
}

/// A `size` square flex-centred in `rect` (`align-items` and
/// `justify-content: center`), offsets in layout units; the box snaps where
/// it paints.
fn centred(rect: Rect, size: f32, ui: &Ui) -> Rect {
    let ppp = ui.ctx().pixels_per_point();
    let min = rect.min
        + vec2(
            snap::centre(rect.width(), size, ppp),
            snap::centre(rect.height(), size, ppp),
        );
    Rect::from_min_size(min, vec2(size, size))
}

/// `.update-btn` width: text plus `padding: 0 10px` and the 1 px border.
fn update_width(ui: &Ui, label: &str) -> f32 {
    let font = FontId::new(11.0, semibold());
    let w = ui.ctx().fonts_mut(|f| {
        f.layout_no_wrap(label.to_owned(), font, Color32::WHITE)
            .size()
            .x
    });
    w + 20.0 + 2.0
}

/// `.update-btn`: pill, 24 px high, 11px 600.
fn update_button(ui: &mut Ui, rect: Rect, cta: &UpdateCta, props: &TitleBarProps) -> bool {
    let v = props.vars;
    let id = Id::new("titlebar::update");
    let enabled = !cta.disabled;
    let response = ui.interact(
        rect,
        id,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, cta.label));
    let hovered = enabled && response.hovered();
    let ctx = ui.ctx().clone();
    let bg = if hovered {
        v.bg_card.mix(0.70, v.fg)
    } else {
        v.bg_card.mix(0.92, v.fg)
    };
    let bg = anim::transition(&ctx, id.with("bg"), bg, &T120);
    let opacity = anim::transition(&ctx, id.with("op"), if enabled { 1.0 } else { 0.6 }, &T120);
    let mut painter = ui.painter().clone();
    painter.multiply_opacity(opacity);
    BoxStyle {
        background: bg,
        radius: Corners::all(999.0),
        border: Border::all(1.0, v.fg.mix(0.25, v.border)),
        ..BoxStyle::default()
    }
    .paint(&painter, rect);
    let font = FontId::new(11.0, semibold());
    let galley = painter.layout_no_wrap(cta.label.to_owned(), font, v.fg.to_color32());
    painter.galley(
        rect.center() - galley.size() / 2.0,
        galley,
        v.fg.to_color32(),
    );
    enabled && response.clicked()
}

/// The 600 weight face the font setup registers (`font-weight: 600`).
fn semibold() -> egui::FontFamily {
    egui::FontFamily::Name("semibold".into())
}
