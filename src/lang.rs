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

/// A syntax highlighter for one language. Implement this to add a grammar; the editor
/// only needs `highlight`. Returned tokens must be sorted by `start` and non-overlapping
/// (the bundled scanners guarantee this); gaps are rendered as plain text.
pub trait Language {
    /// A short display name (shown in the editor's status line).
    fn name(&self) -> &str;
    /// Tag the colored runs of `src`. Never panics, whatever `src` contains.
    fn highlight(&self, src: &str) -> Vec<Token>;
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
// A small shared char cursor
// ---------------------------------------------------------------------------

/// A byte-indexed cursor over `&str` that always advances by whole `char`s, so it is
/// safe on any UTF-8 input (a lesson learned the hard way: byte-slicing mid-codepoint
/// panics). `pos` is always on a char boundary.
struct Scan<'a> {
    s: &'a [u8],
    pos: usize,
}

impl<'a> Scan<'a> {
    fn new(src: &'a str) -> Self {
        Scan { s: src.as_bytes(), pos: 0 }
    }
    fn done(&self) -> bool {
        self.pos >= self.s.len()
    }
    fn peek(&self) -> u8 {
        if self.pos < self.s.len() { self.s[self.pos] } else { 0 }
    }
    fn peek2(&self) -> u8 {
        if self.pos + 1 < self.s.len() { self.s[self.pos + 1] } else { 0 }
    }
    /// Advance one UTF-8 char (never lands mid-codepoint).
    fn bump(&mut self) {
        if self.pos >= self.s.len() {
            return;
        }
        self.pos += 1;
        while self.pos < self.s.len() && (self.s[self.pos] & 0xC0) == 0x80 {
            self.pos += 1;
        }
    }
}

fn is_ident_start(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphabetic() || b >= 0x80
}
fn is_ident_continue(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphanumeric() || b >= 0x80
}

// ---------------------------------------------------------------------------
// Rust
// ---------------------------------------------------------------------------

/// A Rust highlighter.
pub struct Rust;

const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "fn",
    "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self",
    "Self", "static", "struct", "super", "trait", "type", "union", "unsafe", "use", "where", "while", "yield",
    "macro", "box",
];
const RUST_CONSTS: &[&str] = &["true", "false", "None", "Some", "Ok", "Err", "Option", "Result"];

