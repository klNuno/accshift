//! Platforms driven entirely by the shared `platform_*` commands:
//! `battle-net`, `ubisoft`, `epic`, `gog`, `jagex`, `discord`, and any
//! platform from a user descriptor.
//!
//! What differs between them (account mapping, copy entries, the masked
//! switch log id, the forget toast parameters) is a closed set,
//! [`GenericFlavor`].

use super::PlatformAccount;
use crate::i18n::TranslationValue;
use crate::menu::platform::CopyItemConfig;
use crate::time::unix_ms_to_seconds;
use serde_json::Value;

/// The per-platform behaviour of a generic platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenericFlavor {
    /// `accountId`, `label`, `lastUsedAt` payloads; one account-id copy
    /// entry.
    Default,
    /// Email id, battle tag label, masked switch log, username and email
    /// copy entries, `{ email }` forget toast, no labels.
    BattleNet,
    /// Uuid id, shortened uuid label, uuid copy entry.
    Ubisoft,
}

/// How a generic platform maps accounts and words its toasts.
#[derive(Debug, Clone, PartialEq)]
pub struct GenericConfig {
    /// i18n key prefix (Battle.net keys use `battlenet`).
    pub i18n_prefix: String,
    pub reload_after_add: bool,
    pub no_accounts_toast_key: String,
    pub no_accounts_hint_key: Option<String>,
    /// Params for the no-accounts toast and hint (a user platform's name).
    pub message_params: Option<Vec<(String, TranslationValue)>>,
    pub flavor: GenericFlavor,
}

impl GenericConfig {
    fn simple(id: &str, toast: &str, hint: Option<&str>, reload_after_add: bool) -> Self {
        Self {
            i18n_prefix: id.to_string(),
            reload_after_add,
            no_accounts_toast_key: toast.to_string(),
            no_accounts_hint_key: hint.map(str::to_string),
            message_params: None,
            flavor: GenericFlavor::Default,
        }
    }

    /// The config of a shipped generic platform, `None` for any other id.
    pub fn shipped(id: &str) -> Option<Self> {
        Some(match id {
            "battle-net" => Self {
                i18n_prefix: "battlenet".to_string(),
                flavor: GenericFlavor::BattleNet,
                ..Self::simple(id, "toast.noBattleNetAccountsFound", None, false)
            },
            "ubisoft" => Self {
                flavor: GenericFlavor::Ubisoft,
                ..Self::simple(id, "toast.noUbisoftAccountsFound", None, false)
            },
            "epic" => Self::simple(
                id,
                "toast.noEpicAccountsFound",
                Some("epic.noAccountsHint"),
                true,
            ),
            "gog" => Self::simple(
                id,
                "toast.noGogAccountsFound",
                Some("gog.noAccountsHint"),
                true,
            ),
            "jagex" => Self::simple(
                id,
                "toast.noJagexAccountsFound",
                Some("jagex.noAccountsHint"),
                true,
            ),
            "discord" => Self::simple(
                id,
                "toast.noDiscordAccountsFound",
                Some("discord.noAccountsHint"),
                true,
            ),
            _ => return None,
        })
    }

    /// A user platform: the shared `platform.*` wording with the descriptor's
    /// name filled in.
    pub fn user(name: &str) -> Self {
        Self {
            i18n_prefix: "platform".to_string(),
            reload_after_add: true,
            no_accounts_toast_key: "app.noAccountsFound".to_string(),
            no_accounts_hint_key: Some("app.noAccountsHint".to_string()),
            message_params: Some(vec![("platform".to_string(), name.into())]),
            flavor: GenericFlavor::Default,
        }
    }

    /// `` `${prefix}.${suffix}` ``.
    pub fn key(&self, suffix: &str) -> String {
        format!("{}.{suffix}", self.i18n_prefix)
    }

    /// Whether accounts can be renamed: all but Battle.net.
    pub fn supports_account_labels(&self) -> bool {
        self.flavor != GenericFlavor::BattleNet
    }

    /// The account mapping of the flavor.
    pub fn to_account(&self, raw: &Value) -> PlatformAccount {
        match self.flavor {
            GenericFlavor::Default => default_to_account(raw),
            GenericFlavor::BattleNet => to_battle_net_account(raw),
            GenericFlavor::Ubisoft => to_ubisoft_account(raw),
        }
    }

