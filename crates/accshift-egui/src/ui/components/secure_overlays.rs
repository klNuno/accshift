//! The secure screen: the `.inactivity-frost` of `App.svelte`, the AFK
//! cover and the PIN lock of `AppScreenOverlays.svelte`.
//!
//! ```text
//! .inactivity-frost    inset 0 in .app-shell, z 40: --bg 48% to 62%,
//!                      top to bottom; opacity 620 ms ease-in-out in,
//!                      220 ms ease-out out
//! .inactive-overlay    inset 0 over the stage, z 300, opacity 900 ms
//!   .accshift-text     centred, clamp(28px, min(13vw, 20vh), 170px), line
//!                      height 1; opacity .92 after --afk-reveal-delay
//!     WaveText         VT323 1.18em/700, line height .9, letter-spacing
//!                      .03em, --afk-text at 35% with two glows; one
//!                      inline-block per letter, `contain: paint`
//! .afk-version-strip   bottom 18 px, z 320: 11 px/600, letter-spacing
//!                      .08em, opacity .25 and 8 px up over 1200 ms after
//!                      the same delay, gone at once
//! .pin-lock-overlay    inset 0 over the stage, rgba(0, 0, 0, .35), the
//!                      card centred both ways; opacity 240 ms ease-in-out
//!                      while it unlocks
//!   .overlay-win-btns  top right, two 36 px buttons
//!   .pin-card          min(320px, 86vw), padding 18, radius 12, border 1
//!                      --border, --bg-card, column gap 10,
//!                      0 20px 40px rgba(0, 0, 0, .45)
//! ```

use accshift_app::i18n::{translate, Locale};
use egui::emath::TSTransform;
use egui::{
    pos2, vec2, Id, PointerButton, Rect, Sense, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType,
};

use super::controls::text_input::{self, FieldStyle, InputKind, TextInputProps};
use super::icon;
use crate::app::vars::Vars;
use crate::icons;
use crate::theme::Rgba;
use crate::ui::anim::{self, Timing, Transition};
use crate::ui::css;
use crate::ui::paint::{Border, BoxShadow, BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextAlign, TextStyle, TextTransform, PIXEL_STACK};
use crate::ui::text_shadow::{self, TextShadow};

/// `.inactivity-frost.visible { transition: opacity 620ms ease-in-out }`.
const T_FROST_IN: Transition = Transition::new(0.62, Timing::EASE_IN_OUT);
/// `.inactivity-frost { transition: opacity 220ms ease-out }`.
const T_FROST_OUT: Transition = Transition::new(0.22, Timing::EASE_OUT);
/// `.pin-lock-overlay { transition: opacity 240ms ease-in-out }`, and the
/// card's transform and opacity while it unlocks.
const T_UNLOCK: Transition = Transition::new(0.24, Timing::EASE_IN_OUT);
/// `.overlay-win-btn { transition: background 120ms }`.
const T_BUTTON: Transition = Transition::new(0.12, Timing::EASE);
/// `.pin-input:focus { border-color: #eab308 }`.
const FOCUS: Rgba = Rgba::new(234.0, 179.0, 8.0, 1.0);

/// `.inactive-overlay` and `.accshift-text { transition: opacity 900ms
/// ease-in-out }`.
const T_AFK: Transition = Transition::new(0.9, Timing::EASE_IN_OUT);
/// `.afk-version-strip.visible { transition: opacity 1200ms ease-in-out,
/// transform 1200ms ease-in-out }`.
const T_STRIP: Transition = Transition::new(1.2, Timing::EASE_IN_OUT);
/// `WaveText` defaults: 22 px, 0.3 turns a second, 0.7 rad per letter.
const WAVE_AMPLITUDE: f32 = 22.0;
const WAVE_SPEED: f64 = 0.3;
const WAVE_PHASE_STEP: f64 = 0.7;
/// The AFK wordmark.
const WORDMARK: &str = "ACCSHIFT";

/// `.pin-card` padding and gap.
const CARD_PAD: f32 = 18.0;
const CARD_GAP: f32 = 10.0;
/// `.overlay-win-btn`: 36 px square.
const WIN_BUTTON: f32 = 36.0;

