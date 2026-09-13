//! Code folding: the pure model behind the fold gutter. `foldable` derives collapsible
//! regions from indentation (language-agnostic); [`DisplayMap`] turns the set of folded head
//! lines into the buffer-line ↔ display-row mapping the view renders through, so folded lines
//! collapse to nothing and everything below shifts up.

use std::collections::BTreeSet;

/// The indentation-derived foldable regions as `(head_line, last_line)` pairs: `head_line`
/// stays visible (it gets a fold arrow); `head_line+1 ..= last_line` are the collapsible body.
/// Regions may nest. Blank lines don't break a region and never head one.
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

/// The buffer-line → display-row mapping for a set of folded head lines.
pub(crate) struct DisplayMap {
    /// `row_of[line]` = the display row a buffer line paints on (a hidden line shares its fold
    /// head's row).
    pub(crate) row_of: Vec<usize>,
    /// Visible buffer lines, in display order (`visible[row]` = buffer line for that row).
    pub(crate) visible: Vec<usize>,
    /// True for lines collapsed inside a folded region.
    hidden: Vec<bool>,
}

impl DisplayMap {
    /// Build the map for `line_count` lines given the `folded` head set and the `regions`.
    pub(crate) fn new(line_count: usize, folded: &BTreeSet<usize>, regions: &[(usize, usize)]) -> Self {
        let mut hidden = vec![false; line_count];
        for &(head, last) in regions {
            if folded.contains(&head) {
                let end = last.min(line_count.saturating_sub(1));
                for h in hidden.iter_mut().take(end + 1).skip(head + 1) {
                    *h = true;
                }
            }
        }
        let mut row_of = vec![0usize; line_count];
        let mut visible = Vec::new();
        for line in 0..line_count {
            if hidden[line] {
                row_of[line] = visible.len().saturating_sub(1);
            } else {
                row_of[line] = visible.len();
                visible.push(line);
            }
        }
        DisplayMap { row_of, visible, hidden }
    }

    /// Number of display rows (visible lines).
    pub(crate) fn rows(&self) -> usize {
        self.visible.len()
    }

    /// Whether `line` is collapsed inside a fold.
    pub(crate) fn is_hidden(&self, line: usize) -> bool {
        self.hidden.get(line).copied().unwrap_or(false)
    }

    /// The buffer line shown at display `row` (clamped).
    pub(crate) fn line_at_row(&self, row: usize) -> usize {
        let row = row.min(self.visible.len().saturating_sub(1));
        self.visible.get(row).copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folds(heads: &[usize]) -> BTreeSet<usize> {
        heads.iter().copied().collect()
    }

    #[test]
    fn foldable_finds_indented_blocks() {
        let src = "fn a() {\n    x;\n    y;\n}\ntop\n";
        let r = foldable(src);
        assert!(r.contains(&(0, 2)), "fn body folds lines 1..=2, got {r:?}");
    }

    #[test]
    fn display_map_collapses_folded_lines() {
        // 5 lines; fold head 0 covers 1..=2.
        let regions = vec![(0usize, 2usize)];
        let m = DisplayMap::new(5, &folds(&[0]), &regions);
        assert_eq!(m.rows(), 3, "lines 1,2 hidden → 3 visible rows");
        assert!(m.is_hidden(1) && m.is_hidden(2));
        assert!(!m.is_hidden(0) && !m.is_hidden(3));
        // Rows: 0->line0, 1->line3, 2->line4.
        assert_eq!(m.line_at_row(1), 3);
        assert_eq!(m.row_of[3], 1);
    }

    #[test]
    fn unfolded_map_is_identity() {
        let m = DisplayMap::new(4, &folds(&[]), &[(0, 1)]);
        assert_eq!(m.rows(), 4);
        assert_eq!(m.row_of, vec![0, 1, 2, 3]);
    }
}
