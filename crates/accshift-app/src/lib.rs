//! UI-free logic of the accshift egui port.
//!
//! - [`backend::Backend`]: one blocking method per Tauri command the frontend
//!   calls, with the same payloads ([`types`]) and error strings ([`error`]).
//! - [`mock::MockBackend`]: the TS mock's data, for parity work.
//! - [`real::RealBackend`]: the real backend, a skeleton until it is ported.
//! - [`jobs`]: runs calls off the UI thread and wakes the UI on each result.
//! - [`dispatch`]: the same commands by name, with JSON arguments.
//! - [`storage`]: the raw client stores, their saves and refreshes.
//! - [`stores::ClientStores`]: every client store, typed ([`settings`],
//!   [`folders`], [`personas`] and the small ones), over the storage.
//! - [`i18n`]: the seven dictionaries and `translate`.

pub mod backend;
pub mod dispatch;
pub mod error;
pub mod folders;
pub mod i18n;
pub mod jobs;
pub mod js;
pub mod mock;
pub mod personas;
pub mod real;
pub mod settings;
pub mod storage;
pub mod stores;
pub mod types;

pub use backend::{Backend, COMMANDS};
pub use error::{BackendError, BackendResult};