/// `.inactivity-frost` over `rect`, at its transitioned opacity.
pub fn frost(ui: &Ui, rect: Rect, visible: bool, v: &Vars) {
    let ctx = ui.ctx();
    let spec = if visible { &T_FROST_IN } else { &T_FROST_OUT };
    let opacity = anim::transition_bool(ctx, Id::new("inactivity-frost"), visible, spec);
    if opacity <= 0.0 {
        return;
    }
    let css_rgba = |c: Rgba| format!("rgba({}, {}, {}, {})", c.r, c.g, c.b, c.a);
    let gradient = css::gradient(&format!(
        "linear-gradient(to bottom, {}, {})",
        css_rgba(v.bg.mix(0.48, Rgba::TRANSPARENT)),
        css_rgba(v.bg.mix(0.62, Rgba::TRANSPARENT)),
    ));
    BoxStyle {
        images: gradient.into_iter().collect(),
        opacity,
        ..BoxStyle::default()
    }
    .paint(ui.painter(), snap::rect(rect, ctx.pixels_per_point()));
}

pub struct AfkProps<'a> {
    pub vars: &'a Vars,
    /// `afkOverlayVisible`.
    pub visible: bool,
    /// `afkWaveActive && !motionPaused`.
    pub wave: bool,
    /// `--afk-reveal-delay`, seconds.
    pub reveal_delay: f64,
    /// `afkVersionLabel`.
    pub version: Option<&'a str>,
}

/// `WaveText`'s `frame`: letter `i` at `t` seconds into the wave.
fn wave_offset(t: f64, i: usize) -> f32 {
    let omega = WAVE_SPEED * std::f64::consts::TAU;
    ((t * omega + i as f64 * WAVE_PHASE_STEP).sin() as f32) * WAVE_AMPLITUDE
}

/// A width as Blink sizes a shrink-to-fit box: rounded up to LayoutUnits.
fn fit_width(w: f32, ppp: f32) -> f32 {
    (w * ppp * 64.0).ceil() / 64.0 / ppp
}

/// Where an inline-block of line box `inner` sits in a line whose strut is
/// `strut`, both on the baseline: the line's height and the block's top.
fn on_baseline(strut: text::LineBox, inner: text::LineBox) -> (f32, f32) {
    let above = strut.baseline.max(inner.baseline);
    let below = (strut.height - strut.baseline).max(inner.height - inner.baseline);
    (above + below, above - inner.baseline)
}

