//! A small built-in Markdown renderer for documentation panes (completion docs, hover):
//! paragraphs, headings, fenced/indented code blocks (syntax-highlighted with the editor's
//! own languages), bullet/numbered lists, rules, and inline code spans, emphasis, strong and
//! links. Not a CommonMark implementation — the subset language servers actually emit.

use pebbles::prelude::*;

use crate::theme::EditorTheme;

/// An inline run.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Inline {
    Text(String),
    Code(String),
    Emph(String),
    Strong(String),
    Link { text: String, url: String },
}

/// A block.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Block {
    Para(Vec<Inline>),
    Heading(u8, Vec<Inline>),
    Code {
        lang: String,
        text: String,
    },
    List {
        ordered: bool,
        items: Vec<Vec<Inline>>,
    },
    Rule,
}

/// Parse inline Markdown.
pub(crate) fn inlines(s: &str) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::new();
    let mut buf = String::new();
    let flush = |buf: &mut String, out: &mut Vec<Inline>| {
        if !buf.is_empty() {
            out.push(Inline::Text(std::mem::take(buf)));
        }
    };
    let mut i = 0;
    let b = s.as_bytes();
    while i < s.len() {
        let rest = &s[i..];
        let c = rest.chars().next().unwrap_or(' ');
        // Backslash escape.
        if c == '\\'
            && let Some(n) = rest[1..].chars().next()
            && n.is_ascii_punctuation()
        {
            buf.push(n);
            i += 1 + n.len_utf8();
            continue;
        }
        if c == '`' {
            let ticks = rest.bytes().take_while(|&x| x == b'`').count();
            let fence = &rest[..ticks];
            if let Some(end) = rest[ticks..].find(fence) {
                flush(&mut buf, &mut out);
                let code = &rest[ticks..ticks + end];
                let code = if code.starts_with(' ') && code.ends_with(' ') && code.len() > 1 {
                    &code[1..code.len() - 1]
                } else {
                    code
                };
                out.push(Inline::Code(code.to_string()));
                i += ticks + end + ticks;
                continue;
            }
        }
        if (c == '*' || c == '_') && i + 1 < s.len() {
            let strong = b.get(i + 1) == Some(&(c as u8));
            let d = if strong { 2 } else { 1 };
            let delim = &rest[..d];
            if let Some(end) = rest[d..].find(delim)
                && end > 0
                && !rest[d..].starts_with(' ')
            {
                // `_` inside a word (snake_case) is not emphasis.
                let inword =
                    c == '_' && i > 0 && s[..i].chars().last().is_some_and(char::is_alphanumeric);
                if !inword {
                    flush(&mut buf, &mut out);
                    let t = rest[d..d + end].to_string();
                    out.push(if strong {
                        Inline::Strong(t)
                    } else {
                        Inline::Emph(t)
                    });
                    i += d + end + d;
                    continue;
                }
            }
        }
        if c == '['
            && let Some(close) = rest.find("](")
            && let Some(end) = rest[close + 2..].find(')')
            && !rest[1..close].contains('\n')
        {
            flush(&mut buf, &mut out);
            out.push(Inline::Link {
                text: rest[1..close].to_string(),
                url: rest[close + 2..close + 2 + end].to_string(),
            });
            i += close + 2 + end + 1;
            continue;
        }
        buf.push(c);
        i += c.len_utf8();
    }
    flush(&mut buf, &mut out);
    out
}

fn list_marker(line: &str) -> Option<(bool, &str)> {
    let t = line.trim_start();
    for m in ["- ", "* ", "+ "] {
        if let Some(r) = t.strip_prefix(m) {
            return Some((false, r));
        }
    }
    let digits = t.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 {
        let r = &t[digits..];
        if let Some(r) = r.strip_prefix(". ").or_else(|| r.strip_prefix(") ")) {
            return Some((true, r));
        }
    }
    None
}

