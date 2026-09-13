//! The autocomplete popup: session state, the trigger/accept logic, snippet expansion, and
//! the popup widget. The provider (dev-supplied) returns candidates; this module filters
//! them by the typed prefix, drives selection, and applies the accepted insert (expanding
//! snippet tabstops). Keyboard navigation is wired in [`super`]'s key handler.

use std::cell::RefCell;
use std::rc::Rc;

use pebbles::prelude::*;

use crate::MONO;
use crate::commands::dispatch;
use crate::edit::{ChangeSet, Coalesce, EditorState, History, Selection, Selections, Transaction};
use crate::geometry::{col_of, is_word, line_of, prev_char};
use crate::providers::{CompletionContext, CompletionItem, CompletionKind, CompletionProvider};

/// A live completion session: the filtered items, which one is selected, and the byte where
/// the typed prefix starts (accepting replaces `anchor..caret`).
#[derive(Clone)]
pub(crate) struct Session {
    pub(crate) items: Vec<CompletionItem>,
    pub(crate) selected: usize,
    pub(crate) anchor: usize,
}

impl Session {
    /// Move the highlight (wrapping).
    pub(crate) fn move_by(&mut self, delta: isize) {
        let n = self.items.len() as isize;
        if n > 0 {
            self.selected = (((self.selected as isize + delta) % n + n) % n) as usize;
        }
    }
}

/// The identifier prefix start immediately before `caret`.
fn prefix_start(src: &str, caret: usize) -> usize {
    let mut b = caret;
    while b > 0 {
        let p = prev_char(src, b);
        if src[p..b].chars().next().is_some_and(is_word) {
            b = p;
        } else {
            break;
        }
    }
    b
}

/// Ask the provider for candidates at the caret, filter by the typed prefix, and open (or
/// close) the popup. `explicit` (Ctrl+Space) opens even with an empty prefix.
pub(crate) fn trigger(
    state: Signal<EditorState>,
    session: Signal<Option<Session>>,
    provider: &CompletionProvider,
    explicit: bool,
) {
    let st = state.peek();
    let src = st.text();
    let caret = st.primary().head;
    let anchor = prefix_start(&src, caret);
    let prefix = &src[anchor..caret];
    if prefix.is_empty() && !explicit {
        session.set(None);
        return;
    }
    let mut items = provider(&CompletionContext {
        src: &src,
        cursor: caret,
        prefix,
    });
    if !prefix.is_empty() {
        let pl = prefix.to_lowercase();
        items.retain(|it| it.label.to_lowercase().contains(&pl));
        // Prefix matches rank above mere substring matches.
        items.sort_by_key(|it| !it.label.to_lowercase().starts_with(&pl));
    }
    session.set((!items.is_empty()).then_some(Session {
        items,
        selected: 0,
        anchor,
    }));
}

/// Apply the selected item (expanding snippet tabstops) and close the popup.
pub(crate) fn accept(
    state: Signal<EditorState>,
    history: Signal<Rc<RefCell<History>>>,
    code: Signal<String>,
    goal: Signal<usize>,
    session: Signal<Option<Session>>,
) {
    let Some(s) = session.peek() else { return };
    let Some(item) = s.items.get(s.selected) else { return };
    let caret = state.peek().primary().head;
    let (text, caret_off) = expand_snippet(&item.insert);
    let tx = Transaction::change_and_select(
        ChangeSet::replace(s.anchor, caret, text),
        Selections::single(Selection::caret(s.anchor + caret_off)),
    );
    dispatch(state, history, code, tx, Coalesce::Never);
    goal.set(col_of(&state.peek().text(), state.peek().primary().head));
    session.set(None);
}

