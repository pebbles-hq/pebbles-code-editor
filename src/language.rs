//! **Language services**: LSP-shaped data types and an asynchronous [`LanguageService`]
//! trait — the editor side of a Language Server Protocol client.
//!
//! The editor owns the UI (completion popup, hover card, peek views, …); the host (typically
//! an LSP client such as Gravel's) answers requests. Each request carries the document
//! ([`DocRef`]: uri, text, version), an LSP [`Position`]/[`Range`] (UTF-16 code units, as the
//! protocol mandates) and a `reply` callback. A method returns `false` when the request is
//! unsupported; otherwise it calls `reply` exactly once — synchronously or later, but always
//! **on the UI thread** (replies are not `Send`). The editor tags each request with the
//! document version and drops stale replies (see [`RequestGate`]).
//!
//! The older synchronous providers ([`crate::providers`]) keep working: the editor adapts them
//! onto this trait with [`ProviderService`].

use std::cell::Cell;
use std::rc::Rc;

// ---------------------------------------------------------------------------
// Positions (UTF-16, per LSP) and byte conversion
// ---------------------------------------------------------------------------

/// A zero-based line + UTF-16 code-unit column (LSP `Position`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
    /// Zero-based line.
    pub line: u32,
    /// Zero-based column in UTF-16 code units.
    pub character: u32,
}

impl Position {
    /// A position at `line`, `character`.
    pub fn new(line: u32, character: u32) -> Self {
        Position { line, character }
    }
}

/// A half-open range between two [`Position`]s (LSP `Range`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Range {
    /// Inclusive start.
    pub start: Position,
    /// Exclusive end.
    pub end: Position,
}

impl Range {
    /// A range from `start` to `end`.
    pub fn new(start: Position, end: Position) -> Self {
        Range { start, end }
    }
}

/// The byte offset of the start of `line` (clamped to the end of `text`).
fn line_start(text: &str, line: u32) -> usize {
    if line == 0 {
        return 0;
    }
    let mut seen = 0u32;
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            seen += 1;
            if seen == line {
                return i + 1;
            }
        }
    }
    text.len()
}

/// Convert a byte offset (clamped, snapped back to a char boundary) to an LSP [`Position`].
pub fn byte_to_position(text: &str, byte: usize) -> Position {
    let mut byte = byte.min(text.len());
    while !text.is_char_boundary(byte) {
        byte -= 1;
    }
    let ls = text[..byte].rfind('\n').map_or(0, |i| i + 1);
    let line = text[..ls].bytes().filter(|&b| b == b'\n').count() as u32;
    let character = text[ls..byte].chars().map(char::len_utf16).sum::<usize>() as u32;
    Position { line, character }
}

/// Convert an LSP [`Position`] to a byte offset. A line past the end clamps to the end of the
/// text; a column past the end of its line clamps to the line end; a column inside a
/// surrogate pair snaps to the start of that character.
pub fn position_to_byte(text: &str, pos: Position) -> usize {
    let ls = line_start(text, pos.line);
    let le = text[ls..].find('\n').map_or(text.len(), |i| ls + i);
    let mut units = 0u32;
    for (i, c) in text[ls..le].char_indices() {
        let n = c.len_utf16() as u32;
        if units + n > pos.character {
            return ls + i;
        }
        units += n;
    }
    le
}

/// Convert an LSP [`Range`] to a `(from, to)` byte pair (ordered).
pub fn range_to_bytes(text: &str, range: Range) -> (usize, usize) {
    let a = position_to_byte(text, range.start);
    let b = position_to_byte(text, range.end);
    (a.min(b), a.max(b))
}

/// Convert a byte span to an LSP [`Range`].
pub fn bytes_to_range(text: &str, from: usize, to: usize) -> Range {
    Range {
        start: byte_to_position(text, from),
        end: byte_to_position(text, to),
    }
}

// ---------------------------------------------------------------------------
// Core data types
// ---------------------------------------------------------------------------

/// A document the request is about.
#[derive(Clone, Copy, Debug)]
pub struct DocRef<'a> {
    /// The document URI (set with `CodeEditor::document_uri`; `untitled:` when unset).
    pub uri: &'a str,
    /// The full current text.
    pub text: &'a str,
    /// The document version (bumped on every edit).
    pub version: u64,
}

/// A range inside a (possibly other) document (LSP `Location`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    /// The document URI.
    pub uri: String,
    /// The range inside it.
    pub range: Range,
}

/// A textual edit (LSP `TextEdit`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit {
    /// The range replaced.
    pub range: Range,
    /// The replacement text.
    pub new_text: String,
}

impl TextEdit {
    /// An edit replacing `range` with `new_text`.
    pub fn new(range: Range, new_text: impl Into<String>) -> Self {
        TextEdit {
            range,
            new_text: new_text.into(),
        }
    }
}

/// Edits across documents (LSP `WorkspaceEdit`, `changes` form), in document order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkspaceEdit {
    /// `(uri, edits)` per document.
    pub changes: Vec<(String, Vec<TextEdit>)>,
}

impl WorkspaceEdit {
    /// The edits for `uri`, if any.
    pub fn edits_for(&self, uri: &str) -> Option<&[TextEdit]> {
        self.changes
            .iter()
            .find(|(u, _)| u == uri)
            .map(|(_, e)| e.as_slice())
    }
}

/// How [`MarkupContent::value`] is formatted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MarkupKind {
    /// Plain text.
    #[default]
    PlainText,
    /// CommonMark Markdown (rendered by the editor's built-in renderer).
    Markdown,
}

/// Formatted documentation (LSP `MarkupContent`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MarkupContent {
    /// The format of `value`.
    pub kind: MarkupKind,
    /// The content.
    pub value: String,
}

impl MarkupContent {
    /// Plain-text content.
    pub fn plain(value: impl Into<String>) -> Self {
        MarkupContent {
            kind: MarkupKind::PlainText,
            value: value.into(),
        }
    }
    /// Markdown content.
    pub fn markdown(value: impl Into<String>) -> Self {
        MarkupContent {
            kind: MarkupKind::Markdown,
            value: value.into(),
        }
    }
}

