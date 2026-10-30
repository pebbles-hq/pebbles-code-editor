//! Pure text scanners behind the editor's in-text decorations: Unicode highlighting
//! (invisible / ambiguous / control characters), clickable links, colour literals and
//! linked-editing tag pairs. All operate on one line (or the document for tags) and return
//! byte ranges; no widgets.

/// Why a character is highlighted (VS Code `editor.unicodeHighlight.*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UnicodeKind {
    /// Zero-width / formatting / non-breaking space characters.
    Invisible,
    /// A character easily confused with an ASCII one (Cyrillic `а`, Greek `ο`, …).
    Ambiguous,
    /// An ASCII/C1 control character (rendered as a symbol, `editor.renderControlCharacters`).
    Control,
}

/// The ASCII character `c` is confusable with, if any (a curated subset of Unicode's
/// confusables for Latin letters, digits and punctuation).
pub(crate) fn confusable_with(c: char) -> Option<char> {
    Some(match c {
        'а' => 'a',
        'е' => 'e',
        'о' => 'o',
        'р' => 'p',
        'с' => 'c',
        'у' => 'y',
        'х' => 'x',
        'і' => 'i',
        'ј' => 'j',
        'ѕ' => 's',
        'ԁ' => 'd',
        'ɡ' => 'g',
        'һ' => 'h',
        'ӏ' => 'l',
        'А' => 'A',
        'В' => 'B',
        'Е' => 'E',
        'К' => 'K',
        'М' => 'M',
        'Н' => 'H',
        'О' => 'O',
        'Р' => 'P',
        'С' => 'C',
        'Т' => 'T',
        'Х' => 'X',
        'Ѕ' => 'S',
        'І' => 'I',
        'Ј' => 'J',
        'ο' => 'o',
        'ν' => 'v',
        'Α' => 'A',
        'Β' => 'B',
        'Ε' => 'E',
        'Ζ' => 'Z',
        'Η' => 'H',
        'Ι' => 'I',
        'Κ' => 'K',
        'Μ' => 'M',
        'Ν' => 'N',
        'Ο' => 'O',
        'Ρ' => 'P',
        'Τ' => 'T',
        'Υ' => 'Y',
        'Χ' => 'X',
        '‐' | '‑' | '‒' | '–' | '−' => '-',
        '‘' | '’' | '‚' | '′' => '\'',
        '“' | '”' | '„' | '″' => '"',
        '；' => ';',
        '：' => ':',
        '，' => ',',
        '．' => '.',
        '（' => '(',
        '）' => ')',
        'ǀ' => '|',
        '∕' | '⁄' => '/',
        _ => return None,
    })
}

/// Classify `c` for Unicode highlighting.
pub(crate) fn unicode_kind(c: char) -> Option<UnicodeKind> {
    match c {
        '\t' | '\n' | '\r' => None,
        '\u{0}'..='\u{1F}' | '\u{7F}'..='\u{9F}' => Some(UnicodeKind::Control),
        '\u{A0}'
        | '\u{AD}'
        | '\u{115F}'
        | '\u{1160}'
        | '\u{180E}'
        | '\u{2000}'..='\u{200F}'
        | '\u{2028}'..='\u{202F}'
        | '\u{205F}'..='\u{206F}'
        | '\u{3000}'
        | '\u{3164}'
        | '\u{FEFF}'
        | '\u{FFA0}'
        | '\u{FFF9}'..='\u{FFFC}' => Some(UnicodeKind::Invisible),
        c if confusable_with(c).is_some() => Some(UnicodeKind::Ambiguous),
        _ => None,
    }
}

/// Every highlighted character of `line` as `(char_col, char, kind)`.
pub(crate) fn unicode_marks(line: &str) -> Vec<(usize, char, UnicodeKind)> {
    line.chars()
        .enumerate()
        .filter_map(|(i, c)| unicode_kind(c).map(|k| (i, c, k)))
        .collect()
}

/// The visible symbol for a control character (`␀`…`␟`, `␡`), as VS Code renders them.
pub(crate) fn control_symbol(c: char) -> char {
    match c as u32 {
        n @ 0..=0x1F => char::from_u32(0x2400 + n).unwrap_or('�'),
        0x7F => '␡',
        _ => '�',
    }
}

