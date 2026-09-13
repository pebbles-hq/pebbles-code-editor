//! Bracket domain: the default pairs and nesting-aware match-finding used by bracket
//! highlighting ([`crate::view`]) and auto-closing ([`crate::commands`]). Auto-close
//! *behavior* lives in the command engine; this module is the pure matching logic.

use crate::geometry::{next_char, prev_char};

/// The default bracket pairs matched/auto-closed when no language (or the language's
/// default) applies.
pub(crate) const DEFAULT_BRACKETS: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}')];

/// If the caret sits just after or before a bracket, return `(bracket_byte, match_byte)`
/// for it and its counterpart (nesting-aware). `None` if there's no adjacent bracket or no
/// match. Scans characters only (no string/comment awareness) — the standard baseline.
pub(crate) fn find_bracket_match(
    src: &str,
    brackets: &[(char, char)],
    caret: usize,
) -> Option<(usize, usize)> {
    let mut candidates = Vec::with_capacity(2);
    if caret > 0 {
        candidates.push(prev_char(src, caret)); // char before the caret
    }
    if caret < src.len() {
        candidates.push(caret); // char at the caret
    }
    for cand in candidates {
        let ch = match src.get(cand..).and_then(|s| s.chars().next()) {
            Some(c) => c,
            None => continue,
        };
        if let Some(&(o, c)) = brackets.iter().find(|(o, _)| *o == ch) {
            let after = next_char(src, cand);
            let mut depth = 1i32;
            for (i, x) in src[after..].char_indices() {
                if x == o {
                    depth += 1;
                } else if x == c {
                    depth -= 1;
                    if depth == 0 {
                        return Some((cand, after + i));
                    }
                }
            }
        } else if let Some(&(o, c)) = brackets.iter().find(|(_, c)| *c == ch) {
            let mut depth = 1i32;
            for (i, x) in src[..cand].char_indices().rev() {
                if x == c {
                    depth += 1;
                } else if x == o {
                    depth -= 1;
                    if depth == 0 {
                        return Some((cand, i));
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bracket_match_forward_and_backward() {
        let src = "fn f() { a(b()) }";
        // caret just after the first '(' at index 4 → matches ')' at 5
        assert_eq!(find_bracket_match(src, DEFAULT_BRACKETS, 5), Some((4, 5)));
        // caret before '{' (index 7) → matches the last '}' (index 16)
        assert_eq!(find_bracket_match(src, DEFAULT_BRACKETS, 7), Some((7, 16)));
        // nested: caret after '(' at 10 matches the outer call's ')' at 14
        let (a, b) = find_bracket_match(src, DEFAULT_BRACKETS, 11).unwrap();
        assert_eq!((a, b), (10, 14));
    }

    #[test]
    fn bracket_match_none_when_not_on_a_bracket() {
        let src = "abc def";
        assert_eq!(find_bracket_match(src, DEFAULT_BRACKETS, 2), None);
    }

    #[test]
    fn bracket_match_unbalanced_is_none() {
        let src = "foo(bar";
        // '(' at 3 has no closer
        assert_eq!(find_bracket_match(src, DEFAULT_BRACKETS, 4), None);
    }
}
