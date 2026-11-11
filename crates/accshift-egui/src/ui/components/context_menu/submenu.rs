//! The submenu: which entries it lists, opening and closing it, the delayed
//! switch when the pointer moves to another entry, and its box.

use accshift_app::actions::dialogs::AppMenuEntry;
use accshift_app::i18n::{translate, Locale};
use accshift_app::menu::SubmenuLoader;
use egui::accesskit::Role;
use egui::{vec2, Id, Rect, Sense, Ui};

use super::draw::{draw_entries, paint_box, paint_state, Frame};
use super::layout::{layout, place_submenu, BoxKind, Content, Layout, PAD};
use super::{has_submenu, MenuBox, MenuEvent, MenuRun, Scope, State, SubmenuLoad, Target};
use crate::ui::snap;

/// How long the pointer rests on another entry before an open submenu
/// switches.
const SWITCH_DELAY: f64 = 0.15;

/// What a pending submenu switch does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Switch {
    Open(usize),
    Clear,
}

/// The id the targets of the submenu of entry `index` derive from.
pub(super) fn submenu_id(base: Id, index: Option<usize>) -> Id {
    base.with(("submenu", index))
}

/// What the open submenu lists.
fn submenu_content<'a>(
    entry: &'a AppMenuEntry,
    loads: &'a [(SubmenuLoader, SubmenuLoad)],
    locale: Locale,
) -> Content<'a> {
    let AppMenuEntry::Item {
        submenu, loader, ..
    } = entry
    else {
        return Content::Entries(&[]);
    };
    let entries: &[AppMenuEntry] = match (submenu, loader) {
        (Some(items), _) => items,
        (None, Some(loader)) => match loads.iter().find(|(l, _)| l == loader).map(|(_, s)| s) {
            Some(SubmenuLoad::Ready(items)) => items,
            Some(SubmenuLoad::Failed) => {
                return Content::State {
                    text: translate(locale, "context.submenuError"),
                    error: true,
                    loading: false,
                }
            }
            Some(SubmenuLoad::Loading) | None => {
                return Content::State {
                    text: translate(locale, "context.loading"),
                    error: false,
                    loading: true,
                }
            }
        },
        (None, None) => &[],
    };
    if entries.is_empty() {
        Content::State {
            text: translate(locale, "context.noGamesFound"),
            error: false,
            loading: false,
        }
    } else {
        Content::Entries(entries)
    }
}

/// The entries of an open submenu, once they are in.
pub(super) fn submenu_entries<'a>(
    items: &'a [AppMenuEntry],
    index: Option<usize>,
    loads: &'a [(SubmenuLoader, SubmenuLoad)],
    locale: Locale,
) -> Option<&'a [AppMenuEntry]> {
    let entry = items.get(index?)?;
    match submenu_content(entry, loads, locale) {
        Content::Entries(entries) => Some(entries),
        Content::State { .. } => None,
    }
}

/// Opens the submenu of entry `index`; its loader runs again on every open.
pub(super) fn open_submenu(
    st: &mut State,
    items: &[AppMenuEntry],
    index: usize,
    events: &mut Vec<MenuEvent>,
) {
    st.timer = None;
    let Some(AppMenuEntry::Item {
        submenu, loader, ..
    }) = items.get(index)
    else {
        return;
    };
    if submenu.is_none() && loader.is_none() {
        clear_submenu(st);
        return;
    }
    let reopen = st.submenu == Some(index);
    st.submenu = Some(index);
    st.submenu_entry = items[index].id().map(str::to_string);
    if let (None, Some(loader), false) = (submenu, loader, reopen) {
        events.push(MenuEvent::Load(loader.clone()));
    }
}

pub(super) fn clear_submenu(st: &mut State) {
    st.timer = None;
    st.submenu = None;
    st.submenu_entry = None;
    st.focus_submenu = None;
}

/// The items come anew every frame: an update that drops or moves the open
/// submenu's entry closes the submenu rather than show another entry's.
fn drop_stale_submenu(st: &mut State, items: &[AppMenuEntry]) {
    let Some(index) = st.submenu else {
        return;
    };
    let same = items
        .get(index)
        .is_some_and(|e| has_submenu(e) && e.id() == st.submenu_entry.as_deref());
    if !same {
        clear_submenu(st);
    }
}

/// Switches the submenu at once when none is open, else after the delay.
fn schedule(
    st: &mut State,
    change: Switch,
    items: &[AppMenuEntry],
    now: f64,
    events: &mut Vec<MenuEvent>,
) {
    st.timer = None;
    if st.submenu.is_none() {
        apply_switch(st, change, items, events);
    } else {
        st.timer = Some((now + SWITCH_DELAY, change));
    }
}

fn apply_switch(
    st: &mut State,
    change: Switch,
    items: &[AppMenuEntry],
    events: &mut Vec<MenuEvent>,
) {
    match change {
        Switch::Open(index) => open_submenu(st, items, index, events),
        Switch::Clear => clear_submenu(st),
    }
}