/// URL-like links in `line` as byte ranges (`http(s)://`, `file://`, `mailto:`), trimmed of
/// trailing punctuation the way VS Code's link detector does.
pub(crate) fn find_links(line: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut from = 0;
    while from < line.len() {
        let rest = &line[from..];
        let hit = ["https://", "http://", "file://", "mailto:"]
            .iter()
            .filter_map(|p| rest.find(p).map(|i| (i, p.len())))
            .min_by_key(|x| x.0);
        let Some((i, plen)) = hit else { break };
        let start = from + i;
        let tail = &line[start + plen..];
        let stop = tail
            .char_indices()
            .find(|&(_, c)| {
                c.is_whitespace()
                    || matches!(
                        c,
                        '"' | '\'' | '<' | '>' | '`' | '{' | '}' | '|' | '\\' | '^'
                    )
            })
            .map_or(tail.len(), |(j, _)| j);
        let end = start + plen + stop;
        // Trim trailing punctuation and an unbalanced closing paren.
        let mut s = &line[start..end];
        loop {
            let t = s.trim_end_matches(['.', ',', ';', ':', '!', '?', ']']);
            let t = if t.ends_with(')') && t.matches('(').count() < t.matches(')').count() {
                &t[..t.len() - 1]
            } else {
                t
            };
            if t.len() == s.len() {
                break;
            }
            s = t;
        }
        let end = start + s.len();
        if end > start + plen {
            out.push((start, end));
        }
        from = end.max(start + plen);
    }
    out
}

/// A colour literal found in text: byte range + RGBA.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ColorLit {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) rgba: [u8; 4],
}

