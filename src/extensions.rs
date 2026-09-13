//! Extensibility: the composable [`Extension`] — the editor's plugin model.
//!
//! An extension is a bundle of contributions the editor renders and wires: **decorations**
//! (range styling / line backgrounds), **gutter markers**, **commands** (runnable from the
//! palette), **read-only ranges** (edits there are vetoed), and **event hooks** (change /
//! selection). Compose several with `code_editor(..).extensions(vec![a, b, c])` — each is a
//! plain builder of `Rc<dyn Fn…>` closures, so a "plugin" is just an `Extension` value.
//!
//! Closures own their own state (capture an `Rc<RefCell<_>>`), which is how a plugin keeps
//! private state without the editor needing a typed state-field registry.

use std::cell::RefCell;
use std::rc::Rc;

use pebbles::prelude::{Color, Signal};

use crate::commands::dispatch;
use crate::edit::{ChangeSet, Coalesce, EditorState, History, Selection, Selections, Transaction};
use crate::geometry::col_of;

/// A read-only view of the editor handed to extension callbacks.
pub struct Snapshot<'a> {
    /// The full document text.
    pub text: &'a str,
    /// The primary caret byte offset.
    pub caret: usize,
    /// The primary selection as `(min, max)` bytes (`min == max` for a bare caret).
    pub selection: (usize, usize),
}

/// Inline / line styling applied to a byte range by a [`Decoration`].
#[derive(Clone, Copy, Debug, Default)]
pub struct DecoStyle {
    /// A highlight box behind the range.
    pub background: Option<Color>,
    /// An underline under the range.
    pub underline: Option<Color>,
    /// A whole-line background for every line the range touches.
    pub line_background: Option<Color>,
}

/// A styled byte range contributed by an extension (mark = background/underline, line =
/// whole-line background). Widget/replace decorations (inline widgets) are not supported on
/// the fixed grid — they'd need line reflow.
#[derive(Clone, Copy, Debug)]
pub struct Decoration {
    pub range: (usize, usize),
    pub style: DecoStyle,
}

impl Decoration {
    /// A background highlight over `range`.
    pub fn background(range: (usize, usize), color: Color) -> Self {
        Decoration {
            range,
            style: DecoStyle {
                background: Some(color),
                ..Default::default()
            },
        }
    }
    /// An underline under `range`.
    pub fn underline(range: (usize, usize), color: Color) -> Self {
        Decoration {
            range,
            style: DecoStyle {
                underline: Some(color),
                ..Default::default()
            },
        }
    }
    /// A whole-line background for the lines `range` touches.
    pub fn line(range: (usize, usize), color: Color) -> Self {
        Decoration {
            range,
            style: DecoStyle {
                line_background: Some(color),
                ..Default::default()
            },
        }
    }
}

/// A colored marker in the gutter for a line (breakpoints, VCS gutter, diagnostics, …).
#[derive(Clone, Copy, Debug)]
pub struct GutterMark {
    pub line: usize,
    pub color: Color,
}

/// A mergeable configuration facet an extension can contribute. Each `Some` field overrides
/// the editor's resolved config (extensions apply in order, so a later `Some` wins). This is
/// the editor's "config facet" mechanism — a plugin can ship its own preferred defaults
/// (e.g. a "2-space indent" plugin) without the caller wiring every knob.
#[derive(Clone, Copy, Debug, Default)]
pub struct ConfigPatch {
    pub tab_size: Option<usize>,
    pub insert_spaces: Option<bool>,
    pub indent_guides: Option<bool>,
    pub render_whitespace: Option<bool>,
    pub match_brackets: Option<bool>,
    pub auto_close: Option<bool>,
}

/// The handle a [`Command`] uses to read and mutate the editor.
#[derive(Clone, Copy)]
pub struct EditContext {
    pub(crate) state: Signal<EditorState>,
    pub(crate) history: Signal<Rc<RefCell<History>>>,
    pub(crate) code: Signal<String>,
    pub(crate) goal: Signal<usize>,
}

