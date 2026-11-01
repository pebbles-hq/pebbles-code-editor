//! End-to-end: a VS Code-style `language-configuration.json` (as a typed
//! [`LanguageConfiguration`]) drives typing, Enter, comments and snippet insertion in a
//! plain-text editor (a language the editor has no grammar for).

use pebbles::core::{Ui, create_root_signal};
use pebbles::prelude::*;
use pebbles::render::TextEnv;
use pebbles_code_editor::{
    AutoClosingPair, EditorCommand, EditorController, IndentAction, IndentationRules,
    LanguageConfiguration, NotIn, OnEnterRule, code_editor, editor_controller,
};

fn zig() -> LanguageConfiguration {
    LanguageConfiguration {
        line_comment: Some("//".into()),
        brackets: vec![
            ("{".into(), "}".into()),
            ("[".into(), "]".into()),
            ("(".into(), ")".into()),
        ],
        auto_closing_pairs: vec![
            AutoClosingPair::new("{", "}"),
            AutoClosingPair::new("(", ")"),
            AutoClosingPair::new("\"", "\"").not_in(vec![NotIn::String, NotIn::Comment]),
        ],
        surrounding_pairs: vec![("(".into(), ")".into()), ("|".into(), "|".into())],
        indentation_rules: Some(IndentationRules {
            increase_indent: Some(r"^.*\{[^}]*$".into()),
            decrease_indent: Some(r"^\s*\}".into()),
            ..Default::default()
        }),
        on_enter_rules: vec![OnEnterRule {
            before_text: r"^\s*///.*$".into(),
            indent: IndentAction::None,
            append_text: Some("/// ".into()),
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn harness(initial: &str) -> (Ui, TextEnv, Signal<String>, EditorController) {
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    let code = create_root_signal(String::from(initial));
    let ctrl = editor_controller();
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(
        View::new(
            Color::from_rgba8(255, 255, 255, 255),
            code_editor(code)
                .tab_size(4)
                .language_configuration(zig())
                .controller(ctrl)
                .autofocus(),
        )
        .into_widget(),
    );
    settle(&mut ui, &mut env);
    (ui, env, code, ctrl)
}

fn settle(ui: &mut Ui, env: &mut TextEnv) {
    for _ in 0..3 {
        ui.rebuild_if_dirty();
        ui.layout(env, Size::new(600.0, 400.0));
    }
}

fn key(ui: &mut Ui, env: &mut TextEnv, k: KeyInput) {
    ui.dispatch_key(k);
    settle(ui, env);
}

fn typ(ui: &mut Ui, env: &mut TextEnv, s: &str) {
    for c in s.chars() {
        let k = if c == '\n' {
            KeyInput::Enter
        } else {
            KeyInput::Insert(c.to_string())
        };
        key(ui, env, k);
    }
}

#[test]
fn block_enter_and_closing_brace_follow_the_configuration() {
    let (mut ui, mut env, code, _) = harness("");
    typ(&mut ui, &mut env, "fn main() {");
    assert_eq!(code.peek(), "fn main() {}");
    typ(&mut ui, &mut env, "\nx;\n");
    assert_eq!(code.peek(), "fn main() {\n    x;\n    \n}");
}

#[test]
fn decrease_pattern_outdents_and_on_enter_appends_doc_comment() {
    let (mut ui, mut env, code, ctrl) = harness("const a = {\n    b,\n    ");
    ctrl.set_cursor(2, 4);
    settle(&mut ui, &mut env);
    typ(&mut ui, &mut env, "}");
    assert_eq!(code.peek(), "const a = {\n    b,\n}");

    let (mut ui, mut env, code, ctrl) = harness("/// Docs");
    ctrl.set_cursor(0, 8);
    settle(&mut ui, &mut env);
    typ(&mut ui, &mut env, "\n");
    assert_eq!(code.peek(), "/// Docs\n/// ");
}

#[test]
fn quotes_do_not_close_in_comments_and_selections_surround() {
    let (mut ui, mut env, code, ctrl) = harness("// a ");
    ctrl.set_cursor(0, 5);
    settle(&mut ui, &mut env);
    typ(&mut ui, &mut env, "\"");
    assert_eq!(code.peek(), "// a \"");

    let (mut ui, mut env, code, ctrl) = harness("word");
    ctrl.select((0, 0), (0, 4));
    settle(&mut ui, &mut env);
    typ(&mut ui, &mut env, "|");
    assert_eq!(code.peek(), "|word|");
}

#[test]
fn toggle_comment_uses_the_configured_token_and_snippets_insert() {
    let (mut ui, mut env, code, ctrl) = harness("x");
    ctrl.run_command(EditorCommand::from_id("editor.action.commentLine").unwrap());
    settle(&mut ui, &mut env);
    assert_eq!(code.peek(), "// x");

    let (mut ui, mut env, code, ctrl) = harness("");
    ctrl.insert_snippet("pub fn ${1:name}() void {\n\t$0\n}");
    settle(&mut ui, &mut env);
    assert_eq!(code.peek(), "pub fn name() void {\n    \n}");
    typ(&mut ui, &mut env, "run");
    assert!(code.peek().starts_with("pub fn run()"), "{}", code.peek());
}
