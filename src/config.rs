//! The public builder API: [`code_editor`] and the fluent [`CodeEditor`] configuration,
//! plus the resolved [`Props`] the view consumes. This is the crate's front door — all the
//! knobs (theme, gutter, minimap, auto-close, …) live here; the rendering that reads them
//! lives in [`crate::view`].

use pebbles::prelude::*;

use crate::ADVANCE_RATIO;
use crate::extensions::Extension;
use crate::lang::{Language, Token};
use crate::providers::{
    CompletionProvider, DefinitionProvider, Diagnostics, FormatProvider, HoverProvider, InlayHints,
    SignatureProvider,
};
use crate::theme::EditorTheme;
use crate::view::render_editor;


/// Build a code editor bound to `code`. Configure fluently, then drop it into any tree.
pub fn code_editor(code: Signal<String>) -> CodeEditor {
    CodeEditor {
        code,
        language: None,
        theme: EditorTheme::dark(),
        font_size: 13.5,
        line_height: 1.6,
        font_family: None,
        letter_spacing: 0.0,
        height: None,
        gutter: true,
        read_only: false,
        autofocus: false,
        current_line: true,
        context_menu: true,
        tab_size: 4,
        insert_spaces: true,
        advance_ratio: ADVANCE_RATIO,
        indent_guides: false,
        render_whitespace: false,
        rulers: Vec::new(),
        minimap: false,
        sticky_scroll: false,
        auto_close: true,
        match_brackets: true,
        semantic: None,
        completion: None,
        hover: None,
        signature: None,
        diagnostics: None,
        inlay_hints: None,
        definition: None,
        format: None,
        extensions: Vec::new(),
        title: None,
    }
}

/// A configured code editor. See [`code_editor`].
pub struct CodeEditor {
    code: Signal<String>,
    language: Option<Box<dyn Language>>,
    theme: EditorTheme,
    font_size: f64,
    line_height: f64,
    font_family: Option<String>,
    letter_spacing: f64,
    height: Option<f64>,
    gutter: bool,
    read_only: bool,
    autofocus: bool,
    current_line: bool,
    context_menu: bool,
    tab_size: usize,
    insert_spaces: bool,
    advance_ratio: f64,
    indent_guides: bool,
    render_whitespace: bool,
    rulers: Vec<usize>,
    minimap: bool,
    sticky_scroll: bool,
    auto_close: bool,
    match_brackets: bool,
    semantic: Option<Signal<Vec<Token>>>,
    completion: Option<CompletionProvider>,
    hover: Option<HoverProvider>,
    signature: Option<SignatureProvider>,
    diagnostics: Option<Diagnostics>,
    inlay_hints: Option<InlayHints>,
    definition: Option<DefinitionProvider>,
    format: Option<FormatProvider>,
    extensions: Vec<Extension>,
    title: Option<String>,
}

