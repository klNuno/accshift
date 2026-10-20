//! Boot marks for `.agents/perf/bench-egui.ps1`: one JSON line per event in
//! `%TEMP%\accshift-egui-bench\<pid>.jsonl`, epoch milliseconds.
//!
//! Active in a `startup-bench` build, or with `ACCSHIFT_EGUI_MARKS=1`.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// The harness greps the exe for this string and refuses to launch a build
/// without it: only a bench build keeps its window off screen.
pub const MARKER: &str = "accshift-egui-startup-bench-build";

static SINK: OnceLock<Option<Mutex<File>>> = OnceLock::new();

pub fn enabled() -> bool {
    cfg!(feature = "startup-bench") || std::env::var_os("ACCSHIFT_EGUI_MARKS").is_some()
}

fn sink() -> &'static Option<Mutex<File>> {
    SINK.get_or_init(|| {
        if !enabled() {
            return None;
        }
        let dir = std::env::temp_dir().join("accshift-egui-bench");
        std::fs::create_dir_all(&dir).ok()?;
        let path = dir.join(format!("{}.jsonl", std::process::id()));
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
            .map(Mutex::new)
    })
}

pub fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or_default()
}

pub fn mark(event: &str) {
    mark_with(event, "");
}

pub fn mark_with(event: &str, extra: &str) {
    let Some(sink) = sink() else { return };
    let ts = now_ms();
    let heap = heap::fields();
    let line = if extra.is_empty() {
        format!("{{\"event\":\"{event}\",\"tsMs\":{ts:.3}{heap}}}\n")
    } else {
        format!("{{\"event\":\"{event}\",\"tsMs\":{ts:.3},{extra}{heap}}}\n")
    };
    if let Ok(mut f) = sink.lock() {
        let _ = f.write_all(line.as_bytes());
    }
}

/// A bench build counts the Rust heap: every mark carries the bytes live
/// (`heap`) and the most ever live (`heapPeak`), so a memory figure of the
/// harness splits into the Rust heap and the rest (DirectWrite, the window
/// surface, DLLs). Blocks of 1 MiB or more are listed too (`big`: bytes,
/// negative when freed, since the previous mark), so a peak names its
/// buffers by their sizes.
#[cfg(feature = "startup-bench")]
mod heap {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering::Relaxed};

    static LIVE: AtomicUsize = AtomicUsize::new(0);
    static PEAK: AtomicUsize = AtomicUsize::new(0);

    const BIG: usize = 1 << 20;
    const SLOTS: usize = 1024;
    static BIG_LOG: [AtomicIsize; SLOTS] = [const { AtomicIsize::new(0) }; SLOTS];
    static BIG_LEN: AtomicUsize = AtomicUsize::new(0);
    static BIG_READ: AtomicUsize = AtomicUsize::new(0);

    fn big(bytes: isize) {
        if bytes.unsigned_abs() >= BIG {
            let slot = BIG_LEN.fetch_add(1, Relaxed);
            if slot < SLOTS {
                BIG_LOG[slot].store(bytes, Relaxed);
            }
        }
    }

    struct Counting;

    fn grow(n: usize) {
        let now = LIVE.fetch_add(n, Relaxed) + n;
        PEAK.fetch_max(now, Relaxed);
    }

    // SAFETY: every call is forwarded to `System` unchanged; the counters
    // only observe sizes.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let p = unsafe { System.alloc(layout) };
            if !p.is_null() {
                grow(layout.size());
                big(layout.size() as isize);
            }
            p
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let p = unsafe { System.alloc_zeroed(layout) };
            if !p.is_null() {
                grow(layout.size());
                big(layout.size() as isize);
            }
            p
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) };
            LIVE.fetch_sub(layout.size(), Relaxed);
            big(-(layout.size() as isize));
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            let p = unsafe { System.realloc(ptr, layout, new_size) };
            if !p.is_null() {
                big(-(layout.size() as isize));
                big(new_size as isize);
                if new_size >= layout.size() {
                    grow(new_size - layout.size());
                } else {
                    LIVE.fetch_sub(layout.size() - new_size, Relaxed);
                }
            }
            p
        }
    }

    #[global_allocator]
    static ALLOC: Counting = Counting;

    pub fn fields() -> String {
        let mut out = format!(
            ",\"heap\":{},\"heapPeak\":{}",
            LIVE.load(Relaxed),
            PEAK.load(Relaxed)
        );
        let end = BIG_LEN.load(Relaxed).min(SLOTS);
        let start = BIG_READ.swap(end, Relaxed).min(end);
        if start < end {
            let sizes: Vec<String> = BIG_LOG[start..end]
                .iter()
                .map(|slot| slot.load(Relaxed).to_string())
                .collect();
            out.push_str(&format!(",\"big\":[{}]", sizes.join(",")));
        }
        out
    }
}

#[cfg(not(feature = "startup-bench"))]
mod heap {
    pub fn fields() -> &'static str {
        ""
    }
}

