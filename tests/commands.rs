//! End-to-end tests for the editor command list: real raw key events (as the shell delivers
//! them, before the editing-intent mapping) resolve through the keymap to commands; plus the
//! palette, the host controller and remapping.

use std::collections::BTreeSet;

use pebbles::core::{KeyEventResult, Ui, create_root_signal};
use pebbles::prelude::*;
use pebbles::render::TextEnv;
use pebbles_code_editor::{
    EditorCommand, EditorController, Keymap, code_editor, editor_controller,
};

struct H {
    ui: Ui,
    env: TextEnv,
    code: Signal<String>,
    ctrl: EditorController,
    folds: Signal<BTreeSet<usize>>,
}

fn harness_with(initial: &str, keymap: Keymap) -> H {
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    pebbles::core::keyboard::set_modifiers(false, false, false, false);
    let code = create_root_signal(String::from(initial));
    let folds = create_root_signal(BTreeSet::new());
    let ctrl = editor_controller();
    let mut ui = Ui::new();
    let env = TextEnv::new();
    ui.mount_root(
        View::new(
            Color::from_rgba8(255, 255, 255, 255),
            code_editor(code)
                .language(pebbles_code_editor::lang::by_name("rust"))
                .height(400.0)
                .folds(folds)
                .keymap(keymap)
                .controller(ctrl)
                .autofocus(),
        )
        .into_widget(),
    );
    let mut h = H {
        ui,
        env,
        code,
        ctrl,
        folds,
    };
    for _ in 0..3 {
        h.frame();
    }
    h
}

fn harness(initial: &str) -> H {
    harness_with(initial, Keymap::new())
}

impl H {
    fn frame(&mut self) {
        self.ui.rebuild_if_dirty();
        self.ui.layout(&mut self.env, Size::new(600.0, 400.0));
    }
    /// A chord through the shell's order: raw handler first, then the editing intent.
    fn press(&mut self, chord: &str) -> bool {
        let ev = pebbles::core::shortcuts::chord_event(chord).expect("chord");
        let handled = self.ui.dispatch_key_event(&ev) == KeyEventResult::Handled;
        self.frame();
        handled
    }
    fn key(&mut self, k: KeyInput) {
        self.ui.dispatch_key(k);
        self.frame();
    }
    fn typ(&mut self, s: &str) {
        for ch in s.chars() {
            self.key(KeyInput::Insert(ch.to_string()));
        }
    }
    fn caret(&mut self, line: usize, col: usize) {
        self.ctrl.set_cursor(line, col);
        self.frame();
    }
    fn pos(&self) -> (usize, usize) {
        self.ctrl.cursor().peek()
    }
}

#[test]
fn alt_up_down_move_lines_instead_of_the_caret() {
    let mut h = harness("a\nb\nc");
    h.caret(1, 0);
    assert!(h.press("Alt+Up"));
    assert_eq!(h.code.get(), "b\na\nc");
    assert_eq!(h.pos(), (0, 0));
    assert!(h.press("Alt+Down"));
    assert!(h.press("Alt+Down"));
    assert_eq!(h.code.get(), "a\nc\nb");
    // One undo step per move.
    h.key(KeyInput::Undo);
    assert_eq!(h.code.get(), "a\nb\nc");
}

#[test]
fn copy_delete_join_and_insert_line_chords() {
    let mut h = harness("one\ntwo");
    h.caret(0, 1);
    h.press("Shift+Alt+Down");
    assert_eq!(h.code.get(), "one\none\ntwo");
    assert_eq!(h.pos().0, 1);
    h.press("Ctrl+Shift+K");
    assert_eq!(h.code.get(), "one\ntwo");
    h.caret(0, 0);
    h.press("Ctrl+J");
    assert_eq!(h.code.get(), "one two");
    h.press("Ctrl+Enter");
    h.typ("x");
    assert_eq!(h.code.get(), "one two\nx");
    h.press("Ctrl+Shift+Enter");
    h.typ("y");
    assert_eq!(h.code.get(), "one two\ny\nx");
}

