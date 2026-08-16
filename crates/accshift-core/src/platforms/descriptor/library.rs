//! Adding, previewing and removing a descriptor the user picked from a file.
//!
//! Everything here answers one question before the app changes anything: what
//! would this file add, and what would it touch. A descriptor is a program the
//! engine runs against the user's real launcher folders, so it is read,
//! validated and planned first, and only copied into [`super::user_dir`] once
//! the user has seen the plan.
//!
//! Nothing here executes a descriptor. The plan is built by the same code the
//! dry run uses, so what the preview shows is what a switch would do.

use super::engine::{DescriptorOrigin, DescriptorService};
use super::plan::DryRunPlan;
use super::schema::{Descriptor, DescriptorError, Os};
use super::user_dir;
use crate::context::AppContext;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// A descriptor file judged without installing it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DescriptorPreview {
    /// The file the user picked, as they picked it.
    pub source: String,
    pub descriptor: Descriptor,
    /// The name it would take in the descriptor folder. Always `<id>.json`, so
    /// two files describing the same platform cannot both be installed.
    pub file_name: String,
    /// True when a file of that name is already in the folder, so the button
    /// can say "replace" rather than "add".
    pub replaces: bool,
    /// Why installing this file would add no platform. Empty when it would.
    pub blocked: String,
    /// What a switch on this platform would read, copy, write and close.
    pub plan: Option<DryRunPlan>,
    /// Why no plan could be built. Empty when there is one.
    pub plan_problem: String,
}

/// Reads a descriptor file and reports what installing it would do.
///
/// The file is never copied and the descriptor is never registered: the service
/// built here lives for the length of the call, purely to plan a switch.
pub fn preview_file(
    app: &dyn AppContext,
    path: &Path,
) -> Result<DescriptorPreview, DescriptorError> {
    let source = display_name(path);
    let body = std::fs::read_to_string(path)
        .map_err(|e| DescriptorError::new(&source, "", format!("could not be read: {e}")))?;
    let descriptor = Descriptor::parse(&source, &body)?;

    let file_name = format!("{}.json", descriptor.id);
    let replaces = user_dir(app)
        .map(|dir| dir.join(&file_name).is_file())
        .unwrap_or(false);

    let blocked = blocking_reason(&descriptor);
    let (plan, plan_problem) = match plan_for(app, &descriptor) {
        Ok(plan) => (Some(plan), String::new()),
        Err(problem) => (None, problem),
    };

    Ok(DescriptorPreview {
        source,
        descriptor,
        file_name,
        replaces,
        blocked,
        plan,
        plan_problem,
    })
}

/// Copies a descriptor file into [`super::user_dir`] under `<id>.json`.
///
/// Validated again here rather than trusting the preview: the file could have
/// changed between the two calls, and this is the step that makes the app run
/// it. Returns the name it was written under.
pub fn install_file(app: &dyn AppContext, path: &Path) -> Result<String, String> {
    let preview = preview_file(app, path).map_err(|e| e.to_string())?;
    if !preview.blocked.is_empty() {
        return Err(preview.blocked);
    }

    let dir = user_dir(app)?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    let target = dir.join(&preview.file_name);

    // Copied rather than moved: the file the user picked is theirs, and a
    // failed install must not have eaten it.
    std::fs::copy(path, &target)
        .map_err(|e| format!("Could not write {}: {e}", target.display()))?;
    Ok(preview.file_name)
}

/// Deletes the descriptor file backing a user platform.
///
/// Only ever removes a `<id>.json` inside [`super::user_dir`], and only for an
/// id that is not one this build ships, so a bad id cannot reach a file that
/// does not belong to this feature.
pub fn remove(app: &dyn AppContext, id: &str) -> Result<(), String> {
    if !is_safe_id(id) {
        return Err(format!("`{id}` is not a platform id"));
    }
    if crate::platforms::is_shipped(id) {
        return Err(format!("`{id}` is a platform this build ships"));
    }

    let target = user_dir(app)?.join(format!("{id}.json"));
    match std::fs::remove_file(&target) {
        Ok(()) => Ok(()),
        // Already gone is the state the caller wanted.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Could not remove {}: {e}", target.display())),
    }
}

/// Why this descriptor would not become a platform, empty when it would.
fn blocking_reason(descriptor: &Descriptor) -> String {
    if crate::platforms::is_shipped(&descriptor.id) {
        return format!("`{}` is a platform this build already ships", descriptor.id);
    }
    if descriptor.current_profile().is_none() {
        let described: Vec<&str> = descriptor.os.keys().map(Os::as_str).collect();
        return format!(
            "`{}` describes {} and not this operating system",
            descriptor.id,
            described.join(", ")
        );
    }
    String::new()
}

/// A switch plan for a descriptor nobody has an account on yet.
///
/// The account id is invented from the charset the descriptor declares, so the
/// paths in the plan are shaped exactly like the real ones. It names no real
/// account, which is the point: the preview shows the shape of a switch, not
/// somebody's session.
fn plan_for(app: &dyn AppContext, descriptor: &Descriptor) -> Result<DryRunPlan, String> {
    let profile = descriptor
        .current_profile()
        .ok_or_else(|| "This descriptor has no profile for this operating system.".to_string())?;
    let format = &profile.identity.format;
    let sample = format.charset.sample(format.min_length.max(1));

    let service = DescriptorService::new(descriptor.clone(), DescriptorOrigin::Embedded);
    service.plan_switch(app, &sample)
}

