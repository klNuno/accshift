//! The descriptor and custom theme writes that bypass the client-store
//! queue, counted so a backup or an import can wait for them behind its
//! boundary (`app::backup_flow`).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use accshift_app::backend::Backend;
use egui::Context;

/// The descriptor and custom theme writes, which bypass the client-store
/// queue. Each admitted job counts until its call returns, even when the
/// panel that started it is gone; none is admitted while a backup holds the
/// boundary.
#[derive(Clone, Default)]
pub(super) struct WriteGate(Arc<GateState>);

#[derive(Default)]
struct GateState {
    running: AtomicUsize,
    closed: AtomicBool,
    /// The first write that failed while a backup held the boundary.
    failed: Mutex<Option<String>>,
    /// The UI to wake when an admitted call returns: a boundary waiting
    /// for it draws no frame otherwise, once no one polls the call's result.
    wake: Option<Context>,
    /// Test seam: run once the boundary is published.
    #[cfg(test)]
    published: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

/// One admitted call, uncounted when dropped.
struct Admitted(Arc<GateState>);

impl Drop for Admitted {
    fn drop(&mut self) {
        self.0.running.fetch_sub(1, Ordering::SeqCst);
        if let Some(ctx) = &self.0.wake {
            ctx.request_repaint();
        }
    }
}

impl WriteGate {
    /// A gate that wakes `ctx` each time an admitted call returns.
    pub(super) fn waking(ctx: &Context) -> Self {
        Self(Arc::new(GateState {
            wake: Some(ctx.clone()),
            ..GateState::default()
        }))
    }

    /// `call`, counted until it returns; `None` while a backup holds writes.
    pub(super) fn admit<T, F>(
        &self,
        call: F,
    ) -> Option<impl FnOnce(&dyn Backend) -> T + Send + 'static>
    where
        T: Send + 'static,
        F: FnOnce(&dyn Backend) -> T + Send + 'static,
    {
        if self.0.closed.load(Ordering::SeqCst) {
            return None;
        }
        self.0.running.fetch_add(1, Ordering::SeqCst);
        let admitted = Admitted(Arc::clone(&self.0));
        Some(move |b: &dyn Backend| {
            let _admitted = admitted;
            call(b)
        })
    }

    /// [`Self::admit`] for a call that writes a file: when it fails while a
    /// backup holds the boundary, the backup does not run, since the file
    /// may be left changed.
    pub(super) fn admit_write<R, E, F>(
        &self,
        call: F,
    ) -> Option<impl FnOnce(&dyn Backend) -> Result<R, E> + Send + 'static>
    where
        R: Send + 'static,
        E: std::fmt::Display + Send + 'static,
        F: FnOnce(&dyn Backend) -> Result<R, E> + Send + 'static,
    {
        let state = Arc::clone(&self.0);
        self.admit(move |b| {
            let result = call(b);
            if let Err(error) = &result {
                if state.closed.load(Ordering::SeqCst) {
                    let mut failed = state.failed.lock().unwrap_or_else(|p| p.into_inner());
                    failed.get_or_insert_with(|| error.to_string());
                }
            }
            result
        })
    }

    /// An admitted call has not returned yet.
    pub(super) fn running(&self) -> bool {
        self.0.running.load(Ordering::SeqCst) > 0
    }

    /// The write that failed since the boundary went up, taken.
    pub(super) fn take_failure(&self) -> Option<String> {
        self.0
            .failed
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
    }

    pub(super) fn set_closed(&self, closed: bool) {
        if !closed || self.0.closed.load(Ordering::SeqCst) {
            self.0.closed.store(closed, Ordering::SeqCst);
            return;
        }
        // The failures of an earlier boundary go before this one is
        // published, under the lock a failing write records with: a write
        // that sees the boundary records after this, and is never erased.
        let mut failed = self.0.failed.lock().unwrap_or_else(|p| p.into_inner());
        *failed = None;
        self.0.closed.store(true, Ordering::SeqCst);
        drop(failed);
        #[cfg(test)]
        self.run_published_hook();
    }

    /// Test seam: runs `hook` on the UI thread the moment the next boundary
    /// is published to the write jobs.
    #[cfg(test)]
    pub(super) fn on_published(&self, hook: impl FnOnce() + Send + 'static) {
        *self.0.published.lock().unwrap() = Some(Box::new(hook));
    }

    #[cfg(test)]
    fn run_published_hook(&self) {
        let hook = self.0.published.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
    }
}
