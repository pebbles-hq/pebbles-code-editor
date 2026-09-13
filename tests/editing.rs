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
    // Auto-close made `{}`, so Enter between them opens the block: an indented middle line
    // with the caret, and the closer dropped below.
    assert_eq!(code.get(), "fn main() {\n    \n}");
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
fn auto_close_inserts_skips_and_deletes_pairs() {
    let (mut ui, mut env, code) = harness("");
    key(&mut ui, &mut env, KeyInput::Insert("(".to_string()));
    assert_eq!(code.get(), "()", "opener inserts its pair, caret between");
    key(&mut ui, &mut env, KeyInput::Insert(")".to_string()));
    assert_eq!(code.get(), "()", "typing the closer overtypes, not doubles");
    // Caret is now after ')'. Go back between the pair and backspace → both go.
    key(&mut ui, &mut env, KeyInput::Move { motion: Motion::Left, extend: false });
    key(&mut ui, &mut env, KeyInput::Backspace);
    assert_eq!(code.get(), "", "backspace between an empty pair removes both");
}

#[test]
fn auto_close_wraps_the_selection() {
    let (mut ui, mut env, code) = harness("x");
    key(&mut ui, &mut env, KeyInput::SelectAll);
    key(&mut ui, &mut env, KeyInput::Insert("(".to_string()));
    assert_eq!(code.get(), "(x)", "typing a bracket around a selection wraps it");
}

#[test]
fn completion_popup_types_navigates_and_accepts() {
    use pebbles_code_editor::{CompletionContext, CompletionItem, CompletionKind, CompletionProvider};
    use std::rc::Rc;
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    let code = create_root_signal(String::new());
    let provider: CompletionProvider = Rc::new(|_ctx: &CompletionContext| {
        vec![
            CompletionItem::new("println", CompletionKind::Function).insert("println!()"),
            CompletionItem::new("print", CompletionKind::Function),
        ]
    });
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(
        View::new(white(), code_editor(code).completion(provider).autofocus()).into_widget(),
    );
    for _ in 0..3 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, Size::new(600.0, 400.0));
    }
    // Type "pr" — the popup opens (both items match).
    key(&mut ui, &mut env, KeyInput::Insert("p".to_string()));
    key(&mut ui, &mut env, KeyInput::Insert("r".to_string()));
    // Down selects the 2nd item ("print"); Enter accepts it, replacing the "pr" prefix.
    key(&mut ui, &mut env, KeyInput::Move { motion: Motion::Down, extend: false });
    key(&mut ui, &mut env, KeyInput::Enter);
    assert_eq!(code.get(), "print");
}

#[test]
fn completion_accepts_a_snippet_with_caret_stop() {
    use pebbles_code_editor::{CompletionContext, CompletionItem, CompletionKind, CompletionProvider};
    use std::rc::Rc;
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    let code = create_root_signal(String::new());
    let provider: CompletionProvider = Rc::new(|_ctx: &CompletionContext| {
        vec![CompletionItem::new("main", CompletionKind::Snippet).insert("fn main() {\n    $0\n}")]
    });
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(
        View::new(white(), code_editor(code).completion(provider).autofocus()).into_widget(),
    );
    for _ in 0..3 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, Size::new(600.0, 400.0));
    }
    key(&mut ui, &mut env, KeyInput::Insert("m".to_string()));
    key(&mut ui, &mut env, KeyInput::Enter); // accept the snippet
    // The $0 tabstop is stripped and the caret lands there; typing inserts at that point.
    key(&mut ui, &mut env, KeyInput::Insert("x".to_string()));
    assert_eq!(code.get(), "fn main() {\n    x\n}");
}

#[test]
fn find_bar_opens_and_captures_typing() {
    let (mut ui, mut env, code) = harness("foo bar foo baz");
    let before = ui.element_count();
    key(&mut ui, &mut env, KeyInput::Find);
    assert!(ui.element_count() > before, "find bar rendered");
    // The find field autofocuses, so keystrokes go to it — not the document.
    key(&mut ui, &mut env, KeyInput::Insert("foo".to_string()));
    key(&mut ui, &mut env, KeyInput::Enter);
    assert_eq!(code.get(), "foo bar foo baz", "typing goes to the find field, not the doc");
}

