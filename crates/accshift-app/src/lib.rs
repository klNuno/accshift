//! UI-free logic of the accshift egui port.
//!
//! - [`backend::Backend`]: one blocking method per Tauri command the frontend
//!   calls, with the same payloads ([`types`]) and error strings ([`error`]).
//! - [`mock::MockBackend`]: the TS mock's data, for parity work.
//! - [`real::RealBackend`]: the real backend, a skeleton until it is ported.
//! - [`jobs`]: runs calls off the UI thread and wakes the UI on each result.
//! - [`dispatch`]: the same commands by name, with JSON arguments.

pub mod backend;
pub mod dispatch;
pub mod error;
pub mod jobs;
pub mod mock;
pub mod real;
pub mod types;

pub use backend::{Backend, COMMANDS};
pub use error::{BackendError, BackendResult};
