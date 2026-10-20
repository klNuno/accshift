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

use std::ops::Range;

use egui::{Context, Event, Id, ImeEvent, Key, Modifiers, Pos2, Rect, Response, Ui};

use crate::ui::text::{self, TextAlign, TextStyle};

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
    /// The inner editor's `scrollLeft`, in device px.
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

/// The value as laid out this frame.
#[derive(Clone, Debug)]
pub struct View {
    /// The text drawn: the value, as bullets for a password, with the IME
    /// composition at the caret.
    pub shown: String,
    /// The x of every character boundary of `shown` from the text's start,
    /// in CSS px; the last is the text's width.
    pub offsets: Vec<f32>,
    /// The caret's offset in `shown`.
    pub caret: usize,
    /// The selection in `shown`, empty when collapsed.
    pub selection: Range<usize>,
    /// The composition in `shown`, empty without one.
    pub preedit: Range<usize>,
    /// The line box moved by the scroll: where the text is laid out.
    pub scrolled: Rect,
    /// Where the text starts: `scrolled` past the free space of a centred
    /// line.
    pub line: Rect,
}

impl View {
    pub fn width(&self) -> f32 {
        self.offsets.last().copied().unwrap_or(0.0)
    }

    /// The x of boundary `i` of `shown`, in points.
    pub fn x(&self, i: usize) -> f32 {
        self.line.left() + self.offsets[i.min(self.offsets.len() - 1)]
    }

    /// The selection's highlight over the line box `line` (its vertical
    /// extent), unsnapped; `None` when collapsed.
    pub fn selection_rect(&self, line: Rect) -> Option<Rect> {
        (!self.selection.is_empty()).then(|| {
            Rect::from_x_y_ranges(
                self.x(self.selection.start)..=self.x(self.selection.end),
                line.y_range(),
            )
        })
    }

    /// The composition's underline: one device pixel at the bottom of the
    /// line box `line`, in the text colour.
    pub fn paint_preedit(&self, painter: &egui::Painter, line: Rect, color: egui::Color32) {
        if self.preedit.is_empty() {
            return;
        }
        let ppp = painter.ctx().pixels_per_point();
        let y = crate::ui::snap::coord(line.bottom(), ppp) - 1.0 / ppp;
        painter.rect_filled(
            Rect::from_x_y_ranges(
                self.x(self.preedit.start)..=self.x(self.preedit.end),
                y..=y + 1.0 / ppp,
            ),
            0.0,
            color,
        );
    }
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

    /// The primary button on the field's box `response`: a press places
    /// the caret (Shift extends, a second press selects a word, a third
    /// the value), a drag extends by the press's unit.
    pub fn pointer(
        &mut self,
        ui: &Ui,
        response: &Response,
        value: &str,
        rules: &Rules,
        layout: &View,
    ) {
        self.track(ui, response, value, rules, &|at: Pos2| {
            (hit(layout, at.x), false)
        });
    }

    /// [`Editor::pointer`] in a `<textarea>` laid out as `rows`.
    pub fn area_pointer(
        &mut self,
        ui: &Ui,
        response: &Response,
        value: &str,
        rules: &Rules,
        rows: &Rows,
    ) {
        self.track(ui, response, value, rules, &|at: Pos2| rows.hit(at));
    }