#[test]
fn replace_bar_opens() {
    let (mut ui, mut env, _code) = harness("x");
    let before = ui.element_count();
    key(&mut ui, &mut env, KeyInput::Replace);
    assert!(ui.element_count() > before, "replace bar rendered");
}

#[test]
fn go_to_definition_moves_the_caret() {
    use pebbles_code_editor::DefinitionProvider;
    use std::rc::Rc;
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    let code = create_root_signal(String::from("foo\nbar\nfoo"));
    let def: DefinitionProvider = Rc::new(|_src: &str, _byte: usize| Some(8)); // 2nd "foo"
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(View::new(white(), code_editor(code).definition(def).autofocus()).into_widget());
    for _ in 0..3 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, Size::new(600.0, 400.0));
    }
    key(&mut ui, &mut env, KeyInput::GoToDefinition);
    key(&mut ui, &mut env, KeyInput::Insert("X".to_string()));
    assert_eq!(code.get(), "foo\nbar\nXfoo");
}

#[test]
fn format_replaces_the_document() {
    use pebbles_code_editor::FormatProvider;
    use std::rc::Rc;
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    let code = create_root_signal(String::from("messy   code"));
    let fmt: FormatProvider = Rc::new(|_src: &str| String::from("clean code\n"));
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(View::new(white(), code_editor(code).format(fmt).autofocus()).into_widget());
    for _ in 0..3 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, Size::new(600.0, 400.0));
    }
    key(&mut ui, &mut env, KeyInput::Format);
    assert_eq!(code.get(), "clean code\n");
    // ...and it's one undo step back to the original.
    key(&mut ui, &mut env, KeyInput::Undo);
    assert_eq!(code.get(), "messy   code");
}

#[test]
fn diagnostics_inlay_hover_signature_render() {
    use pebbles_code_editor::{
        Diagnostic, Hover, HoverProvider, InlayHint, Severity, SignatureHelp, SignatureProvider,
    };
    use std::rc::Rc;
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    let code = create_root_signal(String::from("let x = 1;\nlet y = 2;"));
    let diags = create_root_signal(vec![Diagnostic {
        range: (4, 5),
        severity: Severity::Error,
        message: "bad".into(),
    }]);
    let hints = create_root_signal(vec![InlayHint { at: 5, label: ": i32".into() }]);
    let hover: HoverProvider = Rc::new(|_s: &str, _b: usize| {
        Some(Hover { contents: "an int".into(), range: None })
    });
    let sig: SignatureProvider = Rc::new(|_s: &str, _b: usize| {
        Some(SignatureHelp { label: "fn f(x: i32)".into(), params: vec![], active: None })
    });
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(
        View::new(
            white(),
            code_editor(code)
                .diagnostics(diags)
                .inlay_hints(hints)
                .hover(hover)
                .signature_help(sig)
                .height(200.0)
                .autofocus(),
        )
        .into_widget(),
    );
    for _ in 0..3 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, Size::new(600.0, 400.0));
    }
    // Hover over the content, and type "(" to trigger signature help — both render, no panic.
    ui.dispatch_hover(Offset::new(30.0, 15.0));
    ui.rebuild_if_dirty();
    ui.layout(&mut env, Size::new(600.0, 400.0));
    key(&mut ui, &mut env, KeyInput::Insert("(".to_string()));
    assert!(ui.element_count() > 0, "provider overlays built without panicking");
}

