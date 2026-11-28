//! Declarative language editing behaviour — VS Code's `language-configuration.json`: comment
//! tokens, brackets, auto-closing / surrounding pairs, folding markers, indentation rules and
//! on-Enter rules. Set one with [`CodeEditor::language_configuration`](crate::CodeEditor::language_configuration);
//! it overrides the [`Language`](crate::Language)'s comment / bracket metadata and drives
//! typing, Enter, folding and bracket colorization for any language (including plain text).
//!
//! Regexes use the `regex` crate's syntax (a superset of the common JavaScript subset).
//! Patterns it cannot compile (look-around, back-references) are skipped, so a rule using
//! them never fires.

use regex::Regex;

/// Where an auto-closing pair must not close (`notIn`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotIn {
    /// Inside a string literal.
    String,
    /// Inside a comment.
    Comment,
}

/// An `autoClosingPairs` entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoClosingPair {
    /// The opener (may be several chars, e.g. `/**`; the last typed char completes it).
    pub open: String,
    /// The text inserted after the caret.
    pub close: String,
    /// Scopes where the pair does not auto-close.
    pub not_in: Vec<NotIn>,
}

impl AutoClosingPair {
    /// A pair that closes everywhere.
    pub fn new(open: impl Into<String>, close: impl Into<String>) -> Self {
        AutoClosingPair {
            open: open.into(),
            close: close.into(),
            not_in: Vec::new(),
        }
    }
    /// The pair, not closing in `scopes`.
    pub fn not_in(mut self, scopes: impl Into<Vec<NotIn>>) -> Self {
        self.not_in = scopes.into();
        self
    }
}

/// `indentationRules` (regex sources).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IndentationRules {
    /// A line matching this indents the next line.
    pub increase_indent: Option<String>,
    /// A line matching this is outdented one level.
    pub decrease_indent: Option<String>,
    /// Only the next line is indented (e.g. a brace-less `if`).
    pub indent_next_line: Option<String>,
    /// Lines ignored when computing indentation.
    pub unindented_line: Option<String>,
}

/// `onEnterRules[].action.indent`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IndentAction {
    /// Keep the current indentation.
    #[default]
    None,
    /// One level deeper.
    Indent,
    /// Caret line one level deeper, the text after the caret moves to a line at the current
    /// level.
    IndentOutdent,
    /// One level shallower.
    Outdent,
}

/// An `onEnterRules` entry (regex sources).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OnEnterRule {
    /// Must match the line text before the caret.
    pub before_text: String,
    /// When set, must match the line text after the caret.
    pub after_text: Option<String>,
    /// When set, must match the line above the caret line.
    pub previous_line_text: Option<String>,
    /// What to do.
    pub indent: IndentAction,
    /// Text appended after the new line's indentation (e.g. ` * `).
    pub append_text: Option<String>,
    /// Columns removed from the new line's indentation.
    pub remove_text: usize,
}

/// A language's editing configuration (VS Code `language-configuration.json`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LanguageConfiguration {
    /// `comments.lineComment`.
    pub line_comment: Option<String>,
    /// `comments.blockComment`.
    pub block_comment: Option<(String, String)>,
    /// `brackets` (matching, Enter, jump-to-bracket).
    pub brackets: Vec<(String, String)>,
    /// `colorizedBracketPairs` (default: `brackets`).
    pub colorized_bracket_pairs: Option<Vec<(String, String)>>,
    /// `autoClosingPairs` (default: `brackets` plus quotes).
    pub auto_closing_pairs: Vec<AutoClosingPair>,
    /// `autoCloseBefore`: a pair only auto-closes when the next char is one of these (or
    /// the line end). VS Code's default when unset.
    pub auto_close_before: Option<String>,
    /// `surroundingPairs` (default: the auto-closing pairs).
    pub surrounding_pairs: Vec<(String, String)>,
    /// `folding.markers` (start, end) regex sources.
    pub folding_markers: Option<(String, String)>,
    /// `folding.offSide`: blank lines after a block belong to the next block.
    pub off_side: bool,
    /// `wordPattern` regex source (word completion, double-click).
    pub word_pattern: Option<String>,
    /// `indentationRules`.
    pub indentation_rules: Option<IndentationRules>,
    /// `onEnterRules`, first match wins.
    pub on_enter_rules: Vec<OnEnterRule>,
}

/// VS Code's default `autoCloseBefore`.
pub const DEFAULT_AUTO_CLOSE_BEFORE: &str = ";:.,=}])> \n\t";

