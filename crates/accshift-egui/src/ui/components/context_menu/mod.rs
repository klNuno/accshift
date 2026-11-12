//! The menu a right click (or the context menu key) opens, drawn above
//! everything else.
//!
//! The menu box sits at the click point, kept 4 px inside the window, at
//! least 220 px wide on its border box and as wide as its widest entry
//! otherwise: 4 px of padding, the overlay background under the card
//! background image, the theme border, a 6 px radius and a shadow 8 px
//! down, 24 px blur, black at 40%.
//!
//! - An entry: 6 px by 10 px of padding, 12 px text on the body's line
//!   height, a 4 px radius. Hover and keyboard focus paint the muted
//!   background, a press the elevated one, easing over 80 ms. A submenu
//!   parent ends with a `›` in the subtle foreground, 8 px after its label.
//! - A separator: 1 px of the border colour, 4 px above and below, 6 px in
//!   from the sides.
//! - A swatch group: 6 px by 8 px of padding, a 10 px uppercase label in
//!   the muted foreground (letter spacing 0.06em), 6 px, then a row of
//!   18 px swatches 6 px apart, each a 12 px fill in a 1 px border box on
//!   the solid background. The active one has a foreground border and a
//!   1 px foreground ring; hovering or keyboard focus rings the others at
//!   35 %.
//! - A submenu: the same box, at least 240 px wide, at most min(320 px,
//!   window width - 8 px) wide and window height - 8 px high, 4 px to the
//!   right of the menu (or to its left when it does not fit), level with
//!   its parent entry. A loader fills it on open; until then it says it is
//!   loading.
//!
//! The first entry takes focus on open; its ring shows only under keyboard
//! modality. Arrows, Home and End move focus, ArrowRight and ArrowLeft
//! enter and leave a submenu, Enter and Space click, Escape closes.
//! Hovering an entry swaps the submenu after 150 ms while one is open, at
//! once otherwise.
//!
//! A press outside closes the menu; so does a scroll outside it, which the
//! caller reports. The menu reports [`MenuEvent`]s; [`AppDialogs`] runs them.
//!
//! [`AppDialogs`]: accshift_app::actions::dialogs::AppDialogs

mod draw;
mod keys;
mod layout;
mod submenu;

use accshift_app::actions::dialogs::{AppMenuCommand, AppMenuEntry};
use accshift_app::i18n::Locale;
use accshift_app::menu::SubmenuLoader;
use egui::accesskit::Role;
use egui::{vec2, Context, Event, Id, Pos2, Rect, Sense, Ui};

use crate::app::vars::Vars;
use crate::ui::snap;
use draw::{draw_entries, paint_box, Frame};
use layout::{layout, place_menu, BoxKind, Content, Layout, EDGE};
use submenu::{open_submenu, submenu_entries, submenu_id, Switch};

/// A loader's result, as the caller holds it.
#[derive(Clone, Debug, PartialEq)]
pub enum SubmenuLoad {
    Loading,
    Ready(Vec<AppMenuEntry>),
    /// The loader failed: the submenu says so.
    Failed,
}

/// What the menu shows.
pub struct ContextMenuProps<'a> {
    pub vars: &'a Vars,
    pub locale: Locale,
    /// Changes on every open: each open starts a fresh menu.
    pub open_id: u64,
    pub items: &'a [AppMenuEntry],
    /// The click point.
    pub at: Pos2,
    /// The window: what the menu stays inside.
    pub viewport: Rect,
    /// Chromium's keyboard modality: the last input was a key press.
    pub keyboard: bool,
    /// The loaders asked for with [`MenuEvent::Load`] and their state.
    pub loads: &'a [(SubmenuLoader, SubmenuLoad)],
    pub now: f64,
}

/// What the user did.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuEvent {
    /// An entry or swatch: run it, the menu closes.
    Activate(AppMenuCommand),
    /// Close the menu: Escape, a press outside, an entry with nothing to
    /// run.
    Close,
    /// A submenu opened on a loader: run it.
    Load(SubmenuLoader),
}

/// One focusable element: an entry, or one swatch of it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Target {
    entry: usize,
    swatch: Option<usize>,
}

