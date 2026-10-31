//! The change peek (VS Code's dirty-diff peek): clicking a gutter change bar opens an inline
//! panel below the hunk showing the base lines, with Revert, Stage (when the host supports
//! it), previous/next change and close.

use std::cell::RefCell;
use std::rc::Rc;

use pebbles::prelude::*;

use crate::commands::dispatch;
use crate::diff::{ChangeKind, LineDiff};
use crate::edit::{ChangeSet, Coalesce, EditorState, History, Selection, Selections, Transaction};

/// Everything the peek panel needs (signals are `Copy`).
pub(crate) struct PeekCtx<'a> {
    pub(crate) peek: Signal<Option<usize>>,
    pub(crate) state: Signal<EditorState>,
    pub(crate) history: Signal<Rc<RefCell<History>>>,
    pub(crate) code: Signal<String>,
    pub(crate) base: Signal<String>,
    pub(crate) on_stage: Option<crate::config::StageHunk>,
    pub(crate) theme: &'a crate::theme::EditorTheme,
    pub(crate) fs: f64,
    pub(crate) line_px: f64,
    pub(crate) font_family: &'a str,
}

/// The block widget for the open peek, if any: `(anchor_line, height, widget)`.
pub(crate) fn block(
    cx: &PeekCtx,
    diff: &LineDiff,
    src: &str,
) -> Option<crate::extensions::BlockWidget> {
    let i = cx.peek.get()?;
    let hunk = diff.hunks.get(i)?.clone();
    let base = cx.base.peek();
    let base_lines: Vec<String> = base
        .split('\n')
        .skip(hunk.base.start)
        .take(hunk.base.len())
        .map(str::to_string)
        .collect();
    let n_hunks = diff.hunks.len();
    let anchor = if hunk.cur.is_empty() {
        hunk.cur.start.saturating_sub(1)
    } else {
        hunk.cur.end - 1
    };
    let t = cx.theme;
    let kind = match hunk.kind() {
        ChangeKind::Added => "Added",
        ChangeKind::Modified => "Modified",
        ChangeKind::Deleted => "Deleted",
    };
    let peek = cx.peek;
    let (state, history, code, base_sig) = (cx.state, cx.history, cx.code, cx.base);
    let h_rev = hunk.clone();
    let revert = button("Revert")
        .size(ButtonSize::Sm)
        .variant(ButtonVariant::Secondary)
        .on_pressed(move || {
            let cur = state.peek();
            let text = cur.text();
            let (from, to, ins) = crate::diff::revert_hunk(&base_sig.peek(), &text, &h_rev);
            let caret = from.min(text.len());
            dispatch(
                state,
                history,
                code,
                Transaction::change_and_select(
                    ChangeSet::replace(from, to, ins),
                    Selections::single(Selection::caret(caret)),
                ),
                Coalesce::Never,
            );
            peek.set(None);
        });
    let mut header: Vec<AnyWidget> = vec![
        t.ui_text(format!("{kind} · change {} of {n_hunks}", i + 1), 0.0)
            .color(t.foreground)
            .into_widget(),
        expanded(gap_w(1.0)).into_widget(),
        revert.into_widget(),
    ];
    if let Some(cb) = cx.on_stage.clone() {
        let cur_text: String = src
            .split('\n')
            .skip(hunk.cur.start)
            .take(hunk.cur.len())
            .collect::<Vec<_>>()
            .join("\n");
        let h_stage = hunk.clone();
        header.push(gap_w(6.0).into_widget());
        header.push(
            button("Stage")
                .size(ButtonSize::Sm)
                .variant(ButtonVariant::Secondary)
                .on_pressed(move || {
                    cb(&h_stage, &cur_text);
                    peek.set(None);
                })
                .into_widget(),
        );
    }
    header.push(gap_w(6.0).into_widget());
    header.push(
        icon_button(IconKind::ChevronUp)
            .icon_size(16.0)
            .on_pressed(move || peek.set(Some((i + n_hunks - 1) % n_hunks.max(1))))
            .into_widget(),
    );
    header.push(
        icon_button(IconKind::ChevronDown)
            .icon_size(16.0)
            .on_pressed(move || peek.set(Some((i + 1) % n_hunks.max(1))))
            .into_widget(),
    );
    header.push(
        icon_button(IconKind::Close)
            .icon_size(16.0)
            .on_pressed(move || peek.set(None))
            .into_widget(),
    );
    let mut rows: Vec<AnyWidget> = vec![
        row(header)
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .into_widget(),
    ];
    for l in &base_lines {
        rows.push(
            container()
                .height(cx.line_px)
                .decoration(
                    BoxDecoration::new()
                        .color(crate::view::chrome::with_alpha(t.diff_removed, 0.18)),
                )
                .alignment(Alignment::CENTER_LEFT)
                .child(
                    text(format!("- {l}"))
                        .size(cx.fs as f32)
                        .font_family(cx.font_family)
                        .color(t.foreground),
                )
                .into_widget(),
        );
    }
    if base_lines.is_empty() {
        rows.push(
            t.ui_text("(new lines — no base content)", 0.0)
                .color(t.muted)
                .into_widget(),
        );
    }
    let height = cx.line_px * (base_lines.len().max(1) as f64 + 1.6) + 12.0;
    let panel = container()
        .height(height)
        .padding(EdgeInsets::all(6.0))
        .decoration(
            BoxDecoration::new()
                .color(t.overlay_bg)
                .border(Border::new(t.change_modified, 1.0)),
        )
        .child(column(rows).main_axis_size(MainAxisSize::Min));
    Some(crate::extensions::BlockWidget {
        line: anchor,
        height,
        widget: panel.into_widget(),
    })
}
