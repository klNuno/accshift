//! The boot job `main` starts before the window exists
//! ([`super::super::early_boot`]): the model polls it as its own.

use super::super::early_boot;
use super::*;

fn mock() -> Arc<MockBackend> {
    let fixture = include_str!("../../../parity/data/empty.json");
    Arc::new(MockBackend::from_json(fixture, None).unwrap())
}

fn frame(app: &mut AppModel, ctx: &Context) {
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.frame(ui));
    output.textures_delta.clear();
}

#[test]
fn a_boot_read_before_the_model_lands_on_its_first_frame() {
    let ctx = Context::default();
    let mock = mock();
    let (early, task) = early_boot();
    task.run(mock.as_ref());
    let mut app = AppModel::from_backend(&ctx, mock.clone(), Clock::wall(), Some(early));
    frame(&mut app, &ctx);
    assert!(app.booted().is_some());
    assert!(app.boot_error().is_none());
}

#[test]
fn a_boot_read_after_the_model_exists_asks_for_a_frame() {
    let ctx = Context::default();
    let repaints = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = Arc::clone(&repaints);
    ctx.set_request_repaint_callback(move |_| {
        seen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    });
    let mock = mock();
    let (early, task) = early_boot();
    let mut app = AppModel::from_backend(&ctx, mock.clone(), Clock::wall(), Some(early));
    let before = repaints.load(std::sync::atomic::Ordering::Relaxed);
    std::thread::spawn(move || task.run(mock.as_ref()))
        .join()
        .unwrap();
    assert!(repaints.load(std::sync::atomic::Ordering::Relaxed) > before);
    app.wait_boot(&ctx).unwrap();
    assert!(app.booted().is_some());
}

#[test]
fn a_boot_thread_that_dies_reports_the_boot_as_failed() {
    let ctx = Context::default();
    let mock = mock();
    let (early, task) = early_boot();
    drop(task);
    let mut app = AppModel::from_backend(&ctx, mock.clone(), Clock::wall(), Some(early));
    assert!(app.wait_boot(&ctx).is_err());
    assert_eq!(app.boot_error(), Some("boot job panicked"));
}