    /// The primary button for a field whose `hit` gives the value offset at
    /// a point, and whether the caret shows at the end of the row before.
    fn track(
        &mut self,
        ui: &Ui,
        response: &Response,
        value: &str,
        rules: &Rules,
        hit: &dyn Fn(Pos2) -> (usize, bool),
    ) {
        let (pressed, origin, pos, down, time, shift) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.press_origin(),
                i.pointer.interact_pos(),
                i.pointer.primary_down(),
                i.time,
                i.modifiers.shift,
            )
        });
        // As `take_focus`: a press in the same frame as the last release
        // reads the hit test.
        let here = pressed
            && (response.is_pointer_button_down_on()
                || ui
                    .ctx()
                    .viewport(|vp| vp.hits.click.is_some_and(|w| w.id == response.id)));
        let chars: Vec<char> = value.chars().collect();
        let n = chars.len();
        if here {
            let Some(at) = origin.or(pos) else {
                return;
            };
            let (p, up) = hit(at);
            let p = p.min(n);
            let count = match self.press {
                Some((t, last, c))
                    if time - t <= MULTI_CLICK_TIME
                        && (at - last).abs().max_elem() <= MULTI_CLICK_SLOP =>
                {
                    c + 1
                }
                _ => 1,
            };
            self.press = Some((time, at, count));
            self.step = None;
            self.preedit.clear();
            self.goal_x = None;
            self.upstream = false;
            let range = match count {
                1 => {
                    if !shift {
                        self.anchor = p;
                    }
                    self.caret = p;
                    self.upstream = up;
                    self.drag = Some((Unit::Char, self.anchor..self.anchor));
                    return;
                }
                2 => {
                    let w = word_with_space(&chars, p, rules.secret);
                    self.drag = Some((Unit::Word, w.clone()));
                    w
                }
                // `TextGranularity::kParagraph` in a `<textarea>`.
                _ if rules.multiline => {
                    let par = paragraph(&chars, p);
                    self.drag = Some((Unit::Paragraph, par.clone()));
                    par
                }
                _ => {
                    self.drag = Some((Unit::All, 0..n));
                    0..n
                }
            };
            self.anchor = range.start;
            self.caret = range.end;
            return;
        }
        let Some((unit, origin)) = self.drag.clone() else {
            return;
        };
        if !down {
            self.drag = None;
            return;
        }
        let Some(at) = pos else {
            return;
        };
        let (p, up) = hit(at);
        let p = p.min(n);
        match unit {
            Unit::Char => {
                self.caret = p;
                self.upstream = up;
            }
            Unit::Word | Unit::Paragraph => {
                let w = if unit == Unit::Word {
                    word_at(&chars, p, rules.secret)
                } else {
                    paragraph(&chars, p)
                };
                if p < origin.start {
                    self.anchor = origin.end;
                    self.caret = w.start;
                } else {
                    self.anchor = origin.start;
                    self.caret = w.end.max(origin.end);
                }
            }
            Unit::All => {}
        }
    }

    /// Typing, paste, copy, cut, composition and the editing keys.
    pub fn keys(
        &mut self,
        ctx: &Context,
        value: &mut String,
        events: &[Event],
        rules: &Rules,
    ) -> Edited {
        let mut out = Edited::default();
        for event in events {
            self.event(ctx, value, event, rules, &mut out);
        }
        out
    }

    /// [`Editor::keys`] in a `<textarea>` laid out as `rows`: the arrows up
    /// and down move by row and keep the caret's x for the next one, Home
    /// and End go to the row's ends (with Ctrl, the text's).
    pub fn area_keys(
        &mut self,
        ctx: &Context,
        value: &mut String,
        events: &[Event],
        rules: &Rules,
        rows: &Rows,
    ) -> Edited {
        let mut out = Edited::default();
        for event in events {
            if let Event::Key {
                key,
                pressed: true,
                modifiers: m,
                ..
            } = event
            {
                // A composition takes the keys; `rows` holds it too.
                if self.preedit.is_empty() {
                    match key {
                        Key::ArrowUp | Key::ArrowDown if !m.command => {
                            let delta = if *key == Key::ArrowUp { -1 } else { 1 };
                            self.vertical(value, rows, delta, m.shift);
                            continue;
                        }
                        Key::Home | Key::End if !m.command => {
                            self.row_edge(rows, *key == Key::End, m.shift);
                            continue;
                        }
                        Key::PageUp | Key::PageDown if !m.command => {
                            let up = *key == Key::PageUp;
                            out.page |= self.page(value, rows, up, m.shift) && !m.shift;
                            continue;
                        }
                        _ => {}
                    }
                }
            }
            self.event(ctx, value, event, rules, &mut out);
        }
        out
    }

    /// Up or down `delta` rows: Chromium keeps the x of the first move
    /// (`x_pos_for_vertical_arrow_navigation`); past the first row the caret
    /// goes to the start, past the last to the end. Without Shift a range
    /// collapses first, from its start going up and its end going down.
    fn vertical(&mut self, value: &str, rows: &Rows, delta: isize, extend: bool) {
        let n = value.chars().count();
        let sel = self.selection();
        let from = match (extend, sel.is_empty(), delta < 0) {
            (false, false, true) => sel.start,
            (false, false, false) => sel.end,
            _ => self.caret,
        };
        let (row, x) = rows.locate(from, self.upstream && from == self.caret);
        let goal = self.goal_x.unwrap_or(x);
        let target = row as isize + delta;
        let (to, up) = if target < 0 {
            (0, false)
        } else if target as usize >= rows.len() {
            (n, false)
        } else {
            rows.at(target as usize, goal)
        };
        self.caret = to.min(n);
        if !extend {
            self.anchor = self.caret;
        }
        self.upstream = up;
        self.goal_x = Some(goal);
        self.step = None;
    }

    /// PageUp or PageDown (`ModifyWithPageGranularity`): row by row at the
    /// kept x while the caret moves at most [`Rows::paging`]; reaching the
    /// first or last row goes on to the start or the end. With no row in
    /// reach nothing moves, and it returns false.
    fn page(&mut self, value: &str, rows: &Rows, up: bool, extend: bool) -> bool {
        let n = value.chars().count();
        let sel = self.selection();
        let from = match (extend, sel.is_empty(), up) {
            (false, false, true) => sel.start,
            (false, false, false) => sel.end,
            _ => self.caret,
        };
        let (row, x) = rows.locate(from, self.upstream && from == self.caret);
        let goal = self.goal_x.unwrap_or(x);
        let last = rows.len() - 1;
        let reach = ((rows.page + 1e-3) / rows.height).floor().max(0.0) as usize;
        let (to, upstream) = if up && reach >= row {
            (0, false)
        } else if !up && row + reach >= last {
            (n, false)
        } else if reach == 0 {
            return false;
        } else if up {
            rows.at(row - reach, goal)
        } else {
            rows.at(row + reach, goal)
        };
        self.caret = to.min(n);
        if !extend {
            self.anchor = self.caret;
        }
        self.upstream = upstream;
        self.goal_x = Some(goal);
        self.step = None;
        true
    }

    /// Home or End in a `<textarea>`: the start or the end of the caret's
    /// row; the end of a wrapped row keeps the caret on it.
    fn row_edge(&mut self, rows: &Rows, end: bool, extend: bool) {
        let (row, _) = rows.locate(self.caret, self.upstream);
        let (to, up) = if end {
            rows.end(row)
        } else {
            (rows.start(row), false)
        };
        self.caret = to;
        if !extend {
            self.anchor = to;
        }
        self.upstream = up;
        self.goal_x = None;
        self.step = None;
    }

    fn event(
        &mut self,
        ctx: &Context,
        value: &mut String,
        event: &Event,
        rules: &Rules,
        out: &mut Edited,
    ) {
        {
            match event {
                Event::Text(t) => out.changed |= self.insert(value, t, rules, Step::Typing),
                Event::Paste(t) => {
                    // A `<textarea>` keeps the line breaks, as LF.
                    let t = if rules.multiline {
                        t.replace("\r\n", "\n").replace('\r', "\n")
                    } else {
                        paste_text(t)
                    };
                    out.changed |= self.insert(value, &t, rules, Step::Other)
                }
                Event::Copy | Event::Cut => {
                    let sel = self.selection();
                    if rules.secret || sel.is_empty() {
                        return;
                    }
                    ctx.copy_text(slice(value, sel.clone()).to_owned());
                    if matches!(event, Event::Cut) {
                        self.replace(value, sel, "", Step::Other);
                        out.changed = true;
                    }
                }
                Event::Ime(ime) => match ime {
                    ImeEvent::Preedit { text, .. } => {
                        // The composition replaces the selection.
                        let sel = self.selection();
                        if !text.is_empty() && self.preedit.is_empty() && !sel.is_empty() {
                            self.replace(value, sel, "", Step::Other);
                            out.changed = true;
                        }
                        self.preedit = text.clone();
                    }
                    ImeEvent::Commit(t) => {
                        self.preedit.clear();
                        out.changed |= self.insert(value, t, rules, Step::Typing);
                    }
                    _ => self.preedit.clear(),
                },
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => self.key(value, *key, *modifiers, rules, out),
                _ => {}
            }
        }
    }

    fn key(&mut self, value: &mut String, key: Key, m: Modifiers, rules: &Rules, out: &mut Edited) {
        let chars: Vec<char> = value.chars().collect();
        let n = chars.len();
        let word = if cfg!(target_os = "macos") {
            m.alt
        } else {
            m.ctrl
        };
        let sel = self.selection();
        match key {
            Key::ArrowLeft | Key::ArrowRight => {
                let forward = key == Key::ArrowRight;
                let to = if word {
                    if forward {
                        word_right(&chars, self.caret, rules.secret)
                    } else {
                        word_left(&chars, self.caret, rules.secret)
                    }
                } else if !m.shift && !sel.is_empty() {
                    // A range collapses to the side the arrow points to.
                    if forward {
                        sel.end
                    } else {
                        sel.start
                    }
                } else if forward {
                    (self.caret + 1).min(n)
                } else {
                    self.caret.saturating_sub(1)
                };
                self.move_to(to, m.shift);
            }
            Key::Home => self.move_to(0, m.shift),
            Key::End => self.move_to(n, m.shift),
            Key::Backspace | Key::Delete => {
                let forward = key == Key::Delete;
                if !sel.is_empty() {
                    self.replace(value, sel, "", Step::Other);
                    out.changed = true;
                    return;
                }
                let range = match (forward, word) {
                    (false, false) => self.caret.saturating_sub(1)..self.caret,
                    (false, true) => word_left(&chars, self.caret, rules.secret)..self.caret,
                    (true, false) => self.caret..(self.caret + 1).min(n),
                    (true, true) => self.caret..word_right(&chars, self.caret, rules.secret),
                };
                if !range.is_empty() {
                    let step = if forward {
                        Step::DeleteForward
                    } else {
                        Step::DeleteBack
                    };
                    self.replace(value, range, "", step);
                    out.changed = true;
                }
            }
            Key::A if m.command => self.select_all(value),
            Key::Z if m.command && !m.shift => out.changed |= self.undo(value),
            Key::Z if m.command => out.changed |= self.redo(value),
            Key::Y if m.command => out.changed |= self.redo(value),
            // `InsertLineBreak` joins the open typing step.
            Key::Enter if rules.multiline => {
                out.changed |= self.insert(value, "\n", rules, Step::Typing);
            }
            Key::Enter => out.enter = true,
            Key::Escape => out.escape = true,
            _ => {}
        }
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

    /// Lays `value` out in the unscrolled line box `line` and, while
    /// focused, scrolls the inner editor so the caret shows
    /// (`RevealSelection` to the nearest edge), never past the text's end
    /// plus the caret. `caret_w` is the caret's width in device px.
    #[allow(clippy::too_many_arguments)]
    pub fn view(
        &mut self,
        ctx: &Context,
        value: &str,
        secret: bool,
        font: &TextStyle,
        line: Rect,
        caret_w: f32,
        focused: bool,
    ) -> View {
        let ppp = ctx.pixels_per_point();
        let n = value.chars().count();
        self.caret = self.caret.min(n);
        self.anchor = self.anchor.min(n);
        let base: String = if secret {
            "\u{2022}".repeat(n)
        } else {
            value.to_owned()
        };
        let pre = if focused { self.preedit.as_str() } else { "" };
        let split = byte(&base, self.caret);
        let shown = format!("{}{pre}{}", &base[..split], &base[split..]);
        let pre_len = pre.chars().count();
        let offsets = offsets(ctx, &shown, font);
        let width = offsets.last().copied().unwrap_or(0.0);
        let caret = self.caret + pre_len;
        if focused {
            let x = offsets[caret] * ppp;
            let visible = (line.width() * ppp).max(0.0);
            let mut s = self.scroll;
            if x + caret_w - s > visible {
                s = x + caret_w - visible;
            }
            if x < s {
                s = x;
            }
            let max = (width * ppp + caret_w - visible).max(0.0);
            self.scroll = s.clamp(0.0, max).round();
        } else {
            self.scroll = 0.0;
        }
        let sel = self.selection();
        let shift = |i: usize| if i > self.caret { i + pre_len } else { i };
        let lead = if font.align == TextAlign::Center {
            ((line.width() - width) / 2.0).max(0.0)
        } else {
            0.0
        };
        View {
            shown,
            offsets,
            caret,
            selection: shift(sel.start)..shift(sel.end),
            preedit: self.caret..self.caret + pre_len,
            scrolled: line.translate(egui::vec2(-self.scroll / ppp, 0.0)),
            line: line.translate(egui::vec2(lead - self.scroll / ppp, 0.0)),
        }
    }
}

