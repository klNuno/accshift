//! `TelemetryOnboarding.svelte`: the first-run dialog and its feature tour,
//! drawn above everything in the frame (`--z-tour-*`), outside the
//! `.app-shell` the obscured filter blurs.
//!
//! ```text
//! .click-shield   fixed under the title bar, transparent: the app gets no
//!                 pointer, the title bar keeps it
//! .mock-ctx-menu  feature 3: a fake context menu right of the first card
//! .spotlight      features step: the target's rect grown by 6, radius 14,
//!                 a 9999 px dim shadow, an accent ring and glow, pulsing
//! .backdrop       dim (black 60%, blur 8) or clear on the features step,
//!                 280 ms; fadeIn 200 ms; opacity 0 over 800 ms when fading
//!   .modal        centred, min(94vw, 540), or docked 24 px above the
//!                 bottom at min(94vw, 460) on the features step
//!     .dots       the three steps
//!     .persist-error
//!     .step       welcome, features or deal ([`steps`])
//! ```
//!
//! The tour measures its target with `getBoundingClientRect` after the step
//! or feature changes (after Svelte's tick, then once more a frame later)
//! and on resize: the rects the title bar and the grid recorded this frame
//! ([`TourTargets`]) stand in for the DOM.

mod clip;
mod steps;

use egui::emath::TSTransform;
use egui::{pos2, vec2, Context, Id, Pos2, Rect, Sense, Ui, UiBuilder, Vec2};

use accshift_app::i18n::{self, Locale};

use crate::app::onboarding::{OnboardingEvent, OnboardingView, Step};
use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim::{self, Animation, Keyframes, Timing, Transition};
use crate::ui::backdrop_filter::Mark;
use crate::ui::components::controls::{paint_ua_focus_ring, take_focus};
use crate::ui::paint::{self, Border, BoxShadow, BoxStyle, Corners, Transform};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextAlign, TextStyle};

/// `#ef4444`.
const RED: Rgba = Rgba::hex(0xef4444, 1.0);
const BLACK: Rgba = Rgba::hex(0x000000, 1.0);
const WHITE: Rgba = Rgba::hex(0xffffff, 1.0);

/// `cubic-bezier(0.22, 1, 0.36, 1)`, the modal's and the spotlight's curve.
const OUT_QUINT: Timing = Timing::cubic_bezier(0.22, 1.0, 0.36, 1.0);
/// `modalIn` and `dockIn`, 280 ms.
const ENTER: Animation = Animation::new(0.28, OUT_QUINT);
/// `.backdrop { animation: fadeIn 200ms ease-out }`.
const FADE_IN: Animation = Animation::new(0.2, Timing::EASE_OUT);
/// `transition: background 280ms ease-out, backdrop-filter 280ms ease-out`.
const DIM: Transition = Transition::new(0.28, Timing::EASE_OUT);
/// `opacity 800ms ease-out`: the backdrop always, the modal while fading.
const FADE_OUT: Transition = Transition::new(0.8, Timing::EASE_OUT);
/// The spotlight's `left`, `top`, `width`, `height`: 260 ms.
const SPOT_MOVE: Transition = Transition::new(0.26, OUT_QUINT);
/// `pulse 1.8s ease-in-out infinite`.
const PULSE: Animation = Animation::new(1.8, Timing::EASE_IN_OUT);
/// `ctxIn 180ms ease-out`.
const CTX_IN: Animation = Animation::new(0.18, Timing::EASE_OUT);
/// `.dots span { transition: ... 160ms ease-out }`.
const DOT: Transition = Transition::new(0.16, Timing::EASE_OUT);

/// The root id; everything is `.with(mount)` below it.
pub const ONBOARDING_ID: &str = "accshift-onboarding";

/// Where the tour's targets were drawn this frame (`data-tour`), in window
/// points as `getBoundingClientRect` reports them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TourTargets {
    /// `[data-tour="add-account"]`: the title bar plus.
    pub add_account: Option<Rect>,
    /// `[data-tour="account-card"]`: the first account card.
    pub account_card: Option<Rect>,
    /// `[data-tour="platforms"]`: the title bar tabs.
    pub platforms: Option<Rect>,
}

fn targets_id() -> Id {
    Id::unique("accshift::tour-targets")
}

