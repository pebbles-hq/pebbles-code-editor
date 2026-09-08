//! # pebbles-code-editor
//!
//! An embeddable, syntax-highlighting **code editor** widget for the
//! [Pebbles](https://github.com/pebbles-hq/pebbles) GUI framework — the foundation to
//! build an IDE on. The reusable core is here today:
//!
//! - **Syntax highlighting** with a pluggable [`Language`] layer (Rust + JSON bundled;
//!   add your own by implementing the trait).
//! - A **line-number gutter**.
//! - Swappable **themes** ([`EditorTheme`]) — dark and light bundled, or roll your own.
//! - **Monospace, grid-aligned**, scrollable rendering with editor chrome (status bar).
//!
//! ```ignore
//! use pebbles::prelude::*;
//! use pebbles_code_editor::{code_editor, lang::Rust, EditorTheme};
//!
//! fn view() -> impl IntoWidget {
//!     let code = create_signal(String::from("fn main() {}\n"));
//!     code_editor(code).language(Box::new(Rust)).theme(EditorTheme::dark()).height(420.0)
//! }
//! ```
//!
//! ## Status
//! **v0.1 renders and highlights** a code buffer (the read/display pane an editor is
//! built around), with the pluggable-language + theme system an IDE needs. The buffer is
//! held in the [`Signal`](pebbles::prelude::Signal) you pass to [`code_editor`], so
//! programmatic edits re-render live.
//!
//! **Live in-place editing** (caret, selection, keystrokes over the *highlighted* text)
//! is the next milestone. It needs a small enhancement in the framework's text engine:
//! the editable (`RenderTextField`) currently paints a single color and can't be made
//! transparent, so a highlighted overlay can't align to it. The fix is to let the
//! editable paint **styled runs** (a `Vec<(range, color)>`) — then this widget feeds it
//! the same tokens it colors with today, and editing + highlighting are one layer.

pub mod lang;
mod theme;

pub use lang::{Language, Token, TokenKind};
pub use theme::EditorTheme;

use pebbles::prelude::*;

/// The monospace family used for all editor text.
const MONO: &str = "JetBrains Mono";

