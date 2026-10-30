//! Pure editing operations behind the editor's command list ([`crate::actions`]): line
//! operations, multi-cursor commands, comments, case transforms, bracket navigation, subword
//! motions, smart paste, indentation detection.
//!
//! Every function here is a pure function of the source text and the current
//! [`Selections`] → a [`Transaction`] (or `None` when the command does not apply). No signals,
//! no widgets; the view dispatches the result through the single mutation path, so every
//! operation is one undo step and works for every cursor.

use crate::edit::{Change, ChangeSet, Selection, Selections, Transaction};
use crate::geometry::*;

/// The language/indent knobs operations need.
#[derive(Clone, Debug)]
pub(crate) struct OpCfg {
    /// One indent level (`"    "` or `"\t"`).
    pub(crate) tab: String,
    /// Visual width of a tab stop.
    pub(crate) tab_size: usize,
    pub(crate) line_comment: Option<String>,
    pub(crate) block_comment: Option<(String, String)>,
    pub(crate) brackets: Vec<(char, char)>,
}

impl Default for OpCfg {
    fn default() -> Self {
        OpCfg {
            tab: "    ".into(),
            tab_size: 4,
            line_comment: Some("//".into()),
            block_comment: Some(("/*".into(), "*/".into())),
            brackets: crate::brackets::DEFAULT_BRACKETS.to_vec(),
        }
    }
}

// ---------------------------------------------------------------------------
// line blocks
// ---------------------------------------------------------------------------

/// The `(first_line, last_line)` block a selection covers. A multi-line selection ending at
/// column 0 does not include that last line (VS Code semantics).
fn block_of(src: &str, r: &Selection) -> (usize, usize) {
    let l0 = line_of(src, r.min());
    let mut l1 = line_of(src, r.max());
    if l1 > l0 && line_start(src, r.max()) == r.max() {
        l1 -= 1;
    }
    (l0, l1)
}

