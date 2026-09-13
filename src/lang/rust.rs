//! The Rust highlighter — a hand-written, panic-free scanner.

use super::scan::{Scan, is_ident_continue, is_ident_start};
use super::{Language, Token, TokenKind};


/// A Rust highlighter.
pub struct Rust;

const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
    "return", "self", "Self", "static", "struct", "super", "trait", "type", "union", "unsafe",
    "use", "where", "while", "yield", "macro", "box",
];
const RUST_CONSTS: &[&str] = &[
    "true", "false", "None", "Some", "Ok", "Err", "Option", "Result",
];

impl Language for Rust {
    fn line_comment(&self) -> Option<&str> {
        Some("//")
    }
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
                        && (sc.peek().is_ascii_alphanumeric()
                            || sc.peek() == b'.'
                            || sc.peek() == b'_')
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
                    while !sc.done()
                        && sc.peek().is_ascii_punctuation()
                        && sc.peek() != b'"'
                        && sc.peek() != b'\''
                    {
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

