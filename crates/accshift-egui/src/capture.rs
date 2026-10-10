//! Scripted capture run: `accshift-egui --capture <dir>`.
//!
//! The window gets the bench recipe (never activated, off screen, cloaked), a
//! fixed script feeds pointer and text events through `raw_input_hook`, and
//! each shot is read back from the GL framebuffer of the real glow renderer.
//! Shots are written as PAM files (a 7-line header plus raw RGBA), so the
//! binary needs no image encoder; `ffmpeg -i shot.pam shot.png` converts them.
//!
//! The script reproduces the frames of the Tauri recorder's `demo-switch`
//! (rest with hover, armed card, search, after the switch), then the other
//! tabs, a folder and the list view.

use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use egui::{pos2, Event, Modifiers, PointerButton, Pos2, RawInput};

#[derive(Clone, Copy)]
enum Step {
    Wait(f64),
    Move(Pos2),
    Press(Pos2),
    Release(Pos2),
    Text(&'static str),
    Away,
    Shot(&'static str),
    Quit,
}

pub struct Capture {
    dir: PathBuf,
    steps: Vec<Step>,
    next: usize,
    wake_at: f64,
    clock: Instant,
    pending_shot: Option<&'static str>,
    pub done: bool,
}

/// Card centres: 7 columns from x = 45, 110 apart; rows from y = 84, 146 apart.
fn card(col: f32, row: f32) -> Pos2 {
    pos2(45.0 + col * 110.0 + 50.0, 84.0 + row * 146.0 + 68.0)
}

fn click(steps: &mut Vec<Step>, p: Pos2) {
    steps.extend([Step::Move(p), Step::Press(p), Step::Release(p)]);
}

fn script() -> Vec<Step> {
    use Step::*;
    let mut s = vec![Wait(0.6)];
    // switch-0: grid at rest, pointer on "practice".
    s.extend([Move(card(6.0, 1.0)), Wait(0.5), Shot("egui-0-hover")]);
    // switch-2: "bro's account" armed, pointer still on it.
    click(&mut s, card(2.0, 0.0));
    s.extend([Wait(0.6), Shot("egui-2-armed")]);
    // switch-5: query "ra" typed in the search field.
    click(&mut s, pos2(639.0, 60.0));
    s.extend([Text("ra"), Wait(0.6), Shot("egui-5-search")]);
    // switch-9: query cleared, switch to "bro's account" done, pointer on
    // the folder.
    click(&mut s, pos2(741.0, 60.0));
    s.push(Wait(0.4));
    click(&mut s, card(2.0, 0.0));
    s.push(Wait(0.3));
    click(&mut s, card(2.0, 0.0));
    s.extend([
        Wait(1.4),
        Move(card(0.0, 0.0)),
        Wait(0.5),
        Shot("egui-9-switched"),
    ]);
    // Beyond the recorded frames.
    for (name, tab) in [
        ("egui-riot", pos2(447.0, 18.0)),
        ("egui-roblox", pos2(481.0, 18.0)),
    ] {
        click(&mut s, tab);
        s.extend([Away, Wait(0.7), Shot(name)]);
    }
    click(&mut s, pos2(413.0, 18.0));
    s.push(Wait(0.4));
    click(&mut s, card(0.0, 0.0));
    s.extend([Away, Wait(0.7), Shot("egui-folder")]);
    click(&mut s, pos2(35.0, 60.0));
    s.push(Wait(0.3));
    click(&mut s, pos2(816.0, 60.0));
    s.extend([Away, Wait(0.7), Shot("egui-list"), Quit]);
    s
}

impl Capture {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            steps: script(),
            next: 0,
            wake_at: 0.0,
            clock: Instant::now(),
            pending_shot: None,
            done: false,
        }
    }

    /// Inject this frame's scripted events. Nothing runs before `ready`
    /// (every avatar uploaded) or while a screenshot is in flight.
    pub fn feed(&mut self, ctx: &egui::Context, raw: &mut RawInput, ready: bool) {
        // The window is never activated; egui must still treat it as focused
        // for the text field to take the typed query.
        raw.focused = true;
        ctx.request_repaint();
        if !ready || self.pending_shot.is_some() || self.done {
            return;
        }
        let now = self.clock.elapsed().as_secs_f64();
        if self.next == 0 && self.wake_at == 0.0 {
            self.wake_at = now;
        }
        while now >= self.wake_at && self.next < self.steps.len() {
            let step = self.steps[self.next];
            self.next += 1;
            let button = |pos, pressed| Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            match step {
                Step::Wait(secs) => {
                    self.wake_at = now + secs;
                    return;
                }
                Step::Move(p) => raw.events.push(Event::PointerMoved(p)),
                Step::Press(p) => {
                    raw.events.push(button(p, true));
                    // Release on the next frame, like a real click.
                    return;
                }
                Step::Release(p) => raw.events.push(button(p, false)),
                Step::Text(text) => raw.events.push(Event::Text(text.into())),
                Step::Away => raw.events.push(Event::PointerGone),
                Step::Shot(name) => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                    self.pending_shot = Some(name);
                    return;
                }
                Step::Quit => {
                    self.done = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
            }
        }
    }

    /// Write the screenshot that arrived this frame, if any.
    pub fn collect(&mut self, ctx: &egui::Context) {
        let Some(name) = self.pending_shot else {
            return;
        };
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        let Some(image) = image else { return };
        let path = self.dir.join(format!("{name}.pam"));
        if let Err(err) = write_pam(&path, &image) {
            eprintln!("capture: {}: {err}", path.display());
        }
        crate::bench::mark_with(
            "shot",
            &format!(
                "\"name\":\"{name}\",\"w\":{},\"h\":{}",
                image.size[0], image.size[1]
            ),
        );
        self.pending_shot = None;
    }
}

fn write_pam(path: &std::path::Path, image: &egui::ColorImage) -> std::io::Result<()> {
    let [w, h] = image.size;
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    write!(
        out,
        "P7\nWIDTH {w}\nHEIGHT {h}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n"
    )?;
    for px in &image.pixels {
        out.write_all(&px.to_srgba_unmultiplied())?;
    }
    out.flush()
}