/// A command the host executes (LSP `Command`). `arguments` are JSON-encoded values.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LspCommand {
    /// Human-readable title.
    pub title: String,
    /// The command identifier.
    pub command: String,
    /// JSON-encoded arguments.
    pub arguments: Vec<String>,
}

/// Hover information (LSP `Hover`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hover {
    /// The contents.
    pub contents: MarkupContent,
    /// The range the hover applies to.
    pub range: Option<Range>,
}

/// A parameter label: a substring or UTF-16 offsets into the signature label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParameterLabel {
    /// The parameter's text (a substring of the signature label).
    Simple(String),
    /// `[start, end)` UTF-16 offsets into the signature label.
    Offsets(u32, u32),
}

/// One parameter of a signature (LSP `ParameterInformation`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParameterInformation {
    /// The label.
    pub label: ParameterLabel,
    /// Documentation.
    pub documentation: Option<MarkupContent>,
}

/// One signature of a callable (LSP `SignatureInformation`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureInformation {
    /// The full label, e.g. `fn push(&mut self, value: T)`.
    pub label: String,
    /// Documentation.
    pub documentation: Option<MarkupContent>,
    /// The parameters.
    pub parameters: Vec<ParameterInformation>,
    /// The active parameter for this signature (overrides the list-level one).
    pub active_parameter: Option<u32>,
}

/// Signature help (LSP `SignatureHelp`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SignatureHelp {
    /// The overloads.
    pub signatures: Vec<SignatureInformation>,
    /// The active overload.
    pub active_signature: Option<u32>,
    /// The active parameter.
    pub active_parameter: Option<u32>,
}

/// Why signature help was requested.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SignatureHelpTriggerKind {
    /// Invoked manually.
    #[default]
    Invoked,
    /// A trigger character was typed.
    TriggerCharacter,
    /// The cursor moved or the document changed while help was showing.
    ContentChange,
}

/// The context of a signature-help request (LSP `SignatureHelpContext`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SignatureHelpContext {
    /// Why it was requested.
    pub trigger_kind: SignatureHelpTriggerKind,
    /// The trigger character, if any.
    pub trigger_character: Option<String>,
    /// Help was already showing.
    pub is_retrigger: bool,
    /// The help currently showing.
    pub active: Option<SignatureHelp>,
}

/// Diagnostic severity (LSP `DiagnosticSeverity`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    /// Error.
    Error,
    /// Warning.
    Warning,
    /// Information.
    Information,
    /// Hint.
    Hint,
}

/// Extra diagnostic rendering hints (LSP `DiagnosticTag`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiagnosticTag {
    /// Unused / unnecessary code: rendered faded.
    Unnecessary,
    /// Deprecated code: rendered struck through.
    Deprecated,
}

/// A related location for a diagnostic (LSP `DiagnosticRelatedInformation`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticRelatedInformation {
    /// Where.
    pub location: Location,
    /// What.
    pub message: String,
}

/// A diagnostic in LSP coordinates (`textDocument/publishDiagnostics`; also sent in
/// [`CodeActionContext`]). Bind a reactive list with `CodeEditor::lsp_diagnostics`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LspDiagnostic {
    /// The range.
    pub range: Range,
    /// The severity (`None` is shown as an error, as VS Code does).
    pub severity: Option<DiagnosticSeverity>,
    /// The diagnostic code.
    pub code: Option<String>,
    /// A link explaining the code (LSP `codeDescription.href`).
    pub code_href: Option<String>,
    /// The producer (e.g. `rustc`).
    pub source: Option<String>,
    /// The message.
    pub message: String,
    /// Rendering tags (unnecessary = faded, deprecated = strikethrough).
    pub tags: Vec<DiagnosticTag>,
    /// Related locations (shown in the hover and the problem peek).
    pub related_information: Vec<DiagnosticRelatedInformation>,
}

impl LspDiagnostic {
    /// A diagnostic at `range` with `severity` and `message`.
    pub fn new(range: Range, severity: DiagnosticSeverity, message: impl Into<String>) -> Self {
        LspDiagnostic {
            range,
            severity: Some(severity),
            message: message.into(),
            ..Default::default()
        }
    }
    /// The effective severity (missing = error).
    pub fn severity(&self) -> DiagnosticSeverity {
        self.severity.unwrap_or(DiagnosticSeverity::Error)
    }
}

/// Problem counts for the host's status bar / problems panel (see
/// `CodeEditor::problem_counts`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProblemCounts {
    /// Errors.
    pub errors: usize,
    /// Warnings.
    pub warnings: usize,
    /// Informational.
    pub infos: usize,
    /// Hints.
    pub hints: usize,
}

/// Why code actions were requested.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CodeActionTriggerKind {
    /// Explicitly (Ctrl+.).
    #[default]
    Invoked,
    /// Automatically (lightbulb).
    Automatic,
}

/// The context of a code-action request (LSP `CodeActionContext`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodeActionContext {
    /// The diagnostics overlapping the range.
    pub diagnostics: Vec<LspDiagnostic>,
    /// Requested kinds (`quickfix`, `refactor`, `source.organizeImports`, …); empty = all.
    pub only: Vec<String>,
    /// Why it was requested.
    pub trigger_kind: CodeActionTriggerKind,
}

/// A code action (LSP `CodeAction`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodeAction {
    /// The title shown in the menu.
    pub title: String,
    /// The kind (`quickfix`, `refactor.extract`, …).
    pub kind: Option<String>,
    /// The diagnostics it resolves.
    pub diagnostics: Vec<LspDiagnostic>,
    /// The preferred action (auto-fix).
    pub is_preferred: bool,
    /// Why it is disabled, if it is.
    pub disabled: Option<String>,
    /// The edit it applies.
    pub edit: Option<WorkspaceEdit>,
    /// The command it runs (after the edit).
    pub command: Option<LspCommand>,
    /// Opaque data for `resolve_code_action`.
    pub data: Option<String>,
}

