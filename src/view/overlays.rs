//! The stacked overlay layers painted on the monospace grid, in z-order: current-line
//! band, rulers, indent guides, selection rects, bracket-match boxes, the highlighted text,
//! whitespace markers, and the carets. All are positioned by exact grid arithmetic and
//! clipped to the visible-line window; this builder is a pure function of the [`Frame`].

use pebbles::prelude::*;

use crate::MONO;
use crate::brackets::{DEFAULT_BRACKETS, find_bracket_match};
use crate::geometry::{col_of, line_char_len, line_end, line_of, line_start_of};
use crate::highlight::{slice_tokens, to_spans};
use crate::view::Frame;
use crate::view::chrome::band;

/// Build every overlay layer for the current frame, bottom to top.
pub(crate) fn build(f: &Frame) -> Vec<AnyWidget> {
    let mut layers: Vec<AnyWidget> = Vec::new();
    current_line(f, &mut layers);
    rulers(f, &mut layers);
    indent_guides(f, &mut layers);
    selection(f, &mut layers);
    bracket_match(f, &mut layers);
    code_text(f, &mut layers);
    whitespace(f, &mut layers);
    carets(f, &mut layers);
    layers
}

/// The current-line highlight band (primary caret's line, only when it has no selection).
fn current_line(f: &Frame, layers: &mut Vec<AnyWidget>) {
    if f.p.current_line && !f.has_primary_sel && (f.first_line..=f.last_line).contains(&f.cl) {
        layers.push(band(
            f.pad_t + f.cl as f64 * f.line_px,
            f.line_px,
            f.theme.current_line,
        ));
    }
}

/// Thin full-height vertical rulers at the configured print-margin columns.
fn rulers(f: &Frame, layers: &mut Vec<AnyWidget>) {
    for &rc in &f.p.rulers {
        layers.push(
            Positioned::new(
                container()
                    .width(1.0)
                    .height(f.content_h)
                    .decoration(BoxDecoration::new().color(f.theme.ruler)),
            )
            .left(f.pad_l + rc as f64 * f.advance)
            .top(0.0)
            .into_widget(),
        );
    }
}

/// A faint vertical line at each indent level inside a visible line's leading whitespace.
fn indent_guides(f: &Frame, layers: &mut Vec<AnyWidget>) {
    if !f.p.indent_guides {
        return;
    }
    let step = f.p.tab_size.max(1);
    for line in f.first_line..=f.last_line {
        let ls = line_start_of(f.src, line);
        let le = line_end(f.src, ls);
        let lead = f.src[ls..le]
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .count();
        let mut gcol = step;
        while gcol < lead {
            layers.push(
                Positioned::new(
                    container()
                        .width(1.0)
                        .height(f.line_px)
                        .decoration(BoxDecoration::new().color(f.theme.indent_guide)),
                )
                .left(f.pad_l + gcol as f64 * f.advance)
                .top(f.pad_t + line as f64 * f.line_px)
                .into_widget(),
            );
            gcol += step;
        }
    }
}

/// Selection rectangles — one set per non-empty range (multi-cursor), clipped to the window.
fn selection(f: &Frame, layers: &mut Vec<AnyWidget>) {
    for r in f.sels.ranges() {
        let (lo, hi) = (r.min(), r.max());
        if lo == hi {
            continue;
        }
        let (la, ca) = (line_of(f.src, lo), col_of(f.src, lo));
        let (lb, cb) = (line_of(f.src, hi), col_of(f.src, hi));
        for line in la.max(f.first_line)..=lb.min(f.last_line) {
            let start_col = if line == la { ca } else { 0 };
            let end_col = if line == lb {
                cb
            } else {
                line_char_len(f.src, line) + 1
            };
            let x = f.pad_l + start_col as f64 * f.advance;
            let w = ((end_col.saturating_sub(start_col)) as f64 * f.advance).max(2.0);
            layers.push(
                Positioned::new(
                    container()
                        .width(w)
                        .height(f.line_px)
                        .decoration(BoxDecoration::new().color(f.theme.selection)),
                )
                .left(x)
                .top(f.pad_t + line as f64 * f.line_px)
                .into_widget(),
            );
        }
    }
}

