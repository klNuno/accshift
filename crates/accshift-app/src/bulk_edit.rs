//! Bulk edit: the multi-select mode and the model of the Steam bulk edit bar
//! (selection rules, request building, progress and result states).
//!
//! The bar is a plain model created when the mode opens. The paint gesture
//! reads the card under the pointer from the UI.
//!
//! The clipboard write of the URLs and the card color write belong to the
//! card-action module: [`BulkEditBar::copy_urls`] and
//! [`BulkEditBar::apply_color`] hand the values over. [`BulkEdit::poll`]
//! reports finished jobs with the toast each one needs.

use crate::accounts::{Toast, ToastKind};
use crate::error::BackendResult;
use crate::jobs::{Jobs, Pending, Poll};
use crate::platforms::steam::{api as steam_api, to_profile_url};
use crate::platforms::{PlatformBulkEditResult, PlatformDef};
use crate::text::Text;
use crate::types::{
    BulkEditFailure, BulkEditRequest, BulkEditResult, CopyableGame, LaunchOptionEdit,
};

/// Manhattan pixels before a press becomes a paint drag.
pub const PAINT_THRESHOLD: f32 = 5.0;

/// Whether the keyboard shortcut and the palette may toggle the mode: the
/// platform supports it, its tab is usable and the settings are closed.
pub fn toggle_allowed(
    def: Option<&PlatformDef>,
    active_tab_usable: bool,
    settings_open: bool,
) -> bool {
    def.is_some_and(|def| def.capabilities.bulk_edit) && active_tab_usable && !settings_open
}

/// The paint gesture: a press on a card, then a drag that selects or
/// deselects every card it crosses.
#[derive(Debug, Clone, Default)]
struct Paint {
    active: bool,
    /// Select (true) or deselect, locked by the start card for the gesture.
    desired: bool,
    moved: bool,
    start: (f32, f32),
    start_id: Option<String>,
}

type ApplyJob = Pending<BackendResult<BulkEditResult>>;

/// A bulk edit job that ended, as [`BulkEdit::poll`] reports it.
#[derive(Debug)]
pub enum BulkEditOutcome {
    /// An apply ended: its result, or the error as one `all` failure.
    Applied(BulkEditResult),
    /// The games of the launch option picker could not be read.
    GamesFailed(String),
}

impl BulkEditOutcome {
    /// The toast the outcome shows.
    pub fn toast(&self) -> Toast {
        match self {
            BulkEditOutcome::Applied(result) => {
                crate::actions::dialogs::bulk_edit_result_toast(&PlatformBulkEditResult {
                    succeeded: result.succeeded as usize,
                    failed: result.failed.iter().map(|f| f.steam_id.clone()).collect(),
                })
            }
            BulkEditOutcome::GamesFailed(error) => Toast::new(
                Text::with("bulkEdit.gamesLoadFailed", [("error", error.as_str())]),
                ToastKind::Error,
            ),
        }
    }
}

/// The multi-select mode: the selection, the paint gesture and the bar.
#[derive(Debug, Default)]
pub struct BulkEdit {
    /// The platform whose bar is open; `Some` while the mode is open.
    platform_id: Option<String>,
    /// The selected account ids, in selection order.
    selected: Vec<String>,
    bar: Option<BulkEditBar>,
    /// Applies still writing after their bar closed: their result still
    /// needs its toast.
    detached: Vec<ApplyJob>,
    paint: Paint,
    eat_next_click: bool,
}

impl BulkEdit {
    pub fn new() -> Self {
        Self::default()
    }

    /// The mode is open.
    pub fn is_open(&self) -> bool {
        self.platform_id.is_some()
    }

    /// The platform the open bar belongs to.
    pub fn platform_id(&self) -> Option<&str> {
        self.platform_id.as_deref()
    }

    /// The selected account ids, in selection order.
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

    /// The account signed in on the platform is part of the selection.
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

    /// Closes an open mode, or opens the bar of the active platform when it declares the capability.
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

    /// Selects or deselects one account.
    pub fn toggle_account(&mut self, account_id: &str) {
        let selected = self.is_selected(account_id);
        self.set_selected(account_id, !selected);
    }

    /// Adds the visible accounts to the selection, which spans the whole
    /// platform. `visible_ids` is what the user can see, never off-screen
    /// accounts.
    pub fn select_all(&mut self, visible_ids: &[String]) {
        for id in visible_ids {
            self.set_selected(id, true);
        }
    }

