//! The workspace driven like a scene run, without a window: the app on the
//! mock, frames on the wall clock, pointer events by hand.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::{pos2, vec2, Context, Event, Key, Modifiers, PointerButton, Pos2, RawInput};

use crate::harness;
use crate::screens::Screen;

type Box4 = (String, f32, f32, f32, f32);

pub(super) struct Run {
    pub ctx: Context,
    pub screen: Screen,
    start: Instant,
    events: Vec<Event>,
    scale: f32,
}

impl Run {
    pub fn new(scene: &str, scale: f32) -> Run {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("parity/scenes")
            .join(format!("{scene}.json"));
        let scene = harness::load(&path).unwrap();
        let ctx = Context::default();
        let screen = Screen::for_scene(&ctx, &scene).unwrap();
        let mut run = Run {
            ctx,
            screen,
            start: Instant::now(),
            events: Vec::new(),
            scale,
        };
        run.frame();
        run
    }

    pub fn input(&mut self) -> RawInput {
        let mut input = RawInput {
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(860.0, 440.0))),
            time: Some(self.start.elapsed().as_secs_f64()),
            events: std::mem::take(&mut self.events),
            ..Default::default()
        };
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(self.scale);
        input
    }

    pub fn frame(&mut self) -> egui::FullOutput {
        let input = self.input();
        let screen = &mut self.screen;
        let mut out = self.ctx.run_ui(input, |ui| screen.ui(ui));
        out.textures_delta.clear();
        out
    }

    /// Frames until the screen is ready three frames in a row.
    pub fn settle(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut streak = 0;
        while streak < 3 {
            assert!(Instant::now() < deadline, "never settled");
            self.frame();
            streak = if self.screen.ready() { streak + 1 } else { 0 };
            std::thread::sleep(Duration::from_millis(4));
        }
    }

    pub fn send(&mut self, event: Event) {
        self.events.push(event);
    }

    pub fn click(&mut self, at: Pos2) {
        self.send(Event::PointerMoved(at));
        self.frame();
        for pressed in [true, false] {
            self.send(Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            self.frame();
            std::thread::sleep(Duration::from_millis(40));
        }
        self.send(Event::PointerMoved(pos2(250.0, 18.0)));
        self.frame();
        self.settle();
    }

    /// A right click: the context menu opens on the release.
    pub fn rclick(&mut self, at: Pos2) {
        self.send(Event::PointerMoved(at));
        self.frame();
        for pressed in [true, false] {
            self.send(Event::PointerButton {
                pos: at,
                button: PointerButton::Secondary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            self.frame();
        }
        self.frame();
    }

    pub fn key(&mut self, key: Key) {
        for pressed in [true, false] {
            self.send(Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Modifiers::NONE,
            });
            self.frame();
        }
    }

    pub fn workspace(&self) -> &super::Workspace {
        match &self.screen {
            Screen::App(app) => &app.booted().unwrap().workspace,
            _ => panic!("not the app"),
        }
    }

    /// `(label, x, y, w, h)` of every box on screen, scrolled.
    pub fn boxes(&self) -> Vec<Box4> {
        let ws = self.workspace();
        // The content box at (0, 36), padded 10 by 16.
        ws.layout
            .iter()
            .map(|(label, r)| {
                (
                    label.clone(),
                    r.left() + super::PAD_X,
                    r.top() + 36.0 + super::PAD_TOP - ws.scroll(),
                    r.width(),
                    r.height(),
                )
            })
            .collect()
    }
}

/// Rows of 7 cards from `(45, first_y)`, 110 by 146 apart, all 100 by
/// 136: the reference boxes of these scenes.
fn cards(labels: &[&str], first_y: f32) -> Vec<Box4> {
    labels
        .iter()
        .enumerate()
        .map(|(i, l)| {
            (
                l.to_string(),
                45.0 + 110.0 * (i % 7) as f32,
                first_y + 146.0 * (i / 7) as f32,
                100.0,
                136.0,
            )
        })
        .collect()
}

fn only_cards(boxes: Vec<Box4>) -> Vec<Box4> {
    boxes.into_iter().filter(|b| b.4 == 136.0).collect()
}

/// The search field edits like a one-line text input: the caret moves, a paste
/// replaces the selection without its line break, cut and undo work.
#[test]
fn search_edits_like_an_input() {
    let key = |key: Key, modifiers: Modifiers| Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };
    let mut run = Run::new("workspace-glass-dark-100", 1.0);
    run.settle();
    run.click(pos2(639.0, 60.0));
    run.send(Event::Text("rnked".into()));
    run.frame();
    run.send(key(Key::Home, Modifiers::NONE));
    run.send(key(Key::ArrowRight, Modifiers::NONE));
    run.frame();
    run.send(Event::Text("a".into()));
    run.frame();
    run.settle();
    assert_eq!(only_cards(run.boxes()), cards(&["ranked grind"], 84.0));
    run.send(key(Key::A, Modifiers::COMMAND));
    run.frame();
    run.send(Event::Paste("trad\n".into()));
    run.frame();
    run.settle();
    assert_eq!(only_cards(run.boxes()), cards(&["trading"], 84.0));
    run.send(key(Key::A, Modifiers::COMMAND));
    run.frame();
    run.send(Event::Cut);
    run.frame();
    run.settle();
    assert_eq!(only_cards(run.boxes()).len(), 14);
    run.send(key(Key::Z, Modifiers::COMMAND));
    run.frame();
    run.settle();
    assert_eq!(only_cards(run.boxes()), cards(&["trading"], 84.0));
}

