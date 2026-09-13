//! Editor theming & styling. A [`EditorTheme`] carries the **chrome** slots (backgrounds,
//! gutter, caret, selection, borders, scrollbar, diagnostics, …) and a [`HighlightStyle`]
//! — the syntax palette, one [`TokenStyle`] (color + bold/italic/underline) per
//! [`TokenKind`]. The two are separable: swap `theme.syntax` alone to re-color code without
//! touching the UI chrome. Two of each are bundled ([`EditorTheme::dark`]/[`light`],
//! [`HighlightStyle::dark`]/[`light`]); build your own by filling the struct, or compose one
//! over a base with an extension's `theme` transform.

use pebbles::prelude::Color;

use crate::lang::TokenKind;

/// The visual style of one syntax token: a color plus optional weight/slant/underline.
/// This is what makes styling "not just colors" — a theme can render comments italic,
/// keywords bold, or deprecated identifiers underlined.
#[derive(Clone, Copy, Debug)]
pub struct TokenStyle {
    /// The token's text color.
    pub color: Color,
    /// Render bold.
    pub bold: bool,
    /// Render italic.
    pub italic: bool,
    /// Underline the token.
    pub underline: bool,
}

impl TokenStyle {
    /// A plain colored token (no weight/slant/underline).
    pub const fn color(color: Color) -> Self {
        TokenStyle {
            color,
            bold: false,
            italic: false,
            underline: false,
        }
    }
    /// Render this token bold.
    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }
    /// Render this token italic.
    pub fn italic(mut self) -> Self {
        self.italic = true;
        self
    }
    /// Underline this token.
    pub fn underline(mut self) -> Self {
        self.underline = true;
        self
    }
}

/// The syntax palette: a [`TokenStyle`] per [`TokenKind`]. Independent of the UI chrome in
/// [`EditorTheme`], so an IDE can offer a "syntax theme" picker separate from light/dark.
#[derive(Clone)]
pub struct HighlightStyle {
    /// Language keywords.
    pub keyword: TokenStyle,
    /// Type / class names.
    pub type_: TokenStyle,
    /// String and char literals.
    pub string: TokenStyle,
    /// Numeric literals.
    pub number: TokenStyle,
    /// Comments.
    pub comment: TokenStyle,
    /// Function / method names.
    pub function: TokenStyle,
    /// Macros.
    pub macro_: TokenStyle,
    /// Attributes / annotations / decorators.
    pub attribute: TokenStyle,
    /// Constants.
    pub constant: TokenStyle,
    /// Punctuation and operators.
    pub punctuation: TokenStyle,
    /// Object/record properties.
    pub property: TokenStyle,
    /// Identifiers / whitespace / unclassified tokens.
    pub plain: TokenStyle,
}

impl HighlightStyle {
    /// The full style for a token kind.
    pub fn style(&self, kind: TokenKind) -> TokenStyle {
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
            TokenKind::Plain => self.plain,
        }
    }
    /// Just the color for a token kind.
    pub fn color(&self, kind: TokenKind) -> Color {
        self.style(kind).color
    }

    /// The dark syntax palette (comments italic).
    pub fn dark() -> Self {
        let c = |r, g, b| Color::from_rgba8(r, g, b, 0xFF);
        let s = TokenStyle::color;
        HighlightStyle {
            keyword: s(c(0xC5, 0x92, 0xF0)).bold(),   // violet, bold
            type_: s(c(0x6C, 0xD1, 0xC0)),            // teal
            string: s(c(0x9E, 0xD8, 0x7A)),           // green
            number: s(c(0xE6, 0xB4, 0x73)),           // amber
            comment: s(c(0x5D, 0x67, 0x7A)).italic(), // muted slate, italic
            function: s(c(0x76, 0xB2, 0xF0)),         // blue
            macro_: s(c(0x6C, 0xD1, 0xC0)),           // teal
            attribute: s(c(0xE6, 0xB4, 0x73)),        // amber
            constant: s(c(0xE9, 0x8A, 0x8A)),         // soft red
            punctuation: s(c(0x9A, 0xA4, 0xB8)),
            property: s(c(0x76, 0xB2, 0xF0)),
            plain: s(c(0xD7, 0xDC, 0xE6)),
        }
    }

    /// The light syntax palette (comments italic).
    pub fn light() -> Self {
        let c = |r, g, b| Color::from_rgba8(r, g, b, 0xFF);
        let s = TokenStyle::color;
        HighlightStyle {
            keyword: s(c(0x8A, 0x3F, 0xC8)).bold(),
            type_: s(c(0x0F, 0x86, 0x8E)),
            string: s(c(0x3E, 0x8A, 0x3E)),
            number: s(c(0xB0, 0x6A, 0x14)),
            comment: s(c(0x9A, 0xA1, 0xAD)).italic(),
            function: s(c(0x24, 0x5F, 0xC2)),
            macro_: s(c(0x0F, 0x86, 0x8E)),
            attribute: s(c(0xB0, 0x6A, 0x14)),
            constant: s(c(0xC0, 0x3A, 0x4B)),
            punctuation: s(c(0x5A, 0x63, 0x72)),
            property: s(c(0x24, 0x5F, 0xC2)),
            plain: s(c(0x24, 0x29, 0x33)),
        }
    }
}

