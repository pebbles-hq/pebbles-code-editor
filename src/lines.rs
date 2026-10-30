//! A maintained per-line index of the document: line-start table, per-line char lengths,
//! per-line indentation, the indentation-derived foldable regions and the widest line.
//!
//! The view used to re-split the whole document on every rebuild (line count, char lengths,
//! `fold::foldable`, the widest line) and every geometry query (`line_of`, `byte_at`, ...)
//! scanned from the document start, so each keystroke cost O(document). [`LineCache::sync`]
//! instead diffs the new text against the previous one (common prefix / suffix, compared in
//! `memcmp`-sized chunks), re-scans only the lines the edit touched and shifts the rest by the
//! byte delta. Foldable regions are recomputed only when an edit changes a line's indentation
//! or blankness (or the line count) — typing inside a line leaves them untouched.
//!
//! Every query mirrors the semantics of the scanning functions in [`crate::geometry`] and
//! [`crate::fold::foldable`] exactly; the tests compare against them for random edits.

use std::rc::Rc;

/// Indentation of a line in the `fold::foldable` sense: `None` for a blank line, else the
/// byte length of its leading whitespace.
fn indent_of(l: &str) -> Option<usize> {
    if l.trim().is_empty() {
        None
    } else {
        Some(l.len() - l.trim_start().len())
    }
}

/// A folding-region marker line (VS Code `#region` / `#endregion` in any comment syntax):
/// `1` opens a region, `-1` closes one, `0` otherwise.
pub(crate) fn marker_of(l: &str) -> i8 {
    let mut t = l.trim_start();
    for p in ["<!--", "//", "/*", "--", "#", ";", "'", "%"] {
        if let Some(r) = t.strip_prefix(p) {
            t = r.trim_start();
            break;
        }
    }
    let t = t.strip_prefix('#').unwrap_or(t);
    let word_end = |r: &str| r.chars().next().is_none_or(|c| !c.is_alphanumeric());
    if let Some(r) = t.strip_prefix("region")
        && word_end(r)
    {
        1
    } else if let Some(r) = t.strip_prefix("endregion")
        && word_end(r)
    {
        -1
    } else {
        0
    }
}

/// Indentation regions (see below) plus `#region`/`#endregion` marker regions, sorted by
/// head; a marker region replaces an indentation region with the same head.
fn regions_with_markers(indents: &[Option<usize>], markers: &[i8]) -> Vec<(usize, usize)> {
    let mut out = regions_from(indents);
    let mut stack = Vec::new();
    let mut marked = Vec::new();
    for (i, &m) in markers.iter().enumerate() {
        if m > 0 {
            stack.push(i);
        } else if m < 0
            && let Some(h) = stack.pop()
            && i > h
        {
            marked.push((h, i));
        }
    }
    if marked.is_empty() {
        return out;
    }
    out.retain(|r| !marked.iter().any(|m| m.0 == r.0));
    out.extend(marked);
    out.sort_unstable();
    out
}

/// The full foldable-region computation for `src` (the reference `fold::foldable` uses).
pub(crate) fn regions_of(src: &str) -> Vec<(usize, usize)> {
    let (mut ind, mut mk) = (Vec::new(), Vec::new());
    for l in src.split('\n') {
        ind.push(indent_of(l));
        mk.push(marker_of(l));
    }
    regions_with_markers(&ind, &mk)
}

/// The foldable `(head, last)` regions from per-line indentation, in one stack pass.
/// Identical to `fold::foldable`: a non-blank line heads a region spanning every following
/// line up to (not including) the next non-blank line indented no deeper, trimmed to its last
/// non-blank line; regions are sorted by head.
fn regions_from(indents: &[Option<usize>]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new(); // (line, indent)
    let mut last_nonblank = 0usize;
    for (j, ind) in indents.iter().enumerate() {
        let Some(d) = *ind else { continue };
        while let Some(&(i, di)) = stack.last() {
            if di < d {
                break;
            }
            stack.pop();
            if last_nonblank > i {
                out.push((i, last_nonblank));
            }
        }
        stack.push((j, d));
        last_nonblank = j;
    }
    for &(i, _) in &stack {
        if last_nonblank > i {
            out.push((i, last_nonblank));
        }
    }
    out.sort_unstable();
    out
}

