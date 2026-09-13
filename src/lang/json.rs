//! The JSON highlighter — a hand-written, panic-free scanner.

use super::scan::{Scan, is_ident_continue, is_ident_start};
use super::{Language, Token, TokenKind};


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
                    let kind = if look < sc.s.len() && sc.s[look] == b':' {
                        TokenKind::Property
                    } else {
                        TokenKind::Str
                    };
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

