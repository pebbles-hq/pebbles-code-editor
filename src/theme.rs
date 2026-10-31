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
    /// Escape sequences in strings / regexes.
    pub escape: TokenStyle,
    /// Markup tag names (HTML/JSX/XML).
    pub tag: TokenStyle,
    /// Module / namespace names.
    pub namespace: TokenStyle,
    /// Markdown headings.
    pub heading: TokenStyle,
    /// Links / URLs.
    pub link: TokenStyle,
    /// Emphasised markup (bold / italic).
    pub emphasis: TokenStyle,
    /// Bracket-pair colorization palette, cycled by nesting depth (VS Code
    /// `editorBracketHighlight.foreground1..6`).
    pub brackets: Vec<TokenStyle>,
    /// An unmatched closing bracket (`editorBracketHighlight.unexpectedBracket.foreground`).
    pub bracket_unmatched: TokenStyle,
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
            TokenKind::Escape => self.escape,
            TokenKind::Tag => self.tag,
            TokenKind::Namespace => self.namespace,
            TokenKind::Heading => self.heading,
            TokenKind::Link => self.link,
            TokenKind::Emphasis => self.emphasis,
            TokenKind::Bracket(d) => match self.brackets.len() {
                0 => self.punctuation,
                n => self.brackets[d as usize % n],
            },
            TokenKind::BracketUnmatched => self.bracket_unmatched,
            TokenKind::Plain => self.plain,
        }
    }
    /// Just the color for a token kind.
    pub fn color(&self, kind: TokenKind) -> Color {
        self.style(kind).color
    }

    /// The dark syntax palette: Zed One Dark hues, regular weight throughout (neither VS Code
    /// Dark Modern nor Zed One Dark bolds keywords or slants comments). Every code token is
    /// at least 4.5:1 on the dark editor background.
    pub fn dark() -> Self {
        let c = |r, g, b| Color::from_rgba8(r, g, b, 0xFF);
        let s = TokenStyle::color;
        HighlightStyle {
            keyword: s(c(0xB4, 0x77, 0xCF)),     // violet
            type_: s(c(0x6E, 0xB4, 0xBF)),       // teal
            string: s(c(0xA1, 0xC1, 0x81)),      // green
            number: s(c(0xBF, 0x95, 0x6A)),      // amber
            comment: s(c(0x80, 0x87, 0x93)),     // muted grey, upright
            function: s(c(0x73, 0xAD, 0xE9)),    // blue
            macro_: s(c(0x6E, 0xB4, 0xBF)),      // teal
            attribute: s(c(0x74, 0xAD, 0xE8)),   // blue
            constant: s(c(0xDF, 0xC1, 0x84)),    // gold
            punctuation: s(c(0xAC, 0xB2, 0xBE)), // light grey
            property: s(c(0xD0, 0x72, 0x77)),    // soft red
            escape: s(c(0xBF, 0x95, 0x6A)),
            tag: s(c(0x74, 0xAD, 0xE8)),
            namespace: s(c(0xDC, 0xE0, 0xE5)),
            heading: s(c(0x73, 0xAD, 0xE9)).bold(),
            link: s(c(0x6E, 0xB4, 0xBF)).underline(),
            emphasis: s(c(0xDC, 0xE0, 0xE5)).italic(),
            // VS Code Dark Modern bracket colours: gold, orchid, sky.
            brackets: vec![
                s(c(0xFF, 0xD7, 0x00)),
                s(c(0xDA, 0x70, 0xD6)),
                s(c(0x17, 0x9F, 0xFF)),
            ],
            bracket_unmatched: s(c(0xFF, 0x12, 0x12)),
            plain: s(c(0xDC, 0xE0, 0xE5)),
        }
    }

    /// The light syntax palette: VS Code Light Modern hues, regular weight throughout. Every
    /// code token is at least 4.5:1 on white.
    pub fn light() -> Self {
        let c = |r, g, b| Color::from_rgba8(r, g, b, 0xFF);
        let s = TokenStyle::color;
        HighlightStyle {
            keyword: s(c(0xAF, 0x00, 0xDB)),
            type_: s(c(0x26, 0x7F, 0x99)),
            string: s(c(0xA3, 0x15, 0x15)),
            number: s(c(0x09, 0x86, 0x58)),
            comment: s(c(0x00, 0x80, 0x00)),
            function: s(c(0x79, 0x5E, 0x26)),
            macro_: s(c(0x26, 0x7F, 0x99)),
            attribute: s(c(0x79, 0x5E, 0x26)),
            constant: s(c(0x00, 0x70, 0xC1)),
            punctuation: s(c(0x3B, 0x3B, 0x3B)),
            property: s(c(0x00, 0x10, 0x80)),
            escape: s(c(0xEE, 0x00, 0x00)),
            tag: s(c(0x80, 0x00, 0x00)),
            namespace: s(c(0x26, 0x7F, 0x99)),
            heading: s(c(0x00, 0x00, 0x80)).bold(),
            link: s(c(0x26, 0x7F, 0x99)).underline(),
            emphasis: s(c(0x3B, 0x3B, 0x3B)).italic(),
            // VS Code Light Modern bracket colours (the green darkened to 4.5:1).
            brackets: vec![
                s(c(0x04, 0x31, 0xFA)),
                s(c(0x2A, 0x7F, 0x2A)),
                s(c(0x7B, 0x38, 0x14)),
            ],
            bracket_unmatched: s(c(0xCD, 0x31, 0x31)),
            plain: s(c(0x3B, 0x3B, 0x3B)),
        }
    }

    /// VS Code "Dark High Contrast" syntax colours (every token >= 7:1 on black).
    pub fn high_contrast_dark() -> Self {
        let c = |r, g, b| Color::from_rgba8(r, g, b, 0xFF);
        let s = TokenStyle::color;
        HighlightStyle {
            keyword: s(c(0x56, 0x9C, 0xD6)).bold(),
            type_: s(c(0x4E, 0xC9, 0xB0)),
            string: s(c(0xCE, 0x91, 0x78)),
            number: s(c(0xB5, 0xCE, 0xA8)),
            comment: s(c(0x7C, 0xA6, 0x68)).italic(),
            function: s(c(0xDC, 0xDC, 0xAA)),
            macro_: s(c(0x4E, 0xC9, 0xB0)),
            attribute: s(c(0xDC, 0xDC, 0xAA)),
            constant: s(c(0x9C, 0xDC, 0xFE)),
            punctuation: s(c(0xFF, 0xFF, 0xFF)),
            property: s(c(0x9C, 0xDC, 0xFE)),
            escape: s(c(0xD7, 0xBA, 0x7D)),
            tag: s(c(0x56, 0x9C, 0xD6)),
            namespace: s(c(0x4E, 0xC9, 0xB0)),
            heading: s(c(0x6F, 0xC3, 0xDF)).bold(),
            link: s(c(0x3E, 0xA8, 0xFF)).underline(),
            emphasis: s(c(0xFF, 0xFF, 0xFF)).italic(),
            brackets: vec![
                s(c(0xFF, 0xD7, 0x00)),
                s(c(0xDA, 0x70, 0xD6)),
                s(c(0x87, 0xCE, 0xFA)),
            ],
            bracket_unmatched: s(c(0xFF, 0x32, 0x32)),
            plain: s(c(0xFF, 0xFF, 0xFF)),
        }
    }

    /// VS Code "Light High Contrast" syntax colours (every token >= 7:1 on white).
    pub fn high_contrast_light() -> Self {
        let c = |r, g, b| Color::from_rgba8(r, g, b, 0xFF);
        let s = TokenStyle::color;
        HighlightStyle {
            keyword: s(c(0x0F, 0x48, 0x94)).bold(),
            type_: s(c(0x18, 0x5E, 0x73)),
            string: s(c(0x0F, 0x4A, 0x85)),
            number: s(c(0x05, 0x57, 0x4A)).bold(),
            comment: s(c(0x51, 0x5C, 0x40)).italic(),
            function: s(c(0x5E, 0x2C, 0xBC)),
            macro_: s(c(0x18, 0x5E, 0x73)),
            attribute: s(c(0x26, 0x4F, 0x78)),
            constant: s(c(0x00, 0x1A, 0x80)),
            punctuation: s(c(0x29, 0x29, 0x29)),
            property: s(c(0x00, 0x1A, 0x80)),
            escape: s(c(0x8B, 0x00, 0x00)),
            tag: s(c(0x0F, 0x48, 0x94)),
            namespace: s(c(0x18, 0x5E, 0x73)),
            heading: s(c(0x0F, 0x48, 0x94)).bold(),
            link: s(c(0x0F, 0x4A, 0x85)).underline(),
            emphasis: s(c(0x29, 0x29, 0x29)).italic(),
            brackets: vec![
                s(c(0x0F, 0x48, 0x94)),
                s(c(0x0E, 0x5E, 0x0E)),
                s(c(0x7B, 0x38, 0x14)),
            ],
            bracket_unmatched: s(c(0xB5, 0x20, 0x0D)),
            plain: s(c(0x29, 0x29, 0x29)),
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
    /// Diff editor: background of removed lines (`diffEditor.removedLineBackground`).
    pub diff_removed_line: Color,
    /// Diff editor: inserted words (`diffEditor.insertedTextBackground`).
    pub diff_inserted_text: Color,
    /// Diff editor: removed words (`diffEditor.removedTextBackground`).
    pub diff_removed_text: Color,
    /// Gutter change bar: added lines (VS Code `editorGutter.addedBackground`).
    pub change_added: Color,
    /// Gutter change bar: modified lines (`editorGutter.modifiedBackground`).
    pub change_modified: Color,
    /// Gutter change marker: deleted lines (`editorGutter.deletedBackground`).
    pub change_deleted: Color,
    /// Overview-ruler cursor mark (`editorOverviewRuler.*` marks reuse the matching slots).
    pub overview_cursor: Color,
    /// Box drawn around invisible / ambiguous Unicode characters (`editorUnicodeHighlight.border`).
    pub unicode_highlight: Color,
    /// Bracket-pair guide lines (`editorBracketPairGuide.activeBackground1`).
    pub bracket_guide: Color,
    /// Ghost text of inline suggestions / suggestion preview (`editorGhostText.foreground`).
    pub ghost_text: Color,
    /// Background of the folded-region placeholder badge (`editor.foldPlaceholderForeground`).
    pub fold_badge: Color,
    /// Breakpoint dot in the glyph margin (`debugIcon.breakpointForeground`).
    pub breakpoint: Color,
    /// Outline of linked-editing ranges (`editor.linkedEditingBackground`).
    pub linked_editing: Color,
    /// The syntax palette (swappable independently of the chrome above).
    pub syntax: HighlightStyle,
    /// Base font size of the editor's own UI (status bar, find bar, command
    /// palette); the code text size is set on the editor builder (`.font_size(..)`).
    pub ui_font_size: f64,
    /// Font family of the editor's UI text (`None` = the app theme's for the
    /// chrome, the bundled monospace for the command palette).
    pub ui_font_family: Option<String>,
    /// Corner radius of floating surfaces (find bar, completion, palette, tooltips).
    pub overlay_radius: f64,
}

impl EditorTheme {
    /// UI text at `delta` px from [`ui_font_size`](Self::ui_font_size), in
    /// [`ui_font_family`](Self::ui_font_family) when set.
    pub(crate) fn ui_text(&self, s: impl Into<String>, delta: f64) -> pebbles::prelude::Text {
        let t = pebbles::prelude::text(s.into()).size((self.ui_font_size + delta) as f32);
        match &self.ui_font_family {
            Some(f) => t.font_family(f.clone()),
            None => t,
        }
    }

    /// The color for a token kind (delegates to [`syntax`](Self::syntax)).
    pub fn color(&self, kind: TokenKind) -> Color {
        self.syntax.color(kind)
    }
    /// The full style for a token kind (delegates to [`syntax`](Self::syntax)).
    pub fn style(&self, kind: TokenKind) -> TokenStyle {
        self.syntax.style(kind)
    }

    /// The editor theme matching the app's current Pebbles theme: [`light`](Self::light) or
    /// [`dark`](Self::dark) by `theme().dark`. This is the editor's default; calling it inside
    /// a build subscribes that build to light/dark switches.
    pub fn from_theme() -> Self {
        if pebbles::prelude::theme().dark {
            Self::dark()
        } else {
            Self::light()
        }
    }

    /// A calm dark theme — warm-neutral background, high-legibility hues.
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
            foreground: c(0xDC, 0xE0, 0xE5),
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
            change_added: a(0x2E, 0xA0, 0x43, 0xFF),
            diff_removed_line: a(0xFF, 0x00, 0x00, 0x26),
            diff_inserted_text: a(0x9C, 0xCC, 0x2C, 0x40),
            diff_removed_text: a(0xFF, 0x00, 0x00, 0x40),
            change_modified: a(0x00, 0x78, 0xD4, 0xFF),
            change_deleted: a(0xF8, 0x51, 0x49, 0xFF),
            overview_cursor: a(0xA0, 0xA0, 0xA0, 0xCC),
            unicode_highlight: a(0xCD, 0x9D, 0x31, 0xFF),
            bracket_guide: a(0xA0, 0xA0, 0xA0, 0x80),
            ghost_text: a(0xFF, 0xFF, 0xFF, 0x56),
            fold_badge: a(0x76, 0xB2, 0xF0, 0x33),
            linked_editing: a(0xFF, 0x00, 0x00, 0x4D),
            breakpoint: c(0xE5, 0x14, 0x00),
            syntax: HighlightStyle::dark(),
            ui_font_size: 12.5,
            ui_font_family: None,
            overlay_radius: 8.0,
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
            foreground: c(0x3B, 0x3B, 0x3B),
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
            change_added: a(0x2E, 0xA0, 0x43, 0xFF),
            diff_removed_line: a(0xFF, 0x00, 0x00, 0x26),
            diff_inserted_text: a(0x9C, 0xCC, 0x2C, 0x40),
            diff_removed_text: a(0xFF, 0x00, 0x00, 0x40),
            change_modified: a(0x00, 0x5F, 0xB8, 0xFF),
            change_deleted: a(0xF8, 0x51, 0x49, 0xFF),
            overview_cursor: a(0x60, 0x60, 0x60, 0xCC),
            unicode_highlight: a(0xBF, 0x88, 0x03, 0xFF),
            bracket_guide: a(0x60, 0x60, 0x60, 0x80),
            ghost_text: a(0x00, 0x00, 0x00, 0x77),
            fold_badge: a(0x24, 0x5F, 0xC2, 0x26),
            linked_editing: a(0xFF, 0x00, 0x00, 0x30),
            breakpoint: c(0xE5, 0x14, 0x00),
            syntax: HighlightStyle::light(),
            ui_font_size: 12.5,
            ui_font_family: None,
            overlay_radius: 8.0,
        }
    }

    /// VS Code "Dark High Contrast": black background, white text, solid borders
    /// (`contrastBorder` #6FC3DF, `contrastActiveBorder` #F38518).
    pub fn high_contrast_dark() -> Self {
        let c = |r, g, b| Color::from_rgba8(r, g, b, 0xFF);
        let a = Color::from_rgba8;
        EditorTheme {
            background: c(0, 0, 0),
            gutter_bg: c(0, 0, 0),
            gutter_fg: c(0xFF, 0xFF, 0xFF),
            gutter_active_fg: c(0xF3, 0x85, 0x18),
            current_line: a(0xF3, 0x85, 0x18, 0x30),
            caret: c(0xFF, 0xFF, 0xFF),
            selection: a(0xFF, 0xFF, 0xFF, 0x66),
            selection_inactive: a(0xFF, 0xFF, 0xFF, 0x40),
            indent_guide: c(0x7C, 0x7C, 0x7C),
            indent_guide_active: c(0xFF, 0xFF, 0xFF),
            whitespace: c(0x7C, 0x7C, 0x7C),
            ruler: c(0xFF, 0xFF, 0xFF),
            matching_bracket: a(0x6F, 0xC3, 0xDF, 0xFF),
            foreground: c(0xFF, 0xFF, 0xFF),
            border: c(0x6F, 0xC3, 0xDF),
            overlay_bg: c(0, 0, 0),
            muted: c(0xD0, 0xD0, 0xD0),
            accent: c(0xF3, 0x85, 0x18),
            search_match: a(0xF3, 0x85, 0x18, 0x60),
            search_match_current: a(0xF3, 0x85, 0x18, 0xB0),
            scrollbar: a(0x6F, 0xC3, 0xDF, 0x99),
            scrollbar_active: c(0x6F, 0xC3, 0xDF),
            diag_error: c(0xF4, 0x87, 0x71),
            diag_warning: c(0xFF, 0xD3, 0x70),
            diag_info: c(0x6F, 0xC3, 0xDF),
            diag_hint: c(0xEE, 0xEE, 0xEE),
            diff_added: a(0x33, 0xFF, 0x2E, 0x33),
            diff_removed: a(0xFF, 0x00, 0x8F, 0xC0),
            change_added: c(0x48, 0x79, 0x2C),
            diff_removed_line: a(0xFF, 0x00, 0x00, 0x26),
            diff_inserted_text: a(0x9C, 0xCC, 0x2C, 0x40),
            diff_removed_text: a(0xFF, 0x00, 0x00, 0x40),
            change_modified: c(0x1B, 0x81, 0xA8),
            change_deleted: c(0xF4, 0x87, 0x71),
            overview_cursor: c(0xFF, 0xFF, 0xFF),
            unicode_highlight: c(0xFF, 0xD7, 0x00),
            bracket_guide: c(0xFF, 0xFF, 0xFF),
            ghost_text: a(0xFF, 0xFF, 0xFF, 0x99),
            fold_badge: a(0x6F, 0xC3, 0xDF, 0x66),
            linked_editing: c(0xF3, 0x85, 0x18),
            breakpoint: c(0xE5, 0x14, 0x00),
            syntax: HighlightStyle::high_contrast_dark(),
            ui_font_size: 12.5,
            ui_font_family: None,
            overlay_radius: 0.0,
        }
    }

    /// VS Code "Light High Contrast": white background, near-black text, solid borders
    /// (`contrastBorder` #0F4A85, `contrastActiveBorder` #006BBD).
    pub fn high_contrast_light() -> Self {
        let c = |r, g, b| Color::from_rgba8(r, g, b, 0xFF);
        let a = Color::from_rgba8;
        EditorTheme {
            background: c(0xFF, 0xFF, 0xFF),
            gutter_bg: c(0xFF, 0xFF, 0xFF),
            gutter_fg: c(0x29, 0x29, 0x29),
            gutter_active_fg: c(0x00, 0x6B, 0xBD),
            current_line: a(0x00, 0x6B, 0xBD, 0x22),
            caret: c(0, 0, 0),
            selection: a(0x0F, 0x4A, 0x85, 0x40),
            selection_inactive: a(0x0F, 0x4A, 0x85, 0x26),
            indent_guide: c(0x80, 0x80, 0x80),
            indent_guide_active: c(0x29, 0x29, 0x29),
            whitespace: c(0x80, 0x80, 0x80),
            ruler: c(0x29, 0x29, 0x29),
            matching_bracket: c(0x0F, 0x4A, 0x85),
            foreground: c(0x29, 0x29, 0x29),
            border: c(0x0F, 0x4A, 0x85),
            overlay_bg: c(0xFF, 0xFF, 0xFF),
            muted: c(0x4A, 0x4A, 0x4A),
            accent: c(0x00, 0x6B, 0xBD),
            search_match: a(0x0F, 0x4A, 0x85, 0x30),
            search_match_current: a(0x0F, 0x4A, 0x85, 0x66),
            scrollbar: a(0x0F, 0x4A, 0x85, 0x99),
            scrollbar_active: c(0x0F, 0x4A, 0x85),
            diag_error: c(0xB5, 0x20, 0x0D),
            diag_warning: c(0x89, 0x5E, 0x03),
            diag_info: c(0x0F, 0x4A, 0x85),
            diag_hint: c(0x29, 0x29, 0x29),
            diff_added: a(0x37, 0x4E, 0x06, 0x33),
            diff_removed: a(0xAD, 0x07, 0x07, 0xC0),
            change_added: c(0x37, 0x4E, 0x06),
            diff_removed_line: a(0xFF, 0x00, 0x00, 0x26),
            diff_inserted_text: a(0x9C, 0xCC, 0x2C, 0x40),
            diff_removed_text: a(0xFF, 0x00, 0x00, 0x40),
            change_modified: c(0x0F, 0x4A, 0x85),
            change_deleted: c(0xAD, 0x07, 0x07),
            overview_cursor: c(0x29, 0x29, 0x29),
            unicode_highlight: c(0x89, 0x5E, 0x03),
            bracket_guide: c(0x29, 0x29, 0x29),
            ghost_text: a(0x00, 0x00, 0x00, 0x99),
            fold_badge: a(0x0F, 0x4A, 0x85, 0x40),
            linked_editing: c(0x00, 0x6B, 0xBD),
            breakpoint: c(0xE5, 0x14, 0x00),
            syntax: HighlightStyle::high_contrast_light(),
            ui_font_size: 12.5,
            ui_font_family: None,
            overlay_radius: 0.0,
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
    fn default_syntax_keeps_code_regular_weight() {
        // VS Code Dark/Light Modern and Zed One Dark: no bold keywords, upright comments.
        for h in [HighlightStyle::dark(), HighlightStyle::light()] {
            for k in [
                TokenKind::Keyword,
                TokenKind::Comment,
                TokenKind::Type,
                TokenKind::Plain,
            ] {
                let st = h.style(k);
                assert!(!st.bold && !st.italic, "{k:?} is regular weight, upright");
            }
            // Markup keeps its meaning.
            assert!(h.style(TokenKind::Heading).bold);
            assert!(h.style(TokenKind::Emphasis).italic);
        }
    }

    #[test]
    fn default_themes_meet_wcag_aa_for_code() {
        use crate::lang::TokenKind as K;
        let kinds = [
            K::Keyword,
            K::Type,
            K::Str,
            K::Number,
            K::Comment,
            K::Function,
            K::Macro,
            K::Attribute,
            K::Constant,
            K::Punctuation,
            K::Property,
            K::Escape,
            K::Tag,
            K::Namespace,
            K::Heading,
            K::Link,
            K::Emphasis,
            K::Plain,
            K::Bracket(0),
            K::Bracket(1),
            K::Bracket(2),
        ];
        for t in [EditorTheme::dark(), EditorTheme::light()] {
            for k in kinds {
                let r = contrast(t.color(k), t.background);
                assert!(r >= 4.5, "{k:?} contrast {r:.2}");
            }
            assert!(contrast(t.foreground, t.background) >= 7.0);
        }
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

    fn contrast(a: Color, b: Color) -> f64 {
        let lum = |c: Color| {
            let [r, g, b, _] = c.components;
            let ch = |v: f32| {
                let v = v as f64;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b)
        };
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn high_contrast_themes_meet_wcag_aaa() {
        use crate::lang::TokenKind as K;
        let kinds = [
            K::Keyword,
            K::Type,
            K::Str,
            K::Number,
            K::Comment,
            K::Function,
            K::Macro,
            K::Attribute,
            K::Constant,
            K::Punctuation,
            K::Property,
            K::Escape,
            K::Tag,
            K::Namespace,
            K::Heading,
            K::Link,
            K::Emphasis,
            K::Plain,
            K::Bracket(0),
            K::Bracket(1),
            K::Bracket(2),
        ];
        for t in [
            EditorTheme::high_contrast_dark(),
            EditorTheme::high_contrast_light(),
        ] {
            for k in kinds {
                let r = contrast(t.color(k), t.background);
                assert!(r >= 7.0, "{k:?} contrast {r:.2}");
            }
            assert!(contrast(t.foreground, t.background) >= 7.0);
            assert!(contrast(t.gutter_fg, t.background) >= 7.0);
            assert!(contrast(t.caret, t.background) >= 7.0);
        }
    }
}
