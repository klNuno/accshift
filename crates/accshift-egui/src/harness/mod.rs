//! Scene runner: `accshift-egui --scene <scene.json>... --out <dir>`.
//!
//! Replays the scene files shared with `parity/ref.mjs` (format in
//! `parity/README.md`). Each scene resizes the hidden window to its viewport
//! and scale, builds its screen, waits until the screen is ready (every
//! avatar uploaded), then injects its steps as egui raw input on the wall
//! clock, like the reference runner does over CDP. A `shot` reads back the
//! frame the renderer presented and writes `<out>/<id>/<shot>.png`.
//!
//! Failures (a scene the egui side cannot render yet, a shot of the wrong
//! size, a write error) are printed and counted; the process exits non-zero
//! when any happened ([`failures`]).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use egui::{
    pos2, vec2, Event, Key, Modifiers, MouseWheelUnit, PointerButton, Pos2, RawInput, TouchPhase,
    ViewportCommand, ViewportId,
};

static FAILURES: AtomicUsize = AtomicUsize::new(0);

/// Failures recorded by every run in this process.
pub fn failures() -> usize {
    FAILURES.load(Ordering::Relaxed)
}

fn fail(msg: String) {
    eprintln!("scene: {msg}");
    FAILURES.fetch_add(1, Ordering::Relaxed);
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub width: f32,
    pub height: f32,
    pub scale: f32,
}

impl Viewport {
    /// The frame size in device pixels.
    pub fn pixels(&self) -> [usize; 2] {
        [
            (self.width * self.scale).round() as usize,
            (self.height * self.scale).round() as usize,
        ]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Wait(f64),
    Move(Pos2),
    Click(Pos2),
    DblClick(Pos2),
    RClick(Pos2),
    Key(String),
    Text(String),
    Scroll(Pos2, f32),
    Shot(String),
}

#[derive(Clone, Debug)]
pub struct Scene {
    /// The scene file.
    pub path: PathBuf,
    pub id: String,
    pub scenario: String,
    pub os: Option<String>,
    pub viewport: Viewport,
    pub theme: Option<String>,
    pub locale: Option<String>,
    pub seed: Option<serde_json::Value>,
    /// `streaming`: the mock reports streaming software running.
    pub streaming: bool,
    /// `cs2`: the CS2 bridge is enabled and answers with these rows.
    pub cs2: Option<Vec<serde_json::Value>>,
    /// `onboarding`: the first-run onboarding was never completed.
    pub onboarding: bool,
    /// The `html` fragment (resolved next to the scene file) and its text.
    pub html: Option<(PathBuf, String)>,
    pub steps: Vec<Step>,
}

impl Scene {
    /// `parity/data`, next to the `parity/scenes` folder the scene sits in.
    pub fn data_dir(&self) -> PathBuf {
        let scenes = self.path.parent().unwrap_or(Path::new("."));
        scenes.parent().unwrap_or(Path::new(".")).join("data")
    }

