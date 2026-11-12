//! Where the grid puts its boxes: the flat grid and the folder sections, in
//! content coordinates (relative to the content box top left, unscrolled),
//! and the slides that move cards while a drag reorders them.

use std::collections::HashMap;

use accshift_app::display::{
    AccountCardItem, DisplayItem, DisplayLists, FolderCardItem, SectionHeaderItem, SectionLabel,
};
use accshift_app::folders::{FolderSection, ItemKind, ItemRef};
use egui::{pos2, vec2, Context, Rect, Vec2};

use super::Workspace;
use crate::ui::anim::Motion;
use crate::ui::components::use_grid_layout::GAP;

/// Gap between two folder sections.
const SECTIONS_GAP: f32 = 22.0;
/// Gap between a section header and its grid.
const SECTION_GAP: f32 = 8.0;
/// How long a section grid takes to slide open or shut (cubic ease out).
const SLIDE: f64 = 0.22;
/// How long a card slides to its new place while a card is dragged.
const FLIP: f64 = 0.2;
/// Above this many rendered cards, cards jump instead of sliding.
const FLIP_MAX_CARDS: usize = 100;

/// What a laid out box holds.
pub(super) enum Node<'a> {
    Back,
    EmptyFolder,
    Header(&'a SectionHeaderItem),
    Folder(&'a FolderCardItem),
    Account(&'a AccountCardItem),
}

impl Node<'_> {
    /// The card's name, as `DisplayItem::label` gives it.
    #[cfg(test)]
    pub(super) fn label(&self) -> String {
        match self {
            Node::Back => "Back".into(),
            Node::EmptyFolder => "empty".into(),
            Node::Header(h) => match &h.label {
                SectionLabel::Folder(f) => f.name.clone(),
                SectionLabel::Root => "Root".into(),
            },
            Node::Folder(f) => f.folder.name.clone(),
            Node::Account(a) => a.name.clone(),
        }
    }

    /// The folder or account the card shows; none for the other boxes.
    fn card(&self) -> Option<(ItemKind, &str)> {
        match self {
            Node::Folder(f) => Some((ItemKind::Folder, f.folder.id.as_str())),
            Node::Account(a) => Some((ItemKind::Account, a.account.id.as_str())),
            _ => None,
        }
    }

    /// The id of the folder or account the card shows.
    pub(super) fn card_id(&self) -> Option<&str> {
        self.card().map(|(_, id)| id)
    }

    /// The folder or account the card shows.
    pub(super) fn item_ref(&self) -> Option<ItemRef> {
        self.card().map(|(kind, id)| ItemRef {
            kind,
            id: id.to_string(),
        })
    }

    /// Whether the card shows `item`.
    pub(super) fn shows(&self, item: &ItemRef) -> bool {
        self.card() == Some((item.kind, item.id.as_str()))
    }

    /// Whether the card is the one being dragged; none for the boxes that
    /// cannot be dragged.
    fn dragged(&self) -> Option<bool> {
        match self {
            Node::Folder(f) => Some(f.dragged),
            Node::Account(a) => Some(a.dragged),
            _ => None,
        }
    }
}

pub(super) struct Placed<'a> {
    pub key: String,
    /// Layout box relative to the content box top left, unscrolled.
    pub rect: Rect,
    /// Clip of a sliding section grid, same space.
    pub clip: Option<Rect>,
    pub opacity: f32,
    pub node: Node<'a>,
    /// Cards per row of its grid (1 off the grids).
    pub per_row: usize,
}

impl Placed<'_> {
    /// Whether the box, cut to its slide clip, comes within 32 px of
    /// `view`, and its clip is not shut.
    pub(super) fn visible_in(&self, view: Rect) -> bool {
        self.clip
            .map_or(self.rect, |c| self.rect.intersect(c))
            .expand(32.0)
            .intersects(view)
            && self.clip.is_none_or(|c| c.height() > 0.0)
    }
}

/// What one frame lays out: the boxes in paint order, and each folder
/// section's box with its folder (`None` for the root section).
#[derive(Default)]
pub(super) struct Laid<'a> {
    pub placed: Vec<Placed<'a>>,
    pub sections: Vec<(Option<String>, Rect)>,
}

/// The column a grid fills: its left edge and width, and each card's size.
pub(super) struct Grid<'s> {
    pub left: f32,
    pub width: f32,
    pub size: &'s dyn Fn(&DisplayItem) -> Vec2,
}

/// A section grid entering or leaving.
pub(super) struct Slide {
    start: f64,
    intro: bool,
    /// The cards of a leaving section, drawn until it is gone.
    items: Vec<DisplayItem>,
}

/// The sections' state across frames: which were collapsed last frame,
/// and the grids sliding open or shut.
#[derive(Default)]
pub(super) struct Folds {
    collapsed: HashMap<String, bool>,
    pub slides: HashMap<String, Slide>,
}

