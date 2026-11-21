//! Diff-editor rendering (VS Code's diff editor): removed-line bands and removed words on
//! the original side, inserted words on the modified side, filler rows that keep the two
//! sides of a side-by-side diff line-aligned, and (inline mode) the removed base lines shown
//! as red rows above their position.

use pebbles::prelude::*;

use crate::config::Props;
use crate::diff::{LineDiff, diff_lines_ws, unchanged_regions, word_changes};
use crate::extensions::{BlockWidget, Decoration};

/// The decorations and block widgets a diff view contributes to this editor.
#[derive(Default)]
pub(crate) struct DiffParts {
    pub(crate) decos: Vec<Decoration>,
    pub(crate) blocks: Vec<BlockWidget>,
    /// Collapsed unchanged regions: inclusive line ranges this editor hides.
    pub(crate) hidden: Vec<(usize, usize)>,
}

/// VS Code's `diffEditor.hideUnchangedRegions` defaults.
const CONTEXT_LINES: usize = 3;
const MIN_HIDDEN: usize = 3;

/// Collapse this side's unchanged regions (`mine` picks the side's range of each region):
/// hide the lines and show an "N hidden lines" bar (click to expand) on the row above.
fn collapse(
    p: &Props,
    d: &LineDiff,
    base_n: usize,
    cur_n: usize,
    mine: impl Fn(&crate::diff::UnchangedRegion) -> std::ops::Range<usize>,
    line_px: f64,
    out: &mut DiffParts,
) {
    let Some(expanded) = p.diff_collapse else {
        return;
    };
    let open = expanded.get();
    let t = &p.theme;
    for (k, r) in unchanged_regions(d, base_n, cur_n, CONTEXT_LINES, MIN_HIDDEN)
        .iter()
        .enumerate()
    {
        if open.contains(&k) {
            continue;
        }
        let mut range = mine(r);
        // The bar sits below the row above the region: keep line 0 visible.
        range.start = range.start.max(1);
        if range.is_empty() {
            continue;
        }
        let n = range.len();
        out.hidden.push((range.start, range.end - 1));
        let label = format!("{n} hidden line{}", if n == 1 { "" } else { "s" });
        let bar = semantics(
            SemanticsRole::Button,
            format!("Expand {label}"),
            GestureDetector::new(
                container()
                    .height(line_px)
                    .padding(EdgeInsets::symmetric(8.0, 0.0))
                    .alignment(Alignment::CENTER_LEFT)
                    .decoration(BoxDecoration::new().color(t.current_line))
                    .child(
                        row(children![
                            icon(tabler::FOLD).size(12.0).color(t.muted),
                            gap_w(6.0),
                            t.ui_text(label, -1.0).color(t.muted),
                        ])
                        .cross_axis_alignment(CrossAxisAlignment::Center),
                    ),
            )
            .cursor(pebbles::widgets::action_cursor())
            .on_tap(action(move || {
                expanded.update(|s| {
                    s.insert(k);
                });
            })),
        );
        out.blocks.push(BlockWidget {
            line: range.start - 1,
            height: line_px,
            widget: bar.into_widget(),
        });
    }
}

fn line_spans(s: &str) -> Vec<(usize, &str)> {
    let mut at = 0;
    s.split('\n')
        .map(|l| {
            let r = (at, l);
            at += l.len() + 1;
            r
        })
        .collect()
}

fn filler(line: usize, rows: usize, line_px: f64) -> BlockWidget {
    BlockWidget {
        line,
        height: rows as f64 * line_px,
        widget: container().height(rows as f64 * line_px).into_widget(),
    }
}

/// Where rows inserted "before line `at`" go (block widgets render below a line).
fn below(at: usize) -> Option<usize> {
    at.checked_sub(1)
}

/// How many visual rows `line` takes soft-wrapped at `wrap` columns (`0` = no wrap): the
/// display map's rule (tabs to their stops, continuation rows indented like the line).
pub(crate) fn wrapped_rows(line: &str, wrap: usize, tab_size: usize) -> usize {
    if wrap == 0 {
        return 1;
    }
    let ts = tab_size.max(1);
    let mut widths = Vec::new();
    let mut v = 0;
    for c in line.chars() {
        let w = if c == '\t' { ts - v % ts } else { 1 };
        widths.push(w);
        v += w;
    }
    if v <= wrap {
        return 1;
    }
    let ind: usize = line
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .zip(&widths)
        .map(|(_, w)| *w)
        .sum();
    let ind = if ind * 2 < wrap { ind } else { 0 };
    let (mut rows, mut i, mut first) = (0, 0, true);
    while i < widths.len() {
        let width = if first { wrap } else { wrap - ind }.max(1);
        let mut acc = 0;
        let s = i;
        while i < widths.len() && (i == s || acc + widths[i] <= width) {
            acc += widths[i];
            i += 1;
        }
        rows += 1;
        first = false;
    }
    rows.max(1)
}

