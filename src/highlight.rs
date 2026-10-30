//! The highlight view layer: turn [`Token`]s (from a [`crate::lang::Language`]) into themed
//! rich-text [`TextSpan`]s, and the token-list transforms the virtualized renderer needs —
//! slicing tokens to the visible window and overlaying semantic tokens over lexical ones.

use pebbles::prelude::{TextSpan, span};

use crate::lang::{Token, TokenKind};
use crate::theme::EditorTheme;

/// Build themed rich-text spans for `src`, styling each `tokens` run by its kind (color +
/// bold/italic/underline from the theme) and the gaps between them as plain text.
pub(crate) fn to_spans(
    src: &str,
    tokens: &[Token],
    theme: &EditorTheme,
    fs: f64,
    font_family: &str,
) -> Vec<TextSpan> {
    let mut spans = Vec::new();
    let mut cursor = 0usize;
    let push = |text: &str, kind: TokenKind, spans: &mut Vec<TextSpan>| {
        if !text.is_empty() {
            let st = theme.style(kind);
            let mut sp = span(text.to_string())
                .color(st.color)
                .size(fs as f32)
                .font_family(font_family);
            if st.bold {
                sp = sp.bold();
            }
            if st.italic {
                sp = sp.italic();
            }
            if st.underline {
                sp = sp.underline();
            }
            spans.push(sp);
        }
    };
    for t in tokens {
        let start = t.start.min(src.len());
        let end = (t.start + t.len).min(src.len());
        if start < cursor
            || start > end
            || !src.is_char_boundary(start)
            || !src.is_char_boundary(end)
        {
            continue;
        }
        if start > cursor {
            push(&src[cursor..start], TokenKind::Plain, &mut spans);
        }
        push(&src[start..end], t.kind, &mut spans);
        cursor = end;
    }
    if cursor < src.len() {
        push(&src[cursor..], TokenKind::Plain, &mut spans);
    }
    spans
}

/// Overlay `sem`antic tokens on top of `lex`ical ones: any lexical token overlapping a
/// semantic token is dropped, then the semantic tokens are merged in — so provider/LSP
/// semantics win over lexical highlighting. Result is sorted by `start`.
pub(crate) fn merge_tokens(lex: &[Token], sem: &[Token]) -> Vec<Token> {
    let mut sem_sorted = sem.to_vec();
    sem_sorted.sort_by_key(|t| t.start);
    let covered = |s: usize, e: usize| -> bool {
        sem_sorted
            .iter()
            .any(|t| t.start < e && t.start + t.len > s)
    };
    let mut out: Vec<Token> = lex
        .iter()
        .filter(|t| !covered(t.start, t.start + t.len))
        .copied()
        .collect();
    out.extend(sem_sorted);
    out.sort_by_key(|t| t.start);
    out
}

/// Overlay `over` (sorted, disjoint) on `base` (sorted, disjoint) by **splitting** base
/// tokens around each overlay span — linear, and unlike [`merge_tokens`] it keeps the
/// uncovered parts of a partially covered token (bracket colorization over `();`).
pub(crate) fn overlay_split(base: &[Token], over: &[Token]) -> Vec<Token> {
    if over.is_empty() {
        return base.to_vec();
    }
    let mut out = Vec::with_capacity(base.len() + over.len() * 2);
    let mut j = 0;
    for t in base {
        let (mut s, e) = (t.start, t.start + t.len);
        while j < over.len() && over[j].start + over[j].len <= s {
            j += 1;
        }
        let mut k = j;
        while s < e {
            match over.get(k) {
                Some(o) if o.start < e => {
                    if o.start > s {
                        out.push(Token {
                            start: s,
                            len: o.start - s,
                            kind: t.kind,
                        });
                    }
                    s = s.max(o.start + o.len);
                    k += 1;
                }
                _ => {
                    out.push(Token {
                        start: s,
                        len: e - s,
                        kind: t.kind,
                    });
                    s = e;
                }
            }
        }
    }
    out.extend_from_slice(over);
    out.sort_by_key(|t| t.start);
    out
}

/// Bracket-pair colorization tokens for `src[lo..hi]`: every bracket outside strings and
/// comments (per `tokens`) gets `TokenKind::Bracket(depth)`. `depth0` is the nesting depth at
/// `lo`.
pub(crate) fn bracket_tokens(
    src: &str,
    lo: usize,
    hi: usize,
    depth0: i64,
    pairs: &[(char, char)],
    tokens: &[Token],
) -> Vec<Token> {
    let mut out = Vec::new();
    let mut depth = depth0;
    let mut ti = tokens.partition_point(|t| t.start + t.len <= lo);
    let Some(win) = src.get(lo..hi.min(src.len())) else {
        return out;
    };
    for (i, ch) in win.char_indices() {
        let at = lo + i;
        let is_open = pairs.iter().any(|p| p.0 == ch);
        let is_close = !is_open && pairs.iter().any(|p| p.1 == ch);
        if !is_open && !is_close {
            continue;
        }
        while ti < tokens.len() && tokens[ti].start + tokens[ti].len <= at {
            ti += 1;
        }
        let in_lit = tokens.get(ti).is_some_and(|t| {
            t.start <= at && matches!(t.kind, TokenKind::Str | TokenKind::Comment)
        });
        if in_lit {
            continue;
        }
        if is_open {
            out.push(Token {
                start: at,
                len: ch.len_utf8(),
                kind: TokenKind::Bracket(depth.max(0) as u8),
            });
            depth += 1;
        } else {
            depth -= 1;
            let kind = if depth < 0 {
                TokenKind::BracketUnmatched
            } else {
                TokenKind::Bracket(depth as u8)
            };
            out.push(Token {
                start: at,
                len: ch.len_utf8(),
                kind,
            });
            depth = depth.max(0);
        }
    }
    out
}