    /// Clears the selection, across folders.
    pub fn deselect_all(&mut self) {
        self.selected.clear();
    }

    /// Closes the mode, also on a tab change. An apply in
    /// flight keeps running and [`Self::poll`] still reports it.
    pub fn close(&mut self) {
        self.platform_id = None;
        if let Some(job) = self.bar.take().and_then(|mut bar| bar.apply_pending.take()) {
            self.detached.push(job);
        }
        self.selected.clear();
        self.paint = Paint::default();
    }

    // ------------------------------------------------------------ paint

    /// Arms the paint gesture on a primary press over a card. True when it armed, so the caller skips the
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

    /// Past the threshold, paints the start card and every card the pointer
    /// crosses. `card_id` is the card under the pointer.
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

    /// Ends the gesture. After a real drag the click that follows the release
    /// is swallowed for this frame.
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

    /// True when the click must not reach the card (no single-click toggle
    /// after a drag).
    pub fn paint_capture_click(&mut self) -> bool {
        std::mem::take(&mut self.eat_next_click)
    }

    /// Drops an unclaimed swallow: call at the end of the frame that handled
    /// the release.
    pub fn end_frame(&mut self) {
        self.eat_next_click = false;
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

    /// Escape: closes the color menu, else steps back, else closes the mode.
    pub fn escape(&mut self) {
        if let Some(bar) = self.bar.as_mut() {
            if bar.escape() {
                self.close();
            }
        }
    }

    /// One frame: what the finished jobs report, in order. A finished apply
    /// of the open bar reports its result, then closes the mode; one that
    /// outlived its bar leaves the mode as it is.
    pub fn poll(&mut self) -> Vec<BulkEditOutcome> {
        let mut outcomes = Vec::new();
        let mut waiting = Vec::new();
        for mut job in std::mem::take(&mut self.detached) {
            match apply_poll(&mut job) {
                Some(result) => outcomes.push(BulkEditOutcome::Applied(result)),
                None => waiting.push(job),
            }
        }
        self.detached = waiting;
        if let Some(bar) = self.bar.as_mut() {
            let from_bar = bar.poll();
            if from_bar
                .iter()
                .any(|outcome| matches!(outcome, BulkEditOutcome::Applied(_)))
            {
                self.close();
            }
            outcomes.extend(from_bar);
        }
        outcomes
    }

    /// A games load or an apply is in flight, a detached apply included.
    pub fn is_busy(&self) -> bool {
        !self.detached.is_empty() || self.bar.as_ref().is_some_and(BulkEditBar::is_busy)
    }
}

/// A toggle of the bar that can also leave the setting as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TriState {
    #[default]
    Unchanged,
    Enabled,
    Disabled,
}

impl TriState {
    /// The next state on a click: unchanged, enabled, disabled, unchanged.
    pub fn cycle(self) -> Self {
        match self {
            TriState::Unchanged => TriState::Enabled,
            TriState::Enabled => TriState::Disabled,
            TriState::Disabled => TriState::Unchanged,
        }
    }

    /// The button text.
    pub fn label(self) -> Text {
        Text::key(match self {
            TriState::Unchanged => "bulkEdit.unchanged",
            TriState::Enabled => "common.enabled",
            TriState::Disabled => "common.disabled",
        })
    }

    /// The value the request carries, `None` to leave the setting alone.
    pub fn value(self) -> Option<bool> {
        match self {
            TriState::Unchanged => None,
            TriState::Enabled => Some(true),
            TriState::Disabled => Some(false),
        }
    }

    /// The button shows as active.
    pub fn is_changed(self) -> bool {
        self != TriState::Unchanged
    }
}

/// The bar's two steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BarStep {
    /// The bottom bar with the selection tools.
    #[default]
    Select,
    /// The settings dialog.
    Settings,
}

/// One launch option row: a game and the options it gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchOptionDraft {
    pub app_id: String,
    pub value: String,
}

/// The state of the bulk edit bar and its settings dialog.
#[derive(Debug, Default)]
pub struct BulkEditBar {
    step: BarStep,
    color_menu_open: bool,
    pub news_popup: TriState,
    pub do_not_disturb: TriState,
    launch_option_edits: Vec<LaunchOptionDraft>,
    games: Vec<CopyableGame>,
    games_pending: Option<Pending<BackendResult<Vec<CopyableGame>>>>,
    /// The game picked in the select; empty is "Select a game".
    pub selected_game_id: String,
    /// The text of the launch option field.
    pub launch_option_input: String,
    apply_pending: Option<ApplyJob>,
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