/// The kind of a symbol (LSP `SymbolKind`, all 26).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(missing_docs)]
pub enum SymbolKind {
    File,
    Module,
    Namespace,
    Package,
    Class,
    Method,
    Property,
    Field,
    Constructor,
    Enum,
    Interface,
    Function,
    Variable,
    Constant,
    String,
    Number,
    Boolean,
    Array,
    Object,
    Key,
    Null,
    EnumMember,
    Struct,
    Event,
    Operator,
    TypeParameter,
}

/// A hierarchical document symbol (LSP `DocumentSymbol`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentSymbol {
    /// The name.
    pub name: String,
    /// Extra detail (a signature).
    pub detail: Option<String>,
    /// The kind.
    pub kind: SymbolKind,
    /// Deprecated.
    pub deprecated: bool,
    /// The full range (body included).
    pub range: Range,
    /// The name's range.
    pub selection_range: Range,
    /// Nested symbols.
    pub children: Vec<DocumentSymbol>,
}

/// A flat (workspace) symbol (LSP `SymbolInformation`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SymbolInformation {
    /// The name.
    pub name: String,
    /// The kind.
    pub kind: SymbolKind,
    /// Deprecated.
    pub deprecated: bool,
    /// Where it is.
    pub location: Location,
    /// The containing symbol's name.
    pub container_name: Option<String>,
}

impl SymbolKind {
    /// All kinds, in LSP order (`ALL[n - 1]` is LSP kind `n`).
    pub const ALL: [SymbolKind; 26] = {
        use SymbolKind::*;
        [
            File,
            Module,
            Namespace,
            Package,
            Class,
            Method,
            Property,
            Field,
            Constructor,
            Enum,
            Interface,
            Function,
            Variable,
            Constant,
            String,
            Number,
            Boolean,
            Array,
            Object,
            Key,
            Null,
            EnumMember,
            Struct,
            Event,
            Operator,
            TypeParameter,
        ]
    };
    /// The kind for an LSP numeric value (1-based).
    pub fn from_lsp(n: u32) -> Option<Self> {
        (n >= 1).then(|| Self::ALL.get(n as usize - 1).copied())?
    }
    /// The LSP numeric value (1-based).
    pub fn to_lsp(self) -> u32 {
        Self::ALL.iter().position(|k| *k == self).unwrap_or(0) as u32 + 1
    }
    /// The plural category label VS Code groups by in Go to Symbol (`@:`), e.g. `functions`.
    pub fn category(self) -> &'static str {
        use SymbolKind::*;
        match self {
            File => "files",
            Module => "modules",
            Namespace => "namespaces",
            Package => "packages",
            Class => "classes",
            Method => "methods",
            Property => "properties",
            Field => "fields",
            Constructor => "constructors",
            Enum => "enumerations",
            Interface => "interfaces",
            Function => "functions",
            Variable => "variables",
            Constant => "constants",
            String => "strings",
            Number => "numbers",
            Boolean => "booleans",
            Array => "arrays",
            Object => "objects",
            Key => "keys",
            Null => "null",
            EnumMember => "enumeration members",
            Struct => "structs",
            Event => "events",
            Operator => "operators",
            TypeParameter => "type parameters",
        }
    }
}

impl DocumentSymbol {
    /// A symbol with no detail and no children.
    pub fn new(
        name: impl Into<String>,
        kind: SymbolKind,
        range: Range,
        selection_range: Range,
    ) -> Self {
        DocumentSymbol {
            name: name.into(),
            detail: None,
            kind,
            deprecated: false,
            range,
            selection_range,
            children: Vec::new(),
        }
    }
    /// Add a child symbol.
    pub fn child(mut self, c: DocumentSymbol) -> Self {
        self.children.push(c);
        self
    }
}

/// A `textDocument/documentSymbol` reply: hierarchical (`DocumentSymbol[]`) or flat
/// (`SymbolInformation[]`, nested by range containment with [`nest_symbols`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocumentSymbolResponse {
    /// The hierarchical form.
    Nested(Vec<DocumentSymbol>),
    /// The flat form.
    Flat(Vec<SymbolInformation>),
}

impl DocumentSymbolResponse {
    /// The hierarchical outline (flat replies are nested by range containment).
    pub fn into_nested(self) -> Vec<DocumentSymbol> {
        match self {
            DocumentSymbolResponse::Nested(v) => v,
            DocumentSymbolResponse::Flat(v) => nest_symbols(v),
        }
    }
}

/// Nest flat symbols into a hierarchy: each symbol becomes a child of the smallest earlier
/// symbol whose range contains it (the order VS Code's outline uses for flat replies).
pub fn nest_symbols(flat: Vec<SymbolInformation>) -> Vec<DocumentSymbol> {
    let mut v: Vec<SymbolInformation> = flat;
    // Outer ranges first: by start, then longest.
    v.sort_by(|a, b| {
        (
            a.location.range.start,
            std::cmp::Reverse(a.location.range.end),
        )
            .cmp(&(
                b.location.range.start,
                std::cmp::Reverse(b.location.range.end),
            ))
    });
    fn insert(list: &mut Vec<DocumentSymbol>, s: DocumentSymbol) {
        if let Some(last) = list.last_mut()
            && last.range.start <= s.range.start
            && s.range.end <= last.range.end
            && last.range != s.range
        {
            insert(&mut last.children, s);
            return;
        }
        list.push(s);
    }
    let mut out = Vec::new();
    for s in v {
        let mut d = DocumentSymbol::new(s.name, s.kind, s.location.range, s.location.range);
        d.deprecated = s.deprecated;
        d.detail = s.container_name;
        insert(&mut out, d);
    }
    out
}

/// Every symbol in pre-order with its depth.
pub fn flatten_symbols(symbols: &[DocumentSymbol]) -> Vec<(usize, &DocumentSymbol)> {
    fn walk<'a>(v: &'a [DocumentSymbol], d: usize, out: &mut Vec<(usize, &'a DocumentSymbol)>) {
        for s in v {
            out.push((d, s));
            walk(&s.children, d + 1, out);
        }
    }
    let mut out = Vec::new();
    walk(symbols, 0, &mut out);
    out
}

