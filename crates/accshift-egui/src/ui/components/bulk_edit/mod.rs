//! The bulk edit bar (Steam), drawn after the confirm dialog. Two steps,
//! each built fresh:
//!
//! ```text
//! .bar          fixed bottom, --z-sticky, --bg-overlay, 1px top --border,
//!               padding 8px 16px, space-between, slideUp 120ms ease-out
//!   .bar-left   Select all, Deselect all, Copy URLs, Card color (and its
//!               popover), the count; 8 px gaps
//!   .bar-right  Cancel, Next
//! .overlay      fixed, --z-dialog, rgba(0,0,0,.5), blur(4px), fade 100ms
//!   .dialog     440 px, padding 16, --bg-overlay, radius 10, 14 px gaps
//!     header    Settings, the count
//!     body      two tri-state rows, the launch options, the warning
//!     actions   Back, Apply
//! ```
//!
//! The model is [`BulkEditBar`]; this module reads it and returns what the
//! user did. Escape (the bar's window listener) closes the colour menu,
//! steps back, or closes the mode.

use egui::{vec2, Key, Rect, Ui, Vec2};

use accshift_app::bulk_edit::BulkEditBar;
use accshift_app::i18n::Locale;
use accshift_app::text::Text;

use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim::{Animation, Timing, Transition};

mod bar;
mod button;
mod settings;

pub use bar::bar;
pub use settings::settings;

/// The root id; each step mounts under `.with((step, mount))`.
pub const BULK_ID: &str = "accshift-bulk-edit";
/// Where the bar and its popover were drawn, for the paint gesture's
/// hit test: a point over them hits no card.
pub const COVER_ID: &str = "accshift-bulk-edit/cover";

/// `slideUp 120ms ease-out`.
const SLIDE_UP: Animation = Animation::new(0.12, Timing::EASE_OUT);
/// `overlay-fade-in 100ms ease-out`.
const FADE: Animation = Animation::new(0.1, Timing::EASE_OUT);
/// `dialog-slide-in 120ms ease-out`.
const SLIDE_IN: Animation = Animation::new(0.12, Timing::EASE_OUT);
/// `transition: all 100ms` (the default `ease`).
const T100: Transition = Transition::new(0.1, Timing::EASE);
/// `#f59e0b`, the warning.
const AMBER: Rgba = Rgba::hex(0xf59e0b, 1.0);
/// `.bar { padding: 8px 16px }`, also the gap between its items.
const BAR_PAD: Vec2 = vec2(16.0, 8.0);
const GAP: f32 = 8.0;
/// `.color-popover`: 6 px above the button, `padding: 7px 8px`, 6 px gaps.
const POPOVER_OFFSET: f32 = 6.0;
const POPOVER_PAD: Vec2 = vec2(8.0, 7.0);
const SWATCH: f32 = 18.0;
const SWATCH_GAP: f32 = 6.0;
const FILL: f32 = 12.0;
/// `.dialog`.
const DIALOG_W: f32 = 440.0;
const DIALOG_PAD: f32 = 16.0;
const DIALOG_GAP: f32 = 14.0;
const BODY_GAP: f32 = 10.0;
const TRI_MIN_W: f32 = 86.0;
const INPUT_W: f32 = 120.0;
const ROW_GAP: f32 = 6.0;

/// What the bar and the dialog read.
pub struct BulkProps<'a> {
    pub vars: &'a Vars,
    pub locale: Locale,
    /// The window: the bar spans its bottom, the overlay covers it.
    pub viewport: Rect,
    pub bar: &'a BulkEditBar,
    /// `selectedIds.size`.
    pub selected: usize,
    /// The active account is selected: the warning shows.
    pub active_selected: bool,
    /// `--accent-fg`, Next's and Apply's text.
    pub accent_fg: Rgba,
    pub focus_visible: bool,
    pub dark: bool,
    /// Bumped each time a step mounts: its animation starts over.
    pub mount: u64,
}

/// What the user did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BulkEvent {
    SelectAll,
    DeselectAll,
    CopyUrls,
    ToggleColorMenu,
    /// A swatch: the preset colour, empty for none.
    Color(String),
    /// Cancel: the mode closes.
    Close,
    Next,
    /// Back, or a click on the overlay.
    Back,
    Apply,
    CycleNewsPopup,
    CycleDoNotDisturb,
    /// The game select's new value, empty for "Select a game".
    Game(String),
    LaunchInput(String),
    AddLaunchOption,
    RemoveLaunchOption(String),
    /// A primary press anywhere, and whether it
    /// landed in the colour wrap.
    PointerDown(bool),
    /// Escape.
    Escape,
}

fn t(locale: Locale, key: &str) -> String {
    Text::key(key).resolve(locale)
}

/// Escape for the open step: the bar's window `keydown`.
pub fn escape_pressed(ui: &Ui) -> bool {
    ui.input(|i| i.key_pressed(Key::Escape))
}
