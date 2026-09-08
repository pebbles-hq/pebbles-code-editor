//! # pebbles-code-editor
//!
//! An embeddable, syntax-highlighting **code editor** widget for the
//! [Pebbles](https://github.com/pebbles-hq/pebbles) GUI framework — the foundation to
//! build an IDE on.
//!
//! Like every serious editor (CodeMirror, Monaco, AvaloniaEdit), this is a **custom
//! editing engine**, not the framework's text field: it owns the buffer, cursor,
//! selection, layout, and input. Because code is monospace it renders on a fixed grid
//! (caret x = column × advance, line y = row × line-height), which keeps caret placement
//! and click hit-testing exact and cheap.
//!
//! - **Real editing** — type, backspace/delete, word-delete, Enter (auto-indent), Tab,
//!   arrows + word/line/doc motions, Shift-select, Ctrl+A, and Copy/Cut/Paste.
//! - **Syntax highlighting** via a pluggable [`Language`] layer (Rust + JSON bundled).
//! - **Line-number gutter** with an active-line marker, **current-line highlight**,
//!   selection, and a caret.
//! - Swappable **themes** ([`EditorTheme`]).
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

pub mod lang;
mod theme;

pub use lang::{Language, Token, TokenKind};
pub use theme::EditorTheme;

use std::rc::Rc;

use pebbles::prelude::*;

/// The monospace family used for all editor text.
const MONO: &str = "JetBrains Mono";
/// Advance width of one glyph, as a fraction of the font size (JetBrains Mono ≈ 0.6).
const ADVANCE_RATIO: f64 = 0.6;

/// Build a code editor bound to `code`. Configure fluently, then drop it into any tree.
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
        autofocus: false,
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
    autofocus: bool,
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

struct Props {
    code: Signal<String>,
    language: Option<Box<dyn Language>>,
    theme: EditorTheme,
    fs: f64,
    lh: f64,
    height: Option<f64>,
    gutter: bool,
    read_only: bool,
    autofocus: bool,
    title: Option<String>,
}

impl From<CodeEditor> for Props {
    fn from(e: CodeEditor) -> Self {
        Props {
            code: e.code,
            language: e.language,
            theme: e.theme,
            fs: e.font_size,
            lh: e.line_height,
            height: e.height,
            gutter: e.gutter,
            read_only: e.read_only,
            autofocus: e.autofocus,
            title: e.title,
        }
    }
}

