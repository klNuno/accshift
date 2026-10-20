//! Bulk edit: the multi-select mode and the Steam bulk edit bar, ported from
//! `src/lib/app/useBulkEdit.svelte.ts` and the non-drawing parts of
//! `src/lib/platforms/steam/BulkEditBar.svelte` (selection rules, request
//! building, progress and result states).
//!
//! The TS bar is a lazily loaded component chunk; here it is a plain model
//! created when the mode opens, so the load race of `toggleBulkEdit` (the
//! pending open token) has nothing left to guard. The paint gesture reads
//! the card under the pointer from the UI instead of `elementFromPoint`.
//!
//! The toasts for the outcome (`handleBulkEditResult`), the clipboard write
//! of the URLs and the card color write belong to the dialog and card-action
//! modules: [`BulkEdit::poll`] hands the result over, [`BulkEditBar::copy_urls`]
//! and [`BulkEditBar::apply_color`] the values.

use crate::error::BackendResult;
use crate::jobs::{Jobs, Pending, Poll};
use crate::platforms::steam::{api as steam_api, to_profile_url};
use crate::platforms::PlatformDef;
use crate::text::Text;
use crate::types::{
    BulkEditFailure, BulkEditRequest, BulkEditResult, CopyableGame, LaunchOptionEdit,
};

/// `PAINT_THRESHOLD` (useBulkEdit line 34): manhattan pixels before a press
/// becomes a paint drag.
pub const PAINT_THRESHOLD: f32 = 5.0;

/// `isBulkEditToggleAllowed` (App.svelte lines 571-577): the keyboard and
/// palette toggle.
pub fn toggle_allowed(
    def: Option<&PlatformDef>,
    active_tab_usable: bool,
    settings_open: bool,
) -> bool {
    def.is_some_and(|def| def.capabilities.bulk_edit) && active_tab_usable && !settings_open
}

/// `showBulkEdit` of the titlebar (App.svelte line 765).
pub fn titlebar_button_visible(
    def: Option<&PlatformDef>,
    active_tab_usable: bool,
    settings_open: bool,
    personas_open: bool,
) -> bool {
    toggle_allowed(def, active_tab_usable, settings_open) && !personas_open
}

/// The paint gesture state (lines 30-41).
#[derive(Debug, Clone, Default)]
struct Paint {
    active: bool,
    /// Select (true) or deselect, locked by the start card for the gesture.
    desired: bool,
    moved: bool,
    start: (f32, f32),
    start_id: Option<String>,
}

/// `createBulkEditController`.
#[derive(Debug, Default)]
pub struct BulkEdit {
    /// The platform whose bar is open; `Some` is `bulkEditMode`.
    platform_id: Option<String>,
    /// `bulkEditSelectedIds`, in insertion order like a JS `Set`.
    selected: Vec<String>,
    bar: Option<BulkEditBar>,
    paint: Paint,
    eat_next_click: bool,
}

impl BulkEdit {
    pub fn new() -> Self {
        Self::default()
    }

    /// `bulkEditMode`.
    pub fn is_open(&self) -> bool {
        self.platform_id.is_some()
    }

    /// The platform the open bar belongs to.
    pub fn platform_id(&self) -> Option<&str> {
        self.platform_id.as_deref()
    }

    /// `bulkEditSelectedIds`.
    pub fn selected_ids(&self) -> &[String] {
        &self.selected
    }

    pub fn is_selected(&self, account_id: &str) -> bool {
        self.selected.iter().any(|id| id == account_id)
    }

    /// The bar model while the mode is open.
    pub fn bar(&self) -> Option<&BulkEditBar> {
        self.bar.as_ref()
    }

    pub fn bar_mut(&mut self) -> Option<&mut BulkEditBar> {
        self.bar.as_mut()
    }

    /// `bulkEditActiveAccountSelected` (lines 57-61).
    pub fn active_account_selected(&self, current_account_id: Option<&str>) -> bool {
        current_account_id.is_some_and(|id| !id.is_empty() && self.is_selected(id))
    }

    fn set_selected(&mut self, account_id: &str, selected: bool) {
        if selected == self.is_selected(account_id) {
            return;
        }
        if selected {
            self.selected.push(account_id.to_string());
        } else {
            self.selected.retain(|id| id != account_id);
        }
    }

    /// `toggleBulkEdit` (lines 63-94): closes an open mode, or opens the bar
    /// of the active platform when it declares the capability.
    pub fn toggle(&mut self, active_platform: Option<&PlatformDef>) {
        if self.is_open() {
            self.close();
            return;
        }
        let Some(def) = active_platform.filter(|def| def.capabilities.bulk_edit) else {
            return;
        };
        self.platform_id = Some(def.id.clone());
        self.bar = Some(BulkEditBar::new());
    }

