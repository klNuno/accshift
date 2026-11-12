//! The menu's keyboard. Arrows, Home and End move focus inside the box that
//! holds it, ArrowRight and ArrowLeft enter and leave a submenu, Enter and
//! Space click, Escape closes. Keys run before the boxes draw, so a key
//! wins over the pointer in the same frame.

use accshift_app::actions::dialogs::AppMenuEntry;
use egui::{Id, Key, Ui};

use super::submenu::{clear_submenu, open_submenu};
use super::{click, has_submenu, FocusMap, MenuEvent, State, Target};

/// The menu keys of this frame. Each keydown counts: two presses of an
/// arrow in one frame move twice.
struct Keys {
    esc: bool,
    downs: usize,
    ups: usize,
    home: bool,
    end: bool,
    right: bool,
    left: bool,
    /// Enter or Space.
    activate: bool,
}

impl Keys {
    fn read(ui: &Ui) -> Keys {
        let presses = ui.input(|i| {
            [
                Key::Escape,
                Key::ArrowDown,
                Key::ArrowUp,
                Key::Home,
                Key::End,
                Key::ArrowRight,
                Key::ArrowLeft,
                Key::Enter,
                Key::Space,
            ]
            .map(|k| i.num_presses(k))
        });
        let [esc, _, _, home, end, right, left, enter, space] = presses.map(|n| n > 0);
        Keys {
            esc,
            downs: presses[1],
            ups: presses[2],
            home,
            end,
            right,
            left,
            activate: enter || space,
        }
    }
}

/// Runs this frame's menu keys; where focus moves, if it does.
pub(super) fn handle(
    ui: &Ui,
    st: &mut State,
    items: &[AppMenuEntry],
    focus: &FocusMap,
    events: &mut Vec<MenuEvent>,
) -> Option<Id> {
    let keys = Keys::read(ui);
    if keys.downs + keys.ups > 0 || keys.right || keys.left {
        // egui moves focus on arrows too, and its focus lock only holds
        // from the second frame of a focus: a key right after a move would
        // count twice. The menu moves focus itself.
        ui.ctx()
            .memory_mut(|m| m.move_focus(egui::FocusDirection::None));
    }
    if keys.esc {
        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        events.push(MenuEvent::Close);
        return None;
    }
    let in_sub = focus.in_sub.is_some();
    let (scope, current) = match focus.in_sub {
        Some(t) => (&focus.sub, Some(t)),
        None => (&focus.menu, focus.in_menu),
    };
    let list = &scope.targets;
    if !list.is_empty() && keys.downs + keys.ups > 0 {
        let at = current.and_then(|c| list.iter().position(|t| *t == c));
        return step(list.len(), at, keys.downs, keys.ups).map(|i| scope.id_of(list[i]));
    }
    if !list.is_empty() && (keys.home || keys.end) {
        let t = if keys.home {
            list[0]
        } else {
            list[list.len() - 1]
        };
        return Some(scope.id_of(t));
    }
    if keys.right && !in_sub {
        return enter_submenu(st, items, focus, events);
    }
    if keys.left && in_sub {
        return leave_submenu(st, focus);
    }
    if keys.activate {
        activate(st, items, focus, events);
    }
    None
}

/// ArrowLeft in the submenu: close it and focus its parent.
fn leave_submenu(st: &mut State, focus: &FocusMap) -> Option<Id> {
    let parent = st.submenu;
    clear_submenu(st);
    parent.map(|p| focus.menu.id_of(Target::item(p)))
}

/// Enter or Space: click the focused target of the submenu or the menu.
fn activate(st: &mut State, items: &[AppMenuEntry], focus: &FocusMap, events: &mut Vec<MenuEvent>) {
    if let Some(t) = focus.in_sub {
        if let Some(entries) = focus.sub_entries {
            click(st, entries, t, true, events);
        }
    } else if let Some(t) = focus.in_menu {
        click(st, items, t, false, events);
    }
}

/// Where focus lands in a list of `n` after `downs` presses of ArrowDown
/// then `ups` of ArrowUp from `at`, wrapping around. With nothing focused,
/// ArrowDown starts at the first entry and ArrowUp at the last.
fn step(n: usize, mut at: Option<usize>, downs: usize, ups: usize) -> Option<usize> {
    for down in std::iter::repeat_n(true, downs).chain(std::iter::repeat_n(false, ups)) {
        at = Some(match at {
            None if down => 0,
            None => n - 1,
            Some(i) if down => (i + 1) % n,
            Some(i) => (i + n - 1) % n,
        });
    }
    at
}

/// ArrowRight on a submenu parent of the menu: into its open submenu, or
/// open it and focus its first entry once the entries are in.
fn enter_submenu(
    st: &mut State,
    items: &[AppMenuEntry],
    focus: &FocusMap,
    events: &mut Vec<MenuEvent>,
) -> Option<Id> {
    let t = focus
        .in_menu
        .filter(|t| t.swatch.is_none() && items.get(t.entry).is_some_and(has_submenu))?;
    if st.submenu == Some(t.entry) && focus.sub_entries.is_some() {
        return focus.sub.first_id();
    }
    open_submenu(st, items, t.entry, events);
    st.focus_submenu = Some(t.entry);
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_wrap_and_start_from_either_end() {
        assert_eq!(step(3, None, 1, 0), Some(0));
        assert_eq!(step(3, None, 0, 1), Some(2));
        assert_eq!(step(3, Some(2), 1, 0), Some(0));
        assert_eq!(step(3, Some(0), 0, 1), Some(2));
        assert_eq!(step(3, Some(0), 2, 1), Some(1));
    }
}