/// Expand a snippet body: `${N:default}` → `default`, `$N` → "", and the caret lands at the
/// first `$1`/`${1:…}` (else `$0`, else the end). Returns `(text, caret_byte_offset)`.
fn expand_snippet(insert: &str) -> (String, usize) {
    if !insert.contains('$') {
        let len = insert.len();
        return (insert.to_string(), len);
    }
    let mut out = String::with_capacity(insert.len());
    let mut stops: Vec<(u32, usize)> = Vec::new(); // (tabstop number, byte offset in `out`)
    let mut chars = insert.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        // `${N:default}`
        if chars.peek() == Some(&'{') {
            chars.next();
            let mut num = String::new();
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() {
                    num.push(d);
                    chars.next();
                } else {
                    break;
                }
            }
            if chars.peek() == Some(&':') {
                chars.next();
            }
            if let Ok(n) = num.parse::<u32>() {
                stops.push((n, out.len()));
            }
            for d in chars.by_ref() {
                if d == '}' {
                    break;
                }
                out.push(d);
            }
        } else {
            // `$N`
            let mut num = String::new();
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() {
                    num.push(d);
                    chars.next();
                } else {
                    break;
                }
            }
            if let Ok(n) = num.parse::<u32>() {
                stops.push((n, out.len()));
            } else {
                out.push('$');
            }
        }
    }
    // Prefer $1 (first non-zero), else $0, else end of text.
    let caret = stops
        .iter()
        .filter(|(n, _)| *n > 0)
        .min_by_key(|(n, _)| *n)
        .or_else(|| stops.iter().find(|(n, _)| *n == 0))
        .map(|(_, off)| *off)
        .unwrap_or(out.len());
    (out, caret)
}

/// The popup widget, positioned just below the caret line, within the content grid.
pub(crate) fn popup(s: &Session, f: &crate::view::Frame) -> AnyWidget {
    let (theme, fs, line_px) = (f.theme, f.fs, f.line_px);
    let line = line_of(f.src, s.anchor);
    let col = col_of(f.src, s.anchor);
    let x = f.pad_l + col as f64 * f.advance;
    let y = f.pad_t + (line + 1) as f64 * line_px + 2.0;

    let mut rows: Vec<AnyWidget> = Vec::new();
    for (i, it) in s.items.iter().take(12).enumerate() {
        let selected = i == s.selected;
        let bg = if selected {
            theme.selection
        } else {
            Color::from_rgba8(0, 0, 0, 0)
        };
        let mut cells: Vec<AnyWidget> = vec![
            text(kind_glyph(it.kind).to_string())
                .size((fs * 0.85) as f32)
                .font_family(MONO)
                .color(theme.function)
                .into_widget(),
            gap_w(6.0).into_widget(),
            text(it.label.clone())
                .size(fs as f32)
                .font_family(MONO)
                .color(theme.foreground)
                .into_widget(),
        ];
        if let Some(d) = &it.detail {
            cells.push(spacer().into_widget());
            cells.push(gap_w(10.0).into_widget());
            cells.push(
                text(d.clone())
                    .size((fs * 0.85) as f32)
                    .font_family(MONO)
                    .color(theme.comment)
                    .into_widget(),
            );
        }
        rows.push(
            container()
                .height(line_px)
                .padding(EdgeInsets::symmetric(8.0, 0.0))
                .decoration(BoxDecoration::new().color(bg))
                .child(row(cells).cross_axis_alignment(CrossAxisAlignment::Center))
                .into_widget(),
        );
    }
    let panel = container()
        .width(340.0)
        .decoration(
            BoxDecoration::new()
                .color(theme.gutter_bg)
                .radius(BorderRadius::all(8.0))
                .border(Border::new(crate::view::chrome::with_alpha(theme.punctuation, 0.5), 1.0)),
        )
        .clip()
        .child(column(rows).main_axis_size(MainAxisSize::Min));
    Positioned::new(panel).left(x).top(y).into_widget()
}

/// A one-glyph marker per completion kind (no icon font dependency).
fn kind_glyph(kind: CompletionKind) -> char {
    match kind {
        CompletionKind::Keyword => 'k',
        CompletionKind::Function => 'ƒ',
        CompletionKind::Method => 'm',
        CompletionKind::Variable => 'v',
        CompletionKind::Field => '.',
        CompletionKind::Type => 'T',
        CompletionKind::Module => 'M',
        CompletionKind::Constant => 'c',
        CompletionKind::Snippet => '⋯',
        CompletionKind::Text => 'a',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_caret_and_placeholders() {
        // caret at $1, placeholder rendered, $0 ignored when $1 present
        let (text, caret) = expand_snippet("if ${1:cond} {\n\t$0\n}");
        assert_eq!(text, "if cond {\n\t\n}");
        assert_eq!(&text[caret..caret + 4], "cond");
        // no tabstops → caret at end
        let (t2, c2) = expand_snippet("println!");
        assert_eq!((t2.as_str(), c2), ("println!", 8));
        // bare $0 → caret there
        let (t3, c3) = expand_snippet("()$0");
        assert_eq!((t3.as_str(), c3), ("()", 2));
    }
}
