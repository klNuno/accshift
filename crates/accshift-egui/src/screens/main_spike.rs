//! What is left of the first spike: the font setup the runner calls before
//! the first frame. The spike's screen is gone; the workspace
//! (`screens::workspace`) fills the panel slot.

/// Installs the UI fonts. Fonts set mid-frame only apply one frame later, so
/// the runner calls this before the first frame.
pub fn install_fonts(ctx: &egui::Context) {
    crate::ui::text::install(ctx, false);
}
