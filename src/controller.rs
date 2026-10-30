//! [`EditorController`] — drive the editor's caret, selection and scroll from the host
//! (Flutter's `TextEditingController` / Monaco's `editor.setPosition` + `revealLine`).
//!
//! Positions are **0-based** `(line, column)`, columns counted in `char`s (Unicode scalar
//! values), clamped to the document. Requests are queued and applied by the editor on its next
//! render — so "open file at line N" works even when the same handler also switches the
//! document (the request lands on the newly mounted editor, not the old one).

use pebbles::prelude::*;

/// A queued host request.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Request {
    /// Collapse the selection to a caret at `(line, col)`.
    Cursor(usize, usize),
    /// Select from `anchor` to `head` (the caret ends at `head`).
    Select((usize, usize), (usize, usize)),
    /// Scroll so `line` is at the top of the viewport (with a one-line margin).
    ScrollTo(usize),
    /// Give the editor keyboard focus.
    Focus,
    /// Run a built-in editor command.
    Run(crate::actions::EditorCommand),
}

/// A handle for driving an editor programmatically. `Copy`; create it with
/// [`editor_controller`] in the host component and pass it to
/// [`CodeEditor::controller`](crate::CodeEditor::controller).
///
/// ```ignore
/// let ctrl = editor_controller();
/// // open at line 42 (0-based 41), column 0, and focus the editor:
/// ctrl.set_cursor(41, 0);
/// ctrl.focus();
/// code_editor(code).height(600.0).controller(ctrl)
/// ```
#[derive(Clone, Copy)]
pub struct EditorController {
    pub(crate) requests: Signal<Vec<Request>>,
    pub(crate) cursor: Signal<(usize, usize)>,
}

/// Create an [`EditorController`] (a hook: call it at the top level of a component, or at app
/// scope).
pub fn editor_controller() -> EditorController {
    EditorController {
        requests: create_signal(Vec::new()),
        cursor: create_signal((0, 0)),
    }
}

impl EditorController {
    /// Move the caret to `(line, col)` (0-based, char columns), clearing any selection. A
    /// fixed-height editor scrolls it into view.
    pub fn set_cursor(&self, line: usize, col: usize) {
        self.requests.update(|r| r.push(Request::Cursor(line, col)));
    }

    /// Select from `anchor` to `head` (each `(line, col)`, 0-based); the caret ends at `head`.
    pub fn select(&self, anchor: (usize, usize), head: (usize, usize)) {
        self.requests
            .update(|r| r.push(Request::Select(anchor, head)));
    }

    /// Scroll a fixed-height editor so `line` (0-based) is at the top of the viewport.
    pub fn scroll_to(&self, line: usize) {
        self.requests.update(|r| r.push(Request::ScrollTo(line)));
    }

    /// Give the editor keyboard focus.
    pub fn focus(&self) {
        self.requests.update(|r| r.push(Request::Focus));
    }

    /// Run a built-in [`EditorCommand`](crate::EditorCommand) — e.g. on save:
    /// `TrimTrailingWhitespace` + `InsertFinalNewline` (VS Code `files.*` save actions), or
    /// any command from a host menu.
    pub fn run_command(&self, cmd: crate::actions::EditorCommand) {
        self.requests.update(|r| r.push(Request::Run(cmd)));
    }

    /// The primary caret's `(line, col)` (0-based, char columns) — reactive: reading it with
    /// `.get()` inside a component re-renders on caret moves (e.g. a "Ln 3, Col 7" status).
    pub fn cursor(&self) -> Signal<(usize, usize)> {
        self.cursor
    }
}