impl Folds {
    pub(super) fn clear(&mut self) {
        self.slides.clear();
        self.collapsed.clear();
    }

    /// Starts a slide for each section whose collapse changed since last
    /// frame (full motion only), and drops the slides that ended or whose
    /// section is gone.
    fn update(&mut self, parts: &[Section], previous: &[DisplayItem], motion: Motion, now: f64) {
        let mut seen = HashMap::new();
        for part in parts {
            let Some(h) = part.header else { continue };
            let was = self.collapsed.get(&part.key).copied();
            seen.insert(part.key.clone(), h.collapsed);
            if motion != Motion::Full {
                continue;
            }
            let slide = match (was, h.collapsed) {
                (Some(false), true) => {
                    let items = section_items(previous, &part.key);
                    if items.is_empty() {
                        continue;
                    }
                    Slide {
                        start: now,
                        intro: false,
                        items,
                    }
                }
                (Some(true), false) => Slide {
                    start: now,
                    intro: true,
                    items: Vec::new(),
                },
                _ => continue,
            };
            self.slides.insert(part.key.clone(), slide);
        }
        self.collapsed = seen;
        self.slides
            .retain(|key, slide| now - slide.start < SLIDE && parts.iter().any(|p| &p.key == key));
    }
}

/// One section of the sections layout.
struct Section<'a> {
    key: String,
    header: Option<&'a SectionHeaderItem>,
    items: Vec<&'a DisplayItem>,
}

pub(super) fn cubic_out(t: f64) -> f64 {
    let f = t - 1.0;
    f * f * f + 1.0
}

/// Lays out `items` as a wrapping row grid from `top`, appending to `out`.
/// Returns the grid height. A line is as tall as its tallest card and each
/// card keeps its own height.
pub(super) fn flow<'a>(
    out: &mut Vec<Placed<'a>>,
    items: &[&'a DisplayItem],
    prefix: &str,
    grid: &Grid,
    top: f32,
) -> f32 {
    let cards: Vec<(Node<'a>, String, Vec2)> = items
        .iter()
        .filter_map(|&item| {
            let (node, key) = match item {
                DisplayItem::Back => (Node::Back, format!("{prefix}/back")),
                DisplayItem::Folder(f) => (Node::Folder(f), format!("{prefix}/f/{}", f.folder.id)),
                DisplayItem::Account(a) => {
                    (Node::Account(a), format!("{prefix}/a/{}", a.account.id))
                }
                DisplayItem::SectionHeader(_) => return None,
            };
            Some((node, key, (grid.size)(item)))
        })
        .collect();
    let Some(card_w) = cards.first().map(|(_, _, s)| s.x) else {
        return 0.0;
    };
    let per_row = (((grid.width + GAP) / (card_w + GAP)).floor() as usize).max(1);
    let mut y = top;
    let mut cards = cards.into_iter().peekable();
    while cards.peek().is_some() {
        let row: Vec<_> = cards.by_ref().take(per_row).collect();
        let line = row.iter().map(|(_, _, s)| s.y).fold(0.0, f32::max);
        for (col, (node, key, card)) in row.into_iter().enumerate() {
            out.push(Placed {
                key,
                rect: Rect::from_min_size(pos2(grid.left + col as f32 * (card.x + GAP), y), card),
                clip: None,
                opacity: 1.0,
                node,
                per_row,
            });
        }
        y += line + GAP;
    }
    y - GAP - top
}

/// Splits the view's items on their headers, following the pipeline's
/// sections (a root section with nothing in it has no header).
fn split_sections<'a>(sections: &[FolderSection], items: &'a [DisplayItem]) -> Vec<Section<'a>> {
    let mut it = items.iter().peekable();
    let mut parts = Vec::new();
    for section in sections {
        let count = section.folder_items.len() + section.account_items.len();
        let header = if section.folder.is_some() || count > 0 {
            match it.next() {
                Some(DisplayItem::SectionHeader(h)) => Some(h),
                _ => None,
            }
        } else {
            None
        };
        let mut cards = Vec::new();
        while let Some(item) = it.next_if(|item| !matches!(item, DisplayItem::SectionHeader(_))) {
            cards.push(item);
        }
        parts.push(Section {
            key: accshift_app::display::section_collapse_key(section).to_string(),
            header,
            items: cards,
        });
    }
    parts
}

