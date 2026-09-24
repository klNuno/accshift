//! Load-time validation: every rule a descriptor must pass before the engine runs it.

#[allow(unused_imports)]
use super::*;

impl Descriptor {
    /// Parse and validate in one step, so no unvalidated descriptor exists.
    pub fn parse(source: &str, json: &str) -> Result<Self, DescriptorError> {
        let descriptor: Descriptor = serde_json::from_str(json).map_err(|e| {
            DescriptorError::new(
                source,
                format!("line {} column {}", e.line(), e.column()),
                format!("could not be read: {e}"),
            )
        })?;
        descriptor.validate(source)?;
        Ok(descriptor)
    }

    /// The profile for the OS this build runs on, if the platform supports it.
    pub fn current_profile(&self) -> Option<&OsProfile> {
        Os::current().and_then(|os| self.os.get(&os))
    }

    pub fn validate(&self, source: &str) -> Result<(), DescriptorError> {
        if self.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(DescriptorError::new(
                source,
                "schemaVersion",
                format!(
                    "expected {CURRENT_SCHEMA_VERSION}, found {}",
                    self.schema_version
                ),
            ));
        }
        validate_id(source, "id", &self.id)?;
        for (field, value) in [("name", &self.name), ("shortName", &self.short_name)] {
            if value.trim().is_empty() {
                return Err(DescriptorError::new(
                    source,
                    field,
                    "expected a non-empty display name, found an empty string",
                ));
            }
        }
        if self.os.is_empty() {
            return Err(DescriptorError::new(
                source,
                "os",
                "expected at least one of `windows`, `macos` or `linux`, found none",
            ));
        }
        for (os, profile) in &self.os {
            profile.validate(source, &format!("os.{}", os.as_str()), &self.id, *os)?;
        }
        Ok(())
    }
}

pub(super) fn validate_id(source: &str, field: &str, id: &str) -> Result<(), DescriptorError> {
    let ok = !id.is_empty()
        && id.len() <= 32
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(DescriptorError::new(
            source,
            field,
            format!("expected 1 to 32 lowercase letters, digits or `-`, found `{id}`"),
        ))
    }
}

impl OsProfile {
    pub(super) fn validate(
        &self,
        source: &str,
        field: &str,
        platform_id: &str,
        os: Os,
    ) -> Result<(), DescriptorError> {
        // The escape hatch is per platform, and it is checked here rather than
        // where the source is validated so the allowlist reads once.
        if let IdentitySource::NativeHook { name, .. } = &self.identity.source {
            if !NATIVE_HOOK_ALLOWLIST.contains(&platform_id) {
                return Err(DescriptorError::new(
                    source,
                    format!("{field}.identity.source.name"),
                    format!(
                        "expected no native hook: only {} may name one, found `{name}` on `{platform_id}`",
                        NATIVE_HOOK_ALLOWLIST.join(", ")
                    ),
                ));
            }
        }

        for (index, root) in self.roots.files.iter().enumerate() {
            let at = format!("{field}.roots.files[{index}]");
            root.validate(source, &at)?;
            validate_root_placeholders(source, &at, root, os)?;
            validate_root_depth(source, &at, root)?;
        }
        for (index, root) in self.roots.registry.iter().enumerate() {
            validate_registry_key(
                source,
                &format!("{field}.roots.registry[{index}].key"),
                &root.key,
            )?;
        }

        if !self.detect.executable_resolves && self.detect.path_exists.is_empty() {
            return Err(DescriptorError::new(
                source,
                format!("{field}.detect"),
                "expected `executableResolves` or at least one `pathExists` entry, found neither",
            ));
        }
        for (index, path) in self.detect.path_exists.iter().enumerate() {
            path.validate(source, &format!("{field}.detect.pathExists[{index}]"))?;
        }

        match &self.executable {
            Some(executable) => executable.validate(source, &format!("{field}.executable"), os)?,
            None => {
                if self.detect.executable_resolves {
                    return Err(DescriptorError::new(
                        source,
                        format!("{field}.executable"),
                        "expected an executable block, found none while `detect.executableResolves` is set",
                    ));
                }
                if self.launch.is_some() {
                    return Err(DescriptorError::new(
                        source,
                        format!("{field}.executable"),
                        "expected an executable block, found none while `launch` is set",
                    ));
                }
            }
        }

        self.identity
            .validate(source, &format!("{field}.identity"), &self.roots)?;
        self.validate_state(source, field)?;

        if let Some(launch) = &self.launch {
            let guard = launch.args_only_for.trim();
            if !guard.is_empty() && guard.contains(['/', '\\']) {
                return Err(DescriptorError::new(
                    source,
                    format!("{field}.launch.argsOnlyFor"),
                    format!("expected a bare binary name, found `{guard}`"),
                ));
            }
            if !guard.is_empty() && launch.args.is_empty() {
                return Err(DescriptorError::new(
                    source,
                    format!("{field}.launch.args"),
                    "expected the arguments `argsOnlyFor` guards, found none",
                ));
            }
        }

        for (index, process) in self.close.processes.iter().enumerate() {
            if process.trim().is_empty() || process.contains(['/', '\\']) {
                return Err(DescriptorError::new(
                    source,
                    format!("{field}.close.processes[{index}]"),
                    format!("expected a bare process name, found `{process}`"),
                ));
            }
        }

        for (index, condition) in self
            .setup
            .trigger
            .iter()
            .chain(self.setup.confirm.iter())
            .enumerate()
        {
            condition.validate(source, &format!("{field}.setup[{index}]"), &self.roots)?;
        }

        if !self.state.is_empty() && self.close.processes.is_empty() {
            return Err(DescriptorError::new(
                source,
                format!("{field}.close.processes"),
                "expected the processes to close before session files are replaced, found none",
            ));
        }

        Ok(())
    }

