//! End-to-end editing tests: drive real `KeyInput`s into a focused editor through the
//! framework's key dispatch and assert the bound `Signal<String>` — exercising the whole
//! path (key → transaction → dispatch → rope → sync), including undo/redo grouping.

use pebbles::core::{Ui, create_root_signal};
use pebbles::prelude::*;
use pebbles::render::TextEnv;
use pebbles_code_editor::code_editor;

fn white() -> Color {
    Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xFF)
}

/// Mount an autofocused editor bound to a fresh root signal, run a frame so focus + effects
/// settle, and return the harness + the bound code signal.
fn harness(initial: &str) -> (Ui, TextEnv, Signal<String>) {
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    let code = create_root_signal(String::from(initial));
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(View::new(white(), code_editor(code).autofocus()).into_widget());
    for _ in 0..3 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, Size::new(600.0, 400.0));
    }
    (ui, env, code)
}

fn key(ui: &mut Ui, env: &mut TextEnv, k: KeyInput) {
    ui.dispatch_key(k);
    ui.rebuild_if_dirty();
    ui.layout(env, Size::new(600.0, 400.0));
}

/// Like [`harness`] but with the gutter off, so click x-coordinates are just
/// `pad_l + col * advance` (no gutter width to account for) — for mouse tests.
fn mouse_harness(initial: &str) -> (Ui, TextEnv, Signal<String>) {
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    pebbles::core::keyboard::set_modifiers(false, false, false, false);
    let code = create_root_signal(String::from(initial));
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(View::new(white(), code_editor(code).gutter(false).autofocus()).into_widget());
    for _ in 0..3 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, Size::new(600.0, 400.0));
    }
    (ui, env, code)
}

/// The monospace grid geometry (mirrors the editor's defaults: fs 13.5, lh 1.6).
const ADV: f64 = 13.5 * 0.6;
const LINE: f64 = 13.5 * 1.6;
/// Window offset of column `col` on line `line` (with the gutter off).
fn at(line: usize, col: usize) -> Offset {
    Offset::new(14.0 + col as f64 * ADV, 10.0 + line as f64 * LINE + LINE / 2.0)
}

fn click(ui: &mut Ui, env: &mut TextEnv, pos: Offset) {
    ui.dispatch_pointer_down(pos);
    ui.rebuild_if_dirty();
    ui.layout(env, Size::new(600.0, 400.0));
}

fn drag(ui: &mut Ui, env: &mut TextEnv, from: Offset, to: Offset) {
    if let Some(target) = ui.pan_target_at(from) {
        ui.dispatch_pan_start(target, from);
        ui.dispatch_pan_update(target, to);
        ui.dispatch_pan_end(target, to);
    }
    ui.rebuild_if_dirty();
    ui.layout(env, Size::new(600.0, 400.0));
}

#[test]
fn typing_inserts_into_the_bound_signal() {
    let (mut ui, mut env, code) = harness("");
    for ch in "hello".chars() {
        key(&mut ui, &mut env, KeyInput::Insert(ch.to_string()));
    }
    assert_eq!(code.get(), "hello");
}

#[test]
fn undo_and_redo_a_typing_run() {
    let (mut ui, mut env, code) = harness("");
    for ch in "abc".chars() {
        key(&mut ui, &mut env, KeyInput::Insert(ch.to_string()));
    }
    assert_eq!(code.get(), "abc");
    // Contiguous typing coalesces into one undo step.
    key(&mut ui, &mut env, KeyInput::Undo);
    assert_eq!(code.get(), "");
    key(&mut ui, &mut env, KeyInput::Redo);
    assert_eq!(code.get(), "abc");
}

#[test]
fn backspace_then_undo_restores() {
    let (mut ui, mut env, code) = harness("");
    for ch in "hi".chars() {
        key(&mut ui, &mut env, KeyInput::Insert(ch.to_string()));
    }
    key(&mut ui, &mut env, KeyInput::Backspace);
    assert_eq!(code.get(), "h");
    // The backspace is a separate undo step from the typing.
    key(&mut ui, &mut env, KeyInput::Undo);
    assert_eq!(code.get(), "hi");
    key(&mut ui, &mut env, KeyInput::Undo);
    assert_eq!(code.get(), "");
}

#[test]
fn enter_auto_indents() {
    let (mut ui, mut env, code) = harness("");
    for ch in "fn main() {".chars() {
        key(&mut ui, &mut env, KeyInput::Insert(ch.to_string()));
    }
    key(&mut ui, &mut env, KeyInput::Enter);
    // Newline after `{` adds one indent level (4 spaces by default).
    assert_eq!(code.get(), "fn main() {\n    ");
}

