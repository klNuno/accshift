//! The field rules the validators share: names, registry keys, placeholders
//! and the roots a path may sit under.

#[allow(unused_imports)]
use super::*;

/// Name of the placeholder standing for the launcher's install directory.
pub const INSTALL_DIR: &str = "installDir";

/// Resolves a hook name against the compiled registry. A descriptor naming a
/// hook this build does not have would otherwise be a step that silently does
/// nothing on the machine where it matters.
pub(super) fn validate_hook_name(
    source: &str,
    field: &str,
    name: &str,
) -> Result<&'static dyn hooks::NativeHook, DescriptorError> {
    if name.trim().is_empty() {
        return Err(DescriptorError::new(
            source,
            field,
            "expected a hook name, found an empty string",
        ));
    }
    hooks::hook(name).ok_or_else(|| {
        DescriptorError::new(
            source,
            field,
            format!(
                "expected one of {}, found `{name}`",
                hooks::names().join(", ")
            ),
        )
    })
}

pub(super) fn describe_keys(paths: &BTreeMap<String, PathTemplate>) -> String {
    if paths.is_empty() {
        "none".to_string()
    } else {
        paths.keys().cloned().collect::<Vec<_>>().join(", ")
    }
}

pub(super) fn validate_snapshot_name(
    source: &str,
    field: &str,
    name: &str,
) -> Result<(), DescriptorError> {
    let ok = !name.trim().is_empty()
        && name == name.trim()
        && !name.contains(['/', '\\'])
        && name != "."
        && name != "..";
    if ok {
        Ok(())
    } else {
        Err(DescriptorError::new(
            source,
            field,
            format!("expected a bare file or directory name, found `{name}`"),
        ))
    }
}

pub(super) fn validate_registry_key(
    source: &str,
    field: &str,
    key: &str,
) -> Result<(), DescriptorError> {
    let ok = !key.trim().is_empty()
        && !key.contains('/')
        && !key.split('\\').any(|part| part == ".." || part.is_empty());
    if ok {
        Ok(())
    } else {
        Err(DescriptorError::new(
            source,
            field,
            format!("expected a backslash-separated registry key, found `{key}`"),
        ))
    }
}

/// Placeholders a root may be written with, per OS.
///
/// The list is deliberately short: a root is a well-known per-user or
/// machine-wide directory, plus the launcher's own install directory. Anything
/// else is either a typo or a variable only some machines carry, and since a
/// root that does not resolve now stops the whole profile rather than being
/// dropped, the descriptor is refused at load instead of on a user's machine.
pub(super) fn known_root_placeholders(os: Os) -> &'static [&'static str] {
    match os {
        Os::Windows => &[
            INSTALL_DIR,
            "APPDATA",
            "LOCALAPPDATA",
            "ProgramData",
            "ProgramFiles",
            "ProgramFiles(x86)",
            "PUBLIC",
            "SystemDrive",
            "USERPROFILE",
        ],
        Os::Macos => &[INSTALL_DIR, "HOME"],
        Os::Linux => &[
            INSTALL_DIR,
            "HOME",
            "XDG_CACHE_HOME",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_STATE_HOME",
        ],
    }
}

/// Refuses a root written against a placeholder that has no meaning on the OS
/// the profile targets.
pub(super) fn validate_root_placeholders(
    source: &str,
    field: &str,
    root: &PathTemplate,
    os: Os,
) -> Result<(), DescriptorError> {
    let known = known_root_placeholders(os);
    for name in root.placeholders() {
        // Windows environment variable names are case-insensitive, and the
        // resolver folds case when it looks one up. The installDir built-in
        // is matched exactly on every OS.
        let matches = known.iter().any(|candidate| {
            if os == Os::Windows && *candidate != INSTALL_DIR {
                candidate.eq_ignore_ascii_case(&name)
            } else {
                *candidate == name
            }
        });
        if !matches {
            return Err(DescriptorError::new(
                source,
                field,
                format!(
                    "expected a root built from one of {}, found `${{{name}}}`",
                    known.join(", ")
                ),
            ));
        }
    }
    Ok(())
}

