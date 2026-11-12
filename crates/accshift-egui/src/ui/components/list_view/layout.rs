//! The rows panel's column: the items grouped into boxes, then every box
//! placed in content coordinates, 2 px apart.

use std::collections::HashMap;

use accshift_app::display::{DisplayItem, SectionHeaderItem, SectionLabel};

use super::motion::Slide;
use super::row::row_height;
use super::{GAP, PAD};
use crate::app::vars::Vars;

/// A box of the panel's column.
pub(super) enum Block<'a> {
    /// A row of the flat layout.
    Row(&'a DisplayItem),
    Section(Section<'a>),
}

/// A section: its header, 2 px, then its rows unless it is folded. Its
/// rows slide when it folds or unfolds.
pub(super) struct Section<'a> {
    pub(super) key: String,
    /// None for the root section added with no header.
    pub(super) header: Option<&'a DisplayItem>,
    pub(super) collapsed: bool,
    pub(super) rows: Vec<&'a DisplayItem>,
}

impl Section<'_> {
    /// The folder the section shows, none for the root.
    fn folder(&self) -> Option<String> {
        match self.header {
            Some(DisplayItem::SectionHeader(SectionHeaderItem {
                label: SectionLabel::Folder(f),
                ..
            })) => Some(f.id.clone()),
            _ => None,
        }
    }
}

/// A header or row placed in the column, in content coordinates.
pub(super) struct Place<'a> {
    pub(super) item: &'a DisplayItem,
    pub(super) top: f32,
    pub(super) h: f32,
    /// The sliding rows box it shows through, top to bottom.
    pub(super) clip: Option<(f32, f32)>,
    pub(super) opacity: f32,
}

impl<'a> Place<'a> {
    fn new(item: &'a DisplayItem, top: f32, h: f32) -> Self {
        Place {
            item,
            top,
            h,
            clip: None,
            opacity: 1.0,
        }
    }
}

/// Groups the items into the column's boxes. In the sections layout every
/// item after a header belongs to its section. The root section comes last
/// and is there even with nothing in it (with no header then).
pub(super) fn blocks(items: &[DisplayItem]) -> Vec<Block<'_>> {
    let mut blocks: Vec<Block> = Vec::new();
    for item in items {
        match (item, blocks.last_mut()) {
            (DisplayItem::SectionHeader(h), _) => blocks.push(Block::Section(Section {
                key: h.collapse_key.clone(),
                header: Some(item),
                collapsed: h.collapsed,
                rows: Vec::new(),
            })),
            (_, Some(Block::Section(section))) => section.rows.push(item),
            _ => blocks.push(Block::Row(item)),
        }
    }
    let root_header = items.iter().any(|item| {
        matches!(item, DisplayItem::SectionHeader(h) if matches!(h.label, SectionLabel::Root))
    });
    if blocks.iter().any(|b| matches!(b, Block::Section(_))) && !root_header {
        blocks.push(Block::Section(Section {
            key: String::new(),
            header: None,
            collapsed: false,
            rows: Vec::new(),
        }));
    }
    blocks
}

/// A box's height. A section header is its folder icon (16) or its count
/// pill (15), plus 4 px above and below.
fn box_h(item: &DisplayItem, v: &Vars, ppp: f32) -> f32 {
    match item {
        DisplayItem::SectionHeader(h) if matches!(h.label, SectionLabel::Folder(_)) => 24.0,
        DisplayItem::SectionHeader(_) => 23.0,
        DisplayItem::Account(a) => row_height(v, Some(a), ppp),
        _ => row_height(v, None, ppp),
    }
}

/// The rows of `group` stacked 2 px apart.
fn full_h(group: &[&DisplayItem], box_h: &dyn Fn(&DisplayItem) -> f32) -> f32 {
    let rows: f32 = group.iter().map(|item| box_h(item)).sum();
    rows + GAP * group.len().saturating_sub(1) as f32
}

/// The column laid out.
pub(super) struct Column<'a> {
    pub(super) places: Vec<Place<'a>>,
    /// The section boxes: their folder (none for the root's), top and bottom.
    pub(super) wrappers: Vec<(Option<String>, f32, f32)>,
    /// The bottom of the last box.
    pub(super) bottom: f32,
}

/// Places every box of `blocks`, the sliding sections at `now`.
pub(super) fn column<'a>(
    blocks: &[Block<'a>],
    slides: &'a HashMap<String, Slide>,
    v: &Vars,
    ppp: f32,
    now: f64,
) -> Column<'a> {
    let box_h = |item: &DisplayItem| box_h(item, v, ppp);
    let mut column = Column {
        places: Vec::new(),
        wrappers: Vec::new(),
        bottom: PAD,
    };
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            column.bottom += GAP;
        }
        match block {
            Block::Row(item) => column.push(item, box_h(item)),
            Block::Section(section) => {
                let top = column.bottom;
                if let Some(item) = section.header {
                    column.push(item, box_h(item));
                }
                column.section_rows(section, slides.get(&section.key), now, &box_h);
                column.wrappers.push((section.folder(), top, column.bottom));
            }
        }
    }
    column
}

impl<'a> Column<'a> {
    /// Places `item`, `h` tall, at the bottom.
    fn push(&mut self, item: &'a DisplayItem, h: f32) {
        self.places.push(Place::new(item, self.bottom, h));
        self.bottom += h;
    }

    /// Places a section's rows below its header. A folding section draws
    /// the rows it had until it is gone; while sliding, the rows show
    /// through the section's sliding height and fade in over its first 5 %.
    fn section_rows(
        &mut self,
        section: &Section<'a>,
        slide: Option<&'a Slide>,
        now: f64,
        box_h: &dyn Fn(&DisplayItem) -> f32,
    ) {
        let leaving: Vec<&DisplayItem>;
        let group: &[&DisplayItem] = match slide {
            Some(s) if !s.intro => {
                leaving = s.rows.iter().collect();
                &leaving
            }
            _ if section.collapsed => return,
            _ => &section.rows,
        };
        if section.header.is_some() {
            self.bottom += GAP;
        }
        let factor = slide.map_or(1.0, |s| s.factor(now));
        let top = self.bottom;
        let clip = slide.map(|_| (top, top + full_h(group, box_h) * factor));
        let mut full = 0.0;
        for (k, item) in group.iter().enumerate() {
            if k > 0 {
                full += GAP;
            }
            let h = box_h(item);
            let mut place = Place::new(item, top + full, h);
            if clip.is_some() {
                place.clip = clip;
                place.opacity = (factor * 20.0).min(1.0);
            }
            self.places.push(place);
            full += h;
        }
        self.bottom += full * factor;
    }
}