#[test]
fn add_cursor_below_then_type_edits_both_lines() {
    let mut h = harness("ab\ncd");
    h.caret(0, 1);
    assert!(h.press("Ctrl+Alt+Down"));
    h.typ("X");
    assert_eq!(h.code.get(), "aXb\ncXd");
}

#[test]
fn select_all_occurrences_and_cursors_at_line_ends() {
    let mut h = harness("foo bar foo");
    h.caret(0, 1);
    h.press("Ctrl+Shift+L");
    h.typ("z");
    assert_eq!(h.code.get(), "z bar z");
    let mut h = harness("a\nbb\nc");
    h.key(KeyInput::SelectAll);
    h.press("Shift+Alt+I");
    h.typ(";");
    assert_eq!(h.code.get(), "a;\nbb;\nc;");
}

#[test]
fn column_select_by_keyboard() {
    let mut h = harness("abcd\nefgh\nijkl");
    h.caret(0, 1);
    h.press("Ctrl+Shift+Alt+Down");
    h.press("Ctrl+Shift+Alt+Down");
    h.press("Ctrl+Shift+Alt+Right");
    h.typ("_");
    assert_eq!(h.code.get(), "a_cd\ne_gh\ni_kl");
}

#[test]
fn multi_stroke_chord_folds_all_and_unfolds() {
    let mut h = harness("fn a() {\n    x\n}\nfn b() {\n    y\n}\n");
    assert!(h.press("Ctrl+K"));
    assert!(h.press("Ctrl+0"));
    assert_eq!(h.folds.get(), BTreeSet::from([0, 3]));
    h.press("Ctrl+K");
    h.press("Ctrl+J");
    assert!(h.folds.get().is_empty());
    // Fold / unfold at the cursor.
    h.caret(1, 0);
    h.press("Ctrl+Shift+[");
    assert_eq!(h.folds.get(), BTreeSet::from([0]));
    h.press("Ctrl+Shift+]");
    assert!(h.folds.get().is_empty());
    // An unbound second stroke cancels the chord without typing.
    h.press("Ctrl+K");
    h.press("q");
    assert_eq!(h.code.get(), "fn a() {\n    x\n}\nfn b() {\n    y\n}\n");
}

#[test]
fn fold_level_and_region_markers() {
    let src = "// #region top\nfn a() {\n    if x {\n        y\n    }\n}\n// #endregion\n";
    let mut h = harness(src);
    h.caret(6, 0);
    h.press("Ctrl+K");
    h.press("Ctrl+2");
    assert_eq!(h.folds.get(), BTreeSet::from([1]));
    h.press("Ctrl+K");
    h.press("Ctrl+8");
    assert!(h.folds.get().contains(&0));
}

#[test]
fn toggle_block_comment_and_line_comment_chords() {
    let mut h = harness("let x = 1;");
    h.caret(0, 0);
    h.press("Shift+Alt+A");
    assert_eq!(h.code.get(), "/* let x = 1; */");
    h.press("Shift+Alt+A");
    assert_eq!(h.code.get(), "let x = 1;");
    h.press("Ctrl+K");
    h.press("Ctrl+C");
    assert_eq!(h.code.get(), "// let x = 1;");
    h.press("Ctrl+K");
    h.press("Ctrl+U");
    assert_eq!(h.code.get(), "let x = 1;");
}

#[test]
fn jump_to_bracket_and_subword_motion() {
    let mut h = harness("f(a, b)");
    h.caret(0, 1);
    h.press("Ctrl+Shift+\\");
    assert_eq!(h.pos(), (0, 7));
    let mut h = harness("fooBarBaz");
    h.caret(0, 0);
    h.press("Ctrl+Alt+Right");
    assert_eq!(h.pos(), (0, 3));
    h.press("Ctrl+Alt+Right");
    assert_eq!(h.pos(), (0, 6));
    h.press("Ctrl+Alt+Backspace");
    assert_eq!(h.code.get(), "fooBaz");
}

#[test]
fn smart_home_toggles_between_indent_and_column_zero() {
    let mut h = harness("    let x");
    h.caret(0, 7);
    h.key(KeyInput::Move {
        motion: Motion::LineStart,
        extend: false,
    });
    assert_eq!(h.pos(), (0, 4));
    h.key(KeyInput::Move {
        motion: Motion::LineStart,
        extend: false,
    });
    assert_eq!(h.pos(), (0, 0));
}

