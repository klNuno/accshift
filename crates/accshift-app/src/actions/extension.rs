//! The extension panel content of each visible account card, ported from
//! `src/lib/app/useExtensionContent.svelte.ts`.
//!
//! A card shows, in order: the add flow's setup content when the account is
//! being set up (it replaces the rest), else a warnings section, a note
//! section (unless notes show inline) and the platform's extra sections (the
//! CS2 bridge on Steam). One memo entry per account is rebuilt only when that
//! account's own inputs change, so the other cards keep the same `Arc`.

use crate::i18n::Locale;
use crate::js;
use crate::platforms::card::{warning_chips_to_extension_chips, CardSection};
use crate::platforms::warnings::{WarningChip, WarningPresentation};
use crate::text::Text;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// `CardExtensionContent`: the sections of one card, `None` when it has none.
pub type CardExtension = Option<Arc<Vec<CardSection>>>;

/// What the memo reads on each frame.
pub struct ExtensionInputs<'a> {
    /// `shell.locale`.
    pub locale: Locale,
    /// `loader.warningStates`.
    pub warning_states: &'a BTreeMap<String, WarningPresentation>,
    /// `display.visibleRenderedAccountIds`.
    pub visible_ids: &'a [String],
    /// `addFlow.getSetupExtensionContent(id)`, its sections.
    pub setup_content: &'a dyn Fn(&str) -> Option<Vec<CardSection>>,
    /// `getAccountNote(id)`.
    pub account_note: &'a dyn Fn(&str) -> String,
    /// `cardNoteVersion`.
    pub card_note_version: u64,
    /// `settings.accountDisplay.showCardNotesInline`.
    pub show_card_notes_inline: bool,
    /// `getExtraSections(id)`: Steam's CS2 sections, nothing elsewhere.
    pub extra_sections: &'a dyn Fn(&str) -> Vec<CardSection>,
    /// `getCs2BridgeVersion()`.
    pub extra_sections_version: u64,
}

#[derive(Debug, Clone, PartialEq)]
struct SharedKey {
    locale: Locale,
    card_note_version: u64,
    show_card_notes_inline: bool,
    extra_sections_version: u64,
}

/// `accountKey(id)`: the account's own inputs.
#[derive(Debug, Clone, PartialEq)]
struct AccountKey {
    tooltip: Option<Text>,
    chips: Vec<WarningChip>,
    note: String,
    setup: Option<Vec<CardSection>>,
}

#[derive(Debug, Clone)]
struct Entry {
    key: AccountKey,
    content: CardExtension,
}

/// `createExtensionContentController`.
#[derive(Debug, Clone, Default)]
pub struct ExtensionContent {
    shared_key: Option<SharedKey>,
    entries: HashMap<String, Entry>,
    cache: Arc<HashMap<String, CardExtension>>,
}

/// `createWarningExtensionSection`: chips when the warning has any, else the
/// tooltip lines joined with a bullet.
pub fn warning_section(
    warning: Option<&WarningPresentation>,
    locale: Locale,
) -> Option<CardSection> {
    let warning = warning?;
    let chips = warning_chips_to_extension_chips(&warning.chips);
    let tooltip = warning.tooltip.resolve(locale);
    let lines: Vec<String> = tooltip
        .split('\n')
        .map(|line| js::trim(line).to_string())
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() && chips.is_empty() {
        return None;
    }
    let has_chips = !chips.is_empty();
    Some(CardSection {
        title: Some(Text::key("card.extensionWarnings")),
        text: (!has_chips).then(|| Text::raw(lines.join(" \u{2022} "))),
        lines: if has_chips { Vec::new() } else { lines },
        chips,
        ..CardSection::default()
    })
}

/// `createNoteExtensionSection`.
pub fn note_section(note: &str, show_inline: bool) -> Option<CardSection> {
    if show_inline {
        return None;
    }
    let note = js::trim(note);
    if note.is_empty() {
        return None;
    }
    Some(CardSection {
        title: Some(Text::key("card.extensionNote")),
        lines: vec![note.to_string()],
        ..CardSection::default()
    })
}

impl ExtensionContent {
    pub fn new() -> Self {
        Self::default()
    }

    fn account_key(inputs: &ExtensionInputs<'_>, id: &str) -> AccountKey {
        let warning = inputs.warning_states.get(id);
        AccountKey {
            tooltip: warning.map(|warning| warning.tooltip.clone()),
            chips: warning
                .map(|warning| warning.chips.clone())
                .unwrap_or_default(),
            note: (inputs.account_note)(id),
            setup: (inputs.setup_content)(id),
        }
    }

    fn build_content(inputs: &ExtensionInputs<'_>, id: &str, key: &AccountKey) -> CardExtension {
        if let Some(setup) = &key.setup {
            return Some(Arc::new(setup.clone()));
        }
        let mut sections = Vec::new();
        sections.extend(warning_section(
            inputs.warning_states.get(id),
            inputs.locale,
        ));
        sections.extend(note_section(&key.note, inputs.show_card_notes_inline));
        sections.extend((inputs.extra_sections)(id));
        (!sections.is_empty()).then(|| Arc::new(sections))
    }

    /// `accountExtensionContentById`: the same map `Arc` while nothing
    /// changed, and the same entry `Arc` for every card whose inputs held.
    pub fn content_by_id(
        &mut self,
        inputs: &ExtensionInputs<'_>,
    ) -> Arc<HashMap<String, CardExtension>> {
        let shared = SharedKey {
            locale: inputs.locale,
            card_note_version: inputs.card_note_version,
            show_card_notes_inline: inputs.show_card_notes_inline,
            extra_sections_version: inputs.extra_sections_version,
        };
        let same_shared = self.shared_key.as_ref() == Some(&shared);
        let previous = if same_shared {
            std::mem::take(&mut self.entries)
        } else {
            HashMap::new()
        };
        let mut changed = !same_shared || inputs.visible_ids.len() != self.cache.len();
        let mut next = HashMap::with_capacity(inputs.visible_ids.len());
        for id in inputs.visible_ids {
            let key = Self::account_key(inputs, id);
            if let Some(cached) = previous.get(id).filter(|cached| cached.key == key) {
                if !self.cache.contains_key(id) {
                    changed = true;
                }
                next.insert(id.clone(), cached.clone());
                continue;
            }
            let content = Self::build_content(inputs, id, &key);
            next.insert(id.clone(), Entry { key, content });
            changed = true;
        }
        self.shared_key = Some(shared);
        self.entries = next;
        if changed {
            let map = self
                .entries
                .iter()
                .map(|(id, entry)| (id.clone(), entry.content.clone()))
                .collect();
            self.cache = Arc::new(map);
        }
        Arc::clone(&self.cache)
    }
}
