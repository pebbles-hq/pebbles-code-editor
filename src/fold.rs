//! The display model behind code folding **and** soft wrap: a list of visual [`Row`]s, each a
//! `(buffer line, char-column span)`. A hidden (folded) line contributes zero rows; a wrapped
//! line contributes several. The whole view renders in visual-row space, so folding collapses
//! rows and wrapping splits them — one abstraction for both.

use std::collections::BTreeSet;

/// The indentation-derived foldable regions as `(head_line, last_line)` pairs (see below).
pub(crate) fn foldable(src: &str) -> Vec<(usize, usize)> {
    let lines: Vec<&str> = src.split('\n').collect();
    let indent = |l: &str| l.len() - l.trim_start().len();
    let is_blank = |l: &str| l.trim().is_empty();
    let mut regions = Vec::new();
    for i in 0..lines.len() {
        if is_blank(lines[i]) {
            continue;
        }
        let ind = indent(lines[i]);
        let mut j = i + 1;
        let mut last = i;
        while j < lines.len() {
            if is_blank(lines[j]) {
                j += 1;
                continue;
            }
            if indent(lines[j]) > ind {
                last = j;
                j += 1;
            } else {
                break;
            }
        }
        if last > i {
            regions.push((i, last));
        }
    }
    regions
}

/// One visual row: a slice `[start, end)` (char columns) of buffer `line`. `first` marks the
/// line's first row (which shows the line number + fold arrow in the gutter).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub first: bool,
}

/// Buffer-line ↔ visual-row mapping for a fold set + wrap width.
pub(crate) struct DisplayMap {
    pub(crate) rows: Vec<Row>,
    row0: Vec<usize>, // line -> index of its first row (hidden lines -> the fold head's row)
    hidden: Vec<bool>,
}

impl DisplayMap {
    /// Build the map. `char_lens[line]` is that line's length in chars; `wrap_cols == 0`
    /// disables wrapping (one row per visible line).
    pub(crate) fn new(
        line_count: usize,
        folded: &BTreeSet<usize>,
        regions: &[(usize, usize)],
        char_lens: &[usize],
        wrap_cols: usize,
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
            if wrap_cols == 0 || len <= wrap_cols {
                rows.push(Row { line, start: 0, end: len, first: true });
            } else {
                let mut s = 0;
                let mut first = true;
                while s < len {
                    let e = (s + wrap_cols).min(len);
                    rows.push(Row { line, start: s, end: e, first });
                    s = e;
                    first = false;
                }
            }
        }
        DisplayMap { rows, row0, hidden }
    }

    pub(crate) fn rows(&self) -> usize {
        self.rows.len()
    }

    pub(crate) fn is_hidden(&self, line: usize) -> bool {
        self.hidden.get(line).copied().unwrap_or(false)
    }

    /// The buffer line shown at display `row` (clamped).
    pub(crate) fn line_at_row(&self, row: usize) -> usize {
        let row = row.min(self.rows.len().saturating_sub(1));
        self.rows.get(row).map(|r| r.line).unwrap_or(0)
    }

    /// The visual row at `row` (clamped).
    pub(crate) fn row_at(&self, row: usize) -> Row {
        let row = row.min(self.rows.len().saturating_sub(1));
        self.rows.get(row).copied().unwrap_or(Row { line: 0, start: 0, end: 0, first: true })
    }

    /// Place `(line, col)` → `(display_row, x_col_within_row)`.
    pub(crate) fn place(&self, line: usize, col: usize) -> (usize, usize) {
        let mut i = self.row0.get(line).copied().unwrap_or(0);
        while i + 1 < self.rows.len()
            && self.rows[i + 1].line == line
            && self.rows[i].end <= col
        {
            i += 1;
        }
        let start = self.rows.get(i).map(|r| r.start).unwrap_or(0);
        (i, col.saturating_sub(start))
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
        let m = DisplayMap::new(5, &folds(&[0]), &[(0, 2)], &[8, 6, 6, 1, 0], 0);
        assert_eq!(m.rows(), 3);
        assert!(m.is_hidden(1) && m.is_hidden(2));
        assert_eq!(m.line_at_row(1), 3);
    }

    #[test]
    fn wrapping_splits_a_long_line() {
        // One 25-char line, wrap at 10 → 3 rows.
        let m = DisplayMap::new(1, &folds(&[]), &[], &[25], 10);
        assert_eq!(m.rows(), 3);
        assert_eq!(m.row_at(0), Row { line: 0, start: 0, end: 10, first: true });
        assert_eq!(m.row_at(1), Row { line: 0, start: 10, end: 20, first: false });
        assert_eq!(m.row_at(2), Row { line: 0, start: 20, end: 25, first: false });
        // col 14 lands on row 1, x-col 4.
        assert_eq!(m.place(0, 14), (1, 4));
        assert_eq!(m.place(0, 0), (0, 0));
        assert_eq!(m.place(0, 25), (2, 5));
    }

    #[test]
    fn no_wrap_is_one_row_per_line() {
        let m = DisplayMap::new(3, &folds(&[]), &[], &[4, 4, 4], 0);
        assert_eq!(m.rows(), 3);
        assert_eq!(m.place(2, 3), (2, 3));
    }
}
