//! Editing in a single-line `<input>` as Chromium does it on Windows
//! (`TextFieldInputType`, `EditingBehavior`, `SelectionController`): a caret
//! and a selection, the arrows with Shift and Ctrl, Home and End, Backspace
//! and Delete by character or word, Ctrl+A, the clipboard, undo, the mouse
//! (click, Shift+click, drag, double-click for a word, triple-click for the
//! whole value) and IME composition.
//!
//! A field keeps its [`Editor`] in egui's memory under its id: it runs the
//! frame's pointer and keys through [`Editor::edit`], lays the value out with
//! [`Editor::view`] (which scrolls the inner editor to the caret) and paints
//! from the result.
//!
//! A `<textarea>` ([`Rules::area`]) lays its text out itself and passes the
//! [`Rows`] to [`Editor::area_pointer`] and [`Editor::area_keys`]: Enter
//! breaks the line, pasted line breaks stay, the arrows up and down move by
//! row and a triple-click selects the paragraph.

mod input;
mod layout;
mod words;

#[cfg(test)]
mod tests;

pub use layout::{Rows, View};

use std::ops::Range;

use accshift_app::js::utf16_len;
use egui::{Context, Event, Id, Pos2, Rect, Response, Ui};

/// Windows' double-click time (`GetDoubleClickTime`), in seconds.
const MULTI_CLICK_TIME: f64 = 0.5;
/// Half of `SM_CXDOUBLECLK`: a press this close to the last one counts on.
const MULTI_CLICK_SLOP: f32 = 2.0;
/// Undo steps kept per field.
const UNDO_DEPTH: usize = 100;

/// What a field accepts.
#[derive(Clone, Copy)]
pub struct Rules {
    /// The characters typing or pasting may insert.
    pub accepts: fn(char) -> bool,
    /// `maxlength`, in UTF-16 code units like the DOM.
    pub max_len: Option<usize>,
    /// `type="password"`: no copy or cut, no IME, and a word is the whole
    /// value.
    pub secret: bool,
    /// A `<textarea>`: Enter and paste insert line breaks.
    pub multiline: bool,
}

impl Rules {
    pub fn text() -> Rules {
        Rules {
            accepts: printable,
            max_len: None,
            secret: false,
            multiline: false,
        }
    }

    /// A `<textarea maxlength>`.
    pub fn area(max_len: usize) -> Rules {
        Rules {
            accepts: printable_or_break,
            max_len: Some(max_len),
            secret: false,
            multiline: true,
        }
    }
}

/// Any character a text field takes: not a control character.
pub fn printable(c: char) -> bool {
    !c.is_control()
}

/// What a `<textarea>` takes: printable characters and line breaks.
pub fn printable_or_break(c: char) -> bool {
    c == '\n' || printable(c)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    value: String,
    caret: usize,
    anchor: usize,
}

/// The kind of the last edit: a run of typing or of deleting one way is a
/// single undo step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Typing,
    DeleteBack,
    DeleteForward,
    Other,
}

/// What a drag after a press extends by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unit {
    Char,
    Word,
    /// A `<textarea>`'s paragraph.
    Paragraph,
    All,
}

#[derive(Clone, Debug, Default)]
pub struct Editor {
    /// The caret, in characters.
    pub caret: usize,
    /// The selection's other end; equal to `caret` when it is collapsed.
    pub anchor: usize,
    /// The IME composition, shown at the caret until it commits.
    pub preedit: String,
    /// The inner editor's horizontal scroll, in device px.
    pub scroll: f32,
    /// In a `<textarea>`: the caret sits at the end of a wrapped row rather
    /// than at the start of the next one (upstream affinity).
    pub upstream: bool,
    /// In a `<textarea>`: the x the arrows up and down keep, in points.
    goal_x: Option<f32>,
    /// The value as the field last left it: any other value was set from
    /// outside (`input.value = ...`).
    known: Option<String>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    step: Option<Step>,
    /// The last primary press: time, position and click count.
    press: Option<(f64, Pos2, u32)>,
    /// While the primary button is down after a press on the field: the
    /// unit the drag selects by and the range the press selected.
    drag: Option<(Unit, Range<usize>)>,
}

/// What the frame's input did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Edited {
    /// `input`: the value changed.
    pub changed: bool,
    pub enter: bool,
    pub escape: bool,
    /// PageUp or PageDown moved the caret without Shift: Chromium scrolls
    /// its line to the top (`CursorAlignOnScroll::kAlways`).
    pub page: bool,
}

/// While a field has focus: where the IME places its candidate window.
/// Password fields keep the IME off, as Chromium does.
pub fn ime(ctx: &Context, field: Rect, caret: Rect) {
    ctx.output_mut(|o| {
        o.ime = Some(egui::output::IMEOutput {
            purpose: egui::IMEPurpose::Normal,
            rect: field,
            cursor_rect: caret,
            should_interrupt_composition: false,
        });
    });
}

fn key(id: Id) -> Id {
    id.with("editor")
}

impl Editor {
    /// The editor of field `id`, caught up with `value`.
    pub fn load(ctx: &Context, id: Id, value: &str) -> Editor {
        let mut ed: Editor = ctx.data(|d| d.get_temp(key(id))).unwrap_or_default();
        ed.sync(value);
        ed
    }

