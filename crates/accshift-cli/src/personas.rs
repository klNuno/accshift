//! Client-side persona store, read-only.
//!
//! Personas live in the GUI's `client.personas` store. The CLI lists them
//! and switches to one; it never edits them. The reading rules are the app's
//! `accshift_app::personas::sanitize_personas` (first of each id, valid
//! assignments only, first of each platform, an empty name read as
//! "Persona"), held to it by the parity test in `tests.rs`.

use accshift_core::storage::{client_store_path, read_json_if_exists, STORE_PERSONAS};
use accshift_core::AppContext;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;

const DEFAULT_NAME: &str = "Persona";
/// The app's `NAME_MAX`, in UTF-16 units like the JavaScript it ports.
const NAME_MAX: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Assignment {
    pub platform_id: String,
    pub account_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Persona {
    pub id: String,
    pub name: String,
    pub assignments: Vec<Assignment>,
}

pub fn load(ctx: &dyn AppContext) -> Result<Vec<Persona>, String> {
    let path = client_store_path(ctx, STORE_PERSONAS)?;
    Ok(read_json_if_exists::<Value>(&path)?
        .map(|raw| from_value(&raw))
        .unwrap_or_default())
}

fn utf16_prefix(text: &str, max: usize) -> String {
    let mut units = 0;
    let mut out = String::new();
    for c in text.chars() {
        units += c.len_utf16();
        if units > max {
            break;
        }
        out.push(c);
    }
    out
}

fn assignments(raw: Option<&Value>) -> Vec<Assignment> {
    let Some(items) = raw.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for item in items {
        let Some(record) = item.as_object() else {
            continue;
        };
        let text = |key: &str| record.get(key).and_then(Value::as_str);
        let (Some(platform_id), Some(account_id)) = (text("platformId"), text("accountId")) else {
            continue;
        };
        if platform_id.is_empty() || account_id.is_empty() || !seen.insert(platform_id) {
            continue;
        }
        out.push(Assignment {
            platform_id: platform_id.to_string(),
            account_id: account_id.to_string(),
        });
    }
    out
}

/// The stored personas: the valid ones, first of each id.
pub fn from_value(raw: &Value) -> Vec<Persona> {
    let Some(items) = raw.as_array() else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for item in items {
        let Some(record) = item.as_object() else {
            continue;
        };
        let Some(id) = record
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        else {
            continue;
        };
        if !seen.insert(id.to_string()) {
            continue;
        }
        let name = record
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map_or_else(|| DEFAULT_NAME.to_string(), |n| utf16_prefix(n, NAME_MAX));
        out.push(Persona {
            id: id.to_string(),
            name,
            assignments: assignments(record.get("assignments")),
        });
    }
    out
}

/// Why a persona argument named no single persona.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersonaLookupError {
    NoneConfigured,
    NotFound(Vec<String>),
    /// `(id, name)` of each persona sharing the name.
    Ambiguous(Vec<(String, String)>),
}

/// An exact id first, then a name without case sensitivity.
pub fn resolve<'a>(
    personas: &'a [Persona],
    query: &str,
) -> Result<&'a Persona, PersonaLookupError> {
    if personas.is_empty() {
        return Err(PersonaLookupError::NoneConfigured);
    }
    if let Some(by_id) = personas.iter().find(|p| p.id == query) {
        return Ok(by_id);
    }
    let wanted = query.trim().to_lowercase();
    let by_name: Vec<&Persona> = personas
        .iter()
        .filter(|p| p.name.to_lowercase() == wanted)
        .collect();
    match by_name.as_slice() {
        [one] => Ok(one),
        [] => Err(PersonaLookupError::NotFound(
            personas.iter().map(|p| p.name.clone()).collect(),
        )),
        many => Err(PersonaLookupError::Ambiguous(
            many.iter()
                .map(|p| (p.id.clone(), p.name.clone()))
                .collect(),
        )),
    }
}
