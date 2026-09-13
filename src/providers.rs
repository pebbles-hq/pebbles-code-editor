//! IntelliSense-style **providers**: the dev-supplied hooks the editor renders UI for. The
//! editor owns the popup, tooltip, underlines, gutter markers, and inlay rendering; the app
//! (typically backed by a language server) supplies the *data* through these callbacks and
//! signals — exactly the CodeMirror/Monaco provider model.
//!
//! Providers are synchronous `Rc<dyn Fn…>` callbacks (call your LSP/analysis and return the
//! result). Data that changes out-of-band (diagnostics, inlay hints) is passed as a
//! `Signal` the editor reads reactively.

use std::rc::Rc;

use pebbles::prelude::Signal;

// ---------------------------------------------------------------------------
// Completion
// ---------------------------------------------------------------------------

/// The kind of a completion item — drives the leading glyph/color in the popup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionKind {
    Keyword,
    Function,
    Method,
    Variable,
    Field,
    Type,
    Module,
    Constant,
    Snippet,
    Text,
}

/// One entry in the completion popup.
#[derive(Clone, Debug)]
pub struct CompletionItem {
    /// Text shown in the popup.
    pub label: String,
    /// Text inserted on accept (defaults to `label`). A [`CompletionKind::Snippet`] may embed
    /// tabstops `$1`, `$2`, … and `$0` (final caret); the editor places the caret at the
    /// first tabstop and Tab jumps between them.
    pub insert: String,
    /// The item kind (icon/color).
    pub kind: CompletionKind,
    /// Optional right-aligned detail (a type signature, module path, …).
    pub detail: Option<String>,
}

impl CompletionItem {
    /// A plain item whose inserted text equals its label.
    pub fn new(label: impl Into<String>, kind: CompletionKind) -> Self {
        let label = label.into();
        CompletionItem {
            insert: label.clone(),
            label,
            kind,
            detail: None,
        }
    }
    /// Set a distinct insert text (e.g. a snippet body).
    pub fn insert(mut self, text: impl Into<String>) -> Self {
        self.insert = text.into();
        self
    }
    /// Set the right-aligned detail text.
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

/// What the editor knows when it asks for completions.
pub struct CompletionContext<'a> {
    /// The full document text.
    pub src: &'a str,
    /// The caret byte offset.
    pub cursor: usize,
    /// The identifier prefix immediately before the caret (may be empty).
    pub prefix: &'a str,
}

/// A completion source: given the [`CompletionContext`], return the candidate items. The
/// editor filters them by `prefix` and shows the popup.
pub type CompletionProvider = Rc<dyn Fn(&CompletionContext) -> Vec<CompletionItem>>;

// ---------------------------------------------------------------------------
// Hover / signature help
// ---------------------------------------------------------------------------

/// Hover information for the symbol under the pointer/caret.
#[derive(Clone, Debug)]
pub struct Hover {
    /// Plain-text contents shown in the tooltip.
    pub contents: String,
    /// The byte range the hover applies to (for underline/emphasis), if known.
    pub range: Option<(usize, usize)>,
}

/// A hover source: given `(src, byte)`, return hover info or `None`.
pub type HoverProvider = Rc<dyn Fn(&str, usize) -> Option<Hover>>;

/// Signature help for the call being typed.
#[derive(Clone, Debug)]
pub struct SignatureHelp {
    /// The full signature label, e.g. `fn push(&mut self, value: T)`.
    pub label: String,
    /// The parameter labels, for emphasizing the active one.
    pub params: Vec<String>,
    /// Which parameter (index into `params`) is active, if known.
    pub active: Option<usize>,
}

/// A signature-help source: given `(src, byte)`, return help or `None`.
pub type SignatureProvider = Rc<dyn Fn(&str, usize) -> Option<SignatureHelp>>;

// ---------------------------------------------------------------------------
// Diagnostics / inlay hints (data, not callbacks — pushed via a signal)
// ---------------------------------------------------------------------------

/// The severity of a [`Diagnostic`] — drives the underline color + gutter marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

/// A diagnostic: a byte range flagged with a severity + message (lint/compile output).
#[derive(Clone, Debug)]
pub struct Diagnostic {
    /// The flagged byte range.
    pub range: (usize, usize),
    pub severity: Severity,
    pub message: String,
}

/// A reactive list of diagnostics, updated out-of-band by the app.
pub type Diagnostics = Signal<Vec<Diagnostic>>;

/// An inline hint (parameter name, inferred type) rendered between glyphs at a byte offset.
#[derive(Clone, Debug)]
pub struct InlayHint {
    /// The byte offset the hint is anchored at.
    pub at: usize,
    /// The hint text, e.g. `: i32` or `value:`.
    pub label: String,
}

/// A reactive list of inlay hints, updated out-of-band by the app.
pub type InlayHints = Signal<Vec<InlayHint>>;

// ---------------------------------------------------------------------------
// Definition / formatting
// ---------------------------------------------------------------------------

/// A go-to-definition source: given `(src, byte)`, return the target byte offset (in the same
/// document) to move the caret to, or `None`.
pub type DefinitionProvider = Rc<dyn Fn(&str, usize) -> Option<usize>>;

/// A document formatter: given the full source, return the formatted source. The editor
/// diffs old→new and applies it as one undoable edit.
pub type FormatProvider = Rc<dyn Fn(&str) -> String>;
