//! Platforms described as data instead of code.
//!
//! A descriptor says where a launcher lives, how to read the signed-in account
//! id, which files, folders and registry values make up a session, how to close
//! the launcher and how to start it again. [`engine::DescriptorService`] does
//! the rest, so adding a platform means adding a JSON file, not a Rust module.
//!
//! Shipped descriptors are compiled in and read-only. Every one of them is
//! validated by the test at the bottom of this file, so a broken descriptor
//! fails the build rather than a user's switch.
//!
//! Descriptors the user writes live in [`user_dir`] and are read at run time,
//! so adding a platform takes a file and a reload, never a compiler. They
//! never override a shipped one: an id this build already answers for is
//! refused by name, so a dropped-in file cannot quietly take over Steam.
//!
//! Four platforms stay hand-written, because none of them is a session made of
//! files. Steam parses VDF, reads ban state and edits accounts in bulk.
//! Battle.net has no per-account snapshot at all: switching rewrites one
//! quote-aware CSV field inside a shared config. Roblox keeps its session in a
//! single registry cookie and needs an HTTP client for quick-login codes and
//! avatars. Riot fits the state model but not the account model: it exposes
//! profiles carrying a tag line and a puuid, which the engine's accounts do not
//! describe. Forcing any of them through a descriptor would mean a descriptor
//! that is mostly a native hook, which is the code path this module replaces.

pub mod config_bridge;
pub mod engine;
pub mod hooks;
pub mod paths;
pub mod plan;
pub mod reg;
pub mod schema;

use crate::context::AppContext;
use std::path::PathBuf;
use std::sync::OnceLock;

pub use engine::{DescriptorOrigin, DescriptorService};
pub use schema::{Descriptor, DescriptorError};

/// The descriptors shipped with the app, as `(source name, body)`.
const EMBEDDED: &[(&str, &str)] = &[
    ("gog.json", include_str!("descriptors/gog.json")),
    ("jagex.json", include_str!("descriptors/jagex.json")),
    ("epic.json", include_str!("descriptors/epic.json")),
    ("ubisoft.json", include_str!("descriptors/ubisoft.json")),
    ("discord.json", include_str!("descriptors/discord.json")),
];

/// Parses every shipped descriptor, keeping the failures rather than hiding
/// them: the caller decides whether a bad descriptor is fatal.
pub fn load_embedded() -> (Vec<Descriptor>, Vec<DescriptorError>) {
    let mut loaded = Vec::new();
    let mut errors = Vec::new();
    for (name, body) in EMBEDDED {
        match Descriptor::parse(&format!("embedded:{name}"), body) {
            Ok(descriptor) => loaded.push(descriptor),
            Err(error) => errors.push(error),
        }
    }
    (loaded, errors)
}

/// Every descriptor-driven service usable on this OS, built once.
///
/// A descriptor with no profile for the running OS is skipped: the platform is
/// simply absent here, which is not an error. Services are leaked because the
/// platform registry hands out `&'static dyn PlatformService` and they live as
/// long as the process anyway.
pub fn services() -> &'static [&'static DescriptorService] {
    static SERVICES: OnceLock<Vec<&'static DescriptorService>> = OnceLock::new();
    SERVICES.get_or_init(|| {
        let (descriptors, errors) = load_embedded();
        // A shipped descriptor that does not validate is a build mistake, and
        // `embedded_descriptors_all_validate` catches it in CI. In a debug
        // build it is loud here too; in release the platform is dropped rather
        // than taking the app down with it.
        debug_assert!(
            errors.is_empty(),
            "shipped descriptor rejected: {}",
            errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        );
        descriptors
            .into_iter()
            .filter(|descriptor| descriptor.current_profile().is_some())
            .map(|descriptor| {
                let service = DescriptorService::new(descriptor, DescriptorOrigin::Embedded);
                &*Box::leak(Box::new(service))
            })
            .collect()
    })
}

/// Where a user drops a descriptor of their own.
///
/// Next to the custom themes, under the config root rather than the local data
/// root: a descriptor is something the user wrote and would want to keep, not
/// a cache this machine happens to hold.
pub fn user_dir(app: &dyn AppContext) -> Result<PathBuf, String> {
    Ok(crate::storage::app_config_root(app)?.join("descriptors"))
}

