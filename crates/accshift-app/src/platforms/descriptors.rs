//! The user's own platform descriptors, ported from
//! `src/lib/platforms/descriptors.ts` and the descriptor half of
//! `registry.ts` (`PlatformDescriptor`, `renderTemplate`, `placeholderFrom`).
//!
//! Every call that changes the descriptor folder hands its fresh report to
//! [`super::PlatformRegistry::register_user_platforms`] (`adopt`), so the tab
//! list and the settings list never disagree with the folder.

use super::{PathPlaceholder, PlatformRegistry, RuntimeOs};
use crate::backend::Backend;
use crate::error::BackendResult;
use crate::types::{DescriptorPreview, DryRunPlan, UserPlatformReport};
use serde_json::Value;

/// `PlatformDescriptor`: the part of a descriptor the UI reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformDescriptor {
    pub id: String,
    pub name: String,
    /// Each system the descriptor covers, in declaration order, with the
    /// template of its first `path` candidate.
    pub os: Vec<(String, Option<String>)>,
}

/// The order `Object.keys` gives for a descriptor that went through the
/// engine: `BTreeMap<Os, _>` serializes Windows, macOS, Linux.
fn os_rank(os: &str) -> usize {
    match os {
        "windows" => 0,
        "macos" => 1,
        "linux" => 2,
        _ => 3,
    }
}

impl PlatformDescriptor {
    /// Reads a descriptor from JSON (a shipped file or a report entry).
    /// `serde_json` maps are sorted, so the systems are put back in the
    /// engine's order.
    pub fn from_value(raw: &Value) -> Option<Self> {
        let id = raw.get("id")?.as_str()?.to_string();
        let name = raw
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&id)
            .to_string();
        let mut os: Vec<(String, Option<String>)> = raw
            .get("os")
            .and_then(Value::as_object)
            .map(|profiles| {
                profiles
                    .iter()
                    .map(|(os, profile)| (os.clone(), first_path_template(profile)))
                    .collect()
            })
            .unwrap_or_default();
        os.sort_by_key(|(name, _)| os_rank(name));
        Some(Self { id, name, os })
    }

    /// From the engine's own type.
    pub fn from_core(
        descriptor: &accshift_core::platforms::descriptor::Descriptor,
    ) -> Option<Self> {
        Self::from_value(&serde_json::to_value(descriptor).ok()?)
    }

    /// The descriptors compiled into the engine (`SHIPPED_DESCRIPTORS`).
    pub fn shipped() -> Vec<Self> {
        let (loaded, _errors) = accshift_core::platforms::descriptor::load_embedded();
        loaded.iter().filter_map(Self::from_core).collect()
    }

    /// The systems, as `supportedOs`.
    pub fn supported_os(&self) -> Vec<RuntimeOs> {
        self.os
            .iter()
            .map(|(os, _)| super::to_runtime_os(Some(os)))
            .collect()
    }

    /// `placeholderFrom`: where the launcher usually lives, per system, from
    /// the first place the descriptor says to look.
    pub fn path_placeholder(&self) -> Option<PathPlaceholder> {
        let entries: Vec<(RuntimeOs, String)> = self
            .os
            .iter()
            .filter_map(|(os, template)| {
                let os = super::to_runtime_os(Some(os));
                template
                    .as_deref()
                    .map(|template| (os, render_template(template, os)))
            })
            .collect();
        (!entries.is_empty()).then_some(PathPlaceholder::PerOs(entries))
    }
}

/// `profile.executable?.candidates?.find(kind === "path" && template)`.
fn first_path_template(profile: &Value) -> Option<String> {
    profile
        .get("executable")?
        .get("candidates")?
        .as_array()?
        .iter()
        .find_map(|candidate| {
            if candidate.get("kind").and_then(Value::as_str) != Some("path") {
                return None;
            }
            candidate
                .get("template")
                .and_then(Value::as_str)
                .filter(|template| !template.is_empty())
                .map(str::to_string)
        })
}