    /// The copy menu entries of an account, or the default account-id entry.
    pub fn copy_items(&self, account: &PlatformAccount) -> Vec<CopyItemConfig> {
        match self.flavor {
            GenericFlavor::Default => vec![CopyItemConfig::new(
                "accountId",
                account.id.clone(),
                &self.key("copyLabelAccountId"),
            )],
            GenericFlavor::BattleNet => {
                let username =
                    first_non_empty(&[&account.display_name, &account.username, &account.id]);
                let trimmed = crate::js::trim(username);
                let username = if trimmed.is_empty() {
                    account.id.as_str()
                } else {
                    trimmed
                };
                vec![
                    CopyItemConfig::new("username", username, "battlenet.copyLabelUsername"),
                    CopyItemConfig::new("email", account.id.clone(), "battlenet.copyLabelEmail"),
                ]
            }
            GenericFlavor::Ubisoft => vec![CopyItemConfig::new(
                "uuid",
                account.id.clone(),
                "ubisoft.copyLabelUuid",
            )],
        }
    }

    /// The parameters of the toast after forgetting an account.
    pub fn forget_toast_params(&self, display: &str) -> Vec<(String, TranslationValue)> {
        match self.flavor {
            GenericFlavor::BattleNet => vec![("email".to_string(), display.into())],
            _ => Vec::new(),
        }
    }

    /// The account id as the switch log shows it, `None` when the flavor logs
    /// the id as is.
    pub fn mask_switch_log_id(&self, account_id: &str) -> Option<String> {
        (self.flavor == GenericFlavor::BattleNet).then(|| mask_email(account_id))
    }
}

fn first_non_empty<'a>(values: &[&'a String]) -> &'a str {
    values
        .iter()
        .find(|value| !value.is_empty())
        .map_or("", |value| value.as_str())
}

fn str_field<'a>(raw: &'a Value, name: &str) -> &'a str {
    raw.get(name).and_then(Value::as_str).unwrap_or_default()
}

fn number_field(raw: &Value, name: &str) -> Option<f64> {
    raw.get(name).and_then(Value::as_f64)
}

/// An `accountId`, `label`, `lastUsedAt` payload as a platform account.
pub fn default_to_account(raw: &Value) -> PlatformAccount {
    let id = str_field(raw, "accountId");
    let label = str_field(raw, "label");
    PlatformAccount {
        id: id.to_string(),
        display_name: if label.is_empty() { id } else { label }.to_string(),
        username: id.to_string(),
        last_login_at_sec: unix_ms_to_seconds(number_field(raw, "lastUsedAt")),
    }
}

/// The email's local part.
fn battle_net_display_name(email: &str) -> String {
    let trimmed = crate::js::trim(email);
    let candidate = crate::js::trim(trimmed.split('@').next().unwrap_or_default());
    if candidate.is_empty() {
        trimmed
    } else {
        candidate
    }
    .to_string()
}

/// The battle tag name, else the email's local part.
pub fn to_battle_net_account(raw: &Value) -> PlatformAccount {
    let email = str_field(raw, "email");
    let battle_tag = crate::js::trim(str_field(raw, "battleTag"));
    let display_name = if battle_tag.is_empty() {
        battle_net_display_name(email)
    } else {
        let name = crate::js::trim(battle_tag.split('#').next().unwrap_or_default());
        if name.is_empty() { battle_tag } else { name }.to_string()
    };
    PlatformAccount {
        id: email.to_string(),
        display_name,
        username: String::new(),
        last_login_at_sec: unix_ms_to_seconds(number_field(raw, "lastLoginAt")),
    }
}

/// The first three UTF-16 units of the local part, then an ellipsis, so raw
/// emails stay out of log files.
pub fn mask_email(email: &str) -> String {
    let local: Vec<u16> = email
        .split('@')
        .next()
        .unwrap_or_default()
        .encode_utf16()
        .collect();
    let kept = String::from_utf16_lossy(&local[..local.len().min(3)]);
    format!("{kept}\u{2026}")
}

/// The label, else the uuid's first group.
pub fn to_ubisoft_account(raw: &Value) -> PlatformAccount {
    let uuid = str_field(raw, "uuid");
    let label = crate::js::trim(str_field(raw, "label"));
    let display_name = if label.is_empty() {
        uuid.split('-').next().unwrap_or(uuid).to_string()
    } else {
        label.to_string()
    };
    PlatformAccount {
        id: uuid.to_string(),
        display_name,
        username: String::new(),
        last_login_at_sec: unix_ms_to_seconds(number_field(raw, "lastUsedAt")),
    }
}