/// The chain of symbols whose range contains `pos`, outermost first (the breadcrumbs).
pub fn symbol_path(symbols: &[DocumentSymbol], pos: Position) -> Vec<&DocumentSymbol> {
    let mut out = Vec::new();
    let mut level = symbols;
    while let Some(s) = level
        .iter()
        .find(|s| s.range.start <= pos && pos <= s.range.end)
    {
        out.push(s);
        level = &s.children;
    }
    out
}

/// A code lens (LSP `CodeLens`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeLens {
    /// The line range it annotates.
    pub range: Range,
    /// The command (unresolved lenses have none).
    pub command: Option<LspCommand>,
    /// Opaque data for `resolve_code_lens`.
    pub data: Option<String>,
}

/// How a highlighted occurrence is used.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DocumentHighlightKind {
    /// A textual occurrence.
    #[default]
    Text,
    /// A read.
    Read,
    /// A write.
    Write,
}

/// An occurrence of the symbol under the caret (LSP `DocumentHighlight`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentHighlight {
    /// The range.
    pub range: Range,
    /// The kind.
    pub kind: DocumentHighlightKind,
}

/// The kind of an inlay hint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlayHintKind {
    /// A type annotation.
    Type,
    /// A parameter name.
    Parameter,
}

/// One part of a composite inlay-hint label (LSP `InlayHintLabelPart`): clicking it reveals
/// `location` or runs `command`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InlayHintLabelPart {
    /// The text.
    pub value: String,
    /// Tooltip for this part.
    pub tooltip: Option<MarkupContent>,
    /// Where clicking goes (e.g. the type's definition).
    pub location: Option<Location>,
    /// The command clicking runs.
    pub command: Option<LspCommand>,
}

impl InlayHintLabelPart {
    /// A plain part.
    pub fn new(value: impl Into<String>) -> Self {
        InlayHintLabelPart {
            value: value.into(),
            ..Default::default()
        }
    }
}

/// An inlay hint (LSP `InlayHint`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlayHint {
    /// Where it is shown.
    pub position: Position,
    /// The label (used when `label_parts` is empty).
    pub label: String,
    /// A composite label with clickable parts (LSP `InlayHintLabelPart[]`).
    pub label_parts: Vec<InlayHintLabelPart>,
    /// The kind.
    pub kind: Option<InlayHintKind>,
    /// Pad before.
    pub padding_left: bool,
    /// Pad after.
    pub padding_right: bool,
    /// Tooltip.
    pub tooltip: Option<MarkupContent>,
    /// Opaque data for `resolve_inlay_hint`.
    pub data: Option<String>,
}

impl InlayHint {
    /// A hint with a plain label.
    pub fn new(position: Position, label: impl Into<String>, kind: Option<InlayHintKind>) -> Self {
        InlayHint {
            position,
            label: label.into(),
            label_parts: Vec::new(),
            kind,
            padding_left: false,
            padding_right: false,
            tooltip: None,
            data: None,
        }
    }
    /// The label parts (a plain label as one part).
    pub fn parts(&self) -> Vec<InlayHintLabelPart> {
        if self.label_parts.is_empty() {
            vec![InlayHintLabelPart::new(self.label.clone())]
        } else {
            self.label_parts.clone()
        }
    }
}

/// An item of a call or type hierarchy (LSP `CallHierarchyItem` / `TypeHierarchyItem`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallHierarchyItem {
    /// The name.
    pub name: String,
    /// The kind.
    pub kind: SymbolKind,
    /// Extra detail.
    pub detail: Option<String>,
    /// The document.
    pub uri: String,
    /// The full range.
    pub range: Range,
    /// The name's range.
    pub selection_range: Range,
    /// Opaque data.
    pub data: Option<String>,
}

/// A type-hierarchy item (same shape as [`CallHierarchyItem`]).
pub type TypeHierarchyItem = CallHierarchyItem;

/// An incoming call (LSP `CallHierarchyIncomingCall`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallHierarchyIncomingCall {
    /// The caller.
    pub from: CallHierarchyItem,
    /// The call sites inside the caller.
    pub from_ranges: Vec<Range>,
}

/// An outgoing call (LSP `CallHierarchyOutgoingCall`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallHierarchyOutgoingCall {
    /// The callee.
    pub to: CallHierarchyItem,
    /// The call sites inside the caller.
    pub from_ranges: Vec<Range>,
}

/// Formatting options (LSP `FormattingOptions`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormattingOptions {
    /// Spaces per tab.
    pub tab_size: u32,
    /// Indent with spaces.
    pub insert_spaces: bool,
}

// ---------------------------------------------------------------------------
// Completion
// ---------------------------------------------------------------------------

/// The kind of a completion item (LSP `CompletionItemKind`, all 25).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(missing_docs)]
pub enum CompletionItemKind {
    Text,
    Method,
    Function,
    Constructor,
    Field,
    Variable,
    Class,
    Interface,
    Module,
    Property,
    Unit,
    Value,
    Enum,
    Keyword,
    Snippet,
    Color,
    File,
    Reference,
    Folder,
    EnumMember,
    Constant,
    Struct,
    Event,
    Operator,
    TypeParameter,
}

impl CompletionItemKind {
    /// All kinds, in LSP order (`ALL[n - 1]` is LSP kind `n`).
    pub const ALL: [CompletionItemKind; 25] = {
        use CompletionItemKind::*;
        [
            Text,
            Method,
            Function,
            Constructor,
            Field,
            Variable,
            Class,
            Interface,
            Module,
            Property,
            Unit,
            Value,
            Enum,
            Keyword,
            Snippet,
            Color,
            File,
            Reference,
            Folder,
            EnumMember,
            Constant,
            Struct,
            Event,
            Operator,
            TypeParameter,
        ]
    };
    /// The kind for an LSP numeric value (1-based).
    pub fn from_lsp(n: u32) -> Option<Self> {
        (n >= 1).then(|| Self::ALL.get(n as usize - 1).copied())?
    }
    /// The LSP numeric value (1-based).
    pub fn to_lsp(self) -> u32 {
        Self::ALL.iter().position(|k| *k == self).unwrap_or(0) as u32 + 1
    }
}