impl Language for Rust {
    fn name(&self) -> &str {
        "Rust"
    }
    fn highlight(&self, src: &str) -> Vec<Token> {
        let mut out = Vec::new();
        let mut sc = Scan::new(src);
        while !sc.done() {
            let b = sc.peek();
            let start = sc.pos;
            match b {
                // whitespace — skip (rendered plain)
                b' ' | b'\t' | b'\r' | b'\n' => sc.bump(),
                // line comment // …  or  block comment /* … */
                b'/' if sc.peek2() == b'/' => {
                    while !sc.done() && sc.peek() != b'\n' {
                        sc.bump();
                    }
                    out.push(Token::new(start, sc.pos - start, TokenKind::Comment));
                }
                b'/' if sc.peek2() == b'*' => {
                    sc.bump();
                    sc.bump();
                    while !sc.done() && !(sc.peek() == b'*' && sc.peek2() == b'/') {
                        sc.bump();
                    }
                    sc.bump();
                    sc.bump();
                    out.push(Token::new(start, sc.pos - start, TokenKind::Comment));
                }
                // attribute  #[ … ]  or  #![ … ]
                b'#' if sc.peek2() == b'[' || sc.peek2() == b'!' => {
                    let mut depth = 0i32;
                    while !sc.done() {
                        let c = sc.peek();
                        sc.bump();
                        if c == b'[' {
                            depth += 1;
                        } else if c == b']' {
                            depth -= 1;
                            if depth <= 0 {
                                break;
                            }
                        } else if c == b'\n' && depth == 0 {
                            break;
                        }
                    }
                    out.push(Token::new(start, sc.pos - start, TokenKind::Attribute));
                }
                // string "…" (with \" escapes)
                b'"' => {
                    sc.bump();
                    while !sc.done() {
                        let c = sc.peek();
                        if c == b'\\' {
                            sc.bump();
                            sc.bump();
                            continue;
                        }
                        sc.bump();
                        if c == b'"' {
                            break;
                        }
                    }
                    out.push(Token::new(start, sc.pos - start, TokenKind::Str));
                }
                // char literal 'x' / '\n'  vs  lifetime 'a
                b'\'' => {
                    sc.bump();
                    if sc.peek() == b'\\' {
                        // escaped char literal
                        while !sc.done() && sc.peek() != b'\'' {
                            sc.bump();
                        }
                        sc.bump();
                        out.push(Token::new(start, sc.pos - start, TokenKind::Str));
                    } else if is_ident_start(sc.peek()) && sc.peek2() != b'\'' {
                        // lifetime 'ident (rendered like a keyword-ish type)
                        while is_ident_continue(sc.peek()) {
                            sc.bump();
                        }
                        out.push(Token::new(start, sc.pos - start, TokenKind::Type));
                    } else {
                        // simple char 'x'
                        sc.bump();
                        if sc.peek() == b'\'' {
                            sc.bump();
                        }
                        out.push(Token::new(start, sc.pos - start, TokenKind::Str));
                    }
                }
                // number
                b'0'..=b'9' => {
                    while !sc.done()
                        && (sc.peek().is_ascii_alphanumeric() || sc.peek() == b'.' || sc.peek() == b'_')
                    {
                        sc.bump();
                    }
                    out.push(Token::new(start, sc.pos - start, TokenKind::Number));
                }
                // identifier / keyword / type / macro / function
                _ if is_ident_start(b) => {
                    while is_ident_continue(sc.peek()) {
                        sc.bump();
                    }
                    let word = &src[start..sc.pos];
                    let kind = if RUST_KEYWORDS.contains(&word) {
                        TokenKind::Keyword
                    } else if RUST_CONSTS.contains(&word) {
                        TokenKind::Constant
                    } else if sc.peek() == b'!' {
                        sc.bump(); // include the ! in the macro token
                        TokenKind::Macro
                    } else if word.chars().next().is_some_and(|c| c.is_uppercase()) {
                        TokenKind::Type
                    } else if sc.peek() == b'(' {
                        TokenKind::Function
                    } else {
                        TokenKind::Plain
                    };
                    let len = sc.pos - start;
                    if kind != TokenKind::Plain {
                        out.push(Token::new(start, len, kind));
                    }
                }
                // operators / punctuation — group a run of ASCII punctuation
                _ if b.is_ascii_punctuation() => {
                    while !sc.done() && sc.peek().is_ascii_punctuation() && sc.peek() != b'"' && sc.peek() != b'\'' {
                        sc.bump();
                    }
                    out.push(Token::new(start, sc.pos - start, TokenKind::Punctuation));
                }
                _ => sc.bump(),
            }
            // Safety: guarantee forward progress on any weird byte.
            if sc.pos == start {
                sc.bump();
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------

/// A JSON highlighter (keys vs. string values, numbers, literals).
pub struct Json;

impl Language for Json {
    fn name(&self) -> &str {
        "JSON"
    }
    fn highlight(&self, src: &str) -> Vec<Token> {
        let mut out = Vec::new();
        let mut sc = Scan::new(src);
        while !sc.done() {
            let b = sc.peek();
            let start = sc.pos;
            match b {
                b'"' => {
                    sc.bump();
                    while !sc.done() {
                        let c = sc.peek();
                        if c == b'\\' {
                            sc.bump();
                            sc.bump();
                            continue;
                        }
                        sc.bump();
                        if c == b'"' {
                            break;
                        }
                    }
                    // A string immediately followed by `:` is a property key.
                    let mut look = sc.pos;
                    while look < sc.s.len() && (sc.s[look] == b' ' || sc.s[look] == b'\t') {
                        look += 1;
                    }
                    let kind =
                        if look < sc.s.len() && sc.s[look] == b':' { TokenKind::Property } else { TokenKind::Str };
                    out.push(Token::new(start, sc.pos - start, kind));
                }
                b'-' | b'0'..=b'9' => {
                    sc.bump();
                    while !sc.done()
                        && (sc.peek().is_ascii_alphanumeric()
                            || sc.peek() == b'.'
                            || sc.peek() == b'+'
                            || sc.peek() == b'-')
                    {
                        sc.bump();
                    }
                    out.push(Token::new(start, sc.pos - start, TokenKind::Number));
                }
                _ if is_ident_start(b) => {
                    while is_ident_continue(sc.peek()) {
                        sc.bump();
                    }
                    let word = &src[start..sc.pos];
                    if matches!(word, "true" | "false" | "null") {
                        out.push(Token::new(start, sc.pos - start, TokenKind::Constant));
                    }
                }
                _ => sc.bump(),
            }
            if sc.pos == start {
                sc.bump();
            }
        }
        out
    }
}