/// Every selection's line block, sorted and merged (`adjacent` also merges blocks that
/// touch, as moving lines needs).
fn blocks(src: &str, sels: &Selections, adjacent: bool) -> Vec<(usize, usize)> {
    let mut v: Vec<(usize, usize)> = sels.ranges().iter().map(|r| block_of(src, r)).collect();
    v.sort_unstable();
    let mut out: Vec<(usize, usize)> = Vec::with_capacity(v.len());
    for (a, b) in v {
        match out.last_mut() {
            Some(last) if a <= last.1 + usize::from(adjacent) => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

fn line_count(src: &str) -> usize {
    src.bytes().filter(|&b| b == b'\n').count() + 1
}

/// Byte range of lines `a..=b` (without the trailing newline).
fn lines_range(src: &str, a: usize, b: usize) -> (usize, usize) {
    let s = line_start_of(src, a);
    let e = line_end(src, line_start_of(src, b));
    (s, e)
}

/// Shift every selection inside `[lo, hi]` by `delta` (others unchanged).
fn shift_sels(sels: &Selections, moves: &[(usize, usize, isize)]) -> Selections {
    let shift = |p: usize| -> usize {
        for &(lo, hi, d) in moves {
            if p >= lo && p <= hi {
                return (p as isize + d).max(0) as usize;
            }
        }
        p
    };
    let ranges = sels
        .ranges()
        .iter()
        .map(|r| Selection::range(shift(r.anchor), shift(r.head)))
        .collect();
    Selections::new(ranges, sels.primary_index())
}

/// Move the selected lines up (`up`) or down one line (Alt+Up / Alt+Down).
pub(crate) fn move_lines(src: &str, sels: &Selections, up: bool) -> Option<Transaction> {
    let n = line_count(src);
    let bl = blocks(src, sels, true);
    if bl
        .iter()
        .any(|&(a, b)| (up && a == 0) || (!up && b + 1 >= n))
    {
        return None;
    }
    let mut changes = Vec::new();
    let mut moves = Vec::new();
    // Positions are mapped against the PRE-edit text; deltas accumulate per block.
    for &(a, b) in &bl {
        let (bs, be) = lines_range(src, a, b);
        let block = &src[bs..be];
        if up {
            let (os, oe) = lines_range(src, a - 1, a - 1);
            let other = &src[os..oe];
            changes.push(Change {
                from: os,
                to: be,
                insert: format!("{block}\n{other}"),
            });
            moves.push((bs, be, -((oe - os + 1) as isize)));
        } else {
            let (os, oe) = lines_range(src, b + 1, b + 1);
            let other = &src[os..oe];
            changes.push(Change {
                from: bs,
                to: oe,
                insert: format!("{other}\n{block}"),
            });
            moves.push((bs, be, (oe - os + 1) as isize));
        }
    }
    Some(Transaction::change_and_select(
        ChangeSet::from_changes(changes),
        shift_sels(sels, &moves),
    ))
}

/// Duplicate the selected lines above (`up`, the cursor stays on the upper copy) or below
/// (the cursor moves to the lower copy) — Shift+Alt+Up / Shift+Alt+Down.
pub(crate) fn copy_lines(src: &str, sels: &Selections, up: bool) -> Option<Transaction> {
    let bl = blocks(src, sels, false);
    let mut changes = Vec::new();
    let mut moves = Vec::new();
    let mut acc: isize = 0;
    for &(a, b) in &bl {
        let (bs, be) = lines_range(src, a, b);
        let block = &src[bs..be];
        let add = (block.len() + 1) as isize;
        if up {
            changes.push(Change {
                from: bs,
                to: bs,
                insert: format!("{block}\n"),
            });
            moves.push((bs, be, acc));
        } else {
            changes.push(Change {
                from: be,
                to: be,
                insert: format!("\n{block}"),
            });
            moves.push((bs, be, acc + add));
        }
        acc += add;
    }
    // Selections outside any block shift by the insertions before them.
    let ranges: Vec<Selection> = sels
        .ranges()
        .iter()
        .map(|r| {
            let f = |p: usize| -> usize {
                let mut d: isize = 0;
                for (i, &(a, b)) in bl.iter().enumerate() {
                    let (bs, be) = lines_range(src, a, b);
                    let add = (be - bs + 1) as isize;
                    if p >= bs && p <= be {
                        return (p as isize + moves[i].2).max(0) as usize;
                    }
                    if p > be {
                        d += add;
                    }
                }
                (p as isize + d) as usize
            };
            Selection::range(f(r.anchor), f(r.head))
        })
        .collect();
    Some(Transaction::change_and_select(
        ChangeSet::from_changes(changes),
        Selections::new(ranges, sels.primary_index()),
    ))
}

/// Delete the selected lines entirely (Ctrl+Shift+K). Carets land at the same column on the
/// line that moves up into place.
pub(crate) fn delete_lines(src: &str, sels: &Selections) -> Option<Transaction> {
    let bl = blocks(src, sels, false);
    let n = line_count(src);
    let mut changes = Vec::new();
    let mut carets = Vec::new();
    let mut removed: usize = 0;
    for &(a, b) in &bl {
        let (bs, be) = lines_range(src, a, b);
        let (from, to) = if b + 1 < n {
            (bs, be + 1)
        } else if a > 0 {
            (bs - 1, be)
        } else {
            (bs, be)
        };
        let col = sels
            .ranges()
            .iter()
            .find(|r| block_of(src, r).0 == a)
            .map_or(0, |r| col_of(src, r.head));
        changes.push(Change {
            from,
            to,
            insert: String::new(),
        });
        // Caret position in the NEW text: the line now at `from`.
        let new_from = from - removed;
        removed += to - from;
        carets.push((new_from, col, b + 1 >= n && a > 0));
    }
    let cs = ChangeSet::from_changes(changes);
    let mut doc = ropey::Rope::from_str(src);
    cs.apply(&mut doc);
    let new = doc.to_string();
    let sel: Vec<Selection> = carets
        .into_iter()
        .map(|(at, col, last)| {
            let at = at.min(new.len());
            let line = line_of(&new, if last { line_start(&new, at) } else { at });
            Selection::caret(byte_at(&new, line, col))
        })
        .collect();
    Some(Transaction::change_and_select(cs, Selections::new(sel, 0)))
}

/// Join each selection's lines (or the caret line with the next) into one, replacing each
/// line break + surrounding indentation with a single space (Ctrl+J).
pub(crate) fn join_lines(src: &str, sels: &Selections) -> Option<Transaction> {
    let n = line_count(src);
    let mut changes = Vec::new();
    let mut carets = Vec::new();
    let mut delta: isize = 0;
    for &(a, b) in &blocks(src, sels, false) {
        let last = if a == b { (a + 1).min(n - 1) } else { b };
        if last == a {
            continue;
        }
        let mut caret = 0;
        for l in a..last {
            let le = line_end(src, line_start_of(src, l));
            let trimmed_end = src[..le]
                .trim_end_matches([' ', '\t'])
                .len()
                .max(line_start_of(src, l));
            let next_ls = le + 1;
            let next_content = next_ls
                + (src[next_ls..line_end(src, next_ls)].len()
                    - src[next_ls..line_end(src, next_ls)].trim_start().len());
            let next_empty = next_content == line_end(src, next_ls);
            let this_empty = trimmed_end == line_start_of(src, l);
            let ins = if next_empty || this_empty { "" } else { " " };
            changes.push(Change {
                from: trimmed_end,
                to: next_content,
                insert: ins.to_string(),
            });
            caret = (trimmed_end as isize + delta) as usize;
            delta += ins.len() as isize - (next_content - trimmed_end) as isize;
        }
        carets.push(Selection::caret(caret));
    }
    if changes.is_empty() {
        return None;
    }
    Some(Transaction::change_and_select(
        ChangeSet::from_changes(changes),
        Selections::new(carets, 0),
    ))
}

/// Duplicate each selection right after itself (an empty selection duplicates its line).
pub(crate) fn duplicate_selection(src: &str, sels: &Selections) -> Option<Transaction> {
    if sels.ranges().iter().all(Selection::is_empty) {
        return copy_lines(src, sels, false);
    }
    let mut changes = Vec::new();
    let mut out = Vec::new();
    let mut delta: isize = 0;
    for r in sels.ranges() {
        let text = &src[r.min()..r.max()];
        changes.push(Change {
            from: r.max(),
            to: r.max(),
            insert: text.to_string(),
        });
        let s = (r.max() as isize + delta) as usize;
        out.push(Selection::range(s, s + text.len()));
        delta += text.len() as isize;
    }
    Some(Transaction::change_and_select(
        ChangeSet::from_changes(changes),
        Selections::new(out, sels.primary_index()),
    ))
}

/// Sort the selected lines (the whole document when only a caret) ascending or descending.
pub(crate) fn sort_lines(src: &str, sels: &Selections, descending: bool) -> Option<Transaction> {
    let bl: Vec<(usize, usize)> = if sels.is_empty() {
        vec![(0, line_count(src) - 1)]
    } else {
        blocks(src, sels, false)
    };
    let mut changes = Vec::new();
    for (a, b) in bl {
        if a == b {
            continue;
        }
        let (s, e) = lines_range(src, a, b);
        let mut lines: Vec<&str> = src[s..e].split('\n').collect();
        lines.sort();
        if descending {
            lines.reverse();
        }
        let joined = lines.join("\n");
        if joined != src[s..e] {
            changes.push(Change {
                from: s,
                to: e,
                insert: joined,
            });
        }
    }
    if changes.is_empty() {
        return None;
    }
    let cs = ChangeSet::from_changes(changes);
    let sel = sels.mapped(&cs);
    Some(Transaction::change_and_select(cs, sel))
}

/// Swap the characters around each caret (transposeLetters). At line end, swaps the two
/// characters before the caret.
pub(crate) fn transpose(src: &str, sels: &Selections) -> Option<Transaction> {
    let mut changes = Vec::new();
    let mut carets = Vec::new();
    for r in sels.ranges() {
        if !r.is_empty() {
            continue;
        }
        let mut at = r.head;
        let le = line_end(src, at);
        if at == le {
            at = prev_char(src, at);
        }
        let ls = line_start(src, at);
        if at <= ls || at >= le {
            continue;
        }
        let p = prev_char(src, at);
        let nx = next_char(src, at);
        let a = &src[p..at];
        let b = &src[at..nx];
        changes.push(Change {
            from: p,
            to: nx,
            insert: format!("{b}{a}"),
        });
        carets.push(Selection::caret(nx));
    }
    if changes.is_empty() {
        return None;
    }
    Some(Transaction::change_and_select(
        ChangeSet::from_changes(changes),
        Selections::new(carets, 0),
    ))
}

/// Leading whitespace of the line containing `byte`.
fn indent_of(src: &str, byte: usize) -> &str {
    let ls = line_start(src, byte);
    let le = line_end(src, ls);
    let line = &src[ls..le];
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/// Insert an empty line below (Ctrl+Enter) or above (Ctrl+Shift+Enter) each caret's line,
/// keeping its indentation, and move the caret there.
pub(crate) fn insert_line(src: &str, sels: &Selections, above: bool) -> Option<Transaction> {
    let mut changes = Vec::new();
    let mut carets = Vec::new();
    let mut delta: isize = 0;
    for &(a, b) in &blocks(src, sels, false) {
        let (s, e) = lines_range(src, a, b);
        let indent = indent_of(src, if above { s } else { line_start_of(src, b) }).to_string();
        if above {
            let ins = format!("{indent}\n");
            changes.push(Change {
                from: s,
                to: s,
                insert: ins.clone(),
            });
            carets.push(Selection::caret(
                (s as isize + delta) as usize + indent.len(),
            ));
            delta += ins.len() as isize;
        } else {
            let ins = format!("\n{indent}");
            changes.push(Change {
                from: e,
                to: e,
                insert: ins.clone(),
            });
            carets.push(Selection::caret((e as isize + delta) as usize + ins.len()));
            delta += ins.len() as isize;
        }
    }
    Some(Transaction::change_and_select(
        ChangeSet::from_changes(changes),
        Selections::new(carets, 0),
    ))
}

/// Indent (`outdent == false`) or outdent every selected line by one level (Ctrl+] / Ctrl+[),
/// whatever the selection shape.
pub(crate) fn indent_lines(
    src: &str,
    sels: &Selections,
    cfg: &OpCfg,
    outdent: bool,
) -> Option<Transaction> {
    let mut changes = Vec::new();
    for &(a, b) in &blocks(src, sels, false) {
        for l in a..=b {
            let ls = line_start_of(src, l);
            let le = line_end(src, ls);
            if outdent {
                let line = &src[ls..le];
                let n = if line.starts_with('\t') {
                    1
                } else {
                    line.bytes()
                        .take(cfg.tab_size.max(1))
                        .take_while(|&c| c == b' ')
                        .count()
                };
                if n > 0 {
                    changes.push(Change {
                        from: ls,
                        to: ls + n,
                        insert: String::new(),
                    });
                }
            } else if le > ls {
                changes.push(Change {
                    from: ls,
                    to: ls,
                    insert: cfg.tab.clone(),
                });
            }
        }
    }
    if changes.is_empty() {
        return None;
    }
    let cs = ChangeSet::from_changes(changes);
    let sel = sels.mapped(&cs);
    Some(Transaction::change_and_select(cs, sel))
}

// ---------------------------------------------------------------------------
// comments
// ---------------------------------------------------------------------------

/// Toggle a block comment around each selection (or the caret's line content) —
/// Shift+Alt+A. Unwraps when the selection (trimmed) is already wrapped.
pub(crate) fn toggle_block_comment(
    src: &str,
    sels: &Selections,
    cfg: &OpCfg,
) -> Option<Transaction> {
    let (open, close) = cfg.block_comment.clone()?;
    let mut changes = Vec::new();
    for r in sels.ranges() {
        let (mut s, mut e) = (r.min(), r.max());
        if s == e {
            let ls = line_start(src, s);
            let le = line_end(src, s);
            s = ls + (src[ls..le].len() - src[ls..le].trim_start().len());
            e = ls + src[ls..le].trim_end().len();
        }
        let inner = &src[s..e];
        let t = inner.trim();
        let lead = inner.len() - inner.trim_start().len();
        if t.starts_with(open.as_str())
            && t.ends_with(close.as_str())
            && t.len() >= open.len() + close.len()
        {
            let os = s + lead;
            let ce = os + t.len();
            let mut ol = open.len();
            if src[os + ol..].starts_with(' ') {
                ol += 1;
            }
            let mut cl = close.len();
            if ce >= cl + 1 && src[..ce - cl].ends_with(' ') && ce - cl - 1 >= os + ol {
                cl += 1;
            }
            changes.push(Change {
                from: os,
                to: os + ol,
                insert: String::new(),
            });
            changes.push(Change {
                from: ce - cl,
                to: ce,
                insert: String::new(),
            });
        } else {
            changes.push(Change {
                from: s,
                to: s,
                insert: format!("{open} "),
            });
            changes.push(Change {
                from: e,
                to: e,
                insert: format!(" {close}"),
            });
        }
    }
    let cs = ChangeSet::from_changes(changes);
    let sel = sels.mapped(&cs);
    Some(Transaction::change_and_select(cs, sel))
}

// ---------------------------------------------------------------------------
// case transforms + whitespace
// ---------------------------------------------------------------------------

/// Case transforms (editor.action.transformTo*).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Case {
    Upper,
    Lower,
    Title,
    Snake,
    Kebab,
    Camel,
}

fn words_of(s: &str) -> Vec<String> {
    // Split on non-alphanumerics and camelCase humps.
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut prev_lower = false;
    for ch in s.chars() {
        if !ch.is_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            prev_lower = false;
            continue;
        }
        if ch.is_uppercase() && prev_lower && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        prev_lower = ch.is_lowercase() || ch.is_ascii_digit();
        cur.push(ch);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn transform(s: &str, case: Case) -> String {
    match case {
        Case::Upper => s.to_uppercase(),
        Case::Lower => s.to_lowercase(),
        Case::Title => {
            let mut out = String::with_capacity(s.len());
            let mut start = true;
            for ch in s.chars() {
                if ch.is_alphanumeric() {
                    if start {
                        out.extend(ch.to_uppercase());
                    } else {
                        out.extend(ch.to_lowercase());
                    }
                    start = false;
                } else {
                    out.push(ch);
                    start = true;
                }
            }
            out
        }
        Case::Snake | Case::Kebab => {
            let sep = if case == Case::Snake { "_" } else { "-" };
            words_of(s)
                .iter()
                .map(|w| w.to_lowercase())
                .collect::<Vec<_>>()
                .join(sep)
        }
        Case::Camel => {
            let mut out = String::new();
            for (i, w) in words_of(s).iter().enumerate() {
                let lw = w.to_lowercase();
                if i == 0 {
                    out.push_str(&lw);
                } else {
                    let mut cs = lw.chars();
                    if let Some(f) = cs.next() {
                        out.extend(f.to_uppercase());
                        out.push_str(cs.as_str());
                    }
                }
            }
            out
        }
    }
}

/// Transform each selection's case (a caret transforms the word under it).
pub(crate) fn transform_case(src: &str, sels: &Selections, case: Case) -> Option<Transaction> {
    let mut changes = Vec::new();
    let mut out = Vec::new();
    let mut delta: isize = 0;
    for r in sels.ranges() {
        let (s, e) = if r.is_empty() {
            word_at(src, r.head)
        } else {
            (r.min(), r.max())
        };
        if s >= e {
            out.push(*r);
            continue;
        }
        let t = transform(&src[s..e], case);
        let ns = (s as isize + delta) as usize;
        out.push(if r.is_empty() {
            Selection::caret(ns + (r.head - s).min(t.len()))
        } else {
            Selection::range(ns, ns + t.len())
        });
        delta += t.len() as isize - (e - s) as isize;
        changes.push(Change {
            from: s,
            to: e,
            insert: t,
        });
    }
    if changes.is_empty() {
        return None;
    }
    Some(Transaction::change_and_select(
        ChangeSet::from_changes(changes),
        Selections::new(out, sels.primary_index()),
    ))
}

/// Remove trailing whitespace from every line (editor.action.trimTrailingWhitespace /
/// `files.trimTrailingWhitespace` on save).
pub(crate) fn trim_trailing_whitespace(src: &str, sels: &Selections) -> Option<Transaction> {
    let mut changes = Vec::new();
    let mut at = 0;
    for line in src.split('\n') {
        let t = line.trim_end_matches([' ', '\t', '\r']).len();
        // Keep a lone `\r` (CRLF files): only spaces/tabs before it are trailing whitespace.
        let keep_cr = line.ends_with('\r');
        let end = if keep_cr { line.len() - 1 } else { line.len() };
        if t < end {
            changes.push(Change {
                from: at + t,
                to: at + end,
                insert: String::new(),
            });
        }
        at += line.len() + 1;
    }
    if changes.is_empty() {
        return None;
    }
    let cs = ChangeSet::from_changes(changes);
    let sel = sels.mapped(&cs);
    Some(Transaction::change_and_select(cs, sel))
}

/// Ensure the document ends with exactly one newline (`files.insertFinalNewline`).
pub(crate) fn insert_final_newline(src: &str, sels: &Selections) -> Option<Transaction> {
    if src.is_empty() || src.ends_with('\n') {
        return None;
    }
    let nl = if src.contains("\r\n") { "\r\n" } else { "\n" };
    Some(Transaction::change_and_select(
        ChangeSet::insert(src.len(), nl),
        sels.clone(),
    ))
}

/// Line-ending style of a document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding {
    /// `\n`
    Lf,
    /// `\r\n`
    CrLf,
}

/// The dominant line ending of `src` (LF when there are none).
pub fn detect_line_ending(src: &str) -> LineEnding {
    let crlf = src.matches("\r\n").count();
    let lf = src.matches('\n').count() - crlf;
    if crlf > lf {
        LineEnding::CrLf
    } else {
        LineEnding::Lf
    }
}

/// Convert every line ending to `to`.
pub(crate) fn convert_line_endings(src: &str, to: LineEnding) -> Option<Transaction> {
    let normalized = src.replace("\r\n", "\n");
    let new = match to {
        LineEnding::Lf => normalized,
        LineEnding::CrLf => normalized.replace('\n', "\r\n"),
    };
    if new == src {
        return None;
    }
    Some(Transaction::change_and_select(
        ChangeSet::replace(0, src.len(), new),
        Selections::single(Selection::caret(0)),
    ))
}

// ---------------------------------------------------------------------------
// multi-cursor + selection
// ---------------------------------------------------------------------------

/// Add a caret one line above/below every cursor (Ctrl+Alt+Up / Ctrl+Alt+Down), at the same
/// column (clamped to the line).
pub(crate) fn add_cursor_vertical(src: &str, sels: &Selections, down: bool) -> Option<Transaction> {
    let n = line_count(src);
    let mut ranges = sels.ranges().to_vec();
    let mut primary_new = None;
    let pr = sels.primary();
    for r in sels.ranges() {
        let l = line_of(src, r.head);
        let target = if down {
            if l + 1 >= n {
                continue;
            }
            l + 1
        } else {
            if l == 0 {
                continue;
            }
            l - 1
        };
        let c = Selection::caret(byte_at(src, target, col_of(src, r.head)));
        if *r == pr {
            primary_new = Some(c);
        }
        ranges.push(c);
    }
    let p = primary_new?;
    let idx = ranges.len() - 1;
    let mut sel = Selections::new(ranges, idx);
    // Make the newly added caret (from the primary) the primary.
    if let Some(i) = sel.ranges().iter().position(|r| r.head == p.head) {
        sel = Selections::new(sel.ranges().to_vec(), i);
    }
    Some(Transaction::select(sel))
}

/// The needle for occurrence commands: the primary selection's text, or the word under the
/// caret (then matched as a whole word).
fn occurrence_needle(src: &str, sels: &Selections) -> Option<(String, bool)> {
    let p = sels.primary();
    if p.is_empty() {
        let (s, e) = word_at(src, p.head);
        (e > s).then(|| (src[s..e].to_string(), true))
    } else {
        Some((src[p.min()..p.max()].to_string(), false))
    }
}

/// Select every occurrence of the selection / word under the caret (Ctrl+Shift+L).
pub(crate) fn select_all_occurrences(src: &str, sels: &Selections) -> Option<Transaction> {
    let (needle, whole) = occurrence_needle(src, sels)?;
    let opts = crate::search::Options {
        case_sensitive: true,
        whole_word: whole,
        regex: false,
    };
    let found = crate::search::find_all(src, &needle, &opts);
    if found.is_empty() {
        return None;
    }
    let ph = sels.primary().head;
    let primary = found
        .iter()
        .position(|&(s, e)| s <= ph && ph <= e)
        .unwrap_or(0);
    let ranges = found
        .into_iter()
        .map(|(s, e)| Selection::range(s, e))
        .collect();
    Some(Transaction::select(Selections::new(ranges, primary)))
}

/// Place a caret at the end of every line each selection covers (Shift+Alt+I).
pub(crate) fn cursors_at_line_ends(src: &str, sels: &Selections) -> Option<Transaction> {
    let mut out = Vec::new();
    for r in sels.ranges() {
        let (a, b) = (line_of(src, r.min()), line_of(src, r.max()));
        if a == b {
            out.push(Selection::caret(r.max()));
            continue;
        }
        for l in a..=b {
            let le = if l == b {
                r.max()
            } else {
                line_end(src, line_start_of(src, l))
            };
            out.push(Selection::caret(le));
        }
    }
    let n = out.len();
    Some(Transaction::select(Selections::new(out, n - 1)))
}

/// Extend the selection to whole lines, adding the next line on repeat (Ctrl+L).
pub(crate) fn expand_line_selection(src: &str, sels: &Selections) -> Option<Transaction> {
    let ranges = sels
        .ranges()
        .iter()
        .map(|r| {
            let s = line_start(src, r.min());
            let full_already = !r.is_empty()
                && r.min() == s
                && (r.max() == src.len() || line_start(src, r.max()) == r.max());
            let end_from = if full_already { r.max() } else { r.max() };
            let le = line_end(src, if full_already { end_from } else { r.max() });
            let e = if le < src.len() { le + 1 } else { le };
            Selection::range(s, e)
        })
        .collect();
    Some(Transaction::select(Selections::new(
        ranges,
        sels.primary_index(),
    )))
}

/// Keyboard column (box) selection: grow the box whose fixed corner is the anchor of the
/// first cursor (Ctrl+Shift+Alt+arrows). `dl`/`dc` move the moving corner by lines/columns.
pub(crate) fn column_select(
    src: &str,
    sels: &Selections,
    dl: isize,
    dc: isize,
) -> Option<Transaction> {
    let rs = sels.ranges();
    let pi = sels.primary_index();
    let first = rs.first()?;
    let last = rs.last()?;
    // The fixed corner is at the end opposite the primary.
    let fixed = if pi == 0 && rs.len() > 1 {
        last.anchor
    } else {
        first.anchor
    };
    let moving = sels.primary().head;
    let (al, ac) = (line_of(src, fixed), col_of(src, fixed));
    let (ml, mc) = (line_of(src, moving), col_of(src, moving));
    let n = line_count(src) as isize;
    let nl = (ml as isize + dl).clamp(0, n - 1) as usize;
    let nc = (mc as isize + dc).max(0) as usize;
    let mut out = Vec::new();
    let (lo, hi) = (al.min(nl), al.max(nl));
    for l in lo..=hi {
        out.push(Selection::range(byte_at(src, l, ac), byte_at(src, l, nc)));
    }
    let primary = if nl >= al { out.len() - 1 } else { 0 };
    Some(Transaction::select(Selections::new(out, primary)))
}

// ---------------------------------------------------------------------------
// brackets
// ---------------------------------------------------------------------------

/// The innermost bracket pair enclosing `pos` (opener byte, closer byte).
pub(crate) fn enclosing_pair(
    src: &str,
    brackets: &[(char, char)],
    pos: usize,
) -> Option<(usize, usize)> {
    let mut stack: Vec<(char, usize)> = Vec::new();
    let mut best = None;
    for (i, ch) in src.char_indices() {
        if brackets.iter().any(|&(o, _)| o == ch) {
            stack.push((ch, i));
        } else if let Some(&(o, _)) = brackets.iter().find(|&&(_, c)| c == ch) {
            if let Some(idx) = stack.iter().rposition(|&(x, _)| x == o) {
                let (_, open) = stack[idx];
                stack.truncate(idx);
                if open < pos && pos <= i {
                    // Innermost = the latest-closing smallest; the first hit at depth is the
                    // innermost because inner pairs close first.
                    if best.is_none_or(|(bo, _): (usize, usize)| open > bo) {
                        best = Some((open, i));
                    }
                }
            }
        }
        if i > pos && stack.is_empty() && best.is_some() {
            break;
        }
    }
    best
}

/// Jump to the matching bracket (Ctrl+Shift+\): from a bracket to its partner, else to the
/// closer of the enclosing pair.
pub(crate) fn jump_to_bracket(src: &str, sels: &Selections, cfg: &OpCfg) -> Option<Transaction> {
    let ranges: Vec<Selection> = sels
        .ranges()
        .iter()
        .map(|r| {
            let h = r.head;
            let target =
                if let Some((b, m)) = crate::brackets::find_bracket_match(src, &cfg.brackets, h) {
                    // Land on the far side the way VS Code does: before an opener, after a closer.
                    if m > b { m + 1 } else { m }
                } else if let Some((_, c)) = enclosing_pair(src, &cfg.brackets, h) {
                    c
                } else {
                    h
                };
            Selection::caret(target.min(src.len()))
        })
        .collect();
    Some(Transaction::select(Selections::new(
        ranges,
        sels.primary_index(),
    )))
}

/// Select to the enclosing brackets, including them (editor.action.selectToBracket).
pub(crate) fn select_to_bracket(src: &str, sels: &Selections, cfg: &OpCfg) -> Option<Transaction> {
    let ranges: Vec<Selection> = sels
        .ranges()
        .iter()
        .map(|r| match enclosing_pair(src, &cfg.brackets, r.min()) {
            Some((o, c)) => Selection::range(o, next_char(src, c)),
            None => *r,
        })
        .collect();
    Some(Transaction::select(Selections::new(
        ranges,
        sels.primary_index(),
    )))
}

// ---------------------------------------------------------------------------
// subword + smart home
// ---------------------------------------------------------------------------

/// Character class for subword segmentation.
fn class(ch: char) -> u8 {
    if ch.is_whitespace() {
        0
    } else if ch.is_uppercase() {
        1
    } else if ch.is_lowercase() {
        2
    } else if ch.is_ascii_digit() {
        3
    } else if ch == '_' || ch == '-' {
        4
    } else {
        5
    }
}

/// The next subword boundary after `pos` (camelCase humps, `snake_case` parts, digits).
pub(crate) fn next_subword(src: &str, pos: usize) -> usize {
    let mut it = src[pos.min(src.len())..].char_indices().peekable();
    let base = pos.min(src.len());
    // Skip separators/whitespace first.
    while let Some(&(_, ch)) = it.peek() {
        if class(ch) == 0 || class(ch) == 4 {
            it.next();
        } else {
            break;
        }
    }
    let Some((i0, c0)) = it.next() else {
        return src.len();
    };
    let k0 = class(c0);
    let mut end = base + i0 + c0.len_utf8();
    let mut prev = k0;
    let mut upper_run = usize::from(k0 == 1);
    while let Some(&(i, ch)) = it.peek() {
        let k = class(ch);
        let cont = match (prev, k) {
            (1, 2) => upper_run <= 1, // `Foo`: hump continues into lowercase
            (1, 1) => true,
            (2, 2) | (3, 3) | (5, 5) => true,
            _ => false,
        };
        if !cont {
            // `HTMLParser`: stop before the last capital of an upper run followed by lower.
            if prev == 1 && k == 2 && upper_run > 1 {
                return base + i
                    - src[..base + i]
                        .chars()
                        .next_back()
                        .map_or(0, char::len_utf8);
            }
            return base + i;
        }
        if k == 1 {
            upper_run += 1;
        }
        prev = k;
        end = base + i + ch.len_utf8();
        it.next();
    }
    end
}

/// The previous subword boundary before `pos`.
pub(crate) fn prev_subword(src: &str, pos: usize) -> usize {
    let pos = pos.min(src.len());
    let chars: Vec<(usize, char)> = src[..pos].char_indices().collect();
    let mut i = chars.len();
    while i > 0 && matches!(class(chars[i - 1].1), 0 | 4) {
        i -= 1;
    }
    if i == 0 {
        return 0;
    }
    let k = class(chars[i - 1].1);
    i -= 1;
    match k {
        2 => {
            while i > 0 && class(chars[i - 1].1) == 2 {
                i -= 1;
            }
            if i > 0 && class(chars[i - 1].1) == 1 {
                i -= 1;
            }
        }
        1 => {
            while i > 0 && class(chars[i - 1].1) == 1 {
                i -= 1;
            }
        }
        _ => {
            while i > 0 && class(chars[i - 1].1) == k {
                i -= 1;
            }
        }
    }
    chars[i].0
}

/// Smart Home: the first non-whitespace column of the line, or the line start when already
/// there.
pub(crate) fn smart_home(src: &str, pos: usize) -> usize {
    let ls = line_start(src, pos);
    let first = ls + indent_of(src, pos).len();
    if pos == first { ls } else { first }
}

// ---------------------------------------------------------------------------
// paste + indentation
// ---------------------------------------------------------------------------

/// Re-indent a multi-line paste so its lines keep their relative indentation under
/// `target` (the caret line's indentation). The first line is inserted as-is at the caret.
pub(crate) fn reindent_paste(text: &str, target: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    if lines.len() < 2 {
        return text.to_string();
    }
    let lead = |l: &str| l.len() - l.trim_start_matches([' ', '\t']).len();
    let min = lines
        .iter()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|l| lead(l))
        .min()
        .unwrap_or(0);
    let mut out = String::with_capacity(text.len() + lines.len() * target.len());
    for (i, l) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
            if !l.trim().is_empty() {
                out.push_str(target);
                out.push_str(&l[min.min(lead(l))..]);
            }
        } else {
            out.push_str(l);
        }
    }
    out
}

