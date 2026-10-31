//! Host-facing API regressions found while embedding the editor in an IDE (Gravel):
//! overridable built-in keybindings that bubble to the app, per-document state that resets
//! when the bound document changes, and the programmatic caret/selection controller.

use std::cell::RefCell;

use pebbles::core::{Ui, create_root_signal};
use pebbles::prelude::*;
use pebbles::render::TextEnv;
use pebbles_code_editor::{
    BuiltinCommand, DocumentHistory, EditorController, EditorTheme, Keymap, code_editor,
    editor_controller,
};

/// (doc A, doc B, which is shown, controller)
type Docs = (Signal<String>, Signal<String>, Signal<u8>, EditorController);
/// (reused code signal, document id, controller)
type Same = (Signal<String>, Signal<u64>, EditorController);

const SIZE: Size = Size {
    width: 500.0,
    height: 300.0,
};

fn settle(ui: &mut Ui, env: &mut TextEnv) {
    for _ in 0..4 {
        ui.rebuild_if_dirty();
        ui.layout(env, SIZE);
    }
}

fn mount(root: AnyWidget) -> (Ui, TextEnv) {
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    pebbles::core::keyboard::set_modifiers(false, false, false, false);
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(View::new(Color::from_rgba8(255, 255, 255, 255), root).into_widget());
    settle(&mut ui, &mut env);
    (ui, env)
}

fn key(ui: &mut Ui, env: &mut TextEnv, k: KeyInput) -> bool {
    let handled = ui.dispatch_key(k);
    settle(ui, env);
    handled
}

// ---- keymap: disable / remap built-ins; unserviceable intents bubble -------------------

#[test]
fn builtin_commands_are_handled_by_default() {
    let code = create_root_signal(String::from("fn main() {}\n"));
    let (mut ui, mut env) = mount(code_editor(code).autofocus().into_widget());
    assert!(
        key(&mut ui, &mut env, KeyInput::CommandPalette),
        "palette is a built-in"
    );
    // No completion / definition / format provider configured: nothing to do → bubble.
    for k in [
        KeyInput::TriggerCompletion,
        KeyInput::GoToDefinition,
        KeyInput::Format,
    ] {
        assert!(
            !key(&mut ui, &mut env, k.clone()),
            "{k:?} with no provider must bubble to app shortcuts"
        );
    }
}

#[test]
fn disabled_builtin_bubbles_to_the_app() {
    let code = create_root_signal(String::from("abc\n"));
    let (mut ui, mut env) = mount(
        code_editor(code)
            .autofocus()
            .keymap(
                Keymap::new()
                    .disable(BuiltinCommand::CommandPalette)
                    .disable(BuiltinCommand::Find),
            )
            .into_widget(),
    );
    assert!(
        !key(&mut ui, &mut env, KeyInput::CommandPalette),
        "disabled palette chord is declined"
    );
    assert!(
        !key(&mut ui, &mut env, KeyInput::Find),
        "disabled find chord is declined"
    );
    assert!(
        key(&mut ui, &mut env, KeyInput::Insert("x".into())),
        "typing still works"
    );
    assert_eq!(code.peek(), "xabc\n");
}

#[test]
fn bound_chord_runs_the_builtin_while_focused() {
    let code = create_root_signal(String::from("abc\n"));
    let (mut ui, mut env) = mount(
        code_editor(code)
            .autofocus()
            .keymap(
                Keymap::new()
                    .disable(BuiltinCommand::ToggleComment)
                    .bind("F1", BuiltinCommand::ToggleComment),
            )
            .language(Box::new(pebbles_code_editor::lang::Rust))
            .into_widget(),
    );
    assert!(!key(&mut ui, &mut env, KeyInput::ToggleComment));
    assert_eq!(code.peek(), "abc\n", "default chord disabled");
    let mods = Mods {
        shift: false,
        ctrl: false,
        alt: false,
        meta: false,
    };
    assert!(pebbles::core::shortcuts::dispatch(
        ui.window_id(),
        mods,
        ShortcutKey::F(1)
    ));
    settle(&mut ui, &mut env);
    assert!(
        code.peek().starts_with("//"),
        "F1 toggled the comment: {:?}",
        code.peek()
    );
}

// ---- per-document state ------------------------------------------------------------------

thread_local! {
    static DOCS: RefCell<Option<Docs>> =
        const { RefCell::new(None) };
}

fn docs() -> Docs {
    DOCS.with(|d| d.borrow().expect("docs set"))
}

