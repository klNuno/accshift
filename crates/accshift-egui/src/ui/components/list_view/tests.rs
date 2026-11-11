use super::avatar::box_uv;
use super::preview::{flex_shrink, preview_note};
use super::row::info_height;
use super::*;
use accshift_app::display::{AvatarView, SectionHeaderItem, SectionLabel};
use accshift_app::i18n::translate;
use egui::{Context, Event, Key, Modifiers, PointerButton, RawInput};

struct Fake;

impl ListHost for Fake {
    fn mount(&mut self, key: &str) -> Id {
        Id::unique(key)
    }

    fn avatar(&mut self, _: &str, _: &AccountCardItem, _: u32, _: bool) -> AvatarContent {
        AvatarContent::Loading
    }

    fn blurred(&mut self, _: &AccountCardItem, _: u32, _: f32) -> Option<TextureId> {
        None
    }

    fn card_color(&self, _: &str) -> Option<Rgba> {
        None
    }
}

fn account(id: &str, pending_setup: bool, single_click_switch: bool) -> DisplayItem {
    DisplayItem::Account(Box::new(AccountCardItem {
        account: PlatformAccount {
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
        avatar: AvatarView::default(),
        warning: None,
        dragged: false,
        pending_setup,
        single_click_switch,
        bulk_selected: false,
    }))
}

/// Drives `list_view` over an 800 by 400 box at the window's top left.
struct Bench {
    ctx: Context,
    vars: Vars,
    state: ListState,
    items: Vec<DisplayItem>,
    time: f64,
    locale: Locale,
}

impl Bench {
    fn new(items: Vec<DisplayItem>) -> Bench {
        let vars = Vars::new(&accshift_theme::ResolvedTheme::new(
            &accshift_theme::theme_application(
                &accshift_theme::ThemeRegistry::new().theme(Some("glass-dark")),
                100.0,
                true,
            ),
        ));
        Bench {
            ctx: Context::default(),
            vars,
            state: ListState::default(),
            items,
            time: 0.0,
            locale: Locale::En,
        }
    }

    fn frame(&mut self, events: Vec<Event>) -> Vec<ListEvent> {
        self.frame_with(events, Modifiers::NONE)
    }

    fn frame_with(&mut self, events: Vec<Event>, modifiers: Modifiers) -> Vec<ListEvent> {
        self.time += 0.02;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 400.0))),
            time: Some(self.time),
            events: [Event::ModifiersChanged(modifiers)]
                .into_iter()
                .chain(events)
                .collect(),
            ..Default::default()
        };
        let time = TimeContext {
            now_ms: 0.0,
            locale: Locale::En,
            zone: accshift_app::time::TimeZone::UTC,
        };
        let props = ListProps {
            vars: &self.vars,
            locale: self.locale,
            accent: self.vars.accent,
            focus_visible: false,
            dark: true,
            time: &time,
            kb_focus: None,
            kb_reveal: false,
            drag_over_folder: None,
            drag_over_back: false,
            dragging: false,
            ghost: None,
        };
        let (state, items) = (&mut self.state, &self.items);
        let mut out = Vec::new();
        let mut output = self.ctx.run_ui(input, |ui| {
            let rect = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 400.0));
            out = list_view(ui, rect, state, items, &props, &mut Fake);
        });
        output.textures_delta.clear();
        out
    }

    fn press(&mut self, at: Pos2, button: PointerButton) -> Vec<ListEvent> {
        let mut out = self.frame(vec![Event::PointerMoved(at)]);
        for pressed in [true, false] {
            out.extend(self.frame(vec![Event::PointerButton {
                pos: at,
                button,
                pressed,
                modifiers: Modifiers::NONE,
            }]));
        }
        out
    }
}

fn key(key: Key, modifiers: Modifiers) -> Vec<Event> {
    [true, false]
        .map(|pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        })
        .to_vec()
}

/// The rows' centres: the panel's padding 4, rows 46 tall, 2 apart.
const ROW0: Pos2 = pos2(200.0, 27.0);
const ROW1: Pos2 = pos2(200.0, 75.0);