/// Refuses a root so shallow that clearing inside it could reach system or
/// user-wide folders: `C:/`, `/`, a bare `${USERPROFILE}` or `${SystemDrive}`.
/// A placeholder root needs a folder of its own below it, except
/// `${installDir}`, which already names the launcher; a literal root needs two.
pub(super) fn validate_root_depth(
    source: &str,
    field: &str,
    root: &PathTemplate,
) -> Result<(), DescriptorError> {
    let mut segments = root
        .as_str()
        .split(['/', '\\'])
        .map(str::trim)
        .filter(|segment| !segment.is_empty());
    let first = segments.next().unwrap_or("");
    let below = segments.count();
    let needed = if first == format!("${{{INSTALL_DIR}}}") {
        0
    } else if first.starts_with("${") {
        1
    } else if first.ends_with(':') {
        // `C:` is the drive, not a folder.
        2
    } else {
        // `/home/...`: the first segment is already a folder under `/`.
        1
    };
    if below < needed {
        return Err(DescriptorError::new(
            source,
            field,
            format!(
                "expected a root naming the launcher's own folder, found `{}`",
                root.as_str()
            ),
        ));
    }
    Ok(())
}

/// The sandbox check the loader can make ahead of time: a template whose
/// literal text does not start with a declared root can never resolve inside
/// one, whatever the environment holds.
pub(super) fn validate_in_file_roots(
    source: &str,
    field: &str,
    path: &PathTemplate,
    roots: &Roots,
) -> Result<(), DescriptorError> {
    validate_under_file_roots(source, field, path, roots, true)
}

/// Like [`validate_in_file_roots`], for a path the engine writes over or
/// deletes: it must sit strictly below a root, never be the root itself.
pub(super) fn validate_inside_file_roots(
    source: &str,
    field: &str,
    path: &PathTemplate,
    roots: &Roots,
) -> Result<(), DescriptorError> {
    validate_under_file_roots(source, field, path, roots, false)
}

pub(super) fn validate_under_file_roots(
    source: &str,
    field: &str,
    path: &PathTemplate,
    roots: &Roots,
    allow_root_itself: bool,
) -> Result<(), DescriptorError> {
    if roots.files.is_empty() {
        return Err(DescriptorError::new(
            source,
            field,
            "expected at least one entry in `roots.files`, found none while the descriptor reads or writes files",
        ));
    }
    let candidate = normalise_template_text(path.as_str());
    let covered = roots.files.iter().any(|root| {
        let root_text = normalise_template_text(root.as_str());
        (allow_root_itself && candidate == root_text)
            || candidate.starts_with(&format!("{}/", root_text.trim_end_matches('/')))
    });
    if covered {
        Ok(())
    } else {
        Err(DescriptorError::new(
            source,
            field,
            format!(
                "expected a path under one of the declared roots ({}), found `{}`",
                roots
                    .files
                    .iter()
                    .map(|r| r.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                path.as_str()
            ),
        ))
    }
}

/// Same check for a spec: every candidate has to sit inside the roots, since
/// any of them may be the one that ends up being read or written.
pub(super) fn validate_spec_in_file_roots(
    source: &str,
    field: &str,
    spec: &PathSpec,
    roots: &Roots,
    allow_root_itself: bool,
) -> Result<(), DescriptorError> {
    spec.validate(source, field)?;
    let many = matches!(spec, PathSpec::FirstExisting(_));
    for (index, template) in spec.candidates().iter().enumerate() {
        let at = if many {
            format!("{field}[{index}]")
        } else {
            field.to_string()
        };
        validate_under_file_roots(source, &at, template, roots, allow_root_itself)?;
    }
    Ok(())
}

pub(super) fn validate_in_registry_roots(
    source: &str,
    field: &str,
    hive: RegistryHive,
    key: &str,
    roots: &Roots,
) -> Result<(), DescriptorError> {
    let covered = roots.registry.iter().any(|root| {
        root.root == hive
            && (key.eq_ignore_ascii_case(&root.key)
                || key.to_ascii_lowercase().starts_with(&format!(
                    "{}\\",
                    root.key.trim_end_matches('\\').to_ascii_lowercase()
                )))
    });
    if covered {
        Ok(())
    } else {
        Err(DescriptorError::new(
            source,
            field,
            format!(
                "expected a key under one of the declared registry roots ({}), found `{}\\{key}`",
                roots
                    .registry
                    .iter()
                    .map(|r| format!("{}\\{}", r.root.as_str(), r.key))
                    .collect::<Vec<_>>()
                    .join(", "),
                hive.as_str()
            ),
        ))
    }
}

/// Compare templates on their text, separators and case folded, so a root
/// written with `\` covers a path written with `/`.
pub(super) fn normalise_template_text(text: &str) -> String {
    text.replace('\\', "/").to_ascii_lowercase()
}
