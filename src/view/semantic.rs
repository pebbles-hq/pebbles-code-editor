//! Semantic tokens from the language service: `full/delta` against the previous result when
//! the server supports it, `full` otherwise, `range` for the viewport when full tokens are
//! unsupported. Tokens are decoded with the server's legend, mapped onto the theme's token
//! kinds and layered over the lexical highlighting; between an edit and the next reply the
//! previous tokens are shifted through the edit so colours do not flicker.

use std::rc::Rc;

use crate::lang::{Token, TokenKind};
use crate::language::{self as lsp};
use crate::view::lsp::Lsp;

/// The theme kind for an LSP token type + modifiers (VS Code's standard scopes collapsed onto
/// the editor's palette). `None` = leave the lexical colour.
pub(crate) fn kind_for(ty: &str, modifiers: &[String]) -> Option<TokenKind> {
    let has = |m: &str| modifiers.iter().any(|x| x == m);
    Some(match ty {
        "namespace" | "module" => TokenKind::Namespace,
        "type" | "class" | "enum" | "interface" | "struct" | "typeParameter" | "typeAlias"
        | "builtinType" | "selfType" => TokenKind::Type,
        "function" | "method" => TokenKind::Function,
        "macro" => TokenKind::Macro,
        "decorator" | "attribute" | "derive" => TokenKind::Attribute,
        "property" | "event" => TokenKind::Property,
        "enumMember" => TokenKind::Constant,
        "variable" | "parameter" if has("readonly") && has("static") => TokenKind::Constant,
        "variable" | "parameter" => TokenKind::Plain,
        "keyword" | "modifier" | "selfKeyword" => TokenKind::Keyword,
        "comment" => TokenKind::Comment,
        "string" => TokenKind::Str,
        "regexp" | "escapeSequence" => TokenKind::Escape,
        "number" => TokenKind::Number,
        "boolean" => TokenKind::Constant,
        "operator" => TokenKind::Punctuation,
        _ => return None,
    })
}

/// Decode `data` into byte-range theme tokens over `text`.
pub(crate) fn tokens_from(
    data: &[u32],
    legend: &lsp::SemanticTokensLegend,
    text: &str,
) -> Vec<Token> {
    lsp::decode_semantic_tokens(data, legend)
        .into_iter()
        .filter_map(|t| {
            let kind = kind_for(&t.token_type, &t.modifiers)?;
            let (a, b) = lsp::range_to_bytes(text, t.range);
            (b > a).then_some(Token {
                start: a,
                len: b - a,
                kind,
            })
        })
        .collect()
}

/// Shift `tokens` (over `old`) through the single-range edit turning `old` into `new`;
/// tokens touching the edit are dropped.
fn shift(tokens: &[Token], old: &str, new: &str) -> Vec<Token> {
    let Some(e) = crate::collab::diff(old, new) else {
        return tokens.to_vec();
    };
    let delta = e.insert.len() as isize - (e.to - e.from) as isize;
    tokens
        .iter()
        .filter_map(|t| {
            if t.start + t.len <= e.from {
                Some(*t)
            } else if t.start >= e.to {
                Some(Token {
                    start: (t.start as isize + delta) as usize,
                    ..*t
                })
            } else {
                None
            }
        })
        .collect()
}