/// The AFK cover (`.inactive-overlay`) and the version strip over `stage`.
pub fn afk_overlay(ui: &Ui, stage: Rect, props: &AfkProps) {
    let ctx = ui.ctx();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let id = Id::new("afk");
    let overlay = anim::transition_bool(ctx, id.with("overlay"), props.visible, &T_AFK);
    let reveal = if props.visible {
        T_AFK.with_delay(props.reveal_delay)
    } else {
        T_AFK
    };
    let target = if props.visible { 0.92 } else { 0.0 };
    let text_opacity = anim::transition(ctx, id.with("text"), target, &reveal);
    let now = ctx.input(|i| i.time);

    // `WaveText`: the letters rest until `startDelayMs`, then ride a sine
    // whose clock started with the wave.
    let waving = props.wave && anim::motion(ctx) == anim::Motion::Full;
    let start = ctx.data_mut(|d| {
        let slot = d.get_temp_mut_or_insert_with(id.with("wave"), || None::<f64>);
        match (waving, *slot) {
            (false, _) => *slot = None,
            (true, None) => *slot = Some(now),
            (true, Some(_)) => {}
        }
        *slot
    });
    let wave_t = start.map(|s| now - s).filter(|t| *t >= props.reveal_delay);

    let opacity = overlay * text_opacity;
    if opacity > 0.0 {
        let viewport = ctx.content_rect();
        let size = (0.13 * viewport.width())
            .min(0.2 * viewport.height())
            .clamp(28.0, 170.0);
        let wave_size = 1.18 * size;
        let strut = TextStyle::new(size, 400).line_height(LineHeight::Px(size));
        let style = TextStyle::new(wave_size, 700)
            .family(PIXEL_STACK)
            .line_height(LineHeight::Px(0.9 * wave_size))
            .letter_spacing_px(0.03 * wave_size)
            .transform(TextTransform::Uppercase)
            .nowrap(false);
        let letters: Vec<String> = WORDMARK.chars().map(String::from).collect();
        let widths: Option<Vec<f32>> = letters
            .iter()
            .map(|c| text::measure(ctx, c, &style).map(|w| fit_width(w, ppp)))
            .collect();
        if let (Some(strut_box), Some(wave_box), Some(widths)) =
            (strut.line_box(ctx), style.line_box(ctx), widths)
        {
            let (height, inner_top) = on_baseline(strut_box, wave_box);
            let width: f32 = widths.iter().sum();
            let left = stage.center().x - width / 2.0;
            let top = stage.center().y - height / 2.0 + inner_top;
            let afk = v.afk_text;
            let style = style.color(afk.alpha(afk.a * 0.35 * opacity).to_color32());
            let shadows = [
                TextShadow {
                    offset: Vec2::ZERO,
                    blur: 3.0,
                    color: afk.alpha(afk.a * 0.55),
                },
                TextShadow {
                    offset: Vec2::ZERO,
                    blur: 14.0,
                    color: afk.alpha(afk.a * 0.5),
                },
            ];
            let mut x = left;
            for (i, (letter, w)) in letters.iter().zip(&widths).enumerate() {
                let dy = wave_t.map_or(0.0, |t| wave_offset(t, i));
                let r = Rect::from_min_size(pos2(x, top + dy), vec2(*w, wave_box.height));
                if let Some(layout) = text::layout(ctx, letter, &style, r) {
                    text_shadow::paint(ui.painter(), &layout, &shadows, Some(r), opacity);
                    layout.paint(ui.painter());
                }
                x += w;
            }
            if wave_t.is_some() {
                ctx.request_repaint();
            }
        }
    }
    if let (Some(s), None) = (start, wave_t) {
        // The wave starts moving at the reveal.
        let at = s + props.reveal_delay - now;
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(at.max(0.0)));
    }

    // `.afk-version-strip`: its transition only exists while visible, so
    // it snaps away.
    let strip_id = id.with("strip");
    let Some(version) = props.version else {
        ctx.data_mut(|d| d.insert_temp(strip_id, anim::TransitionState::settled(0.0f32)));
        return;
    };
    let shown = anim::transition_bool(ctx, strip_id, true, &T_STRIP.with_delay(props.reveal_delay));
    let strut = TextStyle::new(16.0, 400).line_height(LineHeight::Px(24.0));
    let style = TextStyle::new(11.0, 600)
        .line_height(LineHeight::Px(11.0))
        .letter_spacing_em(0.08);
    let (Some(strut_box), Some(span_box), Some(w)) = (
        strut.line_box(ctx),
        style.line_box(ctx),
        text::measure(ctx, version, &style),
    ) else {
        return;
    };
    let w = fit_width(w, ppp);
    let (height, inner_top) = on_baseline(strut_box, span_box);
    let top = stage.bottom() - 18.0 - height + inner_top + 8.0 * (1.0 - shown);
    let r = Rect::from_min_size(
        pos2(stage.center().x - w / 2.0, top),
        vec2(w, span_box.height),
    );
    let opacity = 0.25 * shown;
    let afk = v.afk_text;
    let style = style.color(afk.alpha(afk.a * opacity).to_color32());
    if let Some(layout) = text::layout(ctx, version, &style, r) {
        let shadows = [
            TextShadow {
                offset: Vec2::ZERO,
                blur: 10.0,
                color: afk.alpha(afk.a * 0.4),
            },
            TextShadow {
                offset: Vec2::ZERO,
                blur: 24.0,
                color: afk.alpha(afk.a * 0.34),
            },
        ];
        text_shadow::paint(ui.painter(), &layout, &shadows, None, opacity);
        layout.paint(ui.painter());
    }
}

pub struct PinProps<'a> {
    pub vars: &'a Vars,
    pub locale: Locale,
    pub attempt: &'a str,
    /// `pinError`, translated.
    pub error: Option<&'a str>,
    /// The field takes input (not unlocking, not waiting out a retry).
    pub input_enabled: bool,
    /// A matching code: the overlay fades away.
    pub unlocking: bool,
    /// Put keyboard focus in the field this frame.
    pub focus: bool,
    pub code_len: usize,
    pub dark: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PinEvent {
    /// The field's new value.
    Attempt(String),
    Minimize,
    Close,
    /// Mousedown on the backdrop: the overlay covers the title bar.
    StartDrag,
}

/// The field's id: the screen focuses it.
pub fn pin_input_id() -> Id {
    Id::new("pin-lock").with("input")
}