    /// `toggleBulkEditAccount` (lines 96-101).
    pub fn toggle_account(&mut self, account_id: &str) {
        let selected = self.is_selected(account_id);
        self.set_selected(account_id, !selected);
    }

    /// `bulkEditSelectAll` (lines 103-110): additive, the selection spans the
    /// whole platform. `visible_ids` is what the user can see (App.svelte
    /// lines 151-154: never off-screen accounts).
    pub fn select_all(&mut self, visible_ids: &[String]) {
        for id in visible_ids {
            self.set_selected(id, true);
        }
    }

    /// `bulkEditDeselectAll` (lines 112-114): the reset, across folders.
    pub fn deselect_all(&mut self) {
        self.selected.clear();
    }

    /// `closeBulkEdit` (lines 116-120): also on a tab change.
    pub fn close(&mut self) {
        self.platform_id = None;
        self.bar = None;
        self.selected.clear();
        self.paint = Paint::default();
    }

    // ------------------------------------------------------------ paint

    /// `handlePaintMouseDown` (lines 124-136): arms the gesture on a primary
    /// press over a card. True when it armed, so the caller skips the
    /// reorder drag and keeps the cards in place.
    pub fn paint_mouse_down(
        &mut self,
        primary: bool,
        card_id: Option<&str>,
        pos: (f32, f32),
    ) -> bool {
        if !self.is_open() || !primary {
            return false;
        }
        let Some(account_id) = card_id.filter(|id| !id.is_empty()) else {
            return false;
        };
        self.paint = Paint {
            active: true,
            desired: !self.is_selected(account_id),
            moved: false,
            start: pos,
            start_id: Some(account_id.to_string()),
        };
        true
    }

    /// `handlePaintMouseMove` (lines 138-149). `card_id` is the card under
    /// the pointer.
    pub fn paint_mouse_move(&mut self, pos: (f32, f32), card_id: Option<&str>) {
        if !self.paint.active {
            return;
        }
        if !self.paint.moved {
            let distance = (pos.0 - self.paint.start.0).abs() + (pos.1 - self.paint.start.1).abs();
            if distance < PAINT_THRESHOLD {
                return;
            }
            self.paint.moved = true;
            if let Some(start) = self.paint.start_id.clone() {
                self.set_selected(&start, self.paint.desired);
            }
        }
        if let Some(id) = card_id.filter(|id| !id.is_empty()) {
            self.set_selected(id, self.paint.desired);
        }
    }

    /// `handlePaintMouseUp` (lines 151-166). After a real drag the click
    /// that follows the release is swallowed for this frame.
    pub fn paint_mouse_up(&mut self) {
        if !self.paint.active {
            return;
        }
        let moved = self.paint.moved;
        self.paint = Paint::default();
        if moved {
            self.eat_next_click = true;
        }
    }

    /// `handlePaintCaptureClick` (lines 168-174): true when the click must
    /// not reach the card (no single-click toggle after a drag).
    pub fn paint_capture_click(&mut self) -> bool {
        std::mem::take(&mut self.eat_next_click)
    }

    /// The `setTimeout(0)` that drops an unclaimed swallow: call at the end
    /// of the frame that handled the release.
    pub fn end_frame(&mut self) {
        self.eat_next_click = false;
    }

    pub fn is_painting(&self) -> bool {
        self.paint.active
    }

    // ------------------------------------------------------------ bar jobs

    /// The bar's "Next": [`BulkEditBar::go_to_settings`] with the selection.
    pub fn go_to_settings(&mut self, jobs: &Jobs) {
        let first = self.selected.first().cloned();
        if let Some(bar) = self.bar.as_mut() {
            bar.go_to_settings(jobs, first);
        }
    }

    /// The bar's "Apply": [`BulkEditBar::apply`] with the selection.
    pub fn apply(&mut self, jobs: &Jobs) {
        let selected = self.selected.clone();
        if let Some(bar) = self.bar.as_mut() {
            bar.apply(jobs, &selected);
        }
    }

    /// Escape (`handleOverlayKeydown`, bar lines 142-150): closes the color
    /// menu, else steps back, else closes the mode.
    pub fn escape(&mut self) {
        if let Some(bar) = self.bar.as_mut() {
            if bar.escape() {
                self.close();
            }
        }
    }

