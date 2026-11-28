//! Incremental, parser-based highlighting on [tree-sitter](https://tree-sitter.github.io).
//!
//! A [`TreeSitter`] language is a [`Language`] whose [`session`](Language::session) is a
//! stateful [`SyntaxSession`]: it keeps the last parse tree and, on every text change,
//! re-parses **incrementally** (the edit is derived from the old/new text, the tree is
//! `edit`ed, and the parser reuses every untouched subtree). Highlighting runs the grammar's
//! `highlights.scm` query over a **byte range only** — the view passes the visible window —
//! so the cost per frame is proportional to the screen, not the file.
//!
//! Embedded languages are highlighted through the grammar's `injections.scm` (fenced code in
//! Markdown, `<script>`/`<style>` in HTML, Markdown inline spans, …): each injected region is
//! parsed with the target grammar (restricted to that region) and painted over the host.
//!
//! Capture names map to [`TokenKind`]s (see [`kind_for_capture`]); the colors themselves come
//! from the editor theme's [`HighlightStyle`](crate::HighlightStyle), so every color stays
//! themeable. Grammars whose query fails to compile fall back to the hand-written scanner for
//! that language (see [`by_name`](super::by_name)).

use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Mutex, OnceLock};

use tree_sitter::{
    InputEdit, Parser, Point, Query, QueryCursor, Range as TsRange, StreamingIterator, Tree,
};

use super::scope::{ScopeId, intern_scope};
use super::{Language, SyntaxSession, Token, TokenKind};

/// Map a tree-sitter highlight capture name (`keyword`, `function.method`, `string.escape`,
/// `markup.heading`, …) to the editor's [`TokenKind`]. `None` = leave uncolored.
pub fn kind_for_capture(name: &str) -> Option<TokenKind> {
    use TokenKind as K;
    let full = name;
    let head = name.split('.').next().unwrap_or(name);
    Some(match head {
        _ if full.starts_with('_') || full.starts_with("injection") => return None,
        "keyword" | "conditional" | "repeat" | "include" | "exception" | "storageclass"
        | "preproc" | "define" => K::Keyword,
        "variable" if full == "variable.builtin" => K::Keyword,
        "variable" if full == "variable.member" => K::Property,
        "variable" => return None,
        "type" | "constructor" => K::Type,
        "string" if full == "string.escape" || full == "string.special.regex" => K::Escape,
        "string" | "character" => K::Str,
        "escape" => K::Escape,
        "number" | "float" => K::Number,
        "boolean" => K::Constant,
        "comment" => K::Comment,
        "function" if full == "function.macro" => K::Macro,
        "function" | "method" => K::Function,
        "macro" => K::Macro,
        "attribute" | "decorator" | "annotation" => K::Attribute,
        "constant" => K::Constant,
        "property" | "field" => K::Property,
        "punctuation" | "operator" | "delimiter" => K::Punctuation,
        "tag" => K::Tag,
        "label" => K::Attribute,
        "module" | "namespace" => K::Namespace,
        "markup" | "text" => match full {
            "markup.heading" | "text.title" => K::Heading,
            "markup.link" | "markup.link.url" | "markup.link.label" | "text.uri"
            | "text.reference" => K::Link,
            "markup.italic"
            | "markup.bold"
            | "markup.strong"
            | "text.emphasis"
            | "text.strong"
            | "markup.strikethrough"
            | "text.strike" => K::Emphasis,
            "markup.raw" | "markup.raw.block" | "text.literal" => K::Str,
            "markup.quote" => K::Comment,
            "markup.list" => K::Punctuation,
            _ if full.starts_with("markup.heading") => K::Heading,
            _ => return None,
        },
        "embedded" | "none" | "spell" | "conceal" | "parameter" => return None,
        _ => return None,
    })
}

/// A compiled grammar: the parser language + its highlight / injection queries + editing
/// metadata. Built once per thread and shared.
pub struct GrammarDef {
    /// Canonical name (`"rust"`, `"typescript"`, …).
    pub id: &'static str,
    /// Display name (`"Rust"`, `"TypeScript"`, …).
    pub display: &'static str,
    language: tree_sitter::Language,
    highlights: Query,
    /// Capture index → token kind (precomputed from the query's capture names).
    kinds: Vec<Option<TokenKind>>,
    /// Capture index → TextMate scope (without the language suffix).
    scopes: Vec<Option<&'static str>>,
    /// The language's root scope (`source.rust`).
    root_scope: String,
    injections: Option<Query>,
    line_comment: Option<&'static str>,
    block_comment: Option<(&'static str, &'static str)>,
}

impl std::fmt::Debug for GrammarDef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GrammarDef").field("id", &self.id).finish()
    }
}

/// Static description of a bundled grammar (before query compilation).
struct Spec {
    id: &'static str,
    display: &'static str,
    language: fn() -> tree_sitter::Language,
    highlights: &'static [&'static str],
    injections: &'static str,
    line_comment: Option<&'static str>,
    block_comment: Option<(&'static str, &'static str)>,
}

const C_BLOCK: Option<(&str, &str)> = Some(("/*", "*/"));

