//! # pebbles-code-editor
//!
//! An embeddable, syntax-highlighting **code editor** widget for the
//! [Pebbles](https://github.com/pebbles-hq/pebbles) GUI framework — the foundation to
//! build an IDE on.
//!
//! This is a **custom editing engine**, not the framework's text field: it owns the
//! buffer, cursor, selection, layout, and input. Because code is monospace it renders
//! on a fixed grid
//! (caret x = column × advance, line y = row × line-height), which keeps caret placement
//! and click hit-testing exact and cheap.
//!
//! - **Real editing** — type, backspace/delete, word-delete, Enter (auto-indent), Tab,
//!   arrows + word/line/doc motions, Shift-select, Ctrl+A, and Copy/Cut/Paste — all built
//!   on an invertible [`ChangeSet`]/[`Transaction`] model with undo/redo.
//! - **Multiple cursors & selections** — a [`Selections`] set (sorted, merged, with a
//!   primary): Alt-click adds a caret, Ctrl/Cmd+D adds the next occurrence, Shift+Alt-drag
//!   makes a rectangular (column) selection, double-click selects a word, triple-click a
//!   line, Esc collapses; every edit and motion applies to all cursors at once.
//! - **Caret autoscroll** — keyboard navigation keeps the caret in view (when a fixed
//!   `height` makes the editor scroll).
//! - **Syntax highlighting** via a pluggable [`Language`] layer (Rust + JSON bundled).
//! - **Line-number gutter** with an active-line marker, **current-line highlight**,
//!   selection, and a blinking caret.
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

pub mod edit;
pub mod lang;
mod theme;

pub use edit::{ChangeSet, EditorState, History, Selection, Selections, Transaction};
pub use lang::{Language, Token, TokenKind};
pub use theme::EditorTheme;

use std::cell::RefCell;
use std::rc::Rc;

use pebbles::prelude::*;
use pebbles::render::ScrollHandle;
use ropey::Rope;

use crate::edit::{Change, Coalesce};

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
        current_line: true,
        context_menu: true,
        tab_size: 4,
        insert_spaces: true,
        indent_guides: false,
        render_whitespace: false,
        rulers: Vec::new(),
        minimap: false,
        sticky_scroll: false,
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
    current_line: bool,
    context_menu: bool,
    tab_size: usize,
    insert_spaces: bool,
    indent_guides: bool,
    render_whitespace: bool,
    rulers: Vec<usize>,
    minimap: bool,
    sticky_scroll: bool,
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
    current_line: bool,
    context_menu: bool,
    tab_size: usize,
    insert_spaces: bool,
    indent_guides: bool,
    render_whitespace: bool,
    rulers: Vec<usize>,
    minimap: bool,
    sticky_scroll: bool,
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
            current_line: e.current_line,
            context_menu: e.context_menu,
            tab_size: e.tab_size,
            insert_spaces: e.insert_spaces,
            indent_guides: e.indent_guides,
            render_whitespace: e.render_whitespace,
            rulers: e.rulers,
            minimap: e.minimap,
            sticky_scroll: e.sticky_scroll,
            title: e.title,
        }
    }
}