    /// Keeps the editor until the next frame.
    pub fn store(mut self, ctx: &Context, id: Id, value: &str) {
        self.known = Some(value.to_owned());
        ctx.data_mut(|d| d.insert_temp(key(id), self));
    }

    /// A value the field did not write: like `HTMLInputElement::setValue`,
    /// the caret goes to its end; the history no longer applies.
    fn sync(&mut self, value: &str) {
        if self.known.as_deref() == Some(value) {
            return;
        }
        let n = value.chars().count();
        self.caret = n;
        self.anchor = n;
        self.preedit.clear();
        self.undo.clear();
        self.redo.clear();
        self.step = None;
        self.known = Some(value.to_owned());
    }

    pub fn selection(&self) -> Range<usize> {
        self.caret.min(self.anchor)..self.caret.max(self.anchor)
    }

    /// `input.select()`.
    pub fn select_all(&mut self, value: &str) {
        self.anchor = 0;
        self.caret = value.chars().count();
        self.step = None;
        self.upstream = false;
        self.goal_x = None;
    }

    /// Focus arrived. Tab selects the whole value
    /// (`SelectionBehaviorOnFocus::kReset`); `focus()` from script keeps
    /// the last selection; a press places the caret itself.
    pub fn focus(&mut self, value: &str, by_tab: bool) {
        if by_tab {
            self.select_all(value);
        }
    }

    /// Focus left: the selection stays for the next `focus()`, the inner
    /// editor scrolls back to its start.
    pub fn blur(&mut self) {
        self.scroll = 0.0;
        self.preedit.clear();
        self.drag = None;
    }

    /// One frame of a focused field: the pointer on `response` (the field's
    /// box), then `events`. `layout` is the value's [`View`] before this
    /// frame, for hit tests.
    pub fn edit(
        &mut self,
        ui: &Ui,
        response: &Response,
        value: &mut String,
        rules: &Rules,
        layout: &View,
        events: &[Event],
    ) -> Edited {
        self.pointer(ui, response, value, rules, layout);
        self.keys(ui.ctx(), value, events, rules)
    }

    fn move_to(&mut self, to: usize, extend: bool) {
        self.caret = to;
        if !extend {
            self.anchor = to;
        }
        self.step = None;
        self.upstream = false;
        self.goal_x = None;
    }

    /// Typed, pasted or composed `text` replaces the selection:
    /// `HandleBeforeTextInsertedEvent` drops what the field refuses and
    /// cuts what passes `maxlength`.
    fn insert(&mut self, value: &mut String, text: &str, rules: &Rules, step: Step) -> bool {
        let mut text: String = text.chars().filter(|c| (rules.accepts)(*c)).collect();
        let sel = self.selection();
        if let Some(max) = rules.max_len {
            let used = utf16_len(value) - utf16_len(slice(value, sel.clone()));
            text = truncate_utf16(&text, max.saturating_sub(used));
        }
        if text.is_empty() {
            return false;
        }
        let step = if sel.is_empty() { step } else { Step::Other };
        self.replace(value, sel, &text, step);
        true
    }

    fn replace(&mut self, value: &mut String, range: Range<usize>, text: &str, step: Step) {
        let coalesce = step != Step::Other && self.step == Some(step);
        if !coalesce {
            self.undo.push(Snapshot {
                value: value.clone(),
                caret: self.caret,
                anchor: self.anchor,
            });
            if self.undo.len() > UNDO_DEPTH {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.step = Some(step);
        let (a, b) = (byte(value, range.start), byte(value, range.end));
        value.replace_range(a..b, text);
        self.caret = range.start + text.chars().count();
        self.anchor = self.caret;
        self.upstream = false;
        self.goal_x = None;
    }

    fn undo(&mut self, value: &mut String) -> bool {
        let Some(s) = self.undo.pop() else {
            return false;
        };
        self.redo.push(self.snapshot(value));
        self.restore(value, s);
        true
    }

    fn redo(&mut self, value: &mut String) -> bool {
        let Some(s) = self.redo.pop() else {
            return false;
        };
        self.undo.push(self.snapshot(value));
        self.restore(value, s);
        true
    }

    fn snapshot(&self, value: &str) -> Snapshot {
        Snapshot {
            value: value.to_owned(),
            caret: self.caret,
            anchor: self.anchor,
        }
    }

    fn restore(&mut self, value: &mut String, s: Snapshot) {
        *value = s.value;
        self.caret = s.caret;
        self.anchor = s.anchor;
        self.step = None;
        self.upstream = false;
        self.goal_x = None;
    }
}

/// `TextFieldInputType::HandleBeforeTextInsertedEvent` on pasted text:
/// trailing line breaks go, the others become spaces.
pub fn paste_text(text: &str) -> String {
    text.trim_end_matches(['\r', '\n'])
        .replace("\r\n", " ")
        .replace(['\r', '\n'], " ")
}

fn byte(s: &str, chars: usize) -> usize {
    s.char_indices().nth(chars).map_or(s.len(), |(b, _)| b)
}

fn slice(s: &str, range: Range<usize>) -> &str {
    &s[byte(s, range.start)..byte(s, range.end)]
}

/// The longest prefix of `s` within `max` UTF-16 code units: never half a
/// surrogate pair.
fn truncate_utf16(s: &str, max: usize) -> String {
    let mut used = 0;
    s.chars()
        .take_while(|c| {
            used += c.len_utf16();
            used <= max
        })
        .collect()
}