fn render_editor(p: &Props) -> AnyWidget {
    let code = p.code;
    let caret = create_signal(0usize);
    let anchor = create_signal(0usize);
    let goal = create_signal(0usize); // preserved column for Up/Down
    let focus = create_focus();

    // One-time: grab focus on mount if requested.
    if p.autofocus {
        create_effect(move || focus.request_focus());
    }

    // Register the key handler (semantic edit commands from the framework).
    let read_only = p.read_only;
    let tab = "    "; // 4 spaces
    focus.register_editor(Rc::new(move |k: KeyInput| {
        apply_key(k, code, caret, anchor, goal, read_only, tab);
    }));

    // ---- read model ----
    let src = code.get();
    let fs = p.fs;
    let lh = p.lh;
    let line_px = fs * lh;
    let advance = fs * ADVANCE_RATIO;
    let pad_l = 14.0;
    let pad_t = 10.0;
    let theme = &p.theme;

    let (car_a, car_c) = (anchor.get(), caret.get());
    let (lo, hi) = (car_a.min(car_c), car_a.max(car_c));
    let has_sel = lo != hi;
    let cl = line_of(&src, car_c);
    let cc = col_of(&src, car_c);
    let line_count = src.split('\n').count().max(1);
    let content_h = line_count as f64 * line_px + pad_t * 2.0;
    let focused = focus.is_focused();

    // ---- overlay layers on the monospace grid ----
    let mut layers: Vec<AnyWidget> = Vec::new();

    // current-line band (only when there's no selection)
    if !has_sel {
        layers.push(band(pad_t + cl as f64 * line_px, line_px, theme.current_line));
    }

    // selection rects
    if has_sel {
        let (la, ca) = (line_of(&src, lo), col_of(&src, lo));
        let (lb, cb) = (line_of(&src, hi), col_of(&src, hi));
        for line in la..=lb {
            let start_col = if line == la { ca } else { 0 };
            let end_col = if line == lb { cb } else { line_char_len(&src, line) + 1 };
            let x = pad_l + start_col as f64 * advance;
            let w = ((end_col.saturating_sub(start_col)) as f64 * advance).max(2.0);
            layers.push(
                Positioned::new(
                    container()
                        .width(w)
                        .height(line_px)
                        .decoration(BoxDecoration::new().color(theme.selection)),
                )
                .left(x)
                .top(pad_t + line as f64 * line_px)
                .into_widget(),
            );
        }
    }

    // highlighted text
    let tokens = p.language.as_ref().map(|l| l.highlight(&src)).unwrap_or_default();
    let spans = to_spans(&src, &tokens, theme, fs);
    layers.push(
        Positioned::new(text_rich(spans).line_height(lh as f32)).left(pad_l).top(pad_t).into_widget(),
    );

    // caret
    if focused {
        layers.push(
            Positioned::new(
                container().width(2.0).height(fs * 1.15).decoration(BoxDecoration::new().color(theme.caret)),
            )
            .left(pad_l + cc as f64 * advance)
            .top(pad_t + cl as f64 * line_px + (line_px - fs * 1.15) / 2.0)
            .into_widget(),
        );
    }

    let grid = stack(layers).fit(StackFit::Expand).alignment(Alignment::TOP_LEFT);

    // ---- mouse: click to place caret, drag to select ----
    let hit = move |pos: Offset, extend: bool| {
        let b = pos_to_byte(&code.peek(), pos, pad_l, pad_t, advance, line_px);
        caret.set(b);
        if !extend {
            anchor.set(b);
        }
        goal.set(col_of(&code.peek(), b));
    };
    let click_area = GestureDetector::new(container().height(content_h).child(grid))
        .on_pointer_down(action_event(move |e| {
            focus.request_focus();
            hit(e.position, false);
        }))
        .on_pan_update(action_event(move |e| hit(e.position, true)));

    // ---- gutter ----
    let body: AnyWidget = if p.gutter {
        let digits = line_count.to_string().len().max(2);
        let gutter_w = digits as f64 * advance + 22.0;
        let mut nums: Vec<AnyWidget> = Vec::new();
        for n in 0..line_count {
            let active = n == cl;
            nums.push(
                container()
                    .height(line_px)
                    .alignment(Alignment::CENTER_RIGHT)
                    .child(
                        text((n + 1).to_string())
                            .size(fs as f32)
                            .line_height(lh as f32)
                            .font_family(MONO)
                            .color(if active { theme.gutter_active_fg } else { theme.gutter_fg }),
                    )
                    .into_widget(),
            );
        }
        let gutter_col = container()
            .width(gutter_w)
            .decoration(BoxDecoration::new().color(theme.gutter_bg))
            .padding(EdgeInsets::only(0.0, pad_t, 8.0, 0.0))
            .child(column(nums).cross_axis_alignment(CrossAxisAlignment::Stretch).main_axis_size(MainAxisSize::Min));
        row(children![gutter_col, expanded(click_area)])
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .into_widget()
    } else {
        click_area.into_widget()
    };

    // With a fixed height the editor scrolls within a viewport; without one it grows to
    // its content (for inline, read-only snippets embedded in a page).
    let code_area: AnyWidget = match p.height {
        Some(h) => container()
            .height(h)
            .decoration(BoxDecoration::new().color(theme.background))
            .child(scroll_view(body))
            .into_widget(),
        None => container().decoration(BoxDecoration::new().color(theme.background)).child(body).into_widget(),
    };

    // ---- chrome ----
    let border = with_alpha(theme.punctuation, 0.35);
    let lang_name = p.language.as_ref().map(|l| l.name().to_string()).unwrap_or_else(|| "Plain Text".into());
    let mut col: Vec<AnyWidget> = Vec::new();
    if let Some(t) = &p.title {
        col.push(status_bar(t, &lang_name, cl + 1, cc + 1, theme).into_widget());
        col.push(container().height(1.0).decoration(BoxDecoration::new().color(border)).into_widget());
    }
    col.push(code_area);

    container()
        .decoration(
            BoxDecoration::new().color(theme.background).radius(BorderRadius::all(12.0)).border(Border::new(border, 1.0)),
        )
        .clip()
        .child(column(col).cross_axis_alignment(CrossAxisAlignment::Stretch).main_axis_size(MainAxisSize::Min))
        .into_widget()
}

fn band(top: f64, h: f64, color: Color) -> AnyWidget {
    Positioned::new(container().height(h).decoration(BoxDecoration::new().color(color)))
        .left(0.0)
        .right(0.0)
        .top(top)
        .into_widget()
}