/// `.pin-input`: 16 px centred, letter-spacing 0.22em, padding 9 10, a
/// 1 px `--border` turning yellow on focus, on `--bg`, radius 8.
fn input_style(v: &Vars) -> FieldStyle {
    FieldStyle {
        font: TextStyle::new(16.0, 400)
            .line_height(LineHeight::Px(24.0))
            .letter_spacing_em(0.22)
            .align(TextAlign::Center),
        line: 24.0,
        padding: vec2(10.0, 9.0),
        border: 1.0,
        radius: 8.0,
        background: v.bg,
        border_color: v.border,
        border_focus: FOCUS,
        // Segoe UI's ascent plus descent at 16 px.
        caret_height: 21.28,
        caret_scaled: true,
        border_transition: false,
    }
}

/// The PIN lock over `window`.
pub fn pin_overlay(ui: &mut Ui, window: Rect, props: &PinProps) -> Vec<PinEvent> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let id = Id::new("pin-lock");
    let mut events = Vec::new();
    let leaving = anim::transition_bool(&ctx, id.with("unlocking"), props.unlocking, &T_UNLOCK);
    let opacity = 1.0 - leaving;

    let title_style = TextStyle::new(15.0, 400)
        .line_height(LineHeight::Px(22.5))
        .color(v.fg.to_color32());
    let prompt_style = TextStyle::new(12.0, 400)
        .line_height(LineHeight::Px(18.0))
        .color(v.fg_muted.to_color32());
    let error_style = TextStyle::new(11.0, 400)
        .line_height(LineHeight::Px(16.5))
        .color(v.danger.to_color32());
    let field = input_style(v);
    let field_h = field.height(ppp);

    // `.pin-card`: its content stacks 10 px apart.
    let card_w = 320f32.min(window.width() * 0.86);
    let mut content_h = 22.5 + CARD_GAP + 18.0 + CARD_GAP + field_h;
    if props.error.is_some() {
        content_h += CARD_GAP + 16.5;
    }
    let border = snap::border_width(1.0, ppp);
    let card_h = content_h + 2.0 * (CARD_PAD + border);
    let card = Rect::from_min_size(
        window.min
            + vec2(
                (window.width() - card_w) / 2.0,
                (window.height() - card_h) / 2.0,
            ),
        vec2(card_w, card_h),
    );
    let buttons = Rect::from_min_max(
        pos2(window.right() - 2.0 * WIN_BUTTON, window.top()),
        pos2(window.right(), window.top() + WIN_BUTTON),
    );

    let mut layer = ui.new_child(UiBuilder::new().max_rect(window));
    layer.multiply_opacity(opacity);
    // `pointer-events: none` once it unlocks.
    let live = !props.unlocking;
    if live {
        let back = layer.interact(window, id, Sense::click_and_drag());
        let on_backdrop = ctx
            .pointer_interact_pos()
            .is_some_and(|p| !card.contains(p) && !buttons.contains(p));
        if back.drag_started_by(PointerButton::Primary) && on_backdrop {
            events.push(PinEvent::StartDrag);
        }
    }
    layer
        .painter()
        .rect_filled(window, 0.0, Rgba::new(0.0, 0.0, 0.0, 0.35).to_color32());

    // `.overlay-win-btns`.
    let caption = [
        (
            icons::MINIMIZE,
            "titlebar.minimize",
            PinEvent::Minimize,
            false,
        ),
        (icons::CLOSE, "titlebar.close", PinEvent::Close, true),
    ];
    for (index, (svg, key, event, close)) in caption.into_iter().enumerate() {
        let r = Rect::from_min_size(
            pos2(buttons.left() + index as f32 * WIN_BUTTON, buttons.top()),
            vec2(WIN_BUTTON, WIN_BUTTON),
        );
        let bid = id.with(("button", index));
        let label = translate(props.locale, key);
        let hovered = live && {
            let response = layer.interact(r, bid, Sense::click());
            response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
            if response.clicked() {
                events.push(event);
            }
            response.hovered()
        };
        let bg = match (hovered, close) {
            (true, true) => v.danger,
            (true, false) => v.bg_muted,
            (false, _) => Rgba::TRANSPARENT,
        };
        let bg = anim::transition(&ctx, bid.with("bg"), bg, &T_BUTTON);
        layer
            .painter()
            .rect_filled(snap::rect(r, ppp), 0.0, bg.to_color32());
        // `color: var(--fg-muted); opacity: .7`, `--fg` at full on hover.
        let fg = if hovered {
            v.fg
        } else {
            v.fg_muted.alpha(v.fg_muted.a * 0.7)
        };
        let glyph = Rect::from_min_size(
            r.min
                + vec2(
                    snap::centre(WIN_BUTTON, 12.0, ppp),
                    snap::centre(WIN_BUTTON, 12.0, ppp),
                ),
            vec2(12.0, 12.0),
        );
        icon::paint(layer.painter(), svg, glyph, fg, 1.0);
    }

    // `.pin-card`, sliding 8 px down and shrinking to 0.98 as it fades.
    let transform =
        TSTransform::from_translation(card.center().to_vec2() + vec2(0.0, 8.0 * leaving))
            * TSTransform::from_scaling(1.0 - 0.02 * leaving)
            * TSTransform::from_translation(-card.center().to_vec2());
    layer.with_visual_transform(transform, |ui| {
        let mut card_ui = ui.new_child(UiBuilder::new().max_rect(card));
        card_ui.multiply_opacity(1.0 - leaving);
        BoxStyle {
            background: v.bg_card,
            radius: Corners::all(12.0),
            border: Border::all(border, v.border),
            shadows: vec![BoxShadow {
                offset: vec2(0.0, 20.0),
                blur: 40.0,
                spread: 0.0,
                color: Rgba::new(0.0, 0.0, 0.0, 0.45),
                inset: false,
            }],
            ..BoxStyle::default()
        }
        .paint(card_ui.painter(), snap::rect(card, ppp));
        let left = card.left() + border + CARD_PAD;
        let width = card.width() - 2.0 * (border + CARD_PAD);
        let mut y = card.top() + border + CARD_PAD;
        let mut line = |h: f32| {
            let r = Rect::from_min_size(pos2(left, y), vec2(width, h));
            y += h + CARD_GAP;
            r
        };
        let title = line(22.5);
        text::paint(
            card_ui.painter(),
            title,
            &translate(props.locale, "pin.lockedTitle"),
            &title_style,
        );
        let prompt_text = translate(props.locale, "pin.lockedPrompt");
        text::paint(card_ui.painter(), line(18.0), &prompt_text, &prompt_style);
        let input = line(field_h);
        if props.focus && props.input_enabled {
            ctx.memory_mut(|m| m.request_focus(pin_input_id()));
        }
        let mut value = props.attempt.to_string();
        let changed = text_input::text_input(
            &mut card_ui,
            input,
            pin_input_id(),
            &mut value,
            &TextInputProps {
                style: &field,
                kind: InputKind::Password,
                placeholder: &translate(props.locale, "pin.placeholder"),
                label: &prompt_text,
                max_len: Some(props.code_len),
                enabled: props.input_enabled && live,
                vars: v,
                dark: props.dark,
            },
        )
        .changed;
        if changed {
            events.push(PinEvent::Attempt(value));
        }
        if let Some(error) = props.error {
            text::paint(card_ui.painter(), line(16.5), error, &error_style);
        }
    });
    events
}