/// A `<textarea>`'s text as laid out: per row, its first character and the
/// x of each character boundary in it.
#[derive(Clone, Debug)]
pub struct Rows {
    rows: Vec<Row>,
    /// The first row's top, in points.
    top: f32,
    /// The line box's height, in points.
    height: f32,
    /// How far PageUp and PageDown move the caret, in points.
    page: f32,
    /// The composition, in the laid out text: hits inside it go to its
    /// start, after it they move back by its length.
    preedit: Range<usize>,
}

#[derive(Clone, Debug)]
struct Row {
    /// Its first character in the laid out text.
    start: usize,
    /// The x of each boundary, in points: one more than its characters
    /// (a wrapped row keeps the space it broke at).
    xs: Vec<f32>,
    /// A line break ends it, rather than a wrap.
    newline: bool,
}

impl Rows {
    /// The rows of `laid`, the text laid out at `origin` (its first line
    /// box's top left) with `height`-point lines; `None` lays out an empty
    /// text. `preedit` is the composition's range in the laid out text.
    pub fn new(
        laid: Option<&text::TextLayout>,
        origin: Pos2,
        height: f32,
        preedit: Range<usize>,
    ) -> Rows {
        let mut rows = Vec::new();
        if let Some(laid) = laid {
            let mut start = 0;
            for placed in &laid.galley.rows {
                let x0 = laid.pos.x + placed.pos.x;
                let glyphs = &placed.row.glyphs;
                let mut xs: Vec<f32> = glyphs.iter().map(|g| x0 + g.pos.x).collect();
                xs.push(
                    glyphs
                        .last()
                        .map_or(origin.x, |g| x0 + g.pos.x + g.advance_width),
                );
                rows.push(Row {
                    start,
                    xs,
                    newline: placed.ends_with_newline,
                });
                start += glyphs.len() + usize::from(placed.ends_with_newline);
            }
        }
        if rows.is_empty() {
            rows.push(Row {
                start: 0,
                xs: vec![origin.x],
                newline: false,
            });
        }
        Rows {
            rows,
            top: origin.y,
            height,
            page: 0.0,
            preedit,
        }
    }

