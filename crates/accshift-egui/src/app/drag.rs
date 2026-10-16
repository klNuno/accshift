//! The card drag of `App.svelte`: the grid wrapper's `mousedown`, the
//! document's `mousemove`, `scroll` and `mouseup` listeners and the frame's
//! `updateDragAt`, read against the boxes the workspace drew last frame
//! (`elementFromPoint` and `getBoundingClientRect` read the DOM before it
//! re-renders too).

use accshift_app::drag::{PointHit, PressedCard, SlotRect, Slots, StoreHost};
use accshift_app::folders::{ItemKind, ItemRef};
use egui::{Context, Event, Id, PointerButton, Pos2, Rect};

use super::AppModel;
use crate::screens::workspace::{Ghost, Workspace};
use crate::ui::components::bulk_edit::COVER_ID;

/// `closest("[data-account-id], [data-folder-id]")` under a press: a card,
/// else the `.section` wrapper holding the point.
fn pressed_card(ws: &Workspace, pos: Pos2) -> Option<PressedCard> {
    let section_of = |pos: Pos2| {
        ws.sections
            .iter()
            .find(|(_, r)| r.contains(pos))
            .and_then(|(folder, _)| folder.clone())
    };
    if let Some((item, _)) = ws.cards.iter().rev().find(|(_, r)| r.contains(pos)) {
        return Some(PressedCard {
            item: item.clone(),
            section_card: false,
            section_folder: section_of(pos),
        });
    }
    let folder = section_of(pos)?;
    Some(PressedCard {
        item: ItemRef::folder(&folder),
        section_card: true,
        section_folder: Some(folder),
    })
}

/// `elementFromPoint(x, y)?.closest("[data-folder-id], [data-back-card],
/// [data-account-id]")`: the ghost lets the pointer through.
fn point_hit(ctx: &Context, ws: &Workspace, cover: &[Rect], pos: Pos2) -> PointHit {
    if !ctx.content_rect().contains(pos) {
        return PointHit::Nothing;
    }
    if !ws.viewport.is_some_and(|r| r.contains(pos)) || cover.iter().any(|r| r.contains(pos)) {
        return PointHit::Grid;
    }
    if let Some((item, _)) = ws.cards.iter().rev().find(|(_, r)| r.contains(pos)) {
        return match item.kind {
            ItemKind::Account => PointHit::Account,
            ItemKind::Folder => PointHit::Folder {
                id: item.id.clone(),
                section_card: false,
            },
        };
    }
    if ws.back.is_some_and(|r| r.contains(pos)) {
        return PointHit::Back;
    }
    match ws.sections.iter().find(|(_, r)| r.contains(pos)) {
        Some((Some(id), _)) => PointHit::Folder {
            id: id.clone(),
            section_card: true,
        },
        _ => PointHit::Grid,
    }
}

/// `refreshSlotRects`: the section wrappers when a folder moves among
/// sections, else the grid's (or list's) cards of the dragged kind.
fn slots(ws: &Workspace, item: &ItemRef) -> Slots {
    let rect = |r: &Rect| SlotRect {
        left: r.left(),
        top: r.top(),
        width: r.width(),
        height: r.height(),
    };
    if ws.sections_mode && item.kind == ItemKind::Folder {
        return Slots {
            rects: ws
                .sections
                .iter()
                .filter(|(folder, _)| folder.is_some())
                .map(|(_, r)| rect(r))
                .collect(),
            list_mode: true,
        };
    }
    Slots {
        rects: ws
            .cards
            .iter()
            .filter(|(card, _)| card.kind == item.kind)
            .map(|(_, r)| rect(r))
            .collect(),
        list_mode: ws.list_mode,
    }
}

