//! Host-facing API regressions found while embedding the editor in an IDE (Gravel):
//! overridable built-in keybindings that bubble to the app
//! (more host-API cases are added alongside the features they cover).

use pebbles::core::{Ui, create_root_signal};
use pebbles::prelude::*;
use pebbles::render::TextEnv;
use pebbles_code_editor::{BuiltinCommand, Keymap, code_editor};

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
