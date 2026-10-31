//! Emmet abbreviations for HTML and CSS (VS Code's built-in Emmet): `ul>li.item$*3`,
//! `div#main.box>p{Hello}+a[href=#]`, `m10` → `margin: 10px;`. A compact implementation of
//! the common syntax — elements with `#id`, `.class`, `[attr=value]`, `{text}`, `*N` with
//! `$` numbering, and the `>` child, `+` sibling and `^` climb-up operators.

/// One parsed element.
#[derive(Clone, Debug, Default)]
struct Node {
    tag: String,
    id: Option<String>,
    classes: Vec<String>,
    attrs: Vec<(String, String)>,
    text: Option<String>,
    mult: usize,
    children: Vec<usize>,
}

const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track",
    "wbr",
];

/// The abbreviation ending at `caret`: back to whitespace (outside `{…}` / `[…]`) or the
/// line start. Returns its start byte.
pub(crate) fn abbreviation_start(src: &str, caret: usize) -> usize {
    let mut depth = 0i32;
    let mut start = caret;
    for (i, c) in src[..caret].char_indices().rev() {
        match c {
            '}' | ']' => depth += 1,
            '{' | '[' => depth -= 1,
            '\n' => break,
            c if c.is_whitespace() && depth <= 0 => break,
            _ => {}
        }
        start = i;
    }
    start
}

fn parse_element(s: &str) -> Option<Node> {
    let mut n = Node {
        mult: 1,
        ..Default::default()
    };
    let b = s.as_bytes();
    let mut i = 0;
    let ident = |i: &mut usize| {
        let st = *i;
        while *i < b.len()
            && (b[*i].is_ascii_alphanumeric() || matches!(b[*i], b'-' | b'_' | b'$' | b':'))
        {
            *i += 1;
        }
        s[st..*i].to_string()
    };
    n.tag = ident(&mut i);
    while i < b.len() {
        match b[i] {
            b'#' => {
                i += 1;
                n.id = Some(ident(&mut i));
            }
            b'.' => {
                i += 1;
                n.classes.push(ident(&mut i));
            }
            b'[' => {
                let end = s[i..].find(']')? + i;
                for part in s[i + 1..end].split_whitespace() {
                    let (k, v) = part.split_once('=').unwrap_or((part, ""));
                    n.attrs
                        .push((k.to_string(), v.trim_matches(['"', '\'']).to_string()));
                }
                i = end + 1;
            }
            b'{' => {
                let end = s[i..].find('}')? + i;
                n.text = Some(s[i + 1..end].to_string());
                i = end + 1;
            }
            b'*' => {
                i += 1;
                let st = i;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
                n.mult = s[st..i]
                    .parse()
                    .ok()
                    .filter(|&m: &usize| m > 0 && m <= 1000)?;
            }
            _ => return None,
        }
    }
    if n.tag.is_empty() {
        if n.id.is_none() && n.classes.is_empty() && n.attrs.is_empty() && n.text.is_none() {
            return None;
        }
        n.tag = "div".to_string();
    }
    if !n.tag.chars().next()?.is_ascii_alphabetic() {
        return None;
    }
    Some(n)
}

/// Split at the top-level operators, keeping each element's preceding operator.
fn split_ops(abbr: &str) -> Option<Vec<(char, &str)>> {
    let mut out = Vec::new();
    let (mut depth, mut st, mut op) = (0i32, 0usize, ' ');
    for (i, c) in abbr.char_indices() {
        match c {
            '{' | '[' => depth += 1,
            '}' | ']' => depth -= 1,
            '>' | '+' | '^' if depth == 0 => {
                out.push((op, &abbr[st..i]));
                op = c;
                st = i + 1;
            }
            _ => {}
        }
    }
    out.push((op, &abbr[st..]));
    Some(out)
}

fn number(s: &str, i: usize) -> String {
    s.replace('$', &i.to_string())
}