/// Parse Markdown into blocks.
pub(crate) fn parse(md: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let lines: Vec<&str> = md.lines().collect();
    let mut i = 0;
    let mut para: Vec<&str> = Vec::new();
    let flush = |para: &mut Vec<&str>, out: &mut Vec<Block>| {
        if !para.is_empty() {
            out.push(Block::Para(inlines(&para.join(" "))));
            para.clear();
        }
    };
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            flush(&mut para, &mut out);
            let fence = &t[..3];
            let lang = t[3..].trim().to_string();
            let mut body = Vec::new();
            i += 1;
            while i < lines.len() && !lines[i].trim_start().starts_with(fence) {
                body.push(lines[i]);
                i += 1;
            }
            i += 1;
            out.push(Block::Code {
                lang,
                text: body.join("\n"),
            });
            continue;
        }
        if line.trim().is_empty() {
            flush(&mut para, &mut out);
            i += 1;
            continue;
        }
        if para.is_empty() && (line.starts_with("    ") || line.starts_with('\t')) {
            let mut body = Vec::new();
            while i < lines.len()
                && (lines[i].starts_with("    ")
                    || lines[i].starts_with('\t')
                    || lines[i].trim().is_empty())
            {
                body.push(
                    lines[i]
                        .strip_prefix("    ")
                        .or(lines[i].strip_prefix('\t'))
                        .unwrap_or(""),
                );
                i += 1;
            }
            while body.last().is_some_and(|l| l.is_empty()) {
                body.pop();
            }
            out.push(Block::Code {
                lang: String::new(),
                text: body.join("\n"),
            });
            continue;
        }
        if t.starts_with('#') {
            let level = t.bytes().take_while(|&b| b == b'#').count();
            if level <= 6 && t[level..].starts_with(' ') {
                flush(&mut para, &mut out);
                out.push(Block::Heading(level as u8, inlines(t[level..].trim())));
                i += 1;
                continue;
            }
        }
        if matches!(t.trim_end(), "---" | "***" | "___") {
            flush(&mut para, &mut out);
            out.push(Block::Rule);
            i += 1;
            continue;
        }
        if let Some((ordered, first)) = list_marker(line) {
            flush(&mut para, &mut out);
            let mut items = vec![first.to_string()];
            i += 1;
            while i < lines.len() && !lines[i].trim().is_empty() {
                if let Some((_, r)) = list_marker(lines[i]) {
                    items.push(r.to_string());
                } else if let Some(last) = items.last_mut() {
                    last.push(' ');
                    last.push_str(lines[i].trim());
                }
                i += 1;
            }
            out.push(Block::List {
                ordered,
                items: items.iter().map(|s| inlines(s)).collect(),
            });
            continue;
        }
        para.push(line.trim());
        i += 1;
    }
    flush(&mut para, &mut out);
    out
}

fn ui_span(t: &str, ui: Option<&str>, fs: f32, color: Color) -> TextSpan {
    let s = span(t.to_string()).size(fs).color(color);
    match ui {
        Some(u) => s.font_family(u),
        None => s,
    }
}

fn spans(
    inl: &[Inline],
    theme: &EditorTheme,
    fs: f32,
    mono: &str,
    ui: Option<&str>,
) -> Vec<TextSpan> {
    inl.iter()
        .map(|x| match x {
            Inline::Text(t) => ui_span(t, ui, fs, theme.foreground),
            Inline::Code(t) => span(t.clone())
                .font_family(mono)
                .size(fs * 0.95)
                .color(theme.foreground)
                .chip(theme.current_line),
            Inline::Emph(t) => ui_span(t, ui, fs, theme.foreground).italic(),
            Inline::Strong(t) => ui_span(t, ui, fs, theme.foreground).bold(),
            Inline::Link { text, url } => ui_span(text, ui, fs, theme.accent)
                .underline()
                .link(url.clone()),
        })
        .collect()
}

/// Highlight `code` as `lang` (an info string; empty = plain) into colored spans.
pub(crate) fn code_spans(
    code: &str,
    lang: &str,
    theme: &EditorTheme,
    fs: f32,
    mono: &str,
) -> Vec<TextSpan> {
    let tokens = if lang.is_empty() {
        Vec::new()
    } else {
        crate::lang::by_name(lang).highlight(code)
    };
    let mut out = Vec::new();
    let mut at = 0;
    let plain = |s: &str| {
        span(s.to_string())
            .font_family(mono)
            .size(fs)
            .color(theme.foreground)
    };
    for t in tokens {
        let (a, b) = (t.start.min(code.len()), (t.start + t.len).min(code.len()));
        if a < at || !code.is_char_boundary(a) || !code.is_char_boundary(b) {
            continue;
        }
        if a > at {
            out.push(plain(&code[at..a]));
        }
        let st = theme.style(t.kind);
        let mut s = span(code[a..b].to_string())
            .font_family(mono)
            .size(fs)
            .color(st.color);
        if st.bold {
            s = s.bold();
        }
        if st.italic {
            s = s.italic();
        }
        out.push(s);
        at = b;
    }
    if at < code.len() {
        out.push(plain(&code[at..]));
    }
    out
}

