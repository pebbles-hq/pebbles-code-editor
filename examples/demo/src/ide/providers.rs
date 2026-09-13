//! Sample IntelliSense providers + diagnostics/inlay computers. A real IDE would back these
//! with a language server; here they're small heuristics so every editor capability is live.

use std::rc::Rc;

use pebbles_code_editor::{
    CompletionContext, CompletionItem, CompletionKind, CompletionProvider, DefinitionProvider,
    Diagnostic, FormatProvider, Hover, HoverProvider, InlayHint, Severity, SignatureHelp,
    SignatureProvider, lang, lang::Language,
};

/// Map a language label to a highlighter.
pub fn lang_for(name: &str) -> Box<dyn Language> {
    match name {
        "Rust" => Box::new(lang::Rust),
        "TypeScript" => Box::new(lang::typescript()),
        "JavaScript" => Box::new(lang::javascript()),
        "Python" => Box::new(lang::Python),
        "Go" => Box::new(lang::go()),
        "C" => Box::new(lang::c_lang()),
        "Java" => Box::new(lang::java()),
        "JSON" => Box::new(lang::Json),
        "TOML" => Box::new(lang::toml()),
        _ => Box::new(lang::Plain),
    }
}

const KEYWORDS: &[&str] = &[
    "fn", "let", "mut", "match", "if", "else", "for", "while", "loop", "struct", "enum", "impl",
    "trait", "pub", "mod", "use", "return", "self", "Self", "where", "async", "await", "const",
];

/// Distinct identifiers already in the document (3+ chars) — "words in file" completion.
fn doc_words(src: &str) -> Vec<String> {
    let mut set = std::collections::BTreeSet::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, set: &mut std::collections::BTreeSet<String>| {
        if cur.len() >= 3 && !cur.chars().next().unwrap().is_numeric() {
            set.insert(std::mem::take(cur));
        } else {
            cur.clear();
        }
    };
    for ch in src.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            cur.push(ch);
        } else {
            flush(&mut cur, &mut set);
        }
    }
    flush(&mut cur, &mut set);
    set.into_iter().take(60).collect()
}

/// The word range around `byte` (byte-safe).
pub fn word_range(src: &str, byte: usize) -> (usize, usize) {
    let is_w = |c: char| c.is_alphanumeric() || c == '_';
    let b = byte.min(src.len());
    let mut s = b;
    for (i, ch) in src[..b].char_indices().rev() {
        if is_w(ch) {
            s = i;
        } else {
            break;
        }
    }
    let mut e = b;
    for ch in src[b..].chars() {
        if is_w(ch) {
            e += ch.len_utf8();
        } else {
            break;
        }
    }
    (s, e)
}

pub fn completion() -> CompletionProvider {
    Rc::new(|ctx: &CompletionContext| {
        let mut items: Vec<CompletionItem> = KEYWORDS
            .iter()
            .map(|k| CompletionItem::new(*k, CompletionKind::Keyword))
            .collect();
        items.push(
            CompletionItem::new("println", CompletionKind::Function)
                .insert("println!(\"$1\")$0")
                .detail("macro"),
        );
        items.push(
            CompletionItem::new("fn", CompletionKind::Snippet)
                .insert("fn ${1:name}($2) {\n    $0\n}")
                .detail("snippet"),
        );
        items.push(
            CompletionItem::new("for", CompletionKind::Snippet)
                .insert("for ${1:item} in ${2:iter} {\n    $0\n}")
                .detail("snippet"),
        );
        for w in doc_words(ctx.src) {
            items.push(CompletionItem::new(w, CompletionKind::Variable));
        }
        items
    })
}

pub fn hover() -> HoverProvider {
    Rc::new(|src: &str, byte: usize| {
        let (s, e) = word_range(src, byte);
        if e <= s {
            return None;
        }
        let w = &src[s..e];
        Some(Hover {
            contents: format!("{w}\n\nidentifier · demo hover\n(a real provider returns docs/types)"),
            range: Some((s, e)),
        })
    })
}

pub fn signature() -> SignatureProvider {
    Rc::new(|_src: &str, _byte: usize| {
        Some(SignatureHelp {
            label: "new(name: &str) -> Self".into(),
            params: vec!["name: &str".into()],
            active: Some(0),
        })
    })
}

pub fn definition() -> DefinitionProvider {
    Rc::new(|src: &str, byte: usize| {
        let (s, e) = word_range(src, byte);
        if e.saturating_sub(s) < 2 {
            return None;
        }
        let w = &src[s..e];
        src.match_indices(w)
            .map(|(i, _)| i)
            .find(|&i| i != s)
            .or_else(|| src.match_indices(w).map(|(i, _)| i).next())
    })
}

pub fn format() -> FormatProvider {
    Rc::new(|src: &str| {
        let mut out: String = src.lines().map(|l| l.trim_end()).collect::<Vec<_>>().join("\n");
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out
    })
}

/// Diagnostics derived from the text: flag leftover TODO/FIXME and `unwrap`.
pub fn compute_diagnostics(src: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for pat in ["TODO", "FIXME"] {
        for (i, _) in src.match_indices(pat) {
            out.push(Diagnostic {
                range: (i, i + pat.len()),
                severity: Severity::Warning,
                message: format!("{pat} left in the code"),
            });
        }
    }
    for (i, _) in src.match_indices("unwrap") {
        out.push(Diagnostic {
            range: (i, i + 6),
            severity: Severity::Info,
            message: "consider handling the error".into(),
        });
    }
    out
}

/// Inlay hints: a `: _` type hint after each `let [mut] <name>`.
pub fn compute_inlays(src: &str) -> Vec<InlayHint> {
    let mut out = Vec::new();
    for (i, _) in src.match_indices("let ") {
        let mut start = i + 4;
        if src[start..].starts_with("mut ") {
            start += 4;
        }
        let mut e = start;
        for ch in src[start..].chars() {
            if ch.is_alphanumeric() || ch == '_' {
                e += ch.len_utf8();
            } else {
                break;
            }
        }
        if e > start {
            out.push(InlayHint { at: e, label: ": _".into() });
        }
    }
    out
}
