//! Buffer geometry: navigation over the document (byte ↔ line/column, word/line motions)
//! and the monospace-grid ↔ pixel mapping.
//!
//! Every function here is a **pure** function of the source text (a `&str` snapshot of the
//! rope) and byte offsets — no editor state, no widgets. All offsets stay on UTF-8 char
//! boundaries, so nothing panics on multibyte input. This is the shared coordinate layer the
//! command engine ([`crate::commands`]) and the view ([`crate::view`]) both build on.

use pebbles::prelude::Offset;

use crate::edit::Selection;

/// The sorted, de-duplicated set of line numbers any selection in `ranges` touches.
pub(crate) fn touched_lines(src: &str, ranges: &[Selection]) -> Vec<usize> {
    let mut lines = std::collections::BTreeSet::new();
    for r in ranges {
        for l in line_of(src, r.min())..=line_of(src, r.max()) {
            lines.insert(l);
        }
    }
    lines.into_iter().collect()
}

// ---------------------------------------------------------------------------
// byte ↔ line / column
// ---------------------------------------------------------------------------

/// The 0-based line index containing `byte`.
pub(crate) fn line_of(src: &str, byte: usize) -> usize {
    src.get(..byte.min(src.len()))
        .map(|s| s.bytes().filter(|b| *b == b'\n').count())
        .unwrap_or(0)
}
/// The byte offset of the start of the line containing `byte`.
pub(crate) fn line_start(src: &str, byte: usize) -> usize {
    src.get(..byte.min(src.len()))
        .and_then(|s| s.rfind('\n'))
        .map(|i| i + 1)
        .unwrap_or(0)
}
/// The byte offset of the end of the line containing `byte` (the `\n`, or the doc end).
pub(crate) fn line_end(src: &str, byte: usize) -> usize {
    let b = byte.min(src.len());
    src[b..].find('\n').map(|i| b + i).unwrap_or(src.len())
}
/// The 0-based column (in chars) of `byte` within its line.
pub(crate) fn col_of(src: &str, byte: usize) -> usize {
    let ls = line_start(src, byte);
    src.get(ls..byte.min(src.len()))
        .map(|s| s.chars().count())
        .unwrap_or(0)
}
/// Length (in chars) of line `line`.
pub(crate) fn line_char_len(src: &str, line: usize) -> usize {
    src.split('\n')
        .nth(line)
        .map(|l| l.chars().count())
        .unwrap_or(0)
}
/// The byte offset at `(line, col)`, clamped to the line's end.
pub(crate) fn byte_at(src: &str, line: usize, col: usize) -> usize {
    let start = line_start_of(src, line);
    let end = line_end(src, start);
    let mut b = start;
    for (i, _) in src[start..end].char_indices().take(col) {
        b = start
            + i
            + src[start + i..]
                .chars()
                .next()
                .map(char::len_utf8)
                .unwrap_or(1);
    }
    b.min(end)
}
/// The byte offset of the start of line `line`.
pub(crate) fn line_start_of(src: &str, line: usize) -> usize {
    if line == 0 {
        return 0;
    }
    let mut count = 0;
    for (i, b) in src.bytes().enumerate() {
        if b == b'\n' {
            count += 1;
            if count == line {
                return i + 1;
            }
        }
    }
    src.len()
}

// ---------------------------------------------------------------------------
// char / word motions
// ---------------------------------------------------------------------------

/// The byte offset one char before `byte`.
pub(crate) fn prev_char(src: &str, byte: usize) -> usize {
    src.get(..byte.min(src.len()))
        .and_then(|s| s.chars().next_back())
        .map(|ch| byte - ch.len_utf8())
        .unwrap_or(0)
}
/// The byte offset one char after `byte`.
pub(crate) fn next_char(src: &str, byte: usize) -> usize {
    let b = byte.min(src.len());
    src[b..]
        .chars()
        .next()
        .map(|ch| b + ch.len_utf8())
        .unwrap_or(b)
}
/// Whether `ch` is part of a word (identifier).
pub(crate) fn is_word(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}
/// The byte offset of the previous word boundary before `byte`.
pub(crate) fn prev_word(src: &str, byte: usize) -> usize {
    let mut b = byte.min(src.len());
    // skip whitespace
    while b > 0 {
        let p = prev_char(src, b);
        if src[p..b]
            .chars()
            .next()
            .is_some_and(|ch| ch.is_whitespace())
        {
            b = p;
        } else {
            break;
        }
    }
    // skip a run of word (or non-word) chars
    let word = b > 0
        && src[prev_char(src, b)..b]
            .chars()
            .next()
            .is_some_and(is_word);
    while b > 0 {
        let p = prev_char(src, b);
        let ch = src[p..b].chars().next().unwrap_or(' ');
        if ch.is_whitespace() || is_word(ch) != word {
            break;
        }
        b = p;
    }
    b
}
/// The byte offset of the next word boundary after `byte`.
pub(crate) fn next_word(src: &str, byte: usize) -> usize {
    let mut b = byte.min(src.len());
    while b < src.len() && src[b..].chars().next().is_some_and(|ch| ch.is_whitespace()) {
        b = next_char(src, b);
    }
    let word = b < src.len() && src[b..].chars().next().is_some_and(is_word);
    while b < src.len() {
        let ch = src[b..].chars().next().unwrap_or(' ');
        if ch.is_whitespace() || is_word(ch) != word {
            break;
        }
        b = next_char(src, b);
    }
    b
}
/// The byte range of the word under `byte` (for double-click). If `byte` isn't on/adjacent
/// to a word character, selects just the single character there.
pub(crate) fn word_at(src: &str, byte: usize) -> (usize, usize) {
    let b = byte.min(src.len());
    let on_word = src[b..].chars().next().is_some_and(is_word)
        || (b > 0
            && src[prev_char(src, b)..b]
                .chars()
                .next()
                .is_some_and(is_word));
    if on_word {
        let mut s = b;
        while s > 0 {
            let p = prev_char(src, s);
            if src[p..s].chars().next().is_some_and(is_word) {
                s = p;
            } else {
                break;
            }
        }
        let mut e = b;
        while e < src.len() {
            if src[e..].chars().next().is_some_and(is_word) {
                e = next_char(src, e);
            } else {
                break;
            }
        }
        (s, e)
    } else {
        (b, next_char(src, b))
    }
}

/// The byte offset of the first occurrence of `needle` at or after `from` (for
/// add-next-occurrence). `None` if there is none from there to the end.
pub(crate) fn find_from(src: &str, needle: &str, from: usize) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    let from = from.min(src.len());
    src.get(from..)
        .and_then(|s| s.find(needle))
        .map(|i| from + i)
}

// ---------------------------------------------------------------------------
// grid ↔ pixel
// ---------------------------------------------------------------------------

/// Map a pointer position to a `(line, col)` grid cell, clamped to the document's lines.
pub(crate) fn pos_to_grid(
    src: &str,
    pos: Offset,
    pad_l: f64,
    pad_t: f64,
    advance: f64,
    line_px: f64,
) -> (usize, usize) {
    let line = (((pos.y - pad_t) / line_px).floor()).max(0.0) as usize;
    let col = (((pos.x - pad_l) / advance).round()).max(0.0) as usize;
    let last = src.split('\n').count().saturating_sub(1);
    (line.min(last), col)
}