    /// The games of the picker are loading.
    pub fn games_loading(&self) -> bool {
        self.games_pending.is_some()
    }

    pub fn games(&self) -> &[CopyableGame] {
        &self.games
    }

    pub fn launch_option_edits(&self) -> &[LaunchOptionDraft] {
        &self.launch_option_edits
    }

    /// An apply is in flight.
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

    /// Copy URLs, color and Next are off without a selection.
    pub fn selection_actions_enabled(selected_count: usize) -> bool {
        selected_count > 0
    }

    /// The apply button is enabled.
    pub fn apply_enabled(&self, selected_count: usize) -> bool {
        !self.applying() && selected_count > 0
    }

    /// The apply button text: `...` while applying.
    pub fn apply_label(&self) -> Text {
        if self.applying() {
            Text::raw("...")
        } else {
            Text::key("bulkEdit.apply")
        }
    }

    /// The profile URLs to put on the clipboard.
    pub fn copy_urls(selected_ids: &[String]) -> Vec<String> {
        selected_ids.iter().map(|id| to_profile_url(id)).collect()
    }

    /// Opens or closes the color menu.
    pub fn toggle_color_menu(&mut self) {
        self.color_menu_open = !self.color_menu_open;
    }

    /// Closes the menu and returns the color to set on the selected cards
    /// (empty clears it).
    pub fn apply_color(&mut self, color: &str) -> String {
        self.color_menu_open = false;
        color.to_string()
    }

    /// A press outside the color popover closes it, and the press still runs its own action.
    pub fn pointer_down(&mut self, inside_color_menu: bool) {
        if self.color_menu_open && !inside_color_menu {
            self.color_menu_open = false;
        }
    }

    /// The name of a game of the picker, its app id when unknown.
    pub fn game_name<'a>(&'a self, app_id: &'a str) -> &'a str {
        self.games
            .iter()
            .find(|game| game.app_id == app_id)
            .map_or(app_id, |game| game.name.as_str())
    }

    /// Opens the settings step. The games of the first selected account fill
    /// the launch option picker, once.
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

    /// The add button is enabled.
    pub fn can_add_launch_option(&self) -> bool {
        !self.selected_game_id.is_empty() && !crate::js::trim(&self.launch_option_input).is_empty()
    }

    /// Adds the typed launch option: one row per game, the last one wins.
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

    /// Removes the row of a game.
    pub fn remove_launch_option(&mut self, app_id: &str) {
        self.launch_option_edits
            .retain(|edit| edit.app_id != app_id);
    }

    /// The request an apply sends.
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

    /// Sends the edits for the selected accounts, unless one apply is
    /// already in flight.
    pub fn apply(&mut self, jobs: &Jobs, selected_ids: &[String]) {
        if selected_ids.is_empty() || self.apply_pending.is_some() {
            return;
        }
        let request = self.request(selected_ids);
        self.apply_pending = Some(jobs.run(move |backend| steam_api::bulk_edit(backend, request)));
    }

    /// Escape: closes the color menu, else steps back. True when the mode
    /// closes.
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

    /// Applies finished jobs: a failed games load, then the apply result.
    pub fn poll(&mut self) -> Vec<BulkEditOutcome> {
        let mut outcomes = Vec::new();
        if let Some(pending) = self.games_pending.as_mut() {
            let failure = match pending.poll() {
                Poll::Waiting => None,
                Poll::Ready(Ok(games)) => {
                    self.games = games;
                    self.games_pending = None;
                    None
                }
                Poll::Ready(Err(error)) => Some(error.message),
                Poll::Gone => Some("games load stopped".to_string()),
            };
            if let Some(error) = failure {
                self.games_pending = None;
                outcomes.push(BulkEditOutcome::GamesFailed(error));
            }
        }
        if let Some(job) = self.apply_pending.as_mut() {
            if let Some(result) = apply_poll(job) {
                self.apply_pending = None;
                outcomes.push(BulkEditOutcome::Applied(result));
            }
        }
        outcomes
    }
}

/// The apply result once its job ended, the error as one `all` failure.
fn apply_poll(job: &mut ApplyJob) -> Option<BulkEditResult> {
    match job.poll() {
        Poll::Waiting => None,
        Poll::Ready(Ok(result)) => Some(result),
        Poll::Ready(Err(error)) => Some(failed_everywhere(error.message)),
        Poll::Gone => Some(failed_everywhere("Bulk edit stopped".to_string())),
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