#[test]
fn cursor_undo_restores_previous_selection() {
    let mut h = harness("abc\ndef");
    h.caret(0, 1);
    h.caret(1, 2);
    assert!(h.press("Ctrl+U"));
    assert_eq!(h.pos(), (0, 1));
}

#[test]
fn auto_close_and_surround_for_every_cursor() {
    let mut h = harness("a\nb");
    h.caret(0, 1);
    h.press("Ctrl+Alt+Down");
    h.typ("(");
    assert_eq!(h.code.get(), "a()\nb()");
    h.typ(")");
    assert_eq!(h.code.get(), "a()\nb()"); // stepped over
    // Surround a selection.
    let mut h = harness("word");
    h.key(KeyInput::SelectAll);
    h.typ("[");
    assert_eq!(h.code.get(), "[word]");
    // No auto-close before a word character.
    let mut h = harness("x");
    h.caret(0, 0);
    h.typ("(");
    assert_eq!(h.code.get(), "(x");
}

#[test]
fn paste_reindents_multiline_text() {
    let mut h = harness("fn a() {\n    \n}");
    h.caret(1, 4);
    pebbles::core::clipboard::write("if x {\n    y();\n}");
    h.key(KeyInput::Paste);
    assert_eq!(h.code.get(), "fn a() {\n    if x {\n        y();\n    }\n}");
}

#[test]
fn go_to_line_via_ctrl_g_palette() {
    let mut h = harness("a\nb\nc\nd");
    assert!(h.press("Ctrl+G"));
    // The palette field is focused with ":" — type the line and submit.
    h.typ("3");
    h.key(KeyInput::Enter);
    assert_eq!(h.pos(), (2, 0));
}

#[test]
fn keymap_remaps_a_command() {
    let km = Keymap::new()
        .unbind_command(EditorCommand::MoveLinesUp)
        .bind_command("Ctrl+Alt+K", EditorCommand::MoveLinesUp);
    let mut h = harness_with("a\nb", km);
    h.caret(1, 0);
    assert!(!h.press("Alt+Up"), "default chord removed");
    assert_eq!(h.code.get(), "a\nb");
    assert!(h.press("Ctrl+Alt+K"));
    assert_eq!(h.code.get(), "b\na");
}

#[test]
fn controller_runs_save_actions() {
    let mut h = harness("a  \nb");
    h.ctrl.run_command(EditorCommand::TrimTrailingWhitespace);
    h.ctrl.run_command(EditorCommand::InsertFinalNewline);
    h.frame();
    h.frame();
    assert_eq!(h.code.get(), "a\nb\n");
}

#[test]
fn every_command_runs_without_panicking() {
    // Smoke: every command on a small multi-cursor document, then undo everything.
    for &cmd in EditorCommand::ALL {
        let mut h =
            harness("fn a() {\n    let fooBar = (1, [2]);\n}\n// #region r\nx\n// #endregion\n");
        h.caret(1, 10);
        h.ctrl.run_command(cmd);
        h.frame();
        h.frame();
        for _ in 0..3 {
            h.key(KeyInput::Undo);
        }
    }
}

#[test]
fn word_wrap_toggle_and_zoom_do_not_disturb_text() {
    let mut h = harness("x".repeat(300).as_str());
    assert!(h.press("Alt+Z"));
    assert!(h.press("Ctrl+="));
    assert!(h.press("Ctrl+-"));
    assert!(h.press("Ctrl+0"));
    assert_eq!(h.code.get().len(), 300);
}

#[test]
fn syntax_folding_uses_the_parse_tree() {
    // A multi-line call whose argument lines are NOT indented deeper: indentation folding
    // finds nothing here, the tree-sitter parse does.
    let mut h = harness("fn main() {\nfoo(\n1,\n2);\n}\n");
    h.caret(1, 0);
    h.press("Ctrl+Shift+[");
    assert!(h.folds.get().contains(&1), "{:?}", h.folds.get());
}