impl Lsp {
    /// Ask for semantic tokens when the document (or, in range mode, the viewport) changed.
    pub(crate) fn request_semantic(&self, first: usize, last: usize) {
        let Some(svc) = self.svc.clone() else { return };
        let Some(legend) = svc.semantic_tokens_legend() else {
            return;
        };
        let ver = self.version();
        let range_mode = self.store.sem_full.get() == Some(false);
        let key = (ver, range_mode.then_some((first, last)));
        if self.store.sem_key.get() == Some(key) {
            return;
        }
        self.store.sem_key.set(Some(key));
        let me = self.clone();
        let lg = legend.clone();
        let store_full = move |me: &Lsp, t: lsp::SemanticTokens| {
            let text = me.state.peek().text();
            let toks = tokens_from(&t.data, &lg, &text);
            *me.store.sem_prev.borrow_mut() = Some((t.result_id, t.data));
            *me.store.sem_cache.borrow_mut() = Some((text, Rc::new(toks)));
            me.h.sem_tick.set(me.h.sem_tick.peek() + 1);
        };
        if range_mode {
            let lg = legend.clone();
            let me2 = me.clone();
            let reply = self.guard(
                &self.store.sem_gate,
                move |t: Option<lsp::SemanticTokens>| {
                    let text = me2.state.peek().text();
                    let toks = t
                        .map(|t| tokens_from(&t.data, &lg, &text))
                        .unwrap_or_default();
                    *me2.store.sem_cache.borrow_mut() = Some((text, Rc::new(toks)));
                    me2.h.sem_tick.set(me2.h.sem_tick.peek() + 1);
                },
            );
            let range = lsp::Range::new(
                lsp::Position::new(first as u32, 0),
                lsp::Position::new(last as u32 + 1, 0),
            );
            self.with_doc(|doc, _| svc.semantic_tokens_range(doc, range, reply));
            return;
        }
        // Delta against the previous result, when there is one and the server answers.
        let prev = self.store.sem_prev.borrow().clone();
        if let Some((Some(id), data)) = prev {
            let sf = store_full.clone();
            let m = me.clone();
            let reply = self.guard(
                &self.store.sem_gate,
                move |r: Option<lsp::SemanticTokensFullDeltaResult>| match r {
                    Some(lsp::SemanticTokensFullDeltaResult::Tokens(t)) => sf(&m, t),
                    Some(lsp::SemanticTokensFullDeltaResult::Delta(d)) => sf(
                        &m,
                        lsp::SemanticTokens {
                            result_id: d.result_id,
                            data: lsp::apply_semantic_edits(&data, &d.edits),
                        },
                    ),
                    None => {}
                },
            );
            if self.with_doc(|doc, _| svc.semantic_tokens_full_delta(doc, &id, reply)) {
                return;
            }
        }
        let reply = self.guard(
            &self.store.sem_gate,
            move |t: Option<lsp::SemanticTokens>| {
                if let Some(t) = t {
                    store_full(&me, t);
                }
            },
        );
        if self.with_doc(|doc, _| svc.semantic_tokens_full(doc, reply)) {
            self.store.sem_full.set(Some(true));
        } else {
            // Full tokens unsupported: use the viewport range from now on.
            self.store.sem_full.set(Some(false));
            self.store.sem_key.set(None);
            self.request_semantic(first, last);
        }
    }

    /// The service's tokens for `src` (shifted through edits made since the last reply).
    pub(crate) fn semantic_for(&self, src: &str) -> Option<Rc<Vec<Token>>> {
        let _ = self.h.sem_tick.get();
        let mut cache = self.store.sem_cache.borrow_mut();
        let (text, toks) = cache.as_mut()?;
        if text.as_str() != src {
            let mapped = shift(toks, text, src);
            *toks = Rc::new(mapped);
            *text = src.to_string();
        }
        Some(toks.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_shift_through_an_edit_and_drop_touched_ones() {
        let t = |start, len| Token {
            start,
            len,
            kind: TokenKind::Function,
        };
        let toks = vec![t(0, 3), t(4, 3), t(8, 3)];
        // An insert before a token shifts it.
        let out = shift(&toks, "foo bar baz", "foo XXbar baz");
        assert_eq!(
            out.iter().map(|t| t.start).collect::<Vec<_>>(),
            vec![0, 6, 10]
        );
        // An edit inside a token drops it.
        let out = shift(&toks, "foo bar baz", "foo bXar baz");
        assert_eq!(out.iter().map(|t| t.start).collect::<Vec<_>>(), vec![0, 9]);
        assert_eq!(kind_for("variable", &[]), Some(TokenKind::Plain));
        assert_eq!(kind_for("unknownThing", &[]), None);
    }
}
