//! Production hardening: the editor must never panic on malformed or extreme input — an empty
//! buffer, a huge single line, multi-byte / emoji / combining text, only-newlines, wild
//! external edits, out-of-bounds pointers, and folding + soft wrap over all of it. Each test
//! drives a real sequence; completing without a panic (and with a sane buffer) is the pass.

use pebbles::core::{Ui, create_root_signal};
use pebbles::prelude::*;
use pebbles::render::TextEnv;
use pebbles_code_editor::{EditorTheme, code_editor, lang};

fn white() -> Color {
    Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xFF)
}

const SIZE: Size = Size {
    width: 500.0,
    height: 300.0,
};

/// Mount a fully-featured editor (language + height + folding-capable) on `initial`.
fn editor(initial: &str) -> (Ui, TextEnv, Signal<String>) {
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    pebbles::core::keyboard::set_modifiers(false, false, false, false);
    let code = create_root_signal(String::from(initial));
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(
        View::new(
            white(),
            code_editor(code)
                .language(Box::new(lang::Rust))
                .theme(EditorTheme::dark())
                .height(300.0)
                .soft_wrap(true)
                .autofocus(),
        )
        .into_widget(),
    );
    for _ in 0..4 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, SIZE);
    }
    (ui, env, code)
}

fn key(ui: &mut Ui, env: &mut TextEnv, k: KeyInput) {
    ui.dispatch_key(k);
    ui.rebuild_if_dirty();
    ui.layout(env, SIZE);
}

fn tap(ui: &mut Ui, env: &mut TextEnv, x: f64, y: f64) {
    ui.dispatch_pointer_down(Offset::new(x, y));
    ui.dispatch_tap(Offset::new(x, y));
    ui.rebuild_if_dirty();
    ui.layout(env, SIZE);
}

/// A grab-bag of edits/motions/pointers applied to whatever's mounted.
fn hammer(ui: &mut Ui, env: &mut TextEnv) {
    key(ui, env, KeyInput::SelectAll);
    key(
        ui,
        env,
        KeyInput::Move {
            motion: Motion::DocEnd,
            extend: false,
        },
    );
    key(
        ui,
        env,
        KeyInput::Move {
            motion: Motion::DocStart,
            extend: true,
        },
    );
    for m in [
        Motion::Right,
        Motion::Down,
        Motion::WordRight,
        Motion::LineEnd,
        Motion::Up,
    ] {
        key(
            ui,
            env,
            KeyInput::Move {
                motion: m,
                extend: false,
            },
        );
    }
    key(ui, env, KeyInput::Insert("x".to_string()));
    key(ui, env, KeyInput::Backspace);
    key(ui, env, KeyInput::Delete);
    key(ui, env, KeyInput::Enter);
    key(ui, env, KeyInput::Indent);
    // Pointers well outside the content, and inside.
    tap(ui, env, 100_000.0, 100_000.0);
    tap(ui, env, -50.0, -50.0);
    tap(ui, env, 60.0, 40.0);
    ui.dispatch_hover(Offset::new(80.0, 80.0));
    ui.dispatch_scroll(Offset::new(80.0, 80.0), 500.0);
    ui.rebuild_if_dirty();
    ui.layout(env, SIZE);
}

#[test]
fn empty_document_survives_everything() {
    let (mut ui, mut env, _c) = editor("");
    hammer(&mut ui, &mut env);
    key(&mut ui, &mut env, KeyInput::Undo);
    key(&mut ui, &mut env, KeyInput::Redo);
}

#[test]
fn huge_single_line_survives() {
    let (mut ui, mut env, _c) = editor(&"x".repeat(20_000));
    hammer(&mut ui, &mut env);
}

#[test]
fn multibyte_and_emoji_survive() {
    // Multi-byte scripts, an emoji (multi-codepoint), and a combining sequence.
    let (mut ui, mut env, _c) = editor("café 日本語 🎉👨‍👩‍👧 e\u{0301}\nмир שלום مرحبا");
    hammer(&mut ui, &mut env);
    // Backspacing across multi-byte boundaries must not split a char.
    key(
        &mut ui,
        &mut env,
        KeyInput::Move {
            motion: Motion::DocEnd,
            extend: false,
        },
    );
    for _ in 0..30 {
        key(&mut ui, &mut env, KeyInput::Backspace);
    }
}

#[test]
fn only_newlines_survive() {
    let (mut ui, mut env, _c) = editor("\n\n\n\n\n");
    hammer(&mut ui, &mut env);
}

#[test]
fn wild_external_edits_survive() {
    let (mut ui, mut env, code) = editor("start");
    for content in ["", &"y".repeat(5000), "a\n\n\nb", "日本語\n🎉", "x"] {
        code.set(content.to_string());
        ui.rebuild_if_dirty();
        ui.layout(&mut env, SIZE);
        key(
            &mut ui,
            &mut env,
            KeyInput::Move {
                motion: Motion::DocEnd,
                extend: false,
            },
        );
        key(&mut ui, &mut env, KeyInput::Insert("!".to_string()));
    }
    assert!(code.get().ends_with('!'));
}

#[test]
fn ime_preedit_with_multibyte_survives() {
    let (mut ui, mut env, _c) = editor("fn main() {}\n");
    key(&mut ui, &mut env, KeyInput::Preedit("にほ".to_string()));
    key(&mut ui, &mut env, KeyInput::Preedit("日本".to_string()));
    key(&mut ui, &mut env, KeyInput::Insert("日本".to_string())); // commit
    hammer(&mut ui, &mut env);
}

#[test]
fn wrapped_and_folded_extremes_survive() {
    // A deeply-indented, foldable, very-long-lined document with soft wrap on.
    let mut s = String::new();
    for i in 0..200 {
        s.push_str(&"    ".repeat(i % 8));
        s.push_str(&"token ".repeat(40));
        s.push('\n');
    }
    let (mut ui, mut env, _c) = editor(&s);
    hammer(&mut ui, &mut env);
    // Scroll through it.
    for _ in 0..5 {
        ui.dispatch_scroll(Offset::new(100.0, 100.0), 400.0);
        ui.rebuild_if_dirty();
        ui.layout(&mut env, SIZE);
    }
}