impl Target {
    /// An entry that is not a swatch group.
    fn item(entry: usize) -> Target {
        Target {
            entry,
            swatch: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct State {
    open_id: u64,
    /// The entry whose submenu is open.
    submenu: Option<usize>,
    /// The id of the entry at `submenu` when it opened.
    submenu_entry: Option<String>,
    /// A pending submenu switch: when it fires and what it does.
    timer: Option<(f64, Switch)>,
    /// The target under the pointer last frame, in the menu box.
    over: Option<Target>,
    /// The pointer was over the submenu box last frame.
    over_submenu: bool,
    /// A submenu opened from the keyboard: focus its first entry once its
    /// entries are in.
    focus_submenu: Option<usize>,
    /// The first entry still has to take focus.
    mount_focus: bool,
    /// Opened by a key (the context menu key, Shift+F10, Delete), not at
    /// the pointer.
    from_key: bool,
    /// Last drawn boxes, for [`contains`].
    menu_rect: Option<Rect>,
    submenu_rect: Option<Rect>,
}

fn state_id() -> Id {
    Id::unique("accshift::context-menu")
}

/// Whether `pos` falls on the open menu or its submenu (as last drawn).
pub fn contains(ctx: &Context, pos: Pos2) -> bool {
    hit_test(ctx)(pos)
}

/// `contains` with the rects read now, for use inside `ctx.input`, where
/// reading the context again would deadlock.
pub fn hit_test(ctx: &Context) -> impl Fn(Pos2) -> bool {
    let st: Option<State> = ctx.data(|d| d.get_temp(state_id()));
    let rects = st.map_or([None, None], |st| [st.menu_rect, st.submenu_rect]);
    move |pos| rects.iter().flatten().any(|r| r.contains(pos))
}

/// Forgets the menu: the next open starts fresh.
pub fn reset(ctx: &Context) {
    ctx.data_mut(|d| d.remove::<State>(state_id()));
}

fn has_submenu(entry: &AppMenuEntry) -> bool {
    matches!(
        entry,
        AppMenuEntry::Item { submenu, loader, .. } if submenu.is_some() || loader.is_some()
    )
}

/// The focusable targets of `entries`, in order: each entry but separators,
/// each swatch of a group.
fn targets(entries: &[AppMenuEntry]) -> Vec<Target> {
    let mut out = Vec::new();
    for (entry, e) in entries.iter().enumerate() {
        match e {
            AppMenuEntry::Separator => {}
            AppMenuEntry::Item { .. } => out.push(Target::item(entry)),
            AppMenuEntry::Swatches { swatches, .. } => {
                out.extend((0..swatches.len()).map(|k| Target {
                    entry,
                    swatch: Some(k),
                }));
            }
        }
    }
    out
}

fn target_id(base: Id, t: Target) -> Id {
    base.with((t.entry, t.swatch))
}

/// The focusable targets of one box and the id they derive from.
struct Scope {
    id: Id,
    targets: Vec<Target>,
}

impl Scope {
    fn new(id: Id, entries: &[AppMenuEntry]) -> Scope {
        Scope {
            id,
            targets: targets(entries),
        }
    }

    fn id_of(&self, t: Target) -> Id {
        target_id(self.id, t)
    }

    fn first_id(&self) -> Option<Id> {
        self.targets.first().map(|t| self.id_of(*t))
    }

    /// The target `focused` names, if it is one of this box's.
    fn find(&self, focused: Option<Id>) -> Option<Target> {
        self.targets
            .iter()
            .copied()
            .find(|t| Some(self.id_of(*t)) == focused)
    }
}

/// Where keyboard focus is at the start of the frame: the targets of both
/// boxes and the one egui holds focused, if it is theirs.
struct FocusMap<'a> {
    menu: Scope,
    sub: Scope,
    /// The open submenu's entries, once they are in.
    sub_entries: Option<&'a [AppMenuEntry]>,
    in_menu: Option<Target>,
    in_sub: Option<Target>,
}

/// A click on target `t` of `entries`, the menu's (`submenu: false`) or
/// the submenu's: run its command, open its submenu, or close the menu.
fn click(
    st: &mut State,
    entries: &[AppMenuEntry],
    t: Target,
    submenu: bool,
    events: &mut Vec<MenuEvent>,
) {
    let Some(entry) = entries.get(t.entry) else {
        return;
    };
    match (entry, t.swatch) {
        (AppMenuEntry::Swatches { swatches, .. }, Some(k)) => {
            if let Some(sw) = swatches.get(k) {
                events.push(MenuEvent::Activate(sw.command.clone()));
            }
        }
        (AppMenuEntry::Item { command, .. }, None) => match command {
            Some(command) => events.push(MenuEvent::Activate(command.clone())),
            None if !submenu && has_submenu(entry) => {
                open_submenu(st, entries, t.entry, events);
            }
            None => events.push(MenuEvent::Close),
        },
        _ => {}
    }
}

/// The menu box as drawn this frame.
struct MenuBox {
    rect: Rect,
    layout: Layout,
}

/// One frame of the open menu: its state, the events it reports, and where
/// focus goes at the end of the frame.
struct MenuRun<'p> {
    ctx: Context,
    props: &'p ContextMenuProps<'p>,
    st: State,
    events: Vec<MenuEvent>,
    /// The id every target of this open derives from.
    base: Id,
    menu_id: Id,
    /// The widget egui held focused once the first entry took focus.
    focused_id: Option<Id>,
    /// Where focus moves at the end of the frame.
    focus_to: Option<Id>,
}

/// Draws the open menu. `items` are the menu's entries.
pub fn context_menu(ui: &mut Ui, props: &ContextMenuProps) -> Vec<MenuEvent> {
    let mut run = MenuRun::start(ui.ctx(), props);
    run.run_timer();
    let focus = run.resolve_focus();
    run.focus_to = keys::handle(ui, &mut run.st, props.items, &focus, &mut run.events);
    let menu = run.draw_menu(ui, &focus);
    let submenu_rect = run.draw_submenu(ui, &menu);
    if pressed_outside(ui, menu.rect, submenu_rect) {
        run.events.push(MenuEvent::Close);
    }
    run.finish(menu.rect, submenu_rect)
}

impl<'p> MenuRun<'p> {
    /// Picks the state up where the last frame of this open left it, or
    /// starts a fresh one.
    fn start(ctx: &Context, props: &'p ContextMenuProps<'p>) -> MenuRun<'p> {
        let st: State = ctx
            .data(|d| d.get_temp(state_id()))
            .filter(|s: &State| s.open_id == props.open_id)
            .unwrap_or_else(|| State {
                open_id: props.open_id,
                mount_focus: true,
                from_key: props.keyboard,
                ..State::default()
            });
        let base = Id::unique(("accshift::context-menu", props.open_id));
        MenuRun {
            ctx: ctx.clone(),
            props,
            st,
            events: Vec::new(),
            base,
            menu_id: base.with("menu"),
            focused_id: None,
            focus_to: None,
        }
    }

