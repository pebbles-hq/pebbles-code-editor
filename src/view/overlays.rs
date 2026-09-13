//! The stacked overlay layers painted on the monospace grid, in z-order: current-line
//! band, rulers, indent guides, selection rects, bracket-match boxes, the highlighted text,
//! whitespace markers, and the carets. All are positioned by exact grid arithmetic and
//! clipped to the visible-line window; this builder is a pure function of the [`Frame`].

use pebbles::prelude::*;

use crate::brackets::{DEFAULT_BRACKETS, find_bracket_match};
use crate::geometry::{byte_at, col_of, line_char_len, line_end, line_of, line_start_of};
use crate::highlight::{slice_tokens, to_spans};
use crate::providers::Severity;
use crate::theme::EditorTheme;
use crate::view::Frame;
use crate::view::chrome::{band, with_alpha};

/// Build every overlay layer for the current frame, bottom to top.
pub(crate) fn build(f: &Frame) -> Vec<AnyWidget> {
    let mut layers: Vec<AnyWidget> = Vec::new();
    diff_bands(f, &mut layers);
    current_line(f, &mut layers);
    rulers(f, &mut layers);
    indent_guides(f, &mut layers);
    word_occurrences(f, &mut layers);
    selection(f, &mut layers);
    bracket_match(f, &mut layers);
    code_text(f, &mut layers);
    inlay_hints(f, &mut layers);
    diagnostics(f, &mut layers);
    whitespace(f, &mut layers);
    carets(f, &mut layers);
    layers
}

/// The underline color for a diagnostic severity (mapped from the theme palette).
pub(crate) fn severity_color(theme: &EditorTheme, sev: Severity) -> Color {
    match sev {
        Severity::Error => theme.diag_error,
        Severity::Warning => theme.diag_warning,
        Severity::Info => theme.diag_info,
        Severity::Hint => theme.diag_hint,
    }
}

/// Inline-diff bands: a green full-width band on each added line's rows, and a red 2px marker
/// at the top of a line where base lines were deleted.
fn diff_bands(f: &Frame, layers: &mut Vec<AnyWidget>) {
    let Some(d) = f.diff else { return };
    for line in f.first_line..=f.last_line {
        if f.disp.is_hidden(line) {
            continue;
        }
        if d.added.get(line).copied().unwrap_or(false) {
            for (row, _) in f.segments(line) {
                layers.push(band(f.row_y(row), f.line_px, f.theme.diff_added));
            }
        }
        if d.removed_before.contains(&line) {
            layers.push(band(f.y_of(line), 2.0, f.theme.diff_removed));
        }
    }
    if d.removed_at_end && f.line_visible(f.line_count.saturating_sub(1)) {
        let last = f.line_count.saturating_sub(1);
        layers.push(band(f.y_of(last) + f.line_px - 2.0, 2.0, f.theme.diff_removed));
    }
}

/// The current-line highlight band (primary caret's line, only when it has no selection).
fn current_line(f: &Frame, layers: &mut Vec<AnyWidget>) {
    if f.p.current_line && !f.has_primary_sel && f.line_visible(f.cl) {
        for (row, _) in f.segments(f.cl) {
            layers.push(band(f.row_y(row), f.line_px, f.theme.current_line));
        }
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
        if f.disp.is_hidden(line) {
            continue;
        }
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
                .top(f.y_of(line))
                .into_widget(),
            );
            gcol += step;
        }
    }
}

/// Faintly box every OTHER occurrence of the selected word in the visible window — the
/// "highlight matches of the selection" feature. Only when a single-line word (≥2 chars) is
/// selected and the find bar is closed.
fn word_occurrences(f: &Frame, layers: &mut Vec<AnyWidget>) {
    if !f.highlight_word_matches {
        return;
    }
    let sel = f.sels.primary();
    if sel.is_empty() {
        return;
    }
    let (lo, hi) = (sel.min(), sel.max());
    let word = &f.src[lo..hi];
    if word.len() < 2 || word.chars().any(|c| c.is_whitespace()) {
        return;
    }
    let color = with_alpha(f.theme.gutter_active_fg, 0.18);
    let window_start = line_start_of(f.src, f.first_line);
    let window_end = line_end(f.src, line_start_of(f.src, f.last_line));
    let mut from = window_start;
    while let Some(rel) = f.src[from..window_end].find(word) {
        let start = from + rel;
        from = start + word.len();
        if start == lo {
            continue; // skip the actual selection
        }
        let line = line_of(f.src, start);
        if !f.line_visible(line) {
            continue;
        }
        let (x, y) = f.xy(line, col_of(f.src, start));
        let w = word.chars().count() as f64 * f.advance;
        layers.push(
            Positioned::new(
                container()
                    .width(w)
                    .height(f.line_px)
                    .decoration(BoxDecoration::new().color(color).radius(BorderRadius::all(2.0))),
            )
            .left(x)
            .top(y)
            .into_widget(),
        );
    }
}

