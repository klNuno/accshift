//! `src/lib/features/folders/BackCard.svelte`: the first card inside a
//! folder, back to the parent.
//!
//! Same `.card` box as the folder card ([`super::folder_card::frame`]), a
//! 26 px arrow (stroke 2) in the icon wrap and `common.back` under it in
//! `.name { font-size: 12px; color: var(--fg-muted) }`: a `<button>` keeps
//! weight 400 and the 1.5 line height, and the name is an inline-size box
//! centred by the card's `align-items: center`.

use egui::{Id, Rect, Ui};

use accshift_app::i18n::{translate, Locale};

use super::folder_card::{self, CardState};
use crate::app::vars::Vars;
use crate::icons;
use crate::theme::Rgba;
use crate::ui::text::{LineHeight, TextAlign, TextStyle};

/// What the back card reports.
#[derive(Clone, Debug, PartialEq)]
pub enum BackCardEvent {
    /// Click, Enter or Space: `onBack`.
    Back,
}

/// `BackCard.svelte`'s props, minus the callback.
#[derive(Clone, Debug)]
pub struct BackCardProps<'a> {
    pub vars: &'a Vars,
    /// Stable for one mount of the card.
    pub id: Id,
    pub locale: Locale,
    pub accent: Rgba,
    pub drag_over: bool,
    pub focus_visible: bool,
}

/// Draws the back card over `rect` (its layout box).
pub fn back_card(ui: &mut Ui, rect: Rect, props: &BackCardProps) -> Vec<BackCardEvent> {
    let v = props.vars;
    let label = translate(props.locale, "common.back");
    let f = folder_card::frame(
        ui,
        rect,
        props.id,
        &label,
        CardState {
            custom: None,
            accent: props.accent,
            drag_over: props.drag_over,
            dragged: false,
        },
        v,
    );
    folder_card::icon_wrap(ui, &f, v, icons::BACK, 26.0);
    let style = TextStyle::new(12.0, 400)
        .line_height(LineHeight::Number(1.5))
        .nowrap(false)
        .align(TextAlign::Center)
        .color(v.fg_muted.to_color32());
    folder_card::name(
        ui,
        &f,
        folder_card::name_rect(f.rect, v, 18.0),
        &label,
        &style,
    );
    if props.focus_visible && f.response.has_focus() {
        super::focus::card_ring(ui, f.rect, v.radius_md, v);
    }
    let mut events = Vec::new();
    if f.response.clicked() {
        events.push(BackCardEvent::Back);
    }
    events
}