/// `ACCSHIFT_EGUI_BENCH_SHOT=<file.png>` in a bench run: the first frame
/// that is `ready`, saved there.
const SHOT: &str = "ACCSHIFT_EGUI_BENCH_SHOT";

pub fn request_shot(ctx: &egui::Context) {
    let Some(path) = std::env::var_os(SHOT).filter(|_| enabled()) else {
        return;
    };
    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
        egui::ScreenshotCallback::new(move |image| {
            let _ = crate::harness::write_png(std::path::Path::new(&path), &image);
        }),
    ));
}

/// `ACCSHIFT_EGUI_BENCH_SCRIPT=1` in a bench run: from 1.5 s after the
/// first frame, twelve wheel notches down then up over the window centre,
/// then Ctrl+, twice (the settings open, then close) and Ctrl+Tab then
/// Ctrl+Shift+Tab (the next tab and back), each step a `script` mark.
pub struct Script {
    start: std::time::Instant,
    step: usize,
}

enum Step {
    Wheel(f32),
    Key(egui::Key, egui::Modifiers),
}

fn script_step(i: usize) -> Option<(f64, &'static str, Step)> {
    use egui::{Key, Modifiers};
    let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
    Some(match i {
        0..=11 => (0.08 * i as f64, "wheel-down", Step::Wheel(-1.0)),
        12..=23 => (1.5 + 0.08 * (i - 12) as f64, "wheel-up", Step::Wheel(1.0)),
        24 => (3.0, "settings-open", Step::Key(Key::Comma, Modifiers::CTRL)),
        25 => (
            4.0,
            "settings-close",
            Step::Key(Key::Comma, Modifiers::CTRL),
        ),
        26 => (5.0, "next-tab", Step::Key(Key::Tab, Modifiers::CTRL)),
        27 => (6.0, "prev-tab", Step::Key(Key::Tab, ctrl_shift)),
        28 => (7.0, "end", Step::Wheel(0.0)),
        _ => return None,
    })
}

impl Script {
    pub fn from_env() -> Option<Script> {
        std::env::var_os("ACCSHIFT_EGUI_BENCH_SCRIPT")
            .filter(|_| cfg!(feature = "startup-bench"))
            .map(|_| Script {
                start: std::time::Instant::now() + std::time::Duration::from_millis(1500),
                step: 0,
            })
    }

    /// The steps due by now, as this frame's input, and the wait until the
    /// next one, for the frame to ask (egui drops a request made before the
    /// pass).
    pub fn feed(
        &mut self,
        raw: &mut egui::RawInput,
        center: egui::Pos2,
    ) -> Option<std::time::Duration> {
        let now = std::time::Instant::now();
        if now < self.start {
            return Some(self.start - now);
        }
        let elapsed = (now - self.start).as_secs_f64();
        while let Some((at, name, step)) = script_step(self.step) {
            if at > elapsed {
                return Some(std::time::Duration::from_secs_f64(at - elapsed));
            }
            self.step += 1;
            mark_with("script", &format!("\"step\":\"{name}\""));
            raw.events.push(egui::Event::PointerMoved(center));
            match step {
                Step::Wheel(dy) if dy != 0.0 => raw.events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Line,
                    delta: egui::vec2(0.0, dy),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Wheel,
                    modifiers: egui::Modifiers::NONE,
                }),
                Step::Wheel(_) => {}
                Step::Key(key, modifiers) => {
                    raw.events.push(egui::Event::ModifiersChanged(modifiers));
                    for pressed in [true, false] {
                        raw.events.push(egui::Event::Key {
                            key,
                            physical_key: Some(key),
                            pressed,
                            repeat: false,
                            modifiers,
                        });
                    }
                    raw.events
                        .push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
                }
            }
        }
        None
    }
}

thread_local! {
    static SPANS: std::cell::RefCell<Vec<(&'static str, f64)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Times its scope into the frame's `spans` (bench only).
pub struct Span(Option<(&'static str, std::time::Instant)>);

pub fn span(name: &'static str) -> Span {
    Span(cfg!(feature = "startup-bench").then(|| (name, std::time::Instant::now())))
}

impl Drop for Span {
    fn drop(&mut self) {
        if let Some((name, start)) = self.0 {
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            SPANS.with_borrow_mut(|spans| match spans.iter_mut().find(|(n, _)| *n == name) {
                Some((_, total)) => *total += ms,
                None => spans.push((name, ms)),
            });
        }
    }
}

/// The frame's spans as a JSON object, cleared.
pub fn take_spans() -> String {
    SPANS.with_borrow_mut(|spans| {
        let body: Vec<String> = spans
            .drain(..)
            .map(|(name, ms)| format!("\"{name}\":{ms:.3}"))
            .collect();
        format!("{{{}}}", body.join(","))
    })
}

/// [`mark`] the first time only, for a step several places can reach.
pub fn mark_once(event: &'static str) {
    static SEEN: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    if !enabled() {
        return;
    }
    if let Ok(mut seen) = SEEN.lock() {
        if !seen.contains(&event) {
            seen.push(event);
            mark(event);
        }
    }
}