    /// The fragment's file stem: which test screen draws it.
    pub fn fragment_name(&self) -> Option<String> {
        let (path, _) = self.html.as_ref()?;
        Some(path.file_stem()?.to_string_lossy().into_owned())
    }
}

fn point(v: &serde_json::Value) -> Option<Pos2> {
    let a = v.as_array()?;
    Some(pos2(
        a.first()?.as_f64()? as f32,
        a.get(1)?.as_f64()? as f32,
    ))
}

fn parse_step(v: &serde_json::Value) -> Result<Step, String> {
    let o = v.as_object().ok_or("step is not an object")?;
    let bad = || format!("bad step {v}");
    if let Some(ms) = o.get("wait") {
        return Ok(Step::Wait(ms.as_f64().ok_or_else(bad)? / 1000.0));
    }
    if let Some(p) = o.get("move") {
        return Ok(Step::Move(point(p).ok_or_else(bad)?));
    }
    if let Some(p) = o.get("click") {
        return Ok(Step::Click(point(p).ok_or_else(bad)?));
    }
    if let Some(p) = o.get("dblclick") {
        return Ok(Step::DblClick(point(p).ok_or_else(bad)?));
    }
    if let Some(p) = o.get("rclick") {
        return Ok(Step::RClick(point(p).ok_or_else(bad)?));
    }
    if let Some(k) = o.get("key") {
        return Ok(Step::Key(k.as_str().ok_or_else(bad)?.to_owned()));
    }
    if let Some(t) = o.get("text") {
        return Ok(Step::Text(t.as_str().ok_or_else(bad)?.to_owned()));
    }
    if let Some(s) = o.get("scroll") {
        let a = s.as_array().ok_or_else(bad)?;
        let f = |i: usize| a.get(i).and_then(serde_json::Value::as_f64).ok_or_else(bad);
        return Ok(Step::Scroll(pos2(f(0)? as f32, f(1)? as f32), f(2)? as f32));
    }
    if let Some(s) = o.get("shot") {
        return Ok(Step::Shot(s.as_str().ok_or_else(bad)?.to_owned()));
    }
    // `ref.mjs` dumps the reference DOM there; nothing to do on this side.
    if o.contains_key("dom") {
        return Ok(Step::Wait(0.0));
    }
    Err(bad())
}

pub fn load(path: &Path) -> Result<Scene, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_owned);
    let id = s("id").unwrap_or_else(|| {
        path.file_stem()
            .map(|x| x.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    let vp = v.get("viewport");
    let num = |k: &str, d: f64| {
        vp.and_then(|o| o.get(k))
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(d) as f32
    };
    let viewport = Viewport {
        width: num("width", 860.0),
        height: num("height", 440.0),
        scale: num("scale", 1.5),
    };
    let html = match s("html") {
        Some(rel) => {
            let file = path.parent().unwrap_or(Path::new(".")).join(rel);
            let body = std::fs::read_to_string(&file)
                .map_err(|e| format!("{}: html {}: {e}", path.display(), file.display()))?;
            Some((file, body))
        }
        None => None,
    };
    let steps = v
        .get("steps")
        .and_then(|x| x.as_array())
        .map(|a| a.iter().map(parse_step).collect::<Result<Vec<_>, _>>())
        .transpose()
        .map_err(|e| format!("{}: {e}", path.display()))?
        .unwrap_or_default();
    Ok(Scene {
        path: path.to_path_buf(),
        id,
        scenario: s("scenario").unwrap_or_else(|| "demo".into()),
        os: s("os"),
        viewport,
        theme: s("theme"),
        locale: s("locale"),
        seed: v.get("seed").cloned(),
        streaming: v
            .get("streaming")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        cs2: v.get("cs2").and_then(|x| x.as_array()).cloned(),
        onboarding: v
            .get("onboarding")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        html,
        steps,
    })
}

/// `Ctrl+Shift+K`, `Enter`, `a`: the key event and the text it types, if any.
fn key_events(spec: &str) -> Result<Vec<Event>, String> {
    let mut parts: Vec<&str> = spec.split('+').collect();
    let name = parts.pop().ok_or_else(|| format!("empty key {spec}"))?;
    let mut modifiers = Modifiers::NONE;
    for m in parts {
        match m {
            "Ctrl" => {
                modifiers.ctrl = true;
                modifiers.command = true;
            }
            "Shift" => modifiers.shift = true,
            "Alt" => modifiers.alt = true,
            "Meta" => modifiers.mac_cmd = true,
            other => return Err(format!("unknown modifier {other} in {spec}")),
        }
    }
    let key = Key::from_name(name)
        .or_else(|| Key::from_name(&name.to_uppercase()))
        .ok_or_else(|| format!("unknown key {name}"))?;
    let ev = |pressed| Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers,
    };
    let mut out = vec![ev(true)];
    let typed = !(modifiers.ctrl || modifiers.alt || modifiers.mac_cmd);
    if typed {
        if name.chars().count() == 1 {
            let text = if modifiers.shift {
                name.to_uppercase()
            } else {
                name.to_owned()
            };
            out.push(Event::Text(text));
        } else if name == "Space" {
            out.push(Event::Text(" ".into()));
        }
    }
    out.push(ev(false));
    Ok(out)
}

enum Op {
    Wait(f64),
    Event(Event),
    Shot(String),
}

fn expand(steps: &[Step]) -> Result<VecDeque<Op>, String> {
    let mut ops = VecDeque::new();
    let mut pointer = pos2(0.0, 0.0);
    let button = |pos, button, pressed| {
        Op::Event(Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: Modifiers::NONE,
        })
    };
    // Same rhythm as ref.mjs: 40 ms between press and release.
    let press = |ops: &mut VecDeque<Op>, p: Pos2, b: PointerButton| {
        ops.push_back(button(p, b, true));
        ops.push_back(Op::Wait(0.04));
        ops.push_back(button(p, b, false));
    };
    for step in steps {
        match step {
            Step::Wait(s) => ops.push_back(Op::Wait(*s)),
            Step::Move(p) => {
                pointer = *p;
                ops.push_back(Op::Event(Event::PointerMoved(*p)));
            }
            Step::Click(p) | Step::DblClick(p) | Step::RClick(p) => {
                pointer = *p;
                ops.push_back(Op::Event(Event::PointerMoved(*p)));
                let b = if matches!(step, Step::RClick(_)) {
                    PointerButton::Secondary
                } else {
                    PointerButton::Primary
                };
                press(&mut ops, *p, b);
                if matches!(step, Step::DblClick(_)) {
                    press(&mut ops, *p, b);
                }
            }
            Step::Key(spec) => {
                for e in key_events(spec)? {
                    ops.push_back(Op::Event(e));
                }
            }
            Step::Text(text) => {
                for ch in text.chars() {
                    if ch == ' ' {
                        for e in key_events("Space")? {
                            ops.push_back(Op::Event(e));
                        }
                    } else {
                        ops.push_back(Op::Event(Event::Text(ch.to_string())));
                    }
                    ops.push_back(Op::Wait(0.03));
                }
            }
            Step::Scroll(p, dy) => {
                if *p != pointer {
                    pointer = *p;
                    ops.push_back(Op::Event(Event::PointerMoved(*p)));
                }
                // CSS deltaY > 0 scrolls the content up; egui's delta is
                // how the content moves.
                ops.push_back(Op::Event(Event::MouseWheel {
                    unit: MouseWheelUnit::Point,
                    delta: vec2(0.0, -dy),
                    phase: TouchPhase::Move,
                    modifiers: Modifiers::NONE,
                }));
            }
            Step::Shot(name) => ops.push_back(Op::Shot(name.clone())),
        }
    }
    Ok(ops)
}

