//! The settings step: the overlay, its dialog, the tri-state rows and the
//! launch options.

use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim::{self, Keyframes};
use crate::ui::backdrop_filter::Mark;
use crate::ui::components::controls::select::{self, NativeLook, SelectProps};
use crate::ui::components::controls::text_input::{self, FieldStyle, InputKind, TextInputProps};
use crate::ui::paint::{Border, BoxShadow, BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextStyle};
use accshift_app::bulk_edit::BulkEditBar;
use egui::emath::TSTransform;
use egui::{pos2, vec2, Context, Id, Rect, Sense, Ui, UiBuilder};

use super::button::{button, hovered, primary_look, secondary_look, tool_look, tri_look, Btn};
use super::{
    t, BulkEvent, BulkProps, AMBER, BODY_GAP, BULK_ID, COVER_ID, DIALOG_GAP, DIALOG_PAD, DIALOG_W,
    FADE, GAP, INPUT_W, ROW_GAP, SLIDE_IN,
};

/// The height of `text` wrapped to `width`.
fn text_height(ctx: &Context, text: &str, style: &TextStyle, width: f32) -> f32 {
    let one = style.line_box(ctx).map_or(style.size * 1.5, |l| l.height);
    text::layout(
        ctx,
        text,
        style,
        Rect::from_min_size(pos2(0.0, 0.0), vec2(width, one)),
    )
    .map_or(one, |l| l.height)
}

/// `.launch-input`: the inherited 1.5 line height, a caret as tall as the
/// font (ascent plus descent).
fn input_style(ctx: &Context, v: &Vars) -> FieldStyle {
    let font = TextStyle::new(11.0, 400).line_height(LineHeight::Px(16.5));
    let caret = TextStyle::new(11.0, 400)
        .line_height(LineHeight::Normal)
        .line_box(ctx)
        .map_or(15.0, |l| l.height);
    FieldStyle {
        font,
        line: 16.5,
        padding: vec2(8.0, 5.0),
        border: v.border_px(),
        radius: 6.0,
        background: v.bg_solid,
        border_color: v.border,
        border_focus: v.fg_muted.mix(0.55, v.border),
        placeholder: v.fg_subtle.mix(0.55, Rgba::TRANSPARENT),
        caret_height: caret,
        caret_scaled: true,
        border_transition: false,
    }
}

/// `.game-select`: the native menulist with the page's box.
fn select_look(v: &Vars) -> NativeLook {
    NativeLook {
        font: TextStyle::new(11.0, 400)
            .line_height(LineHeight::Normal)
            .color(v.fg.to_color32()),
        padding: vec2(8.0, 5.0),
        radius: 6.0,
        background: v.bg_solid,
        border: v.border,
        arrow: v.fg,
    }
}

/// The intrinsic border-box height of the select: the inner element (its
/// line and 1 px top and bottom, whole device px) in the padding.
fn select_height(ctx: &Context, look: &NativeLook, bw: f32) -> f32 {
    let ppp = ctx.pixels_per_point();
    let line = look.font.line_box(ctx).map_or(13.0, |l| l.height);
    line + 2.0 * ppp.trunc() / ppp + 2.0 * (look.padding.y + bw)
}

