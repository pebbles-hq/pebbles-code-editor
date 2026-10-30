//! Line-level diff against a base text — the "dirty diff" behind the change bars (VS Code's
//! git gutter), the inline change bands, next/previous change and hunk revert.
//!
//! Common leading/trailing lines are trimmed first (an edit in a big file diffs only the
//! changed middle), then an LCS aligns the middle. A middle too large for the quadratic table
//! is treated as one replaced block, so memory stays bounded on any input.

use std::ops::Range;

/// What a hunk did, in VS Code's terms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    /// Lines only in the current text (green bar).
    Added,
    /// Lines replaced (blue bar).
    Modified,
    /// Base lines removed (red triangle at the line they sat before).
    Deleted,
}

/// One contiguous change: base lines `base` became current lines `cur` (either may be
/// empty). Line numbers are 0-based.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeHunk {
    /// The replaced base lines.
    pub base: Range<usize>,
    /// The current lines that replaced them (empty for a pure deletion; then `cur.start` is
    /// the line the deletion sits before).
    pub cur: Range<usize>,
}

impl ChangeHunk {
    /// Added / modified / deleted.
    pub fn kind(&self) -> ChangeKind {
        match (self.base.is_empty(), self.cur.is_empty()) {
            (true, _) => ChangeKind::Added,
            (false, true) => ChangeKind::Deleted,
            (false, false) => ChangeKind::Modified,
        }
    }
}

/// The diff of the current buffer against its base.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LineDiff {
    /// Per current line: `true` if it isn't in the base (added or modified).
    pub added: Vec<bool>,
    /// Current line indices that have one or more base lines deleted immediately before them.
    pub removed_before: Vec<usize>,
    /// Whether base lines were removed after the very last current line.
    pub removed_at_end: bool,
    /// The hunks, in order.
    pub hunks: Vec<ChangeHunk>,
}

impl LineDiff {
    /// The change kind shown in the gutter for current line `line`, if any.
    pub(crate) fn kind_at(&self, line: usize) -> Option<ChangeKind> {
        self.hunks.iter().find_map(|h| {
            if h.cur.contains(&line) {
                Some(h.kind())
            } else if h.cur.is_empty() && h.cur.start == line {
                Some(ChangeKind::Deleted)
            } else {
                None
            }
        })
    }
}

/// Above this many cells the middle is not LCS-aligned (treated as one replaced block).
const MAX_CELLS: usize = 4_000_000;