    /// Gives the first entry focus on open, finds which target holds focus,
    /// and keeps egui's own arrow and Escape handling off it.
    fn resolve_focus(&mut self) -> FocusMap<'p> {
        let props = self.props;
        let menu = Scope::new(self.menu_id, props.items);
        if self.st.mount_focus {
            self.st.mount_focus = false;
            if let Some(first) = menu.first_id() {
                self.ctx.memory_mut(|m| m.request_focus(first));
            }
        }
        self.focused_id = self.ctx.memory(|m| m.focused());
        let sub_entries = submenu_entries(props.items, self.st.submenu, props.loads, props.locale);
        let sub = Scope {
            id: submenu_id(self.base, self.st.submenu),
            targets: sub_entries.map(targets).unwrap_or_default(),
        };
        let focus = FocusMap {
            in_menu: menu.find(self.focused_id),
            in_sub: sub.find(self.focused_id),
            menu,
            sub,
            sub_entries,
        };
        let ours = focus.in_menu.is_some() || focus.in_sub.is_some();
        if let Some(id) = self.focused_id.filter(|_| ours) {
            self.ctx.memory_mut(|m| {
                m.set_focus_lock_filter(
                    id,
                    egui::EventFilter {
                        tab: false,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                )
            });
        }
        focus
    }

