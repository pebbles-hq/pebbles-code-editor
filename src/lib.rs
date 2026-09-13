//! # pebbles-code-editor
//!
//! An embeddable, syntax-highlighting **code editor** widget for the
//! [Pebbles](https://github.com/pebbles-hq/pebbles) GUI framework — the foundation to
//! build an IDE on.
//!
//! This is a **custom editing engine**, not the framework's text field: it owns the
//! buffer, cursor, selection, layout, and input. Because code is monospace it renders
//! on a fixed grid
//! (caret x = column × advance, line y = row × line-height), which keeps caret placement
//! and click hit-testing exact and cheap.
//!
//! - **Real editing** — type, backspace/delete, word-delete, Enter (auto-indent), Tab,
//!   arrows + word/line/doc motions, Shift-select, Ctrl+A, and Copy/Cut/Paste — all built
//!   on an invertible [`ChangeSet`]/[`Transaction`] model with undo/redo.
//! - **Multiple cursors & selections** — a [`Selections`] set (sorted, merged, with a
//!   primary): Alt-click adds a caret, Ctrl/Cmd+D adds the next occurrence, Shift+Alt-drag
//!   makes a rectangular (column) selection, double-click selects a word, triple-click a
//!   line, Esc collapses; every edit and motion applies to all cursors at once.
//! - **Caret autoscroll** — keyboard navigation keeps the caret in view (when a fixed
//!   `height` makes the editor scroll).
//! - **Syntax highlighting** via a pluggable [`Language`] layer (Rust + JSON bundled).
//! - **Line-number gutter** with an active-line marker, **current-line highlight**,
//!   selection, and a blinking caret.
//! - Swappable **themes** ([`EditorTheme`]).
//!
//! ```ignore
//! use pebbles::prelude::*;
//! use pebbles_code_editor::{code_editor, lang::Rust, EditorTheme};
//!
//! fn view() -> impl IntoWidget {
//!     let code = create_signal(String::from("fn main() {}\n"));
//!     code_editor(code).language(Box::new(Rust)).theme(EditorTheme::dark()).height(420.0)
//! }
//! ```

pub mod edit;
pub mod extensions;
pub mod lang;
pub mod providers;

mod brackets;
mod geometry;
mod highlight;
mod commands;
mod config;
mod search;
mod view;
mod theme;

pub use config::{CodeEditor, code_editor};
pub use edit::{ChangeSet, EditorState, History, Selection, Selections, Transaction};
pub use extensions::{
    Command, Decoration, DecoStyle, EditContext, Extension, GutterMark, Snapshot, extension,
};
pub use lang::{Language, SyntaxNode, Token, TokenKind, bracket_tree};
pub use providers::{
    CompletionContext, CompletionItem, CompletionKind, CompletionProvider, DefinitionProvider,
    Diagnostic, Diagnostics, FormatProvider, Hover, HoverProvider, InlayHint, InlayHints, Severity,
    SignatureHelp, SignatureProvider,
};
pub use theme::EditorTheme;

/// The monospace family used for all editor text.
pub(crate) const MONO: &str = "JetBrains Mono";
/// Default advance width of one glyph, as a fraction of the font size (JetBrains Mono ≈ 0.6).
/// Configurable via [`CodeEditor::advance_ratio`] so metrics are exact for any monospace font.
pub(crate) const ADVANCE_RATIO: f64 = 0.6;