    pub(super) fn validate_state(&self, source: &str, field: &str) -> Result<(), DescriptorError> {
        let mut snapshot_names: Vec<&str> = Vec::new();

        for (index, item) in self.state.files.iter().enumerate() {
            let at = format!("{field}.state.files[{index}]");
            validate_spec_in_file_roots(
                source,
                &format!("{at}.live"),
                &item.live,
                &self.roots,
                false,
            )?;
            validate_snapshot_name(source, &format!("{at}.snapshot"), &item.snapshot)?;
            snapshot_names.push(&item.snapshot);
        }
        for (index, item) in self.state.directories.iter().enumerate() {
            let at = format!("{field}.state.directories[{index}]");
            validate_spec_in_file_roots(
                source,
                &format!("{at}.live"),
                &item.live,
                &self.roots,
                false,
            )?;
            validate_snapshot_name(source, &format!("{at}.snapshot"), &item.snapshot)?;
            snapshot_names.push(&item.snapshot);
        }
        for (index, path) in self.state.caches.iter().enumerate() {
            let at = format!("{field}.state.caches[{index}]");
            path.validate(source, &at)?;
            validate_inside_file_roots(source, &at, path, &self.roots)?;
        }
        for (index, condition) in self.state.capture_when.iter().enumerate() {
            condition.validate(
                source,
                &format!("{field}.state.captureWhen[{index}]"),
                &self.roots,
            )?;
        }
        for (index, item) in self.state.registry_values.iter().enumerate() {
            let at = format!("{field}.state.registryValues[{index}]");
            validate_registry_key(source, &format!("{at}.key"), &item.key)?;
            validate_in_registry_roots(
                source,
                &format!("{at}.key"),
                item.root,
                &item.key,
                &self.roots,
            )?;
            if item.value.trim().is_empty() {
                return Err(DescriptorError::new(
                    source,
                    format!("{at}.value"),
                    "expected a registry value name, found an empty string",
                ));
            }
            validate_snapshot_name(source, &format!("{at}.snapshot"), &item.snapshot)?;
            snapshot_names.push(&item.snapshot);
        }

        // Two entries writing the same snapshot name would silently overwrite
        // each other, and the loser would restore the winner's bytes.
        for (index, name) in snapshot_names.iter().enumerate() {
            if snapshot_names[..index]
                .iter()
                .any(|earlier| earlier.eq_ignore_ascii_case(name))
            {
                return Err(DescriptorError::new(
                    source,
                    format!("{field}.state"),
                    format!("expected every `snapshot` name to be unique, found `{name}` twice"),
                ));
            }
        }

        Ok(())
    }
}

