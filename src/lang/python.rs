//! The Python highlighter — a hand-written, panic-free scanner.

use super::scan::{Scan, is_ident_continue, is_ident_start};
use super::{Language, Token, TokenKind};

/// A Python highlighter.
pub struct Python;

const PY_KEYWORDS: &[&str] = &[
    "def", "class", "return", "if", "elif", "else", "for", "while", "break", "continue", "pass",
    "import", "from", "as", "with", "try", "except", "finally", "raise", "yield", "lambda",
    "global", "nonlocal", "del", "assert", "async", "await", "in", "is", "not", "and", "or",
    "match", "case",
];
const PY_CONSTS: &[&str] = &["True", "False", "None", "self", "cls"];

impl Language for Python {
    fn line_comment(&self) -> Option<&str> {
        Some("#")
    }
    fn name(&self) -> &str {
        "Python"
    }
    fn highlight(&self, src: &str) -> Vec<Token> {
        let mut out = Vec::new();
        let mut sc = Scan::new(src);
        while !sc.done() {
            let b = sc.peek();
            let start = sc.pos;
            match b {
                b' ' | b'\t' | b'\r' | b'\n' => sc.bump(),
                b'#' => {
                    while !sc.done() && sc.peek() != b'\n' {
                        sc.bump();
                    }
                    out.push(Token::new(start, sc.pos - start, TokenKind::Comment));
                }
                b'@' => {
                    sc.bump();
                    while is_ident_continue(sc.peek()) {
                        sc.bump();
                    }
                    out.push(Token::new(start, sc.pos - start, TokenKind::Attribute));
                }
                b'"' | b'\'' => {
                    let q = b;
                    let triple =
                        sc.peek2() == q && (sc.pos + 2 < sc.s.len() && sc.s[sc.pos + 2] == q);
                    if triple {
                        sc.bump();
                        sc.bump();
                        sc.bump();
                        while !sc.done()
                            && !(sc.peek() == q
                                && sc.peek2() == q
                                && sc.pos + 2 < sc.s.len()
                                && sc.s[sc.pos + 2] == q)
                        {
                            sc.bump();
                        }
                        sc.bump();
                        sc.bump();
                        sc.bump();
                    } else {
                        sc.bump();
                        while !sc.done() {
                            let c = sc.peek();
                            if c == b'\\' {
                                sc.bump();
                                sc.bump();
                                continue;
                            }
                            sc.bump();
                            if c == q {
                                break;
                            }
                        }
                    }
                    out.push(Token::new(start, sc.pos - start, TokenKind::Str));
                }
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
                _ if is_ident_start(b) => {
                    while is_ident_continue(sc.peek()) {
                        sc.bump();
                    }
                    let word = &src[start..sc.pos];
                    let kind = if PY_KEYWORDS.contains(&word) {
                        TokenKind::Keyword
                    } else if PY_CONSTS.contains(&word) {
                        TokenKind::Constant
                    } else if word.chars().next().is_some_and(|c| c.is_uppercase()) {
                        TokenKind::Type
                    } else if sc.peek() == b'(' {
                        TokenKind::Function
                    } else {
                        TokenKind::Plain
                    };
                    if kind != TokenKind::Plain {
                        out.push(Token::new(start, sc.pos - start, kind));
                    }
                }
                _ if b.is_ascii_punctuation() => {
                    while !sc.done()
                        && sc.peek().is_ascii_punctuation()
                        && !matches!(sc.peek(), b'"' | b'\'')
                    {
                        sc.bump();
                    }
                    out.push(Token::new(start, sc.pos - start, TokenKind::Punctuation));
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