/// Forgets last frame's targets; the title bar and the grid record theirs
/// as they draw.
pub fn reset_targets(ctx: &Context) {
    ctx.data_mut(|d| d.insert_temp(targets_id(), TourTargets::default()));
}

/// Records a target; the first call of a frame wins (`querySelector`).
pub fn record_target(ctx: &Context, f: impl FnOnce(&mut TourTargets)) {
    ctx.data_mut(|d| f(d.get_temp_mut_or_default::<TourTargets>(targets_id())));
}

pub fn targets(ctx: &Context) -> TourTargets {
    ctx.data(|d| d.get_temp(targets_id())).unwrap_or_default()
}

/// Props of the dialog.
pub struct OnboardingProps<'a> {
    pub vars: &'a Vars,
    pub locale: Locale,
    /// The window: the backdrop covers it.
    pub viewport: Rect,
    /// `titlebarH`: the click shield starts under it.
    pub titlebar_h: f32,
    /// `appVersion`.
    pub version: &'a str,
    /// `welcomePlatforms`, names.
    pub platforms: &'a [String],
    /// Detection found something: the label says so.
    pub detected: bool,
    pub view: &'a OnboardingView,
    /// `:focus-visible`.
    pub focus_visible: bool,
    /// `color-scheme: dark`, for the UA focus ring.
    pub dark: bool,
    pub now: f64,
}

/// What `measureCurrent` last found.
#[derive(Clone, Default)]
struct Measure {
    /// Step, feature, window size: a change measures again.
    key: Option<(Step, usize, [i32; 2])>,
    /// Measures taken since the key changed (two: the tick, the frame).
    taken: u8,
    /// `spotlightRect`.
    spot: Option<Rect>,
    /// `contextMenuPos`.
    menu_at: Option<Pos2>,
    /// `spotlightMissing`.
    missing: bool,
    /// Bumped each time the spotlight or the menu mounts.
    spot_mount: u64,
    menu_mount: u64,
}

/// `.mock-ctx-menu`: 170 wide, 145 tall plus its borders (padding 4, four
/// 32 px items, a 1 px separator with 4 px margins).
fn menu_size(ppp: f32) -> Vec2 {
    vec2(170.0, 145.0 + 2.0 * snap::border_width(1.0, ppp))
}

/// The menu's box at `ctxIn` progress `p`: `translateX(-6px) scale(.97)`
/// about its centre at the start.
fn menu_rect(at: Pos2, p: f32, ppp: f32) -> Rect {
    let r = Rect::from_min_size(at, menu_size(ppp));
    let scale = 0.97 + 0.03 * p;
    Rect::from_center_size(r.center() + vec2(-6.0 * (1.0 - p), 0.0), r.size() * scale)
}

fn measure(m: &mut Measure, step: Step, feature: usize, targets: &TourTargets, ppp: f32) {
    if step != Step::Features {
        m.spot = None;
        m.menu_at = None;
        return;
    }
    let target = match feature {
        0 => targets.add_account,
        1 | 2 => targets.account_card,
        _ => targets.platforms,
    };
    let Some(mut rect) = target else {
        m.spot = None;
        m.menu_at = None;
        m.missing = true;
        return;
    };
    m.missing = false;
    if feature == 2 {
        // The menu mounts after the first measure: the second one finds it
        // at the start of its entrance.
        if let Some(at) = m.menu_at {
            rect = rect.union(menu_rect(at, 0.0, ppp));
        } else {
            m.menu_mount += 1;
        }
        let card = target.unwrap_or(rect);
        m.menu_at = Some(pos2(card.right() + 10.0, card.top() + 8.0));
    } else {
        m.menu_at = None;
    }
    if m.spot.is_none() {
        m.spot_mount += 1;
    }
    m.spot = Some(rect.expand(6.0));
}

