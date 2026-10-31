//! The display model behind code folding **and** soft wrap: a list of visual [`Row`]s, each a
//! `(buffer line, char-column span)`. A hidden (folded) line contributes zero rows; a wrapped
//! line contributes several. The whole view renders in visual-row space, so folding collapses
//! rows and wrapping splits them — one abstraction for both.

use std::collections::BTreeSet;

/// The foldable regions as `(head_line, last_line)` pairs: indentation-derived (a non-blank
/// line heads a region spanning the following deeper-indented lines) plus `#region` /
/// `#endregion` marker regions in any comment syntax. Sorted by head.
#[cfg(test)]
pub(crate) fn foldable(src: &str) -> Vec<(usize, usize)> {
    crate::lines::regions_of(src)
}

/// The innermost region containing `line` (a region whose head is `line` wins).
pub(crate) fn region_at(regions: &[(usize, usize)], line: usize) -> Option<(usize, usize)> {
    if let Some(r) = regions.iter().find(|r| r.0 == line) {
        return Some(*r);
    }
    regions
        .iter()
        .filter(|r| r.0 <= line && line <= r.1)
        .min_by_key(|r| r.1 - r.0)
        .copied()
}

/// Nesting depth of each region (1 = top level), parallel to `regions`.
pub(crate) fn depths(regions: &[(usize, usize)]) -> Vec<usize> {
    let mut stack: Vec<(usize, usize)> = Vec::new();
    regions
        .iter()
        .map(|&r| {
            while stack.last().is_some_and(|s| r.0 > s.1) {
                stack.pop();
            }
            stack.push(r);
            stack.len()
        })
        .collect()
}

/// Heads to fold for "Fold Level N": every region at depth `level` (VS Code also leaves the
/// region containing the cursor line… open only for Fold All Except; Fold Level folds them).
pub(crate) fn heads_at_level(regions: &[(usize, usize)], level: usize) -> Vec<usize> {
    regions
        .iter()
        .zip(depths(regions))
        .filter(|(_, d)| *d == level)
        .map(|(r, _)| r.0)
        .collect()
}

/// `region` itself and every region nested inside it.
pub(crate) fn nested_heads(regions: &[(usize, usize)], region: (usize, usize)) -> Vec<usize> {
    regions
        .iter()
        .filter(|r| r.0 >= region.0 && r.1 <= region.1)
        .map(|r| r.0)
        .collect()
}

/// Lines the folds in `folded` hide (for the "N lines" badge on a folded head).
pub(crate) fn hidden_count(regions: &[(usize, usize)], head: usize) -> usize {
    regions
        .iter()
        .find(|r| r.0 == head)
        .map_or(0, |r| r.1 - r.0)
}

/// One visual row: a slice `[start, end)` (char columns) of buffer `line`. `first` marks the
/// line's first row (line number + fold arrow); `gap` marks a blank row reserved below a line
/// for a block widget (rendered by the view, not text).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub first: bool,
    pub gap: bool,
    /// Wrap indent of a continuation row, in columns (its text starts this far right).
    pub indent: usize,
}

/// Inline widgets that occupy space on a line (inlay hints): `line -> [(col, width_cols)]`,
/// sorted by column. A hint at `col` is drawn just before the char at `col` (after the caret
/// position at `col`, like VS Code's injected text), pushing the rest of the line right.
pub(crate) type Inlays = std::collections::HashMap<usize, Vec<(usize, usize)>>;

/// Buffer-line ↔ visual-row mapping for a fold set + wrap width.
pub(crate) struct DisplayMap {
    inlays: std::rc::Rc<Inlays>,
    rows: Vec<Row>,
    row0: Vec<usize>, // line -> index of its first row (hidden lines -> the fold head's row)
    hidden: Vec<bool>,
    /// Identity fast path (nothing folded, no wrap, no gaps): one row per line, answered from
    /// the line index without materializing a row per line.
    ident: Option<std::rc::Rc<crate::lines::LineIndex>>,
}

impl DisplayMap {
    /// Build the map. `char_lens[line]` is that line's length in chars; `wrap_cols == 0`
    /// disables wrapping (one row per visible line).
    #[cfg(test)]
    pub(crate) fn new(
        line_count: usize,
        folded: &BTreeSet<usize>,
        regions: &[(usize, usize)],
        char_lens: &[usize],
        wrap_cols: usize,
        gaps: &[usize],
        wrap_indents: &[usize],
    ) -> Self {
        Self::new_with_inlays(
            line_count,
            folded,
            regions,
            char_lens,
            wrap_cols,
            gaps,
            wrap_indents,
            std::rc::Rc::new(Inlays::new()),
        )
    }