fn render(nodes: &[Node], id: usize, idx: usize, depth: usize, unit: &str, out: &mut String) {
    let n = &nodes[id];
    let pad = unit.repeat(depth);
    let mut open = format!("<{}", n.tag);
    if let Some(v) = &n.id {
        open += &format!(" id=\"{}\"", number(v, idx));
    }
    if !n.classes.is_empty() {
        let c: Vec<String> = n.classes.iter().map(|c| number(c, idx)).collect();
        open += &format!(" class=\"{}\"", c.join(" "));
    }
    let mut attrs = n.attrs.clone();
    if attrs.is_empty() {
        match n.tag.as_str() {
            "a" => attrs.push(("href".into(), String::new())),
            "img" => {
                attrs.push(("src".into(), String::new()));
                attrs.push(("alt".into(), String::new()));
            }
            "input" => attrs.push(("type".into(), "text".into())),
            "link" => {
                attrs.push(("rel".into(), "stylesheet".into()));
                attrs.push(("href".into(), String::new()));
            }
            _ => {}
        }
    }
    for (k, v) in &attrs {
        open += &format!(" {}=\"{}\"", k, number(v, idx));
    }
    open.push('>');
    out.push_str(&pad);
    out.push_str(&open);
    if VOID.contains(&n.tag.as_str()) {
        out.push('\n');
        return;
    }
    let text = n
        .text
        .as_deref()
        .map(|t| number(t, idx))
        .unwrap_or_default();
    if n.children.is_empty() {
        out.push_str(&text);
        out.push_str(&format!("</{}>\n", n.tag));
        return;
    }
    out.push('\n');
    if !text.is_empty() {
        out.push_str(&format!("{pad}{unit}{text}\n"));
    }
    for &c in &n.children {
        for k in 1..=nodes[c].mult {
            render(nodes, c, k, depth + 1, unit, out);
        }
    }
    out.push_str(&format!("{pad}</{}>\n", n.tag));
}

/// Expand an HTML abbreviation (`unit` = one indent). Returns the markup and the caret
/// offset (the first empty attribute value or element body).
pub fn expand_html(abbr: &str, unit: &str) -> Option<(String, usize)> {
    if abbr.is_empty() || abbr.contains(char::is_whitespace) && !abbr.contains('{') {
        return None;
    }
    let mut nodes: Vec<Node> = Vec::new();
    let mut roots: Vec<usize> = Vec::new();
    let mut stack: Vec<Option<usize>> = vec![None];
    let mut last: Option<usize> = None;
    for (op, part) in split_ops(abbr)? {
        let node = parse_element(part)?;
        match op {
            '>' => stack.push(Some(last?)),
            '^' => {
                if stack.len() > 1 {
                    stack.pop();
                }
            }
            _ => {}
        }
        let id = nodes.len();
        nodes.push(node);
        match *stack.last()? {
            Some(p) => nodes[p].children.push(id),
            None => roots.push(id),
        }
        last = Some(id);
    }
    let mut out = String::new();
    for &r in &roots {
        for k in 1..=nodes[r].mult {
            render(&nodes, r, k, 0, unit, &mut out);
        }
    }
    if out.ends_with('\n') {
        out.pop();
    }
    let caret = [out.find("\"\""), out.find("></")]
        .into_iter()
        .flatten()
        .min()
        .map_or(out.len(), |i| i + 1);
    Some((out, caret))
}

/// CSS property abbreviations (Emmet's most used).
const CSS: &[(&str, &str)] = &[
    ("m", "margin"),
    ("mt", "margin-top"),
    ("mr", "margin-right"),
    ("mb", "margin-bottom"),
    ("ml", "margin-left"),
    ("p", "padding"),
    ("pt", "padding-top"),
    ("pr", "padding-right"),
    ("pb", "padding-bottom"),
    ("pl", "padding-left"),
    ("w", "width"),
    ("h", "height"),
    ("maw", "max-width"),
    ("mah", "max-height"),
    ("miw", "min-width"),
    ("mih", "min-height"),
    ("t", "top"),
    ("r", "right"),
    ("b", "bottom"),
    ("l", "left"),
    ("z", "z-index"),
    ("fz", "font-size"),
    ("fw", "font-weight"),
    ("lh", "line-height"),
    ("c", "color"),
    ("bg", "background"),
    ("bgc", "background-color"),
    ("bd", "border"),
    ("bdrs", "border-radius"),
    ("op", "opacity"),
    ("gap", "gap"),
    ("ta", "text-align"),
    ("pos", "position"),
    ("d", "display"),
    ("ov", "overflow"),
    ("cur", "cursor"),
];