/// Draws the dialog; returns what the user did.
pub fn onboarding(ui: &mut Ui, props: &OnboardingProps) -> Vec<OnboardingEvent> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let view = props.view;
    let root = Id::unique(ONBOARDING_ID).with(view.mount);
    let mut events = Vec::new();

    // `measureCurrent`.
    let m_id = root.with("measure");
    let mut m: Measure = ctx.data(|d| d.get_temp(m_id)).unwrap_or_default();
    let size = props.viewport.size();
    let key = (
        view.step,
        view.feature,
        [size.x.round() as i32, size.y.round() as i32],
    );
    if m.key != Some(key) {
        m.key = Some(key);
        m.taken = 0;
    }
    if m.taken < 2 {
        measure(
            &mut m,
            view.step,
            view.feature,
            &targets(&ctx),
            ctx.pixels_per_point(),
        );
        m.taken += 1;
        ctx.request_repaint();
    }
    ctx.data_mut(|d| d.insert_temp(m_id, m.clone()));

    // `.click-shield`: the app under the title bar gets no pointer.
    let shield = Rect::from_min_max(
        pos2(
            props.viewport.left(),
            props.viewport.top() + props.titlebar_h,
        ),
        props.viewport.max,
    );
    ui.interact(shield, root.with("shield"), Sense::click());

    let features = view.step == Step::Features;
    if features {
        if let Some(at) = m.menu_at {
            mock_menu(ui, props, root.with(("menu", m.menu_mount)), at);
        }
        if let Some(spot) = m.spot {
            spotlight(ui, props, root.with(("spot", m.spot_mount)), spot);
        }
    }

    // `.backdrop`.
    let painter = ui.painter().clone();
    let dim_alpha = anim::transition(
        &ctx,
        root.with("dim"),
        if features { 0.0f32 } else { 0.6 },
        &DIM,
    );
    let blur = anim::transition(
        &ctx,
        root.with("blur"),
        if features { 0.0f32 } else { 8.0 },
        &DIM,
    );
    let fade_in = anim::animate(
        &ctx,
        root.with("fade-in"),
        &Keyframes::from_to(0.0f32, 1.0),
        &FADE_IN,
        1.0,
    );
    let shown = anim::transition(
        &ctx,
        root.with("fading"),
        if view.fading { 0.0f32 } else { 1.0 },
        &FADE_OUT,
    );
    let opacity = fade_in * shown;
    // `backdrop-filter: blur(0px)` on the tour is still a filter: the
    // layer starts as a copy of everything since the backdrop root (the
    // see-through shell, the spotlight, the menu) and lands on top of it,
    // so every translucent layer under it counts twice.
    Mark::Begin {
        sigma: blur * ppp,
        opacity,
        saturate: 1.0,
        radius: None,
        ppp,
    }
    .paint(&painter, props.viewport);
    if dim_alpha > 0.0 {
        painter.rect_filled(props.viewport, 0.0, BLACK.alpha(dim_alpha).to_color32());
    }
    modal(ui, props, root, &m, &mut events);
    Mark::End.paint(&painter, props.viewport);
    events
}

/// `.mock-ctx-menu`: `--bg-card`, border, radius 10, `0 18px 40px
/// rgba(0,0,0,.55)`, padding 4; Rename, Set color, Move to folder, a
/// separator, Delete in red. No pointer events.
fn mock_menu(ui: &Ui, props: &OnboardingProps, id: Id, at: Pos2) {
    let ctx = ui.ctx();
    let v = props.vars;
    let p = anim::animate(ctx, id, &Keyframes::from_to(0.0f32, 1.0), &CTX_IN, 1.0);
    let ppp = ctx.pixels_per_point();
    let bw = snap::border_width(1.0, ppp);
    let base = Rect::from_min_size(at, menu_size(ppp));
    let scale = 0.97 + 0.03 * p;
    let shift = vec2(-6.0 * (1.0 - p), 0.0);
    let mut painter = ui.painter().clone();
    painter.multiply_opacity(p);
    BoxStyle {
        background: v.bg_card,
        radius: Corners::all(10.0),
        border: Border::all(bw, v.border),
        shadows: vec![BoxShadow {
            offset: vec2(0.0, 18.0),
            blur: 40.0,
            spread: 0.0,
            color: Rgba::new(0.0, 0.0, 0.0, 0.55),
            inset: false,
        }],
        ..BoxStyle::default()
    }
    .paint_transformed(
        &painter,
        base,
        Transform {
            translate: shift,
            scale,
        },
    );
    // The items as laid out, then the box's transform about its centre.
    let centre = base.center();
    let place = |r: Rect| -> Rect {
        let min = centre + (r.min - centre) * scale + shift;
        Rect::from_min_size(min, r.size() * scale)
    };
    let style = TextStyle::new(12.0, 400).line_height(LineHeight::Number(1.5));
    let items = [
        ("onboarding.features.mockRename", false),
        ("onboarding.features.mockSetColor", false),
        ("onboarding.features.mockMoveToFolder", false),
        ("", false),
        ("onboarding.features.mockDelete", true),
    ];
    let mut y = at.y + bw + 4.0;
    let x = at.x + bw + 4.0;
    let w = base.width() - 2.0 * (bw + 4.0);
    for (key, danger) in items {
        if key.is_empty() {
            // `.mock-ctx-sep`: 1 px, margin 4px 6px.
            let r = Rect::from_min_size(pos2(x + 6.0, y + 4.0), vec2(w - 12.0, 1.0));
            BoxStyle {
                background: v.border,
                ..BoxStyle::default()
            }
            .paint_transformed(
                &painter,
                r,
                Transform {
                    translate: (place(r).center() - r.center()),
                    scale,
                },
            );
            y += 9.0;
            continue;
        }
        let label = i18n::translate(props.locale, key);
        let color = if danger { RED } else { v.fg };
        let text_rect = place(Rect::from_min_size(
            pos2(x + 12.0, y + 7.0),
            vec2(w - 24.0, 18.0),
        ));
        let font = if (scale - 1.0).abs() > 1e-3 {
            TextStyle::new(12.0 * scale, 400).line_height(LineHeight::Px(18.0 * scale))
        } else {
            style.clone()
        };
        text::paint(
            &painter,
            text_rect,
            &label,
            &font.color(color.to_color32()).nowrap(false),
        );
        y += 32.0;
    }
}

