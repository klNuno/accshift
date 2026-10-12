//! CSS animation timing: timing functions, transitions and keyframe
//! animations, with the motion preference of `.agents/rules/animations.md`.
//!
//! - [`Timing`] evaluates `ease`, `linear`, `ease-in`, `ease-out`,
//!   `ease-in-out`, `cubic-bezier()` (solved like Chromium's `gfx::CubicBezier`)
//!   and `steps()`.
//! - [`transition`] follows one value keyed by an [`egui::Id`] and animates it
//!   when its target changes, retargeting mid-flight with the reversing rules
//!   of CSS Transitions 1.
//! - [`animate`] plays [`Keyframes`] with an [`Animation`] (duration, delay,
//!   iteration count, direction, fill mode), sampled through the Web
//!   Animations timing model.
//! - [`set_motion`] with [`Motion::Reduced`] is `html[data-motion="reduced"]`:
//!   transitions jump to their target and keyframe animations show the
//!   underlying value, as `animation: none` would.
//!
//! Time is egui's `input.time` in seconds. Both entry points request a repaint
//! only while something moves (or when a delay ends), never otherwise.

use egui::{Color32, Context, Id, Pos2, Vec2};

use crate::theme::Rgba;

// ---------------------------------------------------------------- timing

/// A CSS `<easing-function>`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Timing {
    Linear,
    CubicBezier(CubicBezier),
    Steps(u32, StepPosition),
}

/// `steps()` jump positions. `start` and `end` are the legacy names of
/// `jump-start` and `jump-end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepPosition {
    JumpStart,
    JumpEnd,
    JumpNone,
    JumpBoth,
}

impl Timing {
    pub const LINEAR: Timing = Timing::Linear;
    pub const EASE: Timing = Timing::CubicBezier(CubicBezier::new(0.25, 0.1, 0.25, 1.0));
    pub const EASE_IN: Timing = Timing::CubicBezier(CubicBezier::new(0.42, 0.0, 1.0, 1.0));
    pub const EASE_OUT: Timing = Timing::CubicBezier(CubicBezier::new(0.0, 0.0, 0.58, 1.0));
    pub const EASE_IN_OUT: Timing = Timing::CubicBezier(CubicBezier::new(0.42, 0.0, 0.58, 1.0));

    pub const fn cubic_bezier(x1: f64, y1: f64, x2: f64, y2: f64) -> Timing {
        Timing::CubicBezier(CubicBezier::new(x1, y1, x2, y2))
    }

    /// Output progress for an input progress. Inputs outside 0..1 are
    /// extrapolated the way Chromium does for cubic curves.
    pub fn eval(&self, x: f64) -> f64 {
        match self {
            Timing::Linear => x,
            Timing::CubicBezier(curve) => curve.solve(x),
            Timing::Steps(steps, position) => steps_eval(*steps, *position, x),
        }
    }

    /// Parse a CSS easing value: a keyword, `cubic-bezier(a, b, c, d)` or
    /// `steps(n[, position])`.
    pub fn parse(css: &str) -> Option<Timing> {
        let s = css.trim();
        match s {
            "linear" => return Some(Timing::LINEAR),
            "ease" => return Some(Timing::EASE),
            "ease-in" => return Some(Timing::EASE_IN),
            "ease-out" => return Some(Timing::EASE_OUT),
            "ease-in-out" => return Some(Timing::EASE_IN_OUT),
            "step-start" => return Some(Timing::Steps(1, StepPosition::JumpStart)),
            "step-end" => return Some(Timing::Steps(1, StepPosition::JumpEnd)),
            _ => {}
        }
        let (name, args) = s.strip_suffix(')')?.split_once('(')?;
        let args: Vec<&str> = args.split(',').map(str::trim).collect();
        match name.trim() {
            "cubic-bezier" if args.len() == 4 => {
                let v: Vec<f64> = args.iter().filter_map(|a| a.parse().ok()).collect();
                if v.len() != 4 || !(0.0..=1.0).contains(&v[0]) || !(0.0..=1.0).contains(&v[2]) {
                    return None;
                }
                Some(Timing::cubic_bezier(v[0], v[1], v[2], v[3]))
            }
            "steps" if !args.is_empty() && args.len() <= 2 => {
                let n: u32 = args[0].parse().ok()?;
                let position = match args.get(1).copied().unwrap_or("end") {
                    "jump-start" | "start" => StepPosition::JumpStart,
                    "jump-end" | "end" => StepPosition::JumpEnd,
                    "jump-none" => StepPosition::JumpNone,
                    "jump-both" => StepPosition::JumpBoth,
                    _ => return None,
                };
                let min = if position == StepPosition::JumpNone {
                    2
                } else {
                    1
                };
                (n >= min).then_some(Timing::Steps(n, position))
            }
            _ => None,
        }
    }
}