enum Phase {
    /// Next scene to set up.
    Next,
    /// Window resized and zoom set; waiting for the frame to match.
    Sizing {
        since: f64,
        resent: bool,
    },
    /// The screen exists; waiting for it to be ready, then a few frames.
    Settling {
        frames: u32,
    },
    Running,
    Done,
}

pub struct SceneRun {
    scenes: Vec<Scene>,
    out: PathBuf,
    index: usize,
    phase: Phase,
    ops: VecDeque<Op>,
    wake_at: f64,
    clock: Instant,
    pending_shot: Option<String>,
    build: Option<Scene>,
}

impl SceneRun {
    pub fn new(scenes: Vec<Scene>, out: PathBuf) -> Self {
        Self {
            scenes,
            out,
            index: 0,
            phase: Phase::Next,
            ops: VecDeque::new(),
            wake_at: 0.0,
            clock: Instant::now(),
            pending_shot: None,
            build: None,
        }
    }

    pub fn scene(&self) -> Option<&Scene> {
        self.scenes.get(self.index)
    }

    /// A scene whose screen the shell must build now.
    pub fn take_build(&mut self) -> Option<Scene> {
        self.build.take()
    }

    /// The current scene cannot be drawn: count it and move on.
    pub fn skip(&mut self, why: &str) {
        if let Some(scene) = self.scenes.get(self.index) {
            fail(format!("{}: {why}", scene.id));
        }
        self.index += 1;
        self.phase = Phase::Next;
        self.ops.clear();
    }