#[test]
fn a_click_selects_and_a_double_click_switches() {
    let mut bench = Bench::new(vec![account("a", false, false), account("b", false, false)]);
    bench.frame(Vec::new());
    let boxes: Vec<Rect> = bench.state.layout.iter().map(|(_, r)| *r).collect();
    assert_eq!(
        boxes[0],
        Rect::from_min_size(pos2(4.0, 4.0), vec2(591.0, 46.0))
    );
    assert_eq!(boxes[1].top(), 52.0);

    let events = bench.press(ROW1, PointerButton::Primary);
    assert_eq!(events, [ListEvent::Activate("b".into())]);
    assert_eq!(bench.state.selected.as_deref(), Some("b"));

    let events = bench.press(ROW1, PointerButton::Primary);
    assert!(
        events.contains(&ListEvent::Switch("b".into())),
        "{events:?}"
    );

    let events = bench.press(ROW0, PointerButton::Secondary);
    assert!(matches!(&events[..], [ListEvent::AccountMenu(a, _)] if a.id == "a"));
}

#[test]
fn the_preview_column_shrinks_like_blink() {
    // 30 px too tall with a 120 px avatar and one 14.4 px meta line:
    // Blink gives 26.79 and 3.21 px, rounded to 1/64.
    let (avatar, meta) = flex_shrink(30.0, 120.0, 14.4, 1.0);
    assert_eq!(avatar, 93.21875);
    assert!((meta - 11.184).abs() < 1.0 / 64.0, "{meta}");
    assert_eq!(flex_shrink(-4.0, 120.0, 14.4, 1.0), (120.0, 14.4));
    assert_eq!(flex_shrink(500.0, 120.0, 14.4, 1.5), (0.0, 0.0));
    let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    let uv = box_uv(
        full,
        Rect::from_min_size(pos2(0.0, 0.0), vec2(120.0, 90.0)),
        1.0,
    );
    assert_eq!(uv, Rect::from_min_max(pos2(0.0, 0.125), pos2(1.0, 0.875)));
    // A 93.2 px box at y 108: the picture snaps to 95..215, the box to
    // 108..201, so the band starts on a whole texel.
    let rect = Rect::from_min_size(pos2(674.0, 108.0), vec2(120.0, 93.21875));
    let uv = box_uv(full, rect, 1.0);
    assert_eq!(uv.top() * 120.0, 13.0);
    assert_eq!(uv.bottom() * 120.0, 106.0);
}

#[test]
fn bulk_edit_switches_on_one_click_and_never_previews() {
    let mut bench = Bench::new(vec![account("a", false, true), account("p", true, true)]);
    bench.frame(Vec::new());
    let events = bench.press(ROW0, PointerButton::Primary);
    assert_eq!(
        events,
        [
            ListEvent::Activate("a".into()),
            ListEvent::Switch("a".into())
        ]
    );
    assert_eq!(bench.state.selected, None);
    // The add flow's placeholder takes no switch.
    let events = bench.press(ROW1, PointerButton::Primary);
    assert_eq!(events, [ListEvent::Activate("p".into())]);
}

#[test]
fn a_selection_that_left_the_list_goes() {
    let mut bench = Bench::new(vec![account("a", false, false)]);
    bench.frame(Vec::new());
    bench.press(ROW0, PointerButton::Primary);
    assert_eq!(bench.state.selected.as_deref(), Some("a"));
    bench.items = vec![account("z", false, false)];
    bench.frame(Vec::new());
    assert_eq!(bench.state.selected, None);
}

#[test]
fn the_wheel_scrolls_a_long_panel() {
    let items = (0..20)
        .map(|i| account(&format!("a{i}"), false, false))
        .collect();
    let mut bench = Bench::new(items);
    bench.frame(Vec::new());
    // 20 rows of 46, 2 apart, padding 4: 966 px in a 400 px panel, so
    // the rows leave room for the 10 px scrollbar.
    assert_eq!(bench.state.layout[0].1.width(), 581.0);
    bench.frame(vec![
        Event::PointerMoved(ROW0),
        Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -10_000.0),
            modifiers: Modifiers::NONE,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
        },
    ]);
    assert_eq!(bench.state.scroll, 566.0);
    bench.frame(Vec::new());
    assert_eq!(bench.state.layout[19].1.bottom(), 396.0);
}

fn header(collapsed: bool) -> DisplayItem {
    DisplayItem::SectionHeader(SectionHeaderItem {
        label: SectionLabel::Folder(FolderInfo {
            id: "f".into(),
            name: "f".into(),
            parent_id: None,
            platform: "steam".into(),
        }),
        collapse_key: "f".into(),
        count: 2,
        card_color: String::new(),
        collapsed,
        dragged: false,
    })
}

fn top_of(bench: &Bench, label: &str) -> f32 {
    let found = bench.state.layout.iter().find(|(l, _)| l == label);
    found.expect(label).1.top()
}