/// `.spotlight`: no fill, four shadows (the dim first, on top), the rect
/// eased over 260 ms, `pulse` brightening it.
fn spotlight(ui: &Ui, props: &OnboardingProps, id: Id, target: Rect) {
    let ctx = ui.ctx();
    let v = props.vars;
    let min = anim::transition(ctx, id.with("min"), target.min, &SPOT_MOVE);
    let size = anim::transition(ctx, id.with("size"), target.size(), &SPOT_MOVE);
    let rect = Rect::from_min_size(min, size);
    // `filter: brightness(1 -> 1.25 -> 1)`.
    let b = anim::animate(
        ctx,
        id.with("pulse"),
        &Keyframes::new([(0.0, 1.0f32), (0.5, 1.25), (1.0, 1.0)]),
        &PULSE.infinite(),
        1.0,
    );
    let bright = |c: Rgba| Rgba::new(c.r * b, c.g * b, c.b * b, c.a);
    let accent = v.accent;
    let painter = ui.painter();
    // Under the dim: the ring, the glow, the inset ring, painted last first.
    BoxStyle {
        radius: Corners::all(14.0),
        shadows: vec![
            BoxShadow {
                offset: Vec2::ZERO,
                blur: 0.0,
                spread: 2.0,
                color: bright(accent),
                inset: false,
            },
            BoxShadow {
                offset: Vec2::ZERO,
                blur: 28.0,
                spread: 0.0,
                color: bright(accent.alpha(accent.a * 0.7)),
                inset: false,
            },
            BoxShadow {
                offset: Vec2::ZERO,
                blur: 0.0,
                spread: 2.0,
                color: bright(accent.alpha(accent.a * 0.9)),
                inset: true,
            },
        ],
        ..BoxStyle::default()
    }
    .paint(painter, rect);
    // `0 0 0 9999px color-mix(#000 55%, transparent)`, listed first: on top.
    paint::paint_outside(
        painter,
        props.viewport,
        rect,
        14.0,
        Rgba::new(0.0, 0.0, 0.0, 0.55),
    );
}