/// Render Markdown as a column of widgets.
pub(crate) fn render(md: &str, theme: &EditorTheme, fs: f64, mono: &str) -> AnyWidget {
    let ui = theme.ui_font_family.as_deref();
    let f = fs as f32;
    let mut kids: Vec<AnyWidget> = Vec::new();
    for (n, b) in parse(md).into_iter().enumerate() {
        if n > 0 {
            kids.push(gap_h(6.0).into_widget());
        }
        kids.push(match b {
            Block::Para(inl) => text_rich(spans(&inl, theme, f, mono, ui)).into_widget(),
            Block::Heading(level, inl) => {
                let size = f * if level <= 1 {
                    1.25
                } else if level == 2 {
                    1.12
                } else {
                    1.0
                };
                text_rich(
                    spans(&inl, theme, size, mono, ui)
                        .into_iter()
                        .map(|s| s.bold())
                        .collect(),
                )
                .into_widget()
            }
            Block::Code { lang, text } => container()
                .padding(EdgeInsets::symmetric(6.0, 4.0))
                .decoration(
                    BoxDecoration::new()
                        .color(theme.current_line)
                        .radius(BorderRadius::all(4.0)),
                )
                .child(text_rich(code_spans(&text, &lang, theme, f * 0.95, mono)))
                .into_widget(),
            Block::List { ordered, items } => {
                let rows: Vec<AnyWidget> = items
                    .iter()
                    .enumerate()
                    .map(|(k, it)| {
                        let bullet = if ordered {
                            format!("{}. ", k + 1)
                        } else {
                            "• ".to_string()
                        };
                        let mut sp = vec![ui_span(&bullet, ui, f, theme.muted)];
                        sp.extend(spans(it, theme, f, mono, ui));
                        text_rich(sp).into_widget()
                    })
                    .collect();
                column(rows)
                    .cross_axis_alignment(CrossAxisAlignment::Start)
                    .into_widget()
            }
            Block::Rule => container()
                .height(1.0)
                .decoration(BoxDecoration::new().color(theme.border))
                .into_widget(),
        });
    }
    column(kids)
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .main_axis_size(MainAxisSize::Min)
        .into_widget()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_blocks_and_inlines_parse() {
        let md = "# Title\n\nSome *emph* and **strong** with `code` and [a link](https://x.y).\nsnake_case_name stays.\n\n```rust\nfn main() {}\n```\n\n- one\n- two `x`\n\n1. first\n2. second\n\n---\n\n    indented";
        let b = parse(md);
        assert_eq!(b[0], Block::Heading(1, vec![Inline::Text("Title".into())]));
        let Block::Para(p) = &b[1] else {
            panic!("{b:?}")
        };
        assert!(p.contains(&Inline::Emph("emph".into())));
        assert!(p.contains(&Inline::Strong("strong".into())));
        assert!(p.contains(&Inline::Code("code".into())));
        assert!(p.contains(&Inline::Link {
            text: "a link".into(),
            url: "https://x.y".into()
        }));
        assert!(
            p.iter()
                .any(|i| matches!(i, Inline::Text(t) if t.contains("snake_case_name")))
        );
        assert_eq!(
            b[2],
            Block::Code {
                lang: "rust".into(),
                text: "fn main() {}".into()
            }
        );
        assert!(matches!(&b[3], Block::List { ordered: false, items } if items.len() == 2));
        assert!(matches!(&b[4], Block::List { ordered: true, items } if items.len() == 2));
        assert_eq!(b[5], Block::Rule);
        assert_eq!(
            b[6],
            Block::Code {
                lang: String::new(),
                text: "indented".into()
            }
        );
    }

    #[test]
    fn code_blocks_are_highlighted() {
        let theme = EditorTheme::dark();
        let sp = code_spans("fn main() {}", "rust", &theme, 12.0, "mono");
        let joined: String = sp.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(joined, "fn main() {}");
        let kw = sp.iter().find(|s| s.text == "fn").expect("keyword run");
        assert_eq!(
            kw.color,
            Some(theme.style(crate::lang::TokenKind::Keyword).color)
        );
    }
}