    /// The menu box at the click point and its entries, then what the
    /// pointer did to them: hovering swaps the submenu, a press focuses an
    /// entry (mouse focus, no ring), a click runs it.
    fn draw_menu(&mut self, ui: &mut Ui, focus: &FocusMap) -> MenuBox {
        let props = self.props;
        let v = props.vars;
        let vp = props.viewport;
        let content = Content::Entries(props.items);
        let menu_layout = layout(&self.ctx, v, props.locale, &content, BoxKind::Menu);
        let size = menu_layout.size;
        let rect = Rect::from_min_size(
            vp.min + place_menu(props.at - vp.min, size, vp.size()),
            size,
        );

        // The box itself takes the pointer: nothing under it reacts. Its top
        // and left pixel edges excepted when it opened at the pointer:
        // WebView2 left the element under such a menu hovered, the pointer
        // on the menu's pixel-snapped corner at 100 and 150 %, while a
        // clamped menu that lands over the pointer takes it at once. A menu
        // a key opened hit-tests on its layout box, as Blink does: at 150 %
        // the pointer on its unsnapped left edge is over it (all measured).
        let hit = if self.st.from_key {
            rect
        } else {
            let hit = snap::rect(rect, self.ctx.pixels_per_point());
            Rect::from_min_max(hit.min + vec2(EDGE, EDGE), hit.max)
        };
        ui.interact(hit, self.menu_id, Sense::click());
        self.ctx
            .accesskit_node_builder(self.menu_id, |node| node.set_role(Role::Menu));
        paint_box(ui, rect, v, menu_layout.border);
        let input = draw_entries(
            ui,
            &Frame {
                rect,
                layout: &menu_layout,
                entries: props.items,
                submenu: false,
                id: self.menu_id,
                focused: focus.menu.find(self.focus_to.or(self.focused_id)),
                expanded: self.st.submenu,
                vars: v,
                locale: props.locale,
                keyboard: props.keyboard,
            },
        );
        self.hover(input.over);
        if let Some(t) = input.pressed {
            self.focus_to = Some(target_id(self.menu_id, t));
        }
        if let Some(t) = input.clicked {
            click(&mut self.st, props.items, t, false, &mut self.events);
        }
        MenuBox {
            rect,
            layout: menu_layout,
        }
    }

    /// Moves focus where the frame sent it and keeps the state for the next
    /// frame.
    fn finish(mut self, menu_rect: Rect, submenu_rect: Option<Rect>) -> Vec<MenuEvent> {
        if let Some(id) = self.focus_to {
            self.ctx.memory_mut(|m| m.request_focus(id));
        }
        self.st.menu_rect = Some(menu_rect);
        self.st.submenu_rect = submenu_rect;
        let st = self.st;
        self.ctx.data_mut(|d| d.insert_temp(state_id(), st));
        self.events
    }
}