/// CSS Easing 1, step easing function (the before flag is not tracked: an
/// animation sampled before its start uses its fill value instead).
fn steps_eval(steps: u32, position: StepPosition, x: f64) -> f64 {
    let n = f64::from(steps);
    let mut current = (x * n).floor();
    if matches!(position, StepPosition::JumpStart | StepPosition::JumpBoth) {
        current += 1.0;
    }
    if x >= 0.0 && current < 0.0 {
        current = 0.0;
    }
    let jumps = match position {
        StepPosition::JumpStart | StepPosition::JumpEnd => n,
        StepPosition::JumpNone => n - 1.0,
        StepPosition::JumpBoth => n + 1.0,
    };
    if x <= 1.0 && current > jumps {
        current = jumps;
    }
    current / jumps
}

/// A cubic Bézier easing curve from (0, 0) to (1, 1), solved like
/// `ui/gfx/geometry/cubic_bezier.cc`: four Newton steps, then bisection, with
/// linear extrapolation outside 0..1 along the end tangents.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CubicBezier {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

const BEZIER_EPSILON: f64 = 1e-7;
const MAX_NEWTON_ITERATIONS: usize = 4;

impl CubicBezier {
    pub const fn new(x1: f64, y1: f64, x2: f64, y2: f64) -> Self {
        Self { x1, y1, x2, y2 }
    }

    fn coefficients(p1: f64, p2: f64) -> (f64, f64, f64) {
        let c = 3.0 * p1;
        let b = 3.0 * (p2 - p1) - c;
        let a = 1.0 - c - b;
        (a, b, c)
    }

    fn sample(p1: f64, p2: f64, t: f64) -> f64 {
        let (a, b, c) = Self::coefficients(p1, p2);
        ((a * t + b) * t + c) * t
    }

    fn sample_derivative_x(&self, t: f64) -> f64 {
        let (a, b, c) = Self::coefficients(self.x1, self.x2);
        (3.0 * a * t + 2.0 * b) * t + c
    }

    fn start_gradient(&self) -> f64 {
        if self.x1 > 0.0 {
            self.y1 / self.x1
        } else if self.y1 == 0.0 && self.x2 > 0.0 {
            self.y2 / self.x2
        } else if self.y1 == 0.0 && self.y2 == 0.0 {
            1.0
        } else {
            0.0
        }
    }

    fn end_gradient(&self) -> f64 {
        if self.x2 < 1.0 {
            (self.y2 - 1.0) / (self.x2 - 1.0)
        } else if self.y2 == 1.0 && self.x1 < 1.0 {
            (self.y1 - 1.0) / (self.x1 - 1.0)
        } else if self.y2 == 1.0 && self.y1 == 1.0 {
            1.0
        } else {
            0.0
        }
    }

