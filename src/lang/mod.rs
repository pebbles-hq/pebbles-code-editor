//! Syntax highlighting — the pluggable language layer. A [`Language`] turns source
//! text into a list of [`Token`]s (byte ranges tagged with a [`TokenKind`]); the editor
//! maps each kind to a color via the active [`crate::EditorTheme`]. Ship your own by
//! implementing [`Language`] — that's how an IDE built on this adds a grammar.
//!
//! The bundled tokenizers ([`Rust`], [`Json`], [`Plain`]) are hand-written, char-based
//! scanners: fast, dependency-free, and panic-free on ANY input (including partial or
//! invalid source, which an editor is full of). They are "good enough to read", not a
//! compiler front end — good enough is exactly what a highlighter needs.

/// The semantic class of a token. Themes map each of these to a color.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenKind {
    /// Language keyword (`fn`, `let`, `if`, `return`, …).
    Keyword,
    /// A type / capitalized identifier.
    Type,
    /// String / char literal.
    Str,
    /// Numeric literal.
    Number,
    /// Comment (line or block).
    Comment,
    /// A function or method name at a call/definition site.
    Function,
    /// A macro invocation (`println!`).
    Macro,
    /// An attribute / annotation (`#[derive(...)]`).
    Attribute,
    /// A boolean / language constant (`true`, `false`, `null`, `None`).
    Constant,
    /// An operator or punctuation.
    Punctuation,
    /// An object/record property or field (a JSON key, `obj.field`).
    Property,
    /// An escape sequence inside a string (`\n`, `\u{1F600}`) or a regex.
    Escape,
    /// A markup tag name (`div` in `<div>`).
    Tag,
    /// A module / namespace path segment.
    Namespace,
    /// A Markdown heading.
    Heading,
    /// A link / URL.
    Link,
    /// Emphasised markup (bold / italic / strikethrough).
    Emphasis,
    /// Anything else (plain identifiers, whitespace).
    Plain,
}

/// A highlighted run: `[start, start+len)` bytes of the source, tagged `kind`.
#[derive(Clone, Copy, Debug)]
pub struct Token {
    /// Byte offset of the run's start in the source.
    pub start: usize,
    /// Length of the run in bytes.
    pub len: usize,
    /// The token's semantic kind (drives its color).
    pub kind: TokenKind,
}

impl Token {
    fn new(start: usize, len: usize, kind: TokenKind) -> Self {
        Token { start, len, kind }
    }
}

/// A node in a coarse structural [syntax tree](Language::tree): a bracket-delimited region
/// (or the whole document, for the root). Not a full AST — it's the bracket-nesting skeleton
/// that tooling (folding, scope queries, structural navigation) needs, analogous in spirit
/// to CodeMirror's Lezer tree at a coarse grain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxNode {
    /// The opening delimiter (`(`, `[`, `{`, …), or `'\0'` for the document root.
    pub open: char,
    /// Byte offset of the opener (0 for the root).
    pub start: usize,
    /// Byte offset just past the closer (the document length for the root, or for an
    /// unclosed node the point where scanning ended).
    pub end: usize,
    /// Directly-nested child regions, in document order.
    pub children: Vec<SyntaxNode>,
}

/// A syntax highlighter for one language. Implement this to add a grammar; the editor
/// only needs `highlight`. Returned tokens must be sorted by `start` and non-overlapping
/// (the bundled scanners guarantee this); gaps are rendered as plain text.
pub trait Language {
    /// A short display name (shown in the editor's status line).
    fn name(&self) -> &str;
    /// Tag the colored runs of `src`. Never panics, whatever `src` contains.
    fn highlight(&self, src: &str) -> Vec<Token>;

    /// The line-comment prefix, if the language has one (`//`, `#`, `--`). Used by the
    /// comment-toggle command (Ctrl+/). `None` disables comment toggling.
    fn line_comment(&self) -> Option<&str> {
        None
    }

    /// The bracket pairs the editor should match and auto-close. Defaults to the common
    /// `()`, `[]`, `{}`; override to add language-specific pairs.
    fn brackets(&self) -> &[(char, char)] {
        &[('(', ')'), ('[', ']'), ('{', '}')]
    }

    /// A coarse structural [`SyntaxNode`] tree for `src` — for tooling (folding, structural
    /// navigation, scope queries). The default builds the bracket-nesting skeleton from
    /// [`brackets`](Self::brackets) and is error-tolerant (mismatched / unclosed brackets are
    /// attached where scanning left them). Override for a real parse tree. Never panics.
    fn tree(&self, src: &str) -> SyntaxNode {
        bracket_tree(src, self.brackets())
    }

    /// The block-comment delimiters (`/*`, `*/`), if the language has them. Used by the
    /// toggle-block-comment command (Shift+Alt+A) and as the line-comment fallback.
    fn block_comment(&self) -> Option<(&str, &str)> {
        None
    }

    /// A stateful, **incremental** highlighting session (a parse tree that is re-parsed
    /// incrementally on edits and queried per viewport). `None` (the default) means the
    /// editor calls [`highlight`](Self::highlight) on the whole text instead.
    fn session(&self) -> Option<Box<dyn SyntaxSession>> {
        None
    }
}

