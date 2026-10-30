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

/// Caret shape (VS Code `editor.cursorStyle`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CursorStyle {
    /// A 2px vertical bar (default).
    #[default]
    Line,
    /// A 1px vertical bar.
    LineThin,
    /// A filled cell.
    Block,
    /// An outlined cell.
    BlockOutline,
    /// A thick underline.
    Underline,
    /// A thin underline.
    UnderlineThin,
}

/// Caret blinking (VS Code `editor.cursorBlinking`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CursorBlinking {
    /// On/off (default).
    #[default]
    Blink,
    /// Fade in and out.
    Smooth,
    /// Eased fade.
    Phase,
    /// Grow/shrink vertically.
    Expand,
    /// No blinking.
    Solid,
}

/// Line-number style (VS Code `editor.lineNumbers`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LineNumbers {
    /// Absolute line numbers (default).
    #[default]
    On,
    /// Distance from the caret line (the caret line shows its absolute number).
    Relative,
    /// No line numbers (the gutter keeps fold arrows and markers).
    Off,
}

/// Build a code editor bound to `code`. Configure fluently, then drop it into any tree.
pub fn code_editor(code: Signal<String>) -> CodeEditor {
    CodeEditor {
        code,
        language: None,
        theme: None,
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
        a11y_label: None,
        on_edit: None,
        on_scroll: None,
        initial_scroll: 0.0,
        selection_handles: false,
        folds: None,
        breakpoints: None,
        soft_wrap: false,
        diff_base: None,
        keymap: crate::keymap::Keymap::default(),
        document_id: None,
        controller: None,
        suggest_preview: false,
        format_on_paste: false,
        inline_suggestions: None,
        line_numbers: LineNumbers::On,
        smooth_caret: false,
        cursor_blinking: CursorBlinking::Blink,
        cursor_style: CursorStyle::Line,
        on_stage_hunk: None,
        on_open_link: None,
        word_based_suggestions: true,
        overview_ruler: true,
        linked_editing: true,
        color_decorators: true,
        unicode_highlight: true,
        bracket_pair_guides: true,
        occurrences_highlight: true,
        bracket_pair_colorization: true,
        detect_indentation: true,
    }
}

/// A configured code editor. See [`code_editor`].
pub struct CodeEditor {
    code: Signal<String>,
    language: Option<Box<dyn Language>>,
    theme: Option<EditorTheme>,
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
    a11y_label: Option<String>,
    on_edit: Option<EditHook>,
    on_scroll: Option<ScrollHook>,
    initial_scroll: f64,
    selection_handles: bool,
    folds: Option<Signal<std::collections::BTreeSet<usize>>>,
    breakpoints: Option<Signal<std::collections::BTreeSet<usize>>>,
    soft_wrap: bool,
    diff_base: Option<Signal<String>>,
    keymap: crate::keymap::Keymap,
    document_id: Option<u64>,
    controller: Option<crate::controller::EditorController>,
    detect_indentation: bool,
    bracket_pair_colorization: bool,
    occurrences_highlight: bool,
    bracket_pair_guides: bool,
    unicode_highlight: bool,
    color_decorators: bool,
    linked_editing: bool,
    overview_ruler: bool,
    word_based_suggestions: bool,
    on_open_link: Option<std::rc::Rc<dyn Fn(&str)>>,
    on_stage_hunk: Option<std::rc::Rc<dyn Fn(&crate::diff::ChangeHunk, &str)>>,
    cursor_style: CursorStyle,
    cursor_blinking: CursorBlinking,
    smooth_caret: bool,
    line_numbers: LineNumbers,
    inline_suggestions: Option<crate::providers::InlineSuggestionProvider>,
    suggest_preview: bool,
    format_on_paste: bool,
}

/// Fired after each change with the minimal [`Edit`] delta; `remote` is true when the change
/// arrived from outside (the `code` signal was set by the app) vs. a local keystroke.
pub type EditHook = std::rc::Rc<dyn Fn(&crate::collab::Edit, bool)>;
/// Fired with the vertical scroll offset (logical px) whenever it changes.
pub type ScrollHook = std::rc::Rc<dyn Fn(f64)>;