/// The mouse places the caret, a double click takes the word and its
/// trailing space, a drag selects.
#[test]
fn search_follows_the_mouse() {
    let mut run = Run::new("workspace-glass-dark-100", 1.0);
    run.settle();
    run.click(pos2(639.0, 60.0));
    let id = egui::Id::unique(crate::app::keys::SEARCH_ID);
    let field = run.ctx.read_response(id).unwrap().rect;
    // The text starts after the 1 px border and the 10 px padding.
    let at = |dx: f32| pos2(field.left() + 11.0 + dx, field.center().y);
    let query = |run: &Run| match &run.screen {
        Screen::App(app) => app
            .booted()
            .unwrap()
            .accounts
            .navigation
            .search_query()
            .to_string(),
        _ => panic!("not the app"),
    };
    let press = |run: &mut Run, pos: Pos2, pressed: bool| {
        run.send(Event::PointerMoved(pos));
        run.send(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        run.frame();
    };

    run.send(Event::Text("anked".into()));
    run.frame();
    run.key(Key::Home);
    // Past the text's end: the caret goes to the end.
    press(&mut run, at(80.0), true);
    press(&mut run, at(80.0), false);
    run.send(Event::Text("x".into()));
    run.frame();
    assert_eq!(query(&run), "ankedx");

    // A drag from the end to the start selects everything.
    press(&mut run, at(80.0), true);
    run.send(Event::PointerMoved(at(0.0)));
    run.frame();
    press(&mut run, at(-4.0), false);
    run.send(Event::Text("ranked grind".into()));
    run.frame();
    assert_eq!(query(&run), "ranked grind");

    // A double click on "ranked" takes "ranked ".
    std::thread::sleep(Duration::from_millis(600));
    for pressed in [true, false, true, false] {
        press(&mut run, at(8.0), pressed);
    }
    run.send(Event::Text("x".into()));
    run.frame();
    assert_eq!(query(&run), "xgrind");
}

/// The theme editor's custom CSS edits like a multi-line text area: Enter
/// breaks the line, the arrows move by row, a paste keeps its lines.
#[test]
fn theme_css_edits_like_a_textarea() {
    fn wait(run: &mut Run, ms: u64) {
        let until = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < until {
            run.frame();
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    let key = |key: Key| Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    // The steps of `theme-editor-glass-dark-100` up to its "css" shot.
    let mut run = Run::new("theme-editor-glass-dark-100", 1.0);
    run.settle();
    wait(&mut run, 600);
    run.click(pos2(87.0, 18.0));
    wait(&mut run, 700);
    run.click(pos2(566.0, 376.0));
    wait(&mut run, 600);
    run.send(Event::PointerMoved(pos2(430.0, 220.0)));
    run.send(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, -4000.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: Modifiers::NONE,
    });
    wait(&mut run, 400);
    run.click(pos2(400.0, 230.0));
    let css = |run: &Run| match &run.screen {
        Screen::App(app) => app
            .settings_panel()
            .and_then(|p| p.theme_editor.as_ref())
            .map(|e| e.css.clone())
            .unwrap(),
        _ => panic!("not the app"),
    };
    for event in [
        Event::Text("a{}".into()),
        key(Key::Enter),
        Event::Text("bc{}".into()),
        key(Key::ArrowLeft),
        key(Key::ArrowUp),
        Event::Text("X".into()),
    ] {
        run.send(event);
        run.frame();
    }
    assert_eq!(css(&run), "a{}X\nbc{}");
    run.send(key(Key::ArrowDown));
    run.frame();
    run.send(key(Key::End));
    run.frame();
    run.send(Event::Paste("\r\nd{}\r\n".into()));
    run.frame();
    assert_eq!(css(&run), "a{}X\nbc{}\nd{}\n");
}

/// Card order and positions equal the reference boxes of the
/// `workspace-*` scenes.
#[test]
fn golden_card_positions() {
    let mut run = Run::new("workspace-glass-dark-100", 1.0);
    run.settle();
    let root = [
        "Smurfs",
        "main",
        "bro\u{27}s account",
        "alt",
        "trading",
        "faceit",
        "ranked grind",
        "chill",
        "lan party",
        "streams",
        "esea",
        "tournaments",
        "family",
        "practice",
    ];
    assert_eq!(only_cards(run.boxes()), cards(&root, 84.0));
    run.click(pos2(95.0, 152.0));
    let smurfs = ["Back", "smurf 1", "smurf 2", "smurf 3", "smurf 4"];
    assert_eq!(only_cards(run.boxes()), cards(&smurfs, 84.0));
    run.click(pos2(95.0, 152.0));
    run.click(pos2(639.0, 60.0));
    run.send(Event::Text("r".into()));
    run.frame();
    run.send(Event::Text("a".into()));
    run.frame();
    run.settle();
    let search = ["trading", "ranked grind", "practice"];
    assert_eq!(only_cards(run.boxes()), cards(&search, 84.0));

    let mut run = Run::new("workspace-sections-glass-dark-100", 1.0);
    run.settle();
    let boxes = run.boxes();
    let headers: Vec<Box4> = boxes.iter().filter(|b| b.4 != 136.0).cloned().collect();
    assert_eq!(
        headers,
        vec![
            ("Smurfs".to_string(), 45.0, 84.0, 789.0, 24.0),
            ("Root".to_string(), 45.0, 274.0, 789.0, 23.0),
        ]
    );
    let all = only_cards(boxes);
    assert_eq!(
        all[..3],
        cards(&["Ranked", "smurf 1", "smurf 2"], 116.0)[..]
    );
    let root13 = &root[1..];
    assert_eq!(all[3..], cards(root13, 305.0)[..]);
    // The scrollable height is 567.
    assert_eq!(run.workspace().content_height, 567.0);

    let mut run = Run::new("workspace-deep-glass-dark-100", 1.0);
    run.settle();
    run.click(pos2(95.0, 152.0));
    run.click(pos2(205.0, 152.0));
    run.click(pos2(205.0, 152.0));
    assert_eq!(only_cards(run.boxes()), cards(&["Back", "smurf 4"], 84.0));

    let mut run = Run::new("workspace-huge-100", 1.0);
    run.settle();
    assert_eq!(run.workspace().content_height, 4288.0);
    run.send(Event::PointerMoved(pos2(430.0, 260.0)));
    run.send(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, -1942.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: Modifiers::NONE,
    });
    run.frame();
    run.settle();
    assert_eq!(run.workspace().scroll(), 1942.0);
    let boxes = only_cards(run.boxes());
    assert_eq!(boxes.len(), 200);
    assert_eq!(
        boxes[0],
        ("account 1".to_string(), 45.0, -1858.0, 100.0, 136.0)
    );
    assert_eq!(boxes[7].2, -1712.0);
}

/// The "Settings saved" toast of an inactivity timeout edit, as in the
/// `pin-setup` shot: hovering it pauses its countdown, leaving resumes
/// it, its cross removes it.
#[test]
fn toast_pauses_on_hover_and_closes_on_its_cross() {
    // Frames on the wall clock, like a scene's `wait`.
    fn wait(run: &mut Run, ms: u64) {
        let until = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < until {
            run.frame();
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    let mut run = Run::new("settings-glass-dark-100", 1.0);
    run.settle();
    wait(&mut run, 600);
    // The steps and waits of `settings-glass-dark-100`.
    run.click(pos2(87.0, 18.0));
    wait(&mut run, 700);
    run.click(pos2(93.0, 176.0));
    run.click(pos2(350.0, 192.0));
    run.send(Event::Text("45".into()));
    run.frame();
    // The PIN toggle blurs the field, which commits it, and the save
    // toast follows the 220 ms save debounce.
    run.click(pos2(766.0, 167.0));
    wait(&mut run, 400);
    let toasts = |run: &Run| match &run.screen {
        Screen::App(app) => app.toasts().toasts().to_vec(),
        _ => panic!("not the app"),
    };
    let shown = toasts(&run);
    assert_eq!(shown.len(), 1);
    assert!(shown[0].deadline().is_some());
    // The reference draws the toast at 694..850 x 386..424.
    run.send(Event::PointerMoved(pos2(760.0, 405.0)));
    run.frame();
    run.frame();
    assert!(toasts(&run)[0].hovered());
    assert_eq!(toasts(&run)[0].deadline(), None);
    run.send(Event::PointerMoved(pos2(430.0, 5.0)));
    run.frame();
    run.frame();
    assert!(toasts(&run)[0].deadline().is_some());
    run.click(pos2(821.0, 405.0));
    assert!(toasts(&run).is_empty());
}

/// `streamer-*` scenes: the mock reports OBS running, the overlay
/// covers the app and swallows its input; "Disable" hides it for the
/// session, "Disable permanently" also saves `streamerMode: "off"`.
#[test]
fn streamer_mode_covers_the_app_until_disabled() {
    use accshift_app::settings::StreamerMode;
    let state = |run: &Run| match &run.screen {
        Screen::App(app) => {
            let b = app.booted().unwrap();
            (
                b.streamer.active(),
                b.stores.settings().streamer_mode,
                b.nav.show_settings,
            )
        }
        _ => panic!("not the app"),
    };
    let wait_active = |run: &mut Run| {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !state(run).0 {
            assert!(Instant::now() < deadline, "streamer mode never came up");
            run.frame();
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    let mut run = Run::new("streamer-glass-dark-100", 1.0);
    run.settle();
    wait_active(&mut run);
    // The settings gear under the overlay gets nothing.
    run.click(pos2(87.0, 18.0));
    assert_eq!(state(&run), (true, StreamerMode::Auto, false));
    run.click(pos2(351.0, 268.0));
    assert_eq!(state(&run), (false, StreamerMode::Auto, false));

    let mut run = Run::new("streamer-glass-dark-100", 1.0);
    run.settle();
    wait_active(&mut run);
    run.click(pos2(472.0, 268.0));
    assert_eq!(state(&run), (false, StreamerMode::Off, false));
}

/// The account menu of the `menu-*` scenes: Escape and a press outside
/// close it, hovering "Copy..." opens its submenu at once, and an entry
/// there copies with a toast (the demo usernames are empty, so SteamID64).
#[test]
fn context_menu_opens_closes_and_copies() {
    use crate::ui::components::context_menu;
    let open = |run: &Run| match &run.screen {
        Screen::App(app) => app.dialogs().context_menu().is_some(),
        _ => panic!("not the app"),
    };
    let mut run = Run::new("menu-glass-dark-100", 1.0);
    run.settle();
    run.rclick(pos2(205.0, 152.0));
    assert!(open(&run));
    run.key(Key::Escape);
    assert!(!open(&run));
    run.rclick(pos2(205.0, 152.0));
    assert!(open(&run));
    // The reference draws the menu at 205..425 x 111..436.
    assert!(context_menu::contains(&run.ctx, pos2(300.0, 300.0)));
    run.click(pos2(845.0, 425.0));
    assert!(!open(&run));
    // "Copy..." at 210..420 x 254..284, its submenu right of the menu.
    run.rclick(pos2(205.0, 152.0));
    run.send(Event::PointerMoved(pos2(260.0, 269.0)));
    run.frame();
    run.frame();
    assert!(context_menu::contains(&run.ctx, pos2(500.0, 270.0)));
    run.send(Event::PointerMoved(pos2(500.0, 305.0)));
    run.frame();
    let mut copied = Vec::new();
    for pressed in [true, false] {
        run.send(Event::PointerButton {
            pos: pos2(500.0, 305.0),
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        for _ in 0..2 {
            let out = run.frame();
            copied.extend(
                out.platform_output
                    .commands
                    .into_iter()
                    .filter_map(|c| match c {
                        egui::OutputCommand::CopyText(text) => Some(text),
                        _ => None,
                    }),
            );
        }
    }
    assert_eq!(copied, ["76561198000000001"]);
    assert!(!open(&run));
    let Screen::App(app) = &run.screen else {
        panic!("not the app")
    };
    assert_eq!(app.toasts().toasts().len(), 1);
    // The same entry by keyboard: ArrowRight opens "Copy..." and focuses
    // its first entry.
    run.rclick(pos2(205.0, 152.0));
    let mut copied = Vec::new();
    for key in [Key::ArrowDown; 4]
        .into_iter()
        .chain([Key::ArrowRight, Key::ArrowDown, Key::Enter])
    {
        for pressed in [true, false] {
            run.send(Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Modifiers::NONE,
            });
            let out = run.frame();
            copied.extend(
                out.platform_output
                    .commands
                    .into_iter()
                    .filter_map(|c| match c {
                        egui::OutputCommand::CopyText(text) => Some(text),
                        _ => None,
                    }),
            );
        }
    }
    assert_eq!(copied, ["76561198000000001"]);
    assert!(!open(&run));
}

/// The input and confirm dialogs of the `dialog-*` scenes, driven like a
/// user: Escape and the overlay cancel, Tab cycles inside, Enter submits
/// the new folder, and the delete confirm opens on Cancel.
#[test]
fn dialogs_cancel_cycle_and_submit() {
    use crate::ui::components::dialog;
    use egui::Id;

    let app = |run: &Run| match &run.screen {
        Screen::App(app) => (
            app.dialogs().input_dialog().map(|d| d.value().to_string()),
            app.dialogs().confirm_dialog().is_some(),
            app.dialog_mounts(),
        ),
        _ => panic!("not the app"),
    };
    let focused = |run: &Run| run.ctx.memory(|m| m.focused());
    let labels = |run: &Run| -> Vec<String> { run.boxes().into_iter().map(|b| b.0).collect() };
    let tab = |run: &mut Run, shift: bool| {
        for pressed in [true, false] {
            run.send(Event::Key {
                key: Key::Tab,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: if shift {
                    Modifiers::SHIFT
                } else {
                    Modifiers::NONE
                },
            });
            run.frame();
        }
    };
    // The scene's "New folder": the empty grid's menu, its only entry.
    let new_folder = |run: &mut Run| {
        run.rclick(pos2(430.0, 400.0));
        run.click(pos2(474.0, 416.0));
    };

    let mut run = Run::new("dialog-light-100", 1.0);
    run.settle();
    new_folder(&mut run);
    let (value, _, (Some(mount), _)) = app(&run) else {
        panic!("no input dialog")
    };
    assert_eq!(value.as_deref(), Some(""));
    let ids = Id::unique(dialog::INPUT_ID).with(mount);
    assert_eq!(focused(&run), Some(ids.with("input")));
    run.send(Event::Text("Ranked".into()));
    run.frame();
    assert_eq!(app(&run).0.as_deref(), Some("Ranked"));
    // The field, Cancel, OK, and around.
    tab(&mut run, false);
    assert_eq!(focused(&run), Some(ids.with("cancel")));
    tab(&mut run, false);
    assert_eq!(focused(&run), Some(ids.with("ok")));
    tab(&mut run, false);
    assert_eq!(focused(&run), Some(ids.with("input")));
    tab(&mut run, true);
    assert_eq!(focused(&run), Some(ids.with("ok")));
    run.key(Key::Escape);
    assert_eq!(app(&run).0, None);
    assert!(!labels(&run).iter().any(|l| l == "Ranked"));

    // A press on the overlay, outside the 290..570 x 146..294 box.
    new_folder(&mut run);
    assert!(app(&run).0.is_some());
    run.click(pos2(40.0, 120.0));
    assert_eq!(app(&run).0, None);

    new_folder(&mut run);
    run.send(Event::Text("Ranked".into()));
    run.frame();
    run.key(Key::Enter);
    run.settle();
    assert_eq!(app(&run).0, None);
    assert!(labels(&run).iter().any(|l| l == "Ranked"));

    // "Delete folder", the folder menu's last entry, opens on Cancel:
    // Enter there keeps the folder, Tab then Enter deletes it.
    let delete = |run: &mut Run| {
        run.rclick(pos2(95.0, 152.0));
        run.key(Key::End);
        run.key(Key::Enter);
        run.frame();
    };
    let smurfs = |run: &Run| labels(run).iter().any(|l| l == "Smurfs");
    assert!(smurfs(&run));
    delete(&mut run);
    let (_, true, (_, Some(mount))) = app(&run) else {
        panic!("no confirm dialog")
    };
    let ids = Id::unique(dialog::CONFIRM_ID).with(mount);
    assert_eq!(focused(&run), Some(ids.with("cancel")));
    run.key(Key::Enter);
    run.settle();
    assert!(!app(&run).1);
    assert!(smurfs(&run));
    delete(&mut run);
    tab(&mut run, false);
    let mount = app(&run).2 .1.unwrap();
    let ids = Id::unique(dialog::CONFIRM_ID).with(mount);
    assert_eq!(focused(&run), Some(ids.with("confirm")));
    run.key(Key::Enter);
    run.settle();
    assert!(!app(&run).1);
    assert!(!smurfs(&run));
}

/// "Forget" on the `main` card, confirmed with its button: the task runs
/// on the pool and its failure becomes a toast. The mock refuses
/// `platform_forget_account`, so the card stays (the mock's message
/// starts with `Error: `; the real backend rejects with the bare
/// message).
#[test]
fn forget_confirm_runs_the_task() {
    use accshift_app::text::Text;
    let labels = |run: &Run| -> Vec<String> { run.boxes().into_iter().map(|b| b.0).collect() };
    let state = |run: &Run| match &run.screen {
        Screen::App(app) => (
            app.dialogs().confirm_dialog().is_some(),
            app.toasts()
                .toasts()
                .iter()
                .map(|t| t.text.clone())
                .collect::<Vec<_>>(),
        ),
        _ => panic!("not the app"),
    };
    let mut run = Run::new("dialog-light-100", 1.0);
    run.settle();
    // The scene's "Forget" entry, then the dialog's red button at the
    // right end of its actions row (250..610 x 242..274).
    run.rclick(pos2(205.0, 152.0));
    run.click(pos2(240.0, 377.0));
    assert!(state(&run).0);
    run.click(pos2(575.0, 258.0));
    let deadline = Instant::now() + Duration::from_secs(5);
    while state(&run).1.is_empty() {
        assert!(Instant::now() < deadline, "the task never landed");
        run.frame();
        std::thread::sleep(Duration::from_millis(10));
    }
    let (open, toasts) = state(&run);
    assert!(!open);
    assert_eq!(
        toasts,
        [Text::raw(
            "mock mode: platform_forget_account is not available"
        )]
    );
    assert!(labels(&run).iter().any(|l| l == "main"));

    // What a successful forget lands: the card leaves the list.
    let Screen::App(app) = &mut run.screen else {
        panic!("not the app")
    };
    let b = app.booted_mut().unwrap();
    b.accounts
        .remove_account(&mut b.stores, &b.nav.active_tab, "76561198000000001");
    run.frame();
    run.settle();
    let left = labels(&run);
    assert!(!left.iter().any(|l| l == "main"));
    assert!(left.iter().any(|l| l == "alt"));
}

/// The streamer mode row of the privacy tab answers the pointer: the
/// telemetry card under it once took its id, and its hit box with it.
#[test]
fn privacy_toggle_keeps_its_hit_box() {
    let mut run = Run::new("settings-glass-dark-100", 1.0);
    run.settle();
    run.click(pos2(87.0, 18.0));
    run.click(pos2(93.0, 176.0));
    run.send(Event::PointerMoved(pos2(448.0, 267.0)));
    run.frame();
    run.frame();
    let hovered = run.ctx.viewport(|vp| {
        vp.interact_widgets.hovered.iter().any(|id| {
            vp.prev_pass
                .widgets
                .get(*id)
                .is_some_and(|w| w.interact_rect.contains(pos2(448.0, 267.0)))
        })
    });
    assert!(hovered);
}

/// The huge scenario scrolled down and back with the CPU renderer:
/// median and p99 of the whole frame (ui, tessellation, raster) over 300
/// frames. `cargo test --release --lib huge_scroll -- --ignored
/// --nocapture`.
#[cfg(feature = "software")]
#[test]
#[ignore]
fn huge_scroll_frame_time() {
    use egui_software_backend::{BufferMutRef, ColorFieldOrder, EguiSoftwareRender};
    for scale in [1.0f32, 1.5] {
        let mut run = Run::new("workspace-huge-100", scale);
        run.settle();
        let (w, h) = ((860.0 * scale) as usize, (440.0 * scale) as usize);
        let mut pixels = vec![[0u8; 4]; w * h];
        let mut render = EguiSoftwareRender::new(ColorFieldOrder::Bgra)
            .with_glyph_coverage(crate::ui::text::coverage_tables());
        let mut times = Vec::new();
        let mut textures = egui::TexturesDelta::default();
        for i in 0..330 {
            // 24 px a frame down, then back up.
            let dy = if (i / 165) % 2 == 0 { 24.0 } else { -24.0 };
            run.send(Event::PointerMoved(pos2(430.0, 260.0)));
            run.send(Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, -dy),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: Modifiers::NONE,
            });
            let started = Instant::now();
            let input = run.input();
            let screen = &mut run.screen;
            let mut out = run.ctx.run_ui(input, |ui| screen.ui(ui));
            textures.append(std::mem::take(&mut out.textures_delta));
            let prims = run.ctx.tessellate(out.shapes, out.pixels_per_point);
            let mut buffer = BufferMutRef::new(&mut pixels, w, h);
            render.render(&mut buffer, &prims, &textures, out.pixels_per_point);
            textures.clear();
            if i >= 30 {
                times.push(started.elapsed().as_secs_f64() * 1000.0);
            }
        }
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = times[times.len() / 2];
        let p99 = times[(times.len() * 99).div_ceil(100) - 1];
        println!(
            "huge scroll @{scale}: {} frames, median {median:.2} ms, p99 {p99:.2} ms, max {:.2} ms",
            times.len(),
            times[times.len() - 1]
        );
    }
}

/// The `list-*` scenes: the toggle shows the rows where the reference
/// put them, a click previews, a right click opens the account menu, a
/// folder row goes in (dropping the preview) and the back row comes out.
#[test]
fn list_view_previews_opens_menus_and_navigates() {
    let open = |run: &Run| match &run.screen {
        Screen::App(app) => app.dialogs().context_menu().is_some(),
        _ => panic!("not the app"),
    };
    let labels = |run: &Run| -> Vec<String> {
        run.workspace()
            .list
            .layout
            .iter()
            .map(|(l, _)| l.clone())
            .collect()
    };
    let mut run = Run::new("list-glass-dark-100", 1.0);
    run.settle();
    run.click(pos2(816.0, 60.0));
    assert_eq!(
        labels(&run)[..4],
        ["Smurfs", "main", "bro\u{27}s account", "alt"]
    );
    // The list box at (16, 84), 818 by 340: rows of 599 by 46 from
    // (20, 88), 48 apart.
    let rows: Vec<egui::Rect> = run
        .workspace()
        .list
        .layout
        .iter()
        .map(|(_, r)| *r)
        .collect();
    assert_eq!(
        rows[1],
        egui::Rect::from_min_size(pos2(20.0, 136.0), vec2(599.0, 46.0))
    );
    assert_eq!(rows[2].top(), 184.0);

    run.click(pos2(300.0, 159.0));
    assert_eq!(
        run.workspace().list.selected.as_deref(),
        Some("76561198000000001")
    );
    run.rclick(pos2(300.0, 207.0));
    assert!(open(&run));
    run.key(Key::Escape);
    assert!(!open(&run));

    run.click(pos2(300.0, 111.0));
    let inside = labels(&run);
    assert_eq!(inside[0], "Back");
    assert!(inside.iter().any(|l| l == "smurf 1"));
    assert_eq!(run.workspace().list.selected, None);
    run.click(pos2(300.0, 111.0));
    assert_eq!(labels(&run)[0], "Smurfs");
}

/// The app's binding table and the command palette: Ctrl+K opens it on
/// its focused field, typing filters, Enter runs, Escape and a second
/// Ctrl+K close, the arrows move the highlight; the tab, view and
/// search shortcuts reach the app.
#[test]
fn card_focus_follows_the_arrows() {
    fn app(run: &Run) -> &crate::app::AppModel {
        match &run.screen {
            Screen::App(app) => app,
            _ => panic!("not the app"),
        }
    }
    let focus = |run: &Run| app(run).booted().unwrap().workspace.kb_focus.clone();
    let label = |run: &Run| {
        let b = app(run).booted().unwrap();
        let id = b.workspace.kb_focus.clone()?;
        b.accounts.view.items().iter().find_map(|item| match item {
            accshift_app::display::DisplayItem::Folder(f) if f.folder.id == id => {
                Some(f.folder.name.clone())
            }
            accshift_app::display::DisplayItem::Account(a) if a.account.id == id => {
                Some(a.name.clone())
            }
            _ => None,
        })
    };
    let mut run = Run::new("workspace-glass-dark-100", 1.0);
    run.settle();
    assert_eq!(focus(&run), None);
    // The first arrow lands on the first card, the next ones move by
    // one or by a row of seven (the folder takes the first slot).
    run.key(Key::ArrowRight);
    assert_eq!(label(&run).as_deref(), Some("Smurfs"));
    run.key(Key::ArrowRight);
    assert_eq!(label(&run).as_deref(), Some("main"));
    run.key(Key::ArrowDown);
    assert_eq!(label(&run).as_deref(), Some("lan party"));
    // Past the last row it stays.
    run.key(Key::ArrowDown);
    assert_eq!(label(&run).as_deref(), Some("lan party"));
    run.key(Key::ArrowUp);
    assert_eq!(label(&run).as_deref(), Some("main"));
    // Escape clears, then a pointer press does.
    run.key(Key::Escape);
    assert_eq!(focus(&run), None);
    run.key(Key::ArrowRight);
    assert!(focus(&run).is_some());
    run.click(pos2(840.0, 420.0));
    assert_eq!(focus(&run), None);
    // Delete opens the menu of the focused card at its centre, F2 the
    // rename dialog of a folder.
    run.key(Key::ArrowRight);
    run.key(Key::Delete);
    assert!(
        app(&run).dialogs().context_menu().is_some(),
        "Delete opens the menu"
    );
    run.key(Key::Escape);
    assert!(app(&run).dialogs().context_menu().is_none());
    assert_eq!(
        label(&run).as_deref(),
        Some("Smurfs"),
        "the menu keeps the focus"
    );
    run.key(Key::F2);
    assert!(
        app(&run).dialogs().input_dialog().is_some(),
        "F2 renames the folder"
    );
    run.key(Key::Escape);
    // Enter opens the focused folder, and the new folder clears it.
    run.key(Key::Enter);
    let b = app(&run).booted().unwrap();
    assert!(b.accounts.navigation.current_folder_id().is_some());
    assert_eq!(focus(&run), None);
}

#[test]
fn keyboard_shortcuts_and_the_palette() {
    use crate::ui::components::command_palette;
    use accshift_app::stores::ViewMode;

    let chord = |run: &mut Run, key: Key, modifiers: Modifiers| {
        for pressed in [true, false] {
            run.send(Event::Key {
                key,
                physical_key: Some(key),
                pressed,
                repeat: false,
                modifiers,
            });
            run.frame();
        }
    };
    fn app(run: &Run) -> &crate::app::AppModel {
        match &run.screen {
            Screen::App(app) => app,
            _ => panic!("not the app"),
        }
    }
    let palette = |run: &Run| app(run).palette().map(|(m, q, a)| (m, q.to_string(), a));
    let nav = |run: &Run| app(run).booted().unwrap().nav.clone();
    let view_mode = |run: &Run| app(run).booted().unwrap().stores.view_mode();
    let ctrl = Modifiers::CTRL;
    let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;

    let mut run = Run::new("workspace-glass-dark-100", 1.0);
    run.settle();
    assert_eq!(palette(&run), None);
    chord(&mut run, Key::K, ctrl);
    let (mount, query, active) = palette(&run).expect("Ctrl+K opens the palette");
    assert_eq!((query.as_str(), active), ("", 0));
    let focused = run.ctx.memory(|m| m.focused());
    assert_eq!(focused, Some(command_palette::input_id(mount)));
    // The arrows wrap; End and Home go to the ends of an idle list.
    run.key(Key::ArrowUp);
    let last = palette(&run).unwrap().2;
    assert!(last > 5, "the idle list holds accounts, actions and tabs");
    run.key(Key::ArrowDown);
    assert_eq!(palette(&run).unwrap().2, 0);
    run.key(Key::End);
    assert_eq!(palette(&run).unwrap().2, last);
    run.key(Key::Home);
    assert_eq!(palette(&run).unwrap().2, 0);
    // A second Ctrl+K closes, Escape too.
    chord(&mut run, Key::K, ctrl);
    assert_eq!(palette(&run), None);
    chord(&mut run, Key::K, ctrl);
    assert!(palette(&run).is_some_and(|p| p.0 > mount), "a fresh mount");
    run.key(Key::Escape);
    assert_eq!(palette(&run), None);
    // Typing filters and resets the highlight; Enter runs the first
    // match, here the settings, and closes.
    chord(&mut run, Key::K, ctrl);
    run.key(Key::ArrowDown);
    run.send(Event::Text("settings".into()));
    run.frame();
    assert_eq!(
        palette(&run).map(|p| (p.1, p.2)),
        Some(("settings".into(), 0))
    );
    run.key(Key::Enter);
    assert_eq!(palette(&run), None);
    assert!(nav(&run).show_settings);
    chord(&mut run, Key::Comma, ctrl);
    assert!(!nav(&run).show_settings);
    // A press on the overlay outside the box closes.
    chord(&mut run, Key::K, ctrl);
    run.click(pos2(30.0, 400.0));
    assert_eq!(palette(&run), None);

    // Tabs: Ctrl+Tab cycles steam, riot, roblox; Ctrl+3 jumps.
    assert_eq!(nav(&run).active_tab, "steam");
    chord(&mut run, Key::Tab, ctrl);
    assert_eq!(nav(&run).active_tab, "riot");
    chord(&mut run, Key::Tab, ctrl_shift);
    assert_eq!(nav(&run).active_tab, "steam");
    chord(&mut run, Key::Num3, ctrl);
    assert_eq!(nav(&run).active_tab, "roblox");
    chord(&mut run, Key::Num1, ctrl);
    assert_eq!(nav(&run).active_tab, "steam");
    run.settle();

    // Ctrl+Shift+L flips the view.
    assert_eq!(view_mode(&run), ViewMode::Grid);
    chord(&mut run, Key::L, ctrl_shift);
    assert_eq!(view_mode(&run), ViewMode::List);
    chord(&mut run, Key::L, ctrl_shift);
    assert_eq!(view_mode(&run), ViewMode::Grid);

    // Ctrl+F focuses the search; Escape clears and blurs it.
    let search = egui::Id::unique(crate::app::keys::SEARCH_ID);
    chord(&mut run, Key::F, ctrl);
    assert!(run.ctx.memory(|m| m.has_focus(search)));
    run.send(Event::Text("zz".into()));
    run.frame();
    let query = |run: &Run| {
        app(run)
            .booted()
            .unwrap()
            .accounts
            .navigation
            .search_query()
            .to_string()
    };
    assert_eq!(query(&run), "zz");
    run.key(Key::Escape);
    assert_eq!(query(&run), "");
    assert!(!run.ctx.memory(|m| m.has_focus(search)));
}

/// Every return to the workspace remounts its cards (200 here) or list
/// rows under fresh ids: the temp state the old mounts wrote goes with
/// them.
#[test]
fn remounts_drop_the_state_of_the_old_mounts() {
    for scene in ["workspace-huge-100", "list-glass-dark-100"] {
        let mut run = Run::new(scene, 1.0);
        run.settle();
        let remount = |run: &mut Run| {
            // A frame without the workspace unmounts every card.
            let input = run.input();
            let mut out = run.ctx.run_ui(input, |_| {});
            out.textures_delta.clear();
            run.frame();
            run.frame();
        };
        remount(&mut run);
        let before = run.ctx.data(|d| d.len());
        for _ in 0..10 {
            remount(&mut run);
        }
        let after = run.ctx.data(|d| d.len());
        assert!(
            after <= before + 16,
            "{scene}: temp data grew from {before} to {after} entries over 10 remounts"
        );
    }
}