/// `.modal` and what it holds.
fn modal(
    ui: &mut Ui,
    props: &OnboardingProps,
    root: Id,
    m: &Measure,
    events: &mut Vec<OnboardingEvent>,
) {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let view = props.view;
    let vw = props.viewport.width();
    let vh = props.viewport.height();
    let docked = view.step == Step::Features;
    let deal = view.step == Step::Deal;
    let short = vh <= 640.0;

    // `.modal`, `.dock-bottom`, `.deal-mode` and the 640 px query.
    let width = if docked {
        (0.94 * vw).min(460.0)
    } else {
        (0.94 * vw).min(540.0)
    };
    let (pad_top, pad_x, pad_bottom, gap) = match view.step {
        Step::Features => (16.0, 18.0, 14.0, 10.0),
        Step::Deal if short => (14.0, 18.0, 12.0, 8.0),
        Step::Deal => (18.0, 22.0, 16.0, 12.0),
        Step::Welcome => (22.0, 22.0, 20.0, 14.0),
    };
    let border = snap::border_width(1.0, ppp);
    let inner_w = width - 2.0 * (border + pad_x);
    let max_h = if deal {
        (0.92 * vh).min(660.0)
    } else {
        0.92 * vh
    };

    // The column: dots (6 + 2 margin), the error row, the step.
    let error = view
        .persist_error
        .then(|| steps::PersistError::new(&ctx, props, inner_w));
    let mut chrome = 8.0 + gap;
    if let Some(e) = &error {
        chrome += e.height + 8.0 + gap;
    }
    let step_max = max_h - 2.0 * border - pad_top - pad_bottom - chrome;
    let step = steps::StepLayout::new(&ctx, props, inner_w, step_max);
    let height = (2.0 * border + pad_top + pad_bottom + chrome + step.height()).min(max_h);
    let left = props.viewport.left() + (vw - width) / 2.0;
    let top = if docked {
        props.viewport.bottom() - 24.0 - height
    } else {
        // `display: grid; place-items: center`.
        props.viewport.top() + snap::centre(vh, height, ppp)
    };
    let boxed = Rect::from_min_size(pos2(left, top), vec2(width, height));

    // The entrance restarts whenever the animation name changes.
    let gen_id = root.with("dock-gen");
    let (last, generation) = ctx
        .data(|d| d.get_temp::<(bool, u64)>(gen_id))
        .unwrap_or((docked, 0));
    let generation = if last != docked {
        generation + 1
    } else {
        generation
    };
    ctx.data_mut(|d| d.insert_temp(gen_id, (docked, generation)));
    let enter = anim::animate(
        &ctx,
        root.with(("enter", generation)),
        &Keyframes::from_to(0.0f32, 1.0),
        &ENTER,
        1.0,
    );
    // `.modal.fading`: transitioned only while it applies.
    let fade_id = root.with("modal-fade");
    let fade = if view.fading {
        anim::transition(&ctx, fade_id, 1.0f32, &FADE_OUT)
    } else {
        ctx.data_mut(|d| d.remove::<anim::TransitionState<f32>>(fade_id));
        anim::transition(&ctx, fade_id, 0.0f32, &FADE_OUT)
    };
    let (scale, rise) = if docked {
        // `translate(-50%, 14px) scale(.97)`: the rise is not scaled.
        let s = 0.97 + 0.03 * enter;
        (s, 14.0 * (1.0 - enter))
    } else {
        // `scale(.96) translateY(8px)`, then `.fading`'s `scale(.98)
        // translateY(6px)`.
        let s = (0.96 + 0.04 * enter) * (1.0 - 0.02 * fade);
        (s, (8.0 * (1.0 - enter) + 6.0 * fade) * s)
    };
    let opacity = enter * (1.0 - fade);
    let centre = boxed.center().to_vec2();
    let transform = TSTransform::from_translation(centre + vec2(0.0, rise))
        * TSTransform::from_scaling(scale)
        * TSTransform::from_translation(-centre);
    ui.with_visual_transform(transform, |ui| {
        let mut child = ui.new_child(UiBuilder::new().max_rect(props.viewport));
        child.multiply_opacity(opacity);
        let snapped = snap::rect(boxed, ppp);
        // The modal takes its own pointer events.
        child.interact(snapped, root.with("box"), Sense::click());
        BoxStyle {
            background: v.bg_card,
            radius: Corners::all(16.0),
            border: Border::all(border, v.border),
            shadows: vec![BoxShadow {
                offset: vec2(0.0, 24.0),
                blur: 64.0,
                spread: 0.0,
                color: Rgba::new(0.0, 0.0, 0.0, 0.55),
                inset: false,
            }],
            ..BoxStyle::default()
        }
        .paint(child.painter(), snapped);
        let x0 = left + border + pad_x;
        let mut y = top + border + pad_top;
        dots(&child, props, root, pos2(x0, y), inner_w);
        y += 8.0 + gap;
        if let Some(e) = &error {
            e.draw(&mut child, props, root, pos2(x0, y), inner_w, events);
            y += e.height + 8.0 + gap;
        }
        step.draw(&mut child, props, root, m, pos2(x0, y), inner_w, events);
    });
}

