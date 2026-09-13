//! The highlight view layer: turn [`Token`]s (from a [`crate::lang::Language`]) into themed
//! rich-text [`TextSpan`]s, and the token-list transforms the virtualized renderer needs —
//! slicing tokens to the visible window and overlaying semantic tokens over lexical ones.

use pebbles::prelude::{TextSpan, span};

use crate::MONO;
use crate::lang::{Token, TokenKind};
use crate::theme::EditorTheme;

/// Build themed rich-text spans for `src`, coloring each `tokens` run by its kind and the
/// gaps between them as plain text.
pub(crate) fn to_spans(src: &str, tokens: &[Token], theme: &EditorTheme, fs: f64) -> Vec<TextSpan> {
    let mut spans = Vec::new();
    let mut cursor = 0usize;
    let push = |text: &str, kind: TokenKind, spans: &mut Vec<TextSpan>| {
        if !text.is_empty() {
            spans.push(
                span(text.to_string())
                    .color(theme.color(kind))
                    .size(fs as f32)
                    .font_family(MONO),
            );
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
        sem_sorted.iter().any(|t| t.start < e && t.start + t.len > s)
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

/// Clip `tokens` to the byte range `from..to` and rebase their offsets to the start of that
/// slice — so the visible block can be highlighted as its own rich-text (virtualization).
pub(crate) fn slice_tokens(tokens: &[Token], from: usize, to: usize) -> Vec<Token> {
    tokens
        .iter()
        .filter_map(|t| {
            let s = t.start.max(from);
            let e = (t.start + t.len).min(to);
            (e > s).then_some(Token {
                start: s - from,
                len: e - s,
                kind: t.kind,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_tokens_override_lexical() {
        let lex = vec![
            Token { start: 0, len: 3, kind: TokenKind::Keyword },
            Token { start: 4, len: 3, kind: TokenKind::Plain },
        ];
        let sem = vec![Token { start: 4, len: 3, kind: TokenKind::Type }];
        let m = merge_tokens(&lex, &sem);
        assert!(m.iter().any(|t| t.start == 4 && t.kind == TokenKind::Type));
        assert!(!m.iter().any(|t| t.start == 4 && t.kind == TokenKind::Plain));
        assert!(m.iter().any(|t| t.start == 0 && t.kind == TokenKind::Keyword));
    }
}