    /// [`DisplayMap::new`] with inlay widths: they count toward the wrap width and shift
    /// every column after them (see [`DisplayMap::place`]).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_with_inlays(
        line_count: usize,
        folded: &BTreeSet<usize>,
        regions: &[(usize, usize)],
        char_lens: &[usize],
        wrap_cols: usize,
        gaps: &[usize],
        wrap_indents: &[usize],
        inlays: std::rc::Rc<Inlays>,
    ) -> Self {
        let mut hidden = vec![false; line_count];
        for &(head, last) in regions {
            if folded.contains(&head) {
                let end = last.min(line_count.saturating_sub(1));
                for h in hidden.iter_mut().take(end + 1).skip(head + 1) {
                    *h = true;
                }
            }
        }
        let mut rows = Vec::new();
        let mut row0 = vec![0usize; line_count];
        for line in 0..line_count {
            if hidden[line] {
                row0[line] = rows.len().saturating_sub(1);
                continue;
            }
            row0[line] = rows.len();
            let len = char_lens.get(line).copied().unwrap_or(0);
            let ins = inlays.get(&line).map(|v| v.as_slice()).unwrap_or(&[]);
            let ins_w: usize = ins.iter().filter(|(c, _)| *c < len).map(|(_, w)| w).sum();
            if wrap_cols == 0 || len + ins_w <= wrap_cols {
                rows.push(Row {
                    line,
                    start: 0,
                    end: len,
                    first: true,
                    gap: false,
                    indent: 0,
                });
            } else {
                let mut s = 0;
                let mut first = true;
                // Wrap indent (VS Code `wrappingIndent: same`): continuation rows start at
                // the line's indentation, unless that would leave less than half the width.
                let ind = wrap_indents.get(line).copied().unwrap_or(0);
                let ind = if ind * 2 < wrap_cols { ind } else { 0 };
                while s < len {
                    let width = if first { wrap_cols } else { wrap_cols - ind };
                    let e = if ins.is_empty() {
                        (s + width.max(1)).min(len)
                    } else {
                        // Visual widths: each char is 1 column plus the hints drawn before it.
                        let mut e = s;
                        let mut acc = 0;
                        while e < len {
                            let w = 1 + ins
                                .iter()
                                .filter(|(c, _)| *c == e)
                                .map(|(_, w)| w)
                                .sum::<usize>();
                            if e > s && acc + w > width.max(1) {
                                break;
                            }
                            acc += w;
                            e += 1;
                        }
                        e
                    };
                    rows.push(Row {
                        line,
                        start: s,
                        end: e,
                        first,
                        gap: false,
                        indent: if first { 0 } else { ind },
                    });
                    s = e;
                    first = false;
                }
            }
            // Reserve blank rows below the line for a block widget.
            for _ in 0..gaps.get(line).copied().unwrap_or(0) {
                rows.push(Row {
                    line,
                    start: 0,
                    end: 0,
                    first: false,
                    gap: true,
                    indent: 0,
                });
            }
        }
        DisplayMap {
            inlays,
            rows,
            row0,
            hidden,
            ident: None,
        }
    }

    /// Like [`DisplayMap::new`] over `index`'s lines and foldable regions, but O(1) when no
    /// fold applies, wrapping is off and there are no gap rows (the common case): the map is
    /// then the identity and is answered lazily from the index.
    pub(crate) fn for_index(
        index: &std::rc::Rc<crate::lines::LineIndex>,
        folded: &BTreeSet<usize>,
        wrap_cols: usize,
        gaps: &[usize],
    ) -> Self {
        Self::for_index_inlays(
            index,
            folded,
            wrap_cols,
            gaps,
            std::rc::Rc::new(Inlays::new()),
        )
    }

    /// [`DisplayMap::for_index`] with inlay widths (the identity fast path still applies
    /// when nothing folds, wraps or reserves gap rows).
    pub(crate) fn for_index_inlays(
        index: &std::rc::Rc<crate::lines::LineIndex>,
        folded: &BTreeSet<usize>,
        wrap_cols: usize,
        gaps: &[usize],
        inlays: std::rc::Rc<Inlays>,
    ) -> Self {
        let regions = index.regions();
        let any_fold = folded.iter().any(|h| index.is_fold_head(*h));
        if !any_fold && wrap_cols == 0 && gaps.iter().all(|g| *g == 0) {
            return DisplayMap {
                inlays,
                rows: Vec::new(),
                row0: Vec::new(),
                hidden: Vec::new(),
                ident: Some(index.clone()),
            };
        }
        let wrap_indents = if wrap_cols > 0 {
            index.indent_cols()
        } else {
            Vec::new()
        };
        DisplayMap::new_with_inlays(
            index.line_count(),
            folded,
            regions,
            index.char_lens(),
            wrap_cols,
            gaps,
            &wrap_indents,
            inlays,
        )
    }

    /// The inlays of `line` (sorted `(col, width_cols)`).
    pub(crate) fn inlays_of(&self, line: usize) -> &[(usize, usize)] {
        self.inlays.get(&line).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Total inlay width on `line` for hint columns in `[lo, hi)`.
    fn inlay_sum(&self, line: usize, lo: usize, hi: usize) -> usize {
        self.inlays_of(line)
            .iter()
            .filter(|(c, _)| *c >= lo && *c < hi)
            .map(|(_, w)| w)
            .sum()
    }

    /// The x column (within `row`) of the caret position at `col`: before any hint at `col`.
    pub(crate) fn caret_xcol(&self, row: &Row, col: usize) -> usize {
        col.saturating_sub(row.start) + self.inlay_sum(row.line, row.start, col) + row.indent
    }

    /// The x column (within `row`) where the char at `col` is drawn: after the hints at `col`.
    pub(crate) fn char_xcol(&self, row: &Row, col: usize) -> usize {
        col.saturating_sub(row.start) + self.inlay_sum(row.line, row.start, col + 1) + row.indent
    }

    /// Hit-test: the buffer `(line, col)` whose caret x is nearest `xcol` (fractional
    /// columns) on display `row` — a click on a hint lands on the nearer side of it.
    pub(crate) fn hit(
        &self,
        row: usize,
        xcol: f64,
        line_len: impl Fn(usize) -> usize,
    ) -> (usize, usize) {
        let r = self.row_at(row);
        let len = line_len(r.line);
        let end = r.end.min(len);
        if self.inlays_of(r.line).is_empty() {
            let c = (r.start as f64 + (xcol - r.indent as f64).max(0.0)).round() as usize;
            return (r.line, c.min(end));
        }
        let mut best = (r.start, f64::MAX);
        for c in r.start..=end {
            let d = (self.caret_xcol(&r, c) as f64 - xcol).abs();
            if d < best.1 {
                best = (c, d);
            }
        }
        (r.line, best.0)
    }

    #[cfg(test)]
    /// The `(display_row, x_col)` at which the char at `(line, col)` is drawn (after hints).
    pub(crate) fn place_char(&self, line: usize, col: usize) -> (usize, usize) {
        let (row, _) = self.place(line, col);
        (row, self.char_xcol(&self.row_at(row), col))
    }

    pub(crate) fn rows(&self) -> usize {
        match &self.ident {
            Some(ix) => ix.line_count(),
            None => self.rows.len(),
        }
    }

    pub(crate) fn is_hidden(&self, line: usize) -> bool {
        self.hidden.get(line).copied().unwrap_or(false)
    }

    /// The buffer line shown at display `row` (clamped).
    pub(crate) fn line_at_row(&self, row: usize) -> usize {
        if let Some(ix) = &self.ident {
            return row.min(ix.line_count() - 1);
        }
        let row = row.min(self.rows.len().saturating_sub(1));
        self.rows.get(row).map(|r| r.line).unwrap_or(0)
    }

    /// The visual row at `row` (clamped).
    pub(crate) fn row_at(&self, row: usize) -> Row {
        if let Some(ix) = &self.ident {
            let line = row.min(ix.line_count() - 1);
            return Row {
                line,
                start: 0,
                end: ix.line_char_len(line),
                first: true,
                gap: false,
                indent: 0,
            };
        }
        let row = row.min(self.rows.len().saturating_sub(1));
        self.rows.get(row).copied().unwrap_or(Row {
            line: 0,
            start: 0,
            end: 0,
            first: true,
            gap: false,
            indent: 0,
        })
    }

    /// The first reserved gap-row index directly below `line`, if any (block-widget anchor).
    pub(crate) fn gap_row_of(&self, line: usize) -> Option<usize> {
        if self.ident.is_some() {
            return None;
        }
        let start = self.row0.get(line).copied()?;
        for (i, r) in self.rows.iter().enumerate().skip(start) {
            if r.line != line {
                break;
            }
            if r.gap {
                return Some(i);
            }
        }
        None
    }

    /// Place `(line, col)` → `(display_row, x_col_within_row)` (never on a gap row).
    pub(crate) fn place(&self, line: usize, col: usize) -> (usize, usize) {
        if let Some(ix) = &self.ident {
            let line = if line < ix.line_count() { line } else { 0 };
            return (line, col + self.inlay_sum(line, 0, col));
        }
        let mut i = self.row0.get(line).copied().unwrap_or(0);
        while i + 1 < self.rows.len()
            && self.rows[i + 1].line == line
            && !self.rows[i + 1].gap
            && self.rows[i].end <= col
        {
            i += 1;
        }
        match self.rows.get(i) {
            Some(r) => (i, self.caret_xcol(r, col)),
            None => (i, col),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folds(h: &[usize]) -> BTreeSet<usize> {
        h.iter().copied().collect()
    }

    #[test]
    fn foldable_finds_indented_blocks() {
        let r = foldable("fn a() {\n    x;\n    y;\n}\ntop\n");
        assert!(r.contains(&(0, 2)));
    }

    #[test]
    fn folding_hides_lines() {
        let m = DisplayMap::new(5, &folds(&[0]), &[(0, 2)], &[8, 6, 6, 1, 0], 0, &[], &[]);
        assert_eq!(m.rows(), 3);
        assert!(m.is_hidden(1) && m.is_hidden(2));
        assert_eq!(m.line_at_row(1), 3);
    }

    #[test]
    fn wrapping_splits_a_long_line() {
        // One 25-char line, wrap at 10 → 3 rows.
        let m = DisplayMap::new(1, &folds(&[]), &[], &[25], 10, &[], &[]);
        assert_eq!(m.rows(), 3);
        assert_eq!(
            m.row_at(0),
            Row {
                line: 0,
                start: 0,
                end: 10,
                first: true,
                gap: false,
                indent: 0,
            }
        );
        assert_eq!(
            m.row_at(1),
            Row {
                line: 0,
                start: 10,
                end: 20,
                first: false,
                gap: false,
                indent: 0,
            }
        );
        assert_eq!(
            m.row_at(2),
            Row {
                line: 0,
                start: 20,
                end: 25,
                first: false,
                gap: false,
                indent: 0,
            }
        );
        // col 14 lands on row 1, x-col 4.
        assert_eq!(m.place(0, 14), (1, 4));
        assert_eq!(m.place(0, 0), (0, 0));
        assert_eq!(m.place(0, 25), (2, 5));
    }

    /// The identity fast path answers every query exactly like the materialized map.
    #[test]
    fn identity_fast_path_matches_full_map() {
        let src = "fn a() {\n    x;\n\n    yy;\n}\ntail";
        let ix = std::rc::Rc::new(crate::lines::LineIndex::build(src));
        let regions = foldable(src);
        let lens: Vec<usize> = src.split('\n').map(|l| l.chars().count()).collect();
        // `5` is not a fold head, so folding it changes nothing either.
        for f in [folds(&[]), folds(&[5])] {
            let fast = DisplayMap::for_index(&ix, &f, 0, &[0; 6]);
            assert!(fast.ident.is_some());
            let full = DisplayMap::new(lens.len(), &f, &regions, &lens, 0, &[], &[]);
            assert_eq!(fast.rows(), full.rows());
            for r in 0..full.rows() + 3 {
                assert_eq!(fast.line_at_row(r), full.line_at_row(r));
                assert_eq!(fast.row_at(r), full.row_at(r));
            }
            for l in 0..lens.len() + 2 {
                assert_eq!(fast.is_hidden(l), full.is_hidden(l));
                assert_eq!(fast.gap_row_of(l), full.gap_row_of(l));
                for c in [0, 2, 9] {
                    assert_eq!(fast.place(l, c), full.place(l, c));
                }
            }
        }
        assert!(
            DisplayMap::for_index(&ix, &folds(&[0]), 0, &[])
                .ident
                .is_none()
        );
        assert!(
            DisplayMap::for_index(&ix, &folds(&[]), 4, &[])
                .ident
                .is_none()
        );
        assert!(
            DisplayMap::for_index(&ix, &folds(&[]), 0, &[0, 1])
                .ident
                .is_none()
        );
    }

    #[test]
    fn no_wrap_is_one_row_per_line() {
        let m = DisplayMap::new(3, &folds(&[]), &[], &[4, 4, 4], 0, &[], &[]);
        assert_eq!(m.rows(), 3);
        assert_eq!(m.place(2, 3), (2, 3));
    }

    #[test]
    fn region_markers_fold_in_any_comment_syntax() {
        let src = "a\n// #region Setup\nx\ny\n// #endregion\nb\n# region py\nz\n#endregion\n";
        let r = foldable(src);
        assert!(r.contains(&(1, 4)), "{r:?}");
        assert!(r.contains(&(6, 8)), "{r:?}");
        // C# style (no comment prefix) and HTML comments too.
        assert!(foldable("#region A\nx\n#endregion").contains(&(0, 2)));
        assert!(foldable("<!-- #region -->\nx\n<!-- #endregion -->").contains(&(0, 2)));
        // `regional` is not a marker.
        assert!(foldable("// regional\nx\n// endregion").is_empty());
    }

    #[test]
    fn fold_level_nesting_and_region_lookup() {
        let src = "a {\n  b {\n    c\n  }\n  d {\n    e\n  }\n}\nf {\n  g\n}";
        let r = foldable(src);
        assert_eq!(heads_at_level(&r, 1), vec![0, 8]);
        assert_eq!(heads_at_level(&r, 2), vec![1, 4]);
        assert_eq!(region_at(&r, 2), Some((1, 2)));
        assert_eq!(region_at(&r, 0), Some((0, 6)));
        assert_eq!(nested_heads(&r, (0, 6)), vec![0, 1, 4]);
        assert_eq!(hidden_count(&r, 0), 6);
    }

    #[test]
    fn soft_wrap_continuation_rows_keep_the_indent() {
        // A 4-space-indented 20-char line wrapped at 10 columns.
        let m = DisplayMap::new(1, &folds(&[]), &[], &[20], 10, &[], &[4]);
        assert_eq!(m.rows(), 3); // 10 + 6 + 4
        assert_eq!(m.row_at(1).indent, 4);
        assert_eq!((m.row_at(1).start, m.row_at(1).end), (10, 16));
        // Column 12 sits on row 1, drawn at x-col 4 + (12 - 10).
        assert_eq!(m.place(0, 12), (1, 6));
    }

    fn inl(v: &[(usize, usize, usize)]) -> std::rc::Rc<Inlays> {
        let mut m = Inlays::new();
        for &(l, c, w) in v {
            m.entry(l).or_default().push((c, w));
        }
        std::rc::Rc::new(m)
    }

    #[test]
    fn inlays_push_columns_right_caret_before_char_after() {
        // `let x = 1;` with a 5-col hint `: i32` at col 5 (after `x`).
        let ix = std::rc::Rc::new(crate::lines::LineIndex::build("let x = 1;\nnext"));
        let m = DisplayMap::for_index_inlays(&ix, &folds(&[]), 0, &[], inl(&[(0, 5, 5)]));
        assert_eq!(m.place(0, 4), (0, 4));
        // The caret at the hint column sits before the hint...
        assert_eq!(m.place(0, 5), (0, 5));
        // ...the char there is drawn after it, and everything later shifts by its width.
        assert_eq!(m.place_char(0, 5), (0, 10));
        assert_eq!(m.place(0, 6), (0, 11));
        assert_eq!(m.place(0, 10), (0, 15));
        // Other lines are untouched.
        assert_eq!(m.place(1, 2), (1, 2));
    }

    #[test]
    fn inlay_hit_testing_maps_around_hints() {
        let ix = std::rc::Rc::new(crate::lines::LineIndex::build("let x = 1;"));
        let m = DisplayMap::for_index_inlays(&ix, &folds(&[]), 0, &[], inl(&[(0, 5, 5)]));
        let len = |_| 10;
        assert_eq!(m.hit(0, 3.0, len), (0, 3));
        // Clicks on the hint snap to its nearer side.
        assert_eq!(m.hit(0, 6.0, len), (0, 5));
        assert_eq!(m.hit(0, 9.6, len), (0, 6));
        // Past the hint: shifted back by its width.
        assert_eq!(m.hit(0, 13.0, len), (0, 8));
        assert_eq!(m.hit(0, 99.0, len), (0, 10));
    }

    #[test]
    fn inlays_count_toward_soft_wrap() {
        // 12 chars, wrap at 10, a 4-col hint at col 2: visual width 16 -> wraps at col 6.
        let m = DisplayMap::new_with_inlays(
            1,
            &folds(&[]),
            &[],
            &[12],
            10,
            &[],
            &[],
            inl(&[(0, 2, 4)]),
        );
        assert_eq!(m.rows(), 2);
        assert_eq!((m.row_at(0).start, m.row_at(0).end), (0, 6));
        assert_eq!(m.place(0, 6), (1, 0));
        assert_eq!(m.place(0, 3), (0, 7));
        // Without the hint the same line wraps at col 10.
        let plain = DisplayMap::new(1, &folds(&[]), &[], &[12], 10, &[], &[]);
        assert_eq!(plain.row_at(0).end, 10);
    }
}
