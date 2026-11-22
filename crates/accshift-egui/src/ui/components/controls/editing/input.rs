//! Pointer tracking and keyboard / IME / clipboard input handling for [`Editor`].

use egui::{Context, Event, ImeEvent, Key, Modifiers, Pos2, Response, Ui};

use super::layout::hit;
use super::words::{paragraph, word_at, word_left, word_right, word_with_space};
use super::{
    paste_text, slice, Edited, Editor, Rows, Rules, Step, Unit, View, MULTI_CLICK_SLOP,
    MULTI_CLICK_TIME,
};

impl Editor {
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
}