/// Extra Markdown block injections: tree-sitter-md's query names fenced languages from the
/// info string; this adds nothing beyond it but keeps the inline grammar injected.
fn specs() -> Vec<Spec> {
    vec![
        Spec {
            id: "rust",
            display: "Rust",
            language: || tree_sitter_rust::LANGUAGE.into(),
            highlights: &[tree_sitter_rust::HIGHLIGHTS_QUERY],
            injections: tree_sitter_rust::INJECTIONS_QUERY,
            line_comment: Some("//"),
            block_comment: C_BLOCK,
        },
        Spec {
            id: "toml",
            display: "TOML",
            language: || tree_sitter_toml_ng::LANGUAGE.into(),
            highlights: &[tree_sitter_toml_ng::HIGHLIGHTS_QUERY],
            injections: "",
            line_comment: Some("#"),
            block_comment: None,
        },
        Spec {
            id: "json",
            display: "JSON",
            language: || tree_sitter_json::LANGUAGE.into(),
            highlights: &[tree_sitter_json::HIGHLIGHTS_QUERY],
            injections: "",
            line_comment: None,
            block_comment: None,
        },
        Spec {
            id: "markdown",
            display: "Markdown",
            language: || tree_sitter_md::LANGUAGE.into(),
            highlights: &[tree_sitter_md::HIGHLIGHT_QUERY_BLOCK],
            injections: tree_sitter_md::INJECTION_QUERY_BLOCK,
            line_comment: None,
            block_comment: Some(("<!--", "-->")),
        },
        Spec {
            id: "markdown_inline",
            display: "Markdown (inline)",
            language: || tree_sitter_md::INLINE_LANGUAGE.into(),
            highlights: &[tree_sitter_md::HIGHLIGHT_QUERY_INLINE],
            injections: tree_sitter_md::INJECTION_QUERY_INLINE,
            line_comment: None,
            block_comment: None,
        },
        Spec {
            id: "javascript",
            display: "JavaScript",
            language: || tree_sitter_javascript::LANGUAGE.into(),
            highlights: &[
                tree_sitter_javascript::HIGHLIGHT_QUERY,
                tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
            ],
            injections: tree_sitter_javascript::INJECTIONS_QUERY,
            line_comment: Some("//"),
            block_comment: C_BLOCK,
        },
        Spec {
            id: "typescript",
            display: "TypeScript",
            language: || tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            highlights: &[
                tree_sitter_typescript::HIGHLIGHTS_QUERY,
                tree_sitter_javascript::HIGHLIGHT_QUERY,
            ],
            injections: tree_sitter_javascript::INJECTIONS_QUERY,
            line_comment: Some("//"),
            block_comment: C_BLOCK,
        },
        Spec {
            id: "tsx",
            display: "TSX",
            language: || tree_sitter_typescript::LANGUAGE_TSX.into(),
            highlights: &[
                tree_sitter_typescript::HIGHLIGHTS_QUERY,
                tree_sitter_javascript::HIGHLIGHT_QUERY,
                tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
            ],
            injections: tree_sitter_javascript::INJECTIONS_QUERY,
            line_comment: Some("//"),
            block_comment: C_BLOCK,
        },
        Spec {
            id: "python",
            display: "Python",
            language: || tree_sitter_python::LANGUAGE.into(),
            highlights: &[tree_sitter_python::HIGHLIGHTS_QUERY],
            injections: "",
            line_comment: Some("#"),
            block_comment: None,
        },
        Spec {
            id: "go",
            display: "Go",
            language: || tree_sitter_go::LANGUAGE.into(),
            highlights: &[tree_sitter_go::HIGHLIGHTS_QUERY],
            injections: "",
            line_comment: Some("//"),
            block_comment: C_BLOCK,
        },
        Spec {
            id: "c",
            display: "C",
            language: || tree_sitter_c::LANGUAGE.into(),
            highlights: &[tree_sitter_c::HIGHLIGHT_QUERY],
            injections: "",
            line_comment: Some("//"),
            block_comment: C_BLOCK,
        },
        Spec {
            id: "cpp",
            display: "C++",
            language: || tree_sitter_cpp::LANGUAGE.into(),
            highlights: &[
                tree_sitter_cpp::HIGHLIGHT_QUERY,
                tree_sitter_c::HIGHLIGHT_QUERY,
            ],
            injections: "",
            line_comment: Some("//"),
            block_comment: C_BLOCK,
        },
        Spec {
            id: "html",
            display: "HTML",
            language: || tree_sitter_html::LANGUAGE.into(),
            highlights: &[tree_sitter_html::HIGHLIGHTS_QUERY],
            injections: tree_sitter_html::INJECTIONS_QUERY,
            line_comment: None,
            block_comment: Some(("<!--", "-->")),
        },
        Spec {
            id: "css",
            display: "CSS",
            language: || tree_sitter_css::LANGUAGE.into(),
            highlights: &[tree_sitter_css::HIGHLIGHTS_QUERY],
            injections: "",
            line_comment: None,
            block_comment: C_BLOCK,
        },
        Spec {
            id: "yaml",
            display: "YAML",
            language: || tree_sitter_yaml::LANGUAGE.into(),
            highlights: &[tree_sitter_yaml::HIGHLIGHTS_QUERY],
            injections: "",
            line_comment: Some("#"),
            block_comment: None,
        },
        Spec {
            id: "bash",
            display: "Shell",
            language: || tree_sitter_bash::LANGUAGE.into(),
            highlights: &[tree_sitter_bash::HIGHLIGHT_QUERY],
            injections: "",
            line_comment: Some("#"),
            block_comment: None,
        },
        Spec {
            id: "sql",
            display: "SQL",
            language: || tree_sitter_sequel::LANGUAGE.into(),
            highlights: &[tree_sitter_sequel::HIGHLIGHTS_QUERY],
            injections: "",
            line_comment: Some("--"),
            block_comment: C_BLOCK,
        },
    ]
}