/// Build a syntax-highlighting code editor bound to `code`. Configure it fluently, then
/// drop it into any Pebbles tree.
pub fn code_editor(code: Signal<String>) -> CodeEditor {
    CodeEditor {
        code,
        language: None,
        theme: EditorTheme::dark(),
        font_size: 13.5,
        line_height: 1.6,
        height: None,
        gutter: true,
        read_only: false,
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
    height: Option<f64>,
    gutter: bool,
    read_only: bool,
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
    /// Read-only: selectable + copyable, but not editable.
    pub fn read_only(mut self, ro: bool) -> Self {
        self.read_only = ro;
        self
    }
    /// A filename / label shown in the status bar (also enables the status bar).
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

/// Split `src` into contiguous rich-text spans (gaps between tokens become plain
/// foreground text), so the whole document is colored in one paragraph.
fn to_spans(src: &str, tokens: &[Token], theme: &EditorTheme, fs: f64) -> Vec<TextSpan> {
    let mut spans = Vec::new();
    let mut cursor = 0usize;
    let push = |text: &str, kind: TokenKind, spans: &mut Vec<TextSpan>| {
        if text.is_empty() {
            return;
        }
        spans.push(span(text.to_string()).color(theme.color(kind)).size(fs as f32).font_family(MONO));
    };
    for t in tokens {
        let start = t.start.min(src.len());
        let end = (t.start + t.len).min(src.len());
        if start < cursor || start > end {
            continue; // defensive: skip malformed/overlapping tokens
        }
        if start > cursor {
            push(&src[cursor..start], TokenKind::Plain, &mut spans);
        }
        push(&src[start..end], t.kind, &mut spans);
        cursor = end;
    }
    if cursor < src.len() {
        push(&src[cursor..], TokenKind::Plain, &mut spans);
    }
    spans
}

impl IntoWidget for CodeEditor {
    fn into_widget(self) -> AnyWidget {
        // `code`/`read_only` are retained on the builder for the editing milestone (see
        // the crate docs); v1 renders a highlighted, scrollable view of the buffer.
        let CodeEditor { code, language, theme, font_size: fs, line_height: lh, height, gutter, read_only: _, title } =
            self;

        let src = code.get();
        let line_count = src.split('\n').count().max(1);

        // ---- highlighted content ----
        let tokens = language.as_ref().map(|l| l.highlight(&src)).unwrap_or_default();
        let spans = to_spans(&src, &tokens, &theme, fs);
        let content = text_rich(spans).line_height(lh as f32);

        // ---- gutter ----
        let line_px = fs * lh;
        let row: AnyWidget = if gutter {
            let digits = line_count.max(1).to_string().len().max(2);
            let gutter_w = digits as f64 * fs * 0.62 + 20.0;
            let mut nums: Vec<AnyWidget> = Vec::new();
            for n in 1..=line_count {
                nums.push(
                    container()
                        .height(line_px)
                        .alignment(Alignment::CENTER_RIGHT)
                        .child(
                            text(n.to_string())
                                .size(fs as f32)
                                .line_height(lh as f32)
                                .font_family(MONO)
                                .color(theme.gutter_fg),
                        )
                        .into_widget(),
                );
            }
            let gutter_col = container()
                .width(gutter_w)
                .decoration(BoxDecoration::new().color(theme.gutter_bg))
                .padding(EdgeInsets::only(0.0, 10.0, 10.0, 10.0))
                .child(
                    column(nums).cross_axis_alignment(CrossAxisAlignment::Stretch).main_axis_size(MainAxisSize::Min),
                );
            row(children![
                gutter_col,
                expanded(container().padding(EdgeInsets::only(14.0, 10.0, 14.0, 10.0)).child(content)),
            ])
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .into_widget()
        } else {
            container().padding(EdgeInsets::all(12.0)).child(content).into_widget()
        };

        // ---- scroll viewport ----
        let scroller = scroll_view(row);
        let code_area: AnyWidget = match height {
            Some(h) => container()
                .height(h)
                .decoration(BoxDecoration::new().color(theme.background))
                .child(scroller)
                .into_widget(),
            None => container().decoration(BoxDecoration::new().color(theme.background)).child(scroller).into_widget(),
        };

        // ---- optional status bar + panel chrome ----
        let lang_name = language.as_ref().map(|l| l.name().to_string()).unwrap_or_else(|| "Plain Text".into());
        let border = theme.punctuation;
        let mut col: Vec<AnyWidget> = Vec::new();
        if let Some(t) = title {
            col.push(status_bar(&t, &lang_name, line_count, &theme).into_widget());
            col.push(container().height(1.0).decoration(BoxDecoration::new().color(border)).into_widget());
        }
        col.push(code_area);

        container()
            .decoration(
                BoxDecoration::new()
                    .color(theme.background)
                    .radius(BorderRadius::all(12.0))
                    .border(Border::new(with_alpha(border, 0.35), 1.0)),
            )
            .clip()
            .child(column(col).cross_axis_alignment(CrossAxisAlignment::Stretch).main_axis_size(MainAxisSize::Min))
            .into_widget()
    }
}

fn with_alpha(c: Color, a: f32) -> Color {
    let [r, g, b, _] = c.components;
    Color::new([r, g, b, a])
}

fn status_bar(title: &str, lang: &str, lines: usize, theme: &EditorTheme) -> impl IntoWidget {
    container()
        .decoration(BoxDecoration::new().color(theme.gutter_bg))
        .padding(EdgeInsets::symmetric(14.0, 9.0))
        .child(
            row(children![
                icon(lucide::FILE_CODE).size(14.0).color(theme.gutter_active_fg),
                gap_w(8.0),
                text(title.to_string()).size(12.5).weight(600.0).color(theme.foreground),
                spacer(),
                text(format!("{lang} · {lines} lines")).size(11.5).color(theme.gutter_fg),
            ])
            .cross_axis_alignment(CrossAxisAlignment::Center),
        )
}
