//! A restore and a boot on the real backend over a scratch folder, with a
//! file the backend cannot read: locked with no sharing on Windows, its
//! permissions removed elsewhere.

use super::*;
use accshift_core::storage::{save_client_store, themes_dir, STORE_SETTINGS};
use serde_json::json;

const DESCRIPTOR: &str = include_str!("../../../../accshift-app/tests/data/real/descriptor.json");

/// A scratch app folder holding stored settings, so the boot detects no
/// launcher.
fn scratch(tag: &str) -> Arc<Scratch> {
    let root = std::env::temp_dir().join(format!(
        "accshift-egui-unreadable-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let scratch = Arc::new(Scratch { root });
    save_client_store(
        scratch.as_ref(),
        STORE_SETTINGS,
        &json!({"enabledPlatforms": [], "animations": "off"}),
    )
    .unwrap();
    scratch
}

fn boot(scratch: &Arc<Scratch>, ctx: &Context) -> AppModel {
    let backend: Arc<dyn Backend> = Arc::new(RealBackend::with_context(scratch.clone()));
    let mut app = AppModel::with_backend(ctx, backend, None, Clock::wall());
    app.wait_boot(ctx).expect("boot");
    app
}

/// `path` cannot be read until this drops.
struct Unreadable {
    #[cfg(windows)]
    _file: std::fs::File,
    #[cfg(not(windows))]
    path: PathBuf,
}

#[cfg(not(windows))]
impl Drop for Unreadable {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o644));
    }
}

#[cfg(windows)]
fn unreadable(path: &Path) -> Option<Unreadable> {
    use std::os::windows::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(path)
        .unwrap();
    Some(Unreadable { _file: file })
}

/// `None` when this process reads it anyway (root).
#[cfg(not(windows))]
fn unreadable(path: &Path) -> Option<Unreadable> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).unwrap();
    let guard = Unreadable {
        path: path.to_path_buf(),
    };
    std::fs::read(path).is_err().then_some(guard)
}

fn frame(app: &mut AppModel, ctx: &Context, focused: bool) {
    let mut input = egui::RawInput {
        focused,
        ..Default::default()
    };
    input
        .viewports
        .get_mut(&egui::ViewportId::ROOT)
        .unwrap()
        .focused = Some(focused);
    let mut output = ctx.run_ui(input, |ui| app.frame(ui));
    output.textures_delta.clear();
}

fn pump_until(app: &mut AppModel, ctx: &Context, what: &str, done: impl Fn(&AppModel) -> bool) {
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    while !done(app) {
        assert!(Instant::now() < deadline, "{what}");
        frame(app, ctx, true);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

#[test]
fn a_restore_whose_descriptors_do_not_all_read_asks_for_a_restart() {
    // Regression: the real backend answered a reload that could not read a
    // descriptor file as a whole one, so the restored settings were adopted
    // against platforms that may lack the one they enable.
    let from = scratch("restore-from");
    let live = from
        .root
        .join("live")
        .display()
        .to_string()
        .replace('\\', "/");
    let dir = accshift_core::platforms::descriptor::user_dir(from.as_ref()).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("egui-fixture.json"),
        DESCRIPTOR.replace("${LIVE}", &live),
    )
    .unwrap();
    save_client_store(
        from.as_ref(),
        STORE_SETTINGS,
        &json!({"enabledPlatforms": ["egui-fixture"], "defaultPlatformId": "egui-fixture", "animations": "off"}),
    )
    .unwrap();
    let file = from.root.join("backup.json");
    accshift_core::storage::export_backup(from.as_ref(), &file).unwrap();

    let to = scratch("restore-to");
    let dir = accshift_core::platforms::descriptor::user_dir(to.as_ref()).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let locked = dir.join("other.json");
    std::fs::write(&locked, DESCRIPTOR.replace("egui-fixture", "other-fixture")).unwrap();
    let ctx = Context::default();
    let mut app = boot(&to, &ctx);
    let Some(lock) = unreadable(&locked) else {
        return;
    };
    app.start_backup_restore(file.display().to_string());
    pump_until(&mut app, &ctx, "the restore never ended", |a| {
        a.backup.restart_needed() || !a.backup.holds_writes()
    });
    drop(lock);
    let settings = app.booted().unwrap().stores.settings();
    assert!(
        app.backup.restart_needed(),
        "RELOAD_INCOMPLETE: adopted after a partial reload, enabled {:?}",
        settings.enabled_platforms
    );
    assert!(app.backup.holds_writes());
}

#[test]
fn a_theme_locked_at_boot_is_read_again_on_the_next_focus() {
    // Regression: the boot took a themes folder it could not read whole
    // for read, so with the folder unchanged no focus read the locked
    // theme once it unlocked.
    let scratch = scratch("boot-theme");
    let dir = themes_dir(scratch.as_ref()).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("mine.json");
    std::fs::write(
        &path,
        r##"{"schemaVersion":1,"id":"mine","name":"Mine","colorScheme":"dark","tokens":{"bgCard":"#112233"}}"##,
    )
    .unwrap();
    let Some(lock) = unreadable(&path) else {
        return;
    };
    let ctx = Context::default();
    let mut app = boot(&scratch, &ctx);
    assert!(app.booted().unwrap().registry.document("mine").is_none());
    drop(lock);
    frame(&mut app, &ctx, false);
    frame(&mut app, &ctx, true);
    pump_until(
        &mut app,
        &ctx,
        "BOOT_THEMES: the theme locked at boot was never read again",
        |a| a.booted().unwrap().registry.document("mine").is_some(),
    );
}
