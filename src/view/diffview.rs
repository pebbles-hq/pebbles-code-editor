//! Diff-editor rendering (VS Code's diff editor): removed-line bands and removed words on
//! the original side, inserted words on the modified side, filler rows that keep the two
//! sides of a side-by-side diff line-aligned, and (inline mode) the removed base lines shown
//! as red rows above their position.

use pebbles::prelude::*;

use crate::config::Props;
use crate::diff::{diff_lines, word_changes};
use crate::extensions::{BlockWidget, Decoration};

/// The decorations and block widgets a diff view contributes to this editor.
#[derive(Default)]
pub(crate) struct DiffParts {
    pub(crate) decos: Vec<Decoration>,
    pub(crate) blocks: Vec<BlockWidget>,
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

pub(crate) fn parts(p: &Props, src: &str, fs: f64, line_px: f64) -> DiffParts {
    let mut out = DiffParts::default();
    let t = &p.theme;
    let mine = line_spans(src);
    // Original side: this editor holds the base; the other side is the modified text.
    if let Some(m) = p.diff_original_of {
        let other = m.get();
        let theirs = line_spans(&other);
        for h in diff_lines(src, &other).hunks {
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
            if p.diff_align && h.cur.len() > h.base.len() {
                let at = if h.base.is_empty() {
                    below(h.base.start)
                } else {
                    Some(h.base.end - 1)
                };
                if let Some(at) = at {
                    out.blocks
                        .push(filler(at, h.cur.len() - h.base.len(), line_px));
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
    for h in diff_lines(&base, src).hunks {
        for (bl, cl) in h.base.clone().zip(h.cur.clone()) {
            let (s, b) = mine[cl];
            let (rem, ins) = word_changes(theirs[bl].1, b);
            let _ = rem;
            for (x, y) in ins {
                out.decos
                    .push(Decoration::background((s + x, s + y), t.diff_inserted_text));
            }
        }
        if p.diff_align && h.base.len() > h.cur.len() {
            let at = if h.cur.is_empty() {
                below(h.cur.start)
            } else {
                Some(h.cur.end - 1)
            };
            if let Some(at) = at {
                out.blocks
                    .push(filler(at, h.base.len() - h.cur.len(), line_px));
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
                            text(theirs[l].1.replace('\t', "    "))
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
        let parts = super::parts(&p, &base.get(), 14.0, 20.0);
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
        let parts = super::parts(&p, &cur.get(), 14.0, 20.0);
        assert!(parts.decos.iter().any(|d| d.range == (6, 7)));
        // "old" was deleted: one filler row below line 1.
        assert_eq!(parts.blocks.len(), 1);
        assert_eq!(parts.blocks[0].line, 1);
        // Inline mode shows the removed base lines as rows above the change instead.
        let p = Props::from(code_editor(cur).diff_base(base).inline_diff(true));
        let parts = super::parts(&p, &cur.get(), 14.0, 20.0);
        assert_eq!(parts.blocks.len(), 1);
        assert_eq!((parts.blocks[0].line, parts.blocks[0].height), (0, 40.0));
        // Plain change bars (no diff view) contribute nothing.
        let p = Props::from(code_editor(cur).diff_base(base));
        assert!(parts_empty(&super::parts(&p, &cur.get(), 14.0, 20.0)));
    }

    fn parts_empty(p: &DiffParts) -> bool {
        p.decos.is_empty() && p.blocks.is_empty()
    }
}