fn switcher() -> AnyWidget {
    let (a, b, which, ctrl) = docs();
    let code = if which.get() == 0 { a } else { b };
    code_editor(code)
        .theme(EditorTheme::dark())
        .height(300.0)
        .autofocus()
        .controller(ctrl)
        .into_widget()
}

#[test]
fn switching_the_document_signal_resets_editor_state() {
    let a = create_root_signal(String::from("first document\nline two\n"));
    let b = create_root_signal(String::from("B\n"));
    let which = create_root_signal(0u8);
    let ctrl = editor_controller();
    DOCS.with(|d| *d.borrow_mut() = Some((a, b, which, ctrl)));
    let (mut ui, mut env) = mount(component(switcher).into_widget());

    ctrl.set_cursor(1, 4);
    settle(&mut ui, &mut env);
    assert_eq!(ctrl.cursor().peek(), (1, 4));

    // Switch documents: the new editor starts fresh at (0, 0) on B — not on A's state.
    which.set(1);
    settle(&mut ui, &mut env);
    assert_eq!(
        ctrl.cursor().peek(),
        (0, 0),
        "caret reset for the new document"
    );
    // Focus follows the fresh editor's autofocus; typing edits B only.
    ctrl.focus();
    settle(&mut ui, &mut env);
    key(&mut ui, &mut env, KeyInput::Insert("x".into()));
    assert_eq!(b.peek(), "xB\n");
    assert_eq!(a.peek(), "first document\nline two\n", "A untouched");
}

// ---- controller ----------------------------------------------------------------------------

#[test]
fn controller_moves_caret_selects_and_opens_at_line_across_a_switch() {
    let a = create_root_signal(String::from("alpha\nbeta\ngamma\n"));
    let b = create_root_signal(String::from("one\ntwo\nthree\nfour\n"));
    let which = create_root_signal(0u8);
    let ctrl = editor_controller();
    DOCS.with(|d| *d.borrow_mut() = Some((a, b, which, ctrl)));
    let (mut ui, mut env) = mount(component(switcher).into_widget());

    // Caret placement, then typing lands there.
    ctrl.set_cursor(1, 2);
    settle(&mut ui, &mut env);
    key(&mut ui, &mut env, KeyInput::Insert("X".into()));
    assert_eq!(a.peek(), "alpha\nbeXta\ngamma\n");

    // Selection: typing replaces it.
    ctrl.select((2, 0), (2, 5));
    settle(&mut ui, &mut env);
    key(&mut ui, &mut env, KeyInput::Insert("G".into()));
    assert_eq!(a.peek(), "alpha\nbeXta\nG\n");

    // "Open file at line": switch documents and place the caret in the same handler — the
    // request must land on the NEW document.
    which.set(1);
    ctrl.set_cursor(2, 1);
    settle(&mut ui, &mut env);
    assert_eq!(ctrl.cursor().peek(), (2, 1));
    ctrl.focus();
    settle(&mut ui, &mut env);
    key(&mut ui, &mut env, KeyInput::Insert("!".into()));
    assert_eq!(b.peek(), "one\ntwo\nt!hree\nfour\n");
    assert_eq!(a.peek(), "alpha\nbeXta\nG\n", "old document untouched");

    // Out-of-range positions clamp instead of panicking.
    ctrl.set_cursor(999, 999);
    ctrl.scroll_to(999);
    settle(&mut ui, &mut env);
}

thread_local! {
    static SAME: RefCell<Option<Same>> =
        const { RefCell::new(None) };
}

fn same_signal() -> AnyWidget {
    let (code, id, ctrl) = SAME.with(|d| d.borrow().expect("set"));
    code_editor(code)
        .document_id(id.get())
        .controller(ctrl)
        .into_widget()
}

#[test]
fn document_id_change_resets_state_for_a_reused_signal() {
    let code = create_root_signal(String::from(
        "aaaa
bbbb
",
    ));
    let id = create_root_signal(1u64);
    let ctrl = editor_controller();
    SAME.with(|d| *d.borrow_mut() = Some((code, id, ctrl)));
    let (mut ui, mut env) = mount(component(same_signal).into_widget());
    ctrl.set_cursor(1, 3);
    settle(&mut ui, &mut env);
    assert_eq!(ctrl.cursor().peek(), (1, 3));
    // Same signal, new file: set the text AND bump the id → fresh state at (0, 0).
    code.set(String::from("other file\nmore\n"));
    id.set(2);
    settle(&mut ui, &mut env);
    assert_eq!(ctrl.cursor().peek(), (0, 0));
}

