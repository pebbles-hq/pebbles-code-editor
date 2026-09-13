//! Search: the pure find (and match-collection) logic behind the find/replace widget.
//!
//! Given a query + [`Options`] it returns every match as a byte range over the document —
//! plain substring (case-sensitive or not), whole-word, or regex (via the `regex` crate).
//! Never panics: an invalid regex simply yields no matches. The view ([`crate::view`]) drives
//! the widget, highlights the matches, and applies replacements.

use crate::geometry::is_word;

/// Find options (mirrored by the widget's toggle chips).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Options {
    pub(crate) case_sensitive: bool,
    pub(crate) whole_word: bool,
    pub(crate) regex: bool,
}

/// Every match of `query` in `src` as a byte range `[start, end)`, in document order. Empty
/// when the query is empty or (in regex mode) the pattern doesn't compile.
pub(crate) fn find_all(src: &str, query: &str, opts: &Options) -> Vec<(usize, usize)> {
    if query.is_empty() {
        return Vec::new();
    }
    if opts.regex {
        return regex_matches(src, query, opts);
    }
    literal_matches(src, query, opts)
}

/// Whether `[start, end)` is bounded by non-word chars (or doc edges) — the whole-word test.
fn is_whole_word(src: &str, start: usize, end: usize) -> bool {
    let before_ok = start == 0 || !src[..start].chars().next_back().is_some_and(is_word);
    let after_ok = end >= src.len() || !src[end..].chars().next().is_some_and(is_word);
    before_ok && after_ok
}

fn literal_matches(src: &str, query: &str, opts: &Options) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    if opts.case_sensitive {
        let mut from = 0;
        while let Some(i) = src[from..].find(query) {
            let start = from + i;
            let end = start + query.len();
            if !opts.whole_word || is_whole_word(src, start, end) {
                out.push((start, end));
            }
            from = start + query.chars().next().map(char::len_utf8).unwrap_or(1);
        }
    } else {
        // Case-insensitive: scan on the lowercased haystack/needle. Both are ASCII-foldable
        // for the common case; for correctness on any input we compare lowercased strings and
        // map the lowercased index back — since lowercasing can change byte length, we instead
        // walk char windows. Simpler + robust: use the regex engine's case-insensitive flag.
        return regex_matches(
            src,
            &regex::escape(query),
            &Options {
                regex: true,
                ..*opts
            },
        );
    }
    out
}

fn regex_matches(src: &str, pattern: &str, opts: &Options) -> Vec<(usize, usize)> {
    let pattern = if opts.whole_word {
        format!(r"\b(?:{pattern})\b")
    } else {
        pattern.to_string()
    };
    let re = match regex::RegexBuilder::new(&pattern)
        .case_insensitive(!opts.case_sensitive)
        .build()
    {
        Ok(re) => re,
        Err(_) => return Vec::new(),
    };
    re.find_iter(src)
        .filter(|m| m.start() != m.end()) // skip zero-width matches
        .map(|m| (m.start(), m.end()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(case: bool, word: bool, rx: bool) -> Options {
        Options {
            case_sensitive: case,
            whole_word: word,
            regex: rx,
        }
    }

    #[test]
    fn plain_case_insensitive_by_default() {
        let m = find_all("Foo foo FOO", "foo", &opts(false, false, false));
        assert_eq!(m, vec![(0, 3), (4, 7), (8, 11)]);
    }

    #[test]
    fn case_sensitive() {
        let m = find_all("Foo foo FOO", "foo", &opts(true, false, false));
        assert_eq!(m, vec![(4, 7)]);
    }

    #[test]
    fn whole_word() {
        let m = find_all("foo foobar foo", "foo", &opts(true, true, false));
        assert_eq!(m, vec![(0, 3), (11, 14)]);
    }

    #[test]
    fn regex_mode() {
        let m = find_all("a1 b22 c333", r"\d+", &opts(true, false, true));
        assert_eq!(m, vec![(1, 2), (4, 6), (8, 11)]);
    }

    #[test]
    fn invalid_regex_is_empty_not_panic() {
        assert!(find_all("abc", "(", &opts(true, false, true)).is_empty());
    }
}
