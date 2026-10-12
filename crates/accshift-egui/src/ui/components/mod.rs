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

pub mod icon;
pub mod platform_icons;
pub mod title_bar;