/// The visual rows of `lines[range]` wrapped at `wrap`.
fn rows_in(
    lines: &[(usize, &str)],
    range: std::ops::Range<usize>,
    wrap: usize,
    ts: usize,
) -> usize {
    range
        .filter_map(|l| lines.get(l))
        .map(|(_, t)| wrapped_rows(t, wrap, ts))
        .sum()
}

pub(crate) fn parts(p: &Props, src: &str, fs: f64, line_px: f64, wrap: usize) -> DiffParts {
    let mut out = DiffParts::default();
    let ts = p.tab_size;
    let t = &p.theme;
    let mine = line_spans(src);
    // Original side: this editor holds the base; the other side is the modified text.
    if let Some(m) = p.diff_original_of {
        let other = m.get();
        let theirs = line_spans(&other);
        let d = diff_lines_ws(src, &other, p.diff_ignore_ws);
        collapse(
            p,
            &d,
            mine.len(),
            theirs.len(),
            |r| r.base.clone(),
            line_px,
            &mut out,
        );
        for h in d.hunks {
            for l in h.base.clone() {
                let (s, text) = mine[l];
                out.decos
                    .push(Decoration::line((s, s + text.len()), t.diff_removed_line));
            }
            for (bl, cl) in h.base.clone().zip(h.cur.clone()) {
                let (s, a) = mine[bl];
                let (rem, _) = word_changes(a, theirs[cl].1);
                for (x, y) in rem {
                    out.decos
                        .push(Decoration::background((s + x, s + y), t.diff_removed_text));
                }
            }
            // Filler rows: the other side's rows of the hunk beyond this side's (wrapped
            // rows count when soft wrap is on).
            let (me, them) = (
                rows_in(&mine, h.base.clone(), wrap, ts),
                rows_in(&theirs, h.cur.clone(), wrap, ts),
            );
            if p.diff_align && them > me {
                let at = if h.base.is_empty() {
                    below(h.base.start)
                } else {
                    Some(h.base.end - 1)
                };
                if let Some(at) = at {
                    out.blocks.push(filler(at, them - me, line_px));
                }
            }
        }
        return out;
    }
    // Modified side: the base is `diff_base`.
    let Some(base_sig) = p.diff_base else {
        return out;
    };
    if !(p.inline_diff || p.diff_align) {
        return out;
    }
    let base = base_sig.get();
    let theirs = line_spans(&base);
    let d = diff_lines_ws(&base, src, p.diff_ignore_ws);
    collapse(
        p,
        &d,
        theirs.len(),
        mine.len(),
        |r| r.cur.clone(),
        line_px,
        &mut out,
    );
    for h in d.hunks {
        for (bl, cl) in h.base.clone().zip(h.cur.clone()) {
            let (s, b) = mine[cl];
            let (rem, ins) = word_changes(theirs[bl].1, b);
            let _ = rem;
            for (x, y) in ins {
                out.decos
                    .push(Decoration::background((s + x, s + y), t.diff_inserted_text));
            }
        }
        let (me, them) = (
            rows_in(&mine, h.cur.clone(), wrap, ts),
            rows_in(&theirs, h.base.clone(), wrap, ts),
        );
        if p.diff_align && them > me {
            let at = if h.cur.is_empty() {
                below(h.cur.start)
            } else {
                Some(h.cur.end - 1)
            };
            if let Some(at) = at {
                out.blocks.push(filler(at, them - me, line_px));
            }
        }
        if p.inline_diff
            && !h.base.is_empty()
            && let Some(at) = below(h.cur.start)
        {
            let rows: Vec<AnyWidget> = h
                .base
                .clone()
                .map(|l| {
                    container()
                        .height(line_px)
                        .alignment(Alignment::CENTER_LEFT)
                        .decoration(BoxDecoration::new().color(t.diff_removed_line))
                        .child(
                            text(crate::geometry::expand_tabs(theirs[l].1, p.tab_size))
                                .size(fs as f32)
                                .line_height(p.lh as f32)
                                .font_family(p.font_family.clone())
                                .color(t.foreground),
                        )
                        .into_widget()
                })
                .collect();
            out.blocks.push(BlockWidget {
                line: at,
                height: h.base.len() as f64 * line_px,
                widget: column(rows)
                    .cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .main_axis_size(MainAxisSize::Min)
                    .into_widget(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::code_editor;
    use pebbles::core::create_root_signal;

    #[test]
    fn original_side_marks_removed_lines_words_and_fillers() {
        let base = create_root_signal(String::from("a\nlet x = 1;\nc\n"));
        let cur = create_root_signal(String::from("a\nlet y = 1;\nc\nd\n"));
        let p = Props::from(code_editor(base).diff_original_of(cur).diff_align(true));
        let parts = super::parts(&p, &base.get(), 14.0, 20.0, 0);
        // Line 1 band + the removed word "x".
        assert!(
            parts
                .decos
                .iter()
                .any(|d| d.range == (2, 12) && d.style.line_background.is_some())
        );
        assert!(
            parts
                .decos
                .iter()
                .any(|d| d.range == (6, 7) && d.style.background.is_some())
        );
        // One filler row (for the added "d") below line 2.
        assert_eq!(parts.blocks.len(), 1);
        assert_eq!((parts.blocks[0].line, parts.blocks[0].height), (2, 20.0));
    }

    #[test]
    fn modified_side_marks_inserted_words_fillers_and_inline_rows() {
        let base = create_root_signal(String::from("a\nlet x = 1;\nold\nc\n"));
        let cur = create_root_signal(String::from("a\nlet y = 1;\nc\n"));
        let p = Props::from(code_editor(cur).diff_base(base).diff_align(true));
        let parts = super::parts(&p, &cur.get(), 14.0, 20.0, 0);
        assert!(parts.decos.iter().any(|d| d.range == (6, 7)));
        // "old" was deleted: one filler row below line 1.
        assert_eq!(parts.blocks.len(), 1);
        assert_eq!(parts.blocks[0].line, 1);
        // Inline mode shows the removed base lines as rows above the change instead.
        let p = Props::from(code_editor(cur).diff_base(base).inline_diff(true));
        let parts = super::parts(&p, &cur.get(), 14.0, 20.0, 0);
        assert_eq!(parts.blocks.len(), 1);
        assert_eq!((parts.blocks[0].line, parts.blocks[0].height), (0, 40.0));
        // Plain change bars (no diff view) contribute nothing.
        let p = Props::from(code_editor(cur).diff_base(base));
        assert!(parts_empty(&super::parts(&p, &cur.get(), 14.0, 20.0, 0)));
    }

    fn parts_empty(p: &DiffParts) -> bool {
        p.decos.is_empty() && p.blocks.is_empty()
    }

    #[test]
    fn wrapped_rows_follow_the_display_map_rule() {
        assert_eq!(super::wrapped_rows("abc", 0, 4), 1);
        assert_eq!(super::wrapped_rows("abcdefghij", 10, 4), 1);
        assert_eq!(super::wrapped_rows("abcdefghijk", 10, 4), 2);
        // Continuation rows keep the indent: 2 + 8, then 8 per row.
        assert_eq!(super::wrapped_rows("  abcdefghijklmnopq", 10, 4), 3);
        // A tab takes its tab stop.
        assert_eq!(super::wrapped_rows("\tabcdefg", 10, 4), 2);
    }

    #[test]
    fn soft_wrapped_hunks_get_fillers_for_their_extra_rows() {
        // One changed line: the modified one is long and wraps to 3 rows at 10 cols.
        let base = create_root_signal(String::from("a\nshort\nz\n"));
        let cur = create_root_signal(String::from("a\nthis one is much longer\nz\n"));
        let p = Props::from(code_editor(base).diff_original_of(cur).diff_align(true));
        let parts = super::parts(&p, &base.get(), 14.0, 20.0, 10);
        assert_eq!(parts.blocks.len(), 1);
        assert_eq!(parts.blocks[0].line, 1);
        assert_eq!(parts.blocks[0].height, 2.0 * 20.0, "3 rows against 1");
        // Without wrap the same hunk is balanced.
        assert!(
            super::parts(&p, &base.get(), 14.0, 20.0, 0)
                .blocks
                .is_empty()
        );
    }
}
