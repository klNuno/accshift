//! Single-line [`View`] and multi-line [`Rows`] layout and hit-testing.

use std::ops::Range;

use egui::{Context, Pos2, Rect};

use crate::ui::text::{self, TextAlign, TextStyle};

use super::{byte, Editor};

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

impl Editor {
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
    pub(super) rows: Vec<Row>,
    /// The first row's top, in points.
    pub(super) top: f32,
    /// The line box's height, in points.
    pub(super) height: f32,
    /// How far PageUp and PageDown move the caret, in points.
    pub(super) page: f32,
    /// The composition, in the laid out text: hits inside it go to its
    /// start, after it they move back by its length.
    pub(super) preedit: Range<usize>,
}

#[derive(Clone, Debug)]
pub(super) struct Row {
    /// Its first character in the laid out text.
    pub(super) start: usize,
    /// The x of each boundary, in points: one more than its characters
    /// (a wrapped row keeps the space it broke at).
    pub(super) xs: Vec<f32>,
    /// A line break ends it, rather than a wrap.
    pub(super) newline: bool,
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

    pub(super) fn start(&self, row: usize) -> usize {
        self.rows[row].start
    }

    /// The offset after row `row`'s last character, and whether the caret
    /// stays on the row there (a wrapped row, not the last).
    pub(super) fn end(&self, row: usize) -> (usize, bool) {
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
    pub(super) fn at(&self, row: usize, x: f32) -> (usize, bool) {
        let r = &self.rows[row];
        let i = nearest(&r.xs, x);
        let wrapped = !r.newline && row + 1 < self.rows.len();
        (r.start + i, wrapped && i == r.xs.len() - 1)
    }

    /// The value offset at `at`: the row under it (the first above the
    /// text, the last below it), then the nearest boundary.
    pub(super) fn hit(&self, at: Pos2) -> (usize, bool) {
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
pub(super) fn hit(layout: &View, x: f32) -> usize {
    let x = x - layout.line.left();
    unshift(nearest(&layout.offsets, x), &layout.preedit)
}

/// The x of every character boundary of `text` laid out in `font`, in CSS
/// px from its start; the last is its width.
fn offsets(ctx: &Context, text: &str, font: &TextStyle) -> Vec<f32> {
    let n = text.chars().count();
    text::offsets(ctx, text, font).unwrap_or_else(|| vec![0.0; n + 1])
}

/// The boundary in `offsets` nearest to `x`.
pub(super) fn nearest(offsets: &[f32], x: f32) -> usize {
    let mut best = 0;
    for (i, o) in offsets.iter().enumerate() {
        if (o - x).abs() < (offsets[best] - x).abs() {
            best = i;
        }
    }
    best
}
