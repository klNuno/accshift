//! Session marks shared by every platform: what the backend learned about an
//! account's saved session (expired, or the launcher does not keep the
//! password) and how a card, a list row and a tooltip show it. Orange chip and
//! outline, never red: the account still works once the user signs in.

use super::warnings::{WarningChip, WarningPresentation, WarningTone};
use crate::error::BackendResult;
use crate::text::Text;
use crate::types::SessionMarkInfo;
use std::collections::BTreeMap;

/// What a mark says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// The launcher refused the saved session: sign in again.
    Expired,
    /// The launcher does not keep the password: it asks at every switch.
    PasswordNeeded,
}

impl SessionState {
    /// The backend's wording; anything else marks nothing.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "expired" => Some(Self::Expired),
            "passwordNeeded" => Some(Self::PasswordNeeded),
            _ => None,
        }
    }
}

/// The warning of a mark.
pub fn presentation(state: SessionState) -> WarningPresentation {
    let (chip, tooltip) = match state {
        SessionState::Expired => ("session.expired", "session.expiredTooltip"),
        SessionState::PasswordNeeded => ("session.passwordNeeded", "session.passwordNeededTooltip"),
    };
    WarningPresentation {
        tooltip: Text::key(tooltip),
        card_outline_tone: Some(WarningTone::Blue),
        list_has_red: false,
        list_has_orange: false,
        list_has_blue: true,
        chips: vec![WarningChip {
            tone: WarningTone::Blue,
            text: Text::key(chip),
        }],
    }
}

/// Adds the session warnings to a platform's other warnings. An account with
/// both keeps its own tooltip and outline (red wins) and gains the chip.
pub fn overlay(
    warnings: &mut BTreeMap<String, WarningPresentation>,
    sessions: BTreeMap<String, WarningPresentation>,
) {
    for (id, session) in sessions {
        match warnings.get_mut(&id) {
            // A clean account (Steam lists every one) takes the session
            // warning whole.
            Some(existing) if existing.chips.is_empty() && existing.card_outline_tone.is_none() => {
                *existing = session;
            }
            Some(existing) => {
                existing.list_has_blue = true;
                if existing.card_outline_tone.is_none() {
                    existing.card_outline_tone = Some(WarningTone::Blue);
                }
                for chip in session.chips {
                    if !existing.chips.contains(&chip) {
                        existing.chips.push(chip);
                    }
                }
            }
            None => {
                warnings.insert(id, session);
            }
        }
    }
}

/// The marks this session knows of, by account id.
#[derive(Debug, Clone, Default)]
pub struct SessionMarks {
    marks: BTreeMap<String, SessionState>,
}

impl SessionMarks {
    pub fn mark(&mut self, account_id: &str, state: SessionState) {
        if !account_id.is_empty() {
            self.marks.insert(account_id.to_string(), state);
        }
    }

    /// A fresh capture or a switch that ended signed in.
    pub fn clear(&mut self, account_id: &str) {
        self.marks.remove(account_id);
    }

    pub fn state(&self, account_id: &str) -> Option<SessionState> {
        self.marks.get(account_id).copied()
    }

    pub fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }

    /// Replaces every mark with these ones.
    pub fn replace(&mut self, marks: impl IntoIterator<Item = (String, SessionState)>) {
        self.marks = marks.into_iter().filter(|(id, _)| !id.is_empty()).collect();
    }

    /// Stores the backend's answer; a failure keeps what was known, and a
    /// state the app does not know marks nothing.
    pub fn finish_load(&mut self, result: BackendResult<Vec<SessionMarkInfo>>) {
        if let Ok(marks) = result {
            self.replace(marks.into_iter().filter_map(|mark| {
                SessionState::parse(&mark.state).map(|state| (mark.account_id, state))
            }));
        }
    }

    /// The marks as a warning map, each shown by `show`.
    pub fn warning_map_with(
        &self,
        show: impl Fn(SessionState) -> WarningPresentation,
    ) -> BTreeMap<String, WarningPresentation> {
        self.marks
            .iter()
            .map(|(id, state)| (id.clone(), show(*state)))
            .collect()
    }

    /// The marks with the shared `session.*` wording.
    pub fn warning_map(&self) -> BTreeMap<String, WarningPresentation> {
        self.warning_map_with(presentation)
    }
}