/// An incremental highlighter bound to one document — see [`Language::session`]. The editor
/// keeps one per open document and calls [`update`](Self::update) with the current text on
/// every change (the session derives the edit itself), then
/// [`highlight_range`](Self::highlight_range) for the visible window only.
pub trait SyntaxSession {
    /// Bring the parse up to date with `src` (incremental when a previous parse exists).
    fn update(&mut self, src: &str);
    /// Highlight tokens intersecting `range` (bytes), sorted and non-overlapping.
    fn highlight_range(&mut self, src: &str, range: std::ops::Range<usize>) -> Vec<Token>;
    /// Syntax-derived foldable regions as `(head_line, last_line)`, one per head line.
    fn fold_ranges(&mut self) -> Vec<(usize, usize)> {
        Vec::new()
    }
    /// A counter bumped on every reparse (for caching derived data).
    fn version(&self) -> u64 {
        0
    }
}

/// Cap on the bracket nesting the tree represents. [`SyntaxNode`] is a recursive
/// `Vec<SyntaxNode>`, so its derived `Drop` recurses once per level — source nested past a
/// sane depth (pathological or malicious input, e.g. 100k unmatched `[`) would overflow the
/// stack when the tree is dropped. Openers deeper than this are ignored, keeping the tree
/// shallow enough to build AND drop safely; real source never comes close.
const MAX_TREE_DEPTH: usize = 256;

/// Build the bracket-nesting tree of `src` (the default [`Language::tree`]). Character-based
/// and error-tolerant; ignores which brackets sit inside strings/comments (the coarse
/// baseline). Nesting is capped at a fixed maximum depth so it never overflows the stack on
/// pathologically deep input. Never panics.
pub fn bracket_tree(src: &str, brackets: &[(char, char)]) -> SyntaxNode {
    let mut root = SyntaxNode {
        open: '\0',
        start: 0,
        end: src.len(),
        children: Vec::new(),
    };
    let mut stack: Vec<SyntaxNode> = Vec::new();
    let attach = |stack: &mut Vec<SyntaxNode>, root: &mut SyntaxNode, node: SyntaxNode| match stack
        .last_mut()
    {
        Some(parent) => parent.children.push(node),
        None => root.children.push(node),
    };
    for (i, ch) in src.char_indices() {
        if brackets.iter().any(|(o, _)| *o == ch) {
            // Past the depth cap, drop the opener rather than nest deeper — a deeper tree
            // risks a stack overflow when its recursive `Drop` runs.
            if stack.len() < MAX_TREE_DEPTH {
                stack.push(SyntaxNode {
                    open: ch,
                    start: i,
                    end: i,
                    children: Vec::new(),
                });
            }
        } else if let Some(&(o, _)) = brackets.iter().find(|(_, c)| *c == ch)
            && let Some(mut node) = stack.pop_if(|n| n.open == o)
        {
            // (a stray / mismatched closer leaves the stack untouched — error-tolerant)
            node.end = i + ch.len_utf8();
            attach(&mut stack, &mut root, node);
        }
    }
    // Unclosed openers: close them where scanning ended, innermost first.
    while let Some(mut node) = stack.pop() {
        node.end = src.len();
        attach(&mut stack, &mut root, node);
    }
    root.children.sort_by_key(|n| n.start);
    root
}

