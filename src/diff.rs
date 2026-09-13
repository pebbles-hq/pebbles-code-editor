//! Line-level diff against a base text (the inline "changes" gutter/view). An LCS over lines
//! classifies each CURRENT line as added or unchanged, and records where base lines were
//! removed — the editor paints added lines with a green band + a `+` gutter mark, and shows a
//! red marker where lines were deleted.

/// The diff of the current buffer against its base.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LineDiff {
    /// Per current line: `true` if it isn't in the base (an addition).
    pub added: Vec<bool>,
    /// Current line indices that have one or more base lines deleted immediately before them
    /// (a `-` marker sits at the top of that line). `usize::MAX` here would be end-of-file.
    pub removed_before: Vec<usize>,
    /// Whether base lines were removed after the very last current line.
    pub removed_at_end: bool,
}

/// Diff `cur` against `base` at line granularity via an LCS.
pub(crate) fn diff_lines(base: &str, cur: &str) -> LineDiff {
    let b: Vec<&str> = base.split('\n').collect();
    let c: Vec<&str> = cur.split('\n').collect();
    let (n, m) = (b.len(), c.len());
    // LCS length table.
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if b[i] == c[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    // Walk the table: mark current lines added; count removed base lines between matches.
    let mut added = vec![false; m];
    let mut removed_before = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while j < m {
        if i < n && b[i] == c[j] {
            i += 1;
            j += 1;
        } else if i < n && (j + 1 > m || lcs[i + 1][j] >= lcs[i][j + 1]) {
            // A base line was deleted here (before current line j).
            removed_before.push(j);
            i += 1;
        } else {
            added[j] = true;
            j += 1;
        }
    }
    let removed_at_end = i < n;
    LineDiff {
        added,
        removed_before,
        removed_at_end,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_added_lines() {
        let d = diff_lines("a\nb\nc", "a\nX\nb\nc");
        assert_eq!(d.added, vec![false, true, false, false]);
        assert!(d.removed_before.is_empty());
    }

    #[test]
    fn detects_removed_lines() {
        let d = diff_lines("a\nb\nc", "a\nc");
        // `b` was removed before current line 1 (`c`).
        assert_eq!(d.added, vec![false, false]);
        assert_eq!(d.removed_before, vec![1]);
    }

    #[test]
    fn identical_has_no_changes() {
        let d = diff_lines("a\nb", "a\nb");
        assert_eq!(d.added, vec![false, false]);
        assert!(d.removed_before.is_empty() && !d.removed_at_end);
    }
}