/// A press this frame outside the menu and its submenu.
fn pressed_outside(ui: &Ui, menu: Rect, submenu: Option<Rect>) -> bool {
    ui.input(|i| {
        i.events.iter().any(|e| {
            matches!(e, Event::PointerButton { pos, pressed: true, .. }
                if !menu.contains(*pos) && !submenu.is_some_and(|r| r.contains(*pos)))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use accshift_app::actions::dialogs::AppMenuSwatch;
    use accshift_app::text::Text;
    use egui::{pos2, Key};

    pub(super) fn item(id: &str) -> AppMenuEntry {
        AppMenuEntry::item(id, Text::raw(id), AppMenuCommand::Custom(id.into()))
    }

    #[test]
    fn swatches_and_items_are_focusable_separators_not() {
        let entries = vec![
            item("a"),
            AppMenuEntry::Separator,
            AppMenuEntry::Swatches {
                id: "s".into(),
                label: Text::raw("Colour"),
                swatches: vec![
                    AppMenuSwatch {
                        id: "x".into(),
                        label: Text::raw("x"),
                        color: String::new(),
                        active: true,
                        command: AppMenuCommand::Custom("x".into()),
                    };
                    2
                ],
            },
        ];
        let t = targets(&entries);
        assert_eq!(t.len(), 3);
        assert_eq!(t[0].entry, 0);
        assert_eq!((t[1].entry, t[1].swatch), (2, Some(0)));
    }

    #[test]
    fn a_plain_entry_without_command_closes() {
        let entries = vec![AppMenuEntry::Item {
            id: "x".into(),
            label: Text::raw("x"),
            command: None,
            submenu: None,
            loader: None,
        }];
        let mut st = State::default();
        let mut events = Vec::new();
        click(
            &mut st,
            &entries,
            Target {
                entry: 0,
                swatch: None,
            },
            false,
            &mut events,
        );
        assert_eq!(events, vec![MenuEvent::Close]);
    }

    fn vars() -> Vars {
        Vars::new(&accshift_theme::ResolvedTheme::new(
            &accshift_theme::theme_application(
                &accshift_theme::ThemeRegistry::new().theme(Some("glass-dark")),
                100.0,
                true,
            ),
        ))
    }

    fn parent(id: &str) -> AppMenuEntry {
        AppMenuEntry::Item {
            id: id.into(),
            label: Text::raw(id),
            command: None,
            submenu: Some(vec![item("child")]),
            loader: None,
        }
    }

    /// The items come anew every frame: when the open submenu's entry goes
    /// or another takes its index, the submenu closes, and an empty menu
    /// draws without a panic.
    #[test]
    fn a_submenu_whose_entry_went_closes() {
        let ctx = Context::default();
        let vars = vars();
        let run = |items: &[AppMenuEntry]| {
            let props = ContextMenuProps {
                vars: &vars,
                locale: Locale::En,
                open_id: 7,
                items,
                at: pos2(10.0, 10.0),
                viewport: Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)),
                keyboard: false,
                loads: &[],
                now: 0.0,
            };
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                context_menu(ui, &props);
            });
            out.textures_delta.clear();
        };
        let submenu = || {
            ctx.data(|d| d.get_temp::<State>(state_id()))
                .unwrap()
                .submenu
        };
        let open = |items: &[AppMenuEntry], index: usize| {
            ctx.data_mut(|d| {
                let mut st: State = d.get_temp(state_id()).unwrap();
                open_submenu(&mut st, items, index, &mut Vec::new());
                d.insert_temp(state_id(), st);
            })
        };

        let full = vec![item("a"), item("b"), parent("c"), parent("d")];
        run(&full);
        open(&full, 2);
        run(&full);
        assert_eq!(submenu(), Some(2));

        // "a" goes: "d" now sits at index 2.
        let shifted = vec![item("b"), item("x"), parent("d")];
        run(&shifted);
        assert_eq!(submenu(), None);

        open(&shifted, 2);
        run(&[item("a")]);
        assert_eq!(submenu(), None);
        open(&shifted, 2);
        run(&[]);
        assert_eq!(submenu(), None);
    }

    /// egui also clicks a focused widget on Enter: the menu's key path is
    /// the only one that may run the entry.
    #[test]
    fn enter_on_a_focused_item_activates_it_once() {
        let ctx = Context::default();
        let vars = vars();
        let items = vec![item("a"), item("b")];
        let props = ContextMenuProps {
            vars: &vars,
            locale: Locale::En,
            open_id: 7,
            items: &items,
            at: pos2(10.0, 10.0),
            viewport: Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)),
            keyboard: true,
            loads: &[],
            now: 0.0,
        };
        let run = |events: Vec<Event>| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                events,
                ..Default::default()
            };
            let mut got = Vec::new();
            let mut out = ctx.run_ui(input, |ui| {
                got = context_menu(ui, &props);
            });
            out.textures_delta.clear();
            got
        };
        run(Vec::new());
        run(Vec::new());
        let first = target_id(
            Id::unique(("accshift::context-menu", 7_u64)).with("menu"),
            Target {
                entry: 0,
                swatch: None,
            },
        );
        assert_eq!(ctx.memory(|m| m.focused()), Some(first));
        let got = run(vec![Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        let runs = got
            .iter()
            .filter(|e| matches!(e, MenuEvent::Activate(_)))
            .count();
        assert_eq!(runs, 1);
    }
}