impl CodeEditor {
    /// The syntax highlighter (default: none — plain text).
    pub fn language(mut self, lang: Box<dyn Language>) -> Self {
        self.language = Some(lang);
        self
    }
    /// The color theme (default: [`EditorTheme::dark`]).
    pub fn theme(mut self, theme: EditorTheme) -> Self {
        self.theme = theme;
        self
    }
    /// Font size in logical px (default 13.5).
    pub fn font_size(mut self, px: f64) -> Self {
        self.font_size = px;
        self
    }
    /// Line height as a multiple of the font size (default 1.6).
    pub fn line_height(mut self, factor: f64) -> Self {
        self.line_height = factor.max(1.0);
        self
    }
    /// The monospace font family for code text (default: JetBrains Mono). Use any installed
    /// monospace family; pair it with [`advance_ratio`](Self::advance_ratio) so the caret grid
    /// matches the font's real glyph advance.
    pub fn font_family(mut self, family: impl Into<String>) -> Self {
        self.font_family = Some(family.into());
        self
    }
    /// Extra spacing between glyphs in logical px (default 0). Folded into the grid advance so
    /// caret/click/selection stay exact.
    pub fn letter_spacing(mut self, px: f64) -> Self {
        self.letter_spacing = px;
        self
    }
    /// The editor's viewport height; it scrolls within. Unset = grows to content.
    pub fn height(mut self, px: f64) -> Self {
        self.height = Some(px);
        self
    }
    /// Show the line-number gutter (default true).
    pub fn gutter(mut self, show: bool) -> Self {
        self.gutter = show;
        self
    }
    /// Read-only: navigable/selectable/copyable, but keystrokes never mutate the buffer.
    pub fn read_only(mut self, ro: bool) -> Self {
        self.read_only = ro;
        self
    }
    /// Grab keyboard focus (and show the caret) on mount.
    pub fn autofocus(mut self) -> Self {
        self.autofocus = true;
        self
    }
    /// Highlight the line the caret is on (default true).
    pub fn current_line(mut self, on: bool) -> Self {
        self.current_line = on;
        self
    }
    /// Show the right-click context menu (Cut/Copy/Paste/Select All) — default true.
    pub fn context_menu(mut self, on: bool) -> Self {
        self.context_menu = on;
        self
    }
    /// Indent width in columns (default 4).
    pub fn tab_size(mut self, n: usize) -> Self {
        self.tab_size = n.max(1);
        self
    }
    /// Indent with spaces (default) vs. a tab character.
    pub fn insert_spaces(mut self, spaces: bool) -> Self {
        self.insert_spaces = spaces;
        self
    }
    /// Glyph advance as a fraction of the font size — set this to your monospace font's
    /// real advance ratio (default ≈ 0.6, JetBrains Mono) so caret/click/selection math is
    /// exact. Programming ligature fonts keep the cell width, so ligatures stay exact too.
    pub fn advance_ratio(mut self, ratio: f64) -> Self {
        self.advance_ratio = ratio.max(0.1);
        self
    }
    /// Draw faint vertical indent guides at each indentation level (default off).
    pub fn indent_guides(mut self, on: bool) -> Self {
        self.indent_guides = on;
        self
    }
    /// Render whitespace: middots for spaces, arrows for tabs (default off).
    pub fn render_whitespace(mut self, on: bool) -> Self {
        self.render_whitespace = on;
        self
    }
    /// Vertical rulers / print-margin lines at the given columns (e.g. `[80, 120]`).
    pub fn rulers(mut self, columns: impl Into<Vec<usize>>) -> Self {
        self.rulers = columns.into();
        self
    }
    /// Show a minimap (scaled document overview) on the right — click/drag to scroll.
    /// Only shown when the editor has a fixed `height`. Default off.
    pub fn minimap(mut self, on: bool) -> Self {
        self.minimap = on;
        self
    }
    /// Pin the enclosing scopes (by indentation) to the top as you scroll — "sticky scroll".
    /// Only shown when the editor has a fixed `height`. Default off.
    pub fn sticky_scroll(mut self, on: bool) -> Self {
        self.sticky_scroll = on;
        self
    }
    /// Auto-close brackets and quotes: typing an opener inserts its closer, typing the
    /// closer over an auto-inserted one skips it, and backspacing an empty pair clears both.
    /// Default on.
    pub fn auto_close(mut self, on: bool) -> Self {
        self.auto_close = on;
        self
    }
    /// Highlight the bracket under/next to the caret together with its match. Default on.
    pub fn match_brackets(mut self, on: bool) -> Self {
        self.match_brackets = on;
        self
    }
    /// Overlay provider-supplied **semantic tokens** on top of the lexical highlighting —
    /// the hook an IDE feeds from a language server (LSP semantic tokens) or its own
    /// analysis. Semantic tokens win over lexical ones on any overlap, and the editor
    /// re-highlights whenever the signal changes.
    /// The autocomplete source (Ctrl+Space, and as you type). See [`CompletionProvider`].
    pub fn completion(mut self, provider: CompletionProvider) -> Self {
        self.completion = Some(provider);
        self
    }
    /// The hover-tooltip source. See [`HoverProvider`].
    pub fn hover(mut self, provider: HoverProvider) -> Self {
        self.hover = Some(provider);
        self
    }
    /// The signature-help source (shown while typing a call). See [`SignatureProvider`].
    pub fn signature_help(mut self, provider: SignatureProvider) -> Self {
        self.signature = Some(provider);
        self
    }
    /// Diagnostics to underline + mark in the gutter (a reactive list). See [`Diagnostics`].
    pub fn diagnostics(mut self, diagnostics: Diagnostics) -> Self {
        self.diagnostics = Some(diagnostics);
        self
    }
    /// Inlay hints to render inline (a reactive list). See [`InlayHints`].
    pub fn inlay_hints(mut self, hints: InlayHints) -> Self {
        self.inlay_hints = Some(hints);
        self
    }
    /// The go-to-definition source (F12). See [`DefinitionProvider`].
    pub fn definition(mut self, provider: DefinitionProvider) -> Self {
        self.definition = Some(provider);
        self
    }
    /// The document formatter (Shift+Alt+F). See [`FormatProvider`].
    pub fn format(mut self, provider: FormatProvider) -> Self {
        self.format = Some(provider);
        self
    }
    /// Add one editor [`Extension`] (plugin) — decorations, commands, gutter markers,
    /// read-only ranges, and event hooks. Call repeatedly or use [`extensions`](Self::extensions).
    pub fn extension(mut self, ext: Extension) -> Self {
        self.extensions.push(ext);
        self
    }
    /// Add several extensions at once (composed in order).
    pub fn extensions(mut self, exts: impl IntoIterator<Item = Extension>) -> Self {
        self.extensions.extend(exts);
        self
    }
    pub fn semantic_tokens(mut self, tokens: Signal<Vec<Token>>) -> Self {
        self.semantic = Some(tokens);
        self
    }
    /// A filename / label shown in the status bar (also enables the status bar).
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

impl IntoWidget for CodeEditor {
    fn into_widget(self) -> AnyWidget {
        component_props(render_editor, Props::from(self)).into_widget()
    }
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

pub(crate) struct Props {
    pub(crate) code: Signal<String>,
    pub(crate) language: Option<Box<dyn Language>>,
    pub(crate) theme: EditorTheme,
    pub(crate) fs: f64,
    pub(crate) lh: f64,
    pub(crate) font_family: String,
    pub(crate) letter_spacing: f64,
    pub(crate) height: Option<f64>,
    pub(crate) gutter: bool,
    pub(crate) read_only: bool,
    pub(crate) autofocus: bool,
    pub(crate) current_line: bool,
    pub(crate) context_menu: bool,
    pub(crate) tab_size: usize,
    pub(crate) insert_spaces: bool,
    pub(crate) advance_ratio: f64,
    pub(crate) indent_guides: bool,
    pub(crate) render_whitespace: bool,
    pub(crate) rulers: Vec<usize>,
    pub(crate) minimap: bool,
    pub(crate) sticky_scroll: bool,
    pub(crate) auto_close: bool,
    pub(crate) match_brackets: bool,
    pub(crate) semantic: Option<Signal<Vec<Token>>>,
    pub(crate) completion: Option<CompletionProvider>,
    pub(crate) hover: Option<HoverProvider>,
    pub(crate) signature: Option<SignatureProvider>,
    pub(crate) diagnostics: Option<Diagnostics>,
    pub(crate) inlay_hints: Option<InlayHints>,
    pub(crate) definition: Option<DefinitionProvider>,
    pub(crate) format: Option<FormatProvider>,
    pub(crate) extensions: Vec<Extension>,
    pub(crate) title: Option<String>,
}

impl From<CodeEditor> for Props {
    fn from(e: CodeEditor) -> Self {
        // Merge extension config facets over the builder values (applied in order — a later
        // `Some` wins), so a plugin can ship its preferred defaults without wiring every knob.
        let mut tab_size = e.tab_size;
        let mut insert_spaces = e.insert_spaces;
        let mut indent_guides = e.indent_guides;
        let mut render_whitespace = e.render_whitespace;
        let mut match_brackets = e.match_brackets;
        let mut auto_close = e.auto_close;
        for patch in e.extensions.iter().filter_map(|x| x.config) {
            if let Some(v) = patch.tab_size {
                tab_size = v.max(1);
            }
            if let Some(v) = patch.insert_spaces {
                insert_spaces = v;
            }
            if let Some(v) = patch.indent_guides {
                indent_guides = v;
            }
            if let Some(v) = patch.render_whitespace {
                render_whitespace = v;
            }
            if let Some(v) = patch.match_brackets {
                match_brackets = v;
            }
            if let Some(v) = patch.auto_close {
                auto_close = v;
            }
        }
        // Fold extension theme transforms over the builder theme (applied in order) — this is
        // "theme = an extension": a plugin can fully replace or partially compose the theme.
        let mut theme = e.theme;
        for tx in e.extensions.iter().filter_map(|x| x.theme.as_ref()) {
            theme = tx(theme);
        }
        Props {
            code: e.code,
            language: e.language,
            theme,
            fs: e.font_size,
            lh: e.line_height,
            font_family: e.font_family.unwrap_or_else(|| crate::MONO.to_string()),
            letter_spacing: e.letter_spacing,
            height: e.height,
            gutter: e.gutter,
            read_only: e.read_only,
            autofocus: e.autofocus,
            current_line: e.current_line,
            context_menu: e.context_menu,
            tab_size,
            insert_spaces,
            advance_ratio: e.advance_ratio,
            indent_guides,
            render_whitespace,
            rulers: e.rulers,
            minimap: e.minimap,
            sticky_scroll: e.sticky_scroll,
            auto_close,
            match_brackets,
            semantic: e.semantic,
            completion: e.completion,
            hover: e.hover,
            signature: e.signature,
            diagnostics: e.diagnostics,
            inlay_hints: e.inlay_hints,
            definition: e.definition,
            format: e.format,
            extensions: e.extensions,
            title: e.title,
        }
    }
}