/// A complete editor color scheme: chrome slots + a swappable [`HighlightStyle`]. Every
/// visual surface has a slot, so an editor built on this is fully stylable.
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
    /// The text caret.
    pub caret: Color,
    /// The selection highlight (editor focused).
    pub selection: Color,
    /// The selection highlight when the editor is not focused.
    pub selection_inactive: Color,
    /// Indent-guide vertical lines (and their brighter active variant on the caret's block).
    pub indent_guide: Color,
    /// The brighter indent guide on the caret's indentation block.
    pub indent_guide_active: Color,
    /// Whitespace/EOL markers (middots, tab arrows, ¶) when rendering is on.
    pub whitespace: Color,
    /// Vertical ruler / print-margin lines.
    pub ruler: Color,
    /// Highlight box drawn around a bracket and its match.
    pub matching_bracket: Color,
    /// Default text (identifiers, unclassified tokens) — mirrors `syntax.plain.color`.
    pub foreground: Color,
    /// Borders on floating surfaces (find bar, completion, palette, tooltip, cards).
    pub border: Color,
    /// Background of floating surfaces (find bar, completion, palette, tooltip).
    pub overlay_bg: Color,
    /// Secondary / de-emphasized text (completion detail, "no matches", inlay hints).
    pub muted: Color,
    /// UI accent (selected-item marks, completion kind icon).
    pub accent: Color,
    /// Search-match highlight (all matches) and the current match (stronger).
    pub search_match: Color,
    /// The current search match (stronger highlight).
    pub search_match_current: Color,
    /// Scrollbar thumb (idle) and hovered/active.
    pub scrollbar: Color,
    /// The scrollbar thumb when hovered/active.
    pub scrollbar_active: Color,
    /// Diagnostic underline + gutter-dot colors by severity.
    pub diag_error: Color,
    /// Warning diagnostic color.
    pub diag_warning: Color,
    /// Info diagnostic color.
    pub diag_info: Color,
    /// Hint diagnostic color.
    pub diag_hint: Color,
    /// Inline-diff band for an added line.
    pub diff_added: Color,
    /// Inline-diff marker for a removed line.
    pub diff_removed: Color,
    /// The syntax palette (swappable independently of the chrome above).
    pub syntax: HighlightStyle,
}

impl EditorTheme {
    /// The color for a token kind (delegates to [`syntax`](Self::syntax)).
    pub fn color(&self, kind: TokenKind) -> Color {
        self.syntax.color(kind)
    }
    /// The full style for a token kind (delegates to [`syntax`](Self::syntax)).
    pub fn style(&self, kind: TokenKind) -> TokenStyle {
        self.syntax.style(kind)
    }

    /// A calm dark theme (the default) — warm-neutral background, high-legibility hues.
    pub fn dark() -> Self {
        let c = |r, g, b| Color::from_rgba8(r, g, b, 0xFF);
        let a = Color::from_rgba8;
        EditorTheme {
            // A simple neutral dark, matching the framework's default dark background.
            background: c(0x0A, 0x0A, 0x0B),
            gutter_bg: c(0x0A, 0x0A, 0x0B),
            gutter_fg: c(0x44, 0x4C, 0x5C),
            gutter_active_fg: c(0x9A, 0xA4, 0xB8),
            current_line: a(0xFF, 0xFF, 0xFF, 0x0C),
            caret: c(0x8A, 0xB4, 0xF8),
            selection: a(0x3D, 0x59, 0x8F, 0x66),
            selection_inactive: a(0x3D, 0x59, 0x8F, 0x33),
            indent_guide: a(0xFF, 0xFF, 0xFF, 0x12),
            indent_guide_active: a(0xFF, 0xFF, 0xFF, 0x30),
            whitespace: a(0xFF, 0xFF, 0xFF, 0x24),
            ruler: a(0xFF, 0xFF, 0xFF, 0x10),
            matching_bracket: a(0x8A, 0xB4, 0xF8, 0x4D),
            foreground: c(0xD7, 0xDC, 0xE6),
            border: a(0x9A, 0xA4, 0xB8, 0x50),
            overlay_bg: c(0x18, 0x18, 0x1B),
            muted: c(0x5D, 0x67, 0x7A),
            accent: c(0x76, 0xB2, 0xF0),
            search_match: a(0xE6, 0xB4, 0x73, 0x47),
            search_match_current: a(0xE6, 0xB4, 0x73, 0x8C),
            scrollbar: a(0xFF, 0xFF, 0xFF, 0x1E),
            scrollbar_active: a(0xFF, 0xFF, 0xFF, 0x3C),
            diag_error: c(0xE9, 0x8A, 0x8A),
            diag_warning: c(0xE6, 0xB4, 0x73),
            diag_info: c(0x76, 0xB2, 0xF0),
            diag_hint: c(0x5D, 0x67, 0x7A),
            diff_added: a(0x6C, 0xD1, 0x8A, 0x22),
            diff_removed: a(0xE9, 0x6A, 0x6A, 0xC0),
            syntax: HighlightStyle::dark(),
        }
    }

