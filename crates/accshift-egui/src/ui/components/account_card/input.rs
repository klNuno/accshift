//! Sensing the card: its AccessKit node, its hover and press, and the
//! arming that makes a first press select and a second one switch.

use egui::accesskit::Role;
use egui::{Context, Key, Rect, Response, Ui, WidgetInfo};

use accshift_app::display::AccountCardItem;

use super::{Card, CardEvent, State};
use crate::ui::components::{self as components, folder_card};

/// Senses the card box as a button, announced with the account's name and
/// whether it is the active one.
pub(super) fn sense(ui: &mut Ui, card: &Card) -> Response {
    let item = card.item();
    // An account still in setup takes its clicks, so none reaches the
    // workspace, and does nothing with them.
    let enabled = !item.pending_setup;
    folder_card::sense_button(ui, card.rect(), card.id(), || {
        WidgetInfo::selected(Role::Button, enabled, item.active, &item.name)
    })
}

/// The pointer over the card this frame.
#[derive(Clone, Copy, Debug)]
pub(super) struct Hover {
    /// Over the card or one of the extension chips it showed last frame.
    pub hovered: bool,
    /// The hover started this frame.
    pub entered: bool,
    pub pressed: bool,
    /// Hovered, neither active nor dragged: the card lifts.
    pub lift: bool,
}

impl Hover {
    pub fn read(ctx: &Context, response: &Response, st: &State, item: &AccountCardItem) -> Self {
        let on_chip = || {
            st.chips
                .iter()
                .any(|chip| ctx.read_response(*chip).is_some_and(|r| r.hovered()))
        };
        let hovered = components::css_hovered(response) || on_chip();
        Self {
            hovered,
            entered: hovered && !st.hovered,
            pressed: components::held(response),
            lift: hovered && !item.active && !item.dragged,
        }
    }
}

/// A press that started outside `rect` this frame.
fn pressed_outside(ctx: &Context, rect: Rect) -> bool {
    ctx.input(|i| {
        i.pointer.any_pressed() && i.pointer.press_origin().is_some_and(|p| !rect.contains(p))
    })
}

/// Shift+F10 or the context menu key.
fn menu_key_pressed(ctx: &Context) -> bool {
    ctx.input(|i| {
        (i.modifiers.shift && i.key_pressed(Key::F10)) || i.key_pressed(crate::CONTEXT_MENU_KEY)
    })
}

/// The arming rule of a press: it switches at once when one press switches
/// (bulk edit); otherwise the first press arms the card and the second one
/// disarms it and switches.
fn press_switches(st: &mut State, item: &AccountCardItem) -> bool {
    if item.single_click_switch {
        return true;
    }
    st.confirm = !st.confirm;
    !st.confirm
}

/// Updates the armed state from this frame's input and returns the events,
/// in the order the workspace handles them.
pub(super) fn arm(ui: &Ui, response: &Response, st: &mut State, card: &Card) -> Vec<CardEvent> {
    let ctx = ui.ctx();
    let item = card.item();
    // A drag, a press elsewhere or the focus leaving disarms the card: Tab
    // then Enter on another card arms that one, and only one card may wait
    // for its second press.
    let has_focus = response.has_focus();
    let focus_left = st.had_focus && !has_focus;
    if st.confirm && (item.dragged || pressed_outside(ctx, card.rect()) || focus_left) {
        st.confirm = false;
    }
    if item.pending_setup {
        st.had_focus = has_focus;
        return Vec::new();
    }
    // A press focuses the card like any button: its keys then reach it.
    let keys = components::controls::take_focus(ui, response);
    st.had_focus = has_focus || keys;
    let key_menu = keys && menu_key_pressed(ctx);

    let mut events = Vec::new();
    if !item.dragged && components::clicked(response) {
        events.push(CardEvent::Activate);
        if press_switches(st, item) {
            events.push(CardEvent::Switch);
        }
    }
    if response.secondary_clicked() || key_menu {
        // From the keyboard the menu opens at the layout box's centre, not
        // its snapped edges.
        let at = if key_menu {
            card.layout().center()
        } else {
            components::context_click_pos(ui).unwrap_or(card.rect().center())
        };
        events.push(CardEvent::Activate);
        st.confirm = false;
        events.push(CardEvent::ContextMenu(at));
    }
    events
}