/// How `insert_text` / `text_edit.new_text` is interpreted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InsertTextFormat {
    /// Literal text.
    #[default]
    PlainText,
    /// LSP snippet syntax (`$1`, `${1:placeholder}`, `${1|a,b|}`, `$0`, variables).
    Snippet,
}

/// The edit a completion applies (LSP `TextEdit | InsertReplaceEdit`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompletionTextEdit {
    /// Replace `range` with `new_text`.
    Edit(TextEdit),
    /// Insert-or-replace: the editor uses `replace` (VS Code `editor.suggest.insertMode:
    /// replace` is the LSP-recommended default for accept).
    InsertReplace {
        /// The text.
        new_text: String,
        /// The range used when inserting.
        insert: Range,
        /// The range used when replacing.
        replace: Range,
    },
}

/// Extra label parts (LSP `CompletionItemLabelDetails`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompletionItemLabelDetails {
    /// Shown right after the label, without spacing (e.g. a signature `(a, b)`).
    pub detail: Option<String>,
    /// Shown right-aligned (e.g. a module path).
    pub description: Option<String>,
}

/// A completion candidate (LSP `CompletionItem`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionItem {
    /// The label (also the default insert and filter text).
    pub label: String,
    /// Extra label parts.
    pub label_details: Option<CompletionItemLabelDetails>,
    /// The kind (icon).
    pub kind: Option<CompletionItemKind>,
    /// Deprecated (rendered struck through).
    pub deprecated: bool,
    /// Detail line (a type or signature) shown in the docs pane and right of the label.
    pub detail: Option<String>,
    /// Documentation shown in the side pane.
    pub documentation: Option<MarkupContent>,
    /// Select this item first.
    pub preselect: bool,
    /// Sort key among equally scored items (default: the label).
    pub sort_text: Option<String>,
    /// Filter key matched against the typed word (default: the label).
    pub filter_text: Option<String>,
    /// Inserted text (default: the label). Ignored when `text_edit` is set.
    pub insert_text: Option<String>,
    /// How the inserted text is interpreted.
    pub insert_text_format: InsertTextFormat,
    /// The primary edit (its range must be on the caret line and contain the caret).
    pub text_edit: Option<CompletionTextEdit>,
    /// Extra edits elsewhere (e.g. an auto-import), applied in the same undo step.
    pub additional_text_edits: Vec<TextEdit>,
    /// Typing one of these accepts the item and then types the character.
    pub commit_characters: Vec<String>,
    /// A command run after accepting.
    pub command: Option<LspCommand>,
    /// Opaque data for `resolve_completion`.
    pub data: Option<String>,
}

impl CompletionItem {
    /// An item with `label` and `kind`, everything else default.
    pub fn new(label: impl Into<String>, kind: CompletionItemKind) -> Self {
        CompletionItem {
            label: label.into(),
            label_details: None,
            kind: Some(kind),
            deprecated: false,
            detail: None,
            documentation: None,
            preselect: false,
            sort_text: None,
            filter_text: None,
            insert_text: None,
            insert_text_format: InsertTextFormat::PlainText,
            text_edit: None,
            additional_text_edits: Vec::new(),
            commit_characters: Vec::new(),
            command: None,
            data: None,
        }
    }
    /// Set the inserted text.
    pub fn insert_text(mut self, text: impl Into<String>) -> Self {
        self.insert_text = Some(text.into());
        self
    }
    /// Set the inserted text as an LSP snippet.
    pub fn snippet(mut self, body: impl Into<String>) -> Self {
        self.insert_text = Some(body.into());
        self.insert_text_format = InsertTextFormat::Snippet;
        self
    }
    /// Set the detail line.
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
    /// Set Markdown documentation.
    pub fn documentation(mut self, md: impl Into<String>) -> Self {
        self.documentation = Some(MarkupContent::markdown(md));
        self
    }
    /// The text the filter matches against.
    pub fn filter_key(&self) -> &str {
        self.filter_text.as_deref().unwrap_or(&self.label)
    }
    /// The key used to order equally scored items.
    pub fn sort_key(&self) -> &str {
        self.sort_text.as_deref().unwrap_or(&self.label)
    }
    /// The text to insert (before snippet expansion).
    pub fn new_text(&self) -> &str {
        match &self.text_edit {
            Some(CompletionTextEdit::Edit(e)) => &e.new_text,
            Some(CompletionTextEdit::InsertReplace { new_text, .. }) => new_text,
            None => self.insert_text.as_deref().unwrap_or(&self.label),
        }
    }
}

/// A completion result (LSP `CompletionList`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompletionList {
    /// More typing should re-query the server (the list is not final).
    pub is_incomplete: bool,
    /// The items.
    pub items: Vec<CompletionItem>,
}

/// Why completion was requested (LSP `CompletionTriggerKind`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CompletionTriggerKind {
    /// Typing an identifier or Ctrl+Space.
    #[default]
    Invoked,
    /// A trigger character (e.g. `.`) was typed.
    TriggerCharacter,
    /// Re-query of an incomplete list.
    TriggerForIncompleteCompletions,
}

/// The context of a completion request (LSP `CompletionContext`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompletionContext {
    /// Why it was requested.
    pub trigger_kind: CompletionTriggerKind,
    /// The trigger character, if any.
    pub trigger_character: Option<String>,
}

// ---------------------------------------------------------------------------
// The service trait
// ---------------------------------------------------------------------------

/// A reply callback. Call it exactly once, on the UI thread.
pub type Reply<T> = Box<dyn FnOnce(T)>;

/// The language intelligence behind one editor (one method per LSP request). Every request
/// method returns `false` when unsupported (the default) and otherwise calls `reply` exactly
/// once on the UI thread. Positions are UTF-16 ([`Position`]); use [`position_to_byte`] /
/// [`byte_to_position`] to convert.
#[allow(unused_variables)]
pub trait LanguageService {
    /// Characters that open completion when typed (server `triggerCharacters`, e.g. `.`).
    fn completion_trigger_characters(&self) -> Vec<String> {
        Vec::new()
    }
    /// Characters that open signature help (default `(` and `,`).
    fn signature_trigger_characters(&self) -> Vec<String> {
        vec!["(".into(), ",".into()]
    }
    /// Characters that trigger on-type formatting.
    fn on_type_formatting_trigger_characters(&self) -> Vec<String> {
        Vec::new()
    }

