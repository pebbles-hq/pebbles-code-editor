//! The command engine: translate a semantic [`KeyInput`] into a [`Transaction`] against the
//! document and dispatch it through the single mutation path.
//!
//! This is the editor's "controller" layer — it owns no widgets. It reads the current
//! [`EditorState`] (via the `state` signal), builds edits/motions for **every** cursor
//! (multi-cursor by construction), records undo history, and keeps the public
//! `code: Signal<String>` in sync. Auto-close, comment toggling, and smart indent live here,
//! parameterized by [`EditCfg`] (language-derived knobs).

use std::cell::RefCell;
use std::rc::Rc;

use pebbles::prelude::*;

use crate::edit::{
    Change, ChangeSet, Coalesce, EditorState, History, Selection, Selections, Transaction,
};
use crate::geometry::*;

/// The single mutation path: apply `tx` to the state, record its inverse for undo (edits
/// only), and keep the public `code` signal in sync with the document.
pub(crate) fn dispatch(
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

/// Language-derived knobs the key handler needs (owned so it can live in the 'static
/// handler): the indent unit, the line-comment prefix, the bracket pairs, and auto-close.
pub(crate) struct EditCfg {
    pub(crate) tab: String,
    pub(crate) line_comment: Option<String>,
    pub(crate) brackets: Vec<(char, char)>,
    pub(crate) auto_close: bool,
}

pub(crate) fn apply_key(
    k: KeyInput,
    state: Signal<EditorState>,
    history: Signal<Rc<RefCell<History>>>,
    code: Signal<String>,
    goal: Signal<usize>,
    read_only: bool,
    cfg: &EditCfg,
) {
    let tab = cfg.tab.as_str();
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
            // Auto-close (single caret / selection only): type an opener → insert its pair
            // (caret between) or wrap the selection; type a closer/quote sitting right at the
            // caret → skip over it instead of doubling.
            if cfg.auto_close && !is_multi && s.chars().count() == 1 {
                let ch = s.chars().next().unwrap();
                let is_quote = matches!(ch, '"' | '\'' | '`');
                let opener = cfg.brackets.iter().find(|(o, _)| *o == ch).copied();
                let closer_or_quote =
                    cfg.brackets.iter().any(|(_, c)| *c == ch) || is_quote;
                let head = primary.head;
                let at_caret = src.get(head..).and_then(|s| s.chars().next());
                if closer_or_quote && at_caret == Some(ch) {
                    // Overtype the auto-inserted closer/quote.
                    select_many(Selections::single(Selection::caret(next_char(&src, head))));
                    return;
                }
                let pair = opener.or(if is_quote { Some((ch, ch)) } else { None });
                if let Some((o, c)) = pair {
                    // Don't pair a quote right after a word char (apostrophes in `don't`).
                    let after_word = is_quote
                        && head > 0
                        && src[..head].chars().next_back().is_some_and(|p| p.is_alphanumeric());
                    if !after_word {
                        let (lo, hi) = (primary.min(), primary.max());
                        let (cs, caret) = if primary.is_empty() {
                            (ChangeSet::insert(head, format!("{o}{c}")), head + o.len_utf8())
                        } else {
                            (
                                ChangeSet::from_changes(vec![
                                    Change { from: lo, to: lo, insert: o.to_string() },
                                    Change { from: hi, to: hi, insert: c.to_string() },
                                ]),
                                hi + o.len_utf8(),
                            )
                        };
                        let sel = if primary.is_empty() {
                            Selections::single(Selection::caret(caret))
                        } else {
                            Selections::single(Selection::range(lo + o.len_utf8(), caret))
                        };
                        dispatch(
                            state,
                            history,
                            code,
                            Transaction::change_and_select(cs, sel),
                            Coalesce::Never,
                        );
                        goal.set(col_of(&state.peek().text(), state.peek().primary().head));
                        return;
                    }
                }
            }
            replace(&s, coalesce);
        }
        KeyInput::Enter if !read_only => {
            // Pressing Enter with the caret between a bracket pair (`{|}`) opens the block:
            // the caret lands on an indented middle line and the closer drops below it.
            if !is_multi && primary.is_empty() {
                let head = primary.head;
                let before = src[..head].chars().next_back();
                let after = src.get(head..).and_then(|s| s.chars().next());
                if let (Some(b), Some(a)) = (before, after)
                    && cfg.brackets.iter().any(|&(o, c)| o == b && c == a)
                {
                    let ls = line_start(&src, head);
                    let indent: String = src[ls..head]
                        .chars()
                        .take_while(|ch| *ch == ' ' || *ch == '\t')
                        .collect();
                    let inner = format!("\n{indent}{tab}");
                    let caret = head + inner.len();
                    let tx = Transaction::change_and_select(
                        ChangeSet::insert(head, format!("{inner}\n{indent}")),
                        Selections::single(Selection::caret(caret)),
                    );
                    dispatch(state, history, code, tx, Coalesce::Never);
                    goal.set(col_of(&state.peek().text(), caret));
                    return;
                }
            }
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
            // Auto-pair delete: a single caret sitting between an empty pair removes both.
            if cfg.auto_close && !is_multi && primary.is_empty() && primary.head > 0 {
                let head = primary.head;
                let before = src[..head].chars().next_back();
                let after = src.get(head..).and_then(|s| s.chars().next());
                if let (Some(b), Some(a)) = (before, after) {
                    let empty_pair = cfg.brackets.iter().any(|&(o, c)| o == b && c == a)
                        || (matches!(b, '"' | '\'' | '`') && a == b);
                    if empty_pair {
                        let (from, to) = (prev_char(&src, head), next_char(&src, head));
                        dispatch(
                            state,
                            history,
                            code,
                            Transaction::change_and_select(
                                ChangeSet::delete(from, to),
                                Selections::single(Selection::caret(from)),
                            ),
                            Coalesce::Deleting,
                        );
                        goal.set(col_of(&state.peek().text(), from));
                        return;
                    }
                }
            }
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
        KeyInput::ToggleComment if !read_only => {
            // Ctrl+/: comment or uncomment the touched lines with the language's line prefix.
            // If every non-blank line is already commented, uncomment; else comment (aligned
            // to the shallowest indentation). No-op for languages without a line comment.
            if let Some(prefix) = cfg.line_comment.as_deref() {
                let non_blank: Vec<usize> = touched_lines(&src, &ranges)
                    .into_iter()
                    .filter(|&l| {
                        let ls = line_start_of(&src, l);
                        !src[ls..line_end(&src, ls)].trim().is_empty()
                    })
                    .collect();
                if !non_blank.is_empty() {
                    let leading = |l: usize| -> usize {
                        let ls = line_start_of(&src, l);
                        let content = &src[ls..line_end(&src, ls)];
                        content.len() - content.trim_start().len()
                    };
                    let all_commented = non_blank.iter().all(|&l| {
                        let ls = line_start_of(&src, l);
                        src[ls..line_end(&src, ls)].trim_start().starts_with(prefix)
                    });
                    let mut changes: Vec<Change> = Vec::new();
                    if all_commented {
                        for &l in &non_blank {
                            let at = line_start_of(&src, l) + leading(l);
                            let mut rm = prefix.len();
                            if src[at + prefix.len()..].starts_with(' ') {
                                rm += 1;
                            }
                            changes.push(Change {
                                from: at,
                                to: at + rm,
                                insert: String::new(),
                            });
                        }
                    } else {
                        let col = non_blank.iter().map(|&l| leading(l)).min().unwrap_or(0);
                        for &l in &non_blank {
                            let at = line_start_of(&src, l) + col;
                            changes.push(Change {
                                from: at,
                                to: at,
                                insert: format!("{prefix} "),
                            });
                        }
                    }
                    dispatch(
                        state,
                        history,
                        code,
                        Transaction::change(ChangeSet::from_changes(changes)),
                        Coalesce::Never,
                    );
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