type Hist = (
    Signal<String>,
    Signal<String>,
    Signal<u8>,
    EditorController,
    DocumentHistory,
    DocumentHistory,
);

thread_local! {
    static HIST: RefCell<Option<Hist>> = const { RefCell::new(None) };
}

fn with_histories() -> AnyWidget {
    let (a, b, which, ctrl, ha, hb) = HIST.with(|d| d.borrow().clone().expect("set"));
    let (code, hist) = if which.get() == 0 { (a, ha) } else { (b, hb) };
    code_editor(code)
        .history(hist)
        .controller(ctrl)
        .autofocus()
        .into_widget()
}

/// VS Code keeps each model's undo stack for the session: switching documents (which
/// remounts the editor) and coming back must still undo/redo; a text change made while
/// detached drops the now-stale history instead of corrupting the text.
#[test]
fn document_history_survives_switching_documents() {
    let a = create_root_signal(String::from("alpha\n"));
    let b = create_root_signal(String::from("beta\n"));
    let which = create_root_signal(0u8);
    let ctrl = editor_controller();
    let (ha, hb) = (DocumentHistory::new(), DocumentHistory::new());
    HIST.with(|d| *d.borrow_mut() = Some((a, b, which, ctrl, ha.clone(), hb.clone())));
    let (mut ui, mut env) = mount(component(with_histories).into_widget());
    settle(&mut ui, &mut env);
    key(&mut ui, &mut env, KeyInput::Insert("X".into()));
    assert_eq!(a.peek(), "Xalpha\n");
    which.set(1);
    settle(&mut ui, &mut env);
    ctrl.focus();
    settle(&mut ui, &mut env);
    key(&mut ui, &mut env, KeyInput::Insert("Y".into()));
    assert_eq!(b.peek(), "Ybeta\n");
    which.set(0);
    settle(&mut ui, &mut env);
    ctrl.focus();
    settle(&mut ui, &mut env);
    assert!(ha.can_undo());
    key(&mut ui, &mut env, KeyInput::Undo);
    assert_eq!(
        a.peek(),
        "alpha\n",
        "a's typing undone after the round trip"
    );
    assert!(ha.can_redo());
    key(&mut ui, &mut env, KeyInput::Redo);
    assert_eq!(a.peek(), "Xalpha\n");
    // b changes on disk while a is shown: its history no longer applies.
    b.set(String::from("reloaded\n"));
    which.set(1);
    settle(&mut ui, &mut env);
    ctrl.focus();
    settle(&mut ui, &mut env);
    key(&mut ui, &mut env, KeyInput::Undo);
    assert_eq!(b.peek(), "reloaded\n", "stale history dropped");
    assert!(!hb.can_undo());
}

// ---- theming: the editor's own UI follows EditorTheme (an IDE theme drives it) ----------

fn paragraphs(ui: &Ui) -> Vec<(String, f32, Color)> {
    let rt = ui.render_tree();
    rt.find_all::<pebbles::render::RenderParagraph>()
        .into_iter()
        .filter_map(|id| {
            rt.object_ref(id)
                .downcast_ref::<pebbles::render::RenderParagraph>()
                .map(|p| (p.text.clone(), p.style.font_size, p.style.color))
        })
        .collect()
}

#[test]
fn editor_ui_chrome_is_driven_by_the_theme_tokens() {
    let code = create_root_signal(String::from("fn main() {}\n"));
    let mut theme = EditorTheme::dark();
    theme.ui_font_size = 20.0;
    theme.foreground = Color::from_rgba8(1, 2, 3, 255);
    theme.gutter_fg = Color::from_rgba8(4, 5, 6, 255);
    theme.overlay_radius = 3.0;
    let (ui, _env) = mount(
        code_editor(code)
            .title("main.rs")
            .theme(theme)
            .height(300.0)
            .into_widget(),
    );
    let ps = paragraphs(&ui);
    let title = ps.iter().find(|p| p.0 == "main.rs").expect("title painted");
    assert_eq!(title.1, 20.0, "title at ui_font_size");
    assert_eq!(
        title.2,
        Color::from_rgba8(1, 2, 3, 255),
        "title in foreground"
    );
    let status = ps
        .iter()
        .find(|p| p.0.contains("Ln 1, Col 1"))
        .expect("status painted");
    assert_eq!(status.1, 19.0, "status one px smaller");
    assert_eq!(status.2, Color::from_rgba8(4, 5, 6, 255));
}