/// Selection rectangles — one set per non-empty range (multi-cursor), clipped to the window.
fn selection(f: &Frame, layers: &mut Vec<AnyWidget>) {
    let color = if f.focused { f.theme.selection } else { f.theme.selection_inactive };
    for r in f.sels.ranges() {
        let (lo, hi) = (r.min(), r.max());
        if lo == hi {
            continue;
        }
        let (la, ca) = (line_of(f.src, lo), col_of(f.src, lo));
        let (lb, cb) = (line_of(f.src, hi), col_of(f.src, hi));
        for line in la.max(f.first_line)..=lb.min(f.last_line) {
            if f.disp.is_hidden(line) {
                continue;
            }
            let start_col = if line == la { ca } else { 0 };
            let end_col = if line == lb { cb } else { line_char_len(f.src, line) };
            let extend_nl = line != lb; // hint the wrapped newline on the line's last segment
            let segs = f.segments(line);
            let last = segs.len().saturating_sub(1);
            for (idx, (row, seg)) in segs.into_iter().enumerate() {
                let s = start_col.max(seg.start);
                let e = end_col.min(seg.end);
                let is_last = idx == last;
                if e < s || (e == s && !(extend_nl && is_last)) {
                    continue;
                }
                let x = f.pad_l + (s - seg.start) as f64 * f.advance;
                let mut w = (e - s) as f64 * f.advance;
                if extend_nl && is_last {
                    w += f.advance;
                }
                layers.push(
                    Positioned::new(
                        container()
                            .width(w.max(2.0))
                            .height(f.line_px)
                            .decoration(BoxDecoration::new().color(color)),
                    )
                    .left(x)
                    .top(f.row_y(row))
                    .into_widget(),
                );
            }
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
            if f.line_visible(l) {
                let (x, y) = f.xy(l, col_of(f.src, pos));
                layers.push(
                    Positioned::new(
                        container()
                            .width(f.advance)
                            .height(f.line_px)
                            .decoration(BoxDecoration::new().color(f.theme.matching_bracket)),
                    )
                    .left(x)
                    .top(y)
                    .into_widget(),
                );
            }
        }
    }
}

/// The highlighted text of the visible window, rendered one rich-text per VISUAL row — so
/// folded lines aren't drawn and a wrapped line's segments each land on their own row. Tokens
/// are sliced per segment.
fn code_text(f: &Frame, layers: &mut Vec<AnyWidget>) {
    for line in f.first_line..=f.last_line {
        if f.disp.is_hidden(line) {
            continue;
        }
        for (row, seg) in f.segments(line) {
            // Byte range of this row's char span [seg.start, seg.end).
            let bs = byte_at(f.src, line, seg.start);
            let be = byte_at(f.src, line, seg.end);
            if be <= bs {
                continue;
            }
            let toks = slice_tokens(f.tokens, bs, be);
            let spans = to_spans(&f.src[bs..be], &toks, f.theme, f.fs, f.font_family);
            layers.push(
                Positioned::new(
                    text_rich(spans)
                        .line_height(f.lh as f32)
                        .letter_spacing(f.letter_spacing as f32),
                )
                .left(f.pad_l)
                .top(f.row_y(row))
                .into_widget(),
            );
        }
    }
}