fn re(src: &Option<String>) -> Option<Regex> {
    src.as_deref().and_then(|s| Regex::new(s).ok())
}

/// A [`LanguageConfiguration`] with its regexes compiled.
#[derive(Debug, Default)]
pub(crate) struct Compiled {
    pub(crate) cfg: LanguageConfiguration,
    increase: Option<Regex>,
    decrease: Option<Regex>,
    indent_next: Option<Regex>,
    unindented: Option<Regex>,
    enter: Vec<(Regex, Option<Regex>, Option<Regex>, OnEnterRule)>,
    fold: Option<(Regex, Regex)>,
    pub(crate) word: Option<Regex>,
}

/// The single-char pairs of `pairs` (the editor's bracket machinery is char-based).
pub(crate) fn char_pairs(pairs: &[(String, String)]) -> Vec<(char, char)> {
    pairs
        .iter()
        .filter_map(|(o, c)| {
            let (mut a, mut b) = (o.chars(), c.chars());
            match (a.next(), a.next(), b.next(), b.next()) {
                (Some(x), None, Some(y), None) => Some((x, y)),
                _ => None,
            }
        })
        .collect()
}

impl Compiled {
    pub(crate) fn new(cfg: LanguageConfiguration) -> Self {
        let ir = cfg.indentation_rules.clone().unwrap_or_default();
        let enter = cfg
            .on_enter_rules
            .iter()
            .filter_map(|r| {
                let b = Regex::new(&r.before_text).ok()?;
                let a = match &r.after_text {
                    Some(s) => Some(Regex::new(s).ok()?),
                    None => None,
                };
                let p = match &r.previous_line_text {
                    Some(s) => Some(Regex::new(s).ok()?),
                    None => None,
                };
                Some((b, a, p, r.clone()))
            })
            .collect();
        let fold = cfg
            .folding_markers
            .as_ref()
            .and_then(|(s, e)| Some((Regex::new(s).ok()?, Regex::new(e).ok()?)));
        Compiled {
            increase: re(&ir.increase_indent),
            decrease: re(&ir.decrease_indent),
            indent_next: re(&ir.indent_next_line),
            unindented: re(&ir.unindented_line),
            word: re(&cfg.word_pattern),
            enter,
            fold,
            cfg,
        }
    }

    /// The auto-closing pairs in effect.
    pub(crate) fn auto_pairs(&self) -> Vec<AutoClosingPair> {
        if !self.cfg.auto_closing_pairs.is_empty() {
            return self.cfg.auto_closing_pairs.clone();
        }
        let mut v: Vec<AutoClosingPair> = self
            .cfg
            .brackets
            .iter()
            .map(|(o, c)| AutoClosingPair::new(o.clone(), c.clone()))
            .collect();
        for q in ["\"", "'"] {
            v.push(AutoClosingPair::new(q, q).not_in(vec![NotIn::String, NotIn::Comment]));
        }
        v
    }

    fn surround_pairs(&self) -> Vec<(String, String)> {
        if !self.cfg.surrounding_pairs.is_empty() {
            return self.cfg.surrounding_pairs.clone();
        }
        self.auto_pairs()
            .into_iter()
            .map(|p| (p.open, p.close))
            .collect()
    }

    /// The colorized bracket pairs.
    pub(crate) fn colorized(&self) -> Vec<(char, char)> {
        char_pairs(
            self.cfg
                .colorized_bracket_pairs
                .as_ref()
                .unwrap_or(&self.cfg.brackets),
        )
    }