    /// The curve parameter t whose x is `x`, for x in 0..1.
    pub fn solve_curve_x(&self, x: f64) -> f64 {
        let mut t2 = x;
        for _ in 0..MAX_NEWTON_ITERATIONS {
            let x2 = Self::sample(self.x1, self.x2, t2) - x;
            if x2.abs() < BEZIER_EPSILON {
                return t2;
            }
            let d2 = self.sample_derivative_x(t2);
            if d2.abs() < 1e-6 {
                break;
            }
            t2 -= x2 / d2;
        }
        let (mut t0, mut t1) = (0.0, 1.0);
        t2 = x;
        while t0 < t1 {
            let x2 = Self::sample(self.x1, self.x2, t2);
            if (x2 - x).abs() < BEZIER_EPSILON {
                return t2;
            }
            if x > x2 {
                t0 = t2;
            } else {
                t1 = t2;
            }
            let next = (t1 + t0) * 0.5;
            if next == t2 {
                break;
            }
            t2 = next;
        }
        t2
    }

    pub fn solve(&self, x: f64) -> f64 {
        if x < 0.0 {
            return self.start_gradient() * x;
        }
        if x > 1.0 {
            return 1.0 + self.end_gradient() * (x - 1.0);
        }
        Self::sample(self.y1, self.y2, self.solve_curve_x(x))
    }
}

// ---------------------------------------------------------------- values

/// A value CSS can interpolate.
pub trait Animatable: Clone + PartialEq + Send + Sync + 'static {
    /// `self` at t = 0, `to` at t = 1. Timing functions may push t outside
    /// 0..1 (overshoot); implementations extrapolate.
    fn lerp(&self, to: &Self, t: f32) -> Self;
}

impl Animatable for f32 {
    fn lerp(&self, to: &Self, t: f32) -> Self {
        self + (to - self) * t
    }
}

impl Animatable for Vec2 {
    fn lerp(&self, to: &Self, t: f32) -> Self {
        *self + (*to - *self) * t
    }
}

impl Animatable for Pos2 {
    fn lerp(&self, to: &Self, t: f32) -> Self {
        *self + (*to - *self) * t
    }
}

/// CSS interpolates colours in premultiplied sRGB.
impl Animatable for Rgba {
    fn lerp(&self, to: &Self, t: f32) -> Self {
        let a = (self.a + (to.a - self.a) * t).clamp(0.0, 1.0);
        if a <= 0.0 {
            return Rgba::TRANSPARENT;
        }
        let ch = |x: f32, y: f32| (x * self.a + (y * to.a - x * self.a) * t) / a;
        Rgba::new(ch(self.r, to.r), ch(self.g, to.g), ch(self.b, to.b), a)
    }
}

/// Premultiplied already: a plain lerp is the CSS interpolation.
impl Animatable for Color32 {
    fn lerp(&self, to: &Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let c = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
        Color32::from_rgba_premultiplied(
            c(self.r(), to.r()),
            c(self.g(), to.g()),
            c(self.b(), to.b()),
            c(self.a(), to.a()),
        )
    }
}

// ---------------------------------------------------------------- motion

/// The `data-motion` attribute on `<html>`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Motion {
    #[default]
    Full,
    Reduced,
}

fn motion_id() -> Id {
    Id::new("accshift::anim::motion")
}

pub fn set_motion(ctx: &Context, motion: Motion) {
    ctx.data_mut(|d| d.insert_temp(motion_id(), motion));
}

pub fn motion(ctx: &Context) -> Motion {
    ctx.data(|d| d.get_temp(motion_id())).unwrap_or_default()
}

fn now(ctx: &Context) -> f64 {
    ctx.input(|i| i.time)
}

fn request_frame_at(ctx: &Context, now: f64, at: f64) {
    if at <= now {
        ctx.request_repaint();
    } else {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(at - now));
    }
}

// ---------------------------------------------------------------- transitions

/// `transition: <property> <duration> <timing> <delay>`, durations in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transition {
    pub duration: f64,
    pub delay: f64,
    pub timing: Timing,
}

