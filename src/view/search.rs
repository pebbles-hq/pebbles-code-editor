//! The find / replace UI: the floating bar (query + replace fields, option toggles, match
//! count, prev/next, replace/replace-all) and the match-highlight overlay layers.
//!
//! Search *logic* lives in [`crate::search`]; this module is the widget + the actions that
//! move the caret to a match and apply replacements through the command engine.

use std::cell::RefCell;
use std::rc::Rc;

use pebbles::prelude::*;

use crate::commands::dispatch;
use crate::edit::{
    Change, ChangeSet, Coalesce, EditorState, History, Selection, Selections, Transaction,
};
use crate::geometry::col_of;
use crate::search::Options;
use crate::view::Frame;

/// The reactive find/replace state (all `Copy` signals), owned by `view::render_editor`.
#[derive(Clone, Copy)]
pub(crate) struct State {
    /// 0 = closed, 1 = find, 2 = find + replace.
    pub(crate) open: Signal<u8>,
    pub(crate) query: Signal<String>,
    pub(crate) replace: Signal<String>,
    pub(crate) case: Signal<bool>,
    pub(crate) word: Signal<bool>,
    pub(crate) regex: Signal<bool>,
    /// Index of the "current" match within the match list.
    pub(crate) idx: Signal<usize>,
    /// Find in Selection: the byte range searched (None = whole document).
    pub(crate) scope: Signal<Option<(usize, usize)>>,
    /// Preserve Case on replace.
    pub(crate) preserve: Signal<bool>,
    /// Focus the query field on the next build (set when the bar is opened). One-shot, so
    /// later rebuilds don't pull focus back from the replace field.
    pub(crate) focus_query: Signal<bool>,
}

impl State {
    pub(crate) fn options(&self) -> Options {
        Options {
            case_sensitive: self.case.peek(),
            whole_word: self.word.peek(),
            regex: self.regex.peek(),
        }
    }
}

/// Highlight boxes for every match in the visible window; the current match gets a stronger
/// fill so it stands out.
pub(crate) fn match_layers(
    f: &Frame,
    matches: &[(usize, usize)],
    current: usize,
) -> Vec<AnyWidget> {
    let mut layers = Vec::new();
    let others = f.theme.search_match;
    let cur = f.theme.search_match_current;
    for (i, &(lo, hi)) in matches.iter().enumerate() {
        let (la, ca) = (f.li.line_of(f.src, lo), f.li.col_of(f.src, lo));
        let (lb, cb) = (f.li.line_of(f.src, hi), f.li.col_of(f.src, hi));
        let color = if i == current { cur } else { others };
        for line in la.max(f.first_line)..=lb.min(f.last_line) {
            let start_col = if line == la { ca } else { 0 };
            let end_col = if line == lb { cb } else { start_col + 1 };
            let x = f.pad_l + start_col as f64 * f.advance;
            let w = (end_col.saturating_sub(start_col).max(1) as f64 * f.advance).max(2.0);
            layers.push(
                Positioned::new(
                    container().width(w).height(f.line_px).decoration(
                        BoxDecoration::new()
                            .color(color)
                            .radius(BorderRadius::all(2.0)),
                    ),
                )
                .left(x)
                .top(f.pad_t + line as f64 * f.line_px)
                .into_widget(),
            );
        }
    }
    layers
}

