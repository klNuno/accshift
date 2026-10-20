//! The toast queue, ported from `src/lib/features/notifications/store.svelte.ts`
//! plus the countdown of `src/lib/features/notifications/Toast.svelte`.
//!
//! TS keeps the countdown in each toast component (`setTimeout`, paused while
//! hovered, restarted when the store bumps `resetKey`). Here the queue owns
//! it as a deadline per toast: the UI reports hovers, calls [`ToastQueue::tick`]
//! each frame and repaints at [`ToastQueue::next_deadline`].

use crate::accounts::{Toast, ToastKind};
use crate::text::Text;
use std::time::{Duration, Instant};

/// `MAX_TOASTS`.
pub const MAX_TOASTS: usize = 5;
/// `DEFAULT_DURATION_MS`.
pub const DEFAULT_DURATION_MS: u64 = 3000;

/// `ToastType`. TS has no "default" type: no `type` means `info`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToastType {
    #[default]
    Info,
    Success,
    Error,
}

impl From<ToastKind> for ToastType {
    fn from(kind: ToastKind) -> Self {
        match kind {
            ToastKind::Default | ToastKind::Info => ToastType::Info,
            ToastKind::Success => ToastType::Success,
            ToastKind::Error => ToastType::Error,
        }
    }
}

/// `AddToastOptions.durationMs`: left out, a number, or `null`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DurationRequest {
    /// `undefined`: the default timer.
    #[default]
    Default,
    /// An explicit duration.
    Ms(u64),
    /// `null`: until dismissed.
    Sticky,
}

/// `ToastAction`: a button on the toast. The TS closure becomes an id the
/// app maps to what the button does.
#[derive(Debug, Clone, PartialEq)]
pub struct ToastAction {
    pub label: Text,
    pub id: String,
}

/// `AddToastOptions`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AddToastOptions {
    pub kind: ToastType,
    pub duration: DurationRequest,
    pub action: Option<ToastAction>,
}

impl AddToastOptions {
    pub fn kind(kind: ToastType) -> Self {
        Self {
            kind,
            ..Self::default()
        }
    }
}

/// `resolveToastDuration`: how long a toast stays, `None` meaning until the
/// user dismisses it. An error never counts down, whatever was asked.
pub fn resolve_toast_duration(kind: ToastType, requested: DurationRequest) -> Option<u64> {
    if kind == ToastType::Error {
        return None;
    }
    match requested {
        DurationRequest::Default => Some(DEFAULT_DURATION_MS),
        DurationRequest::Ms(ms) => Some(ms),
        DurationRequest::Sticky => None,
    }
}

/// The countdown state `Toast.svelte` keeps in its closure.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Countdown {
    remaining_ms: u64,
    started_at: Option<Instant>,
    hovered: bool,
}

/// `ToastMessage`.
#[derive(Debug, Clone, PartialEq)]
pub struct ToastEntry {
    pub id: u64,
    pub text: Text,
    pub duration_ms: Option<u64>,
    pub kind: ToastType,
    pub action: Option<ToastAction>,
    /// Bumped when the same text is raised again; the countdown restarts.
    pub reset_key: u32,
    countdown: Countdown,
}

impl ToastEntry {
    /// `hasDuration`: a positive duration. `0` never counts down, as in TS.
    fn has_duration(&self) -> bool {
        self.duration_ms.is_some_and(|ms| ms > 0)
    }

    /// The `$effect` on `resetKey` and `durationMs`: clear, then start again
    /// unless hovered.
    fn restart(&mut self, now: Instant) {
        self.countdown.started_at = None;
        if !self.has_duration() {
            return;
        }
        self.countdown.remaining_ms = self.duration_ms.unwrap_or(0);
        if !self.countdown.hovered {
            self.countdown.started_at = Some(now);
        }
    }

    /// When the running countdown ends, `None` while paused or untimed.
    pub fn deadline(&self) -> Option<Instant> {
        self.countdown
            .started_at
            .map(|start| start + Duration::from_millis(self.countdown.remaining_ms))
    }

