//! UI-free logic of the accshift egui port.
//!
//! - [`backend::Backend`]: one blocking method per Tauri command the frontend
//!   calls, with the same payloads ([`types`]) and error strings ([`error`]).
//! - [`mock::MockBackend`]: the TS mock's data, for parity work.
//! - [`real::RealBackend`]: the real backend on `accshift-core`, the commands
//!   in [`real::PORTED`] filled, the others still "not ported".
//! - [`jobs`]: runs calls off the UI thread and wakes the UI on each result.
//! - [`dispatch`]: the same commands by name, with JSON arguments.
//! - [`storage`]: the raw client stores, their saves and refreshes.
//! - [`stores::ClientStores`]: every client store, typed ([`settings`],
//!   [`folders`], [`personas`] and the small ones), over the storage.
//! - [`i18n`]: the seven dictionaries and `translate`.
//! - [`accounts`]: account loading per tab, profile caches, avatars, priming
//!   and the settings refresh actions, driven through [`jobs`].
//! - [`display`]: folder navigation, search and the flat list of cards a grid
//!   draws.
//! - [`actions`]: dialogs, card actions, navigation, personas, the command
//!   palette, the keyboard model and toasts, as state plus effects.
//! - [`secure`]: the PIN, the inactivity blur, the window activity and the
//!   secure screen; [`streamer`]: streamer mode.
//! - [`bulk_edit`], [`platform_add_flow`], [`platform_shell`] and
//!   [`cs2_bridge`]: the Steam bulk edit, the add-account flow, the platform
//!   shell state and the CS2 bridge queue.

pub mod accounts;
pub mod actions;
pub mod backend;
pub mod bulk_edit;
pub mod cs2_bridge;
pub mod dispatch;
pub mod display;
pub mod error;
pub mod folders;
pub mod i18n;
pub mod jobs;
pub mod js;
pub mod legacy_web_storage;
pub mod menu;
pub mod mock;
pub mod personas;
pub mod platform_add_flow;
pub mod platform_shell;
pub mod platforms;
pub mod real;
pub mod secure;
pub mod settings;
pub mod storage;
pub mod stores;
pub mod streamer;
pub mod text;
pub mod time;
pub mod types;

pub use backend::{Backend, COMMANDS};
pub use error::{BackendError, BackendResult};