    /// A clean light theme.
    pub fn light() -> Self {
        let c = |r, g, b| Color::from_rgba8(r, g, b, 0xFF);
        let a = Color::from_rgba8;
        EditorTheme {
            background: c(0xFF, 0xFF, 0xFF),
            gutter_bg: c(0xFB, 0xFB, 0xFC),
            gutter_fg: c(0xB6, 0xBD, 0xC9),
            gutter_active_fg: c(0x5A, 0x63, 0x72),
            current_line: a(0x00, 0x00, 0x00, 0x08),
            caret: c(0x24, 0x5F, 0xC2),
            selection: a(0x2F, 0x62, 0xE0, 0x33),
            selection_inactive: a(0x2F, 0x62, 0xE0, 0x1A),
            indent_guide: a(0x00, 0x00, 0x00, 0x12),
            indent_guide_active: a(0x00, 0x00, 0x00, 0x2E),
            whitespace: a(0x00, 0x00, 0x00, 0x24),
            ruler: a(0x00, 0x00, 0x00, 0x0E),
            matching_bracket: a(0x24, 0x5F, 0xC2, 0x3D),
            foreground: c(0x24, 0x29, 0x33),
            border: a(0x5A, 0x63, 0x72, 0x40),
            overlay_bg: c(0xFF, 0xFF, 0xFF),
            muted: c(0x9A, 0xA1, 0xAD),
            accent: c(0x24, 0x5F, 0xC2),
            search_match: a(0xB0, 0x6A, 0x14, 0x33),
            search_match_current: a(0xB0, 0x6A, 0x14, 0x66),
            scrollbar: a(0x00, 0x00, 0x00, 0x22),
            scrollbar_active: a(0x00, 0x00, 0x00, 0x44),
            diag_error: c(0xC0, 0x3A, 0x4B),
            diag_warning: c(0xB0, 0x6A, 0x14),
            diag_info: c(0x24, 0x5F, 0xC2),
            diag_hint: c(0x9A, 0xA1, 0xAD),
            diff_added: a(0x3E, 0x8A, 0x3E, 0x1F),
            diff_removed: a(0xC0, 0x3A, 0x4B, 0xB0),
            syntax: HighlightStyle::light(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::TokenKind;

    #[test]
    fn token_style_builders_compose() {
        let s = TokenStyle::color(Color::from_rgba8(1, 2, 3, 255))
            .bold()
            .italic()
            .underline();
        assert!(s.bold && s.italic && s.underline);
    }

    #[test]
    fn dark_syntax_carries_weight_and_slant() {
        let h = HighlightStyle::dark();
        assert!(h.style(TokenKind::Keyword).bold, "keywords are bold");
        assert!(h.style(TokenKind::Comment).italic, "comments are italic");
        assert!(!h.style(TokenKind::Plain).bold, "plain text is neither");
    }

    #[test]
    fn theme_delegates_to_syntax() {
        let t = EditorTheme::dark();
        assert_eq!(
            t.color(TokenKind::Keyword),
            t.syntax.color(TokenKind::Keyword)
        );
        assert_eq!(t.style(TokenKind::Comment).italic, t.syntax.comment.italic);
    }

    #[test]
    fn syntax_swaps_independently_of_chrome() {
        // Dark chrome + light syntax palette: the two are separable.
        let mut t = EditorTheme::dark();
        let dark_bg = t.background;
        t.syntax = HighlightStyle::light();
        assert_eq!(
            t.color(TokenKind::Keyword),
            HighlightStyle::light().color(TokenKind::Keyword)
        );
        assert_eq!(
            t.background, dark_bg,
            "swapping syntax left the chrome untouched"
        );
    }
}