#[cfg(test)]
mod tests {
    use egui::{pos2, vec2, Id, Pos2};

    use accshift_app::i18n::Locale;
    use accshift_app::time::TimeContext;

    use super::super::{account_card, state, AccountCardProps, AvatarContent};
    use super::*;
    use crate::app::vars::Vars;
    use crate::ui::text;

    fn item(id: &str) -> AccountCardItem {
        AccountCardItem {
            account: accshift_app::platforms::PlatformAccount {
                id: id.into(),
                display_name: id.into(),
                username: id.into(),
                last_login_at_sec: None,
            },
            name: id.into(),
            username: id.into(),
            card_color: String::new(),
            note: String::new(),
            show_note_inline: false,
            show_username: false,
            show_last_login: false,
            last_login_at_sec: None,
            last_login_unknown_key: "time.unknown",
            active: false,
            switching: false,
            avatar: accshift_app::display::AvatarView::default(),
            warning: None,
            dragged: false,
            pending_setup: false,
            single_click_switch: false,
            bulk_selected: false,
        }
    }

    /// Enter arms a card for its second press; Tab then Enter on another
    /// card arms that one and disarms the first, so a later Enter back on
    /// the first arms it again instead of switching.
    #[test]
    fn one_card_at_a_time_waits_for_its_second_press() {
        let ctx = Context::default();
        text::install(&ctx, false);
        let vars = Vars::new(&accshift_theme::ResolvedTheme::new(
            &accshift_theme::theme_application(
                &accshift_theme::ThemeRegistry::new().theme(Some("glass-dark")),
                100.0,
                true,
            ),
        ));
        let time = TimeContext {
            now_ms: 0.0,
            locale: Locale::En,
            zone: accshift_app::time::TimeZone::UTC,
        };
        let cards = [item("a"), item("b")];
        let ids = [Id::unique("card-a"), Id::unique("card-b")];
        let mut clock = 0.0;
        let mut frame = |key: Option<Key>| {
            clock += 0.05;
            let events = key
                .map(|key| {
                    [true, false]
                        .map(|pressed| egui::Event::Key {
                            key,
                            physical_key: None,
                            pressed,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        })
                        .to_vec()
                })
                .unwrap_or_default();
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0))),
                time: Some(clock),
                events,
                ..Default::default()
            };
            let mut out = Vec::new();
            let mut full = ctx.run_ui(input, |ui| {
                for (k, (item, id)) in cards.iter().zip(ids).enumerate() {
                    let rect = Rect::from_min_size(
                        pos2(10.0 + 110.0 * k as f32, 10.0),
                        vec2(100.0, 136.0),
                    );
                    let props = AccountCardProps {
                        vars: &vars,
                        id,
                        item,
                        card_color: None,
                        avatar: AvatarContent::Loading,
                        focus_visible: true,
                        locale: Locale::En,
                        time: &time,
                        badge: None,
                        extension: &[],
                        extension_disabled: true,
                        extension_forced: false,
                        extension_hover_disabled: false,
                        clickable_chips: &[],
                        glass: false,
                        kb_focus: false,
                        grid_dragging: false,
                        ghost: false,
                    };
                    out.push(account_card(ui, rect, &props).events);
                }
            });
            full.textures_delta.clear();
            out
        };
        let armed = |k: usize| state(&ctx, ids[k]).confirm;

        frame(None);
        frame(Some(Key::Tab));
        assert!(ctx.memory(|m| m.has_focus(ids[0])));
        frame(Some(Key::Enter));
        assert!(armed(0));

        frame(Some(Key::Tab));
        assert!(ctx.memory(|m| m.has_focus(ids[1])));
        frame(Some(Key::Enter));
        assert!(armed(1));
        assert!(!armed(0), "the first card is still armed");

        ctx.memory_mut(|m| m.request_focus(ids[0]));
        frame(None);
        let events = frame(Some(Key::Enter));
        assert!(!events[0].contains(&CardEvent::Switch), "{:?}", events[0]);
        assert!(armed(0));
    }
}
