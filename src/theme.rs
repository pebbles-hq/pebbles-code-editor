//! Editor color themes. A [`EditorTheme`] carries the chrome colors (background,
//! gutter, current-line, caret, selection) plus a color per [`TokenKind`]. Two are
//! bundled ([`EditorTheme::dark`], [`EditorTheme::light`]); build your own by filling
//! the struct — an IDE would expose a theme picker over these.

use pebbles::prelude::Color;

use crate::lang::TokenKind;

/// A complete editor color scheme.
#[derive(Clone)]
pub struct EditorTheme {
    /// The code area background.
    pub background: Color,
    /// The gutter (line-number column) background.
    pub gutter_bg: Color,
    /// Line-number text.
    pub gutter_fg: Color,
    /// Line-number text for the caret's line (brighter).
    pub gutter_active_fg: Color,
    /// The current-line highlight band.
    pub current_line: Color,
    /// Default text (identifiers, whitespace, unclassified tokens).
    pub foreground: Color,
    /// Per-token colors.
    pub keyword: Color,
    pub type_: Color,
    pub string: Color,
    pub number: Color,
    pub comment: Color,
    pub function: Color,
    pub macro_: Color,
    pub attribute: Color,
    pub constant: Color,
    pub punctuation: Color,
    pub property: Color,
}

impl EditorTheme {
    /// The color for a token kind.
    pub fn color(&self, kind: TokenKind) -> Color {
        match kind {
            TokenKind::Keyword => self.keyword,
            TokenKind::Type => self.type_,
            TokenKind::Str => self.string,
            TokenKind::Number => self.number,
            TokenKind::Comment => self.comment,
            TokenKind::Function => self.function,
            TokenKind::Macro => self.macro_,
            TokenKind::Attribute => self.attribute,
            TokenKind::Constant => self.constant,
            TokenKind::Punctuation => self.punctuation,
            TokenKind::Property => self.property,
            TokenKind::Plain => self.foreground,
        }
    }

    /// A calm dark theme (the default) — warm-neutral background, high-legibility hues.
    pub fn dark() -> Self {
        let c = |r, g, b| Color::from_rgba8(r, g, b, 0xFF);
        EditorTheme {
            background: c(0x0F, 0x14, 0x1F),
            gutter_bg: c(0x0F, 0x14, 0x1F),
            gutter_fg: c(0x44, 0x4C, 0x5C),
            gutter_active_fg: c(0x9A, 0xA4, 0xB8),
            current_line: Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x0C),
            foreground: c(0xD7, 0xDC, 0xE6),
            keyword: c(0xC5, 0x92, 0xF0),   // violet
            type_: c(0x6C, 0xD1, 0xC0),     // teal
            string: c(0x9E, 0xD8, 0x7A),    // green
            number: c(0xE6, 0xB4, 0x73),    // amber
            comment: c(0x5D, 0x67, 0x7A),   // muted slate
            function: c(0x76, 0xB2, 0xF0),  // blue
            macro_: c(0x6C, 0xD1, 0xC0),    // teal
            attribute: c(0xE6, 0xB4, 0x73), // amber
            constant: c(0xE9, 0x8A, 0x8A),  // soft red
            punctuation: c(0x9A, 0xA4, 0xB8),
            property: c(0x76, 0xB2, 0xF0),
        }
    }

    /// A clean light theme.
    pub fn light() -> Self {
        let c = |r, g, b| Color::from_rgba8(r, g, b, 0xFF);
        EditorTheme {
            background: c(0xFF, 0xFF, 0xFF),
            gutter_bg: c(0xFB, 0xFB, 0xFC),
            gutter_fg: c(0xB6, 0xBD, 0xC9),
            gutter_active_fg: c(0x5A, 0x63, 0x72),
            current_line: Color::from_rgba8(0x00, 0x00, 0x00, 0x08),
            foreground: c(0x24, 0x29, 0x33),
            keyword: c(0x8A, 0x3F, 0xC8),
            type_: c(0x0F, 0x86, 0x8E),
            string: c(0x3E, 0x8A, 0x3E),
            number: c(0xB0, 0x6A, 0x14),
            comment: c(0x9A, 0xA1, 0xAD),
            function: c(0x24, 0x5F, 0xC2),
            macro_: c(0x0F, 0x86, 0x8E),
            attribute: c(0xB0, 0x6A, 0x14),
            constant: c(0xC0, 0x3A, 0x4B),
            punctuation: c(0x5A, 0x63, 0x72),
            property: c(0x24, 0x5F, 0xC2),
        }
    }
}
