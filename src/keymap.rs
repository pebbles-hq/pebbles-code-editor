//! The editor's **built-in keybindings** and how a host overrides them — VS Code's
//! `keybindings.json` model reduced to what an embedding app needs: disable a built-in
//! (`-editor.action.quickOpen`) or bind it to a different chord.
//!
//! The framework turns physical chords into semantic [`KeyInput`] intents (Ctrl+F →
//! [`KeyInput::Find`], Ctrl+Shift+P → [`KeyInput::CommandPalette`], …). A disabled built-in
//! is **declined** by the editor, so the key bubbles to the app's own shortcuts
//! (`create_shortcut`) — the Flutter `Shortcuts`/`Actions` bubbling the shell implements.
//! Intents the editor cannot serve (Ctrl+Space with no completion provider, F12 with no
//! definition provider, Shift+Alt+F with no formatter) bubble the same way.

use pebbles::prelude::KeyInput;

/// An editor command that has a built-in keybinding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BuiltinCommand {
    /// Open the find bar (Ctrl+F).
    Find,
    /// Open find-and-replace (Ctrl+H).
    Replace,
    /// Open the editor's command palette (Ctrl+Shift+P).
    CommandPalette,
    /// Request completions (Ctrl+Space).
    TriggerCompletion,
    /// Go to definition (F12).
    GoToDefinition,
    /// Format the document (Shift+Alt+F).
    Format,
    /// Add the next occurrence to the selection (Ctrl+D).
    SelectNextOccurrence,
    /// Toggle line comments (Ctrl+/).
    ToggleComment,
}

impl BuiltinCommand {
    /// The command a framework intent invokes, if it is one of the overridable built-ins.
    pub fn of_key(k: &KeyInput) -> Option<Self> {
        Some(match k {
            KeyInput::Find => Self::Find,
            KeyInput::Replace => Self::Replace,
            KeyInput::CommandPalette => Self::CommandPalette,
            KeyInput::TriggerCompletion => Self::TriggerCompletion,
            KeyInput::GoToDefinition => Self::GoToDefinition,
            KeyInput::Format => Self::Format,
            KeyInput::SelectNextOccurrence => Self::SelectNextOccurrence,
            KeyInput::ToggleComment => Self::ToggleComment,
            _ => return None,
        })
    }

    /// The intent that runs this command.
    pub fn key(self) -> KeyInput {
        match self {
            Self::Find => KeyInput::Find,
            Self::Replace => KeyInput::Replace,
            Self::CommandPalette => KeyInput::CommandPalette,
            Self::TriggerCompletion => KeyInput::TriggerCompletion,
            Self::GoToDefinition => KeyInput::GoToDefinition,
            Self::Format => KeyInput::Format,
            Self::SelectNextOccurrence => KeyInput::SelectNextOccurrence,
            Self::ToggleComment => KeyInput::ToggleComment,
        }
    }
}

/// Overrides for the editor's built-in keybindings. Default: every built-in on its default
/// chord.
///
/// ```ignore
/// // The IDE owns Ctrl+Shift+P (its own palette) and wants the editor palette on F1.
/// code_editor(code).keymap(
///     Keymap::new()
///         .disable(BuiltinCommand::CommandPalette)
///         .bind("F1", BuiltinCommand::CommandPalette),
/// )
/// ```
#[derive(Clone, Debug, Default)]
pub struct Keymap {
    pub(crate) disabled: Vec<BuiltinCommand>,
    pub(crate) bindings: Vec<(String, BuiltinCommand)>,
    /// Editor commands whose default chords are removed.
    pub(crate) unbound: Vec<crate::actions::EditorCommand>,
    /// Extra / replacement chords for editor commands.
    pub(crate) command_bindings: Vec<(String, crate::actions::EditorCommand)>,
}

impl Keymap {
    /// The default keymap (no overrides).
    pub fn new() -> Self {
        Self::default()
    }

    /// Unbind `cmd`'s **default** chord: the editor declines it, so the key bubbles to the
    /// app's shortcuts. Chords added with [`bind`](Self::bind) still run the command.
    pub fn disable(mut self, cmd: BuiltinCommand) -> Self {
        if !self.disabled.contains(&cmd) {
            self.disabled.push(cmd);
        }
        self
    }

    /// Additionally run `cmd` on `chord` (pebbles shortcut syntax, e.g. `"Mod+K"`, `"F1"`)
    /// while the editor is focused. Pair with [`disable`](Self::disable) to *remap*.
    pub fn bind(mut self, chord: impl Into<String>, cmd: BuiltinCommand) -> Self {
        self.bindings.push((chord.into(), cmd));
        self
    }

    /// Remove every default chord of editor command `cmd` (see
    /// [`EditorCommand::default_keys`](crate::EditorCommand::default_keys)); it stays runnable
    /// from the palette and from chords added with [`bind_command`](Self::bind_command).
    pub fn unbind_command(mut self, cmd: crate::actions::EditorCommand) -> Self {
        if !self.unbound.contains(&cmd) {
            self.unbound.push(cmd);
        }
        self
    }

    /// Run editor command `cmd` on `chord` (pebbles shortcut syntax; a space separates the
    /// strokes of a multi-stroke chord, e.g. `"Ctrl+K Ctrl+0"`). A host chord replaces any
    /// default bound to the same chord. Pair with [`unbind_command`](Self::unbind_command) to
    /// move a command to a new chord.
    pub fn bind_command(
        mut self,
        chord: impl Into<String>,
        cmd: crate::actions::EditorCommand,
    ) -> Self {
        self.command_bindings.push((chord.into(), cmd));
        self
    }

    /// The effective chords for `cmd` after overrides (defaults unless unbound, plus host
    /// bindings) — for showing keybindings in menus / the palette.
    pub fn chords_for(&self, cmd: crate::actions::EditorCommand) -> Vec<String> {
        let mut v: Vec<String> = if self.unbound.contains(&cmd) {
            Vec::new()
        } else {
            cmd.default_keys().iter().map(|s| s.to_string()).collect()
        };
        v.extend(
            self.command_bindings
                .iter()
                .filter(|(_, c)| *c == cmd)
                .map(|(k, _)| k.clone()),
        );
        v
    }

    /// Whether `cmd`'s default chord is disabled.
    pub fn is_disabled(&self, cmd: BuiltinCommand) -> bool {
        self.disabled.contains(&cmd)
    }
}
