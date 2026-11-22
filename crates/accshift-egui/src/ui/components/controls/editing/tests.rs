use super::layout::{nearest, Row};
use super::words::{paragraph, word_at, word_left, word_right, word_with_space};
use super::*;
use egui::{pos2, Key, Modifiers};

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

fn type_keys(ed: &mut Editor, value: &mut String, events: &[Event], rules: &Rules) -> Edited {
    let ctx = Context::default();
    ed.keys(&ctx, value, events, rules)
}

fn key(key: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

#[test]
fn ctrl_arrows_move_by_word_like_chromium_on_windows() {
    let c = chars("foo bar, baz.qux  end");
    // Right: past the word and the spaces after it.
    assert_eq!(word_right(&c, 0, false), 4);
    assert_eq!(word_right(&c, 4, false), 7);
    // Punctuation is its own stop, then its spaces go.
    assert_eq!(word_right(&c, 7, false), 9);
    // A period between letters joins them (UAX #29 MidNumLet).
    assert_eq!(word_right(&c, 9, false), 18);
    assert_eq!(word_right(&c, 18, false), 21);
    // Left: back over spaces, to the run's start.
    assert_eq!(word_left(&c, 21, false), 18);
    assert_eq!(word_left(&c, 18, false), 9);
    assert_eq!(word_left(&c, 9, false), 7);
    assert_eq!(word_left(&c, 7, false), 4);
    assert_eq!(word_left(&c, 3, false), 0);
    // A password is one word.
    assert_eq!(word_right(&c, 2, true), c.len());
    assert_eq!(word_left(&c, 9, true), 0);
}

#[test]
fn apostrophes_and_decimal_points_stay_inside_words() {
    let c = chars("don't 3.14 a.");
    assert_eq!(word_right(&c, 0, false), 6);
    assert_eq!(word_right(&c, 6, false), 11);
    assert_eq!(word_at(&c, 7, false), 6..10);
    // A trailing period is punctuation.
    assert_eq!(word_at(&c, 12, false), 12..13);
}

#[test]
fn a_double_click_selects_the_word_and_its_trailing_spaces() {
    let c = chars("alpha  beta");
    assert_eq!(word_with_space(&c, 2, false), 0..7);
    assert_eq!(word_with_space(&c, 9, false), 7..11);
    // Past the end: the last word.
    assert_eq!(word_with_space(&c, 11, false), 7..11);
    assert_eq!(word_with_space(&c, 3, true), 0..11);
}

#[test]
fn pasted_line_breaks_become_spaces_and_trailing_ones_go() {
    assert_eq!(paste_text("a\r\nb\nc\rd\n\n"), "a b c d");
    assert_eq!(paste_text("  cookie  \n"), "  cookie  ");
}

#[test]
fn typing_replaces_the_selection_and_respects_maxlength_in_utf16() {
    let rules = Rules {
        max_len: Some(4),
        ..Rules::text()
    };
    let mut value = "ab".to_string();
    let mut ed = Editor::default();
    ed.sync(&value);
    assert_eq!(ed.caret, 2);
    // "😀" is two UTF-16 units: only one fits after "ab" plus "c".
    let out = type_keys(&mut ed, &mut value, &[Event::Text("c😀d".into())], &rules);
    assert!(out.changed);
    assert_eq!(value, "abc");
    ed.anchor = 0;
    ed.caret = 2;
    type_keys(&mut ed, &mut value, &[Event::Text("xyz".into())], &rules);
    assert_eq!(value, "xyzc");
    assert_eq!((ed.caret, ed.anchor), (3, 3));
}

/// The word modifier: Ctrl, Option on macOS.
const WORD: Modifiers = if cfg!(target_os = "macos") {
    Modifiers::ALT
} else {
    Modifiers::CTRL
};

#[test]
fn the_caret_moves_inserts_and_deletes_in_the_middle() {
    let rules = Rules::text();
    let mut value = "hello world".to_string();
    let mut ed = Editor::default();
    ed.sync(&value);
    let none = Modifiers::NONE;
    type_keys(
        &mut ed,
        &mut value,
        &[key(Key::ArrowLeft, WORD), key(Key::ArrowLeft, none)],
        &rules,
    );
    assert_eq!(ed.caret, 5);
    type_keys(&mut ed, &mut value, &[Event::Text(",".into())], &rules);
    assert_eq!(value, "hello, world");
    type_keys(&mut ed, &mut value, &[key(Key::Delete, none)], &rules);
    assert_eq!(value, "hello,world");
    // The comma is a word of its own.
    type_keys(&mut ed, &mut value, &[key(Key::Backspace, WORD)], &rules);
    assert_eq!(value, "helloworld");
    assert_eq!(ed.caret, 5);
    // Shift+End selects to the end; typing replaces it.
    type_keys(
        &mut ed,
        &mut value,
        &[key(Key::End, Modifiers::SHIFT), Event::Text("X".into())],
        &rules,
    );
    assert_eq!(value, "helloX");
    // Arrows collapse a selection to its side.
    let mut value = "abcd".to_string();
    let mut ed = Editor::default();
    ed.sync(&value);
    type_keys(
        &mut ed,
        &mut value,
        &[key(Key::A, Modifiers::COMMAND)],
        &rules,
    );
    assert_eq!(ed.selection(), 0..4);
    type_keys(&mut ed, &mut value, &[key(Key::ArrowLeft, none)], &rules);
    assert_eq!((ed.caret, ed.anchor), (0, 0));
}

#[test]
fn typing_runs_undo_as_one_step_and_redo_brings_them_back() {
    let rules = Rules::text();
    let mut value = String::new();
    let mut ed = Editor::default();
    ed.sync(&value);
    for c in ["a", "b", "c"] {
        type_keys(&mut ed, &mut value, &[Event::Text(c.into())], &rules);
    }
    type_keys(
        &mut ed,
        &mut value,
        &[key(Key::ArrowLeft, Modifiers::NONE)],
        &rules,
    );
    type_keys(&mut ed, &mut value, &[Event::Paste("XY".into())], &rules);
    assert_eq!(value, "abXYc");
    let undo = key(Key::Z, Modifiers::COMMAND);
    type_keys(&mut ed, &mut value, std::slice::from_ref(&undo), &rules);
    assert_eq!(value, "abc");
    type_keys(&mut ed, &mut value, &[undo], &rules);
    assert_eq!(value, "");
    type_keys(
        &mut ed,
        &mut value,
        &[key(Key::Y, Modifiers::COMMAND)],
        &rules,
    );
    assert_eq!(value, "abc");
}

#[test]
fn a_password_never_reaches_the_clipboard() {
    let ctx = Context::default();
    let secret = Rules {
        secret: true,
        ..Rules::text()
    };
    let mut value = "hunter2".to_string();
    let mut ed = Editor::default();
    ed.sync(&value);
    ed.select_all(&value);
    let out = ed.keys(&ctx, &mut value, &[Event::Cut], &secret);
    assert!(!out.changed);
    assert_eq!(value, "hunter2");
    let out = ed.keys(&ctx, &mut value, &[Event::Cut], &Rules::text());
    assert!(out.changed);
    assert_eq!(value, "");
}

#[test]
fn a_value_set_from_outside_moves_the_caret_to_its_end() {
    let mut ed = Editor::default();
    ed.sync("abc");
    ed.caret = 1;
    ed.anchor = 1;
    ed.known = Some("abc".into());
    ed.sync("abc");
    assert_eq!(ed.caret, 1);
    ed.sync("");
    assert_eq!((ed.caret, ed.anchor), (0, 0));
}

#[test]
fn the_number_filter_applies_to_pasted_text() {
    let rules = Rules {
        accepts: |c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E'),
        ..Rules::text()
    };
    let mut value = String::new();
    let mut ed = Editor::default();
    ed.sync(&value);
    type_keys(&mut ed, &mut value, &[Event::Paste("1 2a3".into())], &rules);
    assert_eq!(value, "123");
}

/// Two rows of four 10 px characters, the first wrapped after "abc ",
/// then a line break and "de".
fn wrapped_rows() -> Rows {
    let row = |start: usize, n: usize, newline: bool| Row {
        start,
        xs: (0..=n).map(|i| i as f32 * 10.0).collect(),
        newline,
    };
    Rows {
        rows: vec![row(0, 4, false), row(4, 3, true), row(8, 2, false)],
        top: 0.0,
        height: 16.0,
        page: 0.0,
        preedit: 0..0,
    }
}

#[test]
fn a_textarea_moves_by_row_and_keeps_its_column() {
    // "abc defg\nde": "abc " wraps, "efg" ends at the break.
    let mut value = "abc efg\nde".to_string();
    let rows = wrapped_rows();
    let ctx = Context::default();
    let rules = Rules::area(100);
    let mut ed = Editor::default();
    ed.sync(&value);
    ed.caret = 2;
    ed.anchor = 2;
    let down = key(Key::ArrowDown, Modifiers::NONE);
    ed.area_keys(&ctx, &mut value, std::slice::from_ref(&down), &rules, &rows);
    assert_eq!(ed.caret, 6);
    // The last row is shorter: its end, and the column comes back.
    ed.area_keys(&ctx, &mut value, std::slice::from_ref(&down), &rules, &rows);
    assert_eq!(ed.caret, 10);
    let up = key(Key::ArrowUp, Modifiers::NONE);
    ed.area_keys(&ctx, &mut value, std::slice::from_ref(&up), &rules, &rows);
    assert_eq!(ed.caret, 6);
    // Past the first row: the start.
    ed.area_keys(&ctx, &mut value, &[up.clone(), up], &rules, &rows);
    assert_eq!(ed.caret, 0);
    // End on the wrapped row stays on it, after its space.
    ed.area_keys(
        &ctx,
        &mut value,
        &[key(Key::End, Modifiers::NONE)],
        &rules,
        &rows,
    );
    assert_eq!((ed.caret, ed.upstream), (4, true));
    assert_eq!(rows.locate(ed.caret, ed.upstream), (0, 40.0));
    assert_eq!(rows.locate(ed.caret, false), (1, 0.0));
    // Ctrl+End: the text's end.
    ed.area_keys(
        &ctx,
        &mut value,
        &[key(Key::End, Modifiers::COMMAND)],
        &rules,
        &rows,
    );
    assert_eq!(ed.caret, 10);
}

#[test]
fn a_textarea_pages_by_whole_rows() {
    // Ten rows "ab\n" of 16 points; a 40 point page reaches two rows.
    let mut value = "ab\n".repeat(9) + "ab";
    let row = |i: usize| Row {
        start: i * 3,
        xs: vec![0.0, 10.0, 20.0],
        newline: i < 9,
    };
    let rows = Rows {
        rows: (0..10).map(row).collect(),
        top: 0.0,
        height: 16.0,
        page: 0.0,
        preedit: 0..0,
    }
    .paging(46.0, 1.0);
    assert_eq!(rows.page, 40.0);
    let ctx = Context::default();
    let rules = Rules::area(100);
    let mut ed = Editor::default();
    ed.sync(&value);
    ed.caret = 1;
    ed.anchor = 1;
    let down = key(Key::PageDown, Modifiers::NONE);
    let out = ed.area_keys(&ctx, &mut value, std::slice::from_ref(&down), &rules, &rows);
    assert!(out.page && !out.changed);
    assert_eq!((ed.caret, ed.anchor), (7, 7));
    // Shift extends and does not align the line.
    let out = ed.area_keys(
        &ctx,
        &mut value,
        &[key(Key::PageDown, Modifiers::SHIFT)],
        &rules,
        &rows,
    );
    assert!(!out.page);
    assert_eq!((ed.anchor, ed.caret), (7, 13));
    // From row 8 the last row is in reach: the end.
    ed.caret = 25;
    ed.anchor = 25;
    ed.goal_x = None;
    ed.area_keys(&ctx, &mut value, std::slice::from_ref(&down), &rules, &rows);
    assert_eq!(ed.caret, 29);
    let up = key(Key::PageUp, Modifiers::NONE);
    ed.area_keys(&ctx, &mut value, std::slice::from_ref(&up), &rules, &rows);
    // The column from before the end comes back.
    assert_eq!(ed.caret, 22);
    ed.area_keys(
        &ctx,
        &mut value,
        &[up.clone(), up.clone(), up.clone()],
        &rules,
        &rows,
    );
    assert_eq!(ed.caret, 4);
    ed.area_keys(&ctx, &mut value, std::slice::from_ref(&up), &rules, &rows);
    assert_eq!(ed.caret, 0);
}

#[test]
fn a_textarea_breaks_lines_and_keeps_pasted_ones() {
    let ctx = Context::default();
    let rules = Rules::area(12);
    let mut value = "a".to_string();
    let mut ed = Editor::default();
    ed.sync(&value);
    let out = ed.keys(
        &ctx,
        &mut value,
        &[
            key(Key::Enter, Modifiers::NONE),
            Event::Paste("b\r\nc\rd\n".into()),
        ],
        &rules,
    );
    assert!(out.changed && !out.enter);
    assert_eq!(value, "a\nb\nc\nd\n");
    // maxlength counts the breaks.
    ed.keys(&ctx, &mut value, &[Event::Paste("efghij".into())], &rules);
    assert_eq!(value, "a\nb\nc\nd\nefgh");
}

#[test]
fn a_textarea_hit_and_selection_follow_the_rows() {
    let rows = wrapped_rows();
    assert_eq!(rows.hit(pos2(14.0, 3.0)), (1, false));
    // Past a wrapped row's end: after its space, on that row.
    assert_eq!(rows.hit(pos2(90.0, 3.0)), (4, true));
    assert_eq!(rows.hit(pos2(90.0, 20.0)), (7, false));
    assert_eq!(rows.hit(pos2(5.0, 99.0)), (8, false));
    // A selection over the break paints it as `newline` wide.
    assert_eq!(
        rows.spans(2..9, 3.0),
        vec![(0, 20.0, 40.0), (1, 0.0, 33.0), (2, 0.0, 10.0)]
    );
    assert_eq!(paragraph(&chars("ab\ncd\nef"), 4), 3..6);
    assert_eq!(paragraph(&chars("ab\ncd\nef"), 8), 6..8);
}

#[test]
fn the_nearest_boundary_wins() {
    let offsets = [0.0, 10.0, 20.0, 35.0];
    assert_eq!(nearest(&offsets, -5.0), 0);
    assert_eq!(nearest(&offsets, 4.9), 0);
    assert_eq!(nearest(&offsets, 5.1), 1);
    assert_eq!(nearest(&offsets, 28.0), 3);
    assert_eq!(nearest(&offsets, 99.0), 3);
}