impl EditContext {
    /// The current document text.
    pub fn text(&self) -> String {
        self.state.peek().text()
    }
    /// The primary caret byte offset.
    pub fn caret(&self) -> usize {
        self.state.peek().primary().head
    }
    /// The primary selection as `(min, max)` bytes.
    pub fn selection(&self) -> (usize, usize) {
        let s = self.state.peek().primary();
        (s.min(), s.max())
    }
    /// Replace `from..to` with `insert`, leaving the caret after it (one undo step).
    pub fn replace(&self, from: usize, to: usize, insert: impl Into<String>) {
        let insert = insert.into();
        let caret = from.min(to) + insert.len();
        dispatch(
            self.state,
            self.history,
            self.code,
            Transaction::change_and_select(
                ChangeSet::replace(from, to, insert),
                Selections::single(Selection::caret(caret)),
            ),
            Coalesce::Never,
        );
        self.goal
            .set(col_of(&self.state.peek().text(), self.state.peek().primary().head));
    }
    /// Insert `text` at the caret (replacing any selection).
    pub fn insert(&self, text: impl Into<String>) {
        let (a, b) = self.selection();
        self.replace(a, b, text);
    }
    /// Move the caret / set the selection to `from..head` (no edit).
    pub fn select(&self, from: usize, head: usize) {
        let cur = self.state.peek();
        self.state.set(EditorState {
            doc: cur.doc.clone(),
            selection: Selections::single(Selection::range(from, head)),
        });
    }
}

/// A command contributed by an extension — runnable from the command palette.
#[derive(Clone)]
pub struct Command {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) run: Rc<dyn Fn(&EditContext)>,
}

impl Command {
    /// Create a command: a stable `id`, a human `title` (shown in the palette), and the action.
    pub fn new(id: impl Into<String>, title: impl Into<String>, run: impl Fn(&EditContext) + 'static) -> Self {
        Command {
            id: id.into(),
            title: title.into(),
            run: Rc::new(run),
        }
    }
    /// The stable command id.
    pub fn id(&self) -> &str {
        &self.id
    }
    /// The palette title.
    pub fn title(&self) -> &str {
        &self.title
    }
}

type DecoFn = Rc<dyn Fn(&Snapshot) -> Vec<Decoration>>;
type GutterFn = Rc<dyn Fn(&Snapshot) -> Vec<GutterMark>>;
type RangeFn = Rc<dyn Fn(&Snapshot) -> Vec<(usize, usize)>>;
type HookFn = Rc<dyn Fn(&Snapshot)>;
type FocusFn = Rc<dyn Fn(bool)>;
/// A per-plugin pointer handler: `(snapshot, byte_offset_of_the_click)`.
type ClickFn = Rc<dyn Fn(&Snapshot, usize)>;
/// A theme transform: takes the current theme and returns a (possibly) modified one.
type ThemeFn = Rc<dyn Fn(crate::theme::EditorTheme) -> crate::theme::EditorTheme>;

/// A configurable keybinding: a chord (framework grammar, e.g. `"Mod+K"`, `"Ctrl+Shift+P"`)
/// bound to a [`Command`] id. The editor registers it while focused.
#[derive(Clone)]
pub(crate) struct KeyBinding {
    pub(crate) chord: String,
    pub(crate) command: String,
}

/// A composable editor extension (plugin). Build one with [`extension`] and add it via
/// `code_editor(..).extensions(..)`.
#[derive(Clone, Default)]
pub struct Extension {
    pub(crate) name: String,
    pub(crate) decorations: Option<DecoFn>,
    pub(crate) gutter: Option<GutterFn>,
    pub(crate) read_only: Option<RangeFn>,
    pub(crate) commands: Vec<Command>,
    pub(crate) keys: Vec<KeyBinding>,
    pub(crate) config: Option<ConfigPatch>,
    pub(crate) theme: Option<ThemeFn>,
    pub(crate) on_change: Option<HookFn>,
    pub(crate) on_selection: Option<HookFn>,
    pub(crate) on_focus: Option<FocusFn>,
    pub(crate) on_click: Option<ClickFn>,
}

