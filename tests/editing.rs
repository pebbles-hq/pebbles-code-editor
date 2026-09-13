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