/// The sections layout: per section its header, then (unless collapsed)
/// its grid 8 px under it; sections 22 px apart. Collapse changes start
/// slides in `folds`. Returns the height.
#[allow(clippy::too_many_arguments)]
pub(super) fn lay_sections<'a>(
    laid: &mut Laid<'a>,
    folds: &'a mut Folds,
    sections: &[FolderSection],
    items: &'a [DisplayItem],
    previous: &[DisplayItem],
    prefix: &str,
    grid: &Grid,
    top: f32,
    motion: Motion,
    now: f64,
) -> f32 {
    let parts = split_sections(sections, items);
    folds.update(&parts, previous, motion, now);
    let slides: &'a HashMap<String, Slide> = &folds.slides;

    let mut y = top;
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            y += SECTIONS_GAP;
        }
        let section_prefix = format!("{prefix}s/{}", part.key);
        let section_top = y;
        let folder = part.header.and_then(|h| match &h.label {
            SectionLabel::Folder(f) => Some(f.id.clone()),
            SectionLabel::Root => None,
        });
        if let Some(h) = part.header {
            let height = if h.label == SectionLabel::Root {
                23.0
            } else {
                24.0
            };
            laid.placed.push(Placed {
                key: format!("{section_prefix}/header"),
                rect: Rect::from_min_size(pos2(grid.left, y), vec2(grid.width, height)),
                clip: None,
                opacity: 1.0,
                node: Node::Header(h),
                per_row: 1,
            });
            y += height;
        }
        let slide = slides.get(&part.key);
        let leaving: Vec<&'a DisplayItem>;
        let cards: &[&'a DisplayItem] = match slide {
            Some(s) if !s.intro => {
                leaving = s.items.iter().collect();
                &leaving
            }
            _ => &part.items,
        };
        if !cards.is_empty() {
            if part.header.is_some() {
                y += SECTION_GAP;
            }
            y += lay_section_grid(
                &mut laid.placed,
                cards,
                &section_prefix,
                grid,
                y,
                slide,
                now,
            );
        }
        laid.sections.push((
            folder,
            Rect::from_min_size(
                pos2(grid.left, section_top),
                vec2(grid.width, y - section_top),
            ),
        ));
    }
    y - top
}

/// One section's grid at full height, then clipped to its slide's height
/// and faded in its first twentieth. Returns the height it takes.
fn lay_section_grid<'a>(
    out: &mut Vec<Placed<'a>>,
    cards: &[&'a DisplayItem],
    prefix: &str,
    grid: &Grid,
    top: f32,
    slide: Option<&Slide>,
    now: f64,
) -> f32 {
    let start = out.len();
    let full = flow(out, cards, prefix, grid, top);
    let Some(slide) = slide else {
        return full;
    };
    let eased = cubic_out(((now - slide.start) / SLIDE).clamp(0.0, 1.0));
    let factor = (if slide.intro { eased } else { 1.0 - eased }) as f32;
    let clip = Rect::from_min_size(
        pos2(grid.left - 64.0, top),
        vec2(grid.width + 128.0, full * factor),
    );
    for p in &mut out[start..] {
        p.clip = Some(clip);
        p.opacity = (factor * 20.0).min(1.0);
    }
    full * factor
}

/// The cards of section `key` in a previous frame's items.
pub(super) fn section_items(items: &[DisplayItem], key: &str) -> Vec<DisplayItem> {
    let mut out = Vec::new();
    let mut inside = false;
    for item in items {
        match item {
            DisplayItem::SectionHeader(h) => {
                if inside {
                    break;
                }
                inside = h.collapse_key == key;
            }
            other if inside => out.push(other.clone()),
            _ => {}
        }
    }
    out
}

/// Whether cards slide to their new place: while a card is dragged and at
/// most [`FLIP_MAX_CARDS`] render.
pub(super) fn flips_on(lists: &DisplayLists, dragging: bool) -> bool {
    let rendered: usize = match &lists.sections {
        Some(sections) => sections
            .iter()
            .map(|s| s.folder_items.len() + s.account_items.len())
            .sum(),
        None => lists.folder_items.len() + lists.account_items.len(),
    };
    dragging && rendered <= FLIP_MAX_CARDS
}

impl Workspace {
    /// A card the drag preview moves slides from where it showed last
    /// frame to its new box; the dragged card jumps. Without `flip` every
    /// card jumps.
    pub(super) fn animate_flips(
        &mut self,
        ctx: &Context,
        placed: &mut [Placed],
        flip: bool,
        now: f64,
    ) {
        let mut boxes = HashMap::new();
        for p in placed {
            let Some(dragged) = p.node.dragged() else {
                continue;
            };
            let layout = p.rect.min;
            if let Some((was, shown)) = self.last_boxes.get(&p.key).copied() {
                if was != layout {
                    if flip && !dragged {
                        self.flips.insert(p.key.clone(), (shown - layout, now));
                    } else {
                        self.flips.remove(&p.key);
                    }
                }
            }
            if let Some((from, start)) = self.flips.get(&p.key).copied() {
                let t = ((now - start) / FLIP).clamp(0.0, 1.0);
                if t >= 1.0 {
                    self.flips.remove(&p.key);
                } else {
                    p.rect = p.rect.translate(from * (1.0 - cubic_out(t) as f32));
                    ctx.request_repaint();
                }
            }
            boxes.insert(p.key.clone(), (layout, p.rect.min));
        }
        self.last_boxes = boxes;
    }
}
