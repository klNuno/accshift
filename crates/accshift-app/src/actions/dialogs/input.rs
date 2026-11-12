//! The input dialog: its draft, the submit rule, and what submitting does.

use super::{AppDialogs, DialogContext};
use crate::actions::AppEffect;
use crate::folders::FolderInfo;
use crate::js;
use crate::menu::InputRequest;
use crate::platforms::{PlatformAccount, PlatformAdapter, PlatformRegistry};
use crate::text::Text;

/// What confirming an input dialog does.
#[derive(Debug, Clone, PartialEq)]
pub enum InputPurpose {
    /// A folder in the current folder of the active
    /// tab, both read when confirming.
    NewFolder,
    RenameFolder {
        folder_id: String,
    },
    /// The note editor of the appearance submenu: set, or clear when blank.
    CardNote {
        account_id: String,
    },
    /// A platform menu's rename.
    PlatformInput(InputRequest),
    /// Renaming an account (F2).
    RenameAccount {
        platform_id: String,
        account_id: String,
    },
    /// Naming a just-added account: a blank answer keeps the detected name.
    RenameNewAccount {
        platform_id: String,
        account_id: String,
    },
}

/// An input dialog and the draft it edits.
#[derive(Debug, Clone, PartialEq)]
pub struct InputDialogState {
    pub title: Text,
    pub placeholder: Text,
    pub initial_value: String,
    pub allow_empty: bool,
    /// The maximum length, in UTF-16 units.
    pub maxlength: Option<usize>,
    pub purpose: InputPurpose,
    value: String,
}

impl InputDialogState {
    pub(super) fn new(
        title: Text,
        placeholder: Text,
        initial_value: String,
        allow_empty: bool,
        maxlength: Option<usize>,
        purpose: InputPurpose,
    ) -> Self {
        Self {
            title,
            placeholder,
            value: initial_value.clone(),
            initial_value,
            allow_empty,
            maxlength,
            purpose,
        }
    }

    /// The draft.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Typing: the input refuses what goes past `maxlength`.
    pub fn set_value(&mut self, value: impl Into<String>) {
        let value = value.into();
        self.value = match self.maxlength {
            Some(max) if js::utf16_len(&value) > max => js::utf16_prefix(&value, max),
            _ => value,
        };
    }

    /// The OK button: enabled with `allow_empty` or a non-blank draft.
    pub fn can_submit(&self) -> bool {
        self.allow_empty || !js::trim(&self.value).is_empty()
    }

    /// The `n/max` counter, shown in the last 20 characters.
    pub fn char_count(&self) -> Option<(usize, usize)> {
        let max = self.maxlength.filter(|max| *max > 0)?;
        let len = js::utf16_len(&self.value);
        (len + 20 >= max).then_some((len, max))
    }

    /// The value confirming applies (trimmed unless
    /// `allow_empty`), `None` when it cannot submit.
    pub fn submitted_value(&self) -> Option<String> {
        if !self.can_submit() {
            return None;
        }
        Some(if self.allow_empty {
            self.value.clone()
        } else {
            js::trim(&self.value).to_string()
        })
    }
}

impl AppDialogs {
    pub fn open_new_folder_dialog(&mut self) {
        self.input_dialog = Some(InputDialogState::new(
            Text::key("dialog.newFolderTitle"),
            Text::key("dialog.folderNamePlaceholder"),
            String::new(),
            false,
            None,
            InputPurpose::NewFolder,
        ));
    }

    pub fn open_rename_folder_dialog(&mut self, folder: &FolderInfo) {
        self.input_dialog = Some(InputDialogState::new(
            Text::key("dialog.renameFolderTitle"),
            Text::key("dialog.folderNamePlaceholder"),
            folder.name.clone(),
            false,
            None,
            InputPurpose::RenameFolder {
                folder_id: folder.id.clone(),
            },
        ));
    }

    /// Nothing when the active adapter cannot
    /// label accounts.
    pub fn open_rename_account_dialog(
        &mut self,
        adapter: Option<&PlatformAdapter>,
        account: &PlatformAccount,
    ) {
        let Some(adapter) = adapter.filter(|adapter| adapter.supports_account_labels()) else {
            return;
        };
        self.input_dialog = Some(InputDialogState::new(
            Text::key("platform.renameTitle"),
            Text::key("platform.renamePlaceholder"),
            account.display_name.clone(),
            true,
            None,
            InputPurpose::RenameAccount {
                platform_id: adapter.id().to_string(),
                account_id: account.id.clone(),
            },
        ));
    }

    /// Asks a name for a just-added account, prefilled with the identity the platform
    /// detected, when the platform can label accounts.
    pub fn prompt_rename_new_account(
        &mut self,
        registry: &PlatformRegistry,
        platform_id: &str,
        account_id: &str,
        display_name: Option<&str>,
    ) {
        let supported = registry
            .adapter(platform_id)
            .is_some_and(|adapter| adapter.supports_account_labels());
        if !supported {
            return;
        }
        self.input_dialog = Some(InputDialogState::new(
            Text::key("platform.renameNewAccount"),
            Text::key("platform.renamePlaceholder"),
            display_name.map(js::trim).unwrap_or_default().to_string(),
            true,
            None,
            InputPurpose::RenameNewAccount {
                platform_id: platform_id.to_string(),
                account_id: account_id.to_string(),
            },
        ));
    }

    /// Cancel or Escape.
    pub fn close_input_dialog(&mut self) {
        self.input_dialog = None;
    }

    /// The OK button or Enter: closes the dialog and applies its purpose with
    /// the submitted value. Nothing while it cannot submit.
    pub fn submit_input(&mut self, ctx: &mut DialogContext<'_>) -> Vec<AppEffect> {
        let Some(value) = self
            .input_dialog
            .as_ref()
            .and_then(InputDialogState::submitted_value)
        else {
            return Vec::new();
        };
        let Some(dialog) = self.input_dialog.take() else {
            return Vec::new();
        };
        let mut effects = Vec::new();
        match dialog.purpose {
            InputPurpose::NewFolder => {
                ctx.stores
                    .create_folder(&value, ctx.current_folder_id, ctx.active_tab);
                effects.push(AppEffect::RefreshCurrentItems);
            }
            InputPurpose::RenameFolder { folder_id } => {
                ctx.stores.rename_folder(&folder_id, &value);
                effects.push(AppEffect::RefreshCurrentItems);
            }
            InputPurpose::CardNote { account_id } => {
                if js::trim(&value).is_empty() {
                    ctx.stores.clear_account_card_note(&account_id);
                } else {
                    ctx.stores.set_account_card_note(&account_id, &value);
                }
                effects.push(AppEffect::CardNotesChanged);
            }
            InputPurpose::PlatformInput(request) => {
                effects.push(AppEffect::RunPlatformTask(request.confirm(&value)));
            }
            InputPurpose::RenameAccount {
                platform_id,
                account_id,
            } => effects.push(AppEffect::RenameAccount {
                platform_id,
                account_id,
                label: value,
            }),
            InputPurpose::RenameNewAccount {
                platform_id,
                account_id,
            } => {
                if !js::trim(&value).is_empty() {
                    effects.push(AppEffect::RenameAccount {
                        platform_id,
                        account_id,
                        label: value,
                    });
                }
            }
        }
        effects
    }
}