    /// `textDocument/completion`.
    fn completion(
        &self,
        doc: &DocRef,
        pos: Position,
        cx: CompletionContext,
        reply: Reply<Option<CompletionList>>,
    ) -> bool {
        false
    }
    /// `completionItem/resolve` (fill in documentation/detail lazily).
    fn resolve_completion(&self, item: CompletionItem, reply: Reply<CompletionItem>) -> bool {
        false
    }
    /// `textDocument/hover`.
    fn hover(&self, doc: &DocRef, pos: Position, reply: Reply<Option<Hover>>) -> bool {
        false
    }
    /// `textDocument/signatureHelp`.
    fn signature_help(
        &self,
        doc: &DocRef,
        pos: Position,
        cx: SignatureHelpContext,
        reply: Reply<Option<SignatureHelp>>,
    ) -> bool {
        false
    }
    /// `textDocument/definition`.
    fn definition(&self, doc: &DocRef, pos: Position, reply: Reply<Vec<Location>>) -> bool {
        false
    }
    /// `textDocument/declaration`.
    fn declaration(&self, doc: &DocRef, pos: Position, reply: Reply<Vec<Location>>) -> bool {
        false
    }
    /// `textDocument/typeDefinition`.
    fn type_definition(&self, doc: &DocRef, pos: Position, reply: Reply<Vec<Location>>) -> bool {
        false
    }
    /// `textDocument/implementation`.
    fn implementation(&self, doc: &DocRef, pos: Position, reply: Reply<Vec<Location>>) -> bool {
        false
    }
    /// `textDocument/references`.
    fn references(
        &self,
        doc: &DocRef,
        pos: Position,
        include_declaration: bool,
        reply: Reply<Vec<Location>>,
    ) -> bool {
        false
    }
    /// `textDocument/documentHighlight`.
    fn document_highlight(
        &self,
        doc: &DocRef,
        pos: Position,
        reply: Reply<Vec<DocumentHighlight>>,
    ) -> bool {
        false
    }
    /// `textDocument/documentSymbol`.
    fn document_symbols(&self, doc: &DocRef, reply: Reply<DocumentSymbolResponse>) -> bool {
        false
    }
    /// `workspace/symbol`.
    fn workspace_symbols(&self, query: &str, reply: Reply<Vec<SymbolInformation>>) -> bool {
        false
    }
    /// `textDocument/codeAction`.
    fn code_actions(
        &self,
        doc: &DocRef,
        range: Range,
        cx: CodeActionContext,
        reply: Reply<Vec<CodeAction>>,
    ) -> bool {
        false
    }
    /// `codeAction/resolve`.
    fn resolve_code_action(&self, action: CodeAction, reply: Reply<CodeAction>) -> bool {
        false
    }
    /// `textDocument/codeLens`.
    fn code_lens(&self, doc: &DocRef, reply: Reply<Vec<CodeLens>>) -> bool {
        false
    }
    /// `codeLens/resolve`.
    fn resolve_code_lens(&self, lens: CodeLens, reply: Reply<CodeLens>) -> bool {
        false
    }
    /// `textDocument/formatting`.
    fn formatting(
        &self,
        doc: &DocRef,
        options: FormattingOptions,
        reply: Reply<Vec<TextEdit>>,
    ) -> bool {
        false
    }
    /// `textDocument/rangeFormatting`.
    fn range_formatting(
        &self,
        doc: &DocRef,
        range: Range,
        options: FormattingOptions,
        reply: Reply<Vec<TextEdit>>,
    ) -> bool {
        false
    }
    /// `textDocument/onTypeFormatting`.
    fn on_type_formatting(
        &self,
        doc: &DocRef,
        pos: Position,
        ch: &str,
        options: FormattingOptions,
        reply: Reply<Vec<TextEdit>>,
    ) -> bool {
        false
    }
    /// `textDocument/prepareRename`: the range to rename, or `None` if not renameable.
    fn prepare_rename(&self, doc: &DocRef, pos: Position, reply: Reply<Option<Range>>) -> bool {
        false
    }
    /// `textDocument/rename`.
    fn rename(
        &self,
        doc: &DocRef,
        pos: Position,
        new_name: &str,
        reply: Reply<Option<WorkspaceEdit>>,
    ) -> bool {
        false
    }
    /// `textDocument/inlayHint`.
    fn inlay_hints(&self, doc: &DocRef, range: Range, reply: Reply<Vec<InlayHint>>) -> bool {
        false
    }
    /// `inlayHint/resolve` (fill in tooltip / label-part locations lazily).
    fn resolve_inlay_hint(&self, hint: InlayHint, reply: Reply<InlayHint>) -> bool {
        false
    }
    /// `textDocument/prepareCallHierarchy`.
    fn prepare_call_hierarchy(
        &self,
        doc: &DocRef,
        pos: Position,
        reply: Reply<Vec<CallHierarchyItem>>,
    ) -> bool {
        false
    }
    /// `callHierarchy/incomingCalls`.
    fn incoming_calls(
        &self,
        item: &CallHierarchyItem,
        reply: Reply<Vec<CallHierarchyIncomingCall>>,
    ) -> bool {
        false
    }
    /// `callHierarchy/outgoingCalls`.
    fn outgoing_calls(
        &self,
        item: &CallHierarchyItem,
        reply: Reply<Vec<CallHierarchyOutgoingCall>>,
    ) -> bool {
        false
    }
    /// `textDocument/prepareTypeHierarchy`.
    fn prepare_type_hierarchy(
        &self,
        doc: &DocRef,
        pos: Position,
        reply: Reply<Vec<TypeHierarchyItem>>,
    ) -> bool {
        false
    }
    /// `typeHierarchy/supertypes`.
    fn supertypes(&self, item: &TypeHierarchyItem, reply: Reply<Vec<TypeHierarchyItem>>) -> bool {
        false
    }
    /// `typeHierarchy/subtypes`.
    fn subtypes(&self, item: &TypeHierarchyItem, reply: Reply<Vec<TypeHierarchyItem>>) -> bool {
        false
    }

