//! The screen a failed boot shows. The app has no settings, theme or locale
//! store yet, so it draws on fixed colours and the OS language: a title, the
//! error as the backend gave it, what to do, and a close button, since the
//! title bar and its caption buttons never mount.

use egui::{Color32, CursorIcon, Id, Rect, Sense, Ui};

use accshift_app::i18n::{translate, Locale};

use crate::ui::text::{self, TextAlign, TextStyle};

/// What the boot error screen reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootErrorEvent {
    /// The close button.
    Close,
    /// A press on the background: the window has no other drag handle.
    StartDrag,
}

const BACKGROUND: Color32 = Color32::from_rgb(0x14, 0x13, 0x1a);
const TITLE: Color32 = Color32::from_rgb(0xf4, 0xf2, 0xfa);
const BODY: Color32 = Color32::from_rgb(0xb4, 0xae, 0xc6);
const DETAIL: Color32 = Color32::from_rgb(0x86, 0x80, 0x9a);
const BUTTON: Color32 = Color32::from_rgb(0xb3, 0x9d, 0xf5);
const BUTTON_HOVER: Color32 = Color32::from_rgb(0xc6, 0xb5, 0xfa);
const BUTTON_TEXT: Color32 = Color32::from_rgb(0x1b, 0x16, 0x2b);

const COLUMN_WIDTH: f32 = 440.0;
const MARGIN: f32 = 24.0;
const BUTTON_SIZE: egui::Vec2 = egui::vec2(120.0, 34.0);

/// Draws the screen over `rect` (the whole window).
pub fn boot_error(ui: &mut Ui, rect: Rect, locale: Locale, message: &str) -> Vec<BootErrorEvent> {
    let mut events = Vec::new();
    let background = ui.interact(rect, Id::unique("boot-error-background"), Sense::drag());
    if background.drag_started() {
        events.push(BootErrorEvent::StartDrag);
    }
    let painter = ui.painter().clone();
    painter.rect_filled(rect, 0.0, BACKGROUND);

    let width = COLUMN_WIDTH.min(rect.width() - 2.0 * MARGIN).max(0.0);
    let left = rect.center().x - width / 2.0;
    let lines = [
        (
            translate(locale, "boot.failedTitle"),
            TextStyle::new(20.0, 600).color(TITLE),
            10.0,
        ),
        (
            translate(locale, "boot.failedHint"),
            TextStyle::new(13.0, 400).color(BODY),
            14.0,
        ),
        (
            message.to_string(),
            TextStyle::new(12.0, 400).overflow_anywhere().color(DETAIL),
            22.0,
        ),
    ];
    let ctx = ui.ctx().clone();
    // The column is centred on its own height: measure, then paint.
    let column = |top: f32, paint: bool| -> f32 {
        let mut y = top;
        for (line, style, gap) in &lines {
            let style = style.clone().align(TextAlign::Center);
            let slot = Rect::from_min_size(egui::pos2(left, y), egui::vec2(width, 0.0));
            let laid = if paint {
                text::paint(&painter, slot, line, &style)
            } else {
                text::layout(&ctx, line, &style, slot)
            };
            y += laid.map_or(0.0, |laid| laid.height) + gap;
        }
        y - top
    };
    let height = column(0.0, false) + BUTTON_SIZE.y;
    let top = (rect.center().y - height / 2.0).max(rect.top() + MARGIN);
    let button_top = top + column(top, true);

    let button = Rect::from_min_size(
        egui::pos2(rect.center().x - BUTTON_SIZE.x / 2.0, button_top),
        BUTTON_SIZE,
    );
    let response = ui
        .interact(button, Id::unique("boot-error-close"), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    let fill = if response.hovered() {
        BUTTON_HOVER
    } else {
        BUTTON
    };
    painter.rect_filled(button, 8.0, fill);
    let label = TextStyle::new(13.0, 600)
        .nowrap(false)
        .align(TextAlign::Center)
        .color(BUTTON_TEXT);
    // 13 px on the 1.5 line height is a 19.5 px line box.
    let label_rect = Rect::from_min_size(
        egui::pos2(button.left(), button.center().y - 9.75),
        egui::vec2(button.width(), 19.5),
    );
    text::paint(
        &painter,
        label_rect,
        &translate(locale, "common.close"),
        &label,
    );
    if response.clicked() {
        events.push(BootErrorEvent::Close);
    }
    events
}