/// Diff `cur` against `base` at line granularity.
pub(crate) fn diff_lines(base: &str, cur: &str) -> LineDiff {
    let b: Vec<&str> = base.split('\n').collect();
    let c: Vec<&str> = cur.split('\n').collect();
    let (n, m) = (b.len(), c.len());
    let mut pre = 0;
    while pre < n && pre < m && b[pre] == c[pre] {
        pre += 1;
    }
    let mut suf = 0;
    while suf < n - pre && suf < m - pre && b[n - 1 - suf] == c[m - 1 - suf] {
        suf += 1;
    }
    let (bm, cm) = (&b[pre..n - suf], &c[pre..m - suf]);
    // Aligned ops over the middle: (base_idx_opt, cur_idx_opt) pairs as Equal / Del / Ins.
    let mut ops: Vec<(Option<usize>, Option<usize>)> = Vec::new();
    if bm.len().saturating_mul(cm.len()) <= MAX_CELLS {
        let (bn, cn) = (bm.len(), cm.len());
        let w = cn + 1;
        let mut lcs = vec![0u32; (bn + 1) * w];
        for i in (0..bn).rev() {
            for j in (0..cn).rev() {
                lcs[i * w + j] = if bm[i] == cm[j] {
                    lcs[(i + 1) * w + j + 1] + 1
                } else {
                    lcs[(i + 1) * w + j].max(lcs[i * w + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < bn || j < cn {
            if i < bn && j < cn && bm[i] == cm[j] {
                ops.push((Some(i), Some(j)));
                i += 1;
                j += 1;
            } else if i < bn && (j >= cn || lcs[(i + 1) * w + j] >= lcs[i * w + j + 1]) {
                ops.push((Some(i), None));
                i += 1;
            } else {
                ops.push((None, Some(j)));
                j += 1;
            }
        }
    } else {
        ops.extend((0..bm.len()).map(|i| (Some(i), None)));
        ops.extend((0..cm.len()).map(|j| (None, Some(j))));
    }
    // Group runs of non-equal ops into hunks.
    let mut hunks = Vec::new();
    let (mut bi, mut ci) = (pre, pre);
    let mut k = 0;
    while k < ops.len() {
        if let (Some(_), Some(_)) = ops[k] {
            bi += 1;
            ci += 1;
            k += 1;
            continue;
        }
        let (b0, c0) = (bi, ci);
        while k < ops.len() && !matches!(ops[k], (Some(_), Some(_))) {
            match ops[k] {
                (Some(_), None) => bi += 1,
                (None, Some(_)) => ci += 1,
                _ => {}
            }
            k += 1;
        }
        hunks.push(ChangeHunk {
            base: b0..bi,
            cur: c0..ci,
        });
    }
    let mut added = vec![false; m];
    let mut removed_before = Vec::new();
    let mut removed_at_end = false;
    for h in &hunks {
        for l in h.cur.clone() {
            added[l] = true;
        }
        if h.cur.is_empty() {
            if h.cur.start >= m {
                removed_at_end = true;
            } else {
                removed_before.push(h.cur.start);
            }
        }
    }
    LineDiff {
        added,
        removed_before,
        removed_at_end,
        hunks,
    }
}

/// The text that reverting `hunk` produces: the current lines `hunk.cur` replaced by the base
/// lines `hunk.base`. Returns `(byte_from, byte_to, insert)` against `cur`.
pub(crate) fn revert_hunk(base: &str, cur: &str, hunk: &ChangeHunk) -> (usize, usize, String) {
    let b: Vec<&str> = base.split('\n').collect();
    let starts: Vec<usize> = std::iter::once(0)
        .chain(cur.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    let base_text = b
        .get(hunk.base.clone())
        .map(|v| v.join("\n"))
        .unwrap_or_default();
    let m = starts.len();
    if hunk.cur.is_empty() {
        // Re-insert the deleted base lines before line `cur.start` (or at the end).
        if hunk.cur.start >= m {
            return (cur.len(), cur.len(), format!("\n{base_text}"));
        }
        let at = starts[hunk.cur.start];
        return (at, at, format!("{base_text}\n"));
    }
    let from = starts[hunk.cur.start];
    let last = hunk.cur.end - 1;
    let to = if last + 1 < m {
        starts[last + 1] - 1
    } else {
        cur.len()
    };
    if hunk.base.is_empty() {
        // Pure addition: remove the lines and one adjoining newline.
        if hunk.cur.end < m {
            return (from, starts[hunk.cur.end], String::new());
        }
        return (from.saturating_sub(1), to, String::new());
    }
    (from, to, base_text)
}

/// Split a line into diff tokens: runs of word chars, runs of whitespace, single others.
fn word_tokens(s: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut it = s.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        let class = |c: char| {
            if c.is_alphanumeric() || c == '_' {
                0
            } else if c.is_whitespace() {
                1
            } else {
                2
            }
        };
        let k = class(c);
        let mut end = i + c.len_utf8();
        if k != 2 {
            while let Some(&(j, d)) = it.peek() {
                if class(d) != k {
                    break;
                }
                end = j + d.len_utf8();
                it.next();
            }
        }
        out.push((i, end));
    }
    out
}

/// Word-level changes between two versions of a line (VS Code's inner-line diff): the byte
/// ranges of `a` that were removed and of `b` that were inserted, from an LCS over word
/// tokens (adjacent ranges merged). Very long lines fall back to one whole-line change.
pub(crate) fn word_changes(a: &str, b: &str) -> (Vec<(usize, usize)>, Vec<(usize, usize)>) {
    let (ta, tb) = (word_tokens(a), word_tokens(b));
    let (n, m) = (ta.len(), tb.len());
    if n * m > 250_000 {
        let r = |s: &str| {
            if s.is_empty() {
                vec![]
            } else {
                vec![(0, s.len())]
            }
        };
        return (r(a), r(b));
    }
    let eq = |i: usize, j: usize| a[ta[i].0..ta[i].1] == b[tb[j].0..tb[j].1];
    let mut dp = vec![0u32; (n + 1) * (m + 1)];
    let w = m + 1;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i * w + j] = if eq(i, j) {
                dp[(i + 1) * w + j + 1] + 1
            } else {
                dp[(i + 1) * w + j].max(dp[i * w + j + 1])
            };
        }
    }
    let (mut rem, mut ins): (Vec<(usize, usize)>, Vec<(usize, usize)>) = (vec![], vec![]);
    let push = |v: &mut Vec<(usize, usize)>, r: (usize, usize)| match v.last_mut() {
        Some(l) if l.1 == r.0 => l.1 = r.1,
        _ => v.push(r),
    };
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && eq(i, j) {
            i += 1;
            j += 1;
        } else if j < m && (i == n || dp[i * w + j + 1] >= dp[(i + 1) * w + j]) {
            push(&mut ins, tb[j]);
            j += 1;
        } else {
            push(&mut rem, ta[i]);
            i += 1;
        }
    }
    (rem, ins)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_added_lines() {
        let d = diff_lines("a\nb\nc", "a\nX\nb\nc");
        assert_eq!(d.added, vec![false, true, false, false]);
        assert!(d.removed_before.is_empty());
        assert_eq!(d.kind_at(1), Some(ChangeKind::Added));
    }

    #[test]
    fn detects_removed_lines() {
        let d = diff_lines("a\nb\nc", "a\nc");
        assert_eq!(d.added, vec![false, false]);
        assert_eq!(d.removed_before, vec![1]);
        assert_eq!(d.kind_at(1), Some(ChangeKind::Deleted));
    }

    #[test]
    fn identical_has_no_changes() {
        let d = diff_lines("a\nb", "a\nb");
        assert_eq!(d.added, vec![false, false]);
        assert!(d.removed_before.is_empty() && !d.removed_at_end && d.hunks.is_empty());
    }

    #[test]
    fn modified_and_trailing_deletion() {
        let d = diff_lines("a\nb\nc\nd", "a\nB\nc");
        assert_eq!(d.kind_at(1), Some(ChangeKind::Modified));
        assert!(d.removed_at_end);
        assert_eq!(d.hunks.len(), 2);
    }

    #[test]
    fn huge_diff_stays_bounded() {
        let base: String = (0..20_000).map(|i| format!("b{i}\n")).collect();
        let cur: String = (0..20_000).map(|i| format!("c{i}\n")).collect();
        let d = diff_lines(&base, &cur);
        assert_eq!(d.hunks.len(), 1);
    }

    #[test]
    fn revert_each_hunk_kind() {
        for (base, cur) in [
            ("a\nb\nc", "a\nX\nc"), // modified
            ("a\nc", "a\nb\nc"),    // added
            ("a\nb\nc", "a\nc"),    // deleted
            ("a\nb", "a"),          // deleted at end
            ("a", "a\nb"),          // added at end
            ("a\nb", "X\nY"),       // all modified
        ] {
            let d = diff_lines(base, cur);
            assert_eq!(d.hunks.len(), 1, "{base:?} → {cur:?}: {:?}", d.hunks);
            let (f, t, ins) = revert_hunk(base, cur, &d.hunks[0]);
            let mut s = cur.to_string();
            s.replace_range(f..t, &ins);
            assert_eq!(s, base, "revert {cur:?}");
        }
    }

    #[test]
    fn word_changes_find_the_changed_words() {
        let (rem, ins) = word_changes("let x = foo(1);", "let y = foo(2);");
        assert_eq!(rem, vec![(4, 5), (12, 13)]);
        assert_eq!(ins, vec![(4, 5), (12, 13)]);
        let (rem, ins) = word_changes("a b", "a new b");
        assert!(rem.is_empty());
        assert_eq!(&"a new b"[ins[0].0..ins[0].1], "new ");
    }
}
