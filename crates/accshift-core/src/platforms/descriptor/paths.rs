//! Turning [`PathTemplate`]s into real paths, and refusing anything that lands
//! outside the descriptor's declared roots.
//!
//! The sandbox is enforced here rather than at the call sites, so a step added
//! to the engine later cannot forget it: the engine has no other way to obtain
//! a path.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use crate::error::{PlatformError, PlatformErrorKind};

use super::schema::{PathTemplate, Roots, INSTALL_DIR};

/// The values a template can be resolved against.
///
/// The environment is captured once per resolver rather than read per lookup,
/// so a switch cannot see two different values for the same variable, and so
/// tests can run against a fabricated environment on any OS.
#[derive(Debug, Clone, Default)]
pub struct PathResolver {
    env: HashMap<String, String>,
    install_dir: Option<PathBuf>,
}

impl PathResolver {
    /// Resolver reading this process's environment.
    pub fn from_process_env() -> Self {
        Self {
            env: std::env::vars().collect(),
            install_dir: None,
        }
    }

    /// Resolver over a fabricated environment, for tests and for dry runs on
    /// a machine that has no launcher installed.
    pub fn from_env<K, V>(vars: impl IntoIterator<Item = (K, V)>) -> Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        Self {
            env: vars
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
            install_dir: None,
        }
    }

    /// Attaches the directory holding the launcher binary, which templates
    /// reach through `${installDir}`.
    pub fn with_install_dir(mut self, install_dir: impl Into<PathBuf>) -> Self {
        self.install_dir = Some(install_dir.into());
        self
    }

    pub fn install_dir(&self) -> Option<&Path> {
        self.install_dir.as_deref()
    }

    fn lookup(&self, name: &str) -> Option<String> {
        if name == INSTALL_DIR {
            return self
                .install_dir
                .as_ref()
                .map(|dir| dir.to_string_lossy().into_owned());
        }
        // Windows environment variable names are case-insensitive, and
        // descriptors are written with the casing Microsoft documents
        // (`LOCALAPPDATA`, `ProgramFiles(x86)`). Falling back to a folded
        // lookup keeps a descriptor working on a machine whose variables are
        // spelled differently.
        self.env.get(name).cloned().or_else(|| {
            self.env
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.clone())
        })
    }

    /// Expands the placeholders and normalises the separators.
    ///
    /// A missing variable is an error rather than an empty expansion: an
    /// empty expansion would silently produce a path at the filesystem root.
    pub fn resolve(&self, template: &PathTemplate) -> Result<PathBuf, PlatformError> {
        let raw = template.as_str();
        let mut out = String::with_capacity(raw.len());
        let mut rest = raw;

        while let Some(start) = rest.find("${") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            let end = after.find('}').ok_or_else(|| {
                PlatformError::other(format!("Unterminated placeholder in path `{raw}`"))
            })?;
            let name = &after[..end];
            let value = self.lookup(name).ok_or_else(|| {
                PlatformError::new(
                    PlatformErrorKind::Io,
                    if name == INSTALL_DIR {
                        "The install directory is not known yet".to_string()
                    } else {
                        format!("{name} is not available on this system")
                    },
                )
            })?;
            out.push_str(value.trim_end_matches(['/', '\\']));
            rest = &after[end + 1..];
        }
        out.push_str(rest);

        Ok(PathBuf::from(normalise_separators(&out)))
    }
}

/// Uses the separator the running OS expects, so a descriptor can be written
/// with either.
fn normalise_separators(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        path.replace('\\', "/")
    }
}

/// The set of directories a descriptor is allowed to read and write.
///
/// Built once from the resolved roots. Roots that cannot resolve on this
/// machine are dropped rather than fatal: a descriptor may declare a root for
/// a variable that only exists on another edition of the OS, and the paths
/// under it will fail to resolve on their own.
#[derive(Debug, Clone, Default)]
pub struct Sandbox {
    roots: Vec<PathBuf>,
}

impl Sandbox {
    pub fn new(roots: &Roots, resolver: &PathResolver) -> Self {
        let roots = roots
            .files
            .iter()
            .filter_map(|template| resolver.resolve(template).ok())
            .map(|path| lexically_normalise(&path))
            .collect();
        Self { roots }
    }