/// Box the bracket adjacent to the collapsed primary caret and its match (visible window).
fn bracket_match(f: &Frame, layers: &mut Vec<AnyWidget>) {
    if !f.p.match_brackets || f.has_primary_sel {
        return;
    }
    let brs = f.p.language.as_ref().map(|l| l.brackets()).unwrap_or(DEFAULT_BRACKETS);
    if let Some((a, b)) = find_bracket_match(f.src, brs, f.pcc) {
        for pos in [a, b] {
            let l = line_of(f.src, pos);
            if (f.first_line..=f.last_line).contains(&l) {
                let col = col_of(f.src, pos);
                layers.push(
                    Positioned::new(
                        container()
                            .width(f.advance)
                            .height(f.line_px)
                            .decoration(BoxDecoration::new().color(f.theme.matching_bracket)),
                    )
                    .left(f.pad_l + col as f64 * f.advance)
                    .top(f.pad_t + l as f64 * f.line_px)
                    .into_widget(),
                );
            }
        }
    }
}

/// The highlighted text of the visible block, as one rich-text at the first visible line.
/// Tokens are sliced to the window (rebased to the slice) so nothing offscreen is laid out.
fn code_text(f: &Frame, layers: &mut Vec<AnyWidget>) {
    let slice_start = line_start_of(f.src, f.first_line);
    let slice_end = line_end(f.src, line_start_of(f.src, f.last_line));
    let visible_src = &f.src[slice_start..slice_end];
    let vis_tokens = slice_tokens(f.tokens, slice_start, slice_end);
    let spans = to_spans(visible_src, &vis_tokens, f.theme, f.fs);
    layers.push(
        Positioned::new(text_rich(spans).line_height(f.lh as f32))
            .left(f.pad_l)
            .top(f.pad_t + f.first_line as f64 * f.line_px)
            .into_widget(),
    );
}

/// Whitespace/EOL markers: spaces→·, tabs→→ (aligned overlay) plus a ¶ at each line end.
fn whitespace(f: &Frame, layers: &mut Vec<AnyWidget>) {
    if !f.p.render_whitespace {
        return;
    }
    let slice_start = line_start_of(f.src, f.first_line);
    let slice_end = line_end(f.src, line_start_of(f.src, f.last_line));
    let visible_src = &f.src[slice_start..slice_end];
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
                    .size(f.fs as f32)
                    .font_family(MONO)
                    .color(f.theme.whitespace),
            ])
            .line_height(f.lh as f32),
        )
        .left(f.pad_l)
        .top(f.pad_t + f.first_line as f64 * f.line_px)
        .into_widget(),
    );
    for line in f.first_line..=f.last_line {
        let x = f.pad_l + line_char_len(f.src, line) as f64 * f.advance;
        layers.push(
            Positioned::new(
                text("¶".to_string())
                    .size((f.fs * 0.9) as f32)
                    .line_height(f.lh as f32)
                    .font_family(MONO)
                    .color(f.theme.whitespace),
            )
            .left(x)
            .top(f.pad_t + line as f64 * f.line_px)
            .into_widget(),
        );
    }
}

/// One blinking caret per cursor head in the visible window (all in phase).
fn carets(f: &Frame, layers: &mut Vec<AnyWidget>) {
    if !f.caret_on {
        return;
    }
    for r in f.sels.ranges() {
        let l = line_of(f.src, r.head);
        if !(f.first_line..=f.last_line).contains(&l) {
            continue;
        }
        let col = col_of(f.src, r.head);
        layers.push(
            Positioned::new(
                container()
                    .width(2.0)
                    .height(f.fs * 1.15)
                    .decoration(BoxDecoration::new().color(f.theme.caret)),
            )
            .left(f.pad_l + col as f64 * f.advance)
            .top(f.pad_t + l as f64 * f.line_px + (f.line_px - f.fs * 1.15) / 2.0)
            .into_widget(),
        );
    }
}
