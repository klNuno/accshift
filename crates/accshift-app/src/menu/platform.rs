//! The copy and forget entries every platform shares, ported from
//! `src/lib/shared/contextMenu/platformMenuBuilder.ts`.

use super::{ConfirmRequest, MenuAction, MenuCommand, PlatformTask};
use crate::i18n::TranslationValue;
use crate::platforms::PlatformAccount;
use crate::text::Text;

/// `CopyItemConfig`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyItemConfig {
    pub field: String,
    pub value: String,
    pub label_key: String,
    pub clipboard_label_key: String,
}

impl CopyItemConfig {
    /// The usual case: the same key labels the entry and the clipboard toast.
    pub fn new(field: &str, value: impl Into<String>, label_key: &str) -> Self {
        Self {
            field: field.to_string(),
            value: value.into(),
            label_key: label_key.to_string(),
            clipboard_label_key: label_key.to_string(),
        }
    }
}

/// `ForgetConfig`. The TS `action` closure is the forget task itself.
#[derive(Debug, Clone, PartialEq)]
pub struct ForgetConfig {
    pub title_key: String,
    pub title_params: Vec<(String, TranslationValue)>,
    pub message_key: String,
    pub confirm_label_key: String,
    pub toast_key: String,
    pub toast_params: Vec<(String, TranslationValue)>,
}

impl ForgetConfig {
    /// The four keys of a platform's forget flow, `<prefix>.<suffix>`.
    pub fn keys(title: &str, message: &str, confirm: &str, toast: &str) -> Self {
        Self {
            title_key: title.to_string(),
            title_params: Vec::new(),
            message_key: message.to_string(),
            confirm_label_key: confirm.to_string(),
            toast_key: toast.to_string(),
            toast_params: Vec::new(),
        }
    }

    pub fn toast_param(mut self, name: &str, value: impl Into<TranslationValue>) -> Self {
        self.toast_params.push((name.to_string(), value.into()));
        self
    }
}

/// `(account.displayName || account.username || account.id).trim() || account.id`.
pub fn default_display_value(account: &PlatformAccount) -> String {
    let base = [&account.display_name, &account.username, &account.id]
        .into_iter()
        .find(|value| !value.is_empty())
        .map_or("", |value| value.as_str());
    let trimmed = crate::js::trim(base);
    if trimmed.is_empty() {
        account.id.clone()
    } else {
        trimmed.to_string()
    }
}

/// `{ display, ...extra }`: a later key replaces an earlier one, in place.
fn merged_params(
    display: &str,
    extra: &[(String, TranslationValue)],
) -> Vec<(String, TranslationValue)> {
    let mut params = vec![("display".to_string(), TranslationValue::from(display))];
    for (name, value) in extra {
        match params.iter_mut().find(|(existing, _)| existing == name) {
            Some(slot) => slot.1 = value.clone(),
            None => params.push((name.clone(), value.clone())),
        }
    }
    params
}

/// `buildPlatformContextMenu`: one copy entry per item (`platform.copy`),
/// then the forget entry (`platform.danger`) behind a confirmation.
pub fn build_platform_context_menu(
    platform_id: &str,
    account: &PlatformAccount,
    copy_items: &[CopyItemConfig],
    forget: &ForgetConfig,
    display_value: Option<String>,
) -> Vec<MenuAction> {
    let display = display_value.unwrap_or_else(|| default_display_value(account));
    let mut items: Vec<MenuAction> = copy_items
        .iter()
        .map(|item| {
            MenuAction::item(
                format!("{platform_id}.copy.{}.{}", item.field, account.id),
                Some("platform.copy"),
                Text::key(&item.label_key),
                MenuCommand::Copy {
                    value: item.value.clone(),
                    label: Text::key(&item.clipboard_label_key),
                },
            )
        })
        .collect();
    let confirm = ConfirmRequest {
        title: Text::with_optional(
            &forget.title_key,
            Some(merged_params(&display, &forget.title_params)),
        ),
        message: Text::key(&forget.message_key),
        confirm_label: Some(Text::key(&forget.confirm_label_key)),
        on_confirm: PlatformTask::Forget {
            platform_id: platform_id.to_string(),
            account_id: account.id.clone(),
            toast: Text::with_optional(
                &forget.toast_key,
                Some(merged_params(&display, &forget.toast_params)),
            ),
        },
    };
    items.push(MenuAction::item(
        format!("{platform_id}.forget.{}", account.id),
        Some("platform.danger"),
        Text::key(&forget.confirm_label_key),
        MenuCommand::Confirm(Box::new(confirm)),
    ));
    items
}
