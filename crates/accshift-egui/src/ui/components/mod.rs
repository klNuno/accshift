//! One module per Svelte component, same name in snake_case
//! (`TitleBar.svelte` is `title_bar.rs`).
//!
//! A component is a plain function `fn x(ui, rect, &props) -> Vec<Action>`
//! (or a struct with `show`): it reads its props and the [`Vars`], keeps its
//! transient state (hover, transitions, focus) in egui memory under its own
//! ids, never touches the app state, and returns what the user asked for as
//! [`Action`]s, which the [`AppModel`] applies after the frame is drawn.
//!
//! [`Vars`]: crate::app::vars::Vars
//! [`Action`]: crate::app::Action
//! [`AppModel`]: crate::app::AppModel

pub mod account_card;
pub mod avatar_fade_in;
pub mod back_card;
pub mod card_extension;
pub mod breadcrumb;
pub mod context_menu;
pub mod controls;
pub mod dialog;
pub mod focus;
pub mod folder_card;
pub mod icon;
pub mod list_view;
pub mod secure_overlays;
pub mod platform_icons;
pub mod search_cancel;
pub mod section_header;
pub mod title_bar;
pub mod toast;
pub mod use_grid_layout;
pub mod view_toggle;

/// Where the right button went up this frame: Chromium fires `contextmenu`
/// there on Windows, even if the pointer moved again in the same frame.
pub fn context_click_pos(ui: &egui::Ui) -> Option<egui::Pos2> {
    ui.input(|i| {
        i.events.iter().rev().find_map(|e| match e {
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Secondary,
                pressed: false,
                ..
            } => Some(*pos),
            _ => None,
        })
    })
}