fn hex_val(s: &str) -> Option<[u8; 4]> {
    let h = |a: &str| u8::from_str_radix(a, 16).ok();
    let d = |c: &str| h(c).map(|v| v * 17);
    Some(match s.len() {
        3 => [d(&s[0..1])?, d(&s[1..2])?, d(&s[2..3])?, 255],
        4 => [d(&s[0..1])?, d(&s[1..2])?, d(&s[2..3])?, d(&s[3..4])?],
        6 => [h(&s[0..2])?, h(&s[2..4])?, h(&s[4..6])?, 255],
        8 => [h(&s[0..2])?, h(&s[2..4])?, h(&s[4..6])?, h(&s[6..8])?],
        _ => return None,
    })
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> [u8; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = (h.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let f = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    [f(r), f(g), f(b)]
}

/// CSS-style colour literals in `line`: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`,
/// `rgb()/rgba()` and `hsl()/hsla()` (VS Code colour decorators).
pub(crate) fn find_colors(line: &str) -> Vec<ColorLit> {
    let mut out = Vec::new();
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'#' && (i == 0 || !(b[i - 1] as char).is_ascii_alphanumeric()) {
            let mut j = i + 1;
            while j < b.len() && (b[j] as char).is_ascii_hexdigit() {
                j += 1;
            }
            let word_end = j >= b.len() || !(b[j] as char).is_ascii_alphanumeric();
            if word_end && let Some(rgba) = hex_val(&line[i + 1..j]) {
                out.push(ColorLit {
                    start: i,
                    end: j,
                    rgba,
                });
                i = j;
                continue;
            }
        }
        for f in ["rgba(", "rgb(", "hsla(", "hsl("] {
            if line[i..].starts_with(f) && (i == 0 || !(b[i - 1] as char).is_ascii_alphanumeric()) {
                if let Some(close) = line[i..].find(')') {
                    let args: Vec<f64> = line[i + f.len()..i + close]
                        .split([',', ' ', '/'])
                        .filter(|s| !s.trim().is_empty())
                        .filter_map(|s| {
                            let s = s.trim();
                            let (n, pct) = match s.strip_suffix('%') {
                                Some(n) => (n, true),
                                None => (s.trim_end_matches("deg"), false),
                            };
                            n.parse::<f64>()
                                .ok()
                                .map(|v| if pct { v / 100.0 } else { v })
                        })
                        .collect();
                    if args.len() >= 3 {
                        let a = args.get(3).copied().unwrap_or(1.0);
                        let a = if a > 1.0 { a / 255.0 } else { a };
                        let rgb = if f.starts_with("rgb") {
                            let c = |v: f64| {
                                (if v <= 1.0 && line[i..i + close].contains('%') {
                                    v * 255.0
                                } else {
                                    v
                                })
                                .round()
                                .clamp(0.0, 255.0) as u8
                            };
                            [c(args[0]), c(args[1]), c(args[2])]
                        } else {
                            hsl_to_rgb(args[0], args[1].min(1.0), args[2].min(1.0))
                        };
                        out.push(ColorLit {
                            start: i,
                            end: i + close + 1,
                            rgba: [
                                rgb[0],
                                rgb[1],
                                rgb[2],
                                (a.clamp(0.0, 1.0) * 255.0).round() as u8,
                            ],
                        });
                        i += close + 1;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    out
}

/// Format an RGBA colour as a hex literal (`#rrggbb`, or `#rrggbbaa` when translucent).
pub(crate) fn to_hex(rgba: [u8; 4]) -> String {
    if rgba[3] == 255 {
        format!("#{:02x}{:02x}{:02x}", rgba[0], rgba[1], rgba[2])
    } else {
        format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            rgba[0], rgba[1], rgba[2], rgba[3]
        )
    }
}

/// Linked editing for markup tags: if `pos` is inside (or at the edge of) a tag NAME, the
/// byte ranges of that name and of its matching open/close tag's name.
pub(crate) fn linked_tag_names(src: &str, pos: usize) -> Option<((usize, usize), (usize, usize))> {
    let is_name = |c: char| c.is_alphanumeric() || matches!(c, '-' | '_' | ':' | '.');
    // Find the name around pos.
    let bytes = src.as_bytes();
    let mut a = pos.min(src.len());
    while a > 0 && src[..a].chars().next_back().is_some_and(is_name) {
        a -= src[..a].chars().next_back().map_or(1, char::len_utf8);
    }
    let mut b = pos.min(src.len());
    while b < src.len() && src[b..].chars().next().is_some_and(is_name) {
        b += src[b..].chars().next().map_or(1, char::len_utf8);
    }
    // Preceded by `<` (open) or `</` (close)?
    let closing = a >= 2 && &src[a - 2..a] == "</";
    let opening = !closing && a >= 1 && bytes[a - 1] == b'<';
    if !closing && !opening {
        return None;
    }
    let name = &src[a..b];
    // Scan tags to pair: a stack of (name, name_range) for open tags.
    let mut stack: Vec<(&str, (usize, usize))> = Vec::new();
    let mut i = 0;
    let mut pairs: Vec<((usize, usize), (usize, usize))> = Vec::new();
    while let Some(off) = src[i..].find('<') {
        let lt = i + off;
        let close = src[lt..].starts_with("</");
        let ns = lt + if close { 2 } else { 1 };
        let mut ne = ns;
        while ne < src.len() && src[ne..].chars().next().is_some_and(is_name) {
            ne += src[ne..].chars().next().map_or(1, char::len_utf8);
        }
        let Some(gt) = src[ne..].find('>').map(|g| ne + g) else {
            break;
        };
        let self_closing = src[..gt].ends_with('/');
        let nm = &src[ns..ne];
        if close {
            if let Some(k) = stack.iter().rposition(|(n, _)| *n == nm) {
                let (_, r) = stack[k];
                stack.truncate(k);
                pairs.push((r, (ns, ne)));
            }
        } else if !self_closing && !src[ns..].starts_with('!') && !src[ns..].starts_with('?') {
            stack.push((nm, (ns, ne)));
        }
        i = gt + 1;
    }
    let here = (a, b);
    let _ = name;
    pairs.into_iter().find_map(|(o, c)| {
        if o == here {
            Some((o, c))
        } else if c == here {
            Some((c, o))
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_highlighting_classifies_characters() {
        let marks = unicode_marks("a\u{200B}b а\u{1}\u{A0}");
        let kinds: Vec<_> = marks.iter().map(|m| (m.0, m.2)).collect();
        assert_eq!(
            kinds,
            vec![
                (1, UnicodeKind::Invisible),
                (4, UnicodeKind::Ambiguous),
                (5, UnicodeKind::Control),
                (6, UnicodeKind::Invisible),
            ]
        );
        assert_eq!(control_symbol('\u{0}'), '␀');
        assert_eq!(control_symbol('\u{1B}'), '␛');
        assert!(unicode_marks("plain ascii\ttext").is_empty());
    }

    #[test]
    fn links_are_found_and_trimmed() {
        let l = "see https://example.com/a(b). and (http://x.io), mailto:me@x.org";
        let found: Vec<&str> = find_links(l).into_iter().map(|(a, b)| &l[a..b]).collect();
        assert_eq!(
            found,
            vec!["https://example.com/a(b)", "http://x.io", "mailto:me@x.org"]
        );
        assert!(find_links("no links here").is_empty());
    }

    #[test]
    fn color_literals() {
        let c = find_colors(
            "a: #fff; b: #11223344; c: rgb(255, 0, 0); d: hsl(120, 100%, 50%); e: #ggg x#123",
        );
        assert_eq!(c.len(), 4);
        assert_eq!(c[0].rgba, [255, 255, 255, 255]);
        assert_eq!(c[1].rgba, [0x11, 0x22, 0x33, 0x44]);
        assert_eq!(c[2].rgba, [255, 0, 0, 255]);
        assert_eq!(c[3].rgba, [0, 255, 0, 255]);
        assert_eq!(to_hex([255, 0, 16, 255]), "#ff0010");
        assert_eq!(to_hex([1, 2, 3, 4]), "#01020304");
    }

    #[test]
    fn linked_tag_pairs() {
        let s = "<div><span>x</span></div>";
        let (here, other) = linked_tag_names(s, 2).unwrap();
        assert_eq!(&s[here.0..here.1], "div");
        assert_eq!(other, (21, 24));
        let (h2, o2) = linked_tag_names(s, 15).unwrap(); // inside </span>
        assert_eq!(&s[h2.0..h2.1], "span");
        assert_eq!(&s[o2.0..o2.1], "span");
        assert!(linked_tag_names(s, 11).is_none()); // in text
        assert!(linked_tag_names("<br/>", 2).is_none());
    }
}