/// Detect a file's indentation (VS Code `editor.detectIndentation`): `(insert_spaces,
/// size)`, or `None` when the file has no indented lines.
pub fn detect_indentation(src: &str) -> Option<(bool, usize)> {
    let (mut tabs, mut spaces) = (0usize, 0usize);
    let mut widths = [0usize; 9];
    let mut prev = 0usize;
    for line in src.lines().take(10_000) {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with('\t') {
            tabs += 1;
            prev = 0;
            continue;
        }
        let n = line.len() - line.trim_start_matches(' ').len();
        if n > 0 {
            spaces += 1;
        }
        let d = n.abs_diff(prev);
        if (2..=8).contains(&d) {
            widths[d] += 1;
        }
        prev = n;
    }
    if tabs == 0 && spaces == 0 {
        return None;
    }
    if tabs > spaces {
        return Some((false, 4));
    }
    let size = [2usize, 4, 8, 3, 5, 6, 7]
        .into_iter()
        .max_by_key(|&w| (widths[w], usize::from(w == 4)))
        .unwrap_or(4);
    Some((true, if widths[size] == 0 { 4 } else { size }))
}

/// Re-indent the document's leading whitespace from one unit to another
/// (editor.action.indentationToSpaces / indentationToTabs).
pub(crate) fn convert_indentation(
    src: &str,
    to_spaces: bool,
    tab_size: usize,
) -> Option<Transaction> {
    let mut changes = Vec::new();
    let mut at = 0;
    for line in src.split('\n') {
        let lead_len = line.len() - line.trim_start_matches([' ', '\t']).len();
        let lead = &line[..lead_len];
        let width: usize = lead.chars().fold(0, |w, c| {
            if c == '\t' {
                (w / tab_size + 1) * tab_size
            } else {
                w + 1
            }
        });
        let new = if to_spaces {
            " ".repeat(width)
        } else {
            format!(
                "{}{}",
                "\t".repeat(width / tab_size.max(1)),
                " ".repeat(width % tab_size.max(1))
            )
        };
        if new != lead {
            changes.push(Change {
                from: at,
                to: at + lead_len,
                insert: new,
            });
        }
        at += line.len() + 1;
    }
    if changes.is_empty() {
        return None;
    }
    let cs = ChangeSet::from_changes(changes);
    Some(Transaction::change(cs))
}