impl AppModel {
    /// One frame of the drag, before the lists are computed: the preview
    /// order and a drop show in the frame that caused them.
    pub(super) fn track_drag(&mut self, ctx: &Context) {
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        let cover: Vec<Rect> = ctx
            .data(|d| d.get_temp(Id::new(COVER_ID)))
            .unwrap_or_default();
        let hit = ctx.viewport(|vp| vp.hits.click.map(|w| w.id));
        let tab = b.nav.active_tab.clone();
        let events = ctx.input(|i| i.events.clone());
        let ws = &mut b.workspace;
        let accounts = &mut b.accounts;
        let stores = &mut b.stores;
        for event in events {
            match event {
                Event::PointerButton {
                    pos,
                    button,
                    pressed: true,
                    ..
                } => {
                    let primary = button == PointerButton::Primary;
                    b.drag_held |= primary;
                    // `handleWorkspaceMouseDown`: bulk edit paints instead
                    // and a search keeps the order.
                    if b.bulk.is_open() || accounts.navigation.is_searching() {
                        continue;
                    }
                    // The grid wrapper's `onmousedown`: nothing else on top.
                    let on_workspace = ws.drawn_last_frame(ctx)
                        && ws.viewport.is_some_and(|r| r.contains(pos))
                        && hit.is_none_or(|id| ws.owns(id))
                        && !cover.iter().any(|r| r.contains(pos));
                    let card = on_workspace.then(|| pressed_card(ws, pos)).flatten();
                    accounts
                        .drag
                        .mouse_down(primary, card, ws.sections_mode, (pos.x, pos.y));
                }
                Event::PointerMoved(pos) => {
                    if accounts.drag.mouse_move((pos.x, pos.y), b.drag_held) {
                        let Some(item) = accounts.drag.drag_item().cloned() else {
                            continue;
                        };
                        let host = StoreHost {
                            navigation: &accounts.navigation,
                            stores,
                        };
                        accounts.drag.set_slots(slots(ws, &item), &host);
                        b.drag_scroll = ws.scroll();
                        ws.ghost = ws.bounding_rect(ctx, &item).map(|rect| Ghost {
                            item,
                            rect,
                            anchor: pos,
                            id: Id::new(("drag-ghost", ctx.cumulative_frame_nr())),
                        });
                    }
                }
                Event::PointerButton {
                    button: PointerButton::Primary,
                    pressed: false,
                    ..
                } => {
                    b.drag_held = false;
                    let out = {
                        let host = StoreHost {
                            navigation: &accounts.navigation,
                            stores,
                        };
                        accounts.drag.mouse_up(&host)
                    };
                    if let Some(commit) = out.commit {
                        commit.apply(stores, &tab);
                    }
                    if out.refresh {
                        accounts.navigation.refresh_current_items(stores, &tab);
                    }
                }
                _ => {}
            }
        }
        if accounts.drag.is_dragging() {
            // `handleDocScroll`: the slots moved with the content.
            if ws.scroll() != b.drag_scroll {
                b.drag_scroll = ws.scroll();
                accounts.drag.scrolled();
            }
            let host = StoreHost {
                navigation: &accounts.navigation,
                stores,
            };
            if accounts.drag.rects_stale() {
                if let Some(item) = accounts.drag.drag_item().cloned() {
                    accounts.drag.set_slots(slots(ws, &item), &host);
                }
            }
            let (x, y) = accounts.drag.pointer();
            let under = point_hit(ctx, ws, &cover, Pos2::new(x, y));
            accounts.drag.update(&under, &host);
        }
        crate::ui::components::set_click_eaten(ctx, accounts.drag.eats_click());
    }
}

/// The effects' linked list of a keyed `{#each}`, by index into the old
/// order.
struct EachList {
    prev: Vec<Option<usize>>,
    next: Vec<Option<usize>>,
    first: Option<usize>,
}

impl EachList {
    /// `link(state, a, b)`.
    fn link(&mut self, a: Option<usize>, b: Option<usize>) {
        match a {
            None => self.first = b,
            Some(a) => self.next[a] = b,
        }
        if let Some(b) = b {
            self.prev[b] = a;
        }
    }
}