/// The settings step: the overlay and its dialog.
pub fn settings(ui: &mut Ui, props: &BulkProps) -> Vec<BulkEvent> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let locale = props.locale;
    let bar = props.bar;
    let id = Id::unique(BULK_ID).with(("settings", props.mount));
    let bw = snap::border_width(v.border_px(), ppp);
    let mut events = Vec::new();
    ctx.data_mut(|d| d.remove::<Vec<Rect>>(Id::unique(COVER_ID)));

    // The column, top to bottom.
    let width = DIALOG_W.min(crate::zoom::viewport(&ctx).x - 24.0);
    let inset = bw + DIALOG_PAD;
    let inner = width - 2.0 * inset;
    let title_style = TextStyle::new(13.0, 650)
        .line_height(LineHeight::Number(1.5))
        .color(v.fg.to_color32());
    let count_style = TextStyle::new(11.0, 400)
        .line_height(LineHeight::Number(1.5))
        .color(v.fg_muted.to_color32());
    let label_style = TextStyle::new(12.0, 400)
        .line_height(LineHeight::Number(1.5))
        .color(v.fg.to_color32());
    let hint_style = TextStyle::new(11.0, 400)
        .line_height(LineHeight::Number(1.5))
        .color(v.fg_subtle.to_color32());
    let warning_style = TextStyle::new(11.0, 400)
        .line_height(LineHeight::Number(1.5))
        .color(AMBER.to_color32());
    let line_of = |s: &TextStyle| s.line_box(&ctx).map_or(s.size * 1.5, |l| l.height);
    let title_h = line_of(&title_style).max(line_of(&count_style));
    let label_h = line_of(&label_style);
    let tri = Btn::tri();
    let tri_h = tri.height(&ctx, bw);
    let tri_row_h = 2.0 + tri_h.max(label_h) + 2.0;
    let input = input_style(&ctx, v);
    let look = select_look(v);
    let tool = Btn::tool();
    let add_label = t(locale, "bulkEdit.addLaunchOption");
    let add_w = tool.width(&ctx, &add_label, bw);
    let row_h = input
        .height(ppp)
        .max(select_height(&ctx, &look, bw))
        .max(tool.height(&ctx, bw));
    let loading = bar.games_loading();
    let hint_h = line_of(&hint_style);
    let edits = bar.launch_option_edits();
    let item_h = 16.5 + 8.0 + 2.0 * bw;
    let list_h = if edits.is_empty() {
        0.0
    } else {
        edits.len() as f32 * item_h + (edits.len() - 1) as f32 * 3.0
    };
    let launch_h = label_h
        + ROW_GAP
        + if loading { hint_h } else { row_h }
        + if edits.is_empty() {
            0.0
        } else {
            ROW_GAP + list_h
        };
    let warning = t(locale, "bulkEdit.activeAccountWarning");
    let warning_text_w = inner - 2.0 * bw - 20.0;
    let warning_h = if props.active_selected {
        text_height(&ctx, &warning, &warning_style, warning_text_w) + 12.0 + 2.0 * bw
    } else {
        0.0
    };
    let body_h = 2.0 * tri_row_h
        + BODY_GAP
        + BODY_GAP
        + launch_h
        + if props.active_selected {
            BODY_GAP + warning_h
        } else {
            0.0
        };
    let secondary = Btn::action(400);
    let primary = Btn::action(600);
    let action_h = secondary.height(&ctx, bw);
    let height = 2.0 * inset + title_h + DIALOG_GAP + body_h + DIALOG_GAP + action_h;
    let left = props.viewport.left() + snap::centre(props.viewport.width(), width, ppp);
    let top = props.viewport.top() + snap::centre(props.viewport.height(), height, ppp);
    let boxed = Rect::from_min_size(pos2(left, top), vec2(width, height));
    let x0 = left + inset;
    let x1 = x0 + inner;

    // The overlay: a click outside the dialog steps back.
    let ramp = Keyframes::from_to(0.0f32, 1.0);
    let fade = anim::animate(&ctx, id.with("fade"), &ramp, &FADE, 1.0);
    let slide = anim::animate(&ctx, id.with("slide"), &ramp, &SLIDE_IN, 1.0);
    let overlay = ui.interact(props.viewport, id.with("overlay"), Sense::click());
    let painter = ui.painter().clone();
    Mark::Begin {
        sigma: 4.0 * ppp,
        opacity: fade,
        saturate: 1.0,
        radius: None,
        ppp,
    }
    .paint(&painter, props.viewport);
    painter.rect_filled(
        props.viewport,
        0.0,
        Rgba::new(0.0, 0.0, 0.0, 0.5).to_color32(),
    );
    if overlay.clicked() {
        events.push(BulkEvent::Back);
    }

    // `dialog-slide-in`: `scale(.96) translateY(8px)` to none.
    let scale = 0.96 + 0.04 * slide;
    let rise = 8.0 * (1.0 - slide);
    let centre = boxed.center().to_vec2();
    let transform = TSTransform::from_translation(centre + vec2(0.0, rise * scale))
        * TSTransform::from_scaling(scale)
        * TSTransform::from_translation(-centre);
    ui.with_visual_transform(transform, |ui| {
        let mut child = ui.new_child(UiBuilder::new().max_rect(props.viewport));
        child.multiply_opacity(slide);
        let snapped = snap::rect(boxed, ppp);
        child.interact(snapped, id.with("box"), Sense::CLICK);
        BoxStyle {
            background: v.bg_overlay,
            radius: Corners::all(10.0),
            border: Border::all(bw, v.border),
            shadows: vec![BoxShadow {
                offset: vec2(0.0, 16.0),
                blur: 48.0,
                spread: 0.0,
                color: Rgba::new(0.0, 0.0, 0.0, 0.4),
                inset: false,
            }],
            ..BoxStyle::default()
        }
        .paint(child.painter(), snapped);
        let p = child.painter().clone();

        // `.dialog-header`.
        let mut y = top + inset;
        let title = t(locale, "bulkEdit.settings");
        let title_line = line_of(&title_style);
        text::paint(
            &p,
            Rect::from_min_size(
                pos2(x0, y + snap::centre(title_h, title_line, ppp)),
                vec2(inner, title_line),
            ),
            &title,
            &title_style,
        );
        let count = BulkEditBar::selected_label(props.selected).resolve(locale);
        let count_line = line_of(&count_style);
        let count_w = snap::content_width(
            text::measure(&ctx, &count, &count_style).unwrap_or(0.0),
            ppp,
        );
        text::paint(
            &p,
            Rect::from_min_size(
                pos2(x1 - count_w, y + snap::centre(title_h, count_line, ppp)),
                vec2(count_w, count_line),
            ),
            &count,
            &count_style,
        );
        y += title_h + DIALOG_GAP;

        // The two tri-state rows.
        let rows = [
            (
                "bulkEdit.newsPopup",
                bar.news_popup,
                BulkEvent::CycleNewsPopup,
            ),
            (
                "bulkEdit.doNotDisturb",
                bar.do_not_disturb,
                BulkEvent::CycleDoNotDisturb,
            ),
        ];
        for (k, (key, state, event)) in rows.into_iter().enumerate() {
            let inner_h = tri_h.max(label_h);
            let row_top = y + 2.0;
            text::paint(
                &p,
                Rect::from_min_size(
                    pos2(x0, row_top + snap::centre(inner_h, label_h, ppp)),
                    vec2(inner, label_h),
                ),
                &t(locale, key),
                &label_style,
            );
            let label = state.label().resolve(locale);
            let w = tri.width(&ctx, &label, bw);
            let r = Rect::from_min_size(
                pos2(x1 - w, row_top + snap::centre(inner_h, tri_h, ppp)),
                vec2(w, tri_h),
            );
            let bid = id.with(("tri", k));
            let look = tri_look(v, state, hovered(&child, bid));
            if button(&mut child, r, bid, &tri, &label, true, look, true, props).clicked {
                events.push(event);
            }
            y += tri_row_h + BODY_GAP;
        }

        // `.launch-section`.
        text::paint(
            &p,
            Rect::from_min_size(pos2(x0, y), vec2(inner, label_h)),
            &t(locale, "bulkEdit.launchOptions"),
            &label_style,
        );
        y += label_h + ROW_GAP;
        if loading {
            text::paint(
                &p,
                Rect::from_min_size(pos2(x0, y), vec2(inner, hint_h)),
                &t(locale, "bulkEdit.loadingGames"),
                &hint_style,
            );
            y += hint_h;
        } else {
            let select_w = inner - INPUT_W - add_w - 2.0 * ROW_GAP;
            let select_rect = Rect::from_min_size(pos2(x0, y), vec2(select_w, row_h));
            let input_rect =
                Rect::from_min_size(pos2(select_rect.right() + ROW_GAP, y), vec2(INPUT_W, row_h));
            let add_rect =
                Rect::from_min_size(pos2(input_rect.right() + ROW_GAP, y), vec2(add_w, row_h));
            let mut options = vec![t(locale, "bulkEdit.selectGame")];
            options.extend(bar.games().iter().map(|g| g.name.clone()));
            let selected = bar
                .games()
                .iter()
                .position(|g| g.app_id == bar.selected_game_id)
                .map_or(0, |i| i + 1);
            let picked = select::select(
                &mut child,
                select_rect,
                id.with("game"),
                &SelectProps {
                    options: &options,
                    selected,
                    disabled: &[],
                    label: &options[0],
                    vars: v,
                    look: Some(&look),
                },
            );
            if let Some(i) = picked.changed {
                let app_id = if i == 0 {
                    String::new()
                } else {
                    bar.games()[i - 1].app_id.clone()
                };
                events.push(BulkEvent::Game(app_id));
            }
            let mut value = bar.launch_option_input.clone();
            let field = text_input::text_input(
                &mut child,
                input_rect,
                id.with("launch-input"),
                &mut value,
                &TextInputProps {
                    style: &input,
                    kind: InputKind::Text,
                    placeholder: "-fullscreen",
                    label: "-fullscreen",
                    max_len: None,
                    enabled: true,
                    vars: v,
                    dark: props.dark,
                },
            );
            if field.changed {
                events.push(BulkEvent::LaunchInput(value));
            }
            if field.enter {
                events.push(BulkEvent::AddLaunchOption);
            }
            let aid = id.with("add");
            let enabled = bar.can_add_launch_option();
            let look = tool_look(v, enabled && hovered(&child, aid));
            if button(
                &mut child, add_rect, aid, &tool, &add_label, enabled, look, false, props,
            )
            .clicked
            {
                events.push(BulkEvent::AddLaunchOption);
            }
            y += row_h;
        }
        if !edits.is_empty() {
            y += ROW_GAP;
            for edit in edits {
                let r = Rect::from_min_max(pos2(x0, y), pos2(x1, y + item_h));
                if let Some(app_id) = launch_item(&mut child, r, id, edit, bar, props) {
                    events.push(BulkEvent::RemoveLaunchOption(app_id));
                }
                y += item_h + 3.0;
            }
            y -= 3.0;
        }

        // `.warning`.
        if props.active_selected {
            y += BODY_GAP;
            let r = Rect::from_min_max(pos2(x0, y), pos2(x1, y + warning_h));
            BoxStyle {
                background: AMBER.mix(0.08, v.bg_card),
                radius: Corners::all(6.0),
                border: Border::all(bw, AMBER.mix(0.3, v.border)),
                ..BoxStyle::default()
            }
            .paint(&p, snap::rect(r, ppp));
            let text_rect = Rect::from_min_size(
                pos2(x0 + bw + 10.0, y + bw + 6.0),
                vec2(warning_text_w, warning_h - 12.0 - 2.0 * bw),
            );
            text::paint(&p, text_rect, &warning, &warning_style);
        }

        // `.dialog-actions`.
        let y = top + height - inset - action_h;
        let apply = props.bar.apply_label().resolve(locale);
        let back = t(locale, "common.back");
        let apply_w = primary.width(&ctx, &apply, bw);
        let back_w = secondary.width(&ctx, &back, bw);
        let apply_rect = Rect::from_min_size(pos2(x1 - apply_w, y), vec2(apply_w, action_h));
        let back_rect = Rect::from_min_size(
            pos2(apply_rect.left() - GAP - back_w, y),
            vec2(back_w, action_h),
        );
        let bid = id.with("back");
        let look = secondary_look(v, hovered(&child, bid));
        if button(
            &mut child, back_rect, bid, &secondary, &back, true, look, true, props,
        )
        .clicked
        {
            events.push(BulkEvent::Back);
        }
        let aid = id.with("apply");
        let enabled = bar.apply_enabled(props.selected);
        let look = primary_look(v, props.accent_fg, enabled && hovered(&child, aid));
        if button(
            &mut child, apply_rect, aid, &primary, &apply, enabled, look, true, props,
        )
        .clicked
        {
            events.push(BulkEvent::Apply);
        }
    });
    Mark::End.paint(&painter, props.viewport);
    events
}