/// Build the find/replace bar (floating, top-right of the viewport).
#[allow(clippy::too_many_arguments)]
pub(crate) fn bar(
    st: State,
    matches: Vec<(usize, usize)>,
    state: Signal<EditorState>,
    history: Signal<Rc<RefCell<History>>>,
    code: Signal<String>,
    goal: Signal<usize>,
    focus: pebbles::core::focus::FocusNode,
    theme: &crate::theme::EditorTheme,
) -> AnyWidget {
    let count = matches.len();
    let cur = if count == 0 {
        0
    } else {
        st.idx.peek().min(count - 1)
    };

    // Move the editor selection to match `i` (autoscroll follows the state change).
    let go = move |ms: &[(usize, usize)], i: usize| {
        if let Some(&(a, b)) = ms.get(i) {
            let doc = state.peek().doc.clone();
            state.set(EditorState {
                doc,
                selection: Selections::single(Selection::range(a, b)),
            });
            st.idx.set(i);
        }
    };

    // The widget's actions, shared by its buttons and its keyboard bindings.
    let next: Rc<dyn Fn()> = {
        let ms = matches.clone();
        Rc::new(move || {
            if !ms.is_empty() {
                go(&ms, (st.idx.peek() + 1) % ms.len());
            }
        })
    };
    let prev: Rc<dyn Fn()> = {
        let ms = matches.clone();
        Rc::new(move || {
            if !ms.is_empty() {
                go(&ms, (st.idx.peek() + ms.len() - 1) % ms.len());
            }
        })
    };
    // Close the widget and give the keyboard back to the editor (VS Code).
    let close: Rc<dyn Fn()> = Rc::new(move || {
        st.open.set(0);
        focus.request_focus();
    });
    // Select All Matches (Alt+Enter in VS Code): one cursor per match.
    let select_all: Rc<dyn Fn()> = {
        let ms = matches.clone();
        Rc::new(move || {
            if ms.is_empty() {
                return;
            }
            let doc = state.peek().doc.clone();
            let ranges = ms.iter().map(|&(a, b)| Selection::range(a, b)).collect();
            state.set(EditorState {
                doc,
                selection: Selections::new(ranges, 0),
            });
            st.open.set(0);
            focus.request_focus();
        })
    };
    let toggle_scope: Rc<dyn Fn()> = Rc::new(move || {
        if st.scope.peek().is_some() {
            st.scope.set(None);
        } else {
            let p = state.peek().primary();
            if !p.is_empty() {
                st.scope.set(Some((p.min(), p.max())));
            }
        }
        st.idx.set(0);
    });
    let replace_one: Rc<dyn Fn()> = {
        let ms = matches.clone();
        Rc::new(move || {
            let i = st.idx.peek().min(ms.len().saturating_sub(1));
            if let Some(&(a, b)) = ms.get(i) {
                let src = state.peek().text();
                let rep = crate::search::replacement(
                    &src,
                    (a, b),
                    &st.query.peek(),
                    &st.replace.peek(),
                    &st.options(),
                    st.preserve.peek(),
                );
                dispatch(
                    state,
                    history,
                    code,
                    Transaction::change_and_select(
                        ChangeSet::replace(a, b, rep.clone()),
                        Selections::single(Selection::caret(a + rep.len())),
                    ),
                    Coalesce::Never,
                );
                goal.set(col_of(&state.peek().text(), state.peek().primary().head));
            }
        })
    };
    let replace_all: Rc<dyn Fn()> = {
        let ms = matches.clone();
        Rc::new(move || {
            if ms.is_empty() {
                return;
            }
            let src = state.peek().text();
            let (q, r, o, pc) = (
                st.query.peek(),
                st.replace.peek(),
                st.options(),
                st.preserve.peek(),
            );
            let changes: Vec<Change> = ms
                .iter()
                .map(|&(a, b)| Change {
                    from: a,
                    to: b,
                    insert: crate::search::replacement(&src, (a, b), &q, &r, &o, pc),
                })
                .collect();
            dispatch(
                state,
                history,
                code,
                Transaction::change(ChangeSet::from_changes(changes)),
                Coalesce::Never,
            );
        })
    };
    let replacing = st.open.peek() == 2;
    // VS Code's find-widget keybindings, live while either field is focused. `in_replace`:
    // Enter replaces one there instead of moving to the next match.
    let keys = {
        let (next, prev, close, select_all) = (
            next.clone(),
            prev.clone(),
            close.clone(),
            select_all.clone(),
        );
        let (toggle_scope, replace_one, replace_all) = (
            toggle_scope.clone(),
            replace_one.clone(),
            replace_all.clone(),
        );
        move |in_replace: bool| {
            let (next, prev, close, select_all) = (
                next.clone(),
                prev.clone(),
                close.clone(),
                select_all.clone(),
            );
            let (toggle_scope, replace_one, replace_all) = (
                toggle_scope.clone(),
                replace_one.clone(),
                replace_all.clone(),
            );
            move |ev: &KeyEvent| -> KeyEventResult {
                if !ev.is_press() {
                    return KeyEventResult::Ignored;
                }
                let m = ev.mods;
                let cmd = m.ctrl || (cfg!(target_os = "macos") && m.meta);
                let flip = |sig: Signal<bool>| {
                    sig.set(!sig.peek());
                    st.idx.set(0);
                };
                match ev.shortcut_key() {
                    Some(ShortcutKey::Escape) if !cmd && !m.alt && !m.shift => close(),
                    Some(ShortcutKey::Enter) if cmd && m.alt && !m.shift => {
                        if replacing {
                            replace_all();
                        }
                    }
                    Some(ShortcutKey::Enter) if m.alt && !cmd && !m.shift => select_all(),
                    Some(ShortcutKey::Enter) if m.shift && !cmd && !m.alt => prev(),
                    Some(ShortcutKey::Enter) if !m.shift && !cmd && !m.alt => {
                        if in_replace {
                            replace_one();
                        } else {
                            next();
                        }
                    }
                    Some(ShortcutKey::F(3)) if !cmd && !m.alt => {
                        if m.shift {
                            prev();
                        } else {
                            next();
                        }
                    }
                    _ if cmd && m.shift && !m.alt && ev.physical.code() == "Digit1" => {
                        if replacing {
                            replace_one();
                        }
                    }
                    Some(ShortcutKey::Char(c)) if m.alt && !cmd && !m.shift => match c {
                        'c' => flip(st.case),
                        'w' => flip(st.word),
                        'r' => flip(st.regex),
                        'l' => toggle_scope(),
                        'p' if replacing => st.preserve.set(!st.preserve.peek()),
                        _ => return KeyEventResult::Ignored,
                    },
                    _ => return KeyEventResult::Ignored,
                }
                KeyEventResult::Handled
            }
        }
    };

    // ---- find row ----
    let focus_query = st.focus_query.peek();
    if focus_query {
        st.focus_query.set(false);
    }
    let query_field = text_field()
        .placeholder("Find")
        .bind(st.query)
        // The query seeded from the selection / word is selected, so typing replaces it
        // (VS Code `editor.find.seedSearchStringFromSelection`).
        .select_range(0, st.query.peek().len())
        .bare(false)
        .on_changed(move |s| {
            st.query.set(s.to_string());
            st.idx.set(0);
        })
        .on_key_event(keys(false));
    let query_field = if focus_query {
        query_field.autofocus()
    } else {
        query_field
    };

    // Option chips (case / whole-word / regex): a bordered Toggle so the pill is visible
    // even when inactive, with a legible label that stays readable on both states.
    let fg = theme.foreground;
    let ui = theme.clone();
    let toggle_chip = move |on: bool, label: &str, sig: Signal<bool>| {
        toggle(on, ui.ui_text(label, -0.5).weight(600.0).color(fg))
            .variant(ToggleVariant::Outline)
            .size(ToggleSize::Sm)
            .on_changed(move || sig.set(!sig.peek()))
            .into_widget()
    };
    let find_row = row(children![
        expanded(query_field),
        gap_w(8.0),
        theme
            .ui_text(
                format!("{}/{}", if count == 0 { 0 } else { cur + 1 }, count),
                -0.5,
            )
            .color(theme.gutter_fg),
        gap_w(6.0),
        icon_button(IconKind::ChevronUp).size(16.0).on_pressed({
            let prev = prev.clone();
            move || prev()
        }),
        icon_button(IconKind::ChevronDown).size(16.0).on_pressed({
            let next = next.clone();
            move || next()
        }),
        gap_w(6.0),
        toggle_chip(st.case.peek(), "Aa", st.case),
        gap_w(4.0),
        toggle_chip(st.word.peek(), "W", st.word),
        gap_w(4.0),
        toggle_chip(st.regex.peek(), ".*", st.regex),
        gap_w(4.0),
        {
            // Find in Selection: scope the search to the current selection.
            let ui2 = theme.clone();
            toggle(
                st.scope.peek().is_some(),
                ui2.ui_text("≡", -0.5).weight(600.0).color(fg),
            )
            .variant(ToggleVariant::Outline)
            .size(ToggleSize::Sm)
            .on_changed({
                let toggle_scope = toggle_scope.clone();
                move || toggle_scope()
            })
            .into_widget()
        },
        gap_w(4.0),
        {
            // Select All Matches (Alt+Enter in VS Code): one cursor per match.
            let select_all = select_all.clone();
            icon_button(IconKind::Check)
                .size(16.0)
                .on_pressed(move || select_all())
        },
        gap_w(6.0),
        icon_button(IconKind::Close).size(16.0).on_pressed({
            let close = close.clone();
            move || close()
        }),
    ])
    .cross_axis_alignment(CrossAxisAlignment::Center)
    .main_axis_size(MainAxisSize::Min);

    let mut rows: Vec<AnyWidget> = vec![find_row.into_widget()];

    // ---- replace row (mode 2) ----
    if replacing {
        let replace_field = text_field()
            .placeholder("Replace")
            .bind(st.replace)
            .bare(false)
            .on_changed(move |s| st.replace.set(s.to_string()))
            .on_key_event(keys(true));
        let fg2 = theme.foreground;
        let ui3 = theme.clone();
        let replace_row = row(children![
            expanded(replace_field),
            gap_w(6.0),
            toggle(
                st.preserve.peek(),
                ui3.ui_text("AB", -0.5).weight(600.0).color(fg2)
            )
            .variant(ToggleVariant::Outline)
            .size(ToggleSize::Sm)
            .on_changed(move || st.preserve.set(!st.preserve.peek())),
            gap_w(6.0),
            button("Replace")
                .size(ButtonSize::Sm)
                .variant(ButtonVariant::Secondary)
                .on_pressed(move || replace_one()),
            gap_w(6.0),
            button("All")
                .size(ButtonSize::Sm)
                .variant(ButtonVariant::Secondary)
                .on_pressed(move || replace_all()),
        ])
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .main_axis_size(MainAxisSize::Min);
        rows.push(gap_h(6.0).into_widget());
        rows.push(replace_row.into_widget());
    }

    let panel = container()
        .width(if st.open.peek() == 2 { 460.0 } else { 380.0 })
        .decoration(
            BoxDecoration::new()
                .color(theme.overlay_bg)
                .radius(BorderRadius::all(theme.overlay_radius))
                .border(Border::new(theme.border, 1.0)),
        )
        .padding(EdgeInsets::all(8.0))
        .child(column(rows).main_axis_size(MainAxisSize::Min));
    Positioned::new(panel).right(12.0).top(10.0).into_widget()
}