/// Length of the common prefix of `a` and `b` in bytes (chunked slice compares).
pub(crate) fn common_prefix(a: &[u8], b: &[u8]) -> usize {
    const C: usize = 256;
    let n = a.len().min(b.len());
    let mut i = 0;
    while i + C <= n && a[i..i + C] == b[i..i + C] {
        i += C;
    }
    while i < n && a[i] == b[i] {
        i += 1;
    }
    i
}

/// Length of the common suffix of `a` and `b`, at most `max` bytes.
pub(crate) fn common_suffix(a: &[u8], b: &[u8], max: usize) -> usize {
    const C: usize = 256;
    let (la, lb) = (a.len(), b.len());
    let mut i = 0;
    while i + C <= max && a[la - i - C..la - i] == b[lb - i - C..lb - i] {
        i += C;
    }
    while i < max && a[la - i - 1] == b[lb - i - 1] {
        i += 1;
    }
    i
}

/// Immutable snapshot of the per-line facts for one document revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LineIndex {
    /// Byte offset of each line's start (`starts[0] == 0`); one entry per `split('\n')` line.
    starts: Vec<usize>,
    char_lens: Vec<usize>,
    indents: Vec<Option<usize>>,
    markers: Vec<i8>,
    regions: Rc<Vec<(usize, usize)>>,
    max_cols: usize,
    len: usize,
}

impl LineIndex {
    /// Build from scratch (the reference full computation).
    pub(crate) fn build(src: &str) -> Self {
        let mut starts = vec![0];
        let mut char_lens = Vec::new();
        let mut indents = Vec::new();
        let mut markers = Vec::new();
        for l in src.split('\n') {
            char_lens.push(l.chars().count());
            indents.push(indent_of(l));
            markers.push(marker_of(l));
        }
        starts.extend(src.match_indices('\n').map(|(i, _)| i + 1));
        let regions = Rc::new(regions_with_markers(&indents, &markers));
        let max_cols = char_lens.iter().copied().max().unwrap_or(0);
        LineIndex {
            starts,
            char_lens,
            indents,
            markers,
            regions,
            max_cols,
            len: src.len(),
        }
    }

    /// Number of lines (`src.split('\n').count()`, so at least 1).
    pub(crate) fn line_count(&self) -> usize {
        self.starts.len()
    }
    /// Per-line char lengths.
    pub(crate) fn char_lens(&self) -> &[usize] {
        &self.char_lens
    }
    /// The foldable regions (same as `fold::foldable`).
    pub(crate) fn regions(&self) -> &Rc<Vec<(usize, usize)>> {
        &self.regions
    }
    /// Whether `line` heads a foldable region.
    pub(crate) fn is_fold_head(&self, line: usize) -> bool {
        self.regions.binary_search_by_key(&line, |r| r.0).is_ok()
    }
    /// The widest line, in chars.
    pub(crate) fn max_cols(&self) -> usize {
        self.max_cols
    }

    /// `geometry::line_of`.
    pub(crate) fn line_of(&self, src: &str, byte: usize) -> usize {
        let b = byte.min(self.len);
        if !src.is_char_boundary(b) {
            return 0;
        }
        self.starts.partition_point(|&s| s <= b) - 1
    }
    /// `geometry::line_start_of`.
    pub(crate) fn line_start_of(&self, line: usize) -> usize {
        self.starts.get(line).copied().unwrap_or(self.len)
    }
    /// End byte of `line` (its `\n`, or the document end).
    fn line_end_of(&self, line: usize) -> usize {
        self.starts.get(line + 1).map(|s| s - 1).unwrap_or(self.len)
    }
    /// `geometry::col_of`.
    pub(crate) fn col_of(&self, src: &str, byte: usize) -> usize {
        let b = byte.min(self.len);
        if !src.is_char_boundary(b) {
            return 0;
        }
        let ls = self.line_start_of(self.line_of(src, b));
        src.get(ls..b).map(|s| s.chars().count()).unwrap_or(0)
    }
    /// `geometry::line_char_len`.
    pub(crate) fn line_char_len(&self, line: usize) -> usize {
        self.char_lens.get(line).copied().unwrap_or(0)
    }
    /// `geometry::byte_at`.
    pub(crate) fn byte_at(&self, src: &str, line: usize, col: usize) -> usize {
        let start = self.line_start_of(line);
        let end = if line < self.starts.len() {
            self.line_end_of(line)
        } else {
            self.len
        };
        match src[start..end].char_indices().nth(col) {
            Some((i, _)) => start + i,
            None => end,
        }
    }
}

