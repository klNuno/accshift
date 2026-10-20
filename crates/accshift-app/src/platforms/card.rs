//! Card extension content, ported from `src/lib/shared/cardExtension.ts`: the
//! sections a card shows when it expands (the CS2 level and XP bar today).

use super::warnings::{WarningChip, WarningTone};
use crate::text::Text;

/// `CardExtensionChipTone`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChipTone {
    Red,
    Orange,
    Blue,
    Green,
    Slate,
}

impl From<WarningTone> for ChipTone {
    fn from(tone: WarningTone) -> Self {
        match tone {
            WarningTone::Red => ChipTone::Red,
            WarningTone::Orange => ChipTone::Orange,
        }
    }
}

/// `CardExtensionChip`. The TS `onClick` is a UI concern and stays there.
#[derive(Debug, Clone, PartialEq)]
pub struct CardChip {
    pub text: Text,
    pub tone: ChipTone,
}

/// `CardExtensionLink`.
#[derive(Debug, Clone, PartialEq)]
pub struct CardLink {
    pub label: Text,
    pub url: String,
}

/// `CardExtensionProgress`.
#[derive(Debug, Clone, PartialEq)]
pub struct CardProgress {
    pub value: f64,
    pub max: f64,
    /// Small text right of the bar, e.g. "3579/5000".
    pub label: Option<String>,
}

/// `CardExtensionSection`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CardSection {
    pub title: Option<Text>,
    pub text: Option<Text>,
    pub lines: Vec<String>,
    pub link: Option<CardLink>,
    pub chips: Vec<CardChip>,
    pub progress: Option<CardProgress>,
    pub loading: bool,
}

/// `AccountUsernameBadge`: the status icon next to the username.
#[derive(Debug, Clone, PartialEq)]
pub struct UsernameBadge {
    /// `Green` or `Slate`.
    pub tone: ChipTone,
    pub label: Text,
}

fn text_is_blank(text: &Option<Text>) -> bool {
    match text {
        None => true,
        Some(Text::Raw(raw)) => raw.trim().is_empty(),
        // A key always resolves to something.
        Some(Text::Key { .. }) => false,
    }
}

/// `hasCardExtensionContent`.
pub fn has_card_extension_content(sections: &[CardSection]) -> bool {
    sections.iter().any(|section| {
        !text_is_blank(&section.title)
            || !text_is_blank(&section.text)
            || section.lines.iter().any(|line| !line.trim().is_empty())
            || !section.chips.is_empty()
            || section.progress.is_some()
            || section.loading
    })
}

/// `warningChipsToExtensionChips`.
pub fn warning_chips_to_extension_chips(chips: &[WarningChip]) -> Vec<CardChip> {
    chips
        .iter()
        .map(|chip| CardChip {
            text: chip.text.clone(),
            tone: chip.tone.into(),
        })
        .collect()
}