    /// Is byte `at` inside a string or comment? A line-local scan using the comment tokens
    /// and the quote-like auto-closing pairs (VS Code asks its tokenizer; this is the
    /// declarative approximation).
    pub(crate) fn scope_at(&self, src: &str, at: usize) -> Option<NotIn> {
        let ls = src[..at].rfind('\n').map_or(0, |i| i + 1);
        let line = &src[ls..at];
        let quotes: Vec<String> = self
            .auto_pairs()
            .into_iter()
            .filter(|p| p.open == p.close)
            .map(|p| p.open)
            .collect();
        let mut i = 0;
        let mut in_str: Option<&str> = None;
        let mut in_block: Option<&str> = None;
        while i < line.len() {
            let rest = &line[i..];
            if let Some(q) = in_str {
                if rest.starts_with('\\') {
                    i += 1 + rest[1..].chars().next().map_or(0, char::len_utf8);
                    continue;
                }
                if rest.starts_with(q) {
                    in_str = None;
                    i += q.len();
                    continue;
                }
            } else if let Some(end) = in_block {
                if rest.starts_with(end) {
                    in_block = None;
                    i += end.len();
                    continue;
                }
            } else {
                if let Some(lc) = &self.cfg.line_comment
                    && !lc.is_empty()
                    && rest.starts_with(lc.as_str())
                {
                    return Some(NotIn::Comment);
                }
                if let Some((a, b)) = &self.cfg.block_comment
                    && !a.is_empty()
                    && rest.starts_with(a.as_str())
                {
                    in_block = Some(b.as_str());
                    i += a.len();
                    continue;
                }
                if let Some(q) = quotes
                    .iter()
                    .find(|q| !q.is_empty() && rest.starts_with(q.as_str()))
                {
                    in_str = Some(q.as_str());
                    i += q.len();
                    continue;
                }
            }
            i += rest.chars().next().map_or(1, char::len_utf8);
        }
        if in_str.is_some() {
            Some(NotIn::String)
        } else if in_block.is_some() {
            Some(NotIn::Comment)
        } else {
            None
        }
    }

    /// Typing `ch` with an empty selection at `at`: the auto-close insertion `(text, caret
    /// offset within text)` if a pair applies.
    pub(crate) fn auto_close(&self, src: &str, at: usize, ch: char) -> Option<(String, usize)> {
        let before = &src[..at];
        let next = src[at..].chars().next();
        let allowed = self
            .cfg
            .auto_close_before
            .as_deref()
            .unwrap_or(DEFAULT_AUTO_CLOSE_BEFORE);
        if next.is_some_and(|c| c != '\n' && !allowed.contains(c)) {
            return None;
        }
        let mut pairs = self.auto_pairs();
        // Longest opener first (`/**` beats `/*`).
        pairs.sort_by_key(|p| std::cmp::Reverse(p.open.len()));
        for p in pairs {
            let Some(last) = p.open.chars().next_back() else {
                continue;
            };
            let head = &p.open[..p.open.len() - last.len_utf8()];
            if last != ch || !before.ends_with(head) {
                continue;
            }
            // Quote-like pairs do not close right after a word char.
            if p.open == p.close
                && before[..before.len() - head.len()]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_')
            {
                return None;
            }
            if let Some(scope) = self.scope_at(src, at - head.len())
                && p.not_in.contains(&scope)
            {
                return None;
            }
            let mut s = ch.to_string();
            s.push_str(&p.close);
            return Some((s, ch.len_utf8()));
        }
        None
    }

    /// The surrounding pair for `ch` typed over a selection.
    pub(crate) fn surround(&self, ch: char) -> Option<(String, String)> {
        let mut b = [0u8; 4];
        let s = ch.encode_utf8(&mut b);
        self.surround_pairs().into_iter().find(|(o, _)| o == s)
    }

    /// Closers the caret steps over when typed.
    pub(crate) fn is_closer(&self, ch: char) -> bool {
        self.auto_pairs()
            .iter()
            .any(|p| p.close.chars().count() == 1 && p.close.starts_with(ch))
    }

