//! Sticky scroll: pin the enclosing scopes of the top visible line to the top of the
//! viewport. Scopes are derived from indentation (no syntax tree needed) — walking up, each
//! line with strictly smaller indent than the last is an ancestor. The pinned panel is a
//! pointer barrier so clicks on it never fall through to the content hidden behind it.

use pebbles::prelude::*;

use crate::geometry::{line_end, line_start_of};
use crate::highlight::{slice_tokens, to_spans};
use crate::view::Frame;
use crate::view::chrome::with_alpha;

/// Overlay the pinned scope headers on top of `scroller` (or return it unchanged when sticky
/// scroll is off or there are no scrolled-off ancestors).
pub(crate) fn overlay(f: &Frame, scroller: AnyWidget, scroll_top: Signal<f64>) -> AnyWidget {
    let lines = scoped_lines(f, scroll_top);
    if lines.is_empty() {
        return scroller;
    }
    let mut rows: Vec<AnyWidget> = Vec::new();
    for &ln in &lines {
        let ls = line_start_of(f.src, ln);
        let le = line_end(f.src, ls);
        let spans = to_spans(&f.src[ls..le], &slice_tokens(f.tokens, ls, le), f.theme, f.fs);
        rows.push(
            container()
                .height(f.line_px)
                .decoration(BoxDecoration::new().color(f.theme.background))
                .padding(EdgeInsets::only(f.pad_l, 0.0, 0.0, 0.0))
                .child(text_rich(spans).line_height(f.lh as f32))
                .into_widget(),
        );
    }
    rows.push(
        container()
            .height(1.0)
            .decoration(BoxDecoration::new().color(with_alpha(f.theme.punctuation, 0.35)))
            .into_widget(),
    );
    let sticky_h = lines.len() as f64 * f.line_px + 1.0;
    let panel = absorb_pointer(
        container()
            .height(sticky_h)
            .decoration(BoxDecoration::new().color(f.theme.background))
            .child(column(rows).main_axis_size(MainAxisSize::Min)),
    );
    stack(children![
        scroller,
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
        let ls = line_start_of(f.src, ln);
        let s = &f.src[ls..line_end(f.src, ls)];
        if s.trim().is_empty() {
            None // blank lines don't open scopes
        } else {
            Some(s.chars().take_while(|c| *c == ' ' || *c == '\t').count())
        }
    };
    let top = scroll_top.get();
    let top_line = (((top - f.pad_t) / f.line_px).floor().max(0.0) as usize).min(f.line_count - 1);
    let mut acc = Vec::new();
    if let Some(mut ci) =
        indent_of(top_line).or_else(|| (top_line..f.line_count).find_map(indent_of))
    {
        let mut ln = top_line;
        while ln > 0 && acc.len() < 6 {
            ln -= 1;
            if let Some(ind) = indent_of(ln)
                && ind < ci
                && (f.pad_t + ln as f64 * f.line_px) < top
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
