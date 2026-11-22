//! What the unscaled overlay pass (`AppModel::overlay_ctx`: onboarding,
//! AFK cover, streamer mode, PIN lock) adds to the stage's platform output.
//! The window has one IME; both contexts feed it.

use egui::output::IMEOutput;

/// The window's IME: the stage's (already in stage points), unless the
/// overlay has one, or takes the input (`captures`): then the overlay's
/// wins even when it has none, so a stage field that kept focus under it
/// does not keep the IME on. `ratio` takes the overlay's points to the
/// stage's.
pub fn merge_ime(
    main: Option<IMEOutput>,
    overlay: Option<IMEOutput>,
    ratio: f32,
    captures: bool,
) -> Option<IMEOutput> {
    match overlay {
        Some(mut ime) => {
            ime.rect = ime.rect * ratio;
            ime.cursor_rect = ime.cursor_rect * ratio;
            Some(ime)
        }
        None if captures => None,
        None => main,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, Rect};

    fn ime(x: f32) -> IMEOutput {
        IMEOutput {
            purpose: egui::IMEPurpose::Normal,
            rect: Rect::from_min_size(pos2(x, x), egui::vec2(10.0, 10.0)),
            cursor_rect: Rect::from_min_size(pos2(x, x), egui::vec2(1.0, 10.0)),
            should_interrupt_composition: false,
        }
    }

    #[test]
    fn a_capturing_overlay_without_ime_turns_the_stage_ime_off() {
        assert_eq!(merge_ime(Some(ime(4.0)), None, 1.0, true), None);
        assert_eq!(merge_ime(Some(ime(4.0)), None, 1.0, false), Some(ime(4.0)));
        let merged = merge_ime(Some(ime(4.0)), Some(ime(10.0)), 0.5, true).unwrap();
        assert_eq!(merged.rect.min, pos2(5.0, 5.0));
        assert_eq!(merged.rect.width(), 5.0);
    }
}