/// Every `*.json` in [`user_dir`], parsed, each with the file it came from.
///
/// A missing folder is not an error: it means the user added none. A file that
/// does not validate is kept as an error rather than dropped, so whatever
/// lists them can name the file and the field instead of showing one platform
/// fewer than the user put there.
pub fn load_user(app: &dyn AppContext) -> (Vec<(PathBuf, Descriptor)>, Vec<DescriptorError>) {
    let mut loaded = Vec::new();
    let mut errors = Vec::new();

    let dir = match user_dir(app) {
        Ok(dir) => dir,
        Err(e) => {
            errors.push(DescriptorError::new("user descriptors", "", e));
            return (loaded, errors);
        }
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return (loaded, errors);
    };

    // Sorted, so two machines with the same folder report in the same order
    // and a collision always names the same file as the loser.
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        })
        .collect();
    files.sort();

    for path in files {
        let source = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        match std::fs::read_to_string(&path) {
            Ok(body) => match Descriptor::parse(&source, &body) {
                Ok(descriptor) => loaded.push((path, descriptor)),
                Err(error) => errors.push(error),
            },
            Err(e) => errors.push(DescriptorError::new(
                &source,
                "",
                format!("could not be read: {e}"),
            )),
        }
    }

    (loaded, errors)
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use std::path::Path;

    /// An app whose every root is one scratch directory.
    pub struct TempCtx {
        pub root: PathBuf,
    }

    impl AppContext for TempCtx {
        fn app_config_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
        fn app_data_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
        fn app_local_data_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
        fn app_cache_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
    }

    pub fn scratch(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "accshift-user-descriptors-{}-{}-{:?}",
            tag,
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    /// A descriptor for `id` that nothing on this machine can satisfy: it
    /// detects on a path under the scratch root that is never created, so
    /// registering it can never make a test think a launcher is installed.
    pub fn fixture(id: &str, live_root: &Path) -> String {
        let live = live_root.display().to_string().replace('\\', "/");
        let profile = format!(
            r#"{{
              "roots": {{ "files": ["{live}"] }},
              "detect": {{ "pathExists": ["{live}/never-here"] }},
              "identity": {{
                "source": {{ "kind": "synthetic" }},
                "format": {{ "charset": "alphanumeric", "maxLength": 64 }},
                "current": "config"
              }},
              "state": {{
                "files": [
                  {{ "live": "{live}/session.json", "snapshot": "session.json", "snapshotMarker": true }}
                ]
              }},
              "close": {{ "processes": ["nothing-here.exe"] }},
              "setup": {{ "missingSnapshotHint": "Add this account through setup first." }}
            }}"#
        );
        format!(
            r#"{{
              "id": "{id}",
              "schemaVersion": 1,
              "name": "Fixture Launcher",
              "shortName": "Fixture",
              "os": {{ "windows": {profile}, "linux": {profile}, "macos": {profile} }}
            }}"#
        )
    }

    /// Writes `body` into the user descriptor folder as `name`.
    pub fn drop_in(app: &dyn AppContext, name: &str, body: &str) {
        let dir = user_dir(app).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), body).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use crate::platforms::ids;

    #[test]
    fn a_folder_that_was_never_created_is_no_descriptors_and_no_errors() {
        let root = scratch("absent");
        let ctx = TempCtx { root: root.clone() };

        let (loaded, errors) = load_user(&ctx);

        assert!(loaded.is_empty());
        assert!(errors.is_empty(), "{errors:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_descriptor_the_user_wrote_is_read_with_the_file_it_came_from() {
        let root = scratch("read");
        let ctx = TempCtx { root: root.clone() };
        drop_in(&ctx, "acme.json", &fixture("acme", &root));

        let (loaded, errors) = load_user(&ctx);

        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].1.id, "acme");
        assert_eq!(loaded[0].0.file_name().unwrap(), "acme.json");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_that_is_not_a_descriptor_is_refused_by_name_and_never_dropped() {
        // Silently skipping it would show one platform fewer than the user put
        // in the folder, with nothing saying which file lost.
        let root = scratch("bad");
        let ctx = TempCtx { root: root.clone() };
        drop_in(&ctx, "broken.json", "{ not json at all");
        drop_in(&ctx, "good.json", &fixture("acme", &root));

        let (loaded, errors) = load_user(&ctx);

        assert_eq!(loaded.len(), 1, "the valid file still loads");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].source, "broken.json");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_invalid_field_is_reported_with_the_field_and_what_was_expected() {
        let root = scratch("field");
        let ctx = TempCtx { root: root.clone() };
        let body = fixture("acme", &root).replace("\"schemaVersion\": 1", "\"schemaVersion\": 99");
        drop_in(&ctx, "acme.json", &body);

        let (loaded, errors) = load_user(&ctx);

        assert!(loaded.is_empty());
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].field, "schemaVersion", "{}", errors[0]);
        assert!(errors[0].problem.contains('1'), "{}", errors[0]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn anything_that_is_not_a_json_file_is_left_alone() {
        // The folder is a place a human works in: notes and backups live there
        // too, and neither is a platform nor an error.
        let root = scratch("other-files");
        let ctx = TempCtx { root: root.clone() };
        drop_in(&ctx, "notes.txt", "remember to finish this");
        drop_in(&ctx, "acme.json.bak", "{ half a descriptor");

        let (loaded, errors) = load_user(&ctx);

        assert!(loaded.is_empty());
        assert!(errors.is_empty(), "{errors:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn embedded_descriptors_all_validate() {
        let (loaded, errors) = load_embedded();
        assert!(
            errors.is_empty(),
            "{}",
            errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        );
        assert_eq!(loaded.len(), EMBEDDED.len());
    }

    #[test]
    fn embedded_ids_are_unique_and_known_to_the_app() {
        // An id the rest of the app does not know about would register a
        // service nothing can reach.
        let (loaded, _) = load_embedded();
        let mut seen = Vec::new();
        for descriptor in &loaded {
            assert!(
                ids::ALL.contains(&descriptor.id.as_str()),
                "{} is not in ids::ALL",
                descriptor.id
            );
            assert!(
                !seen.contains(&descriptor.id),
                "duplicate id {}",
                descriptor.id
            );
            seen.push(descriptor.id.clone());
        }
    }

    #[test]
    fn shipped_descriptors_keep_the_error_strings_the_cli_classifies_on() {
        // `accshift switch` maps exit codes by matching these substrings, so
        // renaming a platform in its descriptor would silently change them.
        let (loaded, _) = load_embedded();
        for (id, short_name, name) in [
            (ids::GOG, "GOG", "GOG Galaxy"),
            (ids::JAGEX, "Jagex", "Jagex Launcher"),
            (ids::EPIC, "Epic", "Epic Games Launcher"),
            (ids::UBISOFT, "Ubisoft", "Ubisoft Connect"),
            (ids::DISCORD, "Discord", "Discord"),
        ] {
            let descriptor = loaded.iter().find(|d| d.id == id).unwrap();
            assert_eq!(descriptor.short_name, short_name);
            assert_eq!(descriptor.name, name);
        }
    }

    #[test]
    fn ubisoft_keeps_its_own_wording_for_a_bad_account_id() {
        // The generic message would read "Invalid Ubisoft account ID", and the
        // CLI classifies the exit code on the string the module used to emit.
        let (loaded, _) = load_embedded();
        let ubisoft = loaded.iter().find(|d| d.id == ids::UBISOFT).unwrap();
        let profile = ubisoft.os.values().next().unwrap();
        assert_eq!(
            profile.identity.format.invalid_message,
            "Invalid Ubisoft account UUID"
        );
    }

    #[cfg(windows)]
    #[test]
    fn every_shipped_platform_has_a_service_on_windows() {
        let ids: Vec<&str> = services().iter().map(|s| s.id()).collect();
        for expected in ["gog", "jagex", "epic", "ubisoft", "discord"] {
            assert!(ids.contains(&expected), "{expected} has no service");
        }
    }
}