fn status_bar(title: &str, lang: &str, ln: usize, col: usize, theme: &EditorTheme) -> impl IntoWidget {
    container()
        .decoration(BoxDecoration::new().color(theme.gutter_bg))
        .padding(EdgeInsets::symmetric(14.0, 9.0))
        .child(
            row(children![
                icon(lucide::FILE_CODE).size(14.0).color(theme.gutter_active_fg),
                gap_w(8.0),
                text(title.to_string()).size(12.5).weight(600.0).color(theme.foreground),
                spacer(),
                text(format!("{lang} · Ln {ln}, Col {col}")).size(11.5).color(theme.gutter_fg),
            ])
            .cross_axis_alignment(CrossAxisAlignment::Center),
        )
}

// ---------------------------------------------------------------------------
// Rich-text spans
// ---------------------------------------------------------------------------

fn to_spans(src: &str, tokens: &[Token], theme: &EditorTheme, fs: f64) -> Vec<TextSpan> {
    let mut spans = Vec::new();
    let mut cursor = 0usize;
    let push = |text: &str, kind: TokenKind, spans: &mut Vec<TextSpan>| {
        if !text.is_empty() {
            spans.push(span(text.to_string()).color(theme.color(kind)).size(fs as f32).font_family(MONO));
        }
    };
    for t in tokens {
        let start = t.start.min(src.len());
        let end = (t.start + t.len).min(src.len());
        if start < cursor || start > end || !src.is_char_boundary(start) || !src.is_char_boundary(end) {
            continue;
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

fn with_alpha(c: Color, a: f32) -> Color {
    let [r, g, b, _] = c.components;
    Color::new([r, g, b, a])
}

// ---------------------------------------------------------------------------
// Editing engine — apply a semantic key command to the buffer + cursor
// ---------------------------------------------------------------------------

fn apply_key(
    k: KeyInput,
    code: Signal<String>,
    caret: Signal<usize>,
    anchor: Signal<usize>,
    goal: Signal<usize>,
    read_only: bool,
    tab: &str,
) {
    let src = code.peek();
    let (a, c) = (anchor.peek().min(src.len()), caret.peek().min(src.len()));
    let (lo, hi) = (a.min(c), a.max(c));

    // Replace [lo,hi) with `ins`, placing the caret after it.
    let splice = |ins: &str| {
        let mut t = src.clone();
        t.replace_range(lo..hi, ins);
        let nc = lo + ins.len();
        code.set(t);
        caret.set(nc);
        anchor.set(nc);
        goal.set(col_of(&code.peek(), nc));
    };

    match k {
        KeyInput::Insert(s) if !read_only => splice(&s),
        KeyInput::Enter if !read_only => {
            // Auto-indent: carry the current line's leading whitespace.
            let ls = line_start(&src, lo);
            let indent: String = src[ls..].chars().take_while(|ch| *ch == ' ' || *ch == '\t').collect();
            splice(&format!("\n{indent}"));
        }
        KeyInput::Backspace if !read_only => {
            if lo != hi {
                splice("");
            } else if lo > 0 {
                let p = prev_char(&src, lo);
                let mut t = src.clone();
                t.replace_range(p..lo, "");
                code.set(t);
                caret.set(p);
                anchor.set(p);
                goal.set(col_of(&code.peek(), p));
            }
        }
        KeyInput::Delete if !read_only => {
            if lo != hi {
                splice("");
            } else if hi < src.len() {
                let n = next_char(&src, hi);
                let mut t = src.clone();
                t.replace_range(hi..n, "");
                code.set(t);
            }
        }
        KeyInput::DeleteWordBack if !read_only => {
            let start = if lo != hi { lo } else { prev_word(&src, lo) };
            let mut t = src.clone();
            t.replace_range(start..hi, "");
            code.set(t);
            caret.set(start);
            anchor.set(start);
        }
        KeyInput::DeleteWordForward if !read_only => {
            let end = if lo != hi { hi } else { next_word(&src, hi) };
            let mut t = src.clone();
            t.replace_range(lo..end, "");
            code.set(t);
            caret.set(lo);
            anchor.set(lo);
        }
        KeyInput::Move { motion, extend } => {
            let target = match motion {
                Motion::Left => prev_char(&src, c),
                Motion::Right => next_char(&src, c),
                Motion::WordLeft => prev_word(&src, c),
                Motion::WordRight => next_word(&src, c),
                Motion::LineStart => line_start(&src, c),
                Motion::LineEnd => line_end(&src, c),
                Motion::DocStart => 0,
                Motion::DocEnd => src.len(),
                Motion::Up => {
                    let l = line_of(&src, c);
                    if l == 0 { 0 } else { byte_at(&src, l - 1, goal.peek()) }
                }
                Motion::Down => byte_at(&src, line_of(&src, c) + 1, goal.peek()),
            };
            caret.set(target);
            if !extend {
                anchor.set(target);
            }
            if !matches!(motion, Motion::Up | Motion::Down) {
                goal.set(col_of(&src, target));
            }
        }
        KeyInput::SelectAll => {
            anchor.set(0);
            caret.set(src.len());
        }
        KeyInput::Copy => {
            if lo != hi {
                pebbles::core::clipboard::write(&src[lo..hi]);
            }
        }
        KeyInput::Cut if !read_only => {
            if lo != hi {
                pebbles::core::clipboard::write(&src[lo..hi]);
                splice("");
            }
        }
        KeyInput::Paste if !read_only => {
            let p = pebbles::core::clipboard::read();
            if !p.is_empty() {
                splice(&p);
            }
        }
        _ => {
            let _ = tab;
        }
    }
}

// ---------------------------------------------------------------------------
// Buffer geometry helpers (byte offsets over &str; all char-boundary safe)
// ---------------------------------------------------------------------------

fn line_of(src: &str, byte: usize) -> usize {
    src.get(..byte.min(src.len())).map(|s| s.bytes().filter(|b| *b == b'\n').count()).unwrap_or(0)
}
fn line_start(src: &str, byte: usize) -> usize {
    src.get(..byte.min(src.len())).and_then(|s| s.rfind('\n')).map(|i| i + 1).unwrap_or(0)
}
fn line_end(src: &str, byte: usize) -> usize {
    let b = byte.min(src.len());
    src[b..].find('\n').map(|i| b + i).unwrap_or(src.len())
}
fn col_of(src: &str, byte: usize) -> usize {
    let ls = line_start(src, byte);
    src.get(ls..byte.min(src.len())).map(|s| s.chars().count()).unwrap_or(0)
}
/// Byte length (in chars) of line `line`.
fn line_char_len(src: &str, line: usize) -> usize {
    src.split('\n').nth(line).map(|l| l.chars().count()).unwrap_or(0)
}
/// The byte offset at `(line, col)`, clamped to the line's end.
fn byte_at(src: &str, line: usize, col: usize) -> usize {
    let start = line_start_of(src, line);
    let end = line_end(src, start);
    let mut b = start;
    for (i, _) in src[start..end].char_indices().take(col) {
        b = start + i + src[start + i..].chars().next().map(char::len_utf8).unwrap_or(1);
    }
    b.min(end)
}
fn line_start_of(src: &str, line: usize) -> usize {
    if line == 0 {
        return 0;
    }
    let mut count = 0;
    for (i, b) in src.bytes().enumerate() {
        if b == b'\n' {
            count += 1;
            if count == line {
                return i + 1;
            }
        }
    }
    src.len()
}
fn prev_char(src: &str, byte: usize) -> usize {
    src.get(..byte.min(src.len())).and_then(|s| s.chars().next_back()).map(|ch| byte - ch.len_utf8()).unwrap_or(0)
}
fn next_char(src: &str, byte: usize) -> usize {
    let b = byte.min(src.len());
    src[b..].chars().next().map(|ch| b + ch.len_utf8()).unwrap_or(b)
}
fn is_word(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}
fn prev_word(src: &str, byte: usize) -> usize {
    let mut b = byte.min(src.len());
    // skip whitespace
    while b > 0 {
        let p = prev_char(src, b);
        if src[p..b].chars().next().is_some_and(|ch| ch.is_whitespace()) {
            b = p;
        } else {
            break;
        }
    }
    // skip a run of word (or non-word) chars
    let word = b > 0 && src[prev_char(src, b)..b].chars().next().is_some_and(is_word);
    while b > 0 {
        let p = prev_char(src, b);
        let ch = src[p..b].chars().next().unwrap_or(' ');
        if ch.is_whitespace() || is_word(ch) != word {
            break;
        }
        b = p;
    }
    b
}
fn next_word(src: &str, byte: usize) -> usize {
    let mut b = byte.min(src.len());
    while b < src.len() && src[b..].chars().next().is_some_and(|ch| ch.is_whitespace()) {
        b = next_char(src, b);
    }
    let word = b < src.len() && src[b..].chars().next().is_some_and(is_word);
    while b < src.len() {
        let ch = src[b..].chars().next().unwrap_or(' ');
        if ch.is_whitespace() || is_word(ch) != word {
            break;
        }
        b = next_char(src, b);
    }
    b
}
fn pos_to_byte(src: &str, pos: Offset, pad_l: f64, pad_t: f64, advance: f64, line_px: f64) -> usize {
    let line = (((pos.y - pad_t) / line_px).floor()).max(0.0) as usize;
    let col = (((pos.x - pad_l) / advance).round()).max(0.0) as usize;
    let last = src.split('\n').count().saturating_sub(1);
    byte_at(src, line.min(last), col)
}