    /// Sandbox allowing everything, for the parts of a dry run that only need
    /// to report what a step would touch.
    pub fn unrestricted() -> Self {
        Self { roots: Vec::new() }
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Refuses a path that is not inside a declared root.
    ///
    /// The comparison is lexical on the normalised path: `..` segments and
    /// separator noise are folded first, so a template resolved through an
    /// environment variable holding `C:\Users\x\..\y` cannot climb out.
    pub fn ensure_allowed(&self, path: &Path) -> Result<(), PlatformError> {
        if self.roots.is_empty() {
            return Ok(());
        }
        let candidate = lexically_normalise(path);
        let inside = self
            .roots
            .iter()
            .any(|root| path_starts_with(&candidate, root));
        if inside {
            Ok(())
        } else {
            Err(PlatformError::new(
                PlatformErrorKind::Io,
                format!(
                    "Refused to touch {}: it is outside this platform's declared folders",
                    path.display()
                ),
            ))
        }
    }
}

/// Folds `.` and `..` without touching the filesystem, because the path may
/// not exist yet (a snapshot restored into a directory the launcher will
/// create) and because `canonicalize` would follow symlinks out of the
/// sandbox.
pub fn lexically_normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Prefix test on whole components, so `C:\Data` never covers `C:\Database`.
/// Case-insensitive on Windows, where it matches how the filesystem compares.
fn path_starts_with(candidate: &Path, root: &Path) -> bool {
    let mut candidate_parts = candidate.components();
    for root_part in root.components() {
        let Some(candidate_part) = candidate_parts.next() else {
            return false;
        };
        let equal = if cfg!(windows) {
            candidate_part
                .as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(&root_part.as_os_str().to_string_lossy())
        } else {
            candidate_part == root_part
        };
        if !equal {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platforms::descriptor::schema::PathTemplate;

    fn resolver() -> PathResolver {
        let root = if cfg!(windows) {
            "C:\\Users\\demo\\AppData\\Local"
        } else {
            "/home/demo/.local/share"
        };
        PathResolver::from_env([("LOCALAPPDATA", root)])
    }

    fn template(raw: &str) -> PathTemplate {
        PathTemplate::new(raw)
    }

    #[test]
    fn placeholders_expand_and_separators_follow_the_os() {
        let path = resolver()
            .resolve(&template("${LOCALAPPDATA}/Demo/session.json"))
            .unwrap();
        let expected: PathBuf = if cfg!(windows) {
            "C:\\Users\\demo\\AppData\\Local\\Demo\\session.json".into()
        } else {
            "/home/demo/.local/share/Demo/session.json".into()
        };
        assert_eq!(path, expected);
    }

    #[test]
    fn a_missing_variable_is_an_error_not_an_empty_expansion() {
        // An empty expansion would resolve to the filesystem root and the
        // engine would happily walk it.
        let err = resolver()
            .resolve(&template("${NOT_SET_ANYWHERE}/Demo"))
            .unwrap_err();
        assert_eq!(err.kind, PlatformErrorKind::Io);
        assert_eq!(
            err.message,
            "NOT_SET_ANYWHERE is not available on this system"
        );
    }

    #[test]
    fn install_dir_resolves_only_once_the_binary_is_located() {
        let err = resolver()
            .resolve(&template("${installDir}/Config"))
            .unwrap_err();
        assert_eq!(err.message, "The install directory is not known yet");

        let with_dir = resolver().with_install_dir("C:\\Games\\Demo");
        let path = with_dir.resolve(&template("${installDir}/Config")).unwrap();
        assert!(path.ends_with("Config"));
    }

    #[test]
    fn trailing_separators_in_a_variable_do_not_double_up() {
        let resolver = PathResolver::from_env([("ROOT", "C:\\Demo\\")]);
        let path = resolver.resolve(&template("${ROOT}/session.json")).unwrap();
        assert_eq!(path.to_string_lossy().matches("\\\\").count(), 0);
    }

    #[test]
    fn sandbox_allows_paths_under_a_declared_root() {
        let roots = Roots {
            files: vec![template("${LOCALAPPDATA}/Demo")],
            registry: Vec::new(),
        };
        let resolver = resolver();
        let sandbox = Sandbox::new(&roots, &resolver);
        let inside = resolver
            .resolve(&template("${LOCALAPPDATA}/Demo/sub/session.json"))
            .unwrap();
        assert!(sandbox.ensure_allowed(&inside).is_ok());
    }

    #[test]
    fn sandbox_refuses_a_sibling_that_merely_shares_a_prefix() {
        let roots = Roots {
            files: vec![template("${LOCALAPPDATA}/Demo")],
            registry: Vec::new(),
        };
        let resolver = resolver();
        let sandbox = Sandbox::new(&roots, &resolver);
        let outside = resolver
            .resolve(&template("${LOCALAPPDATA}/DemoOther/session.json"))
            .unwrap();
        let err = sandbox.ensure_allowed(&outside).unwrap_err();
        assert!(err
            .message
            .contains("outside this platform's declared folders"));
    }

    #[test]
    fn sandbox_refuses_a_path_that_climbs_out_through_a_variable() {
        // The template is clean; the environment is not. Normalising before
        // the prefix test is what catches it.
        let resolver = PathResolver::from_env([("SNEAKY", "C:\\Demo\\..\\Windows\\System32")]);
        let roots = Roots {
            files: vec![template("C:/Demo")],
            registry: Vec::new(),
        };
        let sandbox = Sandbox::new(&roots, &resolver);
        let escaped = resolver.resolve(&template("${SNEAKY}/config.sys")).unwrap();
        assert!(sandbox.ensure_allowed(&escaped).is_err());
    }

    #[test]
    fn a_root_that_cannot_resolve_is_dropped_rather_than_fatal() {
        let roots = Roots {
            files: vec![
                template("${LOCALAPPDATA}/Demo"),
                template("${MISSING_ON_THIS_EDITION}/Demo"),
            ],
            registry: Vec::new(),
        };
        let sandbox = Sandbox::new(&roots, &resolver());
        assert_eq!(sandbox.roots().len(), 1);
    }

    #[test]
    fn normalisation_keeps_a_leading_parent_segment_it_cannot_fold() {
        assert_eq!(
            lexically_normalise(Path::new("../x")),
            PathBuf::from("../x")
        );
        assert_eq!(
            lexically_normalise(Path::new("a/./b/../c")),
            PathBuf::from("a/c")
        );
    }
}