/// [`bracket_depth`] continued from `depth` over `text`.
pub(crate) fn bracket_depth_from(depth: i64, text: &str, pairs: &[(char, char)]) -> i64 {
    let mut d = depth;
    for b in text.bytes() {
        if b < 0x80 {
            let c = b as char;
            if pairs.iter().any(|p| p.0 == c) {
                d += 1;
            } else if pairs.iter().any(|p| p.1 == c) {
                d = (d - 1).max(0);
            }
        }
    }
    d
}

/// Naive bracket depth after `prefix` (strings/comments not excluded — the prefix scan is
/// an approximation; the visible window itself is token-aware).
pub(crate) fn bracket_depth(prefix: &str, pairs: &[(char, char)]) -> i64 {
    let mut d: i64 = 0;
    for b in prefix.bytes() {
        if b < 0x80 {
            let c = b as char;
            if pairs.iter().any(|p| p.0 == c) {
                d += 1;
            } else if pairs.iter().any(|p| p.1 == c) {
                d = (d - 1).max(0);
            }
        }
    }
    d
}

/// Clip `tokens` to the byte range `from..to` and rebase their offsets to the start of that
/// slice — so the visible block can be highlighted as its own rich-text (virtualization).
pub(crate) fn slice_tokens(tokens: &[Token], from: usize, to: usize) -> Vec<Token> {
    tokens
        .iter()
        .filter_map(|t| {
            let s = t.start.max(from);
            let e = (t.start + t.len).min(to);
            // `then_some` evaluates its argument eagerly, so guard with `if` to avoid an
            // `e - s` underflow for tokens entirely outside `from..to` (per-line slicing).
            if e > s {
                Some(Token {
                    start: s - from,
                    len: e - s,
                    kind: t.kind,
                })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_tokens_override_lexical() {
        let lex = vec![
            Token {
                start: 0,
                len: 3,
                kind: TokenKind::Keyword,
            },
            Token {
                start: 4,
                len: 3,
                kind: TokenKind::Plain,
            },
        ];
        let sem = vec![Token {
            start: 4,
            len: 3,
            kind: TokenKind::Type,
        }];
        let m = merge_tokens(&lex, &sem);
        assert!(m.iter().any(|t| t.start == 4 && t.kind == TokenKind::Type));
        assert!(!m.iter().any(|t| t.start == 4 && t.kind == TokenKind::Plain));
        assert!(
            m.iter()
                .any(|t| t.start == 0 && t.kind == TokenKind::Keyword)
        );
    }

    #[test]
    fn overlay_split_keeps_uncovered_parts() {
        let base = [Token {
            start: 0,
            len: 3,
            kind: TokenKind::Punctuation,
        }];
        let over = [Token {
            start: 1,
            len: 1,
            kind: TokenKind::Bracket(0),
        }];
        let m = overlay_split(&base, &over);
        assert_eq!(m.len(), 3);
        assert_eq!((m[0].start, m[0].len), (0, 1));
        assert_eq!(m[1].kind, TokenKind::Bracket(0));
        assert_eq!((m[2].start, m[2].len), (2, 1));
    }

    #[test]
    fn bracket_colorization_depths_skip_strings() {
        let src = "f(a[\"(\"]{})";
        let toks = [Token {
            start: 4,
            len: 3,
            kind: TokenKind::Str,
        }];
        let b = bracket_tokens(
            src,
            0,
            src.len(),
            0,
            crate::brackets::DEFAULT_BRACKETS,
            &toks,
        );
        let kinds: Vec<_> = b.iter().map(|t| (t.start, t.kind)).collect();
        assert_eq!(
            kinds,
            vec![
                (1, TokenKind::Bracket(0)),
                (3, TokenKind::Bracket(1)),
                (7, TokenKind::Bracket(1)),
                (8, TokenKind::Bracket(1)),
                (9, TokenKind::Bracket(1)),
                (10, TokenKind::Bracket(0)),
            ]
        );
        assert_eq!(bracket_depth("{ ( [", crate::brackets::DEFAULT_BRACKETS), 3);
        let u = bracket_tokens(")", 0, 1, 0, crate::brackets::DEFAULT_BRACKETS, &[]);
        assert_eq!(u[0].kind, TokenKind::BracketUnmatched);
    }
}