#[test]
fn external_signal_change_is_picked_up() {
    let (mut ui, mut env, code) = harness("one");
    // Simulate the caller loading new content into the bound signal.
    code.set(String::from("two\nthree"));
    ui.rebuild_if_dirty();
    ui.layout(&mut env, Size::new(600.0, 400.0));
    // Editing continues on the new content without panicking.
    key(
        &mut ui,
        &mut env,
        KeyInput::Move {
            motion: Motion::DocEnd,
            extend: false,
        },
    );
    key(&mut ui, &mut env, KeyInput::Insert("!".to_string()));
    assert_eq!(code.get(), "two\nthree!");
}

#[test]
fn tab_indents_the_caret_line() {
    let (mut ui, mut env, code) = harness("ab");
    key(&mut ui, &mut env, KeyInput::Indent); // caret at 0, no selection → insert one level
    assert_eq!(code.get(), "    ab");
}

#[test]
fn tab_indents_a_multiline_selection() {
    let (mut ui, mut env, code) = harness("a\nb\nc");
    key(&mut ui, &mut env, KeyInput::SelectAll);
    key(&mut ui, &mut env, KeyInput::Indent);
    assert_eq!(code.get(), "    a\n    b\n    c");
}

#[test]
fn shift_tab_outdents_touched_lines() {
    let (mut ui, mut env, code) = harness("    a\n  b\nc");
    key(&mut ui, &mut env, KeyInput::SelectAll);
    key(&mut ui, &mut env, KeyInput::Outdent);
    // 4 spaces removed, 2 spaces removed, none to remove.
    assert_eq!(code.get(), "a\nb\nc");
}

#[test]
fn indent_then_undo_is_one_step() {
    let (mut ui, mut env, code) = harness("a\nb");
    key(&mut ui, &mut env, KeyInput::SelectAll);
    key(&mut ui, &mut env, KeyInput::Indent);
    assert_eq!(code.get(), "    a\n    b");
    key(&mut ui, &mut env, KeyInput::Undo);
    assert_eq!(code.get(), "a\nb");
}

#[test]
fn alt_click_adds_a_cursor_and_types_at_both() {
    let (mut ui, mut env, code) = mouse_harness("abc");
    click(&mut ui, &mut env, at(0, 0)); // caret at start
    // Alt-click at the end adds a second caret.
    pebbles::core::keyboard::set_modifiers(false, false, true, false);
    click(&mut ui, &mut env, at(0, 3));
    pebbles::core::keyboard::set_modifiers(false, false, false, false);
    // Typing inserts at both carets.
    key(&mut ui, &mut env, KeyInput::Insert("X".to_string()));
    assert_eq!(code.get(), "XabcX");
}

#[test]
fn escape_collapses_multiple_cursors() {
    let (mut ui, mut env, code) = mouse_harness("abc");
    click(&mut ui, &mut env, at(0, 0));
    pebbles::core::keyboard::set_modifiers(false, false, true, false);
    click(&mut ui, &mut env, at(0, 3));
    pebbles::core::keyboard::set_modifiers(false, false, false, false);
    // Escape drops back to a single caret, so typing inserts once.
    key(&mut ui, &mut env, KeyInput::Escape);
    key(&mut ui, &mut env, KeyInput::Insert("X".to_string()));
    assert_eq!(code.get(), "abcX");
}

#[test]
fn double_click_selects_the_word() {
    let (mut ui, mut env, code) = mouse_harness("foo bar baz");
    // Double-click inside "bar" selects the whole word; typing replaces it.
    ui.dispatch_double_tap(at(0, 5));
    ui.rebuild_if_dirty();
    ui.layout(&mut env, Size::new(600.0, 400.0));
    key(&mut ui, &mut env, KeyInput::Insert("XY".to_string()));
    assert_eq!(code.get(), "foo XY baz");
}

#[test]
fn multi_cursor_backspace_deletes_at_each() {
    let (mut ui, mut env, code) = mouse_harness("ab\ncd");
    // Caret after "ab", alt-click after "cd" → two carets at line ends.
    click(&mut ui, &mut env, at(0, 2));
    pebbles::core::keyboard::set_modifiers(false, false, true, false);
    click(&mut ui, &mut env, at(1, 2));
    pebbles::core::keyboard::set_modifiers(false, false, false, false);
    key(&mut ui, &mut env, KeyInput::Backspace);
    assert_eq!(code.get(), "a\nc");
}

#[test]
fn ctrl_d_selects_word_then_adds_next_occurrence() {
    let (mut ui, mut env, code) = harness("foo bar foo");
    // First Ctrl+D (no selection) selects the word under the caret ("foo" at 0).
    key(&mut ui, &mut env, KeyInput::SelectNextOccurrence);
    // Second Ctrl+D adds a cursor at the next "foo".
    key(&mut ui, &mut env, KeyInput::SelectNextOccurrence);
    // Typing replaces both selections.
    key(&mut ui, &mut env, KeyInput::Insert("X".to_string()));
    assert_eq!(code.get(), "X bar X");
}