/// One `.launch-item`: the game, the value, the remove button. Returns the
/// app id when it was removed.
fn launch_item(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    edit: &accshift_app::bulk_edit::LaunchOptionDraft,
    bar: &BulkEditBar,
    props: &BulkProps,
) -> Option<String> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let bw = snap::border_width(v.border_px(), ppp);
    BoxStyle {
        background: v.bg_card,
        radius: Corners::all(6.0),
        border: Border::all(bw, v.border),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), snap::rect(rect, ppp));
    let game_style = TextStyle::new(11.0, 500)
        .line_height(LineHeight::Number(1.5))
        .nowrap(true)
        .color(v.fg.to_color32());
    let value_style = TextStyle::new(11.0, 400)
        .line_height(LineHeight::Number(1.5))
        .nowrap(true)
        .color(v.fg_muted.to_color32());
    let remove_style = TextStyle::new(14.0, 400)
        .line_height(LineHeight::Number(1.0))
        .color(v.fg_muted.to_color32());
    let top = rect.top() + bw + 4.0;
    let line = 16.5;
    let name = bar.game_name(&edit.app_id);
    let game_w =
        snap::content_width(text::measure(&ctx, name, &game_style).unwrap_or(0.0), ppp).min(160.0);
    let x0 = rect.left() + bw + 8.0;
    let x1 = rect.right() - bw - 8.0;
    // `.remove-btn { padding: 0 2px }`.
    let remove_w = snap::content_width(
        text::measure(&ctx, "\u{d7}", &remove_style).unwrap_or(0.0),
        ppp,
    ) + 4.0;
    text::paint(
        ui.painter(),
        Rect::from_min_size(pos2(x0, top), vec2(game_w, line)),
        name,
        &game_style,
    );
    let value_left = x0 + game_w + 8.0;
    text::paint(
        ui.painter(),
        Rect::from_min_max(
            pos2(value_left, top),
            pos2((x1 - remove_w - 8.0).max(value_left), top + line),
        ),
        &edit.value,
        &value_style,
    );
    let remove_rect = Rect::from_min_size(
        pos2(x1 - remove_w, top + snap::centre(line, 14.0, ppp)),
        vec2(remove_w, 14.0),
    );
    let rid = id.with(("remove", edit.app_id.as_str()));
    let response = ui.interact(snap::rect(remove_rect, ppp), rid, Sense::click());
    let color = if response.hovered() {
        Rgba::hex(0xef4444, 1.0)
    } else {
        v.fg_muted
    };
    text::paint(
        ui.painter(),
        remove_rect.shrink2(vec2(2.0, 0.0)),
        "\u{d7}",
        &remove_style.color(color.to_color32()),
    );
    response.clicked().then(|| edit.app_id.clone())
}
