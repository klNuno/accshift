//! The name's fit: measured on hover, and once it overflows, scrolled to
//! its end and back while the hover lasts.

use std::time::Duration;

use super::input::Hover;
use super::measure::name_style;
use super::{Card, State};
use crate::ui::anim;
use crate::ui::components::shared::ceil_unit_tolerant;
use crate::ui::text;

/// The scroll speed in px per second.
const MARQUEE_SPEED: f32 = 42.0;
/// The pause at each end, in seconds.
const MARQUEE_PAUSE: f64 = 2.0;

/// How the name sits on its line this frame.
pub(super) struct NameFit {
    /// The name's max-content width, in layout units.
    pub width: f32,
    /// A hover found the name overflowing: it sits on its text baseline
    /// at its full width and may scroll.
    pub overflowing: bool,
    /// The scroll offset (0 or negative) while the marquee runs.
    pub offset: f32,
}

/// Measures the name and runs the marquee. The overflow is measured when a
/// hover starts and kept until the name or the username changes.
pub(super) fn fit_name(card: &Card, st: &mut State, hover: Hover) -> NameFit {
    let ctx = &card.ctx;
    let item = card.item();
    let name_key = format!("{}|{}", item.name, item.username);
    let width = ceil_unit_tolerant(
        text::measure(ctx, &item.name, &name_style(card.v())).unwrap_or(0.0),
        card.ppp,
    );
    if st
        .measured
        .as_ref()
        .is_some_and(|(key, _)| *key != name_key)
    {
        st.measured = None;
        st.overflowing = false;
    }
    if hover.entered {
        let overflow = match &st.measured {
            Some((_, px)) => *px,
            None => {
                let px = (width.round() - card.content_w().round()).max(0.0);
                st.measured = Some((name_key, px));
                px
            }
        };
        st.overflowing = overflow > 2.0;
        st.marquee_from = ctx.input(|i| i.time);
    }
    let overflowing = st.overflowing;
    if !(hover.hovered && overflowing) {
        return NameFit {
            width,
            overflowing,
            offset: 0.0,
        };
    }
    let shift = st.measured.as_ref().map_or(0.0, |(_, px)| *px);
    let reduced = anim::motion(ctx) == anim::Motion::Reduced;
    let elapsed = ctx.input(|i| i.time) - st.marquee_from;
    // The name rests at either end: no frame until it moves again.
    match marquee_rest(elapsed, shift, reduced) {
        Some(left) => ctx.request_repaint_after(Duration::from_secs_f64(left)),
        None => ctx.request_repaint(),
    }
    NameFit {
        width,
        overflowing,
        offset: marquee_offset(elapsed, shift, reduced),
    }
}

/// The marquee over time: to the end, pause, back, pause. Reduced motion
/// makes each move near instant: the name jumps, the pauses stay.
fn marquee_offset(elapsed: f64, shift: f32, reduced: bool) -> f32 {
    let d = marquee_move(shift, reduced);
    if d <= 0.0 || elapsed <= 0.0 {
        return 0.0;
    }
    let m = elapsed % (2.0 * (d + MARQUEE_PAUSE));
    let t = if m < d {
        m / d
    } else if m < d + MARQUEE_PAUSE {
        1.0
    } else if m < 2.0 * d + MARQUEE_PAUSE {
        1.0 - (m - d - MARQUEE_PAUSE) / d
    } else {
        0.0
    };
    -shift * t as f32
}

/// Seconds one move of the marquee takes, rounded to the millisecond.
fn marquee_move(shift: f32, reduced: bool) -> f64 {
    if reduced {
        1e-5
    } else {
        (f64::from(shift) / f64::from(MARQUEE_SPEED) * 1000.0).round() / 1000.0
    }
}

/// While the marquee rests at an end, the seconds until it moves again.
fn marquee_rest(elapsed: f64, shift: f32, reduced: bool) -> Option<f64> {
    let d = marquee_move(shift, reduced);
    if d <= 0.0 || elapsed <= 0.0 {
        return None;
    }
    let cycle = 2.0 * (d + MARQUEE_PAUSE);
    let m = elapsed % cycle;
    if (d..d + MARQUEE_PAUSE).contains(&m) {
        Some(d + MARQUEE_PAUSE - m)
    } else if m >= 2.0 * d + MARQUEE_PAUSE {
        Some(cycle - m)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marquee_runs_to_the_end_pauses_and_comes_back() {
        // 42 px at 42 px/s: one second each way, two seconds of pause.
        assert_eq!(marquee_offset(0.0, 42.0, false), 0.0);
        assert!((marquee_offset(0.5, 42.0, false) + 21.0).abs() < 1e-4);
        assert_eq!(marquee_offset(1.0, 42.0, false), -42.0);
        assert_eq!(marquee_offset(2.9, 42.0, false), -42.0);
        assert!((marquee_offset(3.5, 42.0, false) + 21.0).abs() < 1e-4);
        assert_eq!(marquee_offset(5.0, 42.0, false), 0.0);
        assert!((marquee_offset(6.5, 42.0, false) + 21.0).abs() < 1e-4);
    }

    #[test]
    fn reduced_marquee_jumps_and_keeps_the_pauses() {
        assert_eq!(marquee_offset(0.0, 42.0, true), 0.0);
        assert_eq!(marquee_offset(0.1, 42.0, true), -42.0);
        assert_eq!(marquee_offset(1.9, 42.0, true), -42.0);
        assert_eq!(marquee_offset(2.1, 42.0, true), 0.0);
        assert_eq!(marquee_offset(4.1, 42.0, true), -42.0);
    }

    #[test]
    fn the_marquee_asks_for_no_frame_while_it_rests() {
        // One second each way, two seconds of pause at each end.
        assert_eq!(marquee_rest(0.5, 42.0, false), None);
        assert!((marquee_rest(1.5, 42.0, false).unwrap() - 1.5).abs() < 1e-9);
        assert_eq!(marquee_rest(3.5, 42.0, false), None);
        assert!((marquee_rest(5.25, 42.0, false).unwrap() - 0.75).abs() < 1e-9);
        assert!((marquee_rest(1.0, 42.0, true).unwrap() - 1.00001).abs() < 1e-9);
    }
}