fn render_editor(p: &Props) -> AnyWidget {
    let code = p.code;
    // The document + selection live in one immutable-style state, mutated only by
    // dispatching transactions (see the `edit` module). `code` (the public `Signal<String>`)
    // is kept in sync so callers still bind to a plain string.
    let state = create_signal(EditorState::new(&code.peek()));
    let history = create_signal(Rc::new(RefCell::new(History::new())));
    let goal = create_signal(0usize); // preserved column for Up/Down
    let blink_stamp = create_signal(0.0_f64); // loop time of the last edit/move — caret solid then
    let drag_anchor = create_signal::<Option<Offset>>(None); // pointer-down pos for column select
    let scroll_top = create_signal(0.0_f64); // live vertical scroll offset (drives virtualization)
    // Imperative scroll handles — drive caret-into-view autoscroll (only used when a fixed
    // `height` makes the editor a scroll viewport). Held in signals so they survive renders.
    let scroll = create_signal(ScrollHandle::new()).peek(); // vertical
    let hscroll = create_signal(ScrollHandle::new()).peek(); // horizontal
    let focus = create_focus();

    // Two-way binding: if the caller replaces `code` from the outside (e.g. loads a file),
    // rebuild the state from it. The editor's own edits write `code` == `state.text()`, so
    // this effect no-ops on them (it only fires for genuinely external changes).
    create_effect(move || {
        let external = code.get();
        if external != state.peek().text() {
            let sel = state.peek().selection.clamped(external.len());
            state.set(EditorState {
                doc: Rope::from_str(&external),
                selection: sel,
            });
        }
    });

    // One-time: grab focus on mount if requested.
    if p.autofocus {
        create_effect(move || focus.request_focus());
    }

    // Register the key handler (semantic edit commands from the framework).
    let read_only = p.read_only;
    let indent_unit: String = if p.insert_spaces {
        " ".repeat(p.tab_size)
    } else {
        "\t".to_string()
    };
    {
        let t = indent_unit.clone();
        // Register as a *code* editor so the shell routes Tab/Shift+Tab here (indent/outdent)
        // instead of moving focus.
        focus.register_code_editor(Rc::new(move |k: KeyInput| {
            apply_key(k, state, history, code, goal, read_only, &t)
        }));
    }

    // ---- read model ----
    let st = state.get();
    let src = st.text();
    let fs = p.fs;
    let lh = p.lh;
    let line_px = fs * lh;
    let advance = fs * ADVANCE_RATIO;
    let pad_l = 14.0;
    let pad_t = 10.0;
    let theme = &p.theme;

    // All cursors/selections; the primary drives the current-line band + status line.
    let sels = st.selection.clamped(src.len());
    let primary = sels.primary();
    let (pcc, has_primary_sel) = (primary.head, !primary.is_empty());
    let cl = line_of(&src, pcc);
    let cc = col_of(&src, pcc);
    let line_count = src.split('\n').count().max(1);
    let content_h = line_count as f64 * line_px + pad_t * 2.0;
    let focused = focus.is_focused();

    // ---- viewport virtualization ----
    // With a fixed height we render only the lines in view (plus a small overscan), so a
    // 100k-line file costs the same as a screenful. Reading `scroll_top` here makes the
    // component re-render as the viewport scrolls (fed by the scroll view's `on_scroll`).
    // Without a height the editor grows to its content, so every line is "visible".
    const OVERSCAN: usize = 4;
    let (first_line, last_line) = if let Some(h) = p.height {
        let top = scroll_top.get();
        let first = (((top - pad_t) / line_px).floor() as isize - OVERSCAN as isize).max(0) as usize;
        let rows = (h / line_px).ceil() as usize + OVERSCAN * 2;
        let last = (first + rows).min(line_count - 1);
        (first.min(line_count - 1), last)
    } else {
        (0, line_count - 1)
    };

    // Caret blink: a 0.5s phase that ticks ONLY while focused, reset to solid on any edit
    // or caret move (the `blink_stamp` effect below), so the caret is steady while you work.
    let blink_loop = create_loop_while(focused, 0.5);
    // Reset the blink phase to "solid" on any state change (edit, caret move, click).
    create_effect(move || {
        let _ = state.get();
        blink_stamp.set(blink_loop.peek());
    });
    let caret_on =
        focused && (blink_loop.get() - blink_stamp.get()).rem_euclid(1.0) < 0.5;

    // Autoscroll: keep the primary caret in view on every edit/move (only meaningful when a
    // fixed `height` makes the editor a scroll viewport). Runs on any state change.
    let scroll_a = scroll.clone();
    let hscroll_a = hscroll.clone();
    if let Some(vh) = p.height {
        create_effect(move || {
            let st = state.get();
            let text = st.text();
            let head = st.primary().head;
            // Vertical: bring the caret's line into view. `scroll_top` (the signal fed by the
            // scroll view) is the source of truth for the current offset; we compute the new
            // target from it and set it optimistically so the virtualized window follows now,
            // then command the scroll view to match. Only moves when the caret is off-screen,
            // so it never fights a user scroll.
            let line = line_of(&text, head);
            let top = pad_t + line as f64 * line_px;
            let bottom = top + line_px;
            let cur = scroll_top.peek();
            let margin = line_px;
            let target = if top < cur + margin {
                Some((top - margin).max(0.0))
            } else if bottom > cur + vh - margin {
                Some((bottom - vh + margin).max(0.0))
            } else {
                None
            };
            if let Some(t) = target {
                scroll_a.scroll_to(t);
                scroll_top.set(t.min((content_h - vh).max(0.0)));
            }
            // Horizontal: keep the caret's column in view (a few columns of margin).
            let x = pad_l + col_of(&text, head) as f64 * advance;
            hscroll_a.ensure_visible(x, x + advance, advance * 4.0);
        });
    }

    // ---- overlay layers on the monospace grid ----
    let mut layers: Vec<AnyWidget> = Vec::new();

    // current-line band (primary caret's line, only when the primary has no selection)
    if p.current_line && !has_primary_sel && (first_line..=last_line).contains(&cl) {
        layers.push(band(
            pad_t + cl as f64 * line_px,
            line_px,
            theme.current_line,
        ));
    }

    // rulers / print-margin — thin full-height vertical lines at the configured columns.
    for &rc in &p.rulers {
        layers.push(
            Positioned::new(
                container()
                    .width(1.0)
                    .height(content_h)
                    .decoration(BoxDecoration::new().color(theme.ruler)),
            )
            .left(pad_l + rc as f64 * advance)
            .top(0.0)
            .into_widget(),
        );
    }

    // indent guides — a faint vertical line at each indent level inside a line's leading
    // whitespace (visible lines only).
    if p.indent_guides {
        let step = p.tab_size.max(1);
        for line in first_line..=last_line {
            let ls = line_start_of(&src, line);
            let le = line_end(&src, ls);
            let lead = src[ls..le]
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .count();
            let mut gcol = step;
            while gcol < lead {
                layers.push(
                    Positioned::new(
                        container()
                            .width(1.0)
                            .height(line_px)
                            .decoration(BoxDecoration::new().color(theme.indent_guide)),
                    )
                    .left(pad_l + gcol as f64 * advance)
                    .top(pad_t + line as f64 * line_px)
                    .into_widget(),
                );
                gcol += step;
            }
        }
    }

    // selection rects — one set per non-empty range (multi-cursor), clipped to the window.
    for r in sels.ranges() {
        let (lo, hi) = (r.min(), r.max());
        if lo == hi {
            continue;
        }
        let (la, ca) = (line_of(&src, lo), col_of(&src, lo));
        let (lb, cb) = (line_of(&src, hi), col_of(&src, hi));
        for line in la.max(first_line)..=lb.min(last_line) {
            let start_col = if line == la { ca } else { 0 };
            let end_col = if line == lb {
                cb
            } else {
                line_char_len(&src, line) + 1
            };
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

    // highlighted text — only the visible block, as one rich-text positioned at the first
    // visible line. Tokens are computed for the whole doc (for correct multi-line context)
    // then sliced to the window, so we never lay out or paint offscreen text.
    let tokens = p
        .language
        .as_ref()
        .map(|l| l.highlight(&src))
        .unwrap_or_default();
    let slice_start = line_start_of(&src, first_line);
    let slice_end = line_end(&src, line_start_of(&src, last_line));
    let visible_src = &src[slice_start..slice_end];
    let vis_tokens = slice_tokens(&tokens, slice_start, slice_end);
    let spans = to_spans(visible_src, &vis_tokens, theme, fs);
    layers.push(
        Positioned::new(text_rich(spans).line_height(lh as f32))
            .left(pad_l)
            .top(pad_t + first_line as f64 * line_px)
            .into_widget(),
    );

    // whitespace / EOL rendering — a single faint overlay aligned to the text (spaces→·,
    // tabs→→, everything else blanked to preserve columns) plus a ¶ at each visible line end.
    if p.render_whitespace {
        let marks: String = visible_src
            .chars()
            .map(|c| match c {
                ' ' => '·',
                '\t' => '→',
                '\n' => '\n',
                _ => ' ',
            })
            .collect();
        layers.push(
            Positioned::new(
                text_rich(vec![
                    span(marks)
                        .size(fs as f32)
                        .font_family(MONO)
                        .color(theme.whitespace),
                ])
                .line_height(lh as f32),
            )
            .left(pad_l)
            .top(pad_t + first_line as f64 * line_px)
            .into_widget(),
        );
        for line in first_line..=last_line {
            let x = pad_l + line_char_len(&src, line) as f64 * advance;
            layers.push(
                Positioned::new(
                    text("¶".to_string())
                        .size((fs * 0.9) as f32)
                        .line_height(lh as f32)
                        .font_family(MONO)
                        .color(theme.whitespace),
                )
                .left(x)
                .top(pad_t + line as f64 * line_px)
                .into_widget(),
            );
        }
    }

    // carets — one per range's head (multi-cursor); all blink in phase.
    if caret_on {
        for r in sels.ranges() {
            let l = line_of(&src, r.head);
            if !(first_line..=last_line).contains(&l) {
                continue;
            }
            let col = col_of(&src, r.head);
            layers.push(
                Positioned::new(
                    container()
                        .width(2.0)
                        .height(fs * 1.15)
                        .decoration(BoxDecoration::new().color(theme.caret)),
                )
                .left(pad_l + col as f64 * advance)
                .top(pad_t + l as f64 * line_px + (line_px - fs * 1.15) / 2.0)
                .into_widget(),
            );
        }
    }

    let grid = stack(layers)
        .fit(StackFit::Expand)
        .alignment(Alignment::TOP_LEFT);

    // ---- mouse: click to place caret, drag to select, Alt-click to add a cursor ----
    // `add` (Alt) appends a new caret; `extend` (Shift / drag) grows the primary selection;
    // a plain click collapses to a single caret.
    let hit = move |pos: Offset, extend: bool, add: bool| {
        let cur = state.peek();
        let text = cur.text();
        let b = pos_to_byte(&text, pos, pad_l, pad_t, advance, line_px);
        let next = if add {
            cur.selection.pushed(Selection::caret(b))
        } else if extend {
            let anchor = cur.selection.primary().anchor.min(text.len());
            Selections::single(Selection::range(anchor, b))
        } else {
            Selections::single(Selection::caret(b))
        };
        state.set(EditorState {
            doc: cur.doc.clone(),
            selection: next,
        });
        goal.set(col_of(&text, b));
        // A click/drag ends any typing run, so the next keystroke starts a new undo step.
        history.peek().borrow_mut().break_group();
    };
    // Double-click selects the word under the pointer (new single selection).
    let word_select = move |pos: Offset| {
        let cur = state.peek();
        let text = cur.text();
        let b = pos_to_byte(&text, pos, pad_l, pad_t, advance, line_px);
        let (s, e) = word_at(&text, b);
        state.set(EditorState {
            doc: cur.doc.clone(),
            selection: Selections::single(Selection::range(s, e)),
        });
        goal.set(col_of(&text, e));
        history.peek().borrow_mut().break_group();
    };
    // Triple-click selects the whole line under the pointer (including its newline).
    let line_select = move |pos: Offset| {
        let cur = state.peek();
        let text = cur.text();
        let b = pos_to_byte(&text, pos, pad_l, pad_t, advance, line_px);
        let s = line_start(&text, b);
        let le = line_end(&text, b);
        let e = if le < text.len() { next_char(&text, le) } else { le };
        state.set(EditorState {
            doc: cur.doc.clone(),
            selection: Selections::single(Selection::range(s, e)),
        });
        goal.set(col_of(&text, e));
        history.peek().borrow_mut().break_group();
    };
    // Column (rectangular) selection: Shift+Alt+drag makes one caret/range per row across
    // the dragged rectangle — a caret per line if the two edges share a column.
    let column_select = move |from: Offset, to: Offset| {
        let cur = state.peek();
        let text = cur.text();
        let (la, ca) = pos_to_grid(&text, from, pad_l, pad_t, advance, line_px);
        let (lb, cb) = pos_to_grid(&text, to, pad_l, pad_t, advance, line_px);
        let (l0, l1) = (la.min(lb), la.max(lb));
        let rows: Vec<Selection> = (l0..=l1)
            .map(|line| Selection::range(byte_at(&text, line, ca), byte_at(&text, line, cb)))
            .collect();
        // Primary follows the moving edge (the row the pointer is on).
        let pi = if lb >= la { rows.len() - 1 } else { 0 };
        state.set(EditorState {
            doc: cur.doc.clone(),
            selection: Selections::new(rows, pi),
        });
        goal.set(cb);
        history.peek().borrow_mut().break_group();
    };
    // With a fixed height, bound the content to the widest line so it can overflow and
    // scroll horizontally (the gutter stays fixed, outside the horizontal scroll). Without a
    // height the editor grows to content, so the grid fills naturally.
    let content_w = p.height.map(|_| {
        let max_cols = src.split('\n').map(|l| l.chars().count()).max().unwrap_or(0);
        pad_l * 2.0 + max_cols as f64 * advance
    });
    let content_box = match content_w {
        Some(w) => container().width(w).height(content_h).child(grid),
        None => container().height(content_h).child(grid),
    };
    let click_area = GestureDetector::new(content_box)
        .on_pointer_down(action_event(move |e| {
            focus.request_focus();
            let alt = pebbles::core::keyboard::alt_held();
            let shift = pebbles::core::keyboard::shift_held();
            drag_anchor.set(Some(e.position)); // remember where a drag begins
            if shift && alt {
                // Shift+Alt starts a column drag — don't place/add a caret on the press.
            } else {
                // Alt-click adds a caret; Shift-click extends; a plain click collapses.
                hit(e.position, shift, alt);
            }
        }))
        .on_pan_start(action_event(move |e| drag_anchor.set(Some(e.position))))
        .on_pan_update(action_event(move |e| {
            let alt = pebbles::core::keyboard::alt_held();
            let shift = pebbles::core::keyboard::shift_held();
            if shift && alt {
                let from = drag_anchor.peek().unwrap_or(e.position);
                column_select(from, e.position);
            } else {
                hit(e.position, true, false);
            }
        }))
        .on_double_tap(action_event(move |e| word_select(e.position)))
        .on_triple_tap(action_event(move |e| line_select(e.position)));

    // Horizontal scroll: wrap the content (not the gutter) so long lines scroll sideways
    // while the line numbers stay put. Only when the editor is a bounded viewport.
    let content_area: AnyWidget = if content_w.is_some() {
        SingleChildScrollView::horizontal(click_area)
            .controller(hscroll.clone())
            .into_widget()
    } else {
        click_area.into_widget()
    };

    // ---- gutter ----
    // Virtualized to match the text: only the visible line numbers are built, each absolutely
    // positioned at its row inside a full-height (`content_h`) column so the scroll range and
    // vertical alignment stay exact.
    let body: AnyWidget = if p.gutter {
        let digits = line_count.to_string().len().max(2);
        let gutter_w = digits as f64 * advance + 22.0;
        let mut nums: Vec<AnyWidget> = Vec::new();
        for n in first_line..=last_line {
            let active = n == cl;
            nums.push(
                Positioned::new(
                    container()
                        .width(gutter_w - 8.0)
                        .height(line_px)
                        .alignment(Alignment::CENTER_RIGHT)
                        .child(
                            text((n + 1).to_string())
                                .size(fs as f32)
                                .line_height(lh as f32)
                                .font_family(MONO)
                                .color(if active {
                                    theme.gutter_active_fg
                                } else {
                                    theme.gutter_fg
                                }),
                        ),
                )
                .left(0.0)
                .top(pad_t + n as f64 * line_px)
                .into_widget(),
            );
        }
        let gutter_col = container()
            .width(gutter_w)
            .height(content_h)
            .decoration(BoxDecoration::new().color(theme.gutter_bg))
            .child(stack(nums).alignment(Alignment::TOP_LEFT))
            .into_widget();
        row(children![gutter_col, expanded(content_area)])
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .into_widget()
    } else {
        content_area
    };

    // Right-click menu (configurable via `.context_menu(false)`), driving the same edit
    // commands as the keyboard.
    let body: AnyWidget = if p.context_menu {
        context_menu(body)
            .item(menu_item("Cut").on_select(move || {
                apply_key(KeyInput::Cut, state, history, code, goal, read_only, "")
            }))
            .item(menu_item("Copy").on_select(move || {
                apply_key(KeyInput::Copy, state, history, code, goal, read_only, "")
            }))
            .item(menu_item("Paste").on_select(move || {
                apply_key(KeyInput::Paste, state, history, code, goal, read_only, "")
            }))
            .separator()
            .item(menu_item("Select All").on_select(move || {
                apply_key(
                    KeyInput::SelectAll,
                    state,
                    history,
                    code,
                    goal,
                    read_only,
                    "",
                )
            }))
            .into_widget()
    } else {
        body
    };

    // ---- sticky scroll ----
    // The enclosing scopes of the top visible line, derived from indentation: walking up,
    // each line with strictly smaller indent than the last is an ancestor scope. We pin the
    // ones that have scrolled above the viewport top.
    let sticky_lines: Vec<usize> = if p.sticky_scroll && p.height.is_some() {
        let indent_of = |ln: usize| -> Option<usize> {
            let ls = line_start_of(&src, ln);
            let le = line_end(&src, ls);
            let s = &src[ls..le];
            if s.trim().is_empty() {
                None // blank lines don't open scopes
            } else {
                Some(s.chars().take_while(|c| *c == ' ' || *c == '\t').count())
            }
        };
        let top = scroll_top.get();
        let top_line = (((top - pad_t) / line_px).floor().max(0.0) as usize).min(line_count - 1);
        let mut acc = Vec::new();
        if let Some(mut ci) = indent_of(top_line).or_else(|| {
            // blank top line: use the next non-blank line's indent as the reference
            (top_line..line_count).find_map(indent_of)
        }) {
            let mut ln = top_line;
            while ln > 0 && acc.len() < 6 {
                ln -= 1;
                if let Some(ind) = indent_of(ln)
                    && ind < ci
                    && (pad_t + ln as f64 * line_px) < top
                {
                    acc.push(ln);
                    ci = ind;
                    if ci == 0 {
                        break;
                    }
                }
            }
        }
        acc.reverse(); // outermost scope first
        acc
    } else {
        Vec::new()
    };

    // ---- minimap ----
    // A scaled overview of the WHOLE document drawn in a single canvas node (so it stays
    // cheap on huge files): one faint bar per line (indent-offset, length-scaled), with a
    // translucent viewport indicator. Click/drag scrolls the editor. Bounded viewport only.
    let minimap_panel: Option<AnyWidget> = match (p.minimap, p.height) {
        (true, Some(h)) => {
            const MM_W: f64 = 84.0;
            let metrics: Vec<(f32, f32)> = src
                .split('\n')
                .map(|l| {
                    let indent = l.chars().take_while(|c| *c == ' ' || *c == '\t').count() as f32;
                    (indent, l.chars().count() as f32)
                })
                .collect();
            let cur_top = scroll_top.get();
            let ch = content_h;
            let ink = with_alpha(theme.foreground, 0.45);
            let vp = with_alpha(theme.gutter_active_fg, 0.16);
            let painter = move |cv: &mut Canvas<'_>| {
                let size = cv.size();
                let (mm_w, mm_h) = (size.width, size.height);
                let n = metrics.len().max(1);
                let rows = (mm_h.floor() as usize).clamp(1, n);
                let char_w = (mm_w - 6.0) / 90.0; // ~90 columns across
                for row in 0..rows {
                    let li = row * n / rows;
                    let (indent, len) = metrics.get(li).copied().unwrap_or((0.0, 0.0));
                    if len <= 0.0 {
                        continue;
                    }
                    let y = row as f64 * mm_h / rows as f64;
                    let x0 = 3.0 + indent as f64 * char_w;
                    let x1 = (x0 + (len - indent).max(0.0) as f64 * char_w).min(mm_w - 3.0);
                    if x1 > x0 {
                        cv.fill_rect(Rect::new(x0, y, x1, y + 1.5), ink);
                    }
                }
                // viewport indicator
                let vy = (cur_top / ch) * mm_h;
                let vh = (h / ch) * mm_h;
                cv.fill_rect(Rect::new(0.0, vy, mm_w, (vy + vh).min(mm_h)), vp);
            };
            // Click/drag on the minimap centers the viewport on that fraction of the doc.
            let jump = move |scroll_mm: &ScrollHandle, pos: Offset| {
                let frac = (pos.y / h).clamp(0.0, 1.0);
                let t = (frac * ch - h / 2.0).max(0.0);
                scroll_mm.scroll_to(t);
                // Nudge the offset signal so the move is reactive (marks the view dirty and
                // moves the virtualized window now); the scroll view's `on_scroll` then
                // confirms the exact clamped offset.
                scroll_top.set(t.min((ch - h).max(0.0)));
            };
            let (down_scroll, pan_scroll) = (scroll.clone(), scroll.clone());
            Some(
                GestureDetector::new(
                    container()
                        .width(MM_W)
                        .height(h)
                        .decoration(BoxDecoration::new().color(theme.gutter_bg))
                        .child(canvas(painter).width(MM_W).height(h)),
                )
                .on_pointer_down(action_event(move |e| jump(&down_scroll, e.position)))
                .on_pan_update(action_event(move |e| jump(&pan_scroll, e.position)))
                .into_widget(),
            )
        }
        _ => None,
    };

    // With a fixed height the editor scrolls within a viewport; without one it grows to
    // its content (for inline, read-only snippets embedded in a page).
    let code_area: AnyWidget = match p.height {
        Some(h) => {
            let scroller = scroll_view(body)
                .controller(scroll.clone())
                // Re-render the visible window as the viewport scrolls.
                .on_scroll(move |n| scroll_top.set(n.metrics.pixels));
            // Sticky scope headers overlaid at the top of the scroller. The panel is a
            // pointer barrier (absorb_pointer): it pins the enclosing scopes and swallows
            // clicks so they never fall through to the content hidden behind it.
            let scroller: AnyWidget = if sticky_lines.is_empty() {
                scroller.into_widget()
            } else {
                let mut rows: Vec<AnyWidget> = Vec::new();
                for &ln in &sticky_lines {
                    let ls = line_start_of(&src, ln);
                    let le = line_end(&src, ls);
                    let spans = to_spans(&src[ls..le], &slice_tokens(&tokens, ls, le), theme, fs);
                    rows.push(
                        container()
                            .height(line_px)
                            .decoration(BoxDecoration::new().color(theme.background))
                            .padding(EdgeInsets::only(pad_l, 0.0, 0.0, 0.0))
                            .child(text_rich(spans).line_height(lh as f32))
                            .into_widget(),
                    );
                }
                rows.push(
                    container()
                        .height(1.0)
                        .decoration(BoxDecoration::new().color(with_alpha(theme.punctuation, 0.35)))
                        .into_widget(),
                );
                let sticky_h = sticky_lines.len() as f64 * line_px + 1.0;
                let panel = absorb_pointer(
                    container()
                        .height(sticky_h)
                        .decoration(BoxDecoration::new().color(theme.background))
                        .child(column(rows).main_axis_size(MainAxisSize::Min)),
                );
                stack(children![
                    scroller.into_widget(),
                    Positioned::new(panel)
                        .left(0.0)
                        .right(0.0)
                        .top(0.0)
                        .height(sticky_h)
                        .into_widget(),
                ])
                .fit(StackFit::Expand)
                .into_widget()
            };
            let inner: AnyWidget = match minimap_panel {
                Some(mm) => row(children![expanded(scroller), mm])
                    .cross_axis_alignment(CrossAxisAlignment::Start)
                    .into_widget(),
                None => scroller,
            };
            container()
                .height(h)
                .decoration(BoxDecoration::new().color(theme.background))
                .child(inner)
                .into_widget()
        }
        None => container()
            .decoration(BoxDecoration::new().color(theme.background))
            .child(body)
            .into_widget(),
    };

    // ---- chrome ----
    let border = with_alpha(theme.punctuation, 0.35);
    let lang_name = p
        .language
        .as_ref()
        .map(|l| l.name().to_string())
        .unwrap_or_else(|| "Plain Text".into());
    let mut col: Vec<AnyWidget> = Vec::new();
    if let Some(t) = &p.title {
        col.push(status_bar(t, &lang_name, cl + 1, cc + 1, theme).into_widget());
        col.push(
            container()
                .height(1.0)
                .decoration(BoxDecoration::new().color(border))
                .into_widget(),
        );
    }
    col.push(code_area);

    container()
        .decoration(
            BoxDecoration::new()
                .color(theme.background)
                .radius(BorderRadius::all(12.0))
                .border(Border::new(border, 1.0)),
        )
        .clip()
        .child(
            column(col)
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .main_axis_size(MainAxisSize::Min),
        )
        .into_widget()
}

fn band(top: f64, h: f64, color: Color) -> AnyWidget {
    Positioned::new(
        container()
            .height(h)
            .decoration(BoxDecoration::new().color(color)),
    )
    .left(0.0)
    .right(0.0)
    .top(top)
    .into_widget()
}

fn status_bar(
    title: &str,
    lang: &str,
    ln: usize,
    col: usize,
    theme: &EditorTheme,
) -> impl IntoWidget {
    container()
        .decoration(BoxDecoration::new().color(theme.gutter_bg))
        .padding(EdgeInsets::symmetric(14.0, 9.0))
        .child(
            row(children![
                icon(lucide::FILE_CODE)
                    .size(14.0)
                    .color(theme.gutter_active_fg),
                gap_w(8.0),
                text(title.to_string())
                    .size(12.5)
                    .weight(600.0)
                    .color(theme.foreground),
                spacer(),
                text(format!("{lang} · Ln {ln}, Col {col}"))
                    .size(11.5)
                    .color(theme.gutter_fg),
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
            spans.push(
                span(text.to_string())
                    .color(theme.color(kind))
                    .size(fs as f32)
                    .font_family(MONO),
            );
        }
    };
    for t in tokens {
        let start = t.start.min(src.len());
        let end = (t.start + t.len).min(src.len());
        if start < cursor
            || start > end
            || !src.is_char_boundary(start)
            || !src.is_char_boundary(end)
        {
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

/// Clip `tokens` to the byte range `from..to` and rebase their offsets to the start of that
/// slice — so the visible block can be highlighted as its own rich-text (virtualization).
fn slice_tokens(tokens: &[Token], from: usize, to: usize) -> Vec<Token> {
    tokens
        .iter()
        .filter_map(|t| {
            let s = t.start.max(from);
            let e = (t.start + t.len).min(to);
            (e > s).then_some(Token {
                start: s - from,
                len: e - s,
                kind: t.kind,
            })
        })
        .collect()
}

fn with_alpha(c: Color, a: f32) -> Color {
    let [r, g, b, _] = c.components;
    Color::new([r, g, b, a])
}

// ---------------------------------------------------------------------------
// Editing engine — turn a semantic key command into a transaction and dispatch it
// ---------------------------------------------------------------------------

/// The single mutation path: apply `tx` to the state, record its inverse for undo (edits
/// only), and keep the public `code` signal in sync with the document.
fn dispatch(
    state: Signal<EditorState>,
    history: Signal<Rc<RefCell<History>>>,
    code: Signal<String>,
    tx: Transaction,
    coalesce: Coalesce,
) {
    let cur = state.peek();
    let (next, inverse) = cur.apply(&tx);
    if tx.is_edit() {
        history
            .peek()
            .borrow_mut()
            .record(inverse, coalesce, next.primary().head);
        code.set(next.text());
    }
    state.set(next);
}

fn apply_key(
    k: KeyInput,
    state: Signal<EditorState>,
    history: Signal<Rc<RefCell<History>>>,
    code: Signal<String>,
    goal: Signal<usize>,
    read_only: bool,
    tab: &str,
) {
    let st = state.peek();
    let src = st.text();
    let len = src.len();
    // Every cursor/selection; edits and motions apply to all of them (multi-cursor).
    let sels = st.selection.clamped(len);
    let ranges: Vec<Selection> = sels.ranges().to_vec();
    let primary_idx = sels.primary_index();
    let primary = sels.primary();
    let is_multi = sels.is_multi();
    let last = ranges.len().saturating_sub(1);

    // Core edit: apply `f(range) -> (from, to, insert)` to EVERY range as one atomic
    // transaction, leaving a caret after each inserted text. `f` sees the pre-edit
    // document (`src`); the accumulated `delta` keeps each caret correct as earlier
    // edits shift later offsets.
    let edit_ranges = |f: &dyn Fn(Selection) -> (usize, usize, String), coalesce: Coalesce| {
        let mut changes: Vec<Change> = Vec::with_capacity(ranges.len());
        let mut carets: Vec<Selection> = Vec::with_capacity(ranges.len());
        let mut delta: isize = 0;
        for &r in &ranges {
            let (from, to, ins) = f(r);
            let (from, to) = (from.min(to), from.max(to));
            let caret = (from as isize + delta + ins.len() as isize).max(0) as usize;
            delta += ins.len() as isize - (to as isize - from as isize);
            changes.push(Change {
                from,
                to,
                insert: ins,
            });
            carets.push(Selection::caret(caret));
        }
        let pi = primary_idx.min(carets.len().saturating_sub(1));
        let tx = Transaction::change_and_select(
            ChangeSet::from_changes(changes),
            Selections::new(carets, pi),
        );
        dispatch(state, history, code, tx, coalesce);
        goal.set(col_of(&state.peek().text(), state.peek().primary().head));
    };
    // Replace every selection (or insert at every caret) with `ins`.
    let replace = |ins: &str, coalesce: Coalesce| {
        edit_ranges(&|r: Selection| (r.min(), r.max(), ins.to_string()), coalesce);
    };
    // Move all cursors (edits nothing, records no history); ends any typing run.
    let select_many = |next: Selections| {
        state.set(EditorState {
            doc: state.peek().doc.clone(),
            selection: next,
        });
        history.peek().borrow_mut().break_group();
    };

    match k {
        KeyInput::Insert(s) if !read_only => {
            // A single character at a single empty caret coalesces into one undo step; a
            // newline, a selection-replacement, or multi-cursor typing starts a fresh one.
            let coalesce = if !is_multi && primary.is_empty() && s != "\n" && s.chars().count() == 1
            {
                Coalesce::Typing
            } else {
                Coalesce::Never
            };
            replace(&s, coalesce);
        }
        KeyInput::Enter if !read_only => {
            // Auto-indent per caret: carry that line's leading whitespace, and add one level
            // after an opening bracket or a `:` (smart indent).
            edit_ranges(
                &|r| {
                    let at = r.min();
                    let ls = line_start(&src, at);
                    let cur = &src[ls..at];
                    let mut indent: String = cur
                        .chars()
                        .take_while(|ch| *ch == ' ' || *ch == '\t')
                        .collect();
                    if cur.trim_end().ends_with(['{', '(', '[', ':']) {
                        indent.push_str(tab);
                    }
                    (r.min(), r.max(), format!("\n{indent}"))
                },
                Coalesce::Never,
            );
        }
        KeyInput::Backspace if !read_only => {
            // Delete each selection, or the char before each bare caret.
            edit_ranges(
                &|r| {
                    if r.is_empty() {
                        (prev_char(&src, r.head), r.head, String::new())
                    } else {
                        (r.min(), r.max(), String::new())
                    }
                },
                Coalesce::Deleting,
            );
        }
        KeyInput::Delete if !read_only => {
            edit_ranges(
                &|r| {
                    if r.is_empty() {
                        (r.head, next_char(&src, r.head), String::new())
                    } else {
                        (r.min(), r.max(), String::new())
                    }
                },
                Coalesce::Never,
            );
        }
        KeyInput::DeleteWordBack if !read_only => {
            edit_ranges(
                &|r| {
                    let start = if r.is_empty() {
                        prev_word(&src, r.head)
                    } else {
                        r.min()
                    };
                    (start, r.max(), String::new())
                },
                Coalesce::Never,
            );
        }
        KeyInput::DeleteWordForward if !read_only => {
            edit_ranges(
                &|r| {
                    let end = if r.is_empty() {
                        next_word(&src, r.head)
                    } else {
                        r.max()
                    };
                    (r.min(), end, String::new())
                },
                Coalesce::Never,
            );
        }
        KeyInput::Move { motion, extend } => {
            // Move every caret. Up/Down keep a goal column: the shared `goal` for a single
            // caret, each caret's own column when there are several.
            let moved: Vec<Selection> = ranges
                .iter()
                .map(|&r| {
                    let col = if is_multi { col_of(&src, r.head) } else { goal.peek() };
                    let target = match motion {
                        Motion::Left => prev_char(&src, r.head),
                        Motion::Right => next_char(&src, r.head),
                        Motion::WordLeft => prev_word(&src, r.head),
                        Motion::WordRight => next_word(&src, r.head),
                        Motion::LineStart => line_start(&src, r.head),
                        Motion::LineEnd => line_end(&src, r.head),
                        Motion::DocStart => 0,
                        Motion::DocEnd => len,
                        Motion::Up => {
                            let l = line_of(&src, r.head);
                            if l == 0 { 0 } else { byte_at(&src, l - 1, col) }
                        }
                        Motion::Down => byte_at(&src, line_of(&src, r.head) + 1, col),
                    };
                    let anchor = if extend { r.anchor } else { target };
                    Selection::range(anchor, target)
                })
                .collect();
            select_many(Selections::new(moved, primary_idx.min(last)));
            if !is_multi && !matches!(motion, Motion::Up | Motion::Down) {
                goal.set(col_of(&src, state.peek().primary().head));
            }
        }
        KeyInput::SelectAll => select_many(Selections::single(Selection::range(0, len))),
        KeyInput::Escape => {
            // Collapse multiple cursors / any selection down to the primary caret.
            if is_multi || !primary.is_empty() {
                select_many(Selections::single(Selection::caret(primary.head)));
            }
        }
        KeyInput::SelectNextOccurrence => {
            // Ctrl+D: with no selection, select the word under the caret; otherwise add a
            // cursor at the next occurrence of the current selection (wrapping).
            if primary.is_empty() {
                let (s, e) = word_at(&src, primary.head);
                if e > s {
                    select_many(Selections::single(Selection::range(s, e)));
                }
            } else {
                let needle = &src[primary.min()..primary.max()];
                let from = ranges.iter().map(|r| r.max()).max().unwrap_or(0);
                if let Some(start) = find_from(&src, needle, from).or_else(|| find_from(&src, needle, 0))
                {
                    let end = start + needle.len();
                    if !ranges.iter().any(|r| r.min() == start && r.max() == end) {
                        select_many(sels.pushed(Selection::range(start, end)));
                    }
                }
            }
        }
        KeyInput::Copy => {
            // Join each non-empty selection's text with newlines (multi-cursor copy).
            let parts: Vec<String> = ranges
                .iter()
                .filter(|r| !r.is_empty())
                .map(|r| src[r.min()..r.max()].to_string())
                .collect();
            if !parts.is_empty() {
                pebbles::core::clipboard::write(&parts.join("\n"));
            }
        }
        KeyInput::Cut if !read_only => {
            let parts: Vec<String> = ranges
                .iter()
                .filter(|r| !r.is_empty())
                .map(|r| src[r.min()..r.max()].to_string())
                .collect();
            if !parts.is_empty() {
                pebbles::core::clipboard::write(&parts.join("\n"));
                replace("", Coalesce::Never);
            }
        }
        KeyInput::Paste if !read_only => {
            let p = pebbles::core::clipboard::read();
            if !p.is_empty() {
                let lines: Vec<&str> = p.split('\n').collect();
                if is_multi && lines.len() == ranges.len() {
                    // One clipboard line per cursor (as CodeMirror/VS Code do).
                    let idx = std::cell::Cell::new(0usize);
                    edit_ranges(
                        &|r| {
                            let i = idx.get();
                            idx.set(i + 1);
                            (r.min(), r.max(), lines[i].to_string())
                        },
                        Coalesce::Never,
                    );
                } else {
                    replace(&p, Coalesce::Never);
                }
            }
        }
        KeyInput::Undo if !read_only => {
            let restored = history.peek().borrow_mut().undo(&state.peek());
            if let Some(next) = restored {
                code.set(next.text());
                goal.set(col_of(&next.text(), next.primary().head));
                state.set(next);
            }
        }
        KeyInput::Redo if !read_only => {
            let restored = history.peek().borrow_mut().redo(&state.peek());
            if let Some(next) = restored {
                code.set(next.text());
                goal.set(col_of(&next.text(), next.primary().head));
                state.set(next);
            }
        }
        KeyInput::Indent if !read_only => {
            // A selection spanning lines block-indents each touched line; otherwise insert
            // one indent level at each caret / replace each selection.
            let spans_lines = ranges
                .iter()
                .any(|r| line_of(&src, r.min()) != line_of(&src, r.max()));
            if !spans_lines {
                replace(tab, Coalesce::Never);
            } else {
                let changes: Vec<Change> = touched_lines(&src, &ranges)
                    .into_iter()
                    .map(|line| {
                        let ls = line_start_of(&src, line);
                        Change {
                            from: ls,
                            to: ls,
                            insert: tab.to_string(),
                        }
                    })
                    .collect();
                dispatch(
                    state,
                    history,
                    code,
                    Transaction::change(ChangeSet::from_changes(changes)),
                    Coalesce::Never,
                );
            }
        }
        KeyInput::Outdent if !read_only => {
            // Remove up to one indent level of leading whitespace from each touched line.
            let unit_w = if tab == "\t" {
                1
            } else {
                tab.chars().count().max(1)
            };
            let mut changes: Vec<Change> = Vec::new();
            for line in touched_lines(&src, &ranges) {
                let ls = line_start_of(&src, line);
                let le = line_end(&src, ls);
                let mut n = 0usize;
                for ch in src[ls..le].chars() {
                    if ch == '\t' {
                        n += 1; // a leading tab is one level
                        break;
                    } else if ch == ' ' && n < unit_w {
                        n += 1;
                    } else {
                        break;
                    }
                }
                if n > 0 {
                    changes.push(Change {
                        from: ls,
                        to: ls + n,
                        insert: String::new(),
                    });
                }
            }
            if !changes.is_empty() {
                dispatch(
                    state,
                    history,
                    code,
                    Transaction::change(ChangeSet::from_changes(changes)),
                    Coalesce::Never,
                );
            }
        }
        _ => {
            let _ = tab;
        }
    }
}

/// The sorted, de-duplicated set of line numbers any selection in `ranges` touches.
fn touched_lines(src: &str, ranges: &[Selection]) -> Vec<usize> {
    let mut lines = std::collections::BTreeSet::new();
    for r in ranges {
        for l in line_of(src, r.min())..=line_of(src, r.max()) {
            lines.insert(l);
        }
    }
    lines.into_iter().collect()
}

// ---------------------------------------------------------------------------
// Buffer geometry helpers (byte offsets over &str; all char-boundary safe)
// ---------------------------------------------------------------------------

fn line_of(src: &str, byte: usize) -> usize {
    src.get(..byte.min(src.len()))
        .map(|s| s.bytes().filter(|b| *b == b'\n').count())
        .unwrap_or(0)
}
fn line_start(src: &str, byte: usize) -> usize {
    src.get(..byte.min(src.len()))
        .and_then(|s| s.rfind('\n'))
        .map(|i| i + 1)
        .unwrap_or(0)
}
fn line_end(src: &str, byte: usize) -> usize {
    let b = byte.min(src.len());
    src[b..].find('\n').map(|i| b + i).unwrap_or(src.len())
}
fn col_of(src: &str, byte: usize) -> usize {
    let ls = line_start(src, byte);
    src.get(ls..byte.min(src.len()))
        .map(|s| s.chars().count())
        .unwrap_or(0)
}
/// Byte length (in chars) of line `line`.
fn line_char_len(src: &str, line: usize) -> usize {
    src.split('\n')
        .nth(line)
        .map(|l| l.chars().count())
        .unwrap_or(0)
}
/// The byte offset at `(line, col)`, clamped to the line's end.
fn byte_at(src: &str, line: usize, col: usize) -> usize {
    let start = line_start_of(src, line);
    let end = line_end(src, start);
    let mut b = start;
    for (i, _) in src[start..end].char_indices().take(col) {
        b = start
            + i
            + src[start + i..]
                .chars()
                .next()
                .map(char::len_utf8)
                .unwrap_or(1);
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
    src.get(..byte.min(src.len()))
        .and_then(|s| s.chars().next_back())
        .map(|ch| byte - ch.len_utf8())
        .unwrap_or(0)
}
fn next_char(src: &str, byte: usize) -> usize {
    let b = byte.min(src.len());
    src[b..]
        .chars()
        .next()
        .map(|ch| b + ch.len_utf8())
        .unwrap_or(b)
}
fn is_word(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}
fn prev_word(src: &str, byte: usize) -> usize {
    let mut b = byte.min(src.len());
    // skip whitespace
    while b > 0 {
        let p = prev_char(src, b);
        if src[p..b]
            .chars()
            .next()
            .is_some_and(|ch| ch.is_whitespace())
        {
            b = p;
        } else {
            break;
        }
    }
    // skip a run of word (or non-word) chars
    let word = b > 0
        && src[prev_char(src, b)..b]
            .chars()
            .next()
            .is_some_and(is_word);
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
/// The byte offset of the first occurrence of `needle` at or after `from` (for
/// add-next-occurrence). `None` if there is none from there to the end.
fn find_from(src: &str, needle: &str, from: usize) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    let from = from.min(src.len());
    src.get(from..).and_then(|s| s.find(needle)).map(|i| from + i)
}
/// The byte range of the word under `byte` (for double-click). If `byte` isn't on/adjacent
/// to a word character, selects just the single character there.
fn word_at(src: &str, byte: usize) -> (usize, usize) {
    let b = byte.min(src.len());
    let on_word = src[b..].chars().next().is_some_and(is_word)
        || (b > 0 && src[prev_char(src, b)..b].chars().next().is_some_and(is_word));
    if on_word {
        let mut s = b;
        while s > 0 {
            let p = prev_char(src, s);
            if src[p..s].chars().next().is_some_and(is_word) {
                s = p;
            } else {
                break;
            }
        }
        let mut e = b;
        while e < src.len() {
            if src[e..].chars().next().is_some_and(is_word) {
                e = next_char(src, e);
            } else {
                break;
            }
        }
        (s, e)
    } else {
        (b, next_char(src, b))
    }
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
/// Map a pointer position to a `(line, col)` grid cell, clamped to the document's lines.
fn pos_to_grid(
    src: &str,
    pos: Offset,
    pad_l: f64,
    pad_t: f64,
    advance: f64,
    line_px: f64,
) -> (usize, usize) {
    let line = (((pos.y - pad_t) / line_px).floor()).max(0.0) as usize;
    let col = (((pos.x - pad_l) / advance).round()).max(0.0) as usize;
    let last = src.split('\n').count().saturating_sub(1);
    (line.min(last), col)
}
fn pos_to_byte(
    src: &str,
    pos: Offset,
    pad_l: f64,
    pad_t: f64,
    advance: f64,
    line_px: f64,
) -> usize {
    let (line, col) = pos_to_grid(src, pos, pad_l, pad_t, advance, line_px);
    byte_at(src, line, col)
}