/// One `${NAME}` or `${NAME(x)}` at the start of `rest`: its inner text and
/// byte length.
fn variable_at(rest: &str) -> Option<(&str, usize)> {
    let inner = rest.strip_prefix("${")?;
    let word = inner
        .char_indices()
        .find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '_'))
        .map_or(inner.len(), |(index, _)| index);
    if word == 0 {
        return None;
    }
    let mut end = word;
    let after = &inner[word..];
    if let Some(args) = after.strip_prefix('(') {
        let close = args.find(')')?;
        end = word + 1 + close + 1;
    }
    inner[end..]
        .starts_with('}')
        .then(|| (&inner[..end], 2 + end + 1))
}

/// `renderTemplate`: `${LOCALAPPDATA}` is `%LOCALAPPDATA%` on Windows and
/// `$LOCALAPPDATA` elsewhere, with the system's own separator.
pub fn render_template(template: &str, os: RuntimeOs) -> String {
    let mut expanded = String::with_capacity(template.len());
    let mut index = 0;
    while index < template.len() {
        let rest = &template[index..];
        if let Some((name, length)) = variable_at(rest) {
            if os == RuntimeOs::Windows {
                expanded.push('%');
                expanded.push_str(name);
                expanded.push('%');
            } else {
                expanded.push('$');
                expanded.push_str(name);
            }
            index += length;
            continue;
        }
        let c = rest.chars().next().unwrap_or_default();
        expanded.push(c);
        index += c.len_utf8();
    }
    if os == RuntimeOs::Windows {
        expanded.replace('/', "\\")
    } else {
        expanded.replace('\\', "/")
    }
}

/// `adopt`: the report's descriptors become the user platforms.
fn adopt(registry: &mut PlatformRegistry, report: UserPlatformReport) -> UserPlatformReport {
    let descriptors: Vec<PlatformDescriptor> = report
        .loaded
        .iter()
        .filter_map(PlatformDescriptor::from_core)
        .collect();
    registry.register_user_platforms(&descriptors);
    report
}

/// `reloadUserPlatforms`.
pub fn reload_user_platforms(
    registry: &mut PlatformRegistry,
    backend: &dyn Backend,
) -> BackendResult<UserPlatformReport> {
    Ok(adopt(registry, backend.reload_user_platforms()?))
}

/// `selectDescriptorFile`: `None` when the user cancelled or the dialog
/// failed, both of which mean "leave everything alone".
pub fn select_descriptor_file(backend: &dyn Backend) -> Option<String> {
    backend.descriptor_select_file().ok()
}

/// `previewDescriptorFile`.
pub fn preview_descriptor_file(
    backend: &dyn Backend,
    path: &str,
) -> BackendResult<DescriptorPreview> {
    backend.descriptor_preview_file(path)
}

/// `installDescriptorFile`.
pub fn install_descriptor_file(
    registry: &mut PlatformRegistry,
    backend: &dyn Backend,
    path: &str,
) -> BackendResult<UserPlatformReport> {
    Ok(adopt(registry, backend.descriptor_install_file(path)?))
}

/// `removeDescriptor`.
pub fn remove_descriptor(
    registry: &mut PlatformRegistry,
    backend: &dyn Backend,
    platform_id: &str,
) -> BackendResult<UserPlatformReport> {
    Ok(adopt(registry, backend.descriptor_remove(platform_id)?))
}

/// `openDescriptorsFolder`.
pub fn open_descriptors_folder(backend: &dyn Backend) -> BackendResult<()> {
    backend.open_descriptors_folder()
}

/// `dryRunSwitch`.
pub fn dry_run_switch(
    backend: &dyn Backend,
    platform_id: &str,
    account_id: &str,
) -> BackendResult<DryRunPlan> {
    backend.platform_dry_run(platform_id, account_id)
}