    /// Enter at `at`: `(inserted text, caret offset within it)`, or `None` to fall back to
    /// the editor's default.
    pub(crate) fn on_enter(&self, src: &str, at: usize, tab: &str) -> Option<(String, usize)> {
        let ls = src[..at].rfind('\n').map_or(0, |i| i + 1);
        let le = src[at..].find('\n').map_or(src.len(), |i| at + i);
        let before = &src[ls..at];
        let after = &src[at..le];
        let prev = if ls == 0 {
            ""
        } else {
            let pe = ls - 1;
            let ps = src[..pe].rfind('\n').map_or(0, |i| i + 1);
            &src[ps..pe]
        };
        let indent: String = before
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        let finish = |action: IndentAction, append: &str, remove: usize| {
            let mut ind = indent.clone();
            let mut keep = String::new();
            match action {
                IndentAction::None => {}
                IndentAction::Indent => ind.push_str(tab),
                IndentAction::IndentOutdent => {
                    keep = indent.clone();
                    ind.push_str(tab);
                }
                IndentAction::Outdent => ind = outdent(&ind, tab),
            }
            for _ in 0..remove {
                ind.pop();
            }
            let line = format!("\n{ind}{append}");
            let caret = line.len();
            if action == IndentAction::IndentOutdent {
                Some((format!("{line}\n{keep}"), caret))
            } else {
                Some((line, caret))
            }
        };
        for (b, a, p, rule) in &self.enter {
            if b.is_match(before)
                && a.as_ref().is_none_or(|a| a.is_match(after))
                && p.as_ref().is_none_or(|p| p.is_match(prev))
            {
                return finish(
                    rule.indent,
                    rule.append_text.as_deref().unwrap_or(""),
                    rule.remove_text,
                );
            }
        }
        // Brackets: `{|}` opens a block, a trailing opener indents.
        let bt = before.trim_end();
        let at_ = after.trim_start();
        for (o, c) in &self.cfg.brackets {
            if !o.is_empty() && bt.ends_with(o.as_str()) {
                if !c.is_empty() && at_.starts_with(c.as_str()) {
                    return finish(IndentAction::IndentOutdent, "", 0);
                }
                return finish(IndentAction::Indent, "", 0);
            }
        }
        // Indentation rules.
        let inc = self.increase.as_ref().is_some_and(|r| r.is_match(before))
            || self
                .indent_next
                .as_ref()
                .is_some_and(|r| r.is_match(before));
        let dec_after = self.decrease.as_ref().is_some_and(|r| r.is_match(after));
        match (inc, dec_after) {
            (true, true) => finish(IndentAction::IndentOutdent, "", 0),
            (true, false) => finish(IndentAction::Indent, "", 0),
            (false, true) => finish(IndentAction::Outdent, "", 0),
            (false, false) if self.increase.is_some() => finish(IndentAction::None, "", 0),
            _ => None,
        }
    }

    /// After typing on the line containing `at`: the line's new leading whitespace when it
    /// now matches `decreaseIndentPattern` (VS Code `editor.autoIndent: full`).
    pub(crate) fn reindent_line(
        &self,
        src: &str,
        at: usize,
        tab: &str,
    ) -> Option<(usize, usize, String)> {
        let dec = self.decrease.as_ref()?;
        let ls = src[..at].rfind('\n').map_or(0, |i| i + 1);
        let le = src[at..].find('\n').map_or(src.len(), |i| at + i);
        let line = &src[ls..le];
        if !dec.is_match(line) {
            return None;
        }
        // The previous non-blank line that is not an unindented line.
        let mut end = ls;
        let reference = loop {
            if end == 0 {
                break None;
            }
            let pe = end - 1;
            let ps = src[..pe].rfind('\n').map_or(0, |i| i + 1);
            let l = &src[ps..pe];
            end = ps;
            if l.trim().is_empty() || self.unindented.as_ref().is_some_and(|u| u.is_match(l)) {
                continue;
            }
            break Some(l);
        }?;
        let ref_indent: String = reference
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        let target = if self
            .increase
            .as_ref()
            .is_some_and(|r| r.is_match(reference))
        {
            ref_indent
        } else {
            outdent(&ref_indent, tab)
        };
        let cur_len = line.len() - line.trim_start_matches([' ', '\t']).len();
        if line[..cur_len] == target {
            return None;
        }
        Some((ls, ls + cur_len, target))
    }

    /// `folding.markers` regions as `(start_line, end_line)`, nested by a stack.
    pub(crate) fn marker_regions(&self, src: &str) -> Vec<(usize, usize)> {
        let Some((s, e)) = &self.fold else {
            return Vec::new();
        };
        let mut stack = Vec::new();
        let mut out = Vec::new();
        for (i, l) in src.lines().enumerate() {
            if s.is_match(l) {
                stack.push(i);
            } else if e.is_match(l)
                && let Some(h) = stack.pop()
                && i > h
            {
                out.push((h, i));
            }
        }
        out.sort_unstable();
        out
    }
}

