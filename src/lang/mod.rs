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
    /// A JSON object key.
    Property,
    /// Anything else (plain identifiers, whitespace).
    Plain,
}

/// A highlighted run: `[start, start+len)` bytes of the source, tagged `kind`.
#[derive(Clone, Copy, Debug)]
pub struct Token {
    pub start: usize,
    pub len: usize,
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
}

/// Build the bracket-nesting tree of `src` (the default [`Language::tree`]). Character-based
/// and error-tolerant; ignores which brackets sit inside strings/comments (the coarse
/// baseline). Never panics.
pub fn bracket_tree(src: &str, brackets: &[(char, char)]) -> SyntaxNode {
    let mut root = SyntaxNode {
        open: '\0',
        start: 0,
        end: src.len(),
        children: Vec::new(),
    };
    let mut stack: Vec<SyntaxNode> = Vec::new();
    let attach = |stack: &mut Vec<SyntaxNode>, root: &mut SyntaxNode, node: SyntaxNode| {
        match stack.last_mut() {
            Some(parent) => parent.children.push(node),
            None => root.children.push(node),
        }
    };
    for (i, ch) in src.char_indices() {
        if brackets.iter().any(|(o, _)| *o == ch) {
            stack.push(SyntaxNode {
                open: ch,
                start: i,
                end: i,
                children: Vec::new(),
            });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_grammars_highlight_keywords() {
        // Each new grammar tags at least one keyword/comment run.
        assert!(cpp().highlight("class Foo {};").iter().any(|t| t.kind == TokenKind::Keyword));
        assert!(csharp().highlight("public class A {}").iter().any(|t| t.kind == TokenKind::Keyword));
        assert!(kotlin().highlight("fun main() {}").iter().any(|t| t.kind == TokenKind::Keyword));
        assert!(swift().highlight("func f() {}").iter().any(|t| t.kind == TokenKind::Keyword));
        assert!(ruby().highlight("# note\ndef m; end").iter().any(|t| t.kind == TokenKind::Comment));
        assert!(bash().highlight("# c\nif x; then :; fi").iter().any(|t| t.kind == TokenKind::Comment));
    }

    #[test]
    fn grammars_are_panic_free_on_garbage() {
        // An editor is full of half-typed / invalid source — scanners must never panic.
        for g in [cpp(), csharp(), kotlin(), swift(), php(), ruby(), bash(), yaml(), toml()] {
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
    fn line_comment_metadata() {
        assert_eq!(Rust.line_comment(), Some("//"));
        assert_eq!(Python.line_comment(), Some("#"));
        assert_eq!(cpp().line_comment(), Some("//"));
        assert_eq!(bash().line_comment(), Some("#"));
        assert_eq!(Json.line_comment(), None);
    }
}