/// Keyword abbreviations with a fixed value.
const CSS_KW: &[(&str, &str)] = &[
    ("df", "display: flex;"),
    ("dg", "display: grid;"),
    ("db", "display: block;"),
    ("dib", "display: inline-block;"),
    ("dn", "display: none;"),
    ("posa", "position: absolute;"),
    ("posr", "position: relative;"),
    ("posf", "position: fixed;"),
    ("tac", "text-align: center;"),
    ("fwb", "font-weight: bold;"),
    ("ovh", "overflow: hidden;"),
    ("curp", "cursor: pointer;"),
];

/// Expand a CSS abbreviation: `m10` → `margin: 10px;`, `w50p` → `width: 50%;`, `df` →
/// `display: flex;`. Returns the declaration and the caret offset.
pub fn expand_css(abbr: &str) -> Option<(String, usize)> {
    if let Some((_, v)) = CSS_KW.iter().find(|(k, _)| *k == abbr) {
        return Some((v.to_string(), v.len()));
    }
    let split = abbr
        .find(|c: char| c.is_ascii_digit() || c == '-')
        .unwrap_or(abbr.len());
    let (name, val) = abbr.split_at(split);
    let prop = CSS.iter().find(|(k, _)| *k == name)?.1;
    if val.is_empty() {
        let s = format!("{prop}: ;");
        return Some((s, prop.len() + 2));
    }
    let num_end = val
        .find(|c: char| !(c.is_ascii_digit() || c == '-' || c == '.'))
        .unwrap_or(val.len());
    let (num, unit) = val.split_at(num_end);
    num.parse::<f64>().ok()?;
    let unit = match unit {
        "" if num == "0" || prop == "z-index" || prop == "opacity" || prop == "font-weight" => "",
        "" => "px",
        "p" => "%",
        "e" => "em",
        "r" => "rem",
        "x" => "ex",
        u if u.chars().all(|c| c.is_ascii_alphabetic()) => u,
        _ => return None,
    };
    let s = format!("{prop}: {num}{unit};");
    let len = s.len();
    Some((s, len))
}

/// Expand the abbreviation before `caret` for `language`: `(start, text, caret_offset)`.
pub(crate) fn expand_at(
    src: &str,
    caret: usize,
    language: &str,
    unit: &str,
) -> Option<(usize, String, usize)> {
    let start = abbreviation_start(src, caret);
    let abbr = &src[start..caret];
    let (text, off) = match language.to_ascii_lowercase().as_str() {
        "html" | "htm" | "xml" | "vue" | "svelte" | "jsx" | "tsx" => expand_html(abbr, unit)?,
        "css" | "scss" | "less" => expand_css(abbr)?,
        _ => return None,
    };
    // Continuation lines take the abbreviation line's indentation.
    let ls = src[..start].rfind('\n').map_or(0, |i| i + 1);
    let lead: String = src[ls..start]
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let mut caret_off = off;
    let indented = if lead.is_empty() {
        text
    } else {
        let before = text[..off].matches('\n').count();
        caret_off += before * lead.len();
        text.replace('\n', &format!("\n{lead}"))
    };
    Some((start, indented, caret_off))
}

