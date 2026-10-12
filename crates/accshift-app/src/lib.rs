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

pub mod accounts;
pub mod backend;
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
pub mod platforms;
pub mod real;
pub mod settings;
pub mod storage;
pub mod stores;
pub mod text;
pub mod time;
pub mod types;

pub use backend::{Backend, COMMANDS};
pub use error::{BackendError, BackendResult};
