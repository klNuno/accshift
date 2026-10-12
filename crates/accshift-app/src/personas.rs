//! The persona store, ported from `src/lib/features/personas/{types,store,
//! assignments}.ts`.
//!
//! A persona is a named identity bundle: activating it switches every
//! assigned platform to the persona's account in one action.

use crate::js;
use crate::stores::StoreContext;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const NAME_MAX: usize = 40;
/// Covers are downscaled before storage; anything longer is not a cover.
pub const IMAGE_MAX_CHARS: usize = 400_000;
const DEFAULT_NAME: &str = "Persona";

/// One platform slot of a persona: which account that platform switches to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonaAssignment {
    pub platform_id: String,
    pub account_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Persona {
    pub id: String,
    pub name: String,
    /// A card tint, `""` for none.
    pub color: String,
    /// A small `data:image/...` cover, or none (the card shows a mosaic).
    pub image: Option<String>,
    pub assignments: Vec<PersonaAssignment>,
}

/// A persona without its id, for [`crate::stores::ClientStores::create_persona`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PersonaInput {
    pub name: String,
    pub color: String,
    pub image: Option<String>,
    pub assignments: Vec<PersonaAssignment>,
}

/// The fields `updatePersona` replaces; `None` keeps the current value.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PersonaPatch {
    pub name: Option<String>,
    pub color: Option<String>,
    pub image: Option<Option<String>>,
    pub assignments: Option<Vec<PersonaAssignment>>,
}

impl Persona {
    pub fn apply(&mut self, patch: &PersonaPatch) {
        if let Some(name) = &patch.name {
            self.name = name.clone();
        }
        if let Some(color) = &patch.color {
            self.color = color.clone();
        }
        if let Some(image) = &patch.image {
            self.image = image.clone();
        }
        if let Some(assignments) = &patch.assignments {
            self.assignments = assignments.clone();
        }
    }
}

fn sanitize_assignments(value: Option<&Value>, ctx: &StoreContext) -> Vec<PersonaAssignment> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for raw in items {
        // `typeof raw === "object"` lets arrays through; they hold no fields.
        let Some(record) = js::as_record(Some(raw)) else {
            continue;
        };
        let (Some(platform_id), Some(account_id)) = (
            js::as_str(record.get("platformId")),
            js::as_str(record.get("accountId")),
        ) else {
            continue;
        };
        if !ctx.knows_platform(platform_id) || seen.contains(platform_id) {
            continue;
        }
        if account_id.is_empty() {
            continue;
        }
        seen.insert(platform_id.to_string());
        result.push(PersonaAssignment {
            platform_id: platform_id.to_string(),
            account_id: account_id.to_string(),
        });
    }
    result
}

fn is_persona_color(value: &str) -> bool {
    js::is_hex_color(value, &[6])
}

fn sanitize_persona(value: &Value, ctx: &StoreContext) -> Option<Persona> {
    let raw = js::as_record(Some(value))?;
    let id = js::as_str(raw.get("id")).filter(|id| !id.is_empty())?;
    let name = js::as_str(raw.get("name"))
        .map(js::trim)
        .filter(|name| !name.is_empty())
        .map_or_else(
            || DEFAULT_NAME.to_string(),
            |name| js::utf16_prefix(name, NAME_MAX),
        );
    let color = js::as_str(raw.get("color"))
        .filter(|color| is_persona_color(color))
        .unwrap_or_default()
        .to_string();
    let image = js::as_str(raw.get("image"))
        .filter(|image| image.starts_with("data:image/") && js::utf16_len(image) <= IMAGE_MAX_CHARS)
        .map(str::to_string);
    Some(Persona {
        id: id.to_string(),
        name,
        color,
        image,
        assignments: sanitize_assignments(raw.get("assignments"), ctx),
    })
}

/// `sanitizeList`: valid personas, first of each id.
pub fn sanitize_personas(value: &Value, ctx: &StoreContext) -> Vec<Persona> {
    let Value::Array(items) = value else {
        return Vec::new();
    };
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for raw in items {
        let Some(persona) = sanitize_persona(raw, ctx) else {
            continue;
        };
        if !seen.insert(persona.id.clone()) {
            continue;
        }
        result.push(persona);
    }
    result
}

/// The list as the JSON the store holds.
pub fn personas_to_value(personas: &[Persona]) -> Value {
    serde_json::to_value(personas).unwrap_or(Value::Null)
}

/// `newId`: a random UUID v4, as `crypto.randomUUID` gives.
pub fn generate_persona_id() -> String {
    let high = js::random_u64();
    let low = js::random_u64();
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&high.to_be_bytes());
    bytes[8..].copy_from_slice(&low.to_be_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// The wizard state [`persona_assignments_for_save`] reads.
#[derive(Debug, Clone, Copy)]
pub struct AssignmentsForSave<'a> {
    pub original: &'a [PersonaAssignment],
    pub offered_platform_ids: &'a [String],
    /// Platform id to picked account id; `""` clears that platform.
    pub selection: &'a BTreeMap<String, String>,
    /// The account ids each platform listed.
    pub accounts_by_platform: &'a BTreeMap<String, Vec<String>>,
    pub loading: bool,
    pub failed_platform_ids: &'a [String],
}

/// `personaAssignmentsForSave`: the assignments to write when a persona is
/// saved. A platform the wizard is not offering, or whose account read
/// failed, keeps its stored assignment; one whose accounts loaded drops an id
/// no longer listed. Stored order first, new picks after in wizard order.
pub fn persona_assignments_for_save(input: &AssignmentsForSave<'_>) -> Vec<PersonaAssignment> {
    let offered: BTreeSet<&str> = input
        .offered_platform_ids
        .iter()
        .map(String::as_str)
        .collect();
    let failed: BTreeSet<&str> = input
        .failed_platform_ids
        .iter()
        .map(String::as_str)
        .collect();
    let listed = |platform_id: &str, id: &str| {
        input
            .accounts_by_platform
            .get(platform_id)
            .is_some_and(|accounts| accounts.iter().any(|account| account == id))
    };
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();

    for assignment in input.original {
        seen.insert(assignment.platform_id.as_str());
        let edited = input.selection.get(&assignment.platform_id);
        if edited.is_some_and(String::is_empty) {
            continue;
        }
        if !offered.contains(assignment.platform_id.as_str()) {
            result.push(assignment.clone());
            continue;
        }
        let selected = edited.map_or("", String::as_str);
        let id = if selected.is_empty() {
            assignment.account_id.as_str()
        } else {
            selected
        };
        if id.is_empty() {
            continue;
        }
        if !input.loading
            && !failed.contains(assignment.platform_id.as_str())
            && !listed(&assignment.platform_id, id)
        {
            continue;
        }
        result.push(PersonaAssignment {
            platform_id: assignment.platform_id.clone(),
            account_id: id.to_string(),
        });
    }

    for platform_id in input.offered_platform_ids {
        if seen.contains(platform_id.as_str()) {
            continue;
        }
        let Some(id) = input.selection.get(platform_id).filter(|id| !id.is_empty()) else {
            continue;
        };
        if !input.loading && !failed.contains(platform_id.as_str()) && !listed(platform_id, id) {
            continue;
        }
        result.push(PersonaAssignment {
            platform_id: platform_id.clone(),
            account_id: id.clone(),
        });
    }
    result
}
