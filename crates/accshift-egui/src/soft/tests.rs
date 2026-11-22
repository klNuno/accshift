use std::num::NonZeroU32;
use std::time::Duration;

use super::platform::is_shortcut_press;
use super::present::{frame_buffer, Faults, Present, LOGGED_FAULTS};

#[test]
fn shortcut_capture_ignores_synthetic_focus_presses() {
    assert!(!is_shortcut_press(true, false, true));
    assert!(!is_shortcut_press(true, true, false));
    assert!(!is_shortcut_press(false, false, false));
    assert!(is_shortcut_press(true, false, false));
}

/// A presenter whose buffer fails a given number of times.
struct Flaky {
    failures: u32,
    rebuilt: u32,
}

impl Present for Flaky {
    type Buffer<'a> = ();

    fn rebuild(&mut self) -> Result<(), String> {
        self.rebuilt += 1;
        Ok(())
    }

    fn resize(&mut self, _: NonZeroU32, _: NonZeroU32) -> Result<(), String> {
        Ok(())
    }

    fn buffer(&mut self) -> Result<(), String> {
        if self.failures == 0 {
            return Ok(());
        }
        self.failures -= 1;
        Err("buffer: lost".into())
    }
}

#[test]
fn a_failed_frame_is_skipped_and_the_next_rebuilds_the_surface() {
    let size = NonZeroU32::new(8).unwrap();
    let mut presenter = Flaky {
        failures: 1,
        rebuilt: 0,
    };
    let mut faults = Faults::default();
    assert!(frame_buffer(&mut presenter, &mut faults, size, size).is_err());
    assert!(faults.logged());
    assert!(frame_buffer(&mut presenter, &mut faults, size, size).is_ok());
    assert_eq!(presenter.rebuilt, 1);
    faults.succeeded();
    // A healthy frame does not rebuild.
    assert!(frame_buffer(&mut presenter, &mut faults, size, size).is_ok());
    assert_eq!(presenter.rebuilt, 1);
}

#[test]
fn repeated_failures_back_off_and_stop_logging() {
    let mut faults = Faults::default();
    let mut last = Duration::ZERO;
    for _ in 0..LOGGED_FAULTS {
        faults.fail();
        assert!(faults.logged());
        assert!(faults.retry_after() > last);
        last = faults.retry_after();
    }
    for _ in 0..20 {
        faults.fail();
    }
    assert!(!faults.logged());
    assert_eq!(faults.retry_after(), Duration::from_secs(1));
}
