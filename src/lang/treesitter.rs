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
//! that language (see [`by_name`]).

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use tree_sitter::{
    InputEdit, Parser, Point, Query, QueryCursor, Range as TsRange, StreamingIterator, Tree,
};

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

thread_local! {
    /// Compiled grammars, built on first use (query compilation costs milliseconds).
    static GRAMMARS: RefCell<HashMap<&'static str, Option<Rc<GrammarDef>>>> =
        RefCell::new(HashMap::new());
}

/// The compiled grammar for `name` (any alias — see [`canonical_id`]), or `None` when it is
/// unknown or its query does not compile against the bundled parser.
pub fn grammar(name: &str) -> Option<Rc<GrammarDef>> {
    let id = canonical_id(name)?;
    GRAMMARS.with(|g| {
        if let Some(hit) = g.borrow().get(id) {
            return hit.clone();
        }
        let built = specs()
            .into_iter()
            .find(|s| s.id == id)
            .and_then(|s| build(&s).ok())
            .map(Rc::new);
        g.borrow_mut().insert(id, built.clone());
        built
    })
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
        injections,
        line_comment: s.line_comment,
        block_comment: s.block_comment,
    })
}

/// A tree-sitter-backed [`Language`]. Cheap to construct (grammars are compiled lazily and
/// cached per thread). Prefer [`by_name`], which also falls back to a hand-written scanner.
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

/// The incremental parse state for one document.
pub struct Session {
    grammar: Rc<GrammarDef>,
    parser: Parser,
    tree: Option<Tree>,
    text: String,
    /// Bumped on every reparse (lets callers cache derived data).
    version: u64,
}

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
        })
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
        if self.tree.is_some() && self.text == src {
            return;
        }
        let old_tree = match (self.tree.as_mut(), crate::collab::diff(&self.text, src)) {
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
                Some(&*tree)
            }
            _ => None,
        };
        let next = self.parser.parse(src, old_tree);
        self.tree = next;
        self.text.clear();
        self.text.push_str(src);
        self.version = self.version.wrapping_add(1);
    }

    fn highlight_range(&mut self, src: &str, range: Range<usize>) -> Vec<Token> {
        if self.text != src {
            self.update(src);
        }
        let Some(tree) = self.tree.as_ref() else {
            return Vec::new();
        };
        let start = range.start.min(src.len());
        let end = range.end.clamp(start, src.len());
        let mut paint: Vec<Option<TokenKind>> = vec![None; end - start];
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
            if node.is_named() && depth > 0 && last > s {
                out.push((s, last));
            }
            if depth < 64 && cursor.goto_first_child() {
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
    paint: &mut [Option<TokenKind>],
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
                if matches!(k, Some(TokenKind::Str)) {
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
    paint: &mut [Option<TokenKind>],
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
    paint: &mut [Option<TokenKind>],
    base: usize,
) {
    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(range.clone());
    cursor.set_match_limit(1024);
    let mut spans: Vec<(usize, usize, usize, TokenKind)> = Vec::new();
    let mut matches = cursor.matches(&g.highlights, tree.root_node(), src.as_bytes());
    while let Some(m) = matches.next() {
        for c in m.captures() {
            if let Some(Some(kind)) = g.kinds.get(c.index as usize) {
                let r = c.node.byte_range();
                spans.push((r.start, r.end, m.pattern_index, *kind));
            }
        }
    }
    drop(matches);
    spans.sort_by(|a, b| (b.1 - b.0).cmp(&(a.1 - a.0)).then(b.2.cmp(&a.2)));
    for (s, e, _, kind) in spans {
        let lo = s.max(range.start).saturating_sub(base);
        let hi = e.min(range.end).saturating_sub(base);
        if let Some(slot) = paint.get_mut(lo..hi.max(lo)) {
            slot.fill(Some(kind));
        }
    }
}

/// Run-length encode a paint buffer into sorted, non-overlapping tokens.
fn runs(paint: &[Option<TokenKind>], base: usize) -> Vec<Token> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < paint.len() {
        let k = paint[i];
        let mut j = i + 1;
        while j < paint.len() && paint[j] == k {
            j += 1;
        }
        if let Some(kind) = k {
            out.push(Token {
                start: base + i,
                len: j - i,
                kind,
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
}