/// Resolve an alias (file extension, fenced-code info string, display name) to a canonical
/// grammar id. Case-insensitive.
pub fn canonical_id(name: &str) -> Option<&'static str> {
    let n = name.trim().to_ascii_lowercase();
    Some(match n.as_str() {
        "rust" | "rs" => "rust",
        "toml" => "toml",
        "json" | "jsonc" | "json5" => "json",
        "markdown" | "md" | "mdx" => "markdown",
        "markdown_inline" => "markdown_inline",
        "javascript" | "js" | "jsx" | "mjs" | "cjs" | "node" => "javascript",
        "typescript" | "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "python" | "py" | "pyw" | "pyi" => "python",
        "go" | "golang" => "go",
        "c" | "h" => "c",
        "cpp" | "c++" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => "cpp",
        "html" | "htm" | "xhtml" => "html",
        "css" => "css",
        "yaml" | "yml" => "yaml",
        "bash" | "sh" | "shell" | "zsh" | "shellscript" => "bash",
        "sql" => "sql",
        _ => return None,
    })
}

/// One grammar's compile-once cell (shared by every thread: a [`prewarm`] on a background
/// thread and the UI thread's [`grammar`] wait on the same compile, never repeat it).
type GrammarCell = Arc<OnceLock<Option<Arc<GrammarDef>>>>;

/// Compiled grammars, built on first use (a highlight query compiles in tens of ms).
fn cell(id: &'static str) -> GrammarCell {
    static GRAMMARS: OnceLock<Mutex<HashMap<&'static str, GrammarCell>>> = OnceLock::new();
    let map = GRAMMARS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut m = map.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    m.entry(id).or_default().clone()
}

/// The compiled grammar for `name` (any alias — see [`canonical_id`]), or `None` when it is
/// unknown or its query does not compile against the bundled parser.
pub fn grammar(name: &str) -> Option<Arc<GrammarDef>> {
    let id = canonical_id(name)?;
    cell(id)
        .get_or_init(|| specs().into_iter().find(|s| s.id == id).and_then(|s| build(&s).ok()).map(Arc::new))
        .clone()
}

/// Compile `name`'s grammar on a background thread now, so the first editor to open a
/// file of that language doesn't compile its highlight query on the UI thread (tens of
/// milliseconds for Rust/TypeScript). A no-op when unknown or already built.
pub fn prewarm(name: &str) {
    let Some(id) = canonical_id(name) else { return };
    if cell(id).get().is_some() {
        return;
    }
    let _ = std::thread::Builder::new().name(format!("grammar-{id}")).spawn(move || {
        let _ = grammar(id);
    });
}

fn build(s: &Spec) -> Result<GrammarDef, String> {
    let language = (s.language)();
    let src = s.highlights.join("\n");
    let highlights = Query::new(&language, &src).map_err(|e| format!("{}: {e}", s.id))?;
    let kinds = highlights
        .capture_names()
        .iter()
        .map(|n| kind_for_capture(n))
        .collect();
    let scopes = highlights
        .capture_names()
        .iter()
        .map(|n| super::scope::textmate_scope(n))
        .collect();
    let injections = if s.injections.is_empty() {
        None
    } else {
        Query::new(&language, s.injections).ok()
    };
    Ok(GrammarDef {
        id: s.id,
        display: s.display,
        language,
        highlights,
        kinds,
        scopes,
        root_scope: super::scope::root_scope(s.id),
        injections,
        line_comment: s.line_comment,
        block_comment: s.block_comment,
    })
}

/// A tree-sitter-backed [`Language`]. Cheap to construct (grammars are compiled lazily and
/// cached per thread). Prefer [`by_name`](super::by_name), which also falls back to a hand-written scanner.
#[derive(Clone, Copy, Debug)]
pub struct TreeSitter {
    id: &'static str,
    display: &'static str,
    line_comment: Option<&'static str>,
    block_comment: Option<(&'static str, &'static str)>,
}

impl TreeSitter {
    /// The tree-sitter language for `name` (an id, extension or alias), if bundled and its
    /// queries compile.
    pub fn new(name: &str) -> Option<Self> {
        let g = grammar(name)?;
        Some(TreeSitter {
            id: g.id,
            display: g.display,
            line_comment: g.line_comment,
            block_comment: g.block_comment,
        })
    }

    /// The canonical grammar id.
    pub fn id(&self) -> &'static str {
        self.id
    }
}

impl Language for TreeSitter {
    fn name(&self) -> &str {
        self.display
    }

    /// Whole-document highlight (a one-shot parse). The editor uses [`session`](Self::session)
    /// instead, which is incremental and viewport-limited.
    fn highlight(&self, src: &str) -> Vec<Token> {
        match Session::new(self.id) {
            Some(mut s) => {
                s.background = false;
                s.update(src);
                s.highlight_range(src, 0..src.len())
            }
            None => Vec::new(),
        }
    }

    fn line_comment(&self) -> Option<&str> {
        self.line_comment
    }

    fn block_comment(&self) -> Option<(&str, &str)> {
        self.block_comment
    }

    fn session(&self) -> Option<Box<dyn SyntaxSession>> {
        Session::new(self.id).map(|s| Box::new(s) as Box<dyn SyntaxSession>)
    }
}

/// Maximum injection nesting (Markdown → HTML → JavaScript is 2).
const MAX_INJECTION_DEPTH: usize = 3;

/// Whether a multi-line node starting at byte `at` opens a fold region the way VS Code's
/// folding sees one: the next non-blank line is indented deeper than the head line (a
/// block, an argument list, a struct body). A node that merely groups same-level lines
/// (Go's `statement_list`, the body's statements without their braces) is not a region —
/// it put a second chevron on a function's first statement. Comments and strings fold
/// regardless (a block comment's lines often share the head's indent).
fn opens_a_region(text: &str, at: usize, kind: &str) -> bool {
    if kind.contains("comment") || kind.contains("string") {
        return true;
    }
    let at = at.min(text.len());
    let head_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let indent = |line: &str| {
        line.chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .map(|c| if c == '\t' { 4 } else { 1 })
            .sum::<usize>()
    };
    let head_end = text[head_start..]
        .find('\n')
        .map_or(text.len(), |i| head_start + i);
    let head = indent(&text[head_start..head_end]);
    text.get(head_end + 1..)
        .unwrap_or("")
        .lines()
        .find(|l| !l.trim().is_empty())
        .is_some_and(|l| indent(l) > head)
}