impl Executable {
    pub(super) fn validate(
        &self,
        source: &str,
        field: &str,
        os: Os,
    ) -> Result<(), DescriptorError> {
        if self.file_name.trim().is_empty() || self.file_name.contains(['/', '\\']) {
            return Err(DescriptorError::new(
                source,
                format!("{field}.fileName"),
                format!("expected a bare binary name, found `{}`", self.file_name),
            ));
        }
        if self.candidates.is_empty() {
            return Err(DescriptorError::new(
                source,
                format!("{field}.candidates"),
                "expected at least one place to look for the binary, found none",
            ));
        }
        for (index, probe) in self.relative_probes.iter().enumerate() {
            let bad = probe.trim().is_empty()
                || probe != probe.trim()
                || probe.starts_with(['/', '\\'])
                || probe.contains(':')
                || probe
                    .split(['/', '\\'])
                    .any(|part| part == ".." || part.is_empty());
            if bad {
                return Err(DescriptorError::new(
                    source,
                    format!("{field}.relativeProbes[{index}]"),
                    format!("expected a relative sub-directory, found `{probe}`"),
                ));
            }
        }
        for (index, candidate) in self.candidates.iter().enumerate() {
            let at = format!("{field}.candidates[{index}]");
            match candidate {
                ExecutableCandidate::Path { template } => {
                    template.validate(source, &format!("{at}.template"))?;
                    if template.placeholders().iter().any(|p| p == INSTALL_DIR) {
                        return Err(DescriptorError::new(
                            source,
                            format!("{at}.template"),
                            "expected a template that does not use `${installDir}`: the install directory is what this candidate resolves",
                        ));
                    }
                    // Same anchors as a root: a candidate built on `${ComSpec}`
                    // or a literal system path would let a shared descriptor
                    // launch any program on every switch.
                    validate_root_placeholders(source, &format!("{at}.template"), template, os)?;
                    if !template.as_str().trim_start().starts_with("${") {
                        return Err(DescriptorError::new(
                            source,
                            format!("{at}.template"),
                            format!(
                                "expected a template starting with one of {}, found `{}`",
                                known_root_placeholders(os)
                                    .iter()
                                    .filter(|name| **name != INSTALL_DIR)
                                    .map(|name| format!("${{{name}}}"))
                                    .collect::<Vec<_>>()
                                    .join(", "),
                                template.as_str()
                            ),
                        ));
                    }
                }
                ExecutableCandidate::Registry { key, value, .. } => {
                    validate_registry_key(source, &format!("{at}.key"), key)?;
                    if value.trim().is_empty() {
                        return Err(DescriptorError::new(
                            source,
                            format!("{at}.value"),
                            "expected a registry value name, found an empty string",
                        ));
                    }
                }
                ExecutableCandidate::UninstallEntry {
                    display_name,
                    value,
                } => {
                    if display_name.trim().is_empty() {
                        return Err(DescriptorError::new(
                            source,
                            format!("{at}.displayName"),
                            "expected the name the launcher registers under, found an empty string",
                        ));
                    }
                    if value.trim().is_empty() {
                        return Err(DescriptorError::new(
                            source,
                            format!("{at}.value"),
                            "expected a registry value name, found an empty string",
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

impl Identity {
    pub(super) fn validate(
        &self,
        source: &str,
        field: &str,
        roots: &Roots,
    ) -> Result<(), DescriptorError> {
        if self.format.max_length == 0 || self.format.max_length > 256 {
            return Err(DescriptorError::new(
                source,
                format!("{field}.format.maxLength"),
                format!("expected 1 to 256, found {}", self.format.max_length),
            ));
        }
        if self.format.min_length == 0 || self.format.min_length > self.format.max_length {
            return Err(DescriptorError::new(
                source,
                format!("{field}.format.minLength"),
                format!(
                    "expected 1 to maxLength ({}), found {}",
                    self.format.max_length, self.format.min_length
                ),
            ));
        }
        for (index, discovery) in self.discovery.iter().enumerate() {
            let at = format!("{field}.discovery[{index}]");
            match discovery {
                Discovery::DirectoryEntries {
                    path,
                    strip_prefixes,
                    ..
                } => {
                    path.validate(source, &format!("{at}.path"))?;
                    validate_in_file_roots(source, &format!("{at}.path"), path, roots)?;
                    for (prefix_index, prefix) in strip_prefixes.iter().enumerate() {
                        if prefix.is_empty() {
                            return Err(DescriptorError::new(
                                source,
                                format!("{at}.stripPrefixes[{prefix_index}]"),
                                "expected a prefix to strip, found an empty string",
                            ));
                        }
                    }
                }
            }
        }
        if self.blocklist_on_forget && self.discovery.is_empty() {
            return Err(DescriptorError::new(
                source,
                format!("{field}.blocklistOnForget"),
                "expected at least one `discovery` entry: nothing can resurrect a forgotten account without one",
            ));
        }
        match &self.source {
            IdentitySource::Registry { root, key, value } => {
                validate_registry_key(source, &format!("{field}.source.key"), key)?;
                validate_in_registry_roots(
                    source,
                    &format!("{field}.source.key"),
                    *root,
                    key,
                    roots,
                )?;
                if value.trim().is_empty() {
                    return Err(DescriptorError::new(
                        source,
                        format!("{field}.source.value"),
                        "expected a registry value name, found an empty string",
                    ));
                }
            }
            IdentitySource::Synthetic => {
                if self.current == CurrentSource::Identity {
                    return Err(DescriptorError::new(
                        source,
                        format!("{field}.current"),
                        "expected `config`: a synthetic id has no live source to read the current account from, found `identity`",
                    ));
                }
            }
            IdentitySource::LogTail {
                path,
                tail_bytes,
                line_contains,
                prefix,
                near_word,
            } => {
                path.validate(source, &format!("{field}.source.path"))?;
                validate_in_file_roots(source, &format!("{field}.source.path"), path, roots)?;
                if *tail_bytes == 0 {
                    return Err(DescriptorError::new(
                        source,
                        format!("{field}.source.tailBytes"),
                        "expected a tail size above zero, found 0",
                    ));
                }
                if line_contains.trim().is_empty() {
                    return Err(DescriptorError::new(
                        source,
                        format!("{field}.source.lineContains"),
                        "expected the marker that identifies a sign-in line, found an empty string",
                    ));
                }
                if prefix.is_empty() && near_word.is_empty() {
                    return Err(DescriptorError::new(
                        source,
                        format!("{field}.source.prefix"),
                        "expected `prefix` or `nearWord`, found neither: without one, any id on the line would do",
                    ));
                }
            }
            IdentitySource::NativeHook { name, paths } => {
                let hook = validate_hook_name(source, &format!("{field}.source.name"), name)?;
                // The hook only ever sees paths the descriptor declared here,
                // so they are held to the sandbox like any other.
                for (key, path) in paths {
                    let at = format!("{field}.source.paths.{key}");
                    path.validate(source, &at)?;
                    validate_in_file_roots(source, &at, path, roots)?;
                }
                for required in hook.required_paths() {
                    if !paths.contains_key(*required) {
                        return Err(DescriptorError::new(
                            source,
                            format!("{field}.source.paths"),
                            format!(
                                "expected a `{required}` path for hook `{name}`, found {}",
                                describe_keys(paths)
                            ),
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

impl Condition {
    pub(super) fn validate(
        &self,
        source: &str,
        field: &str,
        roots: &Roots,
    ) -> Result<(), DescriptorError> {
        match self {
            Condition::NewIdentity | Condition::IdentityPresent => Ok(()),
            Condition::AnyOf { conditions } => {
                if conditions.is_empty() {
                    return Err(DescriptorError::new(
                        source,
                        format!("{field}.conditions"),
                        "expected at least one nested condition, found an empty list",
                    ));
                }
                for (index, nested) in conditions.iter().enumerate() {
                    nested.validate(source, &format!("{field}.conditions[{index}]"), roots)?;
                }
                Ok(())
            }
            Condition::PathNonEmpty { path, .. } => {
                validate_spec_in_file_roots(source, &format!("{field}.path"), path, roots, true)
            }
            Condition::PathFresh { path, window_ms } => {
                validate_spec_in_file_roots(source, &format!("{field}.path"), path, roots, true)?;
                if *window_ms == 0 {
                    return Err(DescriptorError::new(
                        source,
                        format!("{field}.windowMs"),
                        "expected a freshness window above zero, found 0",
                    ));
                }
                Ok(())
            }
            Condition::SinceStart { ms } => {
                if *ms == 0 {
                    return Err(DescriptorError::new(
                        source,
                        format!("{field}.ms"),
                        "expected a delay above zero, found 0: a condition that always holds is not one",
                    ));
                }
                Ok(())
            }
        }
    }
}

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
        // resolver folds case when it looks one up, so the check does too.
        let matches = known.iter().any(|candidate| {
            if os == Os::Windows {
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
