//! Chromium's animated wheel scrolling on Windows. A mouse wheel notch (a
//! line or a page delta) moves a target the offset eases toward, and a new
//! notch moves the target again; a pixel delta (a precision touchpad, the
//! scene runner's CDP wheel) moves the offset at once, as Chromium does.
//!
//! Without it a notch jumped 100 px in one frame, which reads as a stutter
//! however fast the frames are.

use egui::{Context, Event, Id, MouseWheelUnit};

/// The time constant of the ease: 95 % of a notch in 150 ms, the feel of
/// Chromium's wheel animation.
const TAU: f64 = 0.05;

/// Wheel deltas of one frame, in points, positive when the wheel turns up
/// (the content moves down).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Wheel {
    /// Line and page deltas: animated.
    pub notch: f32,
    /// Point deltas: applied at once.
    pub pixels: f32,
}

impl Wheel {
    /// Adds one wheel event, a line being `line` points and a page `page`.
    pub fn add(&mut self, event: &Event, line: f32, page: f32) {
        if let Event::MouseWheel { unit, delta, .. } = event {
            match unit {
                MouseWheelUnit::Point => self.pixels += delta.y,
                MouseWheelUnit::Line => self.notch += delta.y * line,
                MouseWheelUnit::Page => self.notch += delta.y * page,
            }
        }
    }

    pub fn is_zero(&self) -> bool {
        self.notch == 0.0 && self.pixels == 0.0
    }
}

#[derive(Clone, Copy, Debug)]
struct State {
    target: f32,
    /// The animated offset, before the caller rounds it.
    pos: f32,
    /// What the last frame left in the caller's offset.
    written: f32,
    time: f64,
}

/// Applies this frame's `wheel` to `offset` (scrolled to at most `max`) and
/// advances the running animation of `id`. A caller that moves the offset
/// itself (the thumb, a focus reveal) ends the animation.
pub fn apply(ctx: &Context, id: Id, offset: &mut f32, wheel: Wheel, max: f32) {
    let key = id.with("smooth-scroll");
    let (now, dt) = ctx.input(|i| (i.time, f64::from(i.predicted_dt)));
    let mut state = ctx
        .data(|d| d.get_temp::<State>(key))
        .filter(|s| (s.written - *offset).abs() <= 1.0);
    if wheel.pixels != 0.0 {
        *offset -= wheel.pixels;
        if let Some(s) = &mut state {
            s.target -= wheel.pixels;
            s.pos -= wheel.pixels;
        }
    }
    if wheel.notch != 0.0 {
        let s = state.get_or_insert(State {
            target: *offset,
            pos: *offset,
            written: *offset,
            // The first frame of a notch moves by one frame of the ease.
            time: now - dt,
        });
        s.target = (s.target - wheel.notch).clamp(0.0, max.max(0.0));
    }
    let Some(mut s) = state else {
        ctx.data_mut(|d| d.remove::<State>(key));
        return;
    };
    s.target = s.target.clamp(0.0, max.max(0.0));
    let k = 1.0 - (-(now - s.time).max(0.0) / TAU).exp();
    s.pos += (s.target - s.pos) * k as f32;
    s.time = now;
    if ((s.target - s.pos) * ctx.pixels_per_point()).abs() < 0.5 {
        *offset = s.target;
        ctx.data_mut(|d| d.remove::<State>(key));
    } else {
        *offset = s.pos;
        s.written = s.pos;
        ctx.data_mut(|d| d.insert_temp(key, s));
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{vec2, Modifiers, MouseWheelSource, RawInput, TouchPhase};

    fn wheel(unit: MouseWheelUnit, dy: f32) -> Event {
        Event::MouseWheel {
            unit,
            delta: vec2(0.0, dy),
            phase: TouchPhase::Move,
            source: MouseWheelSource::Wheel,
            modifiers: Modifiers::NONE,
        }
    }

    /// One frame at `t` seconds: the offset after `events`.
    fn frame(ctx: &Context, t: f64, offset: &mut f32, events: &[Event]) {
        let input = RawInput {
            time: Some(t),
            predicted_dt: 1.0 / 144.0,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            let mut w = Wheel::default();
            for e in events {
                w.add(e, 100.0, 400.0);
            }
            apply(ui.ctx(), Id::unique("box"), offset, w, 1000.0);
        });
        out.textures_delta.clear();
    }

    #[test]
    fn a_notch_eases_to_its_target() {
        let ctx = Context::default();
        let mut offset = 0.0;
        frame(&ctx, 0.0, &mut offset, &[wheel(MouseWheelUnit::Line, -1.0)]);
        assert!(offset > 0.0 && offset < 30.0, "first frame moved {offset}");
        let mut t = 0.0;
        let mut last = offset;
        while t < 0.3 {
            t += 1.0 / 144.0;
            frame(&ctx, t, &mut offset, &[]);
            assert!(offset >= last, "monotonic");
            last = offset;
        }
        assert_eq!(offset, 100.0);
    }

    #[test]
    fn notches_add_up_and_pixels_apply_at_once() {
        let ctx = Context::default();
        let mut offset = 0.0;
        frame(&ctx, 0.0, &mut offset, &[wheel(MouseWheelUnit::Line, -1.0)]);
        frame(
            &ctx,
            0.02,
            &mut offset,
            &[wheel(MouseWheelUnit::Line, -1.0)],
        );
        let mut t = 0.02;
        while t < 0.5 {
            t += 1.0 / 144.0;
            frame(&ctx, t, &mut offset, &[]);
        }
        assert_eq!(offset, 200.0);
        frame(
            &ctx,
            1.0,
            &mut offset,
            &[wheel(MouseWheelUnit::Point, 30.0)],
        );
        assert_eq!(offset, 170.0);
    }

    #[test]
    fn moving_the_offset_elsewhere_ends_the_animation() {
        let ctx = Context::default();
        let mut offset = 0.0;
        frame(&ctx, 0.0, &mut offset, &[wheel(MouseWheelUnit::Line, -1.0)]);
        offset = 500.0;
        frame(&ctx, 0.01, &mut offset, &[]);
        assert_eq!(offset, 500.0);
        frame(&ctx, 0.5, &mut offset, &[]);
        assert_eq!(offset, 500.0);
    }

    #[test]
    fn the_target_stops_at_the_ends() {
        let ctx = Context::default();
        let mut offset = 0.0;
        frame(&ctx, 0.0, &mut offset, &[wheel(MouseWheelUnit::Line, 3.0)]);
        assert_eq!(offset, 0.0);
    }
}
