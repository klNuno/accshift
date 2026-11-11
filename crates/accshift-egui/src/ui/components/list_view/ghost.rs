//! The drag ghosts: a clone of the dragged row, or of a dragged folder
//! section with its header and rows, that follows the pointer over
//! everything, scaled 1.05 at 85 % opacity.

use accshift_app::display::DisplayItem;
use accshift_app::folders::ItemKind;
use egui::{vec2, Id, Pos2, Rect};

use super::rows::{draw_item, ItemDraw, Pass};
use super::{ListHost, ListProps, RowGhost};
use crate::ui::components::{account_card, drag_ghost};
use crate::ui::paint::BoxStyle;

/// A box the rows pass drew, kept to draw again in a clone.
#[derive(Clone)]
pub(super) struct GhostPart<'a> {
    pub(super) item: &'a DisplayItem,
    /// Its box on screen.
    pub(super) rect: Rect,
    /// Its mount id in the list, which its avatar animates under.
    pub(super) source: Id,
    /// Its mount key.
    pub(super) key: String,
    pub(super) kb_focus: bool,
}

/// The box of the dragged folder's section on screen, when the drag holds
/// a folder shown as a section.
pub(super) fn dragged_section(
    props: &ListProps,
    section_boxes: &[(Option<String>, Rect)],
) -> Option<Rect> {
    let g = props.ghost.as_ref()?;
    if g.item.kind != ItemKind::Folder {
        return None;
    }
    section_boxes
        .iter()
        .find(|(folder, _)| folder.as_deref() == Some(g.item.id.as_str()))
        .map(|(_, r)| *r)
}

/// A dragged row's clone, `h` tall, where the pointer took it: its own
/// shadow, never hovered. A row has no entrance animation to replay.
pub(super) fn ghost_row(
    ctx: &egui::Context,
    g: &RowGhost,
    part: &GhostPart,
    h: f32,
    selected: Option<&str>,
    props: &ListProps,
    host: &mut dyn ListHost,
) {
    let r = Rect::from_min_size(g.rect.min + g.offset, vec2(g.rect.width(), h));
    drag_ghost(ctx, r, 1.05, 0.85, |gui, r| {
        let draw = ItemDraw {
            item: part.item,
            rect: r,
            id: g.id,
            avatar_id: part.source,
            key: &part.key,
            kb_focus: part.kb_focus,
            selected,
            pass: Pass::Ghost { shadow: true },
        };
        draw_item(gui, &draw, props, host);
    });
}

/// A dragged section's clone: the section box's shadow around no
/// background, then its header and rows where they showed relative to
/// `origin`, the section's corner.
pub(super) fn ghost_section(
    ctx: &egui::Context,
    g: &RowGhost,
    parts: &[GhostPart],
    origin: Pos2,
    selected: Option<&str>,
    props: &ListProps,
    host: &mut dyn ListHost,
) {
    let r = Rect::from_min_size(g.rect.min + g.offset, g.rect.size());
    drag_ghost(ctx, r, 1.05, 0.85, |gui, r| {
        BoxStyle {
            shadows: vec![account_card::GHOST_SHADOW],
            ..BoxStyle::default()
        }
        .paint(gui.painter(), r);
        for part in parts {
            let rect = part.rect.translate(r.min - origin);
            let draw = ItemDraw {
                item: part.item,
                rect,
                id: g.id.with(&part.key),
                avatar_id: part.source,
                key: &part.key,
                kb_focus: part.kb_focus,
                selected,
                pass: Pass::Ghost { shadow: false },
            };
            if matches!(part.item, DisplayItem::SectionHeader(_)) {
                // A header draws in a child of its own, as in the list.
                let mut child = gui.new_child(egui::UiBuilder::new().max_rect(rect));
                draw_item(&mut child, &draw, props, host);
            } else {
                draw_item(gui, &draw, props, host);
            }
        }
    });
}