    /// One frame: applies finished bar jobs. A finished apply closes the mode
    /// (`onResult` then `onClose`) and hands the result over for the toast.
    pub fn poll(&mut self) -> Option<BulkEditResult> {
        let result = self.bar.as_mut()?.poll()?;
        self.close();
        Some(result)
    }

    /// A games load or an apply is in flight.
    pub fn is_busy(&self) -> bool {
        self.bar.as_ref().is_some_and(BulkEditBar::is_busy)
    }
}

/// `TriState` of the bar (line 14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TriState {
    #[default]
    Unchanged,
    Enabled,
    Disabled,
}

impl TriState {
    /// `cycleTriState` (lines 72-76).
    pub fn cycle(self) -> Self {
        match self {
            TriState::Unchanged => TriState::Enabled,
            TriState::Enabled => TriState::Disabled,
            TriState::Disabled => TriState::Unchanged,
        }
    }

    /// `triStateLabel` (lines 78-82).
    pub fn label(self) -> Text {
        Text::key(match self {
            TriState::Unchanged => "bulkEdit.unchanged",
            TriState::Enabled => "common.enabled",
            TriState::Disabled => "common.disabled",
        })
    }

    /// `triStateToValue` (lines 84-88).
    pub fn value(self) -> Option<bool> {
        match self {
            TriState::Unchanged => None,
            TriState::Enabled => Some(true),
            TriState::Disabled => Some(false),
        }
    }

    /// `class:active` on the button.
    pub fn is_changed(self) -> bool {
        self != TriState::Unchanged
    }
}

/// The bar's two steps (line 62).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BarStep {
    /// The bottom bar with the selection tools.
    #[default]
    Select,
    /// The settings dialog.
    Settings,
}

/// One launch option row (`LaunchOptionEdit` of steamApi.ts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchOptionDraft {
    pub app_id: String,
    pub value: String,
}

/// The state of `BulkEditBar.svelte`.
#[derive(Debug, Default)]
pub struct BulkEditBar {
    step: BarStep,
    color_menu_open: bool,
    pub news_popup: TriState,
    pub do_not_disturb: TriState,
    launch_option_edits: Vec<LaunchOptionDraft>,
    games: Vec<CopyableGame>,
    games_pending: Option<Pending<BackendResult<Vec<CopyableGame>>>>,
    /// `bind:value` of the game select; empty is "Select a game".
    pub selected_game_id: String,
    /// `bind:value` of the launch option field.
    pub launch_option_input: String,
    apply_pending: Option<Pending<BackendResult<BulkEditResult>>>,
}

impl BulkEditBar {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn step(&self) -> BarStep {
        self.step
    }

    pub fn color_menu_open(&self) -> bool {
        self.color_menu_open
    }

    /// `gamesLoading`.
    pub fn games_loading(&self) -> bool {
        self.games_pending.is_some()
    }

    pub fn games(&self) -> &[CopyableGame] {
        &self.games
    }

    pub fn launch_option_edits(&self) -> &[LaunchOptionDraft] {
        &self.launch_option_edits
    }

    /// `applying`.
    pub fn applying(&self) -> bool {
        self.apply_pending.is_some()
    }

    pub fn is_busy(&self) -> bool {
        self.games_loading() || self.applying()
    }

    /// `bulkEdit.selected { count }`, in the bar and the dialog header.
    pub fn selected_label(count: usize) -> Text {
        Text::with("bulkEdit.selected", [("count", count as u64)])
    }

    /// Copy URLs, color and Next are off without a selection (lines 160,
    /// 167, 196).
    pub fn selection_actions_enabled(selected_count: usize) -> bool {
        selected_count > 0
    }

    /// The apply button (line 287).
    pub fn apply_enabled(&self, selected_count: usize) -> bool {
        !self.applying() && selected_count > 0
    }

    /// The apply button text (line 288): `...` while applying.
    pub fn apply_label(&self) -> Text {
        if self.applying() {
            Text::raw("...")
        } else {
            Text::key("bulkEdit.apply")
        }
    }

    /// `copyUrls` (lines 38-40): the profile URLs to put on the clipboard.
    pub fn copy_urls(selected_ids: &[String]) -> Vec<String> {
        selected_ids.iter().map(|id| to_profile_url(id)).collect()
    }

    /// `toggleColorMenu` (lines 45-47).
    pub fn toggle_color_menu(&mut self) {
        self.color_menu_open = !self.color_menu_open;
    }

    /// `applyColor` (lines 49-52): closes the menu and returns the color for
    /// `onSetCardColor` (empty clears it).
    pub fn apply_color(&mut self, color: &str) -> String {
        self.color_menu_open = false;
        color.to_string()
    }

