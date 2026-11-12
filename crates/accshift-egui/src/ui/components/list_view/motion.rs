//! The column's motion: a section's rows slide when it folds or unfolds,
//! and in the flat layout a row a drag moved slides from where it showed.

use std::collections::HashMap;

use accshift_app::display::DisplayItem;

use super::layout::{Block, Place};
use super::{dragged, key_of, ListState, FLIP, SLIDE};
use crate::ui::anim::{self, Motion};

/// A section's rows folding or unfolding over 220 ms, easing out.
pub(super) struct Slide {
    start: f64,
    pub(super) intro: bool,
    /// The rows of a folding section, drawn until it is gone.
    pub(super) rows: Vec<DisplayItem>,
}

impl Slide {
    /// How much of the rows' height shows at `now`, 0 to 1.
    pub(super) fn factor(&self, now: f64) -> f32 {
        let e = cubic_out(((now - self.start) / SLIDE).clamp(0.0, 1.0)) as f32;
        if self.intro {
            e
        } else {
            1.0 - e
        }
    }
}

fn cubic_out(t: f64) -> f64 {
    let f = t - 1.0;
    f * f * f + 1.0
}

/// Starts a slide for each section that folded or unfolded since the last
/// frame (under full motion), remembers every section's state, and drops
/// the slides that ended or whose section left.
pub(super) fn update_slides(
    ctx: &egui::Context,
    state: &mut ListState,
    blocks: &[Block],
    now: f64,
) -> HashMap<String, Slide> {
    let mut slides = std::mem::take(&mut state.slides);
    let mut seen = HashMap::new();
    for block in blocks {
        let Block::Section(section) = block else {
            continue;
        };
        if section.header.is_none() {
            continue;
        }
        let (key, collapsed) = (&section.key, section.collapsed);
        if anim::motion(ctx) == Motion::Full {
            // A folding section keeps the rows it had to slide them out.
            let slide = match state.sections.get(key) {
                Some((false, cached)) if collapsed => Some((false, cached.clone())),
                Some((true, _)) if !collapsed => Some((true, Vec::new())),
                _ => None,
            };
            if let Some((intro, rows)) = slide {
                let start = now;
                slides.insert(key.clone(), Slide { start, intro, rows });
            }
        }
        let cached = if collapsed {
            Vec::new()
        } else {
            section.rows.iter().map(|item| (**item).clone()).collect()
        };
        seen.insert(key.clone(), (collapsed, cached));
    }
    state.sections = seen;
    slides.retain(|key, slide| now - slide.start < SLIDE && state.sections.contains_key(key));
    if !slides.is_empty() {
        ctx.request_repaint();
    }
    slides
}

/// In the flat layout, a row whose place changed slides from where it
/// showed, over 200 ms easing out, while a drag is under way and at most
/// 100 rows render. The dragged row itself jumps.
pub(super) fn flip(
    ctx: &egui::Context,
    state: &mut ListState,
    places: &mut [Place],
    dragging: bool,
    now: f64,
) {
    let rendered = places.iter().filter(|p| dragged(p.item).is_some()).count();
    let flip = dragging && rendered <= 100;
    let mut tops = HashMap::new();
    for place in places.iter_mut() {
        let Some(dragged) = dragged(place.item) else {
            continue;
        };
        let key = key_of(place.item);
        let layout = place.top;
        let last = state.last_tops.get(&key).copied();
        if let Some((_, shown)) = last.filter(|(was, _)| *was != layout) {
            if flip && !dragged {
                state.flips.insert(key.clone(), (shown - layout, now));
            } else {
                state.flips.remove(&key);
            }
        }
        if let Some((from, start)) = state.flips.get(&key).copied() {
            let t = ((now - start) / FLIP).clamp(0.0, 1.0);
            if t >= 1.0 {
                state.flips.remove(&key);
            } else {
                place.top += from * (1.0 - cubic_out(t) as f32);
                ctx.request_repaint();
            }
        }
        tops.insert(key, (layout, place.top));
    }
    state.last_tops = tops;
}
