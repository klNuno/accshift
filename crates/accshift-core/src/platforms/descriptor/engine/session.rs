//! Offline session reads on a descriptor platform's snapshots: the
//! `session.notRemembered` INI checks ("password needed").

#[allow(unused_imports)]
use super::*;
use crate::platforms::descriptor::schema::IniCheck;

impl DescriptorService {
    /// The stored accounts whose snapshot says the launcher does not keep
    /// the sign-in. No rules, no snapshot or an unreadable file marks
    /// nothing.
    pub fn not_remembered_ids(&self, app: &dyn AppContext) -> Vec<String> {
        let Some(rules) = self.profile().ok().and_then(|p| p.session.as_ref()) else {
            return Vec::new();
        };
        if rules.not_remembered.is_empty() {
            return Vec::new();
        }
        let Ok(root) = crate::storage::platform_snapshots_dir(app, &self.descriptor.id) else {
            return Vec::new();
        };
        config_bridge::accounts(app, &self.descriptor.id)
            .into_iter()
            .map(|record| record.account_id)
            .filter(|id| safe_dir_name(id))
            .filter(|id| not_remembered_in(&root.join(id), &rules.not_remembered))
            .collect()
    }
}

/// An account id usable as one folder name under the snapshots root.
fn safe_dir_name(id: &str) -> bool {
    let id = id.trim();
    !id.is_empty() && id != "." && id != ".." && !id.contains(['/', '\\', ':'])
}

/// Whether any check matches the snapshot in `dir`.
pub(crate) fn not_remembered_in(dir: &Path, checks: &[IniCheck]) -> bool {
    checks.iter().any(|check| {
        let Ok(bytes) = crate::snapshot_crypto::read_decrypted_bytes(&dir.join(&check.snapshot))
        else {
            return false;
        };
        let Ok(text) = String::from_utf8(bytes) else {
            return false;
        };
        ini_value(&text, &check.section, &check.key)
            .is_some_and(|value| value.eq_ignore_ascii_case(check.equals.trim()))
    })
}

/// The trimmed value of `key` in `[section]`, `None` when either is absent.
/// Section and key names compare case-insensitively, the first match wins.
pub(crate) fn ini_value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut in_section = false;
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_section = name.trim().eq_ignore_ascii_case(section);
            continue;
        }
        if !in_section || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some((name, value)) = line.split_once('=') {
            if name.trim().eq_ignore_ascii_case(key) {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
