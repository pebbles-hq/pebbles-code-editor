//! Sticky scroll: pin the enclosing scopes of the top visible line to the top of the
//! viewport. Scopes are derived from indentation (no syntax tree needed) — walking up, each
//! line with strictly smaller indent than the last is an ancestor. The pinned panel is a
//! pointer barrier so clicks on it never fall through to the content hidden behind it; a
//! click on a header reveals that line instead (VS Code).

use std::rc::Rc;

use pebbles::prelude::*;

use crate::geometry::line_end;
use crate::highlight::{slice_tokens, to_spans};
use crate::view::{Frame, gutter};

/// Overlay the pinned scope headers on top of `scroller` (or return it unchanged when sticky
/// scroll is off or there are no scrolled-off ancestors).
///
/// Each header is drawn like the real line it pins (VS Code `editor.stickyScroll`): its line
/// number right-aligned in a gutter cell of the gutter's width, then the highlighted text at
/// the text column's x (`gutter + pad_l`, minus the horizontal scroll), clipped to the text
/// area and never wrapped. Clicking a header puts the caret at that line's start and scrolls the real line to where
/// the header was
/// (`on_jump(byte, scroll_target)`).
pub(crate) fn overlay(
    f: &Frame,
    scroller: AnyWidget,
    scroll_top: Signal<f64>,
    scroll_left: Signal<f64>,
    on_jump: Rc<dyn Fn(usize, f64)>,
) -> AnyWidget {
    let lines = scoped_lines(f, scroll_top);
    if lines.is_empty() {
        return scroller;
    }
    let sl = scroll_left.get();
    let mut rows: Vec<AnyWidget> = Vec::new();
    for (i, &ln) in lines.iter().enumerate() {
        let ls = f.li.line_start_of(ln);
        let le = line_end(f.src, ls);
        let spans = to_spans(
            &f.src[ls..le],
            &slice_tokens(f.tokens, ls, le),
            f.theme,
            f.fs,
            f.font_family,
            f.font_weight,
        );
        let text_cell = container()
            .height(f.line_px)
            .decoration(BoxDecoration::new().color(f.theme.background))
            .clip()
            .child(
                stack(children![
                    Positioned::new(
                        text_rich(spans)
                            .soft_wrap(false)
                            .line_height(f.lh as f32)
                            .letter_spacing(f.letter_spacing as f32),
                    )
                    .left(f.pad_l - sl)
                    .top(0.0)
                    .into_widget(),
                ])
                .fit(StackFit::Expand)
                .alignment(Alignment::TOP_LEFT),
            );
        let mut cells: Vec<AnyWidget> = Vec::new();
        if f.p.gutter {
            cells.push(
                container()
                    .width(gutter::width(f))
                    .height(f.line_px)
                    .decoration(BoxDecoration::new().color(f.theme.gutter_bg))
                    .child(
                        stack(children![
                            Positioned::new(gutter::number_cell(f, ln))
                                .left(gutter::glyph_margin_w(f.p, f.line_px))
                                .top(0.0)
                                .into_widget(),
                        ])
                        .fit(StackFit::Expand)
                        .alignment(Alignment::TOP_LEFT),
                    )
                    .into_widget(),
            );
        }
        cells.push(expanded(text_cell).into_widget());
        let jump = on_jump.clone();
        // VS Code: scroll so the real line lands where its header was (row `i` of the panel).
        let target = (f.y_of(ln) - i as f64 * f.line_px).max(0.0);
        let at = ls;
        rows.push(
            GestureDetector::new(
                container()
                    .height(f.line_px)
                    .child(row(cells).cross_axis_alignment(CrossAxisAlignment::Start)),
            )
            .cursor(Cursor::Pointer)
            .on_tap(action(move || jump(at, target)))
            .into_widget(),
        );
    }
    rows.push(
        container()
            .height(1.0)
            .decoration(BoxDecoration::new().color(f.theme.border))
            .into_widget(),
    );
    let sticky_h = lines.len() as f64 * f.line_px + 1.0;
    // Rows stretch to the panel's width (a column centres shrink-wrapped children by default,
    // which is what put the headers in the middle of the editor).
    // The panel owns the pointer over its area (presses and hover never reach the content
    // hidden behind it) while its rows still take their taps — `absorb_pointer` would also
    // swallow the rows' own clicks.
    let panel = GestureDetector::new(
        container().height(sticky_h).child(
            column(rows)
                .main_axis_size(MainAxisSize::Min)
                .cross_axis_alignment(CrossAxisAlignment::Stretch),
        ),
    )
    .on_pointer_down(action_event(|_| {}))
    .on_hover_move(action_event(|_| {}));
    stack(children![
        scroller,
        // The scroller is the text column only: the minimap and the vertical bar are its
        // siblings, so the panel spans it edge to edge (VS Code: up to the minimap).
        Positioned::new(panel)
            .left(0.0)
            .right(0.0)
            .top(0.0)
            .height(sticky_h)
            .into_widget(),
    ])
    .fit(StackFit::Expand)
    .into_widget()
}

/// The enclosing scope lines (outermost first) that have scrolled above the viewport top.
fn scoped_lines(f: &Frame, scroll_top: Signal<f64>) -> Vec<usize> {
    if !f.p.sticky_scroll || f.p.height.is_none() {
        return Vec::new();
    }
    let indent_of = |ln: usize| -> Option<usize> {
        let ls = f.li.line_start_of(ln);
        let s = &f.src[ls..line_end(f.src, ls)];
        if s.trim().is_empty() {
            None // blank lines don't open scopes
        } else {
            Some(s.chars().take_while(|c| *c == ' ' || *c == '\t').count())
        }
    };
    let top = scroll_top.get();
    // The buffer line at the viewport top, in visual-row space (folds collapse rows, soft
    // wrap splits them).
    let top_row = ((top - f.pad_t) / f.line_px).floor().max(0.0) as usize;
    let top_line = f.disp.line_at_row(top_row).min(f.line_count - 1);
    let mut acc = Vec::new();
    if let Some(mut ci) =
        indent_of(top_line).or_else(|| (top_line..f.line_count).find_map(indent_of))
    {
        let mut ln = top_line;
        while ln > 0 && acc.len() < 6 {
            ln -= 1;
            if f.disp.is_hidden(ln) {
                continue; // inside a collapsed fold
            }
            if let Some(ind) = indent_of(ln)
                && ind < ci
                && f.y_of(ln) < top
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
}