/// Add (`Some(true)`), remove (`Some(false)`) or toggle (`None`) line comments on the
/// selected lines (editor.action.addCommentLine / removeCommentLine / commentLine).
pub(crate) fn line_comment(
    src: &str,
    sels: &Selections,
    prefix: &str,
    mode: Option<bool>,
) -> Option<Transaction> {
    let mut lines: Vec<usize> = Vec::new();
    for (a, b) in blocks(src, sels, false) {
        lines.extend(a..=b);
    }
    let content = |l: usize| {
        let ls = line_start_of(src, l);
        (ls, &src[ls..line_end(src, ls)])
    };
    let non_blank: Vec<usize> = lines
        .into_iter()
        .filter(|&l| !content(l).1.trim().is_empty())
        .collect();
    if non_blank.is_empty() {
        return None;
    }
    let all_commented = non_blank
        .iter()
        .all(|&l| content(l).1.trim_start().starts_with(prefix));
    let add = mode.unwrap_or(!all_commented);
    let mut changes = Vec::new();
    if add {
        let col = non_blank
            .iter()
            .map(|&l| {
                let c = content(l).1;
                c.len() - c.trim_start().len()
            })
            .min()
            .unwrap_or(0);
        for &l in &non_blank {
            let at = content(l).0 + col;
            changes.push(Change {
                from: at,
                to: at,
                insert: format!("{prefix} "),
            });
        }
    } else {
        for &l in &non_blank {
            let (ls, c) = content(l);
            let lead = c.len() - c.trim_start().len();
            if !c.trim_start().starts_with(prefix) {
                continue;
            }
            let at = ls + lead;
            let mut rm = prefix.len();
            if src[at + rm..].starts_with(' ') {
                rm += 1;
            }
            changes.push(Change {
                from: at,
                to: at + rm,
                insert: String::new(),
            });
        }
    }
    if changes.is_empty() {
        return None;
    }
    let cs = ChangeSet::from_changes(changes);
    let sel = sels.mapped(&cs);
    Some(Transaction::change_and_select(cs, sel))
}

