//! The find / replace UI: the floating bar (query + replace fields, option toggles, match
//! count, prev/next, replace/replace-all) and the match-highlight overlay layers.
//!
//! Search *logic* lives in [`crate::search`]; this module is the widget + the actions that
//! move the caret to a match and apply replacements through the command engine.

use std::cell::RefCell;
use std::rc::Rc;

use pebbles::prelude::*;

use crate::commands::dispatch;
use crate::edit::{Change, ChangeSet, Coalesce, EditorState, History, Selection, Selections, Transaction};
use crate::geometry::{col_of, line_of};
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
pub(crate) fn match_layers(f: &Frame, matches: &[(usize, usize)], current: usize) -> Vec<AnyWidget> {
    let mut layers = Vec::new();
    let others = f.theme.search_match;
    let cur = f.theme.search_match_current;
    for (i, &(lo, hi)) in matches.iter().enumerate() {
        let (la, ca) = (line_of(f.src, lo), col_of(f.src, lo));
        let (lb, cb) = (line_of(f.src, hi), col_of(f.src, hi));
        let color = if i == current { cur } else { others };
        for line in la.max(f.first_line)..=lb.min(f.last_line) {
            let start_col = if line == la { ca } else { 0 };
            let end_col = if line == lb { cb } else { start_col + 1 };
            let x = f.pad_l + start_col as f64 * f.advance;
            let w = (end_col.saturating_sub(start_col).max(1) as f64 * f.advance).max(2.0);
            layers.push(
                Positioned::new(
                    container()
                        .width(w)
                        .height(f.line_px)
                        .decoration(BoxDecoration::new().color(color).radius(BorderRadius::all(2.0))),
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
    let cur = if count == 0 { 0 } else { st.idx.peek().min(count - 1) };

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

    // ---- find row ----
    let query_field = text_field()
        .placeholder("Find")
        .bare(false)
        .autofocus()
        .on_changed(move |s| {
            st.query.set(s.to_string());
            st.idx.set(0);
        })
        .on_submit({
            let ms = matches.clone();
            move |_| {
                if !ms.is_empty() {
                    go(&ms, (st.idx.peek() + 1) % ms.len());
                }
            }
        });

    // Option chips (case / whole-word / regex): a bordered Toggle so the pill is visible
    // even when inactive, with a legible label that stays readable on both states.
    let fg = theme.foreground;
    let toggle_chip = move |on: bool, label: &str, sig: Signal<bool>| {
        toggle(on, text(label.to_string()).size(12.0).weight(600.0).color(fg))
            .variant(ToggleVariant::Outline)
            .size(ToggleSize::Sm)
            .on_changed(move || sig.set(!sig.peek()))
            .into_widget()
    };
    let (m_prev, m_next) = (matches.clone(), matches.clone());
    let find_row = row(children![
        expanded(query_field),
        gap_w(8.0),
        text(format!("{}/{}", if count == 0 { 0 } else { cur + 1 }, count))
            .size(12.0)
            .color(theme.gutter_fg),
        gap_w(6.0),
        icon_button(IconKind::ChevronUp).size(16.0).on_pressed(move || {
            if !m_prev.is_empty() {
                go(&m_prev, (st.idx.peek() + m_prev.len() - 1) % m_prev.len());
            }
        }),
        icon_button(IconKind::ChevronDown).size(16.0).on_pressed(move || {
            if !m_next.is_empty() {
                go(&m_next, (st.idx.peek() + 1) % m_next.len());
            }
        }),
        gap_w(6.0),
        toggle_chip(st.case.peek(), "Aa", st.case),
        gap_w(4.0),
        toggle_chip(st.word.peek(), "W", st.word),
        gap_w(4.0),
        toggle_chip(st.regex.peek(), ".*", st.regex),
        gap_w(6.0),
        icon_button(IconKind::Close).size(16.0).on_pressed(move || {
            st.open.set(0);
            focus.request_focus();
        }),
    ])
    .cross_axis_alignment(CrossAxisAlignment::Center)
    .main_axis_size(MainAxisSize::Min);

    let mut rows: Vec<AnyWidget> = vec![find_row.into_widget()];

    // ---- replace row (mode 2) ----
    if st.open.peek() == 2 {
        let replace_field = text_field()
            .placeholder("Replace")
            .bare(false)
            .on_changed(move |s| st.replace.set(s.to_string()));
        let m_one = matches.clone();
        let m_all = matches.clone();
        let replace_row = row(children![
            expanded(replace_field),
            gap_w(6.0),
            button("Replace").size(ButtonSize::Sm).variant(ButtonVariant::Secondary).on_pressed(
                move || {
                    let i = st.idx.peek().min(m_one.len().saturating_sub(1));
                    if let Some(&(a, b)) = m_one.get(i) {
                        let rep = st.replace.peek();
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
                }
            ),
            gap_w(6.0),
            button("All").size(ButtonSize::Sm).variant(ButtonVariant::Secondary).on_pressed(
                move || {
                    if m_all.is_empty() {
                        return;
                    }
                    let rep = st.replace.peek();
                    let changes: Vec<Change> = m_all
                        .iter()
                        .map(|&(a, b)| Change {
                            from: a,
                            to: b,
                            insert: rep.clone(),
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
            ),
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
                .radius(BorderRadius::all(8.0))
                .border(Border::new(theme.border, 1.0)),
        )
        .padding(EdgeInsets::all(8.0))
        .child(column(rows).main_axis_size(MainAxisSize::Min));
    Positioned::new(panel).right(12.0).top(10.0).into_widget()
}