/// The incremental parse state for one document.
pub struct Session {
    grammar: Arc<GrammarDef>,
    parser: Parser,
    tree: Option<Tree>,
    text: String,
    /// Bumped on every reparse (lets callers cache derived data).
    version: u64,
    /// A large document's FIRST parse, running on a background thread: the text it parses
    /// and where the tree arrives. Until it lands the document shows unhighlighted.
    pending: Option<Pending>,
    /// Whether a large first parse may go to a background thread (off for one-shot
    /// whole-document highlighting, which needs the tree now).
    background: bool,
    /// Bumped by every reparse that changed the tree's structure or the line count.
    structure_version: u64,
    /// Highlighting running on a background thread ([`SyntaxSession::prefetch`]): the parse
    /// version it highlights and where its `(id, tokens)` arrive.
    prefetch: Option<(u64, std::sync::mpsc::Receiver<(usize, Vec<Token>)>)>,
}

/// A parse running on a background thread: the text it parses, where the tree arrives,
/// and (an edit's reparse) the edited previous tree to diff structure against.
struct Pending {
    text: String,
    rx: std::sync::mpsc::Receiver<Option<Tree>>,
    base: Option<Tree>,
    lines_changed: bool,
}

/// Documents at least this large parse off the UI thread — the first parse, and every
/// edit's reparse (2 ms per keystroke on a 5k-line file). Meanwhile the previous tree,
/// edited to the new offsets, keeps highlighting; the new one lands a few ms later. (a 5k-line Rust
/// file parses in ~50 ms; below this it is a few ms).
const BACKGROUND_PARSE_BYTES: usize = 64 * 1024;

impl Session {
    /// A fresh session for grammar `name`.
    pub fn new(name: &str) -> Option<Self> {
        let grammar = grammar(name)?;
        let mut parser = Parser::new();
        parser.set_language(&grammar.language).ok()?;
        Some(Session {
            grammar,
            parser,
            tree: None,
            text: String::new(),
            version: 0,
            pending: None,
            background: true,
            structure_version: 0,
            prefetch: None,
        })
    }