    /// PageUp and PageDown move by 87.5 % of the `scrollport` height (in
    /// points), cut to whole device pixels as Blink's
    /// `VerticalScrollDistance`.
    pub fn paging(mut self, scrollport: f32, ppp: f32) -> Rows {
        self.page = ((scrollport * ppp).trunc() * 0.875).trunc() / ppp;
        self
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The top of row `row`, in points.
    pub fn top(&self, row: usize) -> f32 {
        self.top + row as f32 * self.height
    }

    /// Whether row `row` holds no character.
    pub fn blank(&self, row: usize) -> bool {
        self.rows[row].xs.len() == 1
    }

    fn start(&self, row: usize) -> usize {
        self.rows[row].start
    }

    /// The offset after row `row`'s last character, and whether the caret
    /// stays on the row there (a wrapped row, not the last).
    fn end(&self, row: usize) -> (usize, bool) {
        let r = &self.rows[row];
        let wrapped = !r.newline && row + 1 < self.rows.len();
        (r.start + r.xs.len() - 1, wrapped)
    }

    /// The row the caret at offset `p` shows on and its x: the boundary
    /// between a wrapped row and the next is on the next one unless
    /// `upstream`.
    pub fn locate(&self, p: usize, upstream: bool) -> (usize, f32) {
        let mut row = self.rows.iter().rposition(|r| r.start <= p).unwrap_or(0);
        if upstream && row > 0 && self.rows[row].start == p && !self.rows[row - 1].newline {
            row -= 1;
        }
        let r = &self.rows[row];
        (row, r.xs[(p - r.start).min(r.xs.len() - 1)])
    }

    /// The offset nearest to `x` on row `row`, and whether the caret stays
    /// on that row (past a wrapped row's end).
    fn at(&self, row: usize, x: f32) -> (usize, bool) {
        let r = &self.rows[row];
        let i = nearest(&r.xs, x);
        let wrapped = !r.newline && row + 1 < self.rows.len();
        (r.start + i, wrapped && i == r.xs.len() - 1)
    }

    /// The value offset at `at`: the row under it (the first above the
    /// text, the last below it), then the nearest boundary.
    fn hit(&self, at: Pos2) -> (usize, bool) {
        let row = ((at.y - self.top) / self.height).floor();
        let row = (row.max(0.0) as usize).min(self.rows.len() - 1);
        let (shown, up) = self.at(row, at.x);
        (unshift(shown, &self.preedit), up)
    }

    /// The parts of `range` (in the laid out text) on each row: the row and
    /// the x span. A range that takes a row's line break reaches `newline`
    /// points past its end, where Chromium paints the break selected.
    pub fn spans(&self, range: Range<usize>, newline: f32) -> Vec<(usize, f32, f32)> {
        let mut out = Vec::new();
        if range.is_empty() {
            return out;
        }
        for (i, r) in self.rows.iter().enumerate() {
            let end = r.start + r.xs.len() - 1;
            let (a, b) = (range.start.max(r.start), range.end.min(end));
            let takes_break = r.newline && range.start <= end && range.end > end;
            if a > b || (a == b && !takes_break) {
                continue;
            }
            let x1 = r.xs[a - r.start];
            let mut x2 = r.xs[b - r.start];
            if takes_break {
                x2 += newline;
            }
            out.push((i, x1, x2));
        }
        out
    }
}

/// A boundary of the text shown with the composition `pre` back in the
/// value: inside the composition it is its start.
fn unshift(shown: usize, pre: &Range<usize>) -> usize {
    if shown <= pre.start {
        shown
    } else if shown < pre.end {
        pre.start
    } else {
        shown - pre.len()
    }
}

/// The value offset nearest to `x` (points, in the field): Blink puts the
/// caret before a glyph hit on its first half, after it otherwise.
fn hit(layout: &View, x: f32) -> usize {
    let x = x - layout.line.left();
    unshift(nearest(&layout.offsets, x), &layout.preedit)
}

/// The paragraph a triple-click at `p` selects in a `<textarea>`: the line
/// between two breaks, with the break after it.
fn paragraph(chars: &[char], p: usize) -> Range<usize> {
    let n = chars.len();
    let p = p.min(n);
    let start = chars[..p]
        .iter()
        .rposition(|c| *c == '\n')
        .map_or(0, |i| i + 1);
    let end = chars[p..]
        .iter()
        .position(|c| *c == '\n')
        .map_or(n, |i| p + i + 1);
    start..end
}

/// `TextFieldInputType::HandleBeforeTextInsertedEvent` on pasted text:
/// trailing line breaks go, the others become spaces.
pub fn paste_text(text: &str) -> String {
    text.trim_end_matches(['\r', '\n'])
        .replace("\r\n", " ")
        .replace(['\r', '\n'], " ")
}

/// The x of every character boundary of `text` laid out in `font`, in CSS
/// px from its start; the last is its width.
fn offsets(ctx: &Context, text: &str, font: &TextStyle) -> Vec<f32> {
    let n = text.chars().count();
    text::offsets(ctx, text, font).unwrap_or_else(|| vec![0.0; n + 1])
}

/// The boundary in `offsets` nearest to `x`.
fn nearest(offsets: &[f32], x: f32) -> usize {
    let mut best = 0;
    for (i, o) in offsets.iter().enumerate() {
        if (o - x).abs() < (offsets[best] - x).abs() {
            best = i;
        }
    }
    best
}

fn byte(s: &str, chars: usize) -> usize {
    s.char_indices().nth(chars).map_or(s.len(), |(b, _)| b)
}

fn slice(s: &str, range: Range<usize>) -> &str {
    &s[byte(s, range.start)..byte(s, range.end)]
}

fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
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

/// A character class of the word break rules (UAX #29 as ICU applies it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Space,
    Word,
    Punct,
}