#[cfg(test)]
mod tests {
    use egui::{Context, Event, Modifiers, Pos2, RawInput};

    use super::*;

    const WINDOW: Rect = Rect::from_min_max(Pos2::ZERO, pos2(860.0, 440.0));

    struct Bench {
        ctx: Context,
        vars: Vars,
        time: f64,
        attempt: String,
        unlocking: bool,
    }

    impl Bench {
        fn new() -> Bench {
            let vars = Vars::new(&accshift_theme::ResolvedTheme::new(
                &accshift_theme::theme_application(
                    &accshift_theme::ThemeRegistry::new().theme(Some("glass-dark")),
                    100.0,
                    true,
                ),
            ));
            Bench {
                ctx: Context::default(),
                vars,
                time: 0.0,
                attempt: String::new(),
                unlocking: false,
            }
        }

        fn frame(&mut self, events: Vec<Event>, focus: bool) -> Vec<PinEvent> {
            self.time += 0.02;
            let input = RawInput {
                screen_rect: Some(WINDOW),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            let props = PinProps {
                vars: &self.vars,
                locale: Locale::En,
                attempt: &self.attempt,
                error: None,
                input_enabled: !self.unlocking,
                unlocking: self.unlocking,
                focus,
                code_len: 4,
                dark: true,
            };
            let mut out = Vec::new();
            let mut output = self.ctx.run_ui(input, |ui| {
                out = pin_overlay(ui, WINDOW, &props);
            });
            output.textures_delta.clear();
            // The screen stores what the field reported.
            for event in &out {
                if let PinEvent::Attempt(value) = event {
                    self.attempt = value.clone();
                }
            }
            out
        }

        fn drag(&mut self, from: Pos2) -> Vec<PinEvent> {
            let button = |pos, pressed| Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            let to = from + vec2(20.0, 12.0);
            let mut out = self.frame(vec![Event::PointerMoved(from)], false);
            out.extend(self.frame(vec![button(from, true)], false));
            out.extend(self.frame(vec![Event::PointerMoved(to)], false));
            out.extend(self.frame(vec![button(to, false)], false));
            out
        }
    }

