//! The select step: the bar along the window's bottom and its colour
//! popover.

use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim::{self, Keyframes};
use crate::ui::components::controls::take_focus;
use crate::ui::paint::{
    Border, BoxShadow, BoxStyle, ColorStop, Corners, Gradient, Length, LinearDirection,
    LinearGradient, Sides,
};
use crate::ui::text::{self, LineHeight, TextStyle};
use crate::ui::{css, snap};
use accshift_app::bulk_edit::BulkEditBar;
use accshift_app::menu::account::color_label_key;
use accshift_app::stores::ACCOUNT_CARD_COLOR_PRESETS;
use egui::emath::TSTransform;
use egui::{pos2, vec2, Id, Rect, Sense, Ui, UiBuilder, Vec2};

use super::button::{button, hovered, primary_look, secondary_look, tool_look, Btn};
use super::{
    t, BulkEvent, BulkProps, BAR_PAD, BULK_ID, COVER_ID, FILL, GAP, POPOVER_OFFSET, POPOVER_PAD,
    SLIDE_UP, SWATCH, SWATCH_GAP,
};

/// `.swatch-fill.default`.
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

/// The select step: the bar and, when open, the colour popover. Returns
/// what the user did; the bar's and popover's rects go to [`COVER_ID`].
pub fn bar(ui: &mut Ui, props: &BulkProps) -> Vec<BulkEvent> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let id = Id::unique(BULK_ID).with(("bar", props.mount));
    let bw = snap::border_width(v.border_px(), ppp);
    let locale = props.locale;
    let mut events = Vec::new();

    let tool = Btn::tool();
    let secondary = Btn::action(400);
    let primary = Btn::action(600);
    let tool_h = tool.height(&ctx, bw);
    let action_h = secondary.height(&ctx, bw);
    let content_h = tool_h.max(action_h);
    let bar_h = bw + 2.0 * BAR_PAD.y + content_h;
    let rect = Rect::from_min_max(
        pos2(props.viewport.left(), props.viewport.bottom() - bar_h),
        props.viewport.max,
    );
    let content_top = rect.top() + bw + BAR_PAD.y;
    let left_top = content_top + snap::centre(content_h, tool_h, ppp);
    let right_top = content_top + snap::centre(content_h, action_h, ppp);

    // `.bar-left`.
    let labels = [
        t(locale, "bulkEdit.selectAll"),
        t(locale, "bulkEdit.deselectAll"),
        t(locale, "bulkEdit.copyUrls"),
        t(locale, "bulkEdit.cardColor"),
    ];
    let mut x = rect.left() + BAR_PAD.x;
    let tools: Vec<Rect> = labels
        .iter()
        .map(|label| {
            let w = tool.width(&ctx, label, bw);
            let r = Rect::from_min_size(pos2(x, left_top), vec2(w, tool_h));
            x += w + GAP;
            r
        })
        .collect();
    let count_style = TextStyle::new(12.0, 400)
        .line_height(LineHeight::Number(1.5))
        .color(v.fg_muted.to_color32());
    let count_line = count_style.line_box(&ctx).map_or(18.0, |l| l.height);
    let count = BulkEditBar::selected_label(props.selected).resolve(locale);
    let count_rect = Rect::from_min_size(
        pos2(x, left_top + snap::centre(tool_h, count_line, ppp)),
        vec2(
            snap::content_width(
                text::measure(&ctx, &count, &count_style).unwrap_or(0.0),
                ppp,
            ),
            count_line,
        ),
    );

    // `.bar-right`.
    let cancel = t(locale, "common.cancel");
    let next = t(locale, "bulkEdit.next");
    let cancel_w = secondary.width(&ctx, &cancel, bw);
    let next_w = primary.width(&ctx, &next, bw);
    let right = rect.right() - BAR_PAD.x;
    let next_rect = Rect::from_min_size(pos2(right - next_w, right_top), vec2(next_w, action_h));
    let cancel_rect = Rect::from_min_size(
        pos2(next_rect.left() - GAP - cancel_w, right_top),
        vec2(cancel_w, action_h),
    );

    // The popover, above the Card color button.
    let card = tools[3];
    let n = ACCOUNT_CARD_COLOR_PRESETS.len() as f32;
    let pop_size = vec2(
        2.0 * (bw + POPOVER_PAD.x) + n * SWATCH + (n - 1.0) * SWATCH_GAP,
        2.0 * (bw + POPOVER_PAD.y) + SWATCH,
    );
    let pop_rect = Rect::from_min_size(
        pos2(card.left(), card.top() - POPOVER_OFFSET - pop_size.y),
        pop_size,
    );
    let menu_open = props.bar.color_menu_open();
    let mut cover = vec![rect];
    if menu_open {
        cover.push(pop_rect);
    }
    ctx.data_mut(|d| d.insert_temp(Id::unique(COVER_ID), cover));

    // A press anywhere: the colour wrap is the button and the
    // popover.
    if let Some(pos) = ctx.input(|i| {
        i.pointer
            .primary_pressed()
            .then(|| i.pointer.press_origin())
            .flatten()
    }) {
        let inside = card.contains(pos) || (menu_open && pop_rect.contains(pos));
        events.push(BulkEvent::PointerDown(inside));
    }

    // The bar slides up: from `translateY(100%)` and transparent.
    let ramp = Keyframes::from_to(0.0f32, 1.0);
    let k = anim::animate(&ctx, id.with("slide"), &ramp, &SLIDE_UP, 1.0);
    let transform = TSTransform::from_translation(vec2(0.0, bar_h * (1.0 - k)));
    ui.with_visual_transform(transform, |ui| {
        let mut child = ui.new_child(UiBuilder::new().max_rect(props.viewport));
        child.multiply_opacity(k);
        let boxed = snap::rect(rect, ppp);
        // The bar takes the clicks between its buttons.
        child.interact(boxed, id.with("box"), Sense::CLICK);
        BoxStyle {
            background: v.bg_overlay,
            border: Border {
                width: Sides::new(bw, 0.0, 0.0, 0.0),
                color: Sides::all(v.border),
            },
            ..BoxStyle::default()
        }
        .paint(child.painter(), boxed);

        let enabled = BulkEditBar::selection_actions_enabled(props.selected);
        let actions = [
            (BulkEvent::SelectAll, true),
            (BulkEvent::DeselectAll, true),
            (BulkEvent::CopyUrls, enabled),
            (BulkEvent::ToggleColorMenu, enabled),
        ];
        for (k, ((event, on), label)) in actions.into_iter().zip(&labels).enumerate() {
            let bid = id.with(("tool", k));
            let lit = (on && hovered(&child, bid)) || (k == 3 && menu_open);
            if button(
                &mut child,
                tools[k],
                bid,
                &tool,
                label,
                on,
                tool_look(v, lit),
                false,
                props,
            )
            .clicked
            {
                events.push(event);
            }
        }
        text::paint(child.painter(), count_rect, &count, &count_style);

        let cid = id.with("cancel");
        let look = secondary_look(v, hovered(&child, cid));
        if button(
            &mut child,
            cancel_rect,
            cid,
            &secondary,
            &cancel,
            true,
            look,
            true,
            props,
        )
        .clicked
        {
            events.push(BulkEvent::Close);
        }
        let nid = id.with("next");
        let look = primary_look(v, props.accent_fg, enabled && hovered(&child, nid));
        if button(
            &mut child, next_rect, nid, &primary, &next, enabled, look, true, props,
        )
        .clicked
        {
            events.push(BulkEvent::Next);
        }

        if menu_open {
            events.extend(popover(&mut child, pop_rect, id.with("popover"), props));
        }
    });
    events
}