/// Whitespace/EOL markers: spaces→·, tabs→→ (aligned overlay) plus a ¶ at each line end.
fn whitespace(f: &Frame, layers: &mut Vec<AnyWidget>) {
    if !f.p.render_whitespace {
        return;
    }
    for line in f.first_line..=f.last_line {
        if f.disp.is_hidden(line) {
            continue;
        }
        let len = line_char_len(f.src, line);
        for (row, seg) in f.segments(line) {
            let bs = byte_at(f.src, line, seg.start);
            let be = byte_at(f.src, line, seg.end);
            let marks: String = f.src[bs..be]
                .chars()
                .map(|c| match c {
                    ' ' => '·',
                    '\t' => '→',
                    _ => ' ',
                })
                .collect();
            if !marks.trim().is_empty() {
                layers.push(
                    Positioned::new(
                        text_rich(vec![
                            span(marks)
                                .size(f.fs as f32)
                                .font_family(f.font_family)
                                .color(f.theme.whitespace),
                        ])
                        .line_height(f.lh as f32)
                        .letter_spacing(f.letter_spacing as f32),
                    )
                    .left(f.pad_l)
                    .top(f.row_y(row))
                    .into_widget(),
                );
            }
            // ¶ at the true line end (only on the line's last segment).
            if seg.end == len {
                layers.push(
                    Positioned::new(
                        text("¶".to_string())
                            .size((f.fs * 0.9) as f32)
                            .line_height(f.lh as f32)
                            .font_family(f.font_family)
                            .color(f.theme.whitespace),
                    )
                    .left(f.pad_l + (len - seg.start) as f64 * f.advance)
                    .top(f.row_y(row))
                    .into_widget(),
                );
            }
        }
    }
}

/// Diagnostic underlines: a thin severity-colored line under each diagnostic range, per
/// line-segment, clipped to the visible window. Reads the reactive diagnostics list.
fn diagnostics(f: &Frame, layers: &mut Vec<AnyWidget>) {
    let Some(sig) = f.p.diagnostics else {
        return;
    };
    for d in sig.get() {
        let (lo, hi) = d.range;
        let color = severity_color(f.theme, d.severity);
        let (la, ca) = (line_of(f.src, lo), col_of(f.src, lo));
        let (lb, cb) = (line_of(f.src, hi), col_of(f.src, hi));
        for line in la.max(f.first_line)..=lb.min(f.last_line) {
            if f.disp.is_hidden(line) {
                continue;
            }
            let start_col = if line == la { ca } else { 0 };
            let end_col = if line == lb { cb } else { line_char_len(f.src, line) };
            // Underline per visual segment so it follows wrapped rows.
            for (row, seg) in f.segments(line) {
                let s = start_col.max(seg.start);
                let e = end_col.min(seg.end);
                if e <= s {
                    continue;
                }
                let x = f.pad_l + (s - seg.start) as f64 * f.advance;
                let w = ((e - s) as f64 * f.advance).max(f.advance);
                layers.push(
                    Positioned::new(
                        container()
                            .width(w)
                            .height(2.0)
                            .decoration(BoxDecoration::new().color(color)),
                    )
                    .left(x)
                    .top(f.row_y(row) + f.line_px - 2.0)
                    .into_widget(),
                );
            }
        }
    }
}

/// Inlay hints: faded inline labels anchored at a byte offset (visible window only). On the
/// fixed grid these overlay at the column rather than reflowing the line, so they read best
/// as end-of-line / at-boundary annotations (type hints, parameter names).
fn inlay_hints(f: &Frame, layers: &mut Vec<AnyWidget>) {
    let Some(sig) = f.p.inlay_hints else {
        return;
    };
    for h in sig.get() {
        let line = line_of(f.src, h.at);
        if !f.line_visible(line) {
            continue;
        }
        let (x, y) = f.xy(line, col_of(f.src, h.at));
        layers.push(
            Positioned::new(
                text(h.label.clone())
                    .size((f.fs * 0.85) as f32)
                    .line_height(f.lh as f32)
                    .font_family(f.font_family)
                    .color(f.theme.muted),
            )
            .left(x)
            .top(y)
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
        if !f.line_visible(l) {
            continue;
        }
        let (x, y) = f.xy(l, col_of(f.src, r.head));
        layers.push(
            Positioned::new(
                container()
                    .width(2.0)
                    .height(f.fs * 1.15)
                    .decoration(BoxDecoration::new().color(f.theme.caret)),
            )
            .left(x)
            .top(y + (f.line_px - f.fs * 1.15) / 2.0)
            .into_widget(),
        );
    }
}
