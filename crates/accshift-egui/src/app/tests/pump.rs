//! Frames on the mock backend until a condition holds, each one drawn when
//! the app asks for it, as the window's event loop would.
//!
//! Only the idle wait between frames is bounded, never a frame: a debug
//! frame on a loaded runner can take seconds (the first ones rasterize
//! their glyphs), and a deadline that counted the frames failed waits that
//! were only slow, two frames away from holding.

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use egui::Context;

use super::AppModel;

/// How long the app may sit idle, the condition false and no frame asked
/// for, before the wait counts as a hang.
const IDLE_LIMIT: Duration = Duration::from_secs(3);
/// A backstop for an app that asks for a frame after every frame.
const MAX_FRAMES: u32 = 2_000;
/// How often an idle wait reads the condition again, for a state no frame
/// reports (a mocked call reaching its hold).
const IDLE_POLL: Duration = Duration::from_millis(1);

/// The next frame the app asked for since the last one began, as the
/// context's repaint observer sees it: a job whose result landed asks for
/// one through the pool's wake, a debounce for one at its deadline.
#[derive(Default)]
struct Wakes {
    due: Mutex<Option<Instant>>,
    asked: Condvar,
}

impl Wakes {
    /// Watches every repaint request `ctx` gets from now on.
    fn observe(ctx: &Context) -> Arc<Self> {
        let wakes = Arc::new(Self::default());
        let seen = Arc::clone(&wakes);
        ctx.set_repaint_observer(move |info| {
            let Some(at) = Instant::now().checked_add(info.delay) else {
                return;
            };
            let mut due = seen.due.lock().unwrap_or_else(|p| p.into_inner());
            if due.is_none_or(|due| at < due) {
                *due = Some(at);
                seen.asked.notify_all();
            }
        });
        wakes
    }

    fn clear(&self) {
        *self.due.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }

    /// Waits at most `limit` for a frame the app asked for to come due;
    /// true when one did.
    fn wait_due(&self, limit: Duration) -> bool {
        let due = self.due.lock().unwrap_or_else(|p| p.into_inner());
        let now = Instant::now();
        let wait = match *due {
            Some(at) if at <= now => return true,
            Some(at) => (at - now).min(limit),
            None => limit,
        };
        let (due, _) = self
            .asked
            .wait_timeout(due, wait)
            .unwrap_or_else(|p| p.into_inner());
        due.is_some_and(|at| at <= Instant::now())
    }
}

/// Draws frames with `draw` until `condition` holds.
#[track_caller]
pub(super) fn until(
    app: &mut AppModel,
    ctx: &Context,
    draw: impl FnMut(&mut AppModel, &Context),
    condition: impl Fn(&AppModel) -> bool,
) {
    within(app, ctx, IDLE_LIMIT, draw, condition);
}

/// [`until`], idle for at most `idle_limit` in all.
#[track_caller]
pub(super) fn within(
    app: &mut AppModel,
    ctx: &Context,
    idle_limit: Duration,
    mut draw: impl FnMut(&mut AppModel, &Context),
    condition: impl Fn(&AppModel) -> bool,
) {
    let wakes = Wakes::observe(ctx);
    let mut idle = Duration::ZERO;
    let mut frames = 0;
    while !condition(app) {
        assert!(
            frames < MAX_FRAMES,
            "the condition never held: {frames} frames drawn"
        );
        wakes.clear();
        draw(app, ctx);
        frames += 1;
        loop {
            if condition(app) {
                return;
            }
            assert!(
                idle < idle_limit,
                "the condition never held: the app sat idle {idle:?} with no frame to draw"
            );
            let waited = Instant::now();
            let due = wakes.wait_due(IDLE_POLL);
            idle += waited.elapsed();
            if due {
                break;
            }
        }
    }
}