    /// Collect a finished background first parse (if any); then the tree describes
    /// `self.text` — the next [`update`](SyntaxSession::update) catches up incrementally.
    fn poll_pending(&mut self) {
        let Some(p) = &self.pending else { return };
        match p.rx.try_recv() {
            Ok(tree) => {
                let p = self.pending.take().expect("pending");
                let changed = match (&p.base, tree.as_ref()) {
                    (Some(old), Some(new)) => p.lines_changed || old.changed_ranges(new).next().is_some(),
                    _ => true,
                };
                self.tree = tree;
                self.text = p.text;
                self.version = self.version.wrapping_add(1);
                if changed {
                    self.structure_version = self.structure_version.wrapping_add(1);
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.pending = None,
        }
    }

    /// Parse `src` on a background thread (from `base`, an edited previous tree, if any).
    fn parse_in_background(&mut self, src: &str, base: Option<Tree>, lines_changed: bool) -> bool {
        let (tx, rx) = std::sync::mpsc::channel();
        let language = self.grammar.language.clone();
        let text = src.to_string();
        let copy = text.clone();
        let from = base.clone();
        let spawned = std::thread::Builder::new().name("syntax-parse".into()).spawn(move || {
            let mut parser = Parser::new();
            let tree = parser.set_language(&language).ok().and_then(|()| parser.parse(&copy, from.as_ref()));
            let _ = tx.send(tree);
        });
        if spawned.is_ok() {
            self.pending = Some(Pending { text, rx, base, lines_changed });
        }
        spawned.is_ok()
    }

    /// The current syntax tree (after the last [`update`](SyntaxSession::update)).
    pub fn tree(&self) -> Option<&Tree> {
        self.tree.as_ref()
    }
}

/// Byte offset → tree-sitter `Point` (row, byte column) in `text`.
fn point_at(text: &str, byte: usize) -> Point {
    let mut byte = byte.min(text.len());
    while !text.is_char_boundary(byte) {
        byte -= 1;
    }
    let before = text.get(..byte).unwrap_or("");
    // `str::find(char)` runs on std's (optimized) memchr, so this is fast even when this
    // crate is built at opt-level 0.
    let (mut row, mut last_nl, mut at) = (0usize, None, 0usize);
    while let Some(i) = before.get(at..).and_then(|r| r.find('\n')) {
        row += 1;
        last_nl = Some(at + i);
        at += i + 1;
    }
    let col = last_nl.map_or(byte, |nl| byte - (nl + 1));
    Point { row, column: col }
}

/// Point of `base` advanced over `text` (for the new end of an insertion).
fn advance_point(base: Point, text: &str) -> Point {
    let nls = text.bytes().filter(|&b| b == b'\n').count();
    if nls == 0 {
        Point {
            row: base.row,
            column: base.column + text.len(),
        }
    } else {
        let tail = text.len() - text.rfind('\n').map_or(0, |i| i + 1);
        Point {
            row: base.row + nls,
            column: tail,
        }
    }
}

impl SyntaxSession for Session {
    fn update(&mut self, src: &str) {
        self.poll_pending();
        if self.pending.is_some() {
            return; // still parsing the first version; it catches up when it lands
        }
        if self.tree.is_some() && self.text == src {
            return;
        }
        let background = self.background && src.len() >= BACKGROUND_PARSE_BYTES;
        if self.tree.is_none() && background && self.parse_in_background(src, None, true) {
            return;
        }
        let incremental = match (self.tree.as_mut(), crate::collab::diff(&self.text, src)) {
            (Some(tree), Some(e)) => {
                let start_position = point_at(&self.text, e.from);
                let old_end_position = point_at(&self.text, e.to);
                let new_end_position = advance_point(start_position, &e.insert);
                tree.edit(&InputEdit {
                    start_byte: e.from,
                    old_end_byte: e.to,
                    new_end_byte: e.from + e.insert.len(),
                    start_position,
                    old_end_position,
                    new_end_position,
                });
                true
            }
            _ => false,
        };
        if incremental && background {
            // Keep highlighting with the edited previous tree; reparse off the UI thread.
            let lines_changed = self.text.bytes().filter(|&b| b == b'\n').count()
                != src.bytes().filter(|&b| b == b'\n').count();
            let base = self.tree.clone();
            if self.parse_in_background(src, base, lines_changed) {
                self.text.clear();
                self.text.push_str(src);
                return;
            }
        }
        let next = self.parser.parse(src, if incremental { self.tree.as_ref() } else { None });
        // Did the syntax STRUCTURE change (not just a token's text)? Typing inside an
        // identifier or a string doesn't — then syntax-derived data (fold regions) stands.
        let changed = match (incremental, self.tree.as_ref(), next.as_ref()) {
            (true, Some(old), Some(new)) => {
                old.changed_ranges(new).next().is_some()
                    || self.text.bytes().filter(|&b| b == b'\n').count()
                        != src.bytes().filter(|&b| b == b'\n').count()
            }
            _ => true,
        };
        if changed {
            self.structure_version = self.structure_version.wrapping_add(1);
        }
        self.tree = next;
        self.text.clear();
        self.text.push_str(src);
        self.version = self.version.wrapping_add(1);
    }

    fn highlight_range(&mut self, src: &str, range: Range<usize>) -> Vec<Token> {
        if self.text != src || self.pending.is_some() {
            self.update(src);
        }
        let Some(tree) = self.tree.as_ref() else {
            return Vec::new();
        };
        let start = range.start.min(src.len());
        let end = range.end.clamp(start, src.len());
        let mut paint: Vec<Paint> = vec![None; end - start];
        paint_layer(&self.grammar, tree, src, start..end, &mut paint, 0);
        runs(&paint, start)
    }

    fn fold_ranges(&mut self) -> Vec<(usize, usize)> {
        let Some(tree) = self.tree.as_ref() else {
            return Vec::new();
        };
        let mut out: Vec<(usize, usize)> = Vec::new();
        let mut cursor = tree.walk();
        let mut depth = 0usize;
        // Pre-order walk; every named multi-line node (block, body, list, …) is a region.
        loop {
            let node = cursor.node();
            let (s, e) = (node.start_position().row, node.end_position().row);
            // A region spans its head line to the line before the closer (VS Code shows the
            // closing bracket line), unless the node ends at column 0 of a later line.
            let last = if node.end_position().column == 0 {
                e.saturating_sub(1)
            } else {
                e
            };
            if node.is_named()
                && depth > 0
                && last > s
                && opens_a_region(&self.text, node.start_byte(), node.kind())
            {
                out.push((s, last));
            }
            // A node on one line has only one-line descendants (never a region): skip its
            // subtree. Most of the tree is such leaves, so this walk costs the multi-line
            // nodes only (it ran on every keystroke over every token).
            if depth < 64 && e > s && cursor.goto_first_child() {
                depth += 1;
                continue;
            }
            loop {
                if cursor.goto_next_sibling() {
                    break;
                }
                if !cursor.goto_parent() {
                    out.sort_unstable();
                    // One region per head line: keep the largest.
                    let mut dedup: Vec<(usize, usize)> = Vec::with_capacity(out.len());
                    for (s, e) in out {
                        match dedup.last_mut() {
                            Some(last) if last.0 == s => last.1 = last.1.max(e),
                            _ => dedup.push((s, e)),
                        }
                    }
                    return dedup;
                }
                depth = depth.saturating_sub(1);
            }
        }
    }

    fn version(&self) -> u64 {
        self.version
    }

    fn needs_more(&self) -> bool {
        self.pending.is_some()
    }

    fn prefetch(&mut self, src: &str, ranges: Vec<(usize, Range<usize>)>) {
        if ranges.is_empty() || self.prefetch.is_some() || self.pending.is_some() || self.text != src {
            return;
        }
        let Some(tree) = self.tree.clone() else { return };
        let (tx, rx) = std::sync::mpsc::channel();
        let grammar = self.grammar.clone();
        let text = src.to_string();
        let spawned = std::thread::Builder::new().name("syntax-highlight".into()).spawn(move || {
            for (id, r) in ranges {
                let start = r.start.min(text.len());
                let end = r.end.clamp(start, text.len());
                let mut paint: Vec<Paint> = vec![None; end - start];
                paint_layer(&grammar, &tree, &text, start..end, &mut paint, 0);
                if tx.send((id, runs(&paint, start))).is_err() {
                    return;
                }
            }
        });
        if spawned.is_ok() {
            self.prefetch = Some((self.version, rx));
        }
    }

    fn take_prefetched(&mut self) -> Vec<(usize, Vec<Token>)> {
        self.poll_pending();
        let Some((ver, rx)) = &self.prefetch else { return Vec::new() };
        if *ver != self.version || self.pending.is_some() {
            // Highlighted against a superseded parse: drop it (the thread stops on send).
            self.prefetch = None;
            return Vec::new();
        }
        let mut out = Vec::new();
        loop {
            match rx.try_recv() {
                Ok(r) => out.push(r),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.prefetch = None;
                    break;
                }
            }
        }
        out
    }

    fn structure_version(&self) -> u64 {
        self.structure_version
    }

    fn enclosing_nodes(&mut self, from: usize, to: usize) -> Vec<(usize, usize)> {
        let Some(tree) = self.tree.as_ref() else {
            return Vec::new();
        };
        let mut out: Vec<(usize, usize)> = Vec::new();
        let mut node = tree.root_node().descendant_for_byte_range(from, to);
        while let Some(n) = node {
            let r = (n.start_byte(), n.end_byte());
            if r.0 <= from && to <= r.1 && out.last() != Some(&r) {
                out.push(r);
            }
            node = n.parent();
        }
        out
    }
}

/// Run `g`'s highlight query over `range` of `tree`, painting kinds into `paint` (indexed
/// from `base = range.start` of the TOP-level call), then recurse into injections.
fn paint_layer(
    g: &GrammarDef,
    tree: &Tree,
    src: &str,
    range: Range<usize>,
    paint: &mut [Paint],
    depth: usize,
) {
    paint_into(g, tree, src, range.clone(), paint, range.start);
    if depth >= MAX_INJECTION_DEPTH {
        return;
    }
    let Some(inj) = g.injections.as_ref() else {
        return;
    };
    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(range.clone());
    cursor.set_match_limit(256);
    let names = inj.capture_names();
    let mut matches = cursor.matches(inj, tree.root_node(), src.as_bytes());
    let mut jobs: Vec<(String, TsRange)> = Vec::new();
    while let Some(m) = matches.next() {
        let mut lang: Option<String> = inj
            .property_settings(m.pattern_index)
            .iter()
            .find(|p| &*p.key == "injection.language")
            .and_then(|p| p.value.as_deref().map(str::to_string));
        let mut content: Option<TsRange> = None;
        for c in m.captures() {
            match names.get(c.index as usize).copied() {
                Some("injection.language") => {
                    lang = src.get(c.node.byte_range()).map(str::to_string);
                }
                Some("injection.content") => content = Some(c.node.range()),
                _ => {}
            }
        }
        if let (Some(l), Some(r)) = (lang, content) {
            jobs.push((l, r));
        }
    }
    drop(matches);
    for (lang, r) in jobs {
        let Some(sub) = grammar(&lang) else { continue };
        let mut parser = Parser::new();
        if parser.set_language(&sub.language).is_err() || parser.set_included_ranges(&[r]).is_err()
        {
            continue;
        }
        let Some(sub_tree) = parser.parse(src, None) else {
            continue;
        };
        let lo = r.start_byte.max(range.start);
        let hi = r.end_byte.min(range.end);
        if lo >= hi {
            continue;
        }
        // The injected region repaints over the host (CSS-in-HTML etc.).
        if let Some(slot) = paint.get_mut(lo - range.start..hi - range.start) {
            // Clear only spans the host painted as plain strings/raw text, so host
            // punctuation (``` fences) stays; injected tokens then paint on top.
            for k in slot.iter_mut() {
                if matches!(k, Some((TokenKind::Str, _))) {
                    *k = None;
                }
            }
        }
        paint_layer_at(&sub, &sub_tree, src, lo..hi, paint, range.start, depth + 1);
    }
}

/// [`paint_layer`] for a nested layer whose paint buffer is based at `base`.
fn paint_layer_at(
    g: &GrammarDef,
    tree: &Tree,
    src: &str,
    range: Range<usize>,
    paint: &mut [Paint],
    base: usize,
    depth: usize,
) {
    // Re-slice the buffer so the nested call sees `range.start` as its own base.
    let off = range.start.saturating_sub(base);
    let len = range.end.saturating_sub(range.start);
    if let Some(sub) = paint.get_mut(off..off + len) {
        paint_layer(g, tree, src, range, sub, depth);
    }
}

/// Paint one layer's captures. Larger nodes paint first so nested captures win; for the
/// same node the EARLIER query pattern wins (tree-sitter-highlight semantics).
fn paint_into(
    g: &GrammarDef,
    tree: &Tree,
    src: &str,
    range: Range<usize>,
    paint: &mut [Paint],
    base: usize,
) {
    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(range.clone());
    cursor.set_match_limit(1024);
    let mut spans: Vec<(usize, usize, usize, (TokenKind, ScopeId))> = Vec::new();
    let mut ids: HashMap<(u32, String), ScopeId> = HashMap::new();
    let mut matches = cursor.matches(&g.highlights, tree.root_node(), src.as_bytes());
    while let Some(m) = matches.next() {
        for c in m.captures() {
            let i = c.index as usize;
            let mut kind = g.kinds.get(i).copied().flatten();
            let mut tm = g.scopes.get(i).copied().flatten();
            if kind.is_none() && tm.is_none() {
                continue;
            }
            // Numeric literals some grammars capture as `@constant.builtin`.
            let nk = c.node.kind();
            if kind == Some(TokenKind::Constant)
                && (nk.contains("integer") || nk.contains("float") || nk.contains("number"))
            {
                kind = Some(TokenKind::Number);
                tm = Some("constant.numeric");
            }
            let id = match tm {
                Some(tm) => {
                    let metas = meta_stack(c.node);
                    *ids.entry((c.index, format!("{metas}{tm}")))
                        .or_insert_with(|| {
                            let lang = g.root_scope.rsplit('.').next().unwrap_or("");
                            intern_scope(&format!("{} {metas}{tm}.{lang}", g.root_scope))
                        })
                }
                None => 0,
            };
            let r = c.node.byte_range();
            spans.push((
                r.start,
                r.end,
                m.pattern_index,
                (kind.unwrap_or(TokenKind::Plain), id),
            ));
        }
    }
    drop(matches);
    // Scope-only captures (`@variable`) paint first so a coloured capture of the same node
    // (`@tag` on a JSX identifier) wins whatever the pattern order.
    spans.sort_by(|a, b| {
        (b.1 - b.0)
            .cmp(&(a.1 - a.0))
            .then((a.3.0 != TokenKind::Plain).cmp(&(b.3.0 != TokenKind::Plain)))
            .then(b.2.cmp(&a.2))
    });
    for (s, e, _, kind) in spans {
        let lo = s.max(range.start).saturating_sub(base);
        let hi = e.min(range.end).saturating_sub(base);
        if let Some(slot) = paint.get_mut(lo..hi.max(lo)) {
            slot.fill(Some(kind));
        }
    }
}

/// One painted byte: its kind and interned scope stack.
type Paint = Option<(TokenKind, ScopeId)>;

/// The `meta.*` scopes of `node`'s ancestors, outermost first, each followed by a space.
fn meta_stack(node: tree_sitter::Node<'_>) -> String {
    let mut metas: Vec<&'static str> = Vec::new();
    let mut n = node.parent();
    let mut hops = 0;
    while let Some(p) = n {
        if let Some(m) = super::scope::meta_scope(p.kind())
            && metas.last() != Some(&m)
        {
            metas.push(m);
        }
        hops += 1;
        if hops > 32 {
            break;
        }
        n = p.parent();
    }
    let mut out = String::new();
    for m in metas.iter().rev() {
        out.push_str(m);
        out.push(' ');
    }
    out
}

/// Run-length encode a paint buffer into sorted, non-overlapping tokens.
fn runs(paint: &[Paint], base: usize) -> Vec<Token> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < paint.len() {
        let k = paint[i];
        let mut j = i + 1;
        while j < paint.len() && paint[j] == k {
            j += 1;
        }
        if let Some((kind, scope)) = k {
            out.push(Token {
                start: base + i,
                len: j - i,
                kind,
                scope,
            });
        }
        i = j;
    }
    out
}

