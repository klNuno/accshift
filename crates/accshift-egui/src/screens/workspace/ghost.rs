//! The drag ghosts: a clone of the dragged card, or of the dragged folder
//! section with its header and cards, following the pointer over
//! everything and taking no input.

use std::collections::HashMap;

use accshift_app::platforms::card::UsernameBadge;
use egui::{vec2, Id, Rect, Ui};

use super::cards::{card_color, header_label};
use super::layout::{Node, Placed};
use super::{Frame, Ghost, Workspace};
use crate::app::accounts::Cs2Cards;
use crate::ui::components::account_card::{self, AccountCardProps, AvatarContent};
use crate::ui::components::folder_card::{self, entrance, FolderCardProps};
use crate::ui::components::section_header::{self, SectionHeaderProps};
use crate::ui::paint::BoxStyle;

/// What the card pass keeps for the ghosts.
#[derive(Default)]
pub(super) struct GhostSources {
    /// The dragged account card's avatar.
    pub avatar: Option<AvatarContent>,
    /// The key prefix of the dragged folder section.
    pub section: Option<String>,
    /// The avatars of that section's account cards, by key.
    pub section_avatars: HashMap<String, AvatarContent>,
}

/// What the ghosts read besides their cards.
struct GhostInput<'a> {
    f: &'a Frame<'a>,
    cs2: &'a Cs2Cards,
}

/// Draws the ghost of the dragged card, else of the dragged folder section
/// when its card is not laid out (its section is).
pub(super) fn draw_ghosts(
    ui: &Ui,
    ws: &Workspace,
    f: &Frame,
    placed: &[Placed],
    cs2: &Cs2Cards,
    sources: GhostSources,
) {
    let Some(g) = &ws.ghost else {
        return;
    };
    let input = GhostInput { f, cs2 };
    if let Some(p) = placed.iter().find(|p| p.node.shows(&g.item)) {
        draw_ghost(ui, g, &p.node, &input, sources.avatar);
        return;
    }
    let Some(prefix) = sources.section else {
        return;
    };
    let Some(at) = ws.section_rect(&g.item) else {
        return;
    };
    let parts: Vec<(&Node, Rect, String)> = placed
        .iter()
        .filter(|p| p.key.starts_with(&prefix))
        .map(|p| (&p.node, f.to_screen(p.rect), p.key.clone()))
        .collect();
    draw_section_ghost(g, &parts, at.min, &sources.section_avatars, &input);
}

/// The username badge a ghost's account card shows: none when one click
/// switches (bulk edit).
fn ghost_badge<'c>(node: &Node, cs2: &'c Cs2Cards) -> Option<&'c UsernameBadge> {
    let Node::Account(a) = node else {
        return None;
    };
    cs2.card(&a.account.id)
        .and_then(|c| c.badge.as_ref())
        .filter(|_| !a.single_click_switch)
}

/// The dragged card cloned at the drag start, at the pointer, at 85 %
/// opacity, scaled 1.05 and with its own shadow, over everything.
fn draw_ghost(ui: &Ui, g: &Ghost, node: &Node, input: &GhostInput, avatar: Option<AvatarContent>) {
    let f = input.f;
    let v = f.v;
    let height = match node {
        Node::Account(a) => {
            let badge = ghost_badge(node, input.cs2);
            account_card::height(&f.ctx, v, a, badge.is_some())
        }
        _ => v.grid_card_min_height(),
    };
    // The source's width, kept where the pointer grabbed it.
    let r = Rect::from_min_size(
        g.rect.min + (f.drag.pointer - g.anchor),
        vec2(g.rect.width(), height),
    );
    // The clone replays the card entrance, which overrides the ghost's
    // opacity and scale while it runs.
    let (enter_opacity, _) = entrance(ui, g.id, v);
    let entering = enter_opacity < 1.0;
    let (scale, opacity) = if entering { (1.0, 1.0) } else { (1.05, 0.85) };
    crate::ui::components::drag_ghost(&f.ctx, r, scale, opacity, |gui, r| {
        ghost_card(gui, g.id, node, r, avatar.clone(), true, input);
    });
}