/// Svelte 5's keyed `{#each}` reconcile (`reconcile` in `each.js`) over a
/// reorder of the same keys: the keys whose node it moves. Chromium drops
/// `:active` from a node it moves (`ActiveChainNodeDetached`) and restarts
/// its CSS animations.
pub(super) fn each_moved(old: &[String], new: &[String]) -> Vec<String> {
    let n = old.len();
    let index = |k: &str| old.iter().position(|o| o == k);
    if new.len() != n {
        return Vec::new();
    }
    let mut list = EachList {
        prev: (0..n).map(|i| i.checked_sub(1)).collect(),
        next: (0..n).map(|i| (i + 1 < n).then_some(i + 1)).collect(),
        first: (n > 0).then_some(0),
    };
    let mut current = list.first;
    let mut prev: Option<usize> = None;
    let mut seen = std::collections::BTreeSet::new();
    let mut matched: Vec<usize> = Vec::new();
    let mut stashed: Vec<usize> = Vec::new();
    let mut moved = std::collections::BTreeSet::new();
    let mut i = 0;
    while i < n {
        let Some(effect) = index(&new[i]) else {
            return Vec::new();
        };
        i += 1;
        if Some(effect) != current {
            if seen.contains(&effect) {
                if !matched.is_empty() && matched.len() < stashed.len() {
                    // Later items move to the front.
                    let start = stashed[0];
                    prev = list.prev[start];
                    let (a, b) = (matched[0], matched[matched.len() - 1]);
                    moved.extend(matched.iter().copied());
                    for s in &stashed {
                        seen.remove(s);
                    }
                    list.link(list.prev[a], list.next[b]);
                    list.link(prev, Some(a));
                    list.link(Some(b), Some(start));
                    current = Some(start);
                    prev = Some(b);
                    i -= 1;
                    matched.clear();
                    stashed.clear();
                } else {
                    // Earlier items move to the back.
                    seen.remove(&effect);
                    moved.insert(effect);
                    list.link(list.prev[effect], list.next[effect]);
                    let after = match prev {
                        None => list.first,
                        Some(p) => list.next[p],
                    };
                    list.link(Some(effect), after);
                    list.link(prev, Some(effect));
                    prev = Some(effect);
                }
                continue;
            }
            matched.clear();
            stashed.clear();
            while let Some(c) = current.filter(|&c| c != effect) {
                seen.insert(c);
                stashed.push(c);
                current = list.next[c];
            }
            if current.is_none() {
                continue;
            }
        }
        matched.push(effect);
        prev = Some(effect);
        current = list.next[effect];
    }
    moved.into_iter().map(|i| old[i].clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::each_moved;

    fn keys(s: &str) -> Vec<String> {
        s.chars().map(String::from).collect()
    }

    fn each_moves(old: &[String], new: &[String], key: &str) -> bool {
        each_moved(old, new).iter().any(|k| k == key)
    }

    #[test]
    fn a_card_moved_forward_is_the_node_svelte_moves() {
        assert!(each_moves(&keys("abcde"), &keys("acbde"), "b"));
        assert!(each_moves(&keys("abcde"), &keys("acdeb"), "b"));
    }

    #[test]
    fn one_slot_back_moves_the_neighbour_instead() {
        assert!(!each_moves(&keys("abcde"), &keys("acbde"), "c"));
        assert!(each_moves(&keys("abcde"), &keys("acbde"), "b"));
        assert!(each_moves(&keys("abcde"), &keys("eabcd"), "e"));
        assert!(!each_moves(&keys("abcde"), &keys("eabcd"), "a"));
    }

    #[test]
    fn an_unchanged_order_moves_nothing() {
        assert!(each_moved(&keys("abc"), &keys("abc")).is_empty());
    }

    #[test]
    fn later_items_to_the_front_move_as_a_block() {
        assert_eq!(each_moved(&keys("abcde"), &keys("deabc")), keys("de"));
    }
}