/// A folding section slides its rows out over 220 ms, cubic ease-out; the
/// root section below follows. Box tops: folder header 4, rows 30 and
/// 78, root header 126, its row 151.
#[test]
fn a_folding_section_slides_its_rows_out() {
    let root = DisplayItem::SectionHeader(SectionHeaderItem {
        label: SectionLabel::Root,
        collapse_key: "root".into(),
        count: 1,
        card_color: String::new(),
        collapsed: false,
        dragged: false,
    });
    let open = vec![
        header(false),
        account("a", false, false),
        account("b", false, false),
        root.clone(),
        account("c", false, false),
    ];
    let mut bench = Bench::new(open.clone());
    bench.frame(Vec::new());
    assert_eq!(top_of(&bench, "b"), 78.0);
    assert_eq!(top_of(&bench, "c"), 151.0);

    bench.items = vec![header(true), root, account("c", false, false)];
    bench.frame(Vec::new());
    assert_eq!(top_of(&bench, "c"), 151.0);
    for _ in 0..5 {
        bench.frame(Vec::new());
    }
    // 100 ms in: the 94 px of rows at 1 - cubic_out(0.1 / 0.22).
    let left = 1.0 - (1.0 - (1.0 - 0.1f32 / 0.22).powi(3));
    let c = top_of(&bench, "c");
    assert!(
        (c - (30.0 + 94.0 * left + 2.0 + 23.0 + 2.0)).abs() < 0.5,
        "{c}"
    );
    assert_eq!(top_of(&bench, "a"), 30.0);
    for _ in 0..10 {
        bench.frame(Vec::new());
    }
    assert_eq!(top_of(&bench, "c"), 55.0);
    assert!(bench.state.layout.iter().all(|(l, _)| l != "a"));

    // Unfolding slides them back in.
    bench.items = open;
    bench.frame(Vec::new());
    assert_eq!(top_of(&bench, "c"), 57.0);
    for _ in 0..15 {
        bench.frame(Vec::new());
    }
    assert_eq!(top_of(&bench, "c"), 151.0);
}

/// Shift+F10 and the Menu key open a focused row's menu, as they do a
/// grid card's.
#[test]
fn the_menu_keys_open_a_focused_rows_menu() {
    let mut bench = Bench::new(vec![account("a", false, false), account("b", false, false)]);
    bench.frame(Vec::new());
    bench.frame(key(Key::Tab, Modifiers::NONE));
    bench.frame(Vec::new());
    let focused = bench.ctx.memory(|m| m.focused());
    assert_eq!(focused, Some(Id::unique("list/a/a")));
    for (k, modifiers) in [
        (Key::F10, Modifiers::SHIFT),
        (crate::CONTEXT_MENU_KEY, Modifiers::NONE),
    ] {
        let events = bench.frame_with(key(k, modifiers), modifiers);
        assert!(
            matches!(&events[..], [ListEvent::AccountMenu(a, _)] if a.id == "a"),
            "{k:?}: {events:?}"
        );
    }
    // Without focus the keys do nothing.
    bench
        .ctx
        .memory_mut(|m| m.surrender_focus(Id::unique("list/a/a")));
    bench.frame(Vec::new());
    let events = bench.frame_with(key(Key::F10, Modifiers::SHIFT), Modifiers::SHIFT);
    assert!(events.is_empty(), "{events:?}");
}

/// An account in setup shows no username or last login line, and a
/// blank note no note block, like their grid cards.
#[test]
fn pending_setup_and_blank_notes_show_no_meta() {
    let vars = Bench::new(Vec::new()).vars;
    let DisplayItem::Account(mut a) = account("a", true, false) else {
        unreachable!()
    };
    a.show_username = true;
    a.show_last_login = true;
    a.note = "  \n ".into();
    let name = 12.0 * vars.line_height;
    assert_eq!(info_height(&vars, Some(&a)), name);
    assert!(!has_username(&a) && !shows_last_login(&a));
    assert_eq!(preview_note(&a), None);

    a.pending_setup = false;
    assert_eq!(info_height(&vars, Some(&a)), name + 24.0);
    a.note = " hi\n".into();
    assert_eq!(preview_note(&a), Some("hi"));
}

/// The back row's layout label follows the locale like its painted one.
#[test]
fn the_back_row_label_is_localized() {
    let mut bench = Bench::new(vec![DisplayItem::Back, account("a", false, false)]);
    bench.locale = Locale::Fr;
    bench.frame(Vec::new());
    assert_eq!(
        bench.state.layout[0].0,
        translate(Locale::Fr, "common.back")
    );
    assert_ne!(bench.state.layout[0].0, "Back");
}