impl Transition {
    pub const fn new(duration: f64, timing: Timing) -> Self {
        Self {
            duration,
            delay: 0.0,
            timing,
        }
    }

    pub const fn with_delay(self, delay: f64) -> Self {
        Self { delay, ..self }
    }
}

/// One running or settled transition, as CSS Transitions 1 keeps it.
#[derive(Clone, Debug, PartialEq)]
pub struct TransitionState<T> {
    pub start_value: T,
    pub end_value: T,
    /// Start time including the delay (the delay may be negative).
    pub start_time: f64,
    pub duration: f64,
    pub timing: Timing,
    pub reversing_adjusted_start: T,
    pub reversing_shortening: f64,
}

impl<T: Animatable> TransitionState<T> {
    /// A settled value: no transition running.
    pub fn settled(value: T) -> Self {
        Self {
            start_value: value.clone(),
            end_value: value.clone(),
            start_time: f64::NEG_INFINITY,
            duration: 0.0,
            timing: Timing::LINEAR,
            reversing_adjusted_start: value,
            reversing_shortening: 1.0,
        }
    }

    fn input_progress(&self, now: f64) -> f64 {
        if now < self.start_time {
            0.0
        } else if self.duration <= 0.0 {
            1.0
        } else {
            ((now - self.start_time) / self.duration).min(1.0)
        }
    }

    pub fn running(&self, now: f64) -> bool {
        self.input_progress(now) < 1.0 && self.start_value != self.end_value
    }

    /// The value at `now`.
    pub fn value(&self, now: f64) -> T {
        let p = self.input_progress(now);
        if p >= 1.0 {
            return self.end_value.clone();
        }
        let eased = self.timing.eval(p) as f32;
        self.start_value.lerp(&self.end_value, eased)
    }

    /// The style change event of CSS Transitions 1, section 3, for a new
    /// target: nothing when it is the current end value, a shortened reverse
    /// when it is the reversing-adjusted start of a running transition, a
    /// fresh transition from the current value otherwise.
    pub fn retarget(&mut self, target: T, spec: &Transition, now: f64) {
        if target == self.end_value {
            return;
        }
        let current = self.value(now);
        if spec.duration + spec.delay.min(0.0) <= 0.0 {
            *self = Self::settled(target);
            return;
        }
        if self.running(now) && target == self.reversing_adjusted_start {
            let old_output = self.timing.eval(self.input_progress(now));
            let factor = (old_output * self.reversing_shortening + 1.0 - self.reversing_shortening)
                .abs()
                .clamp(0.0, 1.0);
            let delay = if spec.delay < 0.0 {
                spec.delay * factor
            } else {
                spec.delay
            };
            *self = Self {
                reversing_adjusted_start: self.end_value.clone(),
                start_value: current,
                end_value: target,
                start_time: now + delay,
                duration: spec.duration * factor,
                timing: spec.timing,
                reversing_shortening: factor,
            };
            return;
        }
        *self = Self {
            reversing_adjusted_start: current.clone(),
            start_value: current,
            end_value: target,
            start_time: now + spec.delay,
            duration: spec.duration,
            timing: spec.timing,
            reversing_shortening: 1.0,
        };
    }
}

/// The current value of a transitioned property whose target is `target`.
/// The first call for an id shows the target without animating, like an
/// element's first style. Under [`Motion::Reduced`] the target shows at once.
pub fn transition<T: Animatable>(ctx: &Context, id: Id, target: T, spec: &Transition) -> T {
    let now = now(ctx);
    if motion(ctx) == Motion::Reduced {
        ctx.data_mut(|d| d.insert_temp(id, TransitionState::settled(target.clone())));
        return target;
    }
    let mut state = ctx
        .data(|d| d.get_temp::<TransitionState<T>>(id))
        .unwrap_or_else(|| TransitionState::settled(target.clone()));
    state.retarget(target, spec, now);
    let value = state.value(now);
    if state.running(now) {
        request_frame_at(ctx, now, state.start_time);
    }
    ctx.data_mut(|d| d.insert_temp(id, state));
    value
}