fn outdent(ind: &str, tab: &str) -> String {
    if let Some(s) = ind.strip_suffix(tab) {
        return s.to_string();
    }
    if let Some(s) = ind.strip_suffix('\t') {
        return s.to_string();
    }
    let n = ind.len() - ind.trim_end_matches(' ').len();
    ind[..ind.len() - n.min(tab.len().max(1))].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Modelled on the Elixir extension's language-configuration.json.
    fn elixir() -> Compiled {
        Compiled::new(LanguageConfiguration {
            line_comment: Some("#".into()),
            brackets: vec![
                ("{".into(), "}".into()),
                ("[".into(), "]".into()),
                ("(".into(), ")".into()),
            ],
            auto_closing_pairs: vec![
                AutoClosingPair::new("{", "}"),
                AutoClosingPair::new("(", ")"),
                AutoClosingPair::new("\"", "\"").not_in(vec![NotIn::String, NotIn::Comment]),
                AutoClosingPair::new("\"\"\"", "\"\"\""),
                AutoClosingPair::new("<<", ">>"),
            ],
            folding_markers: Some((r"^\s*#\s*region\b".into(), r"^\s*#\s*endregion\b".into())),
            indentation_rules: Some(IndentationRules {
                increase_indent: Some(
                    r"^\s*(after|else|catch|rescue|fn|[^#]*(do|<\-|\->|\{|\[|\=))\s*$".into(),
                ),
                decrease_indent: Some(r"^\s*((\}|\])\s*$|(after|else|catch|rescue|end)\b)".into()),
                ..Default::default()
            }),
            on_enter_rules: vec![OnEnterRule {
                before_text: r"^\s*#\s*doc".into(),
                indent: IndentAction::None,
                append_text: Some("# ".into()),
                ..Default::default()
            }],
            ..Default::default()
        })
    }

    #[test]
    fn enter_follows_indentation_and_on_enter_rules() {
        let c = elixir();
        let src = "def a do";
        assert_eq!(c.on_enter(src, src.len(), "  "), Some(("\n  ".into(), 3)));
        let src = "  x = 1";
        assert_eq!(c.on_enter(src, src.len(), "  "), Some(("\n  ".into(), 3)));
        let src = "  # doc here";
        assert_eq!(c.on_enter(src, src.len(), "  "), Some(("\n  # ".into(), 5)));
        // do|end splits into an indented middle line.
        let src = "if x do end";
        let at = src.find(" end").unwrap();
        assert_eq!(c.on_enter(src, at, "  "), Some(("\n  \n".into(), 3)));
        // `{|}` via brackets.
        assert_eq!(c.on_enter("m = {}", 5, "  "), Some(("\n  \n".into(), 3)));
    }

    #[test]
    fn typing_end_outdents() {
        let c = elixir();
        let src = "def a do\n  x\n  end";
        let (a, b, ind) = c.reindent_line(src, src.len(), "  ").unwrap();
        assert_eq!((&src[a..b], ind.as_str()), ("  ", ""));
        // Already right: nothing.
        assert!(c.reindent_line("def a do\nend", 12, "  ").is_none());
    }

    #[test]
    fn auto_close_respects_not_in_before_and_multichar() {
        let c = elixir();
        assert_eq!(c.auto_close("x = ", 4, '{'), Some(("{}".into(), 1)));
        assert_eq!(c.auto_close("x = ", 4, '"'), Some(("\"\"".into(), 1)));
        // In a comment the quote does not close; `{` has no notIn.
        assert_eq!(c.auto_close("# a ", 4, '"'), None);
        assert!(c.auto_close("# a ", 4, '{').is_some());
        // Inside a string.
        assert_eq!(c.auto_close("\"ab ", 4, '"'), None);
        // autoCloseBefore: a word char after the caret blocks it.
        assert_eq!(c.auto_close("x(y", 2, '('), None);
        // Multi-char openers.
        assert_eq!(c.auto_close("x = <", 5, '<'), Some(("<>>".into(), 1)));
        assert!(c.is_closer('}'));
        assert_eq!(c.surround('('), Some(("(".into(), ")".into())));
    }

    #[test]
    fn markers_fold_and_bad_regex_is_ignored() {
        let c = elixir();
        let r = c.marker_regions("# region a\nx\n  # region b\ny\n  # endregion\n# endregion\n");
        assert_eq!(r, vec![(0, 5), (2, 4)]);
        let bad = Compiled::new(LanguageConfiguration {
            word_pattern: Some("(?<=x)y".into()),
            on_enter_rules: vec![OnEnterRule {
                before_text: "(?!a)".into(),
                ..Default::default()
            }],
            ..Default::default()
        });
        assert!(bad.word.is_none());
        assert!(bad.on_enter("a", 1, "  ").is_none());
    }

    #[test]
    fn colorized_pairs_default_to_brackets() {
        let c = elixir();
        assert_eq!(c.colorized(), vec![('{', '}'), ('[', ']'), ('(', ')')]);
        let mut cfg = c.cfg.clone();
        cfg.colorized_bracket_pairs =
            Some(vec![("(".into(), ")".into()), ("do".into(), "end".into())]);
        assert_eq!(Compiled::new(cfg).colorized(), vec![('(', ')')]);
    }
}
