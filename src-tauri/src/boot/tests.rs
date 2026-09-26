use super::*;
use tauri::{PhysicalPosition, PhysicalSize};

#[test]
fn deep_link_log_shape_drops_the_account() {
    let shape = |raw: &str| deep_link_log_shape(&tauri::Url::parse(raw).unwrap());

    assert_eq!(
        shape("accshift://switch/battle-net/user%40example.com"),
        "accshift://switch/battle-net/<account>"
    );
    assert_eq!(
        shape("accshift://switch/steam/login?x=1"),
        "accshift://switch/steam/<account><+extra>"
    );
    assert_eq!(shape("accshift://switch"), "accshift://switch");
}

// The unit bug in one assertion: a 1000x520 logical window on a 125%
// display reports 1250x650 physical. Storing that raw is what made the
// window grow by 25% at every launch, because the builder reads the stored
// number as logical.
#[test]
fn a_physical_window_size_converts_back_to_the_logical_one() {
    let scale = 1.25;
    let physical = PhysicalSize::new(1250_u32, 650_u32);
    let logical = physical.to_logical::<f64>(scale);

    assert_eq!((logical.width, logical.height), (1000.0, 520.0));
    assert_eq!(
        (
            accshift_core::config::logical_from_physical(1250.0, scale),
            accshift_core::config::logical_from_physical(650.0, scale),
        ),
        (logical.width, logical.height),
        "the config helper and the tauri conversion must agree"
    );
}

#[test]
fn a_physical_window_position_converts_back_to_the_logical_one() {
    let physical = PhysicalPosition::new(-2400_i32, 150_i32);
    let logical = physical.to_logical::<f64>(1.5);
    assert_eq!((logical.x, logical.y), (-1600.0, 100.0));
}

#[test]
fn a_window_overlapping_a_monitor_is_kept() {
    let monitor = Rect::at(0.0, 0.0, 1920.0, 1040.0);
    // Fully inside.
    assert!(Rect::at(100.0, 100.0, 1000.0, 520.0).overlaps(&monitor));
    // Half off the right edge, still reachable.
    assert!(Rect::at(1900.0, 100.0, 1000.0, 520.0).overlaps(&monitor));
    // A second monitor to the left of the primary one.
    assert!(
        Rect::at(-1800.0, 40.0, 1000.0, 520.0).overlaps(&Rect::at(-1920.0, 0.0, 1920.0, 1040.0))
    );
}

#[test]
fn a_window_off_every_monitor_is_rejected() {
    let monitor = Rect::at(0.0, 0.0, 1920.0, 1040.0);
    // The unplugged second monitor case.
    assert!(!Rect::at(-1800.0, 40.0, 1000.0, 520.0).overlaps(&monitor));
    // Below the taskbar, off the work area.
    assert!(!Rect::at(100.0, 1040.0, 1000.0, 520.0).overlaps(&monitor));
    // Touching edges share no pixel.
    assert!(!Rect::at(1920.0, 0.0, 1000.0, 520.0).overlaps(&monitor));
}