/// Every bundled tree-sitter grammar id.
pub const GRAMMAR_IDS: &[&str] = &[
    "rust",
    "toml",
    "json",
    "markdown",
    "javascript",
    "typescript",
    "tsx",
    "python",
    "go",
    "c",
    "cpp",
    "html",
    "css",
    "yaml",
    "bash",
    "sql",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds_at(lang: &str, src: &str, needle: &str) -> Vec<TokenKind> {
        let at = src.find(needle).expect("needle");
        TreeSitter::new(lang)
            .unwrap_or_else(|| panic!("{lang} grammar"))
            .highlight(src)
            .into_iter()
            .filter(|t| t.start <= at && at < t.start + t.len)
            .map(|t| t.kind)
            .collect()
    }

    #[test]
    fn every_bundled_grammar_compiles_and_highlights() {
        let samples: &[(&str, &str, &str, TokenKind)] = &[
            ("rust", "fn main() { let x = 1; }", "fn", TokenKind::Keyword),
            ("toml", "[package]\nname = \"x\"\n", "\"x\"", TokenKind::Str),
            ("json", "{\"a\": 1}", "1", TokenKind::Number),
            ("markdown", "# Title\n\ntext\n", "Title", TokenKind::Heading),
            (
                "javascript",
                "function f() { return 1; }",
                "return",
                TokenKind::Keyword,
            ),
            (
                "typescript",
                "let x: number = 1;",
                "number",
                TokenKind::Type,
            ),
            ("tsx", "const a = <div>hi</div>;", "div", TokenKind::Tag),
            (
                "python",
                "def f():\n    return 1\n",
                "def",
                TokenKind::Keyword,
            ),
            (
                "go",
                "package main\nfunc main() {}\n",
                "func",
                TokenKind::Keyword,
            ),
            (
                "c",
                "int main(void) { return 0; }",
                "return",
                TokenKind::Keyword,
            ),
            (
                "cpp",
                "class A { public: int x; };",
                "class",
                TokenKind::Keyword,
            ),
            ("html", "<div class=\"a\">x</div>", "div", TokenKind::Tag),
            ("css", "a { color: red; }", "color", TokenKind::Property),
            ("yaml", "key: \"v\"\n", "\"v\"", TokenKind::Str),
            ("bash", "# c\necho hi\n", "# c", TokenKind::Comment),
            ("sql", "SELECT a FROM t;", "SELECT", TokenKind::Keyword),
        ];
        for &(lang, src, needle, want) in samples {
            let got = kinds_at(lang, src, needle);
            assert!(
                got.contains(&want),
                "{lang}: {needle:?} → {got:?}, want {want:?}"
            );
        }
        for id in GRAMMAR_IDS {
            assert!(grammar(id).is_some(), "{id} must compile");
        }
    }

    #[test]
    fn injections_highlight_embedded_languages() {
        // Markdown fenced code → Rust.
        let md = "# T\n\n```rust\nfn main() {}\n```\n";
        assert!(kinds_at("markdown", md, "fn main").contains(&TokenKind::Keyword));
        // HTML <script> → JavaScript, <style> → CSS.
        let html = "<script>let a = 1;</script><style>p { color: red; }</style>";
        assert!(kinds_at("html", html, "let").contains(&TokenKind::Keyword));
        assert!(kinds_at("html", html, "color").contains(&TokenKind::Property));
        // Markdown inline (emphasis) through the inline grammar.
        let md2 = "some *emph* text\n";
        assert!(kinds_at("markdown", md2, "emph").contains(&TokenKind::Emphasis));
    }

    #[test]
    fn incremental_update_matches_a_fresh_parse() {
        let mut s = Session::new("rust").unwrap();
        let a = "fn a() {}\nfn b() {}\n";
        s.update(a);
        let b = "fn a() { let s = \"x\"; }\nfn b() {}\n";
        s.update(b);
        let inc = s.highlight_range(b, 0..b.len());
        let fresh = TreeSitter::new("rust").unwrap().highlight(b);
        assert_eq!(
            inc.iter()
                .map(|t| (t.start, t.len, t.kind))
                .collect::<Vec<_>>(),
            fresh
                .iter()
                .map(|t| (t.start, t.len, t.kind))
                .collect::<Vec<_>>()
        );
        assert_eq!(s.version(), 2);
    }

    #[test]
    fn highlight_range_is_viewport_limited() {
        let src = "fn a() {}\n".repeat(1000);
        let mut s = Session::new("rust").unwrap();
        s.update(&src);
        let toks = s.highlight_range(&src, 500..600);
        assert!(!toks.is_empty());
        assert!(
            toks.iter()
                .all(|t| t.start >= 500 && t.start + t.len <= 600)
        );
    }

    #[test]
    fn syntax_fold_ranges_cover_blocks() {
        let src = "fn a() {\n    let x = 1;\n    x\n}\n";
        let mut s = Session::new("rust").unwrap();
        s.update(src);
        let f = s.fold_ranges();
        assert!(f.iter().any(|&(h, l)| h == 0 && l >= 2), "{f:?}");
    }

    #[test]
    fn go_statement_lists_are_not_fold_regions() {
        // Gravel sweep S14: Go's `statement_list` (a body's statements without their
        // braces) put a second chevron on a function's first statement.
        let src = "package main\n\nfunc main() {\n\ttotal := Add(1, 2)\n\tfmt.Println(total)\n}\n\nfunc f(\n\ta int,\n\tb int,\n) {}\n\n/* a\nb */\n";
        let mut s = Session::new("go").unwrap();
        s.update(src);
        let f = s.fold_ranges();
        assert!(f.contains(&(2, 5)), "the function body folds: {f:?}");
        assert!(
            !f.iter().any(|&(h, _)| h == 3),
            "no region on the first statement: {f:?}"
        );
        assert!(
            f.iter().any(|&(h, _)| h == 7),
            "a multi-line parameter list folds: {f:?}"
        );
        assert!(
            f.iter().any(|&(h, _)| h == 12),
            "a block comment folds: {f:?}"
        );
    }

    #[test]
    fn enclosing_nodes_ladder() {
        let src = "fn f() { let x = a + b; }";
        let mut s = Session::new("rust").unwrap();
        s.update(src);
        let at = src.find('a').unwrap();
        let l = s.enclosing_nodes(at, at);
        assert_eq!(l.first(), Some(&(at, at + 1)));
        assert!(l.iter().any(|&(a, b)| &src[a..b] == "a + b"));
        assert_eq!(l.last(), Some(&(0, src.len())));
    }

    #[test]
    fn garbage_never_panics() {
        for id in GRAMMAR_IDS {
            let l = TreeSitter::new(id).unwrap();
            let _ = l.highlight("\"unterminated /* ' `\u{1F600}\n\t<<<{{{[[[ weird \u{0}");
            let _ = l.highlight("");
        }
    }

    #[test]
    fn by_name_prefers_parser_and_falls_back() {
        assert_eq!(super::super::by_name("rs").name(), "Rust");
        assert!(super::super::by_name("rs").session().is_some());
        // Kotlin has no bundled grammar → hand-written scanner (no session).
        let k = super::super::by_name("kotlin");
        assert_eq!(k.name(), "Kotlin");
        assert!(k.session().is_none());
        assert_eq!(super::super::by_name("nonesuch").name(), "Plain Text");
        assert_eq!(super::super::for_path("src/App.tsx").name(), "TSX");
    }

    #[test]
    fn rust_keywords_numbers_and_scope_stacks() {
        let src = "pub fn main() { let x = 1; }\nstruct A;\nimpl A { fn f(&self) {} }\n";
        let mut sess = TreeSitter::new("rust").unwrap().session().unwrap();
        let toks = sess.highlight_range(src, 0..src.len());
        let at = |w: &str| {
            let at = src.find(w).unwrap();
            *toks
                .iter()
                .find(|t| t.start <= at && at < t.start + t.len)
                .unwrap_or_else(|| panic!("token at {w:?}"))
        };
        assert_eq!(at("fn main").kind, TokenKind::Keyword);
        assert_eq!(at("struct").kind, TokenKind::Keyword);
        assert_eq!(at("let").kind, TokenKind::Keyword);
        assert_eq!(
            at("1;").kind,
            TokenKind::Number,
            "integer literal is a number"
        );
        let name = |w: &str| {
            super::super::scope::scope_name(at(w).scope)
                .unwrap()
                .to_string()
        };
        assert_eq!(
            name("fn main"),
            "source.rust meta.function keyword.other.rust"
        );
        assert_eq!(
            name("main"),
            "source.rust meta.function entity.name.function.rust"
        );
        assert_eq!(
            name("1;"),
            "source.rust meta.function constant.numeric.rust"
        );
        assert_eq!(
            name("self"),
            "source.rust meta.class meta.function variable.language.rust"
        );
        assert!(name("A;").starts_with("source.rust meta.class entity.name.type"));
    }

    /// Every capture of every bundled grammar either colours its token or is one of the
    /// intentionally uncoloured ones (identifiers / parameters / embedded regions), and
    /// every colouring capture has a TextMate scope.
    #[test]
    fn every_capture_is_mapped() {
        for id in GRAMMAR_IDS {
            let g = grammar(id).unwrap();
            for n in g.highlights.capture_names() {
                let kind = kind_for_capture(n);
                let tm = super::super::scope::textmate_scope(n);
                let intentional = n.starts_with("variable")
                    || n.starts_with("injection")
                    || n.starts_with('_')
                    || matches!(*n, "parameter" | "embedded" | "none" | "spell");
                assert!(kind.is_some() || intentional, "{id}: @{n} falls through");
                assert!(
                    tm.is_some() || kind.is_none(),
                    "{id}: @{n} has no TextMate scope"
                );
            }
        }
    }
}