    /// `handleWindowPointerDown` (lines 56-60): a press outside the color
    /// popover closes it, and the press still runs its own action.
    pub fn pointer_down(&mut self, inside_color_menu: bool) {
        if self.color_menu_open && !inside_color_menu {
            self.color_menu_open = false;
        }
    }

    /// `gameName` (lines 90-92).
    pub fn game_name<'a>(&'a self, app_id: &'a str) -> &'a str {
        self.games
            .iter()
            .find(|game| game.app_id == app_id)
            .map_or(app_id, |game| game.name.as_str())
    }

    /// `goToSettings` (lines 94-106): the games of the first selected
    /// account fill the launch option picker, once.
    pub fn go_to_settings(&mut self, jobs: &Jobs, first_selected: Option<String>) {
        self.step = BarStep::Settings;
        if !self.games.is_empty() || self.games_pending.is_some() {
            return;
        }
        if let Some(steam_id) = first_selected {
            self.games_pending =
                Some(jobs.run(move |backend| backend.steam_get_account_games(&steam_id)));
        }
    }

    /// Back to the selection (the overlay click and the Back button).
    pub fn back(&mut self) {
        self.step = BarStep::Select;
    }

    /// The add button is enabled (line 256).
    pub fn can_add_launch_option(&self) -> bool {
        !self.selected_game_id.is_empty() && !crate::js::trim(&self.launch_option_input).is_empty()
    }

    /// `addLaunchOption` (lines 108-116): one row per game, the last one wins.
    pub fn add_launch_option(&mut self) {
        if !self.can_add_launch_option() {
            return;
        }
        let app_id = std::mem::take(&mut self.selected_game_id);
        let value = crate::js::trim(&self.launch_option_input).to_string();
        self.launch_option_input.clear();
        self.launch_option_edits
            .retain(|edit| edit.app_id != app_id);
        self.launch_option_edits
            .push(LaunchOptionDraft { app_id, value });
    }

    /// `removeLaunchOption` (lines 118-120).
    pub fn remove_launch_option(&mut self, app_id: &str) {
        self.launch_option_edits
            .retain(|edit| edit.app_id != app_id);
    }

    /// The request `applyEdits` sends (lines 126-131).
    pub fn request(&self, selected_ids: &[String]) -> BulkEditRequest {
        BulkEditRequest {
            steam_ids: selected_ids.to_vec(),
            news_popup: self.news_popup.value(),
            do_not_disturb: self.do_not_disturb.value(),
            launch_options: self
                .launch_option_edits
                .iter()
                .map(|edit| LaunchOptionEdit {
                    app_id: edit.app_id.clone(),
                    value: edit.value.clone(),
                })
                .collect(),
        }
    }

    /// `applyEdits` (lines 122-140), up to the backend call (logged by
    /// `steamApi.bulkEdit`).
    pub fn apply(&mut self, jobs: &Jobs, selected_ids: &[String]) {
        if selected_ids.is_empty() || self.apply_pending.is_some() {
            return;
        }
        let request = self.request(selected_ids);
        self.apply_pending = Some(jobs.run(move |backend| steam_api::bulk_edit(backend, request)));
    }

    /// `handleOverlayKeydown` (lines 142-150). True when the mode closes.
    pub fn escape(&mut self) -> bool {
        if self.color_menu_open {
            self.color_menu_open = false;
            return false;
        }
        if self.step == BarStep::Settings {
            self.step = BarStep::Select;
            return false;
        }
        true
    }

    /// Applies finished jobs. `Some` once the apply ended: its result, or the
    /// error as one `all` failure (line 135).
    pub fn poll(&mut self) -> Option<BulkEditResult> {
        if let Some(pending) = self.games_pending.as_mut() {
            match pending.poll() {
                Poll::Waiting => {}
                Poll::Ready(result) => {
                    self.games_pending = None;
                    self.games = result.unwrap_or_default();
                }
                Poll::Gone => self.games_pending = None,
            }
        }
        let pending = self.apply_pending.as_mut()?;
        let result = match pending.poll() {
            Poll::Waiting => return None,
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(error)) => failed_everywhere(error.message),
            Poll::Gone => failed_everywhere("Bulk edit stopped".to_string()),
        };
        self.apply_pending = None;
        Some(result)
    }
}

fn failed_everywhere(error: String) -> BulkEditResult {
    BulkEditResult {
        succeeded: 0,
        failed: vec![BulkEditFailure {
            steam_id: "all".to_string(),
            error,
        }],
    }
}