/// The dragged folder section cloned with its header and cards (which
/// replay the card entrance), its shadow around a box with no background.
/// `parts` are the section's nodes with their boxes on screen, `origin` the
/// section's corner they sit in.
fn draw_section_ghost(
    g: &Ghost,
    parts: &[(&Node, Rect, String)],
    origin: egui::Pos2,
    avatars: &HashMap<String, AvatarContent>,
    input: &GhostInput,
) {
    let f = input.f;
    let v = f.v;
    let r = Rect::from_min_size(g.rect.min + (f.drag.pointer - g.anchor), g.rect.size());
    crate::ui::components::drag_ghost(&f.ctx, r, 1.05, 0.85, |gui, r| {
        BoxStyle {
            shadows: vec![account_card::GHOST_SHADOW],
            ..BoxStyle::default()
        }
        .paint(gui.painter(), r);
        for (node, screen, key) in parts {
            let rect = screen.translate(r.min - origin);
            let id = g.id.with(key);
            match node {
                Node::Header(h) => {
                    let (folder, label) = header_label(h, f.locale);
                    let mut child = gui.new_child(egui::UiBuilder::new().max_rect(rect));
                    section_header::section_header(
                        &mut child,
                        rect,
                        &SectionHeaderProps {
                            vars: v,
                            id,
                            folder,
                            label: &label,
                            count: h.count,
                            card_color: card_color(f.resolved, v, &h.card_color),
                            accent: f.accent,
                            collapsed: h.collapsed,
                            focus_visible: false,
                        },
                    );
                }
                Node::Folder(_) | Node::Account(_) => {
                    let mut child = gui.new_child(egui::UiBuilder::new().max_rect(rect));
                    child.set_clip_rect(Rect::EVERYTHING);
                    let avatar = avatars.get(key).cloned();
                    ghost_card(&mut child, id, node, rect, avatar, false, input);
                }
                _ => {}
            }
        }
    });
}

/// The clone inside a ghost: the source card at rest. `shadow`: the card
/// is the ghost itself and takes the ghost's shadow.
fn ghost_card(
    gui: &mut Ui,
    id: Id,
    node: &Node,
    r: Rect,
    avatar: Option<AvatarContent>,
    shadow: bool,
    input: &GhostInput,
) {
    let f = input.f;
    let v = f.v;
    match node {
        Node::Folder(item) => {
            folder_card::folder_card(
                gui,
                r,
                &FolderCardProps {
                    vars: v,
                    id,
                    name: &item.folder.name,
                    card_color: card_color(f.resolved, v, &item.card_color),
                    accent: f.accent,
                    drag_over: false,
                    dragged: false,
                    focus_visible: false,
                    kb_focus: false,
                    grid_dragging: false,
                    ghost: shadow,
                },
            );
        }
        Node::Account(a) => {
            // Cloned before the source was marked dragged.
            let mut item = accshift_app::display::AccountCardItem::clone(a);
            item.dragged = false;
            account_card::account_card(
                gui,
                r,
                &AccountCardProps {
                    vars: v,
                    id,
                    item: &item,
                    card_color: card_color(f.resolved, v, &a.card_color),
                    avatar: avatar.unwrap_or(AvatarContent::Loading),
                    focus_visible: false,
                    locale: f.locale,
                    time: &f.time,
                    badge: ghost_badge(node, input.cs2),
                    extension: &[],
                    extension_disabled: true,
                    extension_forced: false,
                    extension_hover_disabled: false,
                    clickable_chips: &[],
                    glass: f.glass,
                    kb_focus: false,
                    grid_dragging: false,
                    ghost: shadow,
                },
            );
        }
        _ => {}
    }
}