/// Find the command with `id` across `exts` and run it against `ctx` (no-op if absent).
pub(crate) fn run_command_by_id(exts: &[Extension], id: &str, ctx: &EditContext) {
    for e in exts {
        if let Some(cmd) = e.commands.iter().find(|c| c.id == id) {
            (cmd.run)(ctx);
            return;
        }
    }
}

/// Start building an [`Extension`] named `name`.
pub fn extension(name: impl Into<String>) -> Extension {
    Extension {
        name: name.into(),
        ..Default::default()
    }
}

impl Extension {
    /// The extension's name.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Contribute decorations, recomputed from the current [`Snapshot`] each render.
    pub fn decorations(mut self, f: impl Fn(&Snapshot) -> Vec<Decoration> + 'static) -> Self {
        self.decorations = Some(Rc::new(f));
        self
    }
    /// Contribute gutter markers, recomputed from the current [`Snapshot`] each render.
    pub fn gutter_markers(mut self, f: impl Fn(&Snapshot) -> Vec<GutterMark> + 'static) -> Self {
        self.gutter = Some(Rc::new(f));
        self
    }
    /// Declare read-only byte ranges — edits overlapping them are vetoed.
    pub fn read_only_ranges(mut self, f: impl Fn(&Snapshot) -> Vec<(usize, usize)> + 'static) -> Self {
        self.read_only = Some(Rc::new(f));
        self
    }
    /// Add a command (runnable from the palette, or bound to a key via [`keybinding`](Self::keybinding)).
    pub fn command(mut self, command: Command) -> Self {
        self.commands.push(command);
        self
    }
    /// Bind a key `chord` (framework grammar, e.g. `"Mod+K"`, `"Ctrl+Shift+U"`) to one of this
    /// extension's command ids. The binding is active while the editor is focused. This is the
    /// configurable-keymap surface — the same command can be reached from the palette *and* a key.
    pub fn keybinding(mut self, chord: impl Into<String>, command_id: impl Into<String>) -> Self {
        self.keys.push(KeyBinding {
            chord: chord.into(),
            command: command_id.into(),
        });
        self
    }
    /// Contribute a mergeable [`ConfigPatch`] — the plugin's preferred editor settings.
    pub fn config(mut self, patch: ConfigPatch) -> Self {
        self.config = Some(patch);
        self
    }
    /// Contribute a theme transform — "theme as an extension". The closure receives the
    /// current [`EditorTheme`](crate::EditorTheme) and returns a modified one; transforms from
    /// several extensions compose in order. Return a fresh theme to fully replace it, or tweak
    /// a few slots (e.g. `t.syntax.comment = t.syntax.comment.italic()`) to compose over it.
    pub fn theme(
        mut self,
        f: impl Fn(crate::theme::EditorTheme) -> crate::theme::EditorTheme + 'static,
    ) -> Self {
        self.theme = Some(Rc::new(f));
        self
    }
    /// Fire after the document changes.
    pub fn on_change(mut self, f: impl Fn(&Snapshot) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
    /// Fire after the selection changes.
    pub fn on_selection(mut self, f: impl Fn(&Snapshot) + 'static) -> Self {
        self.on_selection = Some(Rc::new(f));
        self
    }
    /// Fire when the editor gains (`true`) or loses (`false`) focus.
    pub fn on_focus(mut self, f: impl Fn(bool) + 'static) -> Self {
        self.on_focus = Some(Rc::new(f));
        self
    }
    /// Fire on a pointer press in the text area, with the byte offset under the pointer.
    pub fn on_click(mut self, f: impl Fn(&Snapshot, usize) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}