/// A boolean state transitioned as 0.0 or 1.0, the common hover case.
pub fn transition_bool(ctx: &Context, id: Id, on: bool, spec: &Transition) -> f32 {
    transition(ctx, id, if on { 1.0f32 } else { 0.0 }, spec)
}

// ---------------------------------------------------------------- keyframes

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    #[default]
    Normal,
    Reverse,
    Alternate,
    AlternateReverse,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Fill {
    #[default]
    None,
    Forwards,
    Backwards,
    Both,
}

/// `animation: <duration> <timing> <delay> <iterations> <direction> <fill>`.
/// `timing` applies to every keyframe interval that has no timing of its own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Animation {
    pub duration: f64,
    pub delay: f64,
    /// `f64::INFINITY` for `infinite`.
    pub iterations: f64,
    pub direction: Direction,
    pub fill: Fill,
    pub timing: Timing,
}

impl Animation {
    pub const fn new(duration: f64, timing: Timing) -> Self {
        Self {
            duration,
            delay: 0.0,
            iterations: 1.0,
            direction: Direction::Normal,
            fill: Fill::None,
            timing,
        }
    }

    pub const fn with_delay(self, delay: f64) -> Self {
        Self { delay, ..self }
    }

    pub const fn with_fill(self, fill: Fill) -> Self {
        Self { fill, ..self }
    }

    pub const fn with_iterations(self, iterations: f64) -> Self {
        Self { iterations, ..self }
    }

    pub const fn with_direction(self, direction: Direction) -> Self {
        Self { direction, ..self }
    }

    pub fn infinite(self) -> Self {
        self.with_iterations(f64::INFINITY)
    }

    fn active_duration(&self) -> f64 {
        if self.duration <= 0.0 || self.iterations <= 0.0 {
            0.0
        } else {
            self.duration * self.iterations
        }
    }

    /// End of the active interval, from the animation's start.
    pub fn end_time(&self) -> f64 {
        (self.delay + self.active_duration()).max(0.0)
    }

    /// The directed progress (0..1 within the current iteration, after
    /// direction) at `local` seconds since the animation started, or `None`
    /// when the fill mode leaves the property alone.
    pub fn progress(&self, local: f64) -> Option<f64> {
        let active = self.active_duration();
        let overall = if local < self.delay {
            if !matches!(self.fill, Fill::Backwards | Fill::Both) {
                return None;
            }
            0.0
        } else if local < self.delay + active {
            (local - self.delay) / self.duration
        } else {
            if !matches!(self.fill, Fill::Forwards | Fill::Both) {
                return None;
            }
            self.iterations.max(0.0)
        };
        let (iteration, mut simple) = if overall.is_infinite() {
            (f64::INFINITY, 1.0)
        } else {
            let iteration = overall.floor();
            let frac = overall - iteration;
            if frac == 0.0 && overall > 0.0 && overall >= self.iterations {
                (iteration - 1.0, 1.0)
            } else {
                (iteration, frac)
            }
        };
        let odd = iteration.is_finite() && (iteration as i64) % 2 == 1;
        let reversed = match self.direction {
            Direction::Normal => false,
            Direction::Reverse => true,
            Direction::Alternate => odd,
            Direction::AlternateReverse => !odd,
        };
        if reversed {
            simple = 1.0 - simple;
        }
        Some(simple)
    }
}

