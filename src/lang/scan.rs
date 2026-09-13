//! Shared scanner primitives for the hand-written grammars: a UTF-8-safe byte cursor that
//! always advances by whole `char`s (byte-slicing mid-codepoint panics), and the identifier
//! classifiers. Used by every bundled scanner in this module.

pub(crate) struct Scan<'a> {
    pub(crate) s: &'a [u8],
    pub(crate) pos: usize,
}

impl<'a> Scan<'a> {
    pub(crate) fn new(src: &'a str) -> Self {
        Scan {
            s: src.as_bytes(),
            pos: 0,
        }
    }
    pub(crate) fn done(&self) -> bool {
        self.pos >= self.s.len()
    }
    pub(crate) fn peek(&self) -> u8 {
        if self.pos < self.s.len() {
            self.s[self.pos]
        } else {
            0
        }
    }
    pub(crate) fn peek2(&self) -> u8 {
        if self.pos + 1 < self.s.len() {
            self.s[self.pos + 1]
        } else {
            0
        }
    }
    /// Advance one UTF-8 char (never lands mid-codepoint).
    pub(crate) fn bump(&mut self) {
        if self.pos >= self.s.len() {
            return;
        }
        self.pos += 1;
        while self.pos < self.s.len() && (self.s[self.pos] & 0xC0) == 0x80 {
            self.pos += 1;
        }
    }
}

pub(crate) fn is_ident_start(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphabetic() || b >= 0x80
}
pub(crate) fn is_ident_continue(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphanumeric() || b >= 0x80
}