#[test]
fn toggle_comment_comments_and_uncomments() {
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    let code = create_root_signal(String::from("let x = 1;"));
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(
        View::new(
            white(),
            code_editor(code)
                .language(Box::new(pebbles_code_editor::lang::Rust))
                .autofocus(),
        )
        .into_widget(),
    );
    for _ in 0..3 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, Size::new(600.0, 400.0));
    }
    key(&mut ui, &mut env, KeyInput::ToggleComment);
    assert_eq!(code.get(), "// let x = 1;");
    key(&mut ui, &mut env, KeyInput::ToggleComment);
    assert_eq!(code.get(), "let x = 1;");
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
fn sticky_scroll_pins_and_navigates_to_scopes() {
    use pebbles::render::RenderScroll;
    let mut body = String::from("fn outer() {\n    if a {\n");
    for i in 0..80 {
        body.push_str(&format!("        stmt_{i}();\n"));
    }
    body.push_str("    }\n}\n");
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    let code = create_root_signal(body);
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    let win = Size::new(600.0, 400.0);
    ui.mount_root(
        View::new(white(), code_editor(code).height(200.0).sticky_scroll(true).autofocus())
            .into_widget(),
    );
    for _ in 0..3 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, win);
    }
    let v_offset = |ui: &Ui| {
        let id = ui.render_tree().find::<RenderScroll>().unwrap();
        ui.render_tree().object_ref(id).downcast_ref::<RenderScroll>().unwrap().offset
    };
    // Scroll deep into the nested block.
    ui.dispatch_key(KeyInput::Move { motion: Motion::DocEnd, extend: false });
    for _ in 0..3 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, win);
    }
    let deep = v_offset(&ui);
    assert!(deep > 300.0, "scrolled deep into the block");
    // The enclosing scopes are pinned at the top as a pointer barrier: a click on the pinned
    // header area is absorbed, so it does NOT move the caret in the content hidden behind it
    // (which would jerk the viewport). Without sticky, the click would land in the content.
    ui.dispatch_pointer_down(Offset::new(40.0, 8.0));
    for _ in 0..2 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, win);
    }
    assert!(
        (v_offset(&ui) - deep).abs() < 2.0,
        "click on the pinned scope header was absorbed (content undisturbed)"
    );
}

#[test]
fn minimap_click_scrolls_the_document() {
    use pebbles::render::RenderScroll;
    let body: String = (0..300).map(|i| format!("line {i} of the doc")).collect::<Vec<_>>().join("\n");
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    let code = create_root_signal(body);
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    let win = Size::new(600.0, 400.0);
    ui.mount_root(
        View::new(white(), code_editor(code).height(200.0).minimap(true).autofocus()).into_widget(),
    );
    for _ in 0..3 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, win);
    }
    let v_offset = |ui: &Ui| {
        let id = ui.render_tree().find::<RenderScroll>().expect("vertical scroll");
        ui.render_tree()
            .object_ref(id)
            .downcast_ref::<RenderScroll>()
            .unwrap()
            .offset
    };
    assert_eq!(v_offset(&ui), 0.0, "starts at the top");
    // Click near the bottom of the minimap (right edge) → jumps the viewport down.
    ui.dispatch_pointer_down(Offset::new(558.0, 175.0));
    ui.rebuild_if_dirty();
    ui.layout(&mut env, win);
    assert!(v_offset(&ui) > 0.0, "minimap click scrolled the document down");
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

// ---------------------------------------------------------------------------
// Extensions & plugins (§7)
// ---------------------------------------------------------------------------

#[test]
fn extension_decorations_render() {
    use pebbles_code_editor::{Decoration, extension};
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    let code = create_root_signal(String::from("hello world"));
    // Decorate the first word on every render.
    let ext = extension("highlight-first-word")
        .decorations(|_snap| vec![Decoration::background((0, 5), Color::from_rgba8(0, 128, 255, 60))]);
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    let before = {
        ui.mount_root(View::new(white(), code_editor(code).autofocus()).into_widget());
        for _ in 0..3 {
            ui.rebuild_if_dirty();
            ui.layout(&mut env, Size::new(600.0, 400.0));
        }
        ui.element_count()
    };
    // Remount with the extension: the decoration adds at least one overlay layer.
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(View::new(white(), code_editor(code).extension(ext).autofocus()).into_widget());
    for _ in 0..3 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, Size::new(600.0, 400.0));
    }
    assert!(ui.element_count() > before, "decoration layer rendered");
}

#[test]
fn command_palette_runs_a_command() {
    use pebbles_code_editor::{Command, extension};
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    let code = create_root_signal(String::from("abc"));
    // A command that appends "!" at the caret.
    let cmd = Command::new("test.bang", "Insert Bang", |ctx| ctx.insert("!"));
    let ext = extension("commands").command(cmd);
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(View::new(white(), code_editor(code).extension(ext).autofocus()).into_widget());
    for _ in 0..3 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, Size::new(600.0, 400.0));
    }
    // Move the caret to the end so the insert lands after "abc".
    key(&mut ui, &mut env, KeyInput::Move { motion: Motion::DocEnd, extend: false });
    // Open the palette, filter to the command, and run it with Enter.
    key(&mut ui, &mut env, KeyInput::CommandPalette);
    key(&mut ui, &mut env, KeyInput::Insert("bang".to_string()));
    key(&mut ui, &mut env, KeyInput::Enter);
    assert_eq!(code.get(), "abc!", "the palette ran the command against the editor");
}