    fn text(t: &str) -> Vec<Event> {
        vec![Event::Text(t.into())]
    }

    #[test]
    fn typing_in_the_focused_field_reports_each_attempt() {
        let mut bench = Bench::new();
        assert!(bench.frame(Vec::new(), true).is_empty());
        assert_eq!(
            bench.frame(text("12"), false),
            [PinEvent::Attempt("12".into())]
        );
        assert_eq!(
            bench.frame(text("3"), false),
            [PinEvent::Attempt("123".into())]
        );
        assert_eq!(
            bench.frame(text("45"), false),
            [PinEvent::Attempt("1234".into())]
        );
        // `maxlength`: a full code takes nothing more.
        assert!(bench.frame(text("6"), false).is_empty());
    }

    #[test]
    fn the_backdrop_drags_the_window_and_the_card_does_not() {
        let mut bench = Bench::new();
        bench.frame(Vec::new(), false);
        assert_eq!(bench.drag(pos2(60.0, 300.0)), [PinEvent::StartDrag]);
        assert!(bench.drag(pos2(430.0, 220.0)).is_empty());
    }

    #[test]
    fn an_unlocking_overlay_takes_no_input() {
        let mut bench = Bench::new();
        bench.frame(Vec::new(), true);
        bench.unlocking = true;
        assert!(bench.frame(text("1"), true).is_empty());
        assert!(bench.drag(pos2(60.0, 300.0)).is_empty());
        let close = pos2(842.0, 18.0);
        let mut out = bench.frame(vec![Event::PointerMoved(close)], false);
        for pressed in [true, false] {
            out.extend(bench.frame(
                vec![Event::PointerButton {
                    pos: close,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                }],
                false,
            ));
        }
        assert!(out.is_empty());
    }

    #[test]
    fn the_wordmark_line_matches_the_measured_box() {
        // glass-dark at 1x: `.accshift-text` 96.453 tall, `.wave-text` 3 px
        // below its top (strut: Segoe UI 88/88, baseline 80; letters: VT323
        // 103.84 on a 93.456 line, baseline 77).
        let strut = text::LineBox {
            height: 88.0,
            baseline: 80.0,
        };
        let letters = text::LineBox {
            height: 93.453125,
            baseline: 77.0,
        };
        assert_eq!(on_baseline(strut, letters), (96.453125, 3.0));
        // The version strip: a 24 px strut (baseline 18) around 11 px text
        // (baseline 10) puts the span 8 px down.
        let strut = text::LineBox {
            height: 24.0,
            baseline: 18.0,
        };
        let span = text::LineBox {
            height: 11.0,
            baseline: 10.0,
        };
        assert_eq!(on_baseline(strut, span), (24.0, 8.0));
        assert_eq!(fit_width(44.6512, 1.0), 44.65625);
    }

    #[test]
    fn the_wave_starts_mid_swing_and_steps_per_letter() {
        // `startMs = now - startDelayMs`: at the reveal the clock reads 2.5 s.
        assert!((wave_offset(2.5, 0) + 22.0).abs() < 1e-3);
        let expected = ((2.5 * 0.3 * std::f64::consts::TAU + 0.7).sin() * 22.0) as f32;
        assert!((wave_offset(2.5, 1) - expected).abs() < 1e-4);
        // One period is 1 / 0.3 s.
        assert!((wave_offset(2.5 + 1.0 / 0.3, 3) - wave_offset(2.5, 3)).abs() < 1e-3);
    }
}
