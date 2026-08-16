//! The escape hatch: compiled steps a descriptor may delegate to.
//!
//! Two launchers keep the signed-in account somewhere no template can point
//! at: Discord buries it in raw leveldb bytes, Riot answers a local HTTPS API.
//! Everything else about them is still data, so rather than keep two more
//! hand-written platforms, a descriptor names one of the hooks below and the
//! engine calls it where its own steps cannot reach.
//!
//! A hook is deliberately narrow. It receives the paths its descriptor
//! declared for it, already resolved and checked against the sandbox, and
//! nothing else: no descriptor, no config, no way to reach the filesystem
//! outside the roots. Adding one means adding it to [`HOOKS`], which is what
//! keeps the allowlist in the schema honest.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(super) mod discord;

/// What a hook is allowed to know about this machine.
///
/// The names are the keys of the descriptor's `paths` object, so the hook and
/// the descriptor agree on what each path is for without the hook ever
/// building one itself.
#[derive(Debug, Clone, Default)]
pub struct HookContext {
    paths: BTreeMap<String, PathBuf>,
}

impl HookContext {
    pub fn new(paths: BTreeMap<String, PathBuf>) -> Self {
        Self { paths }
    }

    /// The path the descriptor declared under `name`, absent when it named
    /// none or when it did not resolve on this machine.
    pub fn path(&self, name: &str) -> Option<&Path> {
        self.paths.get(name).map(PathBuf::as_path)
    }
}

/// The account a hook found signed in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookIdentity {
    pub id: String,
    /// Shown instead of the raw id when the platform exposes a name.
    pub display_name: Option<String>,
}

/// One compiled step, named by the descriptors entitled to it.
pub trait NativeHook: Sync {
    /// The name a descriptor writes to reach this hook.
    fn name(&self) -> &'static str;

    /// Paths the descriptor must declare for this hook. A descriptor missing
    /// one is refused at load, so the hook never has to guess.
    fn required_paths(&self) -> &'static [&'static str];

    /// The account signed in right now.
    ///
    /// `None` means "could not tell", never "signed out": callers must not
    /// treat it as an empty session.
    fn identity(&self, ctx: &HookContext) -> Option<HookIdentity>;
}

/// Every hook a descriptor may name. The schema validates against this list,
/// so a typo is a load error naming the field, not a step that silently does
/// nothing at run time.
static HOOKS: &[&(dyn NativeHook + 'static)] = &[&discord::LEVELDB];

pub fn hook(name: &str) -> Option<&'static dyn NativeHook> {
    HOOKS.iter().copied().find(|hook| hook.name() == name)
}

/// The known hook names, for the error message a bad descriptor gets.
pub fn names() -> Vec<&'static str> {
    HOOKS.iter().map(|hook| hook.name()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hooks_are_reachable_by_the_name_they_declare() {
        for name in names() {
            assert_eq!(hook(name).map(|h| h.name()), Some(name));
        }
    }

    #[test]
    fn an_unknown_hook_is_not_invented() {
        assert!(hook("no-such-hook").is_none());
    }

    #[test]
    fn hook_names_are_unique() {
        let mut seen = names();
        seen.sort_unstable();
        let count = seen.len();
        seen.dedup();
        assert_eq!(seen.len(), count, "two hooks share a name");
    }
}