#[test]
fn read_only_range_vetoes_an_edit() {
    use pebbles_code_editor::extension;
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    let code = create_root_signal(String::from("locked text"));
    // Mark the first 6 bytes read-only.
    let ext = extension("guard").read_only_ranges(|_snap| vec![(0, 6)]);
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(View::new(white(), code_editor(code).extension(ext).autofocus()).into_widget());
    for _ in 0..3 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, Size::new(600.0, 400.0));
    }
    // Caret is at 0 (inside the read-only range): typing is vetoed.
    key(&mut ui, &mut env, KeyInput::Insert("X".to_string()));
    assert_eq!(code.get(), "locked text", "edit inside a read-only range is vetoed");
    // Move past the guarded range: typing works again.
    key(&mut ui, &mut env, KeyInput::Move { motion: Motion::DocEnd, extend: false });
    key(&mut ui, &mut env, KeyInput::Insert("!".to_string()));
    assert_eq!(code.get(), "locked text!", "edit outside the read-only range applies");
}

#[test]
fn extension_on_change_hook_fires() {
    use pebbles_code_editor::extension;
    use std::cell::RefCell;
    use std::rc::Rc;
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    let code = create_root_signal(String::from(""));
    let seen: Rc<RefCell<usize>> = Rc::new(RefCell::new(0));
    let seen_c = seen.clone();
    let ext = extension("counter").on_change(move |_snap| *seen_c.borrow_mut() += 1);
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(View::new(white(), code_editor(code).extension(ext).autofocus()).into_widget());
    for _ in 0..3 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, Size::new(600.0, 400.0));
    }
    let base = *seen.borrow();
    key(&mut ui, &mut env, KeyInput::Insert("a".to_string()));
    assert!(*seen.borrow() > base, "on_change fired after an edit");
}

#[test]
fn extension_config_facet_overrides_tab_size() {
    use pebbles_code_editor::{ConfigPatch, extension};
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    let code = create_root_signal(String::new());
    // A plugin that prefers a 2-space indent (default is 4).
    let ext = extension("two-space").config(ConfigPatch {
        tab_size: Some(2),
        insert_spaces: Some(true),
        ..Default::default()
    });
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(View::new(white(), code_editor(code).extension(ext).autofocus()).into_widget());
    for _ in 0..3 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, Size::new(600.0, 400.0));
    }
    key(&mut ui, &mut env, KeyInput::Indent);
    assert_eq!(code.get(), "  ", "the config facet set a 2-space indent");
}

#[test]
fn extension_focus_hook_fires() {
    use pebbles_code_editor::extension;
    use std::cell::RefCell;
    use std::rc::Rc;
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    let code = create_root_signal(String::from("x"));
    let last: Rc<RefCell<Option<bool>>> = Rc::new(RefCell::new(None));
    let last_c = last.clone();
    let ext = extension("focus-watch").on_focus(move |has| *last_c.borrow_mut() = Some(has));
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(View::new(white(), code_editor(code).extension(ext).autofocus()).into_widget());
    for _ in 0..3 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, Size::new(600.0, 400.0));
    }
    assert_eq!(*last.borrow(), Some(true), "on_focus fired with focus gained");
}

#[test]
fn extension_pointer_handler_reports_byte() {
    use pebbles_code_editor::extension;
    use std::cell::RefCell;
    use std::rc::Rc;
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    pebbles::core::keyboard::set_modifiers(false, false, false, false);
    let code = create_root_signal(String::from("abcdef"));
    let hit: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));
    let hit_c = hit.clone();
    let ext = extension("click-watch").on_click(move |_snap, byte| *hit_c.borrow_mut() = Some(byte));
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(
        View::new(white(), code_editor(code).gutter(false).extension(ext).autofocus()).into_widget(),
    );
    for _ in 0..3 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, Size::new(600.0, 400.0));
    }
    // Click at column 3 on line 0.
    click(&mut ui, &mut env, at(0, 3));
    assert_eq!(*hit.borrow(), Some(3), "on_click reported the byte under the pointer");
}

