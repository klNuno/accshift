//! Account warning presentation.

use crate::text::Text;

/// The colour of a warning chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WarningTone {
    Red,
    Orange,
}

/// One warning chip.
#[derive(Debug, Clone, PartialEq)]
pub struct WarningChip {
    pub tone: WarningTone,
    pub text: Text,
}

/// What a card, a list row and a tooltip show for an account with a warning.
#[derive(Debug, Clone, PartialEq)]
pub struct WarningPresentation {
    pub tooltip: Text,
    pub card_outline_tone: Option<WarningTone>,
    pub list_has_red: bool,
    pub list_has_orange: bool,
    pub chips: Vec<WarningChip>,
}

/// What a warning load hands back: the map to show, how the load ended
/// (`None` when it does not report an end) and the toasts to show.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WarningLoad {
    pub warnings: std::collections::BTreeMap<String, WarningPresentation>,
    pub outcome: Option<super::WarningRefreshOutcome>,
    pub toasts: Vec<super::ToastRequest>,
}