    // -- host hooks ---------------------------------------------------------

    /// Open another document at `loc` (cross-file navigation). Return `false` if the host
    /// cannot (the editor then ignores locations outside its own document).
    fn open_location(&self, loc: &Location) -> bool {
        false
    }
    /// Show a references list in the host UI (Shift+Alt+F12, VS Code's references view).
    /// Return `false` to let the editor show them in its inline peek instead.
    fn show_references(&self, origin: &Location, locations: &[Location]) -> bool {
        false
    }
    /// Read another document's text (for peek views). `None` when unavailable.
    fn read_text(&self, uri: &str) -> Option<String> {
        None
    }
    /// Apply the parts of a workspace edit that touch *other* documents (the editor applies
    /// its own document's edits itself). Return `false` when unsupported.
    fn apply_workspace_edit(&self, edit: &WorkspaceEdit) -> bool {
        false
    }
    /// Execute a command (`workspace/executeCommand`, or a client-side command).
    fn execute_command(&self, command: &LspCommand) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Stale-reply gating
// ---------------------------------------------------------------------------

/// Drops stale replies: a reply is delivered only if no newer request went through the same
/// gate **and** the document version is unchanged since the request was sent. Cheap to
/// clone (shared counters).
#[derive(Clone, Default)]
pub struct RequestGate {
    latest: Rc<Cell<u64>>,
    version: Rc<Cell<u64>>,
}

impl RequestGate {
    /// A fresh gate.
    pub fn new() -> Self {
        Self::default()
    }
    /// The current document version.
    pub fn version(&self) -> u64 {
        self.version.get()
    }
    /// Note that the document changed (invalidates in-flight requests).
    pub fn bump_version(&self) {
        self.version.set(self.version.get() + 1);
    }
    /// Invalidate in-flight requests without a document change (e.g. the popup closed).
    pub fn cancel(&self) {
        self.latest.set(self.latest.get() + 1);
    }
    /// Start a request: wrap `f` so it runs only if the reply is still current.
    pub fn wrap<T: 'static>(&self, f: impl FnOnce(T) + 'static) -> Reply<T> {
        let id = self.latest.get() + 1;
        self.latest.set(id);
        let at = self.version.get();
        let (latest, version) = (self.latest.clone(), self.version.clone());
        Box::new(move |v: T| {
            if latest.get() == id && version.get() == at {
                f(v);
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Adapter: the old synchronous providers as a LanguageService
// ---------------------------------------------------------------------------

/// The pre-LSP synchronous providers ([`crate::providers`]) adapted onto
/// [`LanguageService`] (byte offsets ↔ UTF-16 positions, plain items → LSP items).
#[derive(Clone, Default)]
pub struct ProviderService {
    /// `CodeEditor::completion`.
    pub completion: Option<crate::providers::CompletionProvider>,
    /// `CodeEditor::hover`.
    pub hover: Option<crate::providers::HoverProvider>,
    /// `CodeEditor::signature_help`.
    pub signature: Option<crate::providers::SignatureProvider>,
    /// `CodeEditor::definition` (same document only).
    pub definition: Option<crate::providers::DefinitionProvider>,
    /// `CodeEditor::format` (whole document).
    pub format: Option<crate::providers::FormatProvider>,
}

impl From<crate::providers::CompletionKind> for CompletionItemKind {
    fn from(k: crate::providers::CompletionKind) -> Self {
        use crate::providers::CompletionKind as K;
        match k {
            K::Keyword => Self::Keyword,
            K::Function => Self::Function,
            K::Method => Self::Method,
            K::Variable => Self::Variable,
            K::Field => Self::Field,
            K::Type => Self::Class,
            K::Module => Self::Module,
            K::Constant => Self::Constant,
            K::Snippet => Self::Snippet,
            K::Text => Self::Text,
        }
    }
}

impl From<crate::providers::CompletionItem> for CompletionItem {
    fn from(it: crate::providers::CompletionItem) -> Self {
        let snippet = it.kind == crate::providers::CompletionKind::Snippet;
        let mut out = CompletionItem::new(it.label.clone(), it.kind.into());
        out.detail = it.detail;
        if it.insert != it.label || snippet {
            out.insert_text = Some(it.insert);
        }
        if snippet {
            out.insert_text_format = InsertTextFormat::Snippet;
        }
        out
    }
}

impl LanguageService for ProviderService {
    fn completion(
        &self,
        doc: &DocRef,
        pos: Position,
        _cx: CompletionContext,
        reply: Reply<Option<CompletionList>>,
    ) -> bool {
        let Some(p) = &self.completion else {
            return false;
        };
        let cursor = position_to_byte(doc.text, pos);
        let start = crate::view::completion_prefix_start(doc.text, cursor);
        let items = p(&crate::providers::CompletionContext {
            src: doc.text,
            cursor,
            prefix: &doc.text[start..cursor],
        });
        reply(Some(CompletionList {
            is_incomplete: false,
            items: items.into_iter().map(Into::into).collect(),
        }));
        true
    }
    fn hover(&self, doc: &DocRef, pos: Position, reply: Reply<Option<Hover>>) -> bool {
        let Some(p) = &self.hover else { return false };
        let h = p(doc.text, position_to_byte(doc.text, pos)).map(|h| Hover {
            contents: MarkupContent::plain(h.contents),
            range: h.range.map(|(a, b)| bytes_to_range(doc.text, a, b)),
        });
        reply(h);
        true
    }
    fn signature_help(
        &self,
        doc: &DocRef,
        pos: Position,
        _cx: SignatureHelpContext,
        reply: Reply<Option<SignatureHelp>>,
    ) -> bool {
        let Some(p) = &self.signature else {
            return false;
        };
        let h = p(doc.text, position_to_byte(doc.text, pos)).map(|s| SignatureHelp {
            signatures: vec![SignatureInformation {
                label: s.label,
                documentation: None,
                parameters: s
                    .params
                    .into_iter()
                    .map(|l| ParameterInformation {
                        label: ParameterLabel::Simple(l),
                        documentation: None,
                    })
                    .collect(),
                active_parameter: None,
            }],
            active_signature: Some(0),
            active_parameter: s.active.map(|a| a as u32),
        });
        reply(h);
        true
    }
    fn definition(&self, doc: &DocRef, pos: Position, reply: Reply<Vec<Location>>) -> bool {
        let Some(p) = &self.definition else {
            return false;
        };
        let locs = p(doc.text, position_to_byte(doc.text, pos))
            .map(|t| {
                let at = byte_to_position(doc.text, t);
                Location {
                    uri: doc.uri.to_string(),
                    range: Range::new(at, at),
                }
            })
            .into_iter()
            .collect();
        reply(locs);
        true
    }
    fn formatting(
        &self,
        doc: &DocRef,
        _options: FormattingOptions,
        reply: Reply<Vec<TextEdit>>,
    ) -> bool {
        let Some(p) = &self.format else { return false };
        let new = p(doc.text);
        let edits = crate::collab::diff(doc.text, &new)
            .map(|e| TextEdit::new(bytes_to_range(doc.text, e.from, e.to), e.insert))
            .into_iter()
            .collect();
        reply(edits);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn utf16_positions_round_trip_ascii_cjk_and_emoji() {
        // 'é' is 2 bytes / 1 unit; '漢' 3 bytes / 1 unit; '😀' 4 bytes / 2 units (surrogates).
        let text = "ab\né漢😀x\n";
        assert_eq!(byte_to_position(text, 0), Position::new(0, 0));
        assert_eq!(byte_to_position(text, 2), Position::new(0, 2));
        let l1 = 3;
        assert_eq!(byte_to_position(text, l1), Position::new(1, 0));
        assert_eq!(byte_to_position(text, l1 + 2), Position::new(1, 1)); // after é
        assert_eq!(byte_to_position(text, l1 + 5), Position::new(1, 2)); // after 漢
        assert_eq!(byte_to_position(text, l1 + 9), Position::new(1, 4)); // after 😀
        assert_eq!(byte_to_position(text, l1 + 10), Position::new(1, 5)); // after x
        for b in [0, 2, l1, l1 + 2, l1 + 5, l1 + 9, l1 + 10, text.len()] {
            assert_eq!(position_to_byte(text, byte_to_position(text, b)), b, "{b}");
        }
        // A column inside the surrogate pair snaps to the emoji's start.
        assert_eq!(position_to_byte(text, Position::new(1, 3)), l1 + 5);
        // Past the line end clamps to the line end; past the last line to the text end.
        assert_eq!(position_to_byte(text, Position::new(0, 99)), 2);
        assert_eq!(position_to_byte(text, Position::new(9, 0)), text.len());
        // A byte inside a multi-byte char snaps back to its start.
        assert_eq!(byte_to_position(text, l1 + 1), Position::new(1, 0));
        let r = bytes_to_range(text, l1 + 2, l1 + 9);
        assert_eq!(range_to_bytes(text, r), (l1 + 2, l1 + 9));
    }

    #[test]
    fn stale_replies_are_dropped() {
        let gate = RequestGate::new();
        let got = Rc::new(RefCell::new(Vec::new()));
        let g = got.clone();
        let first = gate.wrap(move |v: u32| g.borrow_mut().push(v));
        let g = got.clone();
        let second = gate.wrap(move |v: u32| g.borrow_mut().push(v));
        first(1); // superseded by `second`
        second(2);
        assert_eq!(*got.borrow(), vec![2]);
        // A document change invalidates an in-flight request.
        let g = got.clone();
        let third = gate.wrap(move |v: u32| g.borrow_mut().push(v));
        gate.bump_version();
        third(3);
        // Cancel invalidates too.
        let g = got.clone();
        let fourth = gate.wrap(move |v: u32| g.borrow_mut().push(v));
        gate.cancel();
        fourth(4);
        assert_eq!(*got.borrow(), vec![2]);
    }

    #[test]
    fn old_providers_adapt_onto_the_service() {
        use crate::providers as p;
        let svc = ProviderService {
            completion: Some(Rc::new(|cx: &p::CompletionContext| {
                vec![
                    p::CompletionItem::new(format!("{}x", cx.prefix), p::CompletionKind::Function),
                    p::CompletionItem::new("fn", p::CompletionKind::Snippet).insert("fn $1() {}"),
                ]
            })),
            definition: Some(Rc::new(|_: &str, _| Some(4))),
            ..Default::default()
        };
        let doc = DocRef {
            uri: "file:///a.rs",
            text: "😀 ab",
            version: 0,
        };
        let out = Rc::new(RefCell::new(None));
        let o = out.clone();
        assert!(svc.completion(
            &doc,
            Position::new(0, 5),
            CompletionContext::default(),
            Box::new(move |l| *o.borrow_mut() = l),
        ));
        let list = out.borrow().clone().unwrap();
        assert_eq!(list.items[0].label, "abx");
        assert_eq!(list.items[0].kind, Some(CompletionItemKind::Function));
        assert_eq!(list.items[1].insert_text_format, InsertTextFormat::Snippet);
        let locs = Rc::new(RefCell::new(Vec::new()));
        let l = locs.clone();
        assert!(svc.definition(
            &doc,
            Position::new(0, 0),
            Box::new(move |v| *l.borrow_mut() = v)
        ));
        assert_eq!(locs.borrow()[0].range.start, Position::new(0, 2)); // byte 4 = after 😀
        assert!(!svc.hover(&doc, Position::new(0, 0), Box::new(|_| {})));
        assert_eq!(
            CompletionItemKind::from_lsp(25),
            Some(CompletionItemKind::TypeParameter)
        );
        assert_eq!(CompletionItemKind::Snippet.to_lsp(), 15);
    }
}