    /// Inject this frame's events. `ready` is the current screen's own
    /// readiness (avatars uploaded); nothing runs before it.
    pub fn feed(&mut self, ctx: &egui::Context, raw: &mut RawInput, ready: bool) {
        // The window is never activated; egui must still treat it as focused
        // for text fields to take typed text.
        raw.focused = true;
        ctx.request_repaint();
        let now = self.clock.elapsed().as_secs_f64();
        match self.phase {
            Phase::Done => {}
            Phase::Next => {
                let Some(scene) = self.scenes.get(self.index) else {
                    self.phase = Phase::Done;
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                    return;
                };
                let native = raw
                    .viewports
                    .get(&ViewportId::ROOT)
                    .and_then(|v| v.native_pixels_per_point)
                    .or_else(|| ctx.native_pixels_per_point())
                    .unwrap_or(1.0);
                let vp = scene.viewport;
                ctx.set_zoom_factor(vp.scale / native);
                ctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2(vp.width, vp.height)));
                match expand(&scene.steps) {
                    Ok(ops) => self.ops = ops,
                    Err(e) => {
                        self.skip(&e);
                        return;
                    }
                }
                // A fresh pointer and fresh widget state for every scene.
                raw.events.push(Event::PointerGone);
                ctx.data_mut(|d| d.clear());
                ctx.clear_animations();
                self.phase = Phase::Sizing {
                    since: now,
                    resent: false,
                };
            }
            Phase::Sizing { since, resent } => {
                let Some(scene) = self.scenes.get(self.index) else {
                    return;
                };
                let vp = scene.viewport;
                let rect = ctx.content_rect();
                let zoomed = (ctx.pixels_per_point() - vp.scale).abs() < 1e-4;
                let fits = zoomed
                    && (rect.width() - vp.width).abs() < 0.01
                    && (rect.height() - vp.height).abs() < 0.01;
                if !fits && zoomed && !resent {
                    // The size went out with the old zoom: ask again in points
                    // of the new one.
                    ctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2(vp.width, vp.height)));
                    self.phase = Phase::Sizing {
                        since,
                        resent: true,
                    };
                } else if fits {
                    self.build = Some(scene.clone());
                    self.phase = Phase::Settling { frames: 0 };
                } else if now - since > 5.0 {
                    self.skip(&format!(
                        "window never reached {}x{} at {} (got {:.1}x{:.1} at {})",
                        vp.width,
                        vp.height,
                        vp.scale,
                        rect.width(),
                        rect.height(),
                        ctx.pixels_per_point()
                    ));
                }
            }
            Phase::Settling { frames } => {
                if self.build.is_some() {
                    return;
                }
                if !ready {
                    return;
                }
                // Two frames with everything uploaded, then the clock starts.
                if frames < 2 {
                    self.phase = Phase::Settling { frames: frames + 1 };
                    return;
                }
                self.phase = Phase::Running;
                self.wake_at = now;
                self.run_ops(ctx, raw, now);
            }
            Phase::Running => self.run_ops(ctx, raw, now),
        }
    }

    fn run_ops(&mut self, ctx: &egui::Context, raw: &mut RawInput, now: f64) {
        if self.pending_shot.is_some() {
            return;
        }
        while now >= self.wake_at {
            let Some(op) = self.ops.pop_front() else {
                self.index += 1;
                self.phase = Phase::Next;
                return;
            };
            match op {
                Op::Wait(secs) => {
                    self.wake_at = now + secs;
                    return;
                }
                Op::Event(e) => {
                    let is_press = matches!(e, Event::PointerButton { pressed: true, .. });
                    raw.events.push(e);
                    if is_press {
                        return;
                    }
                }
                Op::Shot(name) => {
                    ctx.send_viewport_cmd(ViewportCommand::Screenshot(Default::default()));
                    self.pending_shot = Some(name);
                    return;
                }
            }
        }
    }

    /// Write the screenshot that arrived this frame, if any.
    pub fn collect(&mut self, ctx: &egui::Context) {
        let Some(name) = self.pending_shot.clone() else {
            return;
        };
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        let Some(image) = image else { return };
        self.pending_shot = None;
        let Some(scene) = self.scenes.get(self.index) else {
            return;
        };
        let dir = self.out.join(&scene.id);
        let path = dir.join(format!("{name}.png"));
        let want = scene.viewport.pixels();
        if image.size != want {
            fail(format!(
                "{}/{name}: frame {}x{}, expected {}x{}",
                scene.id, image.size[0], image.size[1], want[0], want[1]
            ));
        }
        let written = std::fs::create_dir_all(&dir)
            .map_err(|e| e.to_string())
            .and_then(|()| write_png(&path, &image));
        match written {
            Ok(()) => println!(
                "{}/{name}.png {}x{}",
                scene.id, image.size[0], image.size[1]
            ),
            Err(e) => fail(format!("{}: {e}", path.display())),
        }
        crate::bench::mark_with(
            "shot",
            &format!(
                "\"scene\":\"{}\",\"name\":\"{name}\",\"w\":{},\"h\":{}",
                scene.id, image.size[0], image.size[1]
            ),
        );
    }
}

/// RGBA PNG, unpremultiplied like a browser screenshot.
fn write_png(path: &Path, image: &egui::ColorImage) -> Result<(), String> {
    let [w, h] = image.size;
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    let data: Vec<u8> = image
        .pixels
        .iter()
        .flat_map(|p| p.to_srgba_unmultiplied())
        .collect();
    writer.write_image_data(&data).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spike_scene_parses() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("parity/scenes/spike-main.json");
        let scene = load(&path).unwrap();
        assert_eq!(scene.id, "spike-main");
        assert_eq!(scene.viewport.pixels(), [1290, 660]);
        let shots = scene
            .steps
            .iter()
            .filter(|s| matches!(s, Step::Shot(_)))
            .count();
        assert_eq!(shots, 8);
        assert_eq!(scene.steps[0], Step::Wait(0.6));
    }

    #[test]
    fn keys_expand_to_events() {
        let ev = key_events("Ctrl+K").unwrap();
        assert_eq!(ev.len(), 2);
        let ev = key_events("a").unwrap();
        assert!(matches!(&ev[1], Event::Text(t) if t == "a"));
        assert!(key_events("Hyper+K").is_err());
    }
}