/// `.dots`: three 6 px dots, 6 apart, centred; the current one in `--fg`
/// at 1.25x, the others `--fg` 25%.
fn dots(ui: &Ui, props: &OnboardingProps, root: Id, at: Pos2, width: f32) {
    let ctx = ui.ctx();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let x0 = at.x + snap::centre(width, 30.0, ppp);
    for (i, step) in [Step::Welcome, Step::Features, Step::Deal]
        .into_iter()
        .enumerate()
    {
        let on = props.view.step == step;
        let id = root.with(("dot", i));
        let bg = anim::transition(
            ctx,
            id.with("bg"),
            if on { v.fg } else { v.fg.alpha(v.fg.a * 0.25) },
            &DOT,
        );
        let scale = anim::transition(ctx, id.with("scale"), if on { 1.25f32 } else { 1.0 }, &DOT);
        BoxStyle {
            background: bg,
            radius: Corners::all(3.0),
            ..BoxStyle::default()
        }
        .paint_transformed(
            ui.painter(),
            Rect::from_min_size(pos2(x0 + i as f32 * 12.0, at.y), vec2(6.0, 6.0)),
            Transform::scale(scale),
        );
    }
}

// ---------------------------------------------------------------- buttons

/// The dialog's buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// `.ghost`: bordered, `--fg-muted`, 12 px 600, `padding: 9px 14px`.
    Ghost,
    /// `.primary`: `--fg` fill, `--bg-solid` text, 13 px 700, `padding:
    /// 10px 18px`; hover lifts 1 px at 0.9 opacity.
    Primary,
    /// `.learn-more`: an underlined 12 px link.
    LearnMore,
    /// `.persist-error .retry`: red, bordered, 11 px 700.
    Retry,
    /// `.persist-error .later`: 11 px `--fg-muted`.
    Later,
}

impl Kind {
    fn font(self) -> TextStyle {
        let (size, weight) = match self {
            Kind::Ghost => (12.0, 600),
            Kind::Primary => (13.0, 700),
            Kind::LearnMore => (12.0, 400),
            Kind::Retry => (11.0, 700),
            Kind::Later => (11.0, 400),
        };
        // The error row's `line-height: 1.4` reaches its buttons.
        let line = if matches!(self, Kind::Retry | Kind::Later) {
            1.4
        } else {
            1.5
        };
        TextStyle::new(size, weight).line_height(LineHeight::Number(line))
    }

    /// `padding` left/right and top/bottom.
    fn padding(self) -> Vec2 {
        match self {
            Kind::Ghost => vec2(14.0, 9.0),
            Kind::Primary => vec2(18.0, 10.0),
            Kind::LearnMore => Vec2::ZERO,
            Kind::Retry => vec2(10.0, 4.0),
            Kind::Later => vec2(6.0, 4.0),
        }
    }

    fn border(self) -> f32 {
        match self {
            Kind::Ghost | Kind::Retry => 1.0,
            _ => 0.0,
        }
    }

    fn radius(self) -> f32 {
        match self {
            Kind::Ghost | Kind::Primary => 10.0,
            Kind::Retry => 8.0,
            _ => 0.0,
        }
    }

    /// The border box of a button labelled `label`.
    fn size(self, ctx: &Context, label: &str) -> Vec2 {
        let ppp = ctx.pixels_per_point();
        let font = self.font();
        let w = snap::content_width(text::measure(ctx, label, &font).unwrap_or(0.0), ppp);
        let line = font.line_box(ctx).map_or(font.size * 1.5, |l| l.height);
        let b = snap::border_width(self.border(), ppp);
        vec2(w, line) + 2.0 * self.padding() + Vec2::splat(2.0 * b)
    }
}

/// `transition: color 120ms ease-out, border-color 120ms ease-out` and the
/// primary's opacity and transform.
const T120: Transition = Transition::new(0.12, Timing::EASE_OUT);

