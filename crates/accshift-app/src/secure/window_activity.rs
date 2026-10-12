//! Window focus and visibility, ported from
//! `src/lib/shared/useWindowActivity.svelte.ts` (the state machine only).
//!
//! TS listens to the DOM `focus`, `blur` and `visibilitychange` events and to
//! Tauri's `onFocusChanged` and `onResized`, and asks the native window
//! (`isFocused`, `isMinimized`) on a blur, a resize or a return to visible.
//! The egui shell feeds the same events from its viewport info and answers
//! the native query itself: [`WindowActivity::take_sync_request`] says a
//! query is due, [`WindowActivity::apply_sync`] takes its answer.
//!
//! The async registration race of TS (`takeNativeUnlisteners`, the
//! `generation` counter) has no equivalent: egui events are synchronous, so
//! "a listener that lands after stop" becomes "an event fed while stopped",
//! which is dropped.

/// `createWindowActivity`.
#[derive(Debug, Clone)]
pub struct WindowActivity {
    is_focused: bool,
    is_minimized: bool,
    is_page_visible: bool,
    started: bool,
    /// A `sync()` is due. Several requests before the answer collapse into
    /// one, as the `syncing` / `pendingSync` pair does (lines 38-66).
    sync_requested: bool,
}

impl Default for WindowActivity {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowActivity {
    /// Starts focused, not minimized, visible (lines 22-24).
    pub fn new() -> Self {
        Self {
            is_focused: true,
            is_minimized: false,
            is_page_visible: true,
            started: false,
            sync_requested: false,
        }
    }

    pub fn is_focused(&self) -> bool {
        self.is_focused
    }

    pub fn is_minimized(&self) -> bool {
        self.is_minimized
    }

    pub fn is_page_visible(&self) -> bool {
        self.is_page_visible
    }

    /// `isForeground` (lines 155-157).
    pub fn is_foreground(&self) -> bool {
        self.is_focused && self.is_page_visible && !self.is_minimized
    }

    pub fn is_started(&self) -> bool {
        self.started
    }

    /// `start` (lines 69-134): takes the current visibility and asks for a
    /// first native sync. A second start is a no-op.
    pub fn start(&mut self, page_visible: bool) {
        if self.started {
            return;
        }
        self.started = true;
        self.is_page_visible = page_visible;
        self.sync_requested = true;
    }

    /// `stop` (lines 136-143): events are dropped until the next start.
    pub fn stop(&mut self) {
        if !self.started {
            return;
        }
        self.started = false;
        self.sync_requested = false;
    }

    /// The window gained or lost focus: the DOM `focus`/`blur` pair (lines
    /// 79-87) and Tauri's `onFocusChanged` (lines 110-116), which fire
    /// together in the webview. A focused window is not minimized; a blur
    /// asks the native window whether it got minimized.
    pub fn on_focus_changed(&mut self, focused: bool, page_visible: bool) {
        if !self.started {
            return;
        }
        self.is_focused = focused;
        self.is_page_visible = page_visible;
        if focused {
            self.is_minimized = false;
        } else {
            self.sync_requested = true;
        }
    }

    /// `visibilitychange` (lines 95-100): back to visible asks for a sync.
    pub fn on_visibility_changed(&mut self, visible: bool) {
        if !self.started {
            return;
        }
        self.is_page_visible = visible;
        if visible {
            self.sync_requested = true;
        }
    }

    /// `onResized` (lines 117-119): minimizing is a resize.
    pub fn on_resized(&mut self) {
        if self.started {
            self.sync_requested = true;
        }
    }

    /// True once per due `sync()`: the shell then reads the native focus and
    /// minimized state and calls [`Self::apply_sync`].
    pub fn take_sync_request(&mut self) -> bool {
        std::mem::take(&mut self.sync_requested)
    }

    /// The answer of `sync()` (lines 38-67). A `None` is a query that failed
    /// (`Promise.allSettled` rejected) and keeps the old value.
    pub fn apply_sync(
        &mut self,
        page_visible: bool,
        focused: Option<bool>,
        minimized: Option<bool>,
    ) {
        self.is_page_visible = page_visible;
        if let Some(focused) = focused {
            self.is_focused = focused;
        }
        if let Some(minimized) = minimized {
            self.is_minimized = minimized;
        }
    }
}