/// Letters, digits and `_` make words; an apostrophe, a period or a colon
/// between two letters, or a period, comma or apostrophe between two
/// digits, joins them.
fn classes(chars: &[char]) -> Vec<Class> {
    let wordy = |c: char| c.is_alphanumeric() || c == '_';
    (0..chars.len())
        .map(|i| {
            let c = chars[i];
            if c.is_whitespace() {
                return Class::Space;
            }
            if wordy(c) {
                return Class::Word;
            }
            let prev = i.checked_sub(1).map(|j| chars[j]);
            let next = chars.get(i + 1).copied();
            let both = |f: fn(char) -> bool| prev.is_some_and(f) && next.is_some_and(f);
            let letters =
                matches!(c, '\'' | '.' | ':' | '\u{2019}' | '\u{b7}') && both(char::is_alphabetic);
            let digits =
                matches!(c, '\'' | '.' | ',' | ';' | '\u{2019}') && both(|c| c.is_numeric());
            if letters || digits {
                Class::Word
            } else {
                Class::Punct
            }
        })
        .collect()
}

/// The run of one class around `i` for moving: a word, a run of
/// punctuation or a run of spaces.
fn run(cls: &[Class], i: usize) -> Range<usize> {
    let c = cls[i];
    let mut a = i;
    while a > 0 && cls[a - 1] == c {
        a -= 1;
    }
    let mut b = i + 1;
    while b < cls.len() && cls[b] == c {
        b += 1;
    }
    a..b
}

/// Ctrl+Right on Windows (`NextWordPosition` with `kWordSkipSpaces`): past
/// the word or punctuation run at `p`, then past the spaces after it.
fn word_right(chars: &[char], p: usize, secret: bool) -> usize {
    let n = chars.len();
    if secret || p >= n {
        return n;
    }
    let cls = classes(chars);
    let mut q = run(&cls, p).end;
    if cls[p] != Class::Space {
        while q < n && cls[q] == Class::Space {
            q += 1;
        }
    }
    q
}