#[test]
fn extension_keybinding_runs_a_command() {
    use pebbles::core::shortcuts::{Mods, ShortcutKey};
    use pebbles_code_editor::{Command, extension};
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    let code = create_root_signal(String::from("abc"));
    // A command bound to Ctrl+B that appends "!" at the caret.
    let ext = extension("bang")
        .command(Command::new("edit.bang", "Insert Bang", |ctx| ctx.insert("!")))
        .keybinding("Ctrl+B", "edit.bang");
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(View::new(white(), code_editor(code).extension(ext).autofocus()).into_widget());
    for _ in 0..3 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, Size::new(600.0, 400.0));
    }
    // Caret to the end, then fire the bound chord through the shortcut registry.
    key(&mut ui, &mut env, KeyInput::Move { motion: Motion::DocEnd, extend: false });
    let mods = Mods { shift: false, ctrl: true, alt: false, meta: false };
    let consumed = pebbles::core::shortcuts::dispatch(ui.window_id(), mods, ShortcutKey::Char('b'));
    ui.rebuild_if_dirty();
    ui.layout(&mut env, Size::new(600.0, 400.0));
    assert!(consumed, "the editor consumed the bound chord");
    assert_eq!(code.get(), "abc!", "the keybinding ran its command");
}

// ---------------------------------------------------------------------------
// Theming & styling (§8)
// ---------------------------------------------------------------------------

#[test]
fn theme_extension_composes_and_renders() {
    use pebbles_code_editor::{EditorTheme, HighlightStyle, extension};
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    let code = create_root_signal(String::from("fn main() {}\n// note"));
    // A theme plugin: swap the syntax palette to light and make comments underlined.
    let ext = extension("theme-plugin").theme(|mut t: EditorTheme| {
        t.syntax = HighlightStyle::light();
        t.syntax.comment = t.syntax.comment.underline();
        t
    });
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(
        View::new(
            white(),
            code_editor(code)
                .language(Box::new(pebbles_code_editor::lang::Rust))
                .extension(ext)
                .autofocus(),
        )
        .into_widget(),
    );
    for _ in 0..3 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, Size::new(600.0, 400.0));
    }
    assert!(ui.element_count() > 0, "theme-extension editor rendered without panicking");
}

#[test]
fn font_and_spacing_config_render() {
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    let code = create_root_signal(String::from("let x = 1;"));
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(
        View::new(
            white(),
            code_editor(code)
                .font_family("Fira Code")
                .line_height(2.0)
                .letter_spacing(1.5)
                .autofocus(),
        )
        .into_widget(),
    );
    for _ in 0..3 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, Size::new(600.0, 400.0));
    }
    assert!(ui.element_count() > 0, "font/line-height/letter-spacing editor rendered");
}

#[test]
fn letter_spacing_widens_the_hit_grid() {
    use pebbles_code_editor::extension;
    use std::cell::RefCell;
    use std::rc::Rc;
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    pebbles::core::keyboard::set_modifiers(false, false, false, false);
    let code = create_root_signal(String::from("abcdef"));
    let hit: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));
    let hit_c = hit.clone();
    let ext = extension("click").on_click(move |_s, b| *hit_c.borrow_mut() = Some(b));
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    // letter_spacing widens each cell: advance = 13.5*0.6 + 2.0 = 10.1 (vs 8.1 without).
    ui.mount_root(
        View::new(
            white(),
            code_editor(code).gutter(false).letter_spacing(2.0).extension(ext).autofocus(),
        )
        .into_widget(),
    );
    for _ in 0..3 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, Size::new(600.0, 400.0));
    }
    // Click at x for column 3 on the WIDENED grid; a non-spaced grid would land elsewhere.
    let adv = 13.5 * 0.6 + 2.0;
    let pos = Offset::new(14.0 + 3.0 * adv, 10.0 + 13.5 * 1.6 / 2.0);
    click(&mut ui, &mut env, pos);
    assert_eq!(*hit.borrow(), Some(3), "hit-testing used the letter-spaced advance");
}