impl MenuRun<'_> {
    /// Fires a pending switch that is due (or wakes the frame when it will
    /// be), then closes a submenu whose entry went.
    pub(super) fn run_timer(&mut self) {
        if let Some((at, change)) = self.st.timer {
            let now = self.props.now;
            if now >= at {
                self.st.timer = None;
                apply_switch(&mut self.st, change, self.props.items, &mut self.events);
            } else {
                self.ctx
                    .request_repaint_after(std::time::Duration::from_secs_f64(at - now));
            }
        }
        drop_stale_submenu(&mut self.st, self.props.items);
    }

    /// The pointer moved onto another target of the menu, or off them all.
    /// The open submenu's parent cancels a pending switch, another parent
    /// opens its submenu, anything else closes it.
    pub(super) fn hover(&mut self, over: Option<Target>) {
        if over == self.st.over {
            return;
        }
        self.st.over = over;
        let Some(t) = over else {
            return;
        };
        let item = t.swatch.is_none();
        if item && self.st.submenu == Some(t.entry) {
            self.st.timer = None;
            return;
        }
        let items = self.props.items;
        let change = if item && items.get(t.entry).is_some_and(has_submenu) {
            Switch::Open(t.entry)
        } else {
            Switch::Clear
        };
        schedule(
            &mut self.st,
            change,
            items,
            self.props.now,
            &mut self.events,
        );
    }

    /// Draws the open submenu beside `menu`, level with its parent entry;
    /// its box, if one is open.
    pub(super) fn draw_submenu(&mut self, ui: &mut Ui, menu: &MenuBox) -> Option<Rect> {
        let props = self.props;
        let v = props.vars;
        let Some((index, entry)) = self
            .st
            .submenu
            .and_then(|i| props.items.get(i).map(|e| (i, e)))
        else {
            self.st.over_submenu = false;
            return None;
        };
        let sub_id = submenu_id(self.base, Some(index));
        let content = submenu_content(entry, props.loads, props.locale);
        // The submenu stays inside the window in the stage's points.
        let kind = BoxKind::Submenu {
            viewport: crate::zoom::viewport(&self.ctx),
        };
        let sub_layout = layout(&self.ctx, v, props.locale, &content, kind);
        let border = menu.layout.border;
        // The parent entry's top from the menu's padding edge, rounded.
        let anchor = (PAD + menu.layout.rows.get(index).map_or(0.0, |r| r.top)).round();
        let vp = props.viewport;
        let offset = place_submenu(
            menu.rect.min - vp.min,
            menu.layout.size,
            sub_layout.size,
            anchor,
            vp.size(),
        );
        let rect = Rect::from_min_size(
            menu.rect.min + vec2(border, border) + offset,
            sub_layout.size,
        );
        let ppp = self.ctx.pixels_per_point();
        let response = ui.interact(snap::rect(rect, ppp), sub_id, Sense::click());
        self.ctx
            .accesskit_node_builder(sub_id, |node| node.set_role(Role::Menu));
        // The pointer entering the submenu cancels a pending switch.
        let over = response.hovered() || ui.rect_contains_pointer(rect);
        paint_box(ui, rect, v, sub_layout.border);
        match &content {
            Content::State { text, error, .. } => {
                paint_state(ui, rect, &sub_layout, text, *error, v);
            }
            Content::Entries(entries) => {
                let scope = Scope::new(sub_id, entries);
                self.focus_opened_submenu(index, &scope);
                self.draw_submenu_entries(ui, rect, &sub_layout, entries, &scope);
            }
        }
        if over && !self.st.over_submenu {
            self.st.timer = None;
        }
        self.st.over_submenu = over;
        // A failed or empty submenu has nothing to focus; a loading one
        // keeps the keyboard request for when its entries land.
        if matches!(content, Content::State { loading: false, .. }) {
            self.st.focus_submenu = None;
        }
        Some(rect)
    }

    /// A submenu opened from the keyboard focuses its first entry once its
    /// entries are in.
    fn focus_opened_submenu(&mut self, index: usize, scope: &Scope) {
        if self.st.focus_submenu != Some(index) {
            return;
        }
        self.st.focus_submenu = None;
        if let Some(first) = scope.first_id() {
            self.focus_to = Some(first);
        }
    }

    /// Draws the entries of the submenu: a press focuses an entry (mouse
    /// focus, no ring), a click runs it.
    fn draw_submenu_entries(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        layout: &Layout,
        entries: &[AppMenuEntry],
        scope: &Scope,
    ) {
        let props = self.props;
        let input = draw_entries(
            ui,
            &Frame {
                rect,
                layout,
                entries,
                submenu: true,
                id: scope.id,
                focused: scope.find(self.focus_to.or(self.focused_id)),
                expanded: None,
                vars: props.vars,
                locale: props.locale,
                keyboard: props.keyboard,
            },
        );
        if let Some(t) = input.pressed {
            self.focus_to = Some(scope.id_of(t));
        }
        if let Some(t) = input.clicked {
            super::click(&mut self.st, entries, t, true, &mut self.events);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::components::context_menu::tests::item;

    #[test]
    fn hovering_switches_at_once_without_a_submenu_and_late_with_one() {
        let entries = vec![item("a"), item("b")];
        let mut st = State::default();
        let mut events = Vec::new();
        schedule(&mut st, Switch::Clear, &entries, 1.0, &mut events);
        assert!(st.timer.is_none());
        st.submenu = Some(0);
        schedule(&mut st, Switch::Clear, &entries, 1.0, &mut events);
        assert_eq!(st.timer, Some((1.0 + SWITCH_DELAY, Switch::Clear)));
    }
}