/// Ctrl+Left (`PreviousWordPosition`): back over spaces, then to the start
/// of the word or punctuation run before them.
fn word_left(chars: &[char], p: usize, secret: bool) -> usize {
    if secret || p == 0 {
        return 0;
    }
    let cls = classes(chars);
    let mut q = p.min(chars.len());
    while q > 0 && cls[q - 1] == Class::Space {
        q -= 1;
    }
    if q == 0 {
        return 0;
    }
    run(&cls, q - 1).start
}

/// The word a double-click at offset `p` selects (`ExpandSelectionToWord`):
/// a word, one punctuation character or a run of spaces; past the end, the
/// segment before. A password is one word.
fn word_at(chars: &[char], p: usize, secret: bool) -> Range<usize> {
    let n = chars.len();
    if secret || n == 0 {
        return 0..n;
    }
    let cls = classes(chars);
    let i = p.min(n - 1);
    if cls[i] == Class::Punct {
        i..i + 1
    } else {
        run(&cls, i)
    }
}

/// [`word_at`] plus the spaces after it: Windows selects trailing
/// whitespace on a double-click (`ShouldSelectTrailingWhitespace`).
fn word_with_space(chars: &[char], p: usize, secret: bool) -> Range<usize> {
    let w = word_at(chars, p, secret);
    let mut end = w.end;
    while end < chars.len() && chars[end].is_whitespace() {
        end += 1;
    }
    w.start..end
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    fn type_keys(ed: &mut Editor, value: &mut String, events: &[Event], rules: &Rules) -> Edited {
        let ctx = Context::default();
        ed.keys(&ctx, value, events, rules)
    }

    fn key(key: Key, modifiers: Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn ctrl_arrows_move_by_word_like_chromium_on_windows() {
        let c = chars("foo bar, baz.qux  end");
        // Right: past the word and the spaces after it.
        assert_eq!(word_right(&c, 0, false), 4);
        assert_eq!(word_right(&c, 4, false), 7);
        // Punctuation is its own stop, then its spaces go.
        assert_eq!(word_right(&c, 7, false), 9);
        // A period between letters joins them (UAX #29 MidNumLet).
        assert_eq!(word_right(&c, 9, false), 18);
        assert_eq!(word_right(&c, 18, false), 21);
        // Left: back over spaces, to the run's start.
        assert_eq!(word_left(&c, 21, false), 18);
        assert_eq!(word_left(&c, 18, false), 9);
        assert_eq!(word_left(&c, 9, false), 7);
        assert_eq!(word_left(&c, 7, false), 4);
        assert_eq!(word_left(&c, 3, false), 0);
        // A password is one word.
        assert_eq!(word_right(&c, 2, true), c.len());
        assert_eq!(word_left(&c, 9, true), 0);
    }

    #[test]
    fn apostrophes_and_decimal_points_stay_inside_words() {
        let c = chars("don't 3.14 a.");
        assert_eq!(word_right(&c, 0, false), 6);
        assert_eq!(word_right(&c, 6, false), 11);
        assert_eq!(word_at(&c, 7, false), 6..10);
        // A trailing period is punctuation.
        assert_eq!(word_at(&c, 12, false), 12..13);
    }

    #[test]
    fn a_double_click_selects_the_word_and_its_trailing_spaces() {
        let c = chars("alpha  beta");
        assert_eq!(word_with_space(&c, 2, false), 0..7);
        assert_eq!(word_with_space(&c, 9, false), 7..11);
        // Past the end: the last word.
        assert_eq!(word_with_space(&c, 11, false), 7..11);
        assert_eq!(word_with_space(&c, 3, true), 0..11);
    }

    #[test]
    fn pasted_line_breaks_become_spaces_and_trailing_ones_go() {
        assert_eq!(paste_text("a\r\nb\nc\rd\n\n"), "a b c d");
        assert_eq!(paste_text("  cookie  \n"), "  cookie  ");
    }

    #[test]
    fn typing_replaces_the_selection_and_respects_maxlength_in_utf16() {
        let rules = Rules {
            max_len: Some(4),
            ..Rules::text()
        };
        let mut value = "ab".to_string();
        let mut ed = Editor::default();
        ed.sync(&value);
        assert_eq!(ed.caret, 2);
        // "😀" is two UTF-16 units: only one fits after "ab" plus "c".
        let out = type_keys(&mut ed, &mut value, &[Event::Text("c😀d".into())], &rules);
        assert!(out.changed);
        assert_eq!(value, "abc");
        ed.anchor = 0;
        ed.caret = 2;
        type_keys(&mut ed, &mut value, &[Event::Text("xyz".into())], &rules);
        assert_eq!(value, "xyzc");
        assert_eq!((ed.caret, ed.anchor), (3, 3));
    }

    /// The word modifier: Ctrl, Option on macOS.
    const WORD: Modifiers = if cfg!(target_os = "macos") {
        Modifiers::ALT
    } else {
        Modifiers::CTRL
    };

    #[test]
    fn the_caret_moves_inserts_and_deletes_in_the_middle() {
        let rules = Rules::text();
        let mut value = "hello world".to_string();
        let mut ed = Editor::default();
        ed.sync(&value);
        let none = Modifiers::NONE;
        type_keys(
            &mut ed,
            &mut value,
            &[key(Key::ArrowLeft, WORD), key(Key::ArrowLeft, none)],
            &rules,
        );
        assert_eq!(ed.caret, 5);
        type_keys(&mut ed, &mut value, &[Event::Text(",".into())], &rules);
        assert_eq!(value, "hello, world");
        type_keys(&mut ed, &mut value, &[key(Key::Delete, none)], &rules);
        assert_eq!(value, "hello,world");
        // The comma is a word of its own.
        type_keys(&mut ed, &mut value, &[key(Key::Backspace, WORD)], &rules);
        assert_eq!(value, "helloworld");
        assert_eq!(ed.caret, 5);
        // Shift+End selects to the end; typing replaces it.
        type_keys(
            &mut ed,
            &mut value,
            &[key(Key::End, Modifiers::SHIFT), Event::Text("X".into())],
            &rules,
        );
        assert_eq!(value, "helloX");
        // Arrows collapse a selection to its side.
        let mut value = "abcd".to_string();
        let mut ed = Editor::default();
        ed.sync(&value);
        type_keys(
            &mut ed,
            &mut value,
            &[key(Key::A, Modifiers::COMMAND)],
            &rules,
        );
        assert_eq!(ed.selection(), 0..4);
        type_keys(&mut ed, &mut value, &[key(Key::ArrowLeft, none)], &rules);
        assert_eq!((ed.caret, ed.anchor), (0, 0));
    }

    #[test]
    fn typing_runs_undo_as_one_step_and_redo_brings_them_back() {
        let rules = Rules::text();
        let mut value = String::new();
        let mut ed = Editor::default();
        ed.sync(&value);
        for c in ["a", "b", "c"] {
            type_keys(&mut ed, &mut value, &[Event::Text(c.into())], &rules);
        }
        type_keys(
            &mut ed,
            &mut value,
            &[key(Key::ArrowLeft, Modifiers::NONE)],
            &rules,
        );
        type_keys(&mut ed, &mut value, &[Event::Paste("XY".into())], &rules);
        assert_eq!(value, "abXYc");
        let undo = key(Key::Z, Modifiers::COMMAND);
        type_keys(&mut ed, &mut value, std::slice::from_ref(&undo), &rules);
        assert_eq!(value, "abc");
        type_keys(&mut ed, &mut value, &[undo], &rules);
        assert_eq!(value, "");
        type_keys(
            &mut ed,
            &mut value,
            &[key(Key::Y, Modifiers::COMMAND)],
            &rules,
        );
        assert_eq!(value, "abc");
    }

    #[test]
    fn a_password_never_reaches_the_clipboard() {
        let ctx = Context::default();
        let secret = Rules {
            secret: true,
            ..Rules::text()
        };
        let mut value = "hunter2".to_string();
        let mut ed = Editor::default();
        ed.sync(&value);
        ed.select_all(&value);
        let out = ed.keys(&ctx, &mut value, &[Event::Cut], &secret);
        assert!(!out.changed);
        assert_eq!(value, "hunter2");
        let out = ed.keys(&ctx, &mut value, &[Event::Cut], &Rules::text());
        assert!(out.changed);
        assert_eq!(value, "");
    }

    #[test]
    fn a_value_set_from_outside_moves_the_caret_to_its_end() {
        let mut ed = Editor::default();
        ed.sync("abc");
        ed.caret = 1;
        ed.anchor = 1;
        ed.known = Some("abc".into());
        ed.sync("abc");
        assert_eq!(ed.caret, 1);
        ed.sync("");
        assert_eq!((ed.caret, ed.anchor), (0, 0));
    }

    #[test]
    fn the_number_filter_applies_to_pasted_text() {
        let rules = Rules {
            accepts: |c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E'),
            ..Rules::text()
        };
        let mut value = String::new();
        let mut ed = Editor::default();
        ed.sync(&value);
        type_keys(&mut ed, &mut value, &[Event::Paste("1 2a3".into())], &rules);
        assert_eq!(value, "123");
    }

    /// Two rows of four 10 px characters, the first wrapped after "abc ",
    /// then a line break and "de".
    fn wrapped_rows() -> Rows {
        let row = |start: usize, n: usize, newline: bool| Row {
            start,
            xs: (0..=n).map(|i| i as f32 * 10.0).collect(),
            newline,
        };
        Rows {
            rows: vec![row(0, 4, false), row(4, 3, true), row(8, 2, false)],
            top: 0.0,
            height: 16.0,
            page: 0.0,
            preedit: 0..0,
        }
    }

    #[test]
    fn a_textarea_moves_by_row_and_keeps_its_column() {
        // "abc defg\nde": "abc " wraps, "efg" ends at the break.
        let mut value = "abc efg\nde".to_string();
        let rows = wrapped_rows();
        let ctx = Context::default();
        let rules = Rules::area(100);
        let mut ed = Editor::default();
        ed.sync(&value);
        ed.caret = 2;
        ed.anchor = 2;
        let down = key(Key::ArrowDown, Modifiers::NONE);
        ed.area_keys(&ctx, &mut value, std::slice::from_ref(&down), &rules, &rows);
        assert_eq!(ed.caret, 6);
        // The last row is shorter: its end, and the column comes back.
        ed.area_keys(&ctx, &mut value, std::slice::from_ref(&down), &rules, &rows);
        assert_eq!(ed.caret, 10);
        let up = key(Key::ArrowUp, Modifiers::NONE);
        ed.area_keys(&ctx, &mut value, std::slice::from_ref(&up), &rules, &rows);
        assert_eq!(ed.caret, 6);
        // Past the first row: the start.
        ed.area_keys(&ctx, &mut value, &[up.clone(), up], &rules, &rows);
        assert_eq!(ed.caret, 0);
        // End on the wrapped row stays on it, after its space.
        ed.area_keys(
            &ctx,
            &mut value,
            &[key(Key::End, Modifiers::NONE)],
            &rules,
            &rows,
        );
        assert_eq!((ed.caret, ed.upstream), (4, true));
        assert_eq!(rows.locate(ed.caret, ed.upstream), (0, 40.0));
        assert_eq!(rows.locate(ed.caret, false), (1, 0.0));
        // Ctrl+End: the text's end.
        ed.area_keys(
            &ctx,
            &mut value,
            &[key(Key::End, Modifiers::COMMAND)],
            &rules,
            &rows,
        );
        assert_eq!(ed.caret, 10);
    }

    #[test]
    fn a_textarea_pages_by_whole_rows() {
        // Ten rows "ab\n" of 16 points; a 40 point page reaches two rows.
        let mut value = "ab\n".repeat(9) + "ab";
        let row = |i: usize| Row {
            start: i * 3,
            xs: vec![0.0, 10.0, 20.0],
            newline: i < 9,
        };
        let rows = Rows {
            rows: (0..10).map(row).collect(),
            top: 0.0,
            height: 16.0,
            page: 0.0,
            preedit: 0..0,
        }
        .paging(46.0, 1.0);
        assert_eq!(rows.page, 40.0);
        let ctx = Context::default();
        let rules = Rules::area(100);
        let mut ed = Editor::default();
        ed.sync(&value);
        ed.caret = 1;
        ed.anchor = 1;
        let down = key(Key::PageDown, Modifiers::NONE);
        let out = ed.area_keys(&ctx, &mut value, std::slice::from_ref(&down), &rules, &rows);
        assert!(out.page && !out.changed);
        assert_eq!((ed.caret, ed.anchor), (7, 7));
        // Shift extends and does not align the line.
        let out = ed.area_keys(
            &ctx,
            &mut value,
            &[key(Key::PageDown, Modifiers::SHIFT)],
            &rules,
            &rows,
        );
        assert!(!out.page);
        assert_eq!((ed.anchor, ed.caret), (7, 13));
        // From row 8 the last row is in reach: the end.
        ed.caret = 25;
        ed.anchor = 25;
        ed.goal_x = None;
        ed.area_keys(&ctx, &mut value, std::slice::from_ref(&down), &rules, &rows);
        assert_eq!(ed.caret, 29);
        let up = key(Key::PageUp, Modifiers::NONE);
        ed.area_keys(&ctx, &mut value, std::slice::from_ref(&up), &rules, &rows);
        // The column from before the end comes back.
        assert_eq!(ed.caret, 22);
        ed.area_keys(
            &ctx,
            &mut value,
            &[up.clone(), up.clone(), up.clone()],
            &rules,
            &rows,
        );
        assert_eq!(ed.caret, 4);
        ed.area_keys(&ctx, &mut value, std::slice::from_ref(&up), &rules, &rows);
        assert_eq!(ed.caret, 0);
    }

    #[test]
    fn a_textarea_breaks_lines_and_keeps_pasted_ones() {
        let ctx = Context::default();
        let rules = Rules::area(12);
        let mut value = "a".to_string();
        let mut ed = Editor::default();
        ed.sync(&value);
        let out = ed.keys(
            &ctx,
            &mut value,
            &[
                key(Key::Enter, Modifiers::NONE),
                Event::Paste("b\r\nc\rd\n".into()),
            ],
            &rules,
        );
        assert!(out.changed && !out.enter);
        assert_eq!(value, "a\nb\nc\nd\n");
        // maxlength counts the breaks.
        ed.keys(&ctx, &mut value, &[Event::Paste("efghij".into())], &rules);
        assert_eq!(value, "a\nb\nc\nd\nefgh");
    }

    #[test]
    fn a_textarea_hit_and_selection_follow_the_rows() {
        let rows = wrapped_rows();
        assert_eq!(rows.hit(pos2(14.0, 3.0)), (1, false));
        // Past a wrapped row's end: after its space, on that row.
        assert_eq!(rows.hit(pos2(90.0, 3.0)), (4, true));
        assert_eq!(rows.hit(pos2(90.0, 20.0)), (7, false));
        assert_eq!(rows.hit(pos2(5.0, 99.0)), (8, false));
        // A selection over the break paints it as `newline` wide.
        assert_eq!(
            rows.spans(2..9, 3.0),
            vec![(0, 20.0, 40.0), (1, 0.0, 33.0), (2, 0.0, 10.0)]
        );
        assert_eq!(paragraph(&chars("ab\ncd\nef"), 4), 3..6);
        assert_eq!(paragraph(&chars("ab\ncd\nef"), 8), 6..8);
    }

    #[test]
    fn the_nearest_boundary_wins() {
        let offsets = [0.0, 10.0, 20.0, 35.0];
        assert_eq!(nearest(&offsets, -5.0), 0);
        assert_eq!(nearest(&offsets, 4.9), 0);
        assert_eq!(nearest(&offsets, 5.1), 1);
        assert_eq!(nearest(&offsets, 28.0), 3);
        assert_eq!(nearest(&offsets, 99.0), 3);
    }
}