#[test]
fn triple_click_selects_the_whole_line() {
    let (mut ui, mut env, code) = mouse_harness("aaa\nbbb\nccc");
    ui.dispatch_triple_tap(at(1, 1)); // the "bbb" line
    ui.rebuild_if_dirty();
    ui.layout(&mut env, Size::new(600.0, 400.0));
    // The whole line (incl. its newline) is selected, so typing replaces it.
    key(&mut ui, &mut env, KeyInput::Insert("X".to_string()));
    assert_eq!(code.get(), "aaa\nXccc");
}

#[test]
fn indent_guides_whitespace_and_rulers_render() {
    // Exercise the overlay build/layout path for all three toggles on an indented doc.
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    let code = create_root_signal(String::from("fn main() {\n        let x = 1;\n}\n"));
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(
        View::new(
            white(),
            code_editor(code)
                .height(200.0)
                .indent_guides(true)
                .render_whitespace(true)
                .rulers([4usize, 80])
                .autofocus(),
        )
        .into_widget(),
    );
    for _ in 0..3 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, Size::new(600.0, 400.0));
    }
    assert!(ui.element_count() > 0, "overlays built without panicking");
}

#[test]
fn long_line_scrolls_horizontally_to_the_caret() {
    use pebbles::render::RenderScroll;
    // A short doc (so the VERTICAL scroll can't move) with one very long line.
    let body = format!("short\n{}", "x".repeat(400));
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    let code = create_root_signal(body);
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(View::new(white(), code_editor(code).height(120.0).autofocus()).into_widget());
    let win = Size::new(320.0, 400.0); // narrow, so the long line overflows
    for _ in 0..3 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, win);
    }
    // Max offset across both scroll views; the vertical one can't move (content fits), so any
    // movement is horizontal.
    let max_offset = |ui: &Ui| {
        ui.render_tree()
            .find_all::<RenderScroll>()
            .into_iter()
            .filter_map(|id| ui.render_tree().object_ref(id).downcast_ref::<RenderScroll>())
            .map(|s| s.offset)
            .fold(0.0_f64, f64::max)
    };
    assert_eq!(max_offset(&ui), 0.0, "starts flush left");
    // Move to the end of the long line — the viewport scrolls right to reveal the caret.
    ui.dispatch_key(KeyInput::Move {
        motion: Motion::DocEnd,
        extend: false,
    });
    ui.rebuild_if_dirty();
    ui.layout(&mut env, win);
    assert!(max_offset(&ui) > 0.0, "caret at end of a long line scrolled horizontally");
}

#[test]
fn viewport_is_virtualized_for_large_docs() {
    // A 2000-line doc in a 200px viewport must render only a screenful of nodes, not 2000
    // lines' worth — the virtualization tripwire (render node count tracks the viewport).
    let body: String = (0..2000).map(|i| format!("fn line_{i}() {{}}")).collect::<Vec<_>>().join("\n");
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    let code = create_root_signal(body);
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(View::new(white(), code_editor(code).height(200.0).autofocus()).into_widget());
    for _ in 0..3 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, Size::new(600.0, 400.0));
    }
    let nodes = ui.render_node_count();
    assert!(nodes < 300, "expected a virtualized node count, got {nodes} for 2000 lines");
}

#[test]
fn keyboard_nav_autoscrolls_the_caret_into_view() {
    use pebbles::render::RenderScroll;
    // A short viewport over a tall document: moving to the end must scroll it into view.
    let body: String = (0..100).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    let code = create_root_signal(body);
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(
        View::new(white(), code_editor(code).height(120.0).gutter(false).autofocus()).into_widget(),
    );
    for _ in 0..3 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, Size::new(600.0, 400.0));
    }
    let offset = |ui: &Ui| {
        let id = ui.render_tree().find::<RenderScroll>().expect("a scroll view");
        ui.render_tree()
            .object_ref(id)
            .downcast_ref::<RenderScroll>()
            .unwrap()
            .offset
    };
    assert_eq!(offset(&ui), 0.0, "starts at the top");
    key(
        &mut ui,
        &mut env,
        KeyInput::Move {
            motion: Motion::DocEnd,
            extend: false,
        },
    );
    assert!(offset(&ui) > 500.0, "caret at doc end scrolled the viewport down");
    // Back to the top brings the offset home.
    key(
        &mut ui,
        &mut env,
        KeyInput::Move {
            motion: Motion::DocStart,
            extend: false,
        },
    );
    assert_eq!(offset(&ui), 0.0, "caret at doc start scrolled back to the top");
}

#[test]
fn shift_alt_drag_makes_a_column_of_carets() {
    let (mut ui, mut env, code) = mouse_harness("abc\ndef\nghi");
    // Shift+Alt drag straight down column 1 → a caret on each of the three lines.
    pebbles::core::keyboard::set_modifiers(true, false, true, false);
    drag(&mut ui, &mut env, at(0, 1), at(2, 1));
    pebbles::core::keyboard::set_modifiers(false, false, false, false);
    // Typing inserts at every column caret.
    key(&mut ui, &mut env, KeyInput::Insert("X".to_string()));
    assert_eq!(code.get(), "aXbc\ndXef\ngXhi");
}
