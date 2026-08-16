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
//! Steam stays hand-written: VDF parsing, ban checks, CS2 and bulk edit are not
//! a file-copy problem and would need more escape hatches than data.

pub mod config_bridge;
pub mod engine;
pub mod paths;
pub mod plan;
pub mod reg;
pub mod schema;

use std::sync::OnceLock;

pub use engine::{DescriptorOrigin, DescriptorService};
pub use schema::{Descriptor, DescriptorError};

/// The descriptors shipped with the app, as `(source name, body)`.
const EMBEDDED: &[(&str, &str)] = &[
    ("gog.json", include_str!("descriptors/gog.json")),
    ("jagex.json", include_str!("descriptors/jagex.json")),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platforms::ids;

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
    fn gog_keeps_the_error_strings_the_cli_classifies_on() {
        // `accshift switch` maps exit codes by matching these substrings, so
        // renaming the platform in the descriptor would silently change them.
        let (loaded, _) = load_embedded();
        let gog = loaded.iter().find(|d| d.id == ids::GOG).unwrap();
        assert_eq!(gog.short_name, "GOG");
        assert_eq!(gog.name, "GOG Galaxy");

        let jagex = loaded.iter().find(|d| d.id == ids::JAGEX).unwrap();
        assert_eq!(jagex.short_name, "Jagex");
        assert_eq!(jagex.name, "Jagex Launcher");
    }

    #[cfg(windows)]
    #[test]
    fn both_shipped_platforms_have_a_service_on_windows() {
        let ids: Vec<&str> = services().iter().map(|s| s.id()).collect();
        assert!(ids.contains(&"gog"));
        assert!(ids.contains(&"jagex"));
    }
}