/// [`expand_at`] as a snippet body: every empty attribute value and element body becomes a
/// tab stop (`$1`, `$2`, …) so Tab walks through them; for CSS the caret slot is `$0`.
/// Returns `(start, body)`.
pub(crate) fn expand_at_snippet(
    src: &str,
    caret: usize,
    language: &str,
    unit: &str,
) -> Option<(usize, String)> {
    let (start, text, off) = expand_at(src, caret, language, unit)?;
    let esc = |s: &str| {
        s.replace('\\', "\\\\")
            .replace('$', "\\$")
            .replace('}', "\\}")
    };
    let html = !matches!(
        language.to_ascii_lowercase().as_str(),
        "css" | "scss" | "less"
    );
    let mut slots: Vec<usize> = Vec::new();
    if html {
        slots.extend(text.match_indices("\"\"").map(|(i, _)| i + 1));
        slots.extend(text.match_indices("></").map(|(i, _)| i + 1));
        slots.sort_unstable();
    }
    let mut body = String::new();
    let mut at = 0;
    if slots.is_empty() {
        if off < text.len() {
            body.push_str(&esc(&text[..off]));
            body.push_str("$0");
            body.push_str(&esc(&text[off..]));
        } else {
            body.push_str(&esc(&text));
        }
        return Some((start, body));
    }
    for (k, &s) in slots.iter().enumerate() {
        body.push_str(&esc(&text[at..s]));
        body.push_str(&format!("${}", k + 1));
        at = s;
    }
    body.push_str(&esc(&text[at..]));
    Some((start, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_abbreviations_expand() {
        let (s, c) = expand_html("ul>li.item$*3", "  ").unwrap();
        assert_eq!(
            s,
            "<ul>\n  <li class=\"item1\"></li>\n  <li class=\"item2\"></li>\n  <li class=\"item3\"></li>\n</ul>"
        );
        assert_eq!(&s[..c], "<ul>\n  <li class=\"item1\">");
        let (s, _) = expand_html("div#main.a.b>p{Hi}+a[href=#x]", "\t").unwrap();
        assert_eq!(
            s,
            "<div id=\"main\" class=\"a b\">\n\t<p>Hi</p>\n\t<a href=\"#x\"></a>\n</div>"
        );
        let (s, _) = expand_html("div>p^span", "\t").unwrap();
        assert_eq!(s, "<div>\n\t<p></p>\n</div>\n<span></span>");
        assert_eq!(
            expand_html(".x", "\t").unwrap().0,
            "<div class=\"x\"></div>"
        );
        assert_eq!(
            expand_html("img", "\t").unwrap().0,
            "<img src=\"\" alt=\"\">"
        );
        assert!(expand_html("1abc", "\t").is_none());
        assert!(expand_html("a b", "\t").is_none());
    }

    #[test]
    fn css_abbreviations_expand() {
        assert_eq!(expand_css("m10").unwrap().0, "margin: 10px;");
        assert_eq!(expand_css("w50p").unwrap().0, "width: 50%;");
        assert_eq!(expand_css("p0").unwrap().0, "padding: 0;");
        assert_eq!(expand_css("df").unwrap().0, "display: flex;");
        assert_eq!(expand_css("c"), Some(("color: ;".to_string(), 7)));
        assert!(expand_css("zz9").is_none());
    }

    #[test]
    fn emmet_expansions_become_snippets_with_tab_stops() {
        let (st, body) = expand_at_snippet("x a", 3, "html", "  ").unwrap();
        assert_eq!(st, 2);
        assert_eq!(body, "<a href=\"$1\">$2</a>");
        let (_, body) = expand_at_snippet("ul>li*2", 7, "html", "  ").unwrap();
        assert_eq!(body, "<ul>\n  <li>$1</li>\n  <li>$2</li>\n</ul>");
        let (_, body) = expand_at_snippet("df", 2, "css", "  ").unwrap();
        assert_eq!(body, "display: flex;");
    }

    #[test]
    fn expand_at_finds_the_abbreviation_and_indents() {
        let src = "<body>\n  ul>li*2";
        let (start, text, _) = expand_at(src, src.len(), "HTML", "  ").unwrap();
        assert_eq!(start, 9);
        assert_eq!(text, "<ul>\n    <li></li>\n    <li></li>\n  </ul>");
    }
}