/// No highlighting — everything is plain text.
pub struct Plain;
impl Language for Plain {
    fn name(&self) -> &str {
        "Plain Text"
    }
    fn highlight(&self, _src: &str) -> Vec<Token> {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// Bundled grammars — one module per scanner (see ARCHITECTURE.md).
// ---------------------------------------------------------------------------

mod clike;
mod json;
mod python;
mod rust;
mod scan;

pub use clike::{
    CLike, Grammar, bash, c_lang, cpp, csharp, go, java, javascript, kotlin, php, ruby, swift,
    toml, typescript, yaml,
};
pub use json::Json;
pub use python::Python;
pub use rust::Rust;

#[cfg(feature = "tree-sitter")]
pub mod treesitter;
#[cfg(feature = "tree-sitter")]
pub use treesitter::TreeSitter;

/// The best available [`Language`] for `name` — a language name (`"Rust"`, `"TypeScript"`),
/// grammar id, fenced-code info string or file extension (`"rs"`, `"tsx"`, `"yml"`).
/// Incremental tree-sitter highlighting when the grammar is bundled (feature `tree-sitter`,
/// default), else the hand-written scanner for that language, else [`Plain`] — so this never
/// fails and plain text is the universal fallback.
pub fn by_name(name: &str) -> Box<dyn Language> {
    #[cfg(feature = "tree-sitter")]
    if let Some(ts) = TreeSitter::new(name) {
        return Box::new(ts);
    }
    scanner_by_name(name)
}

/// [`by_name`] for a file path: resolves by extension (and a few well-known file names such
/// as `Makefile`, `Dockerfile`, `.bashrc`).
pub fn for_path(path: &str) -> Box<dyn Language> {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let lower = file.to_ascii_lowercase();
    let ext = match lower.as_str() {
        ".bashrc" | ".zshrc" | ".profile" | ".bash_profile" | "pkgbuild" => "sh",
        "cargo.lock" => "toml",
        _ => lower.rsplit_once('.').map_or("", |(_, e)| e),
    };
    by_name(ext)
}

/// The hand-written scanner for `name` (the fallback when no parser grammar is available).
pub fn scanner_by_name(name: &str) -> Box<dyn Language> {
    match name.trim().to_ascii_lowercase().as_str() {
        "rust" | "rs" => Box::new(Rust),
        "python" | "py" | "pyi" | "pyw" => Box::new(Python),
        "json" | "jsonc" => Box::new(Json),
        "c" | "h" => Box::new(c_lang()),
        "c++" | "cpp" | "cc" | "cxx" | "hpp" | "hh" => Box::new(cpp()),
        "c#" | "csharp" | "cs" => Box::new(csharp()),
        "go" | "golang" => Box::new(go()),
        "java" => Box::new(java()),
        "javascript" | "js" | "jsx" | "mjs" | "cjs" => Box::new(javascript()),
        "typescript" | "ts" | "tsx" | "mts" | "cts" => Box::new(typescript()),
        "kotlin" | "kt" | "kts" => Box::new(kotlin()),
        "php" => Box::new(php()),
        "ruby" | "rb" => Box::new(ruby()),
        "swift" => Box::new(swift()),
        "toml" => Box::new(toml()),
        "yaml" | "yml" => Box::new(yaml()),
        "shell" | "bash" | "sh" | "zsh" => Box::new(bash()),
        _ => Box::new(Plain),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_grammars_highlight_keywords() {
        // Each new grammar tags at least one keyword/comment run.
        assert!(
            cpp()
                .highlight("class Foo {};")
                .iter()
                .any(|t| t.kind == TokenKind::Keyword)
        );
        assert!(
            csharp()
                .highlight("public class A {}")
                .iter()
                .any(|t| t.kind == TokenKind::Keyword)
        );
        assert!(
            kotlin()
                .highlight("fun main() {}")
                .iter()
                .any(|t| t.kind == TokenKind::Keyword)
        );
        assert!(
            swift()
                .highlight("func f() {}")
                .iter()
                .any(|t| t.kind == TokenKind::Keyword)
        );
        assert!(
            ruby()
                .highlight("# note\ndef m; end")
                .iter()
                .any(|t| t.kind == TokenKind::Comment)
        );
        assert!(
            bash()
                .highlight("# c\nif x; then :; fi")
                .iter()
                .any(|t| t.kind == TokenKind::Comment)
        );
    }

    #[test]
    fn grammars_are_panic_free_on_garbage() {
        // An editor is full of half-typed / invalid source — scanners must never panic.
        for g in [
            cpp(),
            csharp(),
            kotlin(),
            swift(),
            php(),
            ruby(),
            bash(),
            yaml(),
            toml(),
        ] {
            let _ = g.highlight("\"unterminated /* nested ' `\u{1F600}\n\t weird");
            let _ = g.highlight("");
        }
    }

    #[test]
    fn bracket_tree_nests_and_tolerates_errors() {
        let brs = [('(', ')'), ('[', ']'), ('{', '}')];
        let t = bracket_tree("a(b[c]{d})", &brs);
        assert_eq!(t.children.len(), 1);
        let paren = &t.children[0];
        assert_eq!((paren.open, paren.start, paren.end), ('(', 1, 10));
        assert_eq!(paren.children.len(), 2);
        assert_eq!(paren.children[0].open, '[');
        assert_eq!(paren.children[1].open, '{');
        // Unclosed opener is tolerated and closed at end-of-source.
        let u = bracket_tree("foo(bar", &brs);
        assert_eq!(u.children.len(), 1);
        assert_eq!((u.children[0].open, u.children[0].end), ('(', 7));
        // Stray closer is ignored.
        assert!(bracket_tree("a)b", &brs).children.is_empty());
    }

    #[test]
    fn bracket_tree_deep_nesting_does_not_overflow() {
        // Pathologically deep input: the nesting cap keeps the recursive SyntaxNode tree
        // shallow enough to build AND drop without overflowing the stack (a SIGSEGV would
        // fail the test as an abnormal exit — merely reaching the asserts is the pass).
        let brs = [('(', ')'), ('[', ']'), ('{', '}')];
        let unclosed = bracket_tree(&"[".repeat(100_000), &brs);
        assert!(unclosed.children.len() <= 1);
        // Balanced deep nesting too (all openers then all closers).
        let balanced = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
        let _ = bracket_tree(&balanced, &brs);
    }

    #[test]
    fn line_comment_metadata() {
        assert_eq!(Rust.line_comment(), Some("//"));
        assert_eq!(Python.line_comment(), Some("#"));
        assert_eq!(cpp().line_comment(), Some("//"));
        assert_eq!(bash().line_comment(), Some("#"));
        assert_eq!(Json.line_comment(), None);
    }
}