    pub fn hovered(&self) -> bool {
        self.countdown.hovered
    }
}

/// The toast store.
#[derive(Debug, Clone, Default)]
pub struct ToastQueue {
    toasts: Vec<ToastEntry>,
    next_id: u64,
}

impl ToastQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// `getToasts`, oldest first.
    pub fn toasts(&self) -> &[ToastEntry] {
        &self.toasts
    }

    pub fn get(&self, id: u64) -> Option<&ToastEntry> {
        self.toasts.iter().find(|toast| toast.id == id)
    }

    /// `addToast`. The same text already on screen takes the new type,
    /// duration and action (an absent action keeps the old one) and restarts
    /// its countdown instead of stacking a duplicate. Beyond
    /// [`MAX_TOASTS`] the oldest go.
    pub fn add(&mut self, text: Text, options: AddToastOptions, now: Instant) -> u64 {
        let duration_ms = resolve_toast_duration(options.kind, options.duration);
        if let Some(existing) = self.toasts.iter_mut().find(|toast| toast.text == text) {
            existing.kind = options.kind;
            existing.duration_ms = duration_ms;
            if options.action.is_some() {
                existing.action = options.action;
            }
            existing.reset_key += 1;
            existing.restart(now);
            return existing.id;
        }
        self.next_id += 1;
        let mut entry = ToastEntry {
            id: self.next_id,
            text,
            duration_ms,
            kind: options.kind,
            action: options.action,
            reset_key: 0,
            countdown: Countdown::default(),
        };
        entry.restart(now);
        self.toasts.push(entry);
        if self.toasts.len() > MAX_TOASTS {
            let excess = self.toasts.len() - MAX_TOASTS;
            self.toasts.drain(..excess);
        }
        self.next_id
    }

    /// A toast the account or platform code asked for.
    pub fn push(&mut self, toast: Toast, now: Instant) -> u64 {
        self.add(toast.text, AddToastOptions::kind(toast.kind.into()), now)
    }

    /// `removeToast`.
    pub fn remove(&mut self, id: u64) {
        self.toasts.retain(|toast| toast.id != id);
    }

    /// `handleMouseEnter` / `handleMouseLeave`. Leaving with no time left
    /// dismisses the toast; true when it did.
    pub fn set_hovered(&mut self, id: u64, hovered: bool, now: Instant) -> bool {
        let Some(toast) = self.toasts.iter_mut().find(|toast| toast.id == id) else {
            return false;
        };
        if hovered {
            toast.countdown.hovered = true;
            if let Some(start) = toast.countdown.started_at.take() {
                let elapsed = now.saturating_duration_since(start).as_millis() as u64;
                toast.countdown.remaining_ms = toast.countdown.remaining_ms.saturating_sub(elapsed);
            }
            return false;
        }
        toast.countdown.hovered = false;
        if !toast.has_duration() {
            return false;
        }
        if toast.countdown.remaining_ms == 0 {
            self.remove(id);
            return true;
        }
        toast.countdown.started_at = Some(now);
        false
    }

    /// `handleAction`: the action id to run; the toast goes either way.
    pub fn activate_action(&mut self, id: u64) -> Option<String> {
        let action = self
            .get(id)
            .and_then(|toast| toast.action.as_ref())
            .map(|action| action.id.clone());
        self.remove(id);
        action
    }

    /// Removes every toast whose countdown ended; their ids.
    pub fn tick(&mut self, now: Instant) -> Vec<u64> {
        let done: Vec<u64> = self
            .toasts
            .iter()
            .filter(|toast| toast.deadline().is_some_and(|deadline| deadline <= now))
            .map(|toast| toast.id)
            .collect();
        self.toasts.retain(|toast| !done.contains(&toast.id));
        done
    }

    /// The next countdown end, for the repaint.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.toasts.iter().filter_map(ToastEntry::deadline).min()
    }
}