/// One `@keyframes` stop: `offset` in 0..1, an optional per-interval timing.
#[derive(Clone, Debug, PartialEq)]
pub struct Keyframe<T> {
    pub offset: f64,
    pub value: T,
    pub timing: Option<Timing>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Keyframes<T> {
    pub frames: Vec<Keyframe<T>>,
}

impl<T: Animatable> Keyframes<T> {
    /// Keyframes sorted by offset, without per-interval timings.
    pub fn new(frames: impl IntoIterator<Item = (f64, T)>) -> Self {
        let mut frames: Vec<Keyframe<T>> = frames
            .into_iter()
            .map(|(offset, value)| Keyframe {
                offset,
                value,
                timing: None,
            })
            .collect();
        frames.sort_by(|a, b| a.offset.total_cmp(&b.offset));
        Self { frames }
    }

    /// `from { x } to { y }`.
    pub fn from_to(from: T, to: T) -> Self {
        Self::new([(0.0, from), (1.0, to)])
    }

    /// The value at a directed progress. A missing 0 % or 100 % keyframe takes
    /// the underlying value `base`, as CSS does.
    pub fn sample(&self, progress: f64, default_timing: Timing, base: &T) -> T {
        let first = self.frames.first();
        let last = self.frames.last();
        let start = match first {
            Some(f) if f.offset <= 0.0 => None,
            _ => Some(Keyframe {
                offset: 0.0,
                value: base.clone(),
                timing: None,
            }),
        };
        let end = match last {
            Some(f) if f.offset >= 1.0 => None,
            _ => Some(Keyframe {
                offset: 1.0,
                value: base.clone(),
                timing: None,
            }),
        };
        let frames: Vec<&Keyframe<T>> = start
            .iter()
            .chain(self.frames.iter())
            .chain(end.iter())
            .collect();
        let mut i = 0;
        while i + 2 < frames.len() && progress >= frames[i + 1].offset {
            i += 1;
        }
        let (a, b) = (frames[i], frames[i + 1]);
        let span = b.offset - a.offset;
        let local = if span <= 0.0 {
            1.0
        } else {
            (progress - a.offset) / span
        };
        let eased = a.timing.unwrap_or(default_timing).eval(local);
        a.value.lerp(&b.value, eased as f32)
    }
}

/// When the animation keyed by `id` started, recorded on its first call.
fn animation_start(ctx: &Context, id: Id, now: f64) -> f64 {
    let key = id.with("accshift::anim::start");
    ctx.data_mut(|d| *d.get_temp_mut_or_insert_with(key, || now))
}

/// Restart the animation keyed by `id` on its next call, like a remount.
pub fn restart(ctx: &Context, id: Id) {
    let key = id.with("accshift::anim::start");
    ctx.data_mut(|d| d.remove::<f64>(key));
}

/// The value of a property animated with `keyframes`, `base` being its
/// underlying (unanimated) value. The animation starts on the first call for
/// `id`. Under [`Motion::Reduced`] it is `animation: none`.
pub fn animate<T: Animatable>(
    ctx: &Context,
    id: Id,
    keyframes: &Keyframes<T>,
    animation: &Animation,
    base: T,
) -> T {
    if motion(ctx) == Motion::Reduced {
        return base;
    }
    let now = now(ctx);
    let start = animation_start(ctx, id, now);
    let local = now - start;
    if local < animation.end_time() || animation.iterations.is_infinite() {
        request_frame_at(ctx, now, start + animation.delay);
    }
    match animation.progress(local) {
        Some(progress) => keyframes.sample(progress, animation.timing, &base),
        None => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    /// Reference values from a 200-step bisection of the curve in f64 (the
    /// same numbers WebKit's and Chromium's tests use for `ease`).
    #[test]
    fn named_curves_match_reference_values() {
        let cases: [(Timing, [f64; 5]); 4] = [
            (
                Timing::EASE,
                [
                    0.094796306,
                    0.408510591,
                    0.802403388,
                    0.960458978,
                    0.994316477,
                ],
            ),
            (
                Timing::EASE_IN,
                [
                    0.01702661,
                    0.093464651,
                    0.315356813,
                    0.621861869,
                    0.839427846,
                ],
            ),
            (
                Timing::EASE_OUT,
                [
                    0.160572154,
                    0.378138131,
                    0.684643187,
                    0.906535349,
                    0.98297339,
                ],
            ),
            (
                Timing::EASE_IN_OUT,
                [0.019722454, 0.129161931, 0.5, 0.870838069, 0.980277546],
            ),
        ];
        for (timing, want) in cases {
            for (x, w) in [0.1, 0.25, 0.5, 0.75, 0.9].iter().zip(want) {
                let got = timing.eval(*x);
                assert!(close(got, w, 1e-6), "{timing:?}({x}) = {got}, want {w}");
            }
        }
    }

    #[test]
    fn custom_and_overshooting_curves() {
        let custom = Timing::parse("cubic-bezier(0.2, 0.8, 0.2, 1)").unwrap();
        for (x, w) in [(0.1, 0.396693549), (0.5, 0.946079462), (0.9, 0.998830259)] {
            assert!(close(custom.eval(x), w, 1e-6), "custom({x})");
        }
        let back = Timing::cubic_bezier(0.34, 1.56, 0.64, 1.0);
        for (x, w) in [(0.25, 0.816289199), (0.5, 1.08740067), (0.75, 1.05964686)] {
            assert!(close(back.eval(x), w, 1e-6), "overshoot({x})");
        }
    }

    #[test]
    fn endpoints_and_extrapolation() {
        for t in [
            Timing::EASE,
            Timing::EASE_IN,
            Timing::EASE_OUT,
            Timing::EASE_IN_OUT,
        ] {
            assert!(close(t.eval(0.0), 0.0, 1e-9));
            assert!(close(t.eval(1.0), 1.0, 1e-9));
        }
        // ease starts with slope 0.1 / 0.25 and ends flat.
        assert!(close(Timing::EASE.eval(-0.5), -0.2, 1e-9));
        assert!(close(Timing::EASE.eval(1.5), 1.0, 1e-9));
    }

    #[test]
    fn steps_follow_css_easing() {
        let end = Timing::parse("steps(4)").unwrap();
        assert_eq!(end.eval(0.0), 0.0);
        assert_eq!(end.eval(0.24), 0.0);
        assert_eq!(end.eval(0.25), 0.25);
        assert_eq!(end.eval(0.99), 0.75);
        assert_eq!(end.eval(1.0), 1.0);
        let start = Timing::parse("steps(4, jump-start)").unwrap();
        assert_eq!(start.eval(0.0), 0.25);
        assert_eq!(start.eval(0.8), 1.0);
        let none = Timing::parse("steps(5, jump-none)").unwrap();
        assert_eq!(none.eval(0.0), 0.0);
        assert_eq!(none.eval(0.5), 0.5);
        assert_eq!(none.eval(1.0), 1.0);
        let both = Timing::parse("steps(3, jump-both)").unwrap();
        assert_eq!(both.eval(0.0), 0.25);
        assert_eq!(both.eval(0.99), 0.75);
        assert_eq!(both.eval(1.0), 1.0);
        assert!(Timing::parse("steps(1, jump-none)").is_none());
    }

    #[test]
    fn transition_reverses_with_shortening() {
        let spec = Transition::new(0.2, Timing::LINEAR);
        let mut s = TransitionState::settled(0.0f32);
        s.retarget(1.0, &spec, 0.0);
        assert!(close(f64::from(s.value(0.05)), 0.25, 1e-6));
        // Reversal at 25 %: back to 0 in a quarter of the duration.
        s.retarget(0.0, &spec, 0.05);
        assert!(close(s.duration, 0.05, 1e-9));
        assert!(close(f64::from(s.value(0.075)), 0.125, 1e-6));
        assert_eq!(s.value(0.1), 0.0);
        // A new target that is not a reversal runs the full duration.
        let mut s = TransitionState::settled(0.0f32);
        s.retarget(1.0, &spec, 0.0);
        s.retarget(2.0, &spec, 0.1);
        assert!(close(s.duration, 0.2, 1e-9));
        assert!(close(f64::from(s.value(0.1)), 0.5, 1e-6));
        assert!(close(f64::from(s.value(0.2)), 1.25, 1e-6));
    }

    #[test]
    fn transition_delay_holds_the_start_value() {
        let spec = Transition::new(0.1, Timing::LINEAR).with_delay(0.05);
        let mut s = TransitionState::settled(0.0f32);
        s.retarget(1.0, &spec, 1.0);
        assert_eq!(s.value(1.04), 0.0);
        assert!(close(f64::from(s.value(1.1)), 0.5, 1e-6));
        assert!(s.running(1.1));
        assert!(!s.running(1.2));
    }

    #[test]
    fn keyframes_fill_modes_and_direction() {
        let kf = Keyframes::from_to(0.0f32, 1.0);
        let base = 5.0f32;
        let anim = Animation::new(1.0, Timing::LINEAR).with_delay(0.5);
        let at = |a: &Animation, t: f64| {
            a.progress(t)
                .map(|p| kf.sample(p, a.timing, &base))
                .unwrap_or(base)
        };
        assert_eq!(at(&anim, 0.2), 5.0);
        assert_eq!(at(&anim, 1.0), 0.5);
        assert_eq!(at(&anim, 2.0), 5.0);
        let both = anim.with_fill(Fill::Both);
        assert_eq!(at(&both, 0.2), 0.0);
        assert_eq!(at(&both, 2.0), 1.0);
        let alt = Animation::new(1.0, Timing::LINEAR)
            .with_iterations(2.0)
            .with_direction(Direction::Alternate)
            .with_fill(Fill::Forwards);
        assert_eq!(at(&alt, 0.25), 0.25);
        assert_eq!(at(&alt, 1.25), 0.75);
        assert_eq!(at(&alt, 3.0), 0.0);
        let spin = Animation::new(1.0, Timing::LINEAR).infinite();
        assert!(close(spin.progress(10.25).unwrap(), 0.25, 1e-9));
    }

    #[test]
    fn keyframe_timing_applies_per_interval() {
        // card-entrance: from { opacity 0 } to { opacity 1 }, ease-out.
        let kf = Keyframes::from_to(0.0f32, 1.0);
        let v = kf.sample(0.5, Timing::EASE_OUT, &1.0);
        assert!(close(f64::from(v), 0.684643187, 1e-6));
        // A missing 100 % keyframe uses the underlying value.
        let half = Keyframes::new([(0.0, 0.0f32)]);
        assert!(close(
            f64::from(half.sample(0.5, Timing::LINEAR, &2.0)),
            1.0,
            1e-6
        ));
    }

    #[test]
    fn reduced_motion_disables_everything() {
        let ctx = Context::default();
        set_motion(&ctx, Motion::Reduced);
        let id = Id::new("t");
        let spec = Transition::new(1.0, Timing::EASE);
        assert_eq!(transition(&ctx, id, 0.0f32, &spec), 0.0);
        assert_eq!(transition(&ctx, id, 1.0f32, &spec), 1.0);
        let kf = Keyframes::from_to(0.0f32, 1.0);
        let anim = Animation::new(1.0, Timing::LINEAR).with_fill(Fill::Both);
        assert_eq!(animate(&ctx, id, &kf, &anim, 7.0), 7.0);
    }

    #[test]
    fn colours_interpolate_premultiplied() {
        // transparent black to opaque white: halfway is white at 50 %.
        let a = Rgba::TRANSPARENT;
        let b = Rgba::new(255.0, 255.0, 255.0, 1.0);
        let m = Animatable::lerp(&a, &b, 0.5);
        assert!((m.r - 255.0).abs() < 1e-3 && (m.a - 0.5).abs() < 1e-6);
    }
}