/// The structural fallback smart-select ladder for `[from, to)` when no parser is
/// available: word → inside the enclosing brackets → including them (repeating outward) →
/// line → document. Innermost first.
pub(crate) fn fallback_ladder(
    src: &str,
    brackets: &[(char, char)],
    from: usize,
    to: usize,
) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let (ws, we) = word_at(src, from);
    if ws <= from && to <= we && we > ws {
        out.push((ws, we));
    }
    let mut pos = from;
    while let Some((o, c)) = enclosing_pair(src, brackets, pos) {
        out.push((o + 1, c));
        out.push((o, c + 1));
        if o == 0 {
            break;
        }
        pos = o;
    }
    out.push((line_start(src, from), line_end(src, to)));
    out.push((0, src.len()));
    out.sort_by_key(|&(a, b)| b - a);
    out.dedup();
    out.retain(|&(a, b)| a <= from && to <= b);
    out
}

/// The next ladder rung strictly larger than `[from, to)`.
pub(crate) fn expand_to(
    ladder: &[(usize, usize)],
    from: usize,
    to: usize,
) -> Option<(usize, usize)> {
    ladder
        .iter()
        .copied()
        .filter(|&(a, b)| a <= from && to <= b && (a, b) != (from, to))
        .min_by_key(|&(a, b)| b - a)
}