/// The file name a preview reports, falling back to the whole path when the
/// picker handed back something with no file component.
fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Ids are joined into a file name, so anything that could climb out of the
/// folder is refused before it reaches the filesystem.
fn is_safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The descriptor folder, created if it is not there yet, so the caller can
/// show it to the user or open it in their file manager.
pub fn ensure_user_dir(app: &dyn AppContext) -> Result<PathBuf, String> {
    let dir = user_dir(app)?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{fixture, scratch, TempCtx};
    use super::*;

    fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn a_file_is_judged_without_being_installed() {
        let root = scratch("preview");
        let ctx = TempCtx { root: root.clone() };
        let picked = write(&root, "whatever-name.json", &fixture("acme", &root));

        let preview = preview_file(&ctx, &picked).unwrap();

        assert_eq!(preview.descriptor.id, "acme");
        assert_eq!(preview.source, "whatever-name.json");
        // The folder decides the name, not the file the user picked.
        assert_eq!(preview.file_name, "acme.json");
        assert!(!preview.replaces);
        assert!(preview.blocked.is_empty(), "{}", preview.blocked);
        assert!(
            !user_dir(&ctx).unwrap().join("acme.json").exists(),
            "a preview must not install anything"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_preview_carries_the_plan_a_switch_would_follow() {
        let root = scratch("preview-plan");
        let ctx = TempCtx { root: root.clone() };
        let picked = write(&root, "acme.json", &fixture("acme", &root));

        let preview = preview_file(&ctx, &picked).unwrap();

        let plan = preview.plan.expect(&preview.plan_problem);
        assert_eq!(plan.platform_id, "acme");
        assert!(!plan.applied, "a plan never claims to have run");
        assert!(!plan.roots.is_empty(), "the sandbox roots are shown");
        assert!(
            plan.steps
                .iter()
                .any(|step| step.target.ends_with("session.json")),
            "{:?}",
            plan.steps
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_that_does_not_validate_names_its_field() {
        let root = scratch("preview-bad");
        let ctx = TempCtx { root: root.clone() };
        let body = fixture("acme", &root).replace("\"schemaVersion\": 1", "\"schemaVersion\": 99");
        let picked = write(&root, "acme.json", &body);

        let error = preview_file(&ctx, &picked).unwrap_err();

        assert_eq!(error.field, "schemaVersion");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_shipped_id_is_blocked_before_it_is_offered() {
        let root = scratch("preview-shipped");
        let ctx = TempCtx { root: root.clone() };
        let picked = write(&root, "steam.json", &fixture("steam", &root));

        let preview = preview_file(&ctx, &picked).unwrap();

        assert!(preview.blocked.contains("already ships"), "{preview:?}");
        assert!(
            install_file(&ctx, &picked).is_err(),
            "and installing it is refused too"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn installing_writes_the_id_as_the_file_name_and_leaves_the_original_alone() {
        let root = scratch("install");
        let ctx = TempCtx { root: root.clone() };
        let picked = write(&root, "downloaded (1).json", &fixture("acme", &root));

        let written = install_file(&ctx, &picked).unwrap();

        assert_eq!(written, "acme.json");
        assert!(user_dir(&ctx).unwrap().join("acme.json").is_file());
        assert!(picked.is_file(), "the file the user picked is still theirs");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn installing_the_same_platform_twice_replaces_it_rather_than_piling_up() {
        let root = scratch("install-twice");
        let ctx = TempCtx { root: root.clone() };
        let first = write(&root, "one.json", &fixture("acme", &root));
        install_file(&ctx, &first).unwrap();

        let second = write(
            &root,
            "two.json",
            &fixture("acme", &root).replace("Fixture Launcher", "Renamed Launcher"),
        );
        let preview = preview_file(&ctx, &second).unwrap();
        install_file(&ctx, &second).unwrap();

        assert!(preview.replaces, "the caller can say replace, not add");
        let body = std::fs::read_to_string(user_dir(&ctx).unwrap().join("acme.json")).unwrap();
        assert!(body.contains("Renamed Launcher"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn removing_deletes_only_a_descriptor_in_the_folder() {
        let root = scratch("remove");
        let ctx = TempCtx { root: root.clone() };
        let picked = write(&root, "acme.json", &fixture("acme", &root));
        install_file(&ctx, &picked).unwrap();

        remove(&ctx, "acme").unwrap();

        assert!(!user_dir(&ctx).unwrap().join("acme.json").exists());
        assert!(
            remove(&ctx, "acme").is_ok(),
            "removing twice is not an error"
        );
        assert!(
            remove(&ctx, "steam").is_err(),
            "a shipped platform is refused"
        );
        assert!(
            remove(&ctx, "../../settings").is_err(),
            "an id that could climb out of the folder is refused"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