/// Draws one button into `rect`; true when activated.
fn button(
    ui: &Ui,
    props: &OnboardingProps,
    id: Id,
    rect: Rect,
    kind: Kind,
    label: &str,
    enabled: bool,
) -> bool {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui.interact(snap::rect(rect, ppp), id, sense);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    let focused = enabled && take_focus(ui, &response);
    let hovered = enabled && response.hovered();
    let (color, border_color, background) = match kind {
        Kind::Ghost => (
            if hovered { v.fg } else { v.fg_muted },
            if hovered {
                v.fg.mix(0.35, v.border)
            } else {
                v.border
            },
            Rgba::TRANSPARENT,
        ),
        Kind::Primary => (v.bg_solid, Rgba::TRANSPARENT, v.fg),
        Kind::LearnMore => (
            if hovered { v.fg } else { v.fg_muted },
            Rgba::TRANSPARENT,
            Rgba::TRANSPARENT,
        ),
        Kind::Retry => (RED, RED.mix(0.55, v.border), Rgba::TRANSPARENT),
        Kind::Later => (
            if hovered { v.fg } else { v.fg_muted },
            Rgba::TRANSPARENT,
            Rgba::TRANSPARENT,
        ),
    };
    let eased = matches!(kind, Kind::Ghost | Kind::LearnMore);
    let (color, border_color) = if eased {
        (
            anim::transition(&ctx, id.with("fg"), color, &T120),
            anim::transition(&ctx, id.with("border"), border_color, &T120),
        )
    } else {
        (color, border_color)
    };
    // `.primary:hover { opacity: .9; transform: translateY(-1px) }`, not
    // under reduced motion for the transform.
    let lift_on = kind == Kind::Primary && hovered;
    let hover_k = anim::transition_bool(&ctx, id.with("lift"), lift_on, &T120);
    let reduced = anim::motion(&ctx) == anim::Motion::Reduced;
    let lift = if reduced { 0.0 } else { -hover_k };
    let mut painter = ui.painter().clone();
    if !enabled {
        painter.multiply_opacity(0.5);
    }
    if kind == Kind::Primary {
        painter.multiply_opacity(1.0 - 0.1 * hover_k);
    }
    let b = snap::border_width(kind.border(), ppp);
    BoxStyle {
        background,
        radius: Corners::all(kind.radius()),
        border: Border::all(b, border_color),
        ..BoxStyle::default()
    }
    .paint_transformed(&painter, rect, Transform::translate(vec2(0.0, lift)));
    let font = kind.font();
    let line = font.line_box(&ctx).map_or(font.size * 1.5, |l| l.height);
    let snapped = snap::rect(rect, ppp);
    // The label sits where layout puts it, unsnapped: after the border and
    // padding, centred in the height a stretched row gives the button.
    let pad = kind.padding();
    let text_top = rect.top() + b + (rect.height() - 2.0 * b - line) / 2.0 + lift;
    let text_rect = Rect::from_min_size(
        pos2(rect.left() + b + pad.x, text_top),
        vec2((rect.width() - 2.0 * (b + pad.x)).max(0.0), line),
    );
    let laid = text::paint(
        &painter,
        text_rect,
        label,
        &font
            .clone()
            .color(color.to_color32())
            .align(TextAlign::Center)
            .nowrap(false),
    );
    if kind == Kind::LearnMore {
        if let Some(l) = laid {
            // `text-decoration: underline`: one device pixel, its top a
            // device pixel under the baseline.
            let t = snap::border_width(1.0, ppp).max(1.0 / ppp);
            let r = Rect::from_min_size(
                pos2(
                    text_rect.center().x - l.width / 2.0,
                    text_top + l.line_box.baseline + 1.0 / ppp,
                ),
                vec2(l.width, t),
            );
            painter.rect_filled(snap::rect(r, ppp), 0.0, color.to_color32());
        }
    }
    if focused && props.focus_visible {
        paint_ua_focus_ring(
            ui.painter(),
            snapped,
            kind.radius(),
            kind.border(),
            props.dark,
        );
    }
    let keyed =
        focused && ui.input(|i| i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter));
    enabled && (response.clicked() || keyed)
}

/// Height of `text` wrapped to `width` in `style`.
fn text_height(ctx: &Context, s: &str, style: &TextStyle, width: f32) -> f32 {
    let one = style.line_box(ctx).map_or(style.size * 1.5, |l| l.height);
    text::layout(
        ctx,
        s,
        style,
        Rect::from_min_size(pos2(0.0, 0.0), vec2(width, one)),
    )
    .map_or(one, |l| l.height)
}
