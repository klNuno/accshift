//! The shared `platform_*` calls, ported from `src/lib/platforms/platformApi.ts`
//! with the log redaction of `src/lib/shared/appLogger.ts` they rely on.
//!
//! A switch refused for want of the PIN comes back as an error whose
//! [`crate::BackendError::is_pin_locked`] is true; TS raises the lock screen
//! from here (`reportPinLockedError`), the egui shell does it from that flag.

use super::add_flow::to_add_flow_status;
use super::AddFlowStatus;
use crate::backend::Backend;
use crate::error::BackendResult;
use serde_json::{json, Map, Value};

/// `REDACTED_KEYS` of the app logger, lowercase.
const REDACTED_KEYS: [&str; 14] = [
    "username",
    "requestedusername",
    "autologinuser",
    "currentaccountfromloginusers",
    "email",
    "targetemail",
    "currentaccount",
    "steamid",
    "accountid",
    "profileid",
    "targetprofileid",
    "currentprofileid",
    "uuid",
    "userid",
];

fn mask_value(value: &Value) -> Value {
    match value {
        Value::String(text) => {
            let units: Vec<u16> = text.encode_utf16().collect();
            if units.len() > 2 {
                Value::String(format!("{}***", String::from_utf16_lossy(&units[..2])))
            } else {
                Value::String("***".to_string())
            }
        }
        Value::Array(items) => Value::String(format!("[{} items]", items.len())),
        _ => Value::String("***".to_string()),
    }
}

fn redact_object(object: &Map<String, Value>) -> Map<String, Value> {
    object
        .iter()
        .map(|(key, value)| {
            let redacted = if REDACTED_KEYS.contains(&key.to_lowercase().as_str()) {
                mask_value(value)
            } else if let Value::Object(inner) = value {
                Value::Object(redact_object(inner))
            } else {
                value.clone()
            };
            (key.clone(), redacted)
        })
        .collect()
}

/// `serializeLogValue`: strings as is, objects redacted then JSON.
pub fn serialize_log_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Object(object) => Value::Object(redact_object(object)).to_string(),
        other => other.to_string(),
    }
}

/// `logAppEvent`: failures are ignored so they never mask the action result.
pub fn log_app_event(
    backend: &dyn Backend,
    level: &str,
    source: &str,
    message: &str,
    details: Option<&Value>,
) {
    let details = details
        .filter(|value| !value.is_null())
        .map(serialize_log_value);
    let _ = backend.log_app_event(level, source, message, details.as_deref());
}

/// `createPlatformApi(platformId)`.
#[derive(Clone, Copy)]
pub struct PlatformApi<'a> {
    backend: &'a dyn Backend,
    platform_id: &'a str,
}

/// `frontend.<id>` with the first `-` made `_`, like `String.replace`.
pub fn log_prefix(platform_id: &str) -> String {
    format!("frontend.{}", platform_id.replacen('-', "_", 1))
}

impl<'a> PlatformApi<'a> {
    pub fn new(backend: &'a dyn Backend, platform_id: &'a str) -> Self {
        Self {
            backend,
            platform_id,
        }
    }

    pub fn backend(&self) -> &'a dyn Backend {
        self.backend
    }

    /// `getAccounts`: the platform's raw account list.
    pub fn get_accounts(&self) -> BackendResult<Value> {
        self.backend.platform_get_accounts(self.platform_id)
    }

    pub fn get_current_account(&self) -> BackendResult<String> {
        self.backend.platform_get_current_account(self.platform_id)
    }

    pub fn get_startup_snapshot(&self) -> BackendResult<Value> {
        self.backend.platform_get_startup_snapshot(self.platform_id)
    }

    /// `switchAccount`, logged before and after. `log_details` defaults to
    /// `{ accountId }`, redacted by the logger.
    pub fn switch_account(
        &self,
        account_id: &str,
        params: &Value,
        log_details: Option<Value>,
    ) -> BackendResult<()> {
        let source = format!("{}.switch", log_prefix(self.platform_id));
        let details = log_details.unwrap_or_else(|| json!({ "accountId": account_id }));
        log_app_event(
            self.backend,
            "info",
            &source,
            "Switch request started",
            Some(&details),
        );
        match self
            .backend
            .platform_switch_account(self.platform_id, account_id, params)
        {
            Ok(()) => {
                log_app_event(
                    self.backend,
                    "info",
                    &source,
                    "Switch request completed",
                    Some(&details),
                );
                Ok(())
            }
            Err(error) => {
                let mut failed = details.as_object().cloned().unwrap_or_default();
                failed.insert("error".to_string(), Value::String(error.message.clone()));
                log_app_event(
                    self.backend,
                    "error",
                    &source,
                    "Switch request failed",
                    Some(&Value::Object(failed)),
                );
                Err(error)
            }
        }
    }

    pub fn begin_setup(&self, params: &Value) -> BackendResult<AddFlowStatus> {
        let payload = self
            .backend
            .platform_begin_setup(self.platform_id, params)?;
        Ok(to_add_flow_status(&payload.setup_id, &payload))
    }

    pub fn get_setup_status(&self, setup_id: &str) -> BackendResult<AddFlowStatus> {
        let payload = self
            .backend
            .platform_get_setup_status(self.platform_id, setup_id)?;
        Ok(to_add_flow_status(&payload.setup_id, &payload))
    }

    pub fn cancel_setup(&self, setup_id: &str) -> BackendResult<()> {
        self.backend
            .platform_cancel_setup(self.platform_id, setup_id)
    }

    pub fn forget_account(&self, account_id: &str) -> BackendResult<()> {
        self.backend
            .platform_forget_account(self.platform_id, account_id)
    }

    pub fn set_account_label(&self, account_id: &str, label: &str) -> BackendResult<()> {
        self.backend
            .platform_set_account_label(self.platform_id, account_id, label)
    }
}