/// The previous text + its index; [`LineCache::sync`] updates both by the edit delta.
pub(crate) struct LineCache {
    text: String,
    index: Rc<LineIndex>,
}

impl Default for LineCache {
    fn default() -> Self {
        LineCache {
            text: String::new(),
            index: Rc::new(LineIndex::build("")),
        }
    }
}

impl LineCache {
    /// Bring the index up to date with `new` and return it. Unchanged text is free (one
    /// compare); otherwise only the lines between the first and last differing byte are
    /// re-scanned.
    pub(crate) fn sync(&mut self, new: &str) -> Rc<LineIndex> {
        if self.text == new {
            return self.index.clone();
        }
        let old = std::mem::take(&mut self.text);
        let (ob, nb) = (old.as_bytes(), new.as_bytes());
        let p = common_prefix(ob, nb);
        let s = common_suffix(ob, nb, ob.len().min(nb.len()) - p);
        let delta = nb.len() as isize - ob.len() as isize;
        let idx = Rc::make_mut(&mut self.index);
        // Old lines a..=b are replaced; line a's start is before the edit, so it is unchanged.
        let a = idx.starts.partition_point(|&x| x <= p) - 1;
        let b = idx.starts.partition_point(|&x| x <= ob.len() - s) - 1;
        let start = idx.starts[a];
        // The replaced span in the new text: from line a's start to the end of the line that
        // holds the end of the changed bytes.
        let ne = nb.len() - s;
        // (`ne` may sit inside a multi-byte char, so search bytes, not the `str`.)
        let end = nb[ne..]
            .iter()
            .position(|&c| c == b'\n')
            .map(|i| ne + i)
            .unwrap_or(nb.len());
        let seg = &new[start..end];
        let mut starts = Vec::new();
        let mut lens = Vec::new();
        let mut inds = Vec::new();
        let mut marks = Vec::new();
        for l in seg.split('\n') {
            lens.push(l.chars().count());
            inds.push(indent_of(l));
            marks.push(marker_of(l));
        }
        starts.push(start);
        starts.extend(seg.match_indices('\n').map(|(i, _)| start + i + 1));
        let removed_max = idx.char_lens[a..=b].iter().copied().max().unwrap_or(0);
        let added_max = lens.iter().copied().max().unwrap_or(0);
        let folds_changed = inds.len() != b + 1 - a
            || idx.indents[a..=b] != inds[..]
            || idx.markers[a..=b] != marks[..];
        idx.starts.splice(a..=b, starts);
        for x in &mut idx.starts[a + lens.len()..] {
            *x = (*x as isize + delta) as usize;
        }
        idx.char_lens.splice(a..=b, lens);
        idx.indents.splice(a..=b, inds);
        idx.markers.splice(a..=b, marks);
        idx.len = nb.len();
        if added_max >= idx.max_cols {
            idx.max_cols = added_max;
        } else if removed_max == idx.max_cols {
            idx.max_cols = idx.char_lens.iter().copied().max().unwrap_or(0);
        }
        if folds_changed {
            idx.regions = Rc::new(regions_with_markers(&idx.indents, &idx.markers));
        }
        self.text = new.to_string();
        self.index.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry;

    /// Tiny deterministic PRNG (xorshift) so the random-edit tests need no dependency.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n.max(1) as u64) as usize
        }
    }

    fn boundary(s: &str, mut b: usize) -> usize {
        b = b.min(s.len());
        while !s.is_char_boundary(b) {
            b -= 1;
        }
        b
    }

    fn check(src: &str, idx: &LineIndex) {
        assert_eq!(
            *idx,
            LineIndex::build(src),
            "incremental != full for {src:?}"
        );
        assert_eq!(idx.line_count(), src.split('\n').count());
        assert_eq!(*idx.regions().as_ref(), crate::fold::foldable(src));
        let lens: Vec<usize> = src.split('\n').map(|l| l.chars().count()).collect();
        assert_eq!(idx.char_lens(), &lens[..]);
        assert_eq!(idx.max_cols(), lens.iter().copied().max().unwrap_or(0));
        let mut bytes: Vec<usize> = (0..=src.len())
            .filter(|&b| src.is_char_boundary(b))
            .collect();
        bytes.push(src.len() + 3);
        for b in bytes {
            assert_eq!(
                idx.line_of(src, b),
                geometry::line_of(src, b),
                "line_of {b}"
            );
            assert_eq!(idx.col_of(src, b), geometry::col_of(src, b), "col_of {b}");
        }
        for line in 0..idx.line_count() + 2 {
            assert_eq!(idx.line_start_of(line), geometry::line_start_of(src, line));
            assert_eq!(idx.line_char_len(line), geometry::line_char_len(src, line));
            for col in [0, 1, 3, 7, 40] {
                assert_eq!(
                    idx.byte_at(src, line, col),
                    geometry::byte_at(src, line, col),
                    "byte_at {line},{col}"
                );
            }
        }
    }

    const PIECES: &[&str] = &[
        "a",
        "x",
        "\n",
        "\n    ",
        "    ",
        "\t",
        " ",
        "}",
        "fn f() {\n    y;\n}",
        "é",
        "\n\n",
        "  \n",
        "日本",
    ];

    #[test]
    fn regions_match_foldable() {
        for src in [
            "",
            "a",
            "fn a() {\n    x;\n    y;\n}\ntop\n",
            "a\n  b\n\n    c\n  d\ne\n\n",
            "  a\nb\n    c\n      d\n\n  e",
            "\n\n  x\n\n",
        ] {
            assert_eq!(
                LineIndex::build(src).regions().as_ref(),
                &crate::fold::foldable(src)
            );
        }
    }

    #[test]
    fn random_edits_match_full_recomputation() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        for round in 0..40 {
            let mut src: String = (0..rng.below(12))
                .map(|_| PIECES[rng.below(PIECES.len())])
                .collect();
            let mut cache = LineCache::default();
            check(&src, &cache.sync(&src));
            for _ in 0..60 {
                let at = boundary(&src, rng.below(src.len() + 1));
                match rng.below(4) {
                    // insert (letters, newlines, indentation, whole blocks)
                    0 | 1 => src.insert_str(at, PIECES[rng.below(PIECES.len())]),
                    // delete a range
                    2 => {
                        let to = boundary(&src, at + rng.below(8));
                        src.replace_range(at.min(to)..to.max(at), "");
                    }
                    // replace a range
                    _ => {
                        let to = boundary(&src, at + rng.below(5));
                        src.replace_range(at..to.max(at), PIECES[rng.below(PIECES.len())]);
                    }
                }
                let idx = cache.sync(&src);
                check(&src, &idx);
            }
            let _ = round;
        }
    }

    #[test]
    fn typing_inside_a_line_keeps_regions_shared() {
        let mut cache = LineCache::default();
        let a = cache.sync("fn a() {\n    x;\n}");
        let b = cache.sync("fn a() {\n    xy;\n}");
        assert!(Rc::ptr_eq(a.regions(), b.regions()));
        let c = cache.sync("fn a() {\n\n    xy;\n}");
        assert_eq!(
            c.regions().as_ref(),
            &crate::fold::foldable("fn a() {\n\n    xy;\n}")
        );
    }
}