impl CodeEditor {
    /// The syntax highlighter (default: none — plain text).
    pub fn language(mut self, lang: Box<dyn Language>) -> Self {
        self.language = Some(lang);
        self
    }
    /// The color theme. Default: [`EditorTheme::from_theme`] — the light or dark editor
    /// theme matching the app's current Pebbles theme, so the editor follows
    /// `ThemeMode` / `toggle_theme` live. Setting a theme pins it.
    pub fn theme(mut self, theme: EditorTheme) -> Self {
        self.theme = Some(theme);
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
    /// Overlay provider/LSP **semantic tokens** on top of the lexical highlighting (a reactive
    /// list). Semantic tokens win on any overlap and re-highlight when the signal changes.
    pub fn semantic_tokens(mut self, tokens: Signal<Vec<Token>>) -> Self {
        self.semantic = Some(tokens);
        self
    }
    /// A filename / label shown in the status bar (also enables the status bar).
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
    /// The accessible name announced by screen readers (default "Code editor"). The editor
    /// exposes itself as a multiline text input with this label and its content as the value.
    pub fn a11y_label(mut self, label: impl Into<String>) -> Self {
        self.a11y_label = Some(label.into());
        self
    }
    /// Observe every change as a minimal [`Edit`](crate::Edit) delta (the second arg is `true`
    /// for remote changes — the `code` signal set from outside). This is the OT/CRDT-ready
    /// outbound op stream; remote edits are applied with the local caret remapped through the
    /// same delta, so a collaborator's edit never moves your cursor.
    pub fn on_edit(mut self, f: impl Fn(&crate::collab::Edit, bool) + 'static) -> Self {
        self.on_edit = Some(std::rc::Rc::new(f));
        self
    }
    /// Observe the vertical scroll offset (logical px) as it changes — persist it to restore
    /// the view later with [`initial_scroll`](Self::initial_scroll).
    pub fn on_scroll(mut self, f: impl Fn(f64) + 'static) -> Self {
        self.on_scroll = Some(std::rc::Rc::new(f));
        self
    }
    /// The starting vertical scroll offset (logical px) — restore a saved view state.
    pub fn initial_scroll(mut self, px: f64) -> Self {
        self.initial_scroll = px.max(0.0);
        self
    }
    /// Show draggable selection handles at the ends of a selection (for touch). Default off
    /// (desktop pointer selection needs no handles). Drag a handle to grow/shrink the selection.
    pub fn selection_handles(mut self, on: bool) -> Self {
        self.selection_handles = on;
        self
    }
    /// Bind fold state to a caller-owned signal (the set of folded head lines) — so folds
    /// persist and restore across mounts (e.g. per file/tab). Without it the editor keeps its
    /// own internal fold state.
    pub fn folds(mut self, folds: Signal<std::collections::BTreeSet<usize>>) -> Self {
        self.folds = Some(folds);
        self
    }
    /// Show the breakpoint area (VS Code glyph margin) left of the line numbers, bound to
    /// a caller-owned set of 0-based breakpoint lines: a click toggles the line's
    /// breakpoint, set lines draw a breakpoint dot. Lines move with edits above them.
    pub fn breakpoints(mut self, lines: Signal<std::collections::BTreeSet<usize>>) -> Self {
        self.breakpoints = Some(lines);
        self
    }
    /// Soft-wrap long lines to the viewport width instead of scrolling horizontally (needs a
    /// fixed [`height`](Self::height)). Default off. Caret, click, and selection all follow the
    /// wrapped rows.
    pub fn soft_wrap(mut self, on: bool) -> Self {
        self.soft_wrap = on;
        self
    }
    /// Show an inline diff against `base` (a reactive signal): added lines get a green band + a
    /// `+` gutter mark, and a red `−` marks where lines were removed. Recomputes as either the
    /// buffer or the base changes.
    pub fn diff_base(mut self, base: Signal<String>) -> Self {
        self.diff_base = Some(base);
        self
    }
    /// Override the built-in keybindings: disable a default chord (the key then bubbles to the
    /// app's shortcuts) or bind a built-in to another chord. See [`Keymap`](crate::Keymap).
    pub fn keymap(mut self, keymap: crate::keymap::Keymap) -> Self {
        self.keymap = keymap;
        self
    }
    /// Identify the document being edited. The editor's internal state (selection, undo
    /// history, scroll, folds, find/palette) is per document: it resets whenever the bound
    /// `code` signal is a *different* signal or `document_id` changes — like a VS Code editor
    /// switching models. Use this when one `Signal<String>` is reused for several files
    /// (set the signal's text AND bump the id). Without it, setting the text of the same
    /// signal is treated as an external edit of the same document (carets are remapped).
    pub fn document_id(mut self, id: u64) -> Self {
        self.document_id = Some(id);
        self
    }
    /// Drive the caret, selection and scroll programmatically (e.g. "open at line") — see
    /// [`EditorController`](crate::EditorController).
    pub fn controller(mut self, controller: crate::controller::EditorController) -> Self {
        self.controller = Some(controller);
        self
    }
    /// Detect the file's indentation (spaces vs tabs and the unit) on open, overriding `tab_size`/`insert_spaces` when the file is indented (VS Code `editor.detectIndentation`, default on).
    pub fn detect_indentation(mut self, v: bool) -> Self {
        self.detect_indentation = v;
        self
    }
    /// Colour brackets by nesting depth from the theme's `syntax.brackets` palette (VS Code `editor.bracketPairColorization.enabled`, default on).
    pub fn bracket_pair_colorization(mut self, v: bool) -> Self {
        self.bracket_pair_colorization = v;
        self
    }
    /// Highlight every occurrence of the word at the caret (VS Code `editor.occurrencesHighlight`, default on). A document-highlight provider replaces the textual match when wired.
    pub fn occurrences_highlight(mut self, v: bool) -> Self {
        self.occurrences_highlight = v;
        self
    }
    /// Draw the bracket-pair guide of the pair enclosing the caret (VS Code `editor.guides.bracketPairs: active`, default on).
    pub fn bracket_pair_guides(mut self, v: bool) -> Self {
        self.bracket_pair_guides = v;
        self
    }
    /// Box invisible / ambiguous Unicode characters and draw control characters as symbols (VS Code `editor.unicodeHighlight.*` + `renderControlCharacters`, default on).
    pub fn unicode_highlight(mut self, v: bool) -> Self {
        self.unicode_highlight = v;
        self
    }
    /// Show a colour swatch (with a picker) before CSS colour literals (VS Code `editor.colorDecorators`, default on).
    pub fn color_decorators(mut self, v: bool) -> Self {
        self.color_decorators = v;
        self
    }
    /// Edit a markup tag's opening and closing names together (VS Code `editor.linkedEditing` + auto-rename tag). Active for HTML / XML / JSX / TSX / Markdown / Vue / Svelte.
    pub fn linked_editing(mut self, v: bool) -> Self {
        self.linked_editing = v;
        self
    }
    /// Show the overview ruler beside the scrollbar (cursor, selections, matches, diagnostics, changes) — fixed-height editors only.
    pub fn overview_ruler(mut self, v: bool) -> Self {
        self.overview_ruler = v;
        self
    }
    /// Offer completion from words in the document when no completion provider is set (VS Code `editor.wordBasedSuggestions`, default on).
    pub fn word_based_suggestions(mut self, v: bool) -> Self {
        self.word_based_suggestions = v;
        self
    }
    /// Open links: Ctrl/Cmd+click on a URL in the text calls `f(url)` (links are underlined while set).
    pub fn on_open_link(mut self, f: impl Fn(&str) + 'static) -> Self {
        self.on_open_link = Some(std::rc::Rc::new(f));
        self
    }
    /// Show **Stage** in the change peek: `f(hunk, current_text_of_the_hunk_lines)` stages that change (the host runs e.g. `git apply --cached`).
    pub fn on_stage_hunk(mut self, f: impl Fn(&crate::diff::ChangeHunk, &str) + 'static) -> Self {
        self.on_stage_hunk = Some(std::rc::Rc::new(f));
        self
    }
    /// Caret shape (VS Code `editor.cursorStyle`).
    pub fn cursor_style(mut self, v: CursorStyle) -> Self {
        self.cursor_style = v;
        self
    }
    /// Caret blinking mode (VS Code `editor.cursorBlinking`).
    pub fn cursor_blinking(mut self, v: CursorBlinking) -> Self {
        self.cursor_blinking = v;
        self
    }
    /// Animate the caret between positions (VS Code `editor.cursorSmoothCaretAnimation`).
    pub fn smooth_caret(mut self, v: bool) -> Self {
        self.smooth_caret = v;
        self
    }
    /// Line-number style (VS Code `editor.lineNumbers`): absolute, relative to the caret, or off.
    pub fn line_numbers(mut self, v: LineNumbers) -> Self {
        self.line_numbers = v;
        self
    }
    /// Inline (ghost-text) suggestions: `provider(text, caret)` returns candidate insertions shown greyed at the caret; Tab accepts, Esc dismisses, Alt+] / Alt+[ cycle.
    pub fn inline_suggestions(mut self, p: crate::providers::InlineSuggestionProvider) -> Self {
        self.inline_suggestions = Some(p);
        self
    }
    /// The change baseline for the gutter change bars, peek, next/previous change and the
    /// overview ruler (e.g. the file at `git show HEAD:path`). Alias of
    /// [`diff_base`](Self::diff_base).
    pub fn change_baseline(self, base: Signal<String>) -> Self {
        self.diff_base(base)
    }
    /// Preview the selected completion inline as ghost text at the caret (VS Code `editor.suggest.preview`, default off).
    pub fn suggest_preview(mut self, v: bool) -> Self {
        self.suggest_preview = v;
        self
    }
    /// Run the [`format`](Self::format) provider after each paste (VS Code
    /// `editor.formatOnPaste`, default off). The format is its own undo step, so one undo
    /// restores the pasted text as typed.
    pub fn format_on_paste(mut self, v: bool) -> Self {
        self.format_on_paste = v;
        self
    }
}

impl IntoWidget for CodeEditor {
    fn into_widget(self) -> AnyWidget {
        // Key the editor by document identity (the bound signal + optional `document_id`), so a
        // different document REMOUNTS it with fresh state instead of keeping the previous
        // document's carets, history and scroll (Flutter: a new Key → a new State).
        let key = format!(
            "pebbles-code-editor:{}:{}",
            self.code.raw_id(),
            self.document_id.map_or_else(String::new, |d| d.to_string())
        );
        keyed(key, component_props(render_editor, Props::from(self))).into_widget()
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
    pub(crate) a11y_label: String,
    pub(crate) on_edit: Option<EditHook>,
    pub(crate) on_scroll: Option<ScrollHook>,
    pub(crate) initial_scroll: f64,
    pub(crate) selection_handles: bool,
    pub(crate) folds: Option<Signal<std::collections::BTreeSet<usize>>>,
    pub(crate) breakpoints: Option<Signal<std::collections::BTreeSet<usize>>>,
    pub(crate) soft_wrap: bool,
    pub(crate) diff_base: Option<Signal<String>>,
    pub(crate) keymap: crate::keymap::Keymap,
    pub(crate) controller: Option<crate::controller::EditorController>,
    pub(crate) detect_indentation: bool,
    pub(crate) bracket_pair_colorization: bool,
    pub(crate) occurrences_highlight: bool,
    pub(crate) bracket_pair_guides: bool,
    pub(crate) unicode_highlight: bool,
    pub(crate) color_decorators: bool,
    pub(crate) linked_editing: bool,
    pub(crate) overview_ruler: bool,
    pub(crate) word_based_suggestions: bool,
    pub(crate) on_open_link: Option<std::rc::Rc<dyn Fn(&str)>>,
    pub(crate) on_stage_hunk: Option<std::rc::Rc<dyn Fn(&crate::diff::ChangeHunk, &str)>>,
    pub(crate) cursor_style: CursorStyle,
    pub(crate) cursor_blinking: CursorBlinking,
    pub(crate) smooth_caret: bool,
    pub(crate) line_numbers: LineNumbers,
    pub(crate) inline_suggestions: Option<crate::providers::InlineSuggestionProvider>,
    pub(crate) suggest_preview: bool,
    pub(crate) format_on_paste: bool,
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
        // Resolved here, inside the parent's build, so reading the app theme subscribes the
        // parent and a light/dark switch rebuilds the editor with the matching palette.
        let mut theme = e.theme.unwrap_or_else(EditorTheme::from_theme);
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
            a11y_label: e.a11y_label.unwrap_or_else(|| "Code editor".to_string()),
            on_edit: e.on_edit,
            on_scroll: e.on_scroll,
            initial_scroll: e.initial_scroll,
            selection_handles: e.selection_handles,
            folds: e.folds,
            breakpoints: e.breakpoints,
            soft_wrap: e.soft_wrap,
            diff_base: e.diff_base,
            keymap: e.keymap,
            controller: e.controller,
            suggest_preview: e.suggest_preview,
            format_on_paste: e.format_on_paste,
            inline_suggestions: e.inline_suggestions,
            line_numbers: e.line_numbers,
            smooth_caret: e.smooth_caret,
            cursor_blinking: e.cursor_blinking,
            cursor_style: e.cursor_style,
            on_stage_hunk: e.on_stage_hunk,
            on_open_link: e.on_open_link,
            word_based_suggestions: e.word_based_suggestions,
            overview_ruler: e.overview_ruler,
            linked_editing: e.linked_editing,
            color_decorators: e.color_decorators,
            unicode_highlight: e.unicode_highlight,
            bracket_pair_guides: e.bracket_pair_guides,
            occurrences_highlight: e.occurrences_highlight,
            bracket_pair_colorization: e.bracket_pair_colorization,
            detect_indentation: e.detect_indentation,
        }
    }
}