/// `.color-popover`: the presets in a row.
fn popover(ui: &mut Ui, rect: Rect, id: Id, props: &BulkProps) -> Vec<BulkEvent> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let bw = snap::border_width(v.border_px(), ppp);
    let mut events = Vec::new();
    let boxed = snap::rect(rect, ppp);
    ui.interact(boxed, id.with("box"), Sense::CLICK);
    BoxStyle {
        background: v.bg_overlay,
        radius: Corners::all(8.0),
        border: Border::all(bw, v.border),
        shadows: vec![BoxShadow {
            offset: vec2(0.0, 8.0),
            blur: 24.0,
            spread: 0.0,
            color: Rgba::new(0.0, 0.0, 0.0, 0.4),
            inset: false,
        }],
        ..BoxStyle::default()
    }
    .paint(ui.painter(), boxed);
    let origin = rect.min + vec2(bw + POPOVER_PAD.x, bw + POPOVER_PAD.y);
    for (k, preset) in ACCOUNT_CARD_COLOR_PRESETS.iter().enumerate() {
        let r = Rect::from_min_size(
            origin + vec2(k as f32 * (SWATCH + SWATCH_GAP), 0.0),
            Vec2::splat(SWATCH),
        );
        let sid = id.with(("swatch", k));
        let swatch = snap::rect(r, ppp);
        let response = ui.interact(swatch, sid, Sense::click());
        let name = color_label_key(preset.id)
            .map(|key| t(props.locale, key))
            .unwrap_or_default();
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
        let focused = take_focus(ui, &response);
        // `:hover, :focus-visible`: a 1 px ring of `--fg` at 35 %.
        let ring = response.hovered() || (focused && props.focus_visible);
        BoxStyle {
            background: Rgba::TRANSPARENT,
            radius: Corners::all(5.0),
            border: Border::all(bw, v.border),
            shadows: if ring {
                vec![BoxShadow {
                    offset: Vec2::ZERO,
                    blur: 0.0,
                    spread: 1.0,
                    color: v.fg.mix(0.35, Rgba::TRANSPARENT),
                    inset: false,
                }]
            } else {
                Vec::new()
            },
            ..BoxStyle::default()
        }
        .paint(ui.painter(), swatch);
        // `display: grid; place-items: center`.
        let off = bw + (SWATCH - 2.0 * bw - FILL) / 2.0;
        let fill = Rect::from_min_size(r.min + Vec2::splat(off), Vec2::splat(FILL));
        let (background, images) =
            match css::color(preset.color).filter(|_| !preset.color.is_empty()) {
                Some(color) => (color, Vec::new()),
                None => (Rgba::TRANSPARENT, vec![default_fill(v)]),
            };
        BoxStyle {
            background,
            images,
            radius: Corners::all(3.0),
            ..BoxStyle::default()
        }
        .paint(ui.painter(), snap::rect(fill, ppp));
        if response.clicked() {
            events.push(BulkEvent::Color(preset.color.to_string()));
        }
    }
    events
}