/// Index into `matches` of the next (`forward`) or previous match relative to `caret`,
/// wrapping.
pub(crate) fn next_match(matches: &[(usize, usize)], caret: usize, forward: bool) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    Some(if forward {
        matches.iter().position(|&(s, _)| s >= caret).unwrap_or(0)
    } else {
        matches
            .iter()
            .rposition(|&(_, e)| e < caret)
            .unwrap_or(matches.len() - 1)
    })
}

/// The first line of each contiguous change hunk of a [`LineDiff`](crate::diff::LineDiff)
/// (added/modified lines, or the line a deletion sits before).
pub(crate) fn change_hunks(d: &crate::diff::LineDiff) -> Vec<usize> {
    let last = d.added.len().saturating_sub(1);
    d.hunks.iter().map(|h| h.cur.start.min(last)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::EditorState;

    /// Apply `tx` to `src` with selections `sels`; return (text, ranges as (anchor, head)).
    fn run(
        src: &str,
        sels: Selections,
        f: impl Fn(&str, &Selections) -> Option<Transaction>,
    ) -> (String, Vec<(usize, usize)>) {
        let st = EditorState {
            doc: ropey::Rope::from_str(src),
            selection: sels.clone(),
        };
        match f(src, &sels) {
            Some(tx) => {
                let (next, _) = st.apply(&tx);
                (
                    next.text(),
                    next.selection
                        .ranges()
                        .iter()
                        .map(|r| (r.anchor, r.head))
                        .collect(),
                )
            }
            None => (
                src.to_string(),
                sels.ranges().iter().map(|r| (r.anchor, r.head)).collect(),
            ),
        }
    }
    fn caret(p: usize) -> Selections {
        Selections::single(Selection::caret(p))
    }

    #[test]
    fn move_lines_up_and_down() {
        let (t, s) = run("a\nbb\nc", caret(3), |s, x| move_lines(s, x, true));
        assert_eq!(t, "bb\na\nc");
        assert_eq!(s, vec![(1, 1)]);
        let (t, s) = run("a\nbb\nc", caret(3), |s, x| move_lines(s, x, false));
        assert_eq!(t, "a\nc\nbb");
        assert_eq!(s, vec![(5, 5)]);
        // Top line can't move up.
        let (t, _) = run("a\nb", caret(0), |s, x| move_lines(s, x, true));
        assert_eq!(t, "a\nb");
        // A two-line selection moves as a block.
        let (t, _) = run(
            "x\na\nb\ny",
            Selections::single(Selection::range(2, 5)),
            |s, x| move_lines(s, x, false),
        );
        assert_eq!(t, "x\ny\na\nb");
    }

    #[test]
    fn copy_lines_up_and_down() {
        let (t, s) = run("a\nb", caret(0), |s, x| copy_lines(s, x, false));
        assert_eq!(t, "a\na\nb");
        assert_eq!(s, vec![(2, 2)]);
        let (t, s) = run("a\nb", caret(2), |s, x| copy_lines(s, x, true));
        assert_eq!(t, "a\nb\nb");
        assert_eq!(s, vec![(2, 2)]);
    }

    #[test]
    fn delete_lines_keeps_column() {
        let (t, s) = run("abc\ndef\nghi", caret(5), delete_lines);
        assert_eq!(t, "abc\nghi");
        assert_eq!(s, vec![(5, 5)]);
        let (t, _) = run("abc\ndef", caret(5), delete_lines);
        assert_eq!(t, "abc");
        let (t, _) = run("only", caret(1), delete_lines);
        assert_eq!(t, "");
    }

    #[test]
    fn join_lines_collapses_indentation() {
        let (t, s) = run("a  \n    b\nc", caret(0), join_lines);
        assert_eq!(t, "a b\nc");
        assert_eq!(s, vec![(1, 1)]);
        let (t, _) = run(
            "a\n b\n c",
            Selections::single(Selection::range(0, 6)),
            join_lines,
        );
        assert_eq!(t, "a b c");
    }

    #[test]
    fn duplicate_sort_transpose() {
        let (t, s) = run(
            "ab",
            Selections::single(Selection::range(0, 2)),
            duplicate_selection,
        );
        assert_eq!((t.as_str(), s), ("abab", vec![(2, 4)]));
        let (t, _) = run(
            "c\na\nb",
            Selections::single(Selection::range(0, 5)),
            |s, x| sort_lines(s, x, false),
        );
        assert_eq!(t, "a\nb\nc");
        let (t, _) = run(
            "c\na\nb",
            Selections::single(Selection::range(0, 5)),
            |s, x| sort_lines(s, x, true),
        );
        assert_eq!(t, "c\nb\na");
        let (t, s) = run("abc", caret(1), transpose);
        assert_eq!((t.as_str(), s), ("bac", vec![(2, 2)]));
        let (t, _) = run("abc", caret(3), transpose);
        assert_eq!(t, "acb");
    }

    #[test]
    fn insert_line_above_and_below() {
        let (t, s) = run("  ab\ncd", caret(3), |s, x| insert_line(s, x, false));
        assert_eq!((t.as_str(), s), ("  ab\n  \ncd", vec![(7, 7)]));
        let (t, s) = run("  ab", caret(3), |s, x| insert_line(s, x, true));
        assert_eq!((t.as_str(), s), ("  \n  ab", vec![(2, 2)]));
    }

    #[test]
    fn indent_and_outdent_lines() {
        let cfg = OpCfg::default();
        let (t, _) = run(
            "a\nb",
            Selections::single(Selection::range(0, 3)),
            |s, x| indent_lines(s, x, &cfg, false),
        );
        assert_eq!(t, "    a\n    b");
        let (t, _) = run("      a", caret(7), |s, x| indent_lines(s, x, &cfg, true));
        assert_eq!(t, "  a");
    }

    #[test]
    fn block_comment_wraps_and_unwraps() {
        let cfg = OpCfg::default();
        let (t, _) = run(
            "let x = 1;",
            Selections::single(Selection::range(4, 5)),
            |s, x| toggle_block_comment(s, x, &cfg),
        );
        assert_eq!(t, "let /* x */ = 1;");
        let (t, _) = run("  foo();", caret(3), |s, x| {
            toggle_block_comment(s, x, &cfg)
        });
        assert_eq!(t, "  /* foo(); */");
        let (t2, _) = run(&t, caret(5), |s, x| toggle_block_comment(s, x, &cfg));
        assert_eq!(t2, "  foo();");
    }

    #[test]
    fn case_transforms() {
        let sel = Selections::single(Selection::range(0, 11));
        assert_eq!(
            run("hello world", sel.clone(), |s, x| transform_case(
                s,
                x,
                Case::Upper
            ))
            .0,
            "HELLO WORLD"
        );
        assert_eq!(
            run("hello world", sel.clone(), |s, x| transform_case(
                s,
                x,
                Case::Title
            ))
            .0,
            "Hello World"
        );
        assert_eq!(
            run("helloWorld!", sel.clone(), |s, x| transform_case(
                s,
                x,
                Case::Snake
            ))
            .0,
            "hello_world"
        );
        assert_eq!(
            run("hello_world", sel.clone(), |s, x| transform_case(
                s,
                x,
                Case::Camel
            ))
            .0,
            "helloWorld"
        );
        assert_eq!(
            run("hello_world", sel, |s, x| transform_case(s, x, Case::Kebab)).0,
            "hello-world"
        );
        // Caret: transforms the word under it.
        assert_eq!(
            run("abc def", caret(1), |s, x| transform_case(
                s,
                x,
                Case::Upper
            ))
            .0,
            "ABC def"
        );
    }

    #[test]
    fn trim_whitespace_final_newline_and_line_endings() {
        assert_eq!(
            run("a  \nb\t\n", caret(0), trim_trailing_whitespace).0,
            "a\nb\n"
        );
        assert_eq!(
            run("a \r\nb", caret(0), trim_trailing_whitespace).0,
            "a\r\nb"
        );
        assert_eq!(run("a", caret(0), insert_final_newline).0, "a\n");
        assert_eq!(run("a\n", caret(0), insert_final_newline).0, "a\n");
        assert_eq!(detect_line_ending("a\r\nb\r\n"), LineEnding::CrLf);
        assert_eq!(detect_line_ending("a\nb"), LineEnding::Lf);
        assert_eq!(
            run("a\r\nb", caret(0), |s, _| convert_line_endings(
                s,
                LineEnding::Lf
            ))
            .0,
            "a\nb"
        );
        assert_eq!(
            run("a\nb", caret(0), |s, _| convert_line_endings(
                s,
                LineEnding::CrLf
            ))
            .0,
            "a\r\nb"
        );
    }

    #[test]
    fn add_cursor_above_below() {
        let (_, s) = run("abc\ndef\nghi", caret(5), |s, x| {
            add_cursor_vertical(s, x, false)
        });
        assert_eq!(s, vec![(1, 1), (5, 5)]);
        let (_, s) = run("abc\ndef\nghi", caret(5), |s, x| {
            add_cursor_vertical(s, x, true)
        });
        assert_eq!(s, vec![(5, 5), (9, 9)]);
        // Shorter line clamps the column.
        let (_, s) = run("abcdef\nx", caret(5), |s, x| {
            add_cursor_vertical(s, x, true)
        });
        assert_eq!(s, vec![(5, 5), (8, 8)]);
    }

    #[test]
    fn select_all_occurrences_word_and_selection() {
        let (_, s) = run("foo bar foo foobar", caret(1), select_all_occurrences);
        assert_eq!(s, vec![(0, 3), (8, 11)]); // whole-word for a caret
        let (_, s) = run(
            "foo bar foo foobar",
            Selections::single(Selection::range(0, 3)),
            select_all_occurrences,
        );
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn cursors_at_line_ends_and_expand_line() {
        let (_, s) = run(
            "ab\ncd\nef",
            Selections::single(Selection::range(0, 7)),
            cursors_at_line_ends,
        );
        assert_eq!(s, vec![(2, 2), (5, 5), (7, 7)]);
        let (_, s) = run("ab\ncd", caret(1), expand_line_selection);
        assert_eq!(s, vec![(0, 3)]);
    }

    #[test]
    fn column_select_grows_a_box() {
        let src = "abcd\nefgh\nijkl";
        let (_, s) = run(src, caret(1), |s, x| column_select(s, x, 1, 0));
        assert_eq!(s, vec![(1, 1), (6, 6)]);
        let sel = Selections::new(vec![Selection::caret(1), Selection::caret(6)], 1);
        let (_, s) = run(src, sel, |s, x| column_select(s, x, 0, 2));
        assert_eq!(s, vec![(1, 3), (6, 8)]);
    }

    #[test]
    fn bracket_jump_and_select() {
        let cfg = OpCfg::default();
        let src = "f(a, [b], c)";
        let (_, s) = run(src, caret(1), |s, x| jump_to_bracket(s, x, &cfg));
        assert_eq!(s, vec![(12, 12)]);
        let (_, s) = run(src, caret(3), |s, x| jump_to_bracket(s, x, &cfg));
        assert_eq!(s, vec![(11, 11)]);
        let (_, s) = run(src, caret(6), |s, x| select_to_bracket(s, x, &cfg));
        assert_eq!(s, vec![(5, 8)]);
    }

    #[test]
    fn subword_motion() {
        let s = "fooBarBaz_qux HTMLParser x2y";
        assert_eq!(next_subword(s, 0), 3);
        assert_eq!(next_subword(s, 3), 6);
        assert_eq!(next_subword(s, 6), 9);
        assert_eq!(next_subword(s, 9), 13);
        assert_eq!(next_subword(s, 13), 18); // HTML|Parser
        assert_eq!(prev_subword(s, 9), 6);
        assert_eq!(prev_subword(s, 6), 3);
        assert_eq!(prev_subword(s, 3), 0);
        assert_eq!(prev_subword(s, 13), 10);
    }

    #[test]
    fn smart_home_toggles() {
        let s = "    let x";
        assert_eq!(smart_home(s, 7), 4);
        assert_eq!(smart_home(s, 4), 0);
        assert_eq!(smart_home(s, 0), 4);
    }

    #[test]
    fn paste_reindents_relative_to_target() {
        let pasted = "if x {\n        y();\n    }";
        assert_eq!(reindent_paste(pasted, "  "), "if x {\n      y();\n  }");
        assert_eq!(reindent_paste("one line", "    "), "one line");
    }

    #[test]
    fn indentation_detection_and_conversion() {
        assert_eq!(detect_indentation("a\n  b\n    c\n  d\n"), Some((true, 2)));
        assert_eq!(detect_indentation("a\n    b\n        c\n"), Some((true, 4)));
        assert_eq!(detect_indentation("a\n\tb\n\t\tc\n"), Some((false, 4)));
        assert_eq!(detect_indentation("a\nb\n"), None);
        assert_eq!(
            run("\ta\n\t\tb", caret(0), |s, _| convert_indentation(
                s, true, 4
            ))
            .0,
            "    a\n        b"
        );
        assert_eq!(
            run("    a\n      b", caret(0), |s, _| convert_indentation(
                s, false, 4
            ))
            .0,
            "\ta\n\t  b"
        );
    }

    #[test]
    fn multi_cursor_line_ops() {
        // Two carets on separate lines: both lines move down together.
        let sel = Selections::new(vec![Selection::caret(0), Selection::caret(4)], 0);
        let (t, _) = run("a\nb\nc\nd", sel, |s, x| move_lines(s, x, false));
        assert_eq!(t, "b\na\nd\nc");
    }

    #[test]
    fn add_remove_toggle_line_comments() {
        let (t, _) = run(
            "a\n  b",
            Selections::single(Selection::range(0, 5)),
            |s, x| line_comment(s, x, "//", Some(true)),
        );
        assert_eq!(t, "// a\n//   b");
        let (t, _) = run(
            "// a\nb",
            Selections::single(Selection::range(0, 6)),
            |s, x| line_comment(s, x, "//", Some(false)),
        );
        assert_eq!(t, "a\nb");
        let (t, _) = run("// a", caret(0), |s, x| line_comment(s, x, "//", None));
        assert_eq!(t, "a");
    }

    #[test]
    fn smart_select_fallback_ladder() {
        let src = "f(a, [bb], c)\nnext";
        let at = 6; // inside `bb`
        let l = fallback_ladder(src, &OpCfg::default().brackets, at, at);
        assert_eq!(l[0], (6, 8)); // word
        assert_eq!(expand_to(&l, 6, 8), Some((5, 9))); // [bb]
        assert_eq!(expand_to(&l, 5, 9), Some((2, 12))); // inside (...)
        assert_eq!(expand_to(&l, 2, 12), Some((1, 13)));
        assert_eq!(expand_to(&l, 1, 13), Some((0, 13))); // line
        assert_eq!(expand_to(&l, 0, 13), Some((0, src.len())));
    }

    #[test]
    fn next_match_wraps() {
        let m = [(0, 1), (5, 6), (9, 10)];
        assert_eq!(next_match(&m, 2, true), Some(1));
        assert_eq!(next_match(&m, 11, true), Some(0));
        assert_eq!(next_match(&m, 5, false), Some(0));
        assert_eq!(next_match(&m, 0, false), Some(2));
    }

    #[test]
    fn change_hunks_from_line_diff() {
        let d = crate::diff::diff_lines("a\nb\nc\nd\ne", "a\nB\nc\nd\nX\nY");
        let h = change_hunks(&d);
        assert_eq!(h, vec![1, 4]);
    }
}
