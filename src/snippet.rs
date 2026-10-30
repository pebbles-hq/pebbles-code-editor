//! **Snippet engine**: the LSP / TextMate snippet grammar and live snippet sessions.
//!
//! Grammar (LSP 3.17 "Snippet Syntax"): `$1` / `${1}` tab stops, `${1:placeholder}` (nested
//! snippets allowed), `${1|one,two|}` choices, `$0` final stop, repeated indices as mirrors,
//! variables `$NAME` / `${NAME:default}` / `${NAME/regex/format/flags}` (TM_FILENAME,
//! CURRENT_YEAR, UUID, …), and `\` escapes for `$`, `}`, `\` (plus `,` `|` inside choices).
//!
//! Inserting a snippet starts a [`SnippetSession`]: the placeholder of the first stop is
//! selected (mirrors get extra cursors, so they update live as you type), Tab / Shift+Tab move
//! between stops, choices open a pick list, and the session ends at `$0`, on Esc, or when the
//! caret leaves the snippet.

use std::collections::BTreeMap;

use std::cell::RefCell;
use std::rc::Rc;

use pebbles::prelude::Signal;

use crate::commands::{clear_recorded, dispatch};
use crate::edit::{
    Assoc, Change, ChangeSet, Coalesce, EditorState, History, Selection, Selections, Transaction,
};

/// A regex transform `/regex/format/flags`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Transform {
    pub(crate) regex: String,
    pub(crate) format: String,
    pub(crate) flags: String,
}

/// A parsed snippet element.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Node {
    Text(String),
    /// `$n`, `${n:placeholder}`, `${n|a,b|}` (a choice), `${n/re/fmt/}`.
    Tabstop {
        index: u32,
        placeholder: Vec<Node>,
        choices: Vec<String>,
        transform: Option<Transform>,
    },
    /// `$NAME`, `${NAME:default}`, `${NAME/re/fmt/flags}`.
    Variable {
        name: String,
        default: Option<Vec<Node>>,
        transform: Option<Transform>,
    },
}

struct Parser {
    s: Vec<char>,
    i: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }
    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn int(&mut self) -> Option<u32> {
        let st = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        (self.i > st).then(|| self.s[st..self.i].iter().collect::<String>().parse().ok())?
    }
    fn var(&mut self) -> Option<String> {
        let st = self.i;
        if !self
            .peek()
            .is_some_and(|c| c == '_' || c.is_ascii_alphabetic())
        {
            return None;
        }
        while self
            .peek()
            .is_some_and(|c| c == '_' || c.is_ascii_alphanumeric())
        {
            self.i += 1;
        }
        Some(self.s[st..self.i].iter().collect())
    }
    /// Raw text up to an unescaped `end` (escapes of `end` and `\` are kept as written for
    /// regex/format strings, except `\/`).
    fn until(&mut self, end: char) -> Option<String> {
        let mut out = String::new();
        while let Some(c) = self.peek() {
            self.i += 1;
            if c == '\\' {
                if let Some(n) = self.peek() {
                    self.i += 1;
                    if n != end {
                        out.push('\\');
                    }
                    out.push(n);
                }
                continue;
            }
            if c == end {
                return Some(out);
            }
            out.push(c);
        }
        None
    }
    /// A format string up to an unescaped `/`, keeping `${…}` blocks (which may contain `/`,
    /// as in `${1:/upcase}`) whole.
    fn format_until_slash(&mut self) -> Option<String> {
        let mut out = String::new();
        while let Some(c) = self.peek() {
            self.i += 1;
            match c {
                '\\' => {
                    if let Some(n) = self.peek() {
                        self.i += 1;
                        if n != '/' {
                            out.push('\\');
                        }
                        out.push(n);
                    }
                }
                '$' if self.peek() == Some('{') => {
                    out.push('$');
                    while let Some(d) = self.peek() {
                        self.i += 1;
                        out.push(d);
                        if d == '}' {
                            break;
                        }
                    }
                }
                '/' => return Some(out),
                c => out.push(c),
            }
        }
        None
    }
    fn transform(&mut self) -> Option<Transform> {
        // After the leading `/`.
        let regex = self.until('/')?;
        let format = self.format_until_slash()?;
        let flags = self.until('}')?;
        Some(Transform {
            regex,
            format,
            flags,
        })
    }
    /// A sequence; inside a placeholder it ends at an unescaped `}` (not consumed).
    fn seq(&mut self, nested: bool) -> Vec<Node> {
        let mut out: Vec<Node> = Vec::new();
        let mut text = String::new();
        while let Some(c) = self.peek() {
            if c == '}' && nested {
                break;
            }
            if c == '\\' {
                if let Some(n) = self.s.get(self.i + 1).copied()
                    && matches!(n, '$' | '}' | '\\')
                {
                    text.push(n);
                    self.i += 2;
                    continue;
                }
                text.push('\\');
                self.i += 1;
                continue;
            }
            if c == '$' {
                let save = self.i;
                self.i += 1;
                if let Some(n) = self.dollar() {
                    if !text.is_empty() {
                        out.push(Node::Text(std::mem::take(&mut text)));
                    }
                    out.push(n);
                    continue;
                }
                self.i = save + 1;
                text.push('$');
                continue;
            }
            text.push(c);
            self.i += 1;
        }
        if !text.is_empty() {
            out.push(Node::Text(text));
        }
        out
    }
    /// After a `$`: a tab stop, placeholder, choice or variable (else `None`, backtrack).
    fn dollar(&mut self) -> Option<Node> {
        if let Some(index) = self.int() {
            return Some(Node::Tabstop {
                index,
                placeholder: Vec::new(),
                choices: Vec::new(),
                transform: None,
            });
        }
        if let Some(name) = self.var() {
            return Some(Node::Variable {
                name,
                default: None,
                transform: None,
            });
        }
        if !self.eat('{') {
            return None;
        }
        if let Some(index) = self.int() {
            if self.eat('}') {
                return Some(Node::Tabstop {
                    index,
                    placeholder: Vec::new(),
                    choices: Vec::new(),
                    transform: None,
                });
            }
            if self.eat(':') {
                let placeholder = self.seq(true);
                return self.eat('}').then_some(Node::Tabstop {
                    index,
                    placeholder,
                    choices: Vec::new(),
                    transform: None,
                });
            }
            if self.eat('|') {
                let mut choices = Vec::new();
                let mut cur = String::new();
                loop {
                    let c = self.peek()?;
                    self.i += 1;
                    match c {
                        '\\' => {
                            let n = self.peek()?;
                            self.i += 1;
                            if !matches!(n, ',' | '|' | '\\' | '$' | '}') {
                                cur.push('\\');
                            }
                            cur.push(n);
                        }
                        ',' => choices.push(std::mem::take(&mut cur)),
                        '|' => {
                            choices.push(std::mem::take(&mut cur));
                            return self.eat('}').then_some(Node::Tabstop {
                                index,
                                placeholder: Vec::new(),
                                choices,
                                transform: None,
                            });
                        }
                        c => cur.push(c),
                    }
                }
            }
            if self.eat('/') {
                let transform = Some(self.transform()?);
                return Some(Node::Tabstop {
                    index,
                    placeholder: Vec::new(),
                    choices: Vec::new(),
                    transform,
                });
            }
            return None;
        }
        let name = self.var()?;
        if self.eat('}') {
            return Some(Node::Variable {
                name,
                default: None,
                transform: None,
            });
        }
        if self.eat(':') {
            let d = self.seq(true);
            return self.eat('}').then_some(Node::Variable {
                name,
                default: Some(d),
                transform: None,
            });
        }
        if self.eat('/') {
            let transform = Some(self.transform()?);
            return Some(Node::Variable {
                name,
                default: None,
                transform,
            });
        }
        None
    }
}

/// Parse a snippet body. Never fails: malformed constructs are literal text.
pub(crate) fn parse(body: &str) -> Vec<Node> {
    let mut p = Parser {
        s: body.chars().collect(),
        i: 0,
    };
    let mut out = p.seq(false);
    out.retain(|n| !matches!(n, Node::Text(t) if t.is_empty()));
    out
}

/// Apply a TextMate format string to one regex match.
fn format(fmt: &str, caps: &regex::Captures) -> String {
    let group = |n: usize| caps.get(n).map_or("", |m| m.as_str()).to_string();
    let mut out = String::new();
    let c: Vec<char> = fmt.chars().collect();
    let mut i = 0;
    while i < c.len() {
        if c[i] == '\\' && i + 1 < c.len() {
            match c[i + 1] {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                x => out.push(x),
            }
            i += 2;
            continue;
        }
        if c[i] == '$' && i + 1 < c.len() {
            if c[i + 1].is_ascii_digit() {
                let mut j = i + 1;
                while j < c.len() && c[j].is_ascii_digit() {
                    j += 1;
                }
                let n: usize = c[i + 1..j].iter().collect::<String>().parse().unwrap_or(0);
                out.push_str(&group(n));
                i = j;
                continue;
            }
            if c[i + 1] == '{' {
                let close = c[i..].iter().position(|&x| x == '}').map(|k| i + k);
                if let Some(close) = close {
                    let inner: String = c[i + 2..close].iter().collect();
                    let (num, rest) = inner
                        .split_once(':')
                        .map_or((inner.as_str(), None), |(a, b)| (a, Some(b)));
                    let n: usize = num.parse().unwrap_or(0);
                    let v = group(n);
                    let s = match rest {
                        None => v,
                        Some("/upcase") => v.to_uppercase(),
                        Some("/downcase") => v.to_lowercase(),
                        Some("/capitalize") => {
                            let mut ch = v.chars();
                            ch.next().map_or(String::new(), |f| {
                                f.to_uppercase().collect::<String>() + ch.as_str()
                            })
                        }
                        Some(r @ ("/camelcase" | "/pascalcase")) => {
                            let mut s = String::new();
                            for (k, w) in v
                                .split(|ch: char| !ch.is_alphanumeric())
                                .filter(|w| !w.is_empty())
                                .enumerate()
                            {
                                let mut ch = w.chars();
                                let f = ch.next().unwrap_or(' ');
                                if k == 0 && r == "/camelcase" {
                                    s.extend(f.to_lowercase());
                                } else {
                                    s.extend(f.to_uppercase());
                                }
                                s.push_str(&ch.as_str().to_lowercase());
                            }
                            s
                        }
                        Some(r) if r.starts_with('+') => {
                            if v.is_empty() {
                                String::new()
                            } else {
                                r[1..].to_string()
                            }
                        }
                        Some(r) if r.starts_with('?') => {
                            let (a, b) = r[1..].split_once(':').unwrap_or((&r[1..], ""));
                            if v.is_empty() {
                                b.to_string()
                            } else {
                                a.to_string()
                            }
                        }
                        Some(r) => {
                            let e = r.strip_prefix('-').unwrap_or(r);
                            if v.is_empty() { e.to_string() } else { v }
                        }
                    };
                    out.push_str(&s);
                    i = close + 1;
                    continue;
                }
            }
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

/// Apply a `/regex/format/flags` transform to `value`.
pub(crate) fn apply_transform(value: &str, t: &Transform) -> String {
    let pat = if t.flags.contains('i') {
        format!("(?i){}", t.regex)
    } else {
        t.regex.clone()
    };
    let Ok(re) = regex::Regex::new(&pat) else {
        return value.to_string();
    };
    let rep = |caps: &regex::Captures| format(&t.format, caps);
    if t.flags.contains('g') {
        re.replace_all(value, rep).into_owned()
    } else {
        re.replace(value, rep).into_owned()
    }
}

/// A tab stop of an expanded snippet: its byte ranges (several = mirrors) and choices.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Stop {
    pub(crate) index: u32,
    pub(crate) ranges: Vec<(usize, usize)>,
    pub(crate) choices: Vec<String>,
}

/// An expanded snippet: the text and its stops in visiting order (`$1`, `$2`, …, `$0` last).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Expanded {
    pub(crate) text: String,
    pub(crate) stops: Vec<Stop>,
}

struct Expander<'a> {
    vars: &'a dyn Fn(&str) -> Option<String>,
    defs: BTreeMap<u32, Vec<Node>>,
    choices: BTreeMap<u32, Vec<String>>,
    transforms: BTreeMap<u32, Transform>,
    stops: BTreeMap<u32, Vec<(usize, usize)>>,
    next_unknown: u32,
}

impl Expander<'_> {
    fn collect(&mut self, nodes: &[Node]) {
        for n in nodes {
            match n {
                Node::Tabstop {
                    index,
                    placeholder,
                    choices,
                    ..
                } => {
                    if !placeholder.is_empty() && !self.defs.contains_key(index) {
                        self.defs.insert(*index, placeholder.clone());
                    }
                    if !choices.is_empty() && !self.choices.contains_key(index) {
                        self.choices.insert(*index, choices.clone());
                    }
                    self.collect(placeholder);
                }
                Node::Variable {
                    default: Some(d), ..
                } => self.collect(d),
                _ => {}
            }
        }
    }
    fn render(&mut self, nodes: &[Node], out: &mut String, record: bool) {
        for n in nodes {
            match n {
                Node::Text(t) => out.push_str(t),
                Node::Tabstop {
                    index, transform, ..
                } => {
                    let start = out.len();
                    if let Some(c) = self.choices.get(index) {
                        out.push_str(c.first().map_or("", String::as_str));
                    } else if let Some(d) = self.defs.get(index).cloned() {
                        // Nested stops are recorded only inside the first (defining) copy.
                        let first = !self.stops.contains_key(index);
                        self.render(&d, out, record && first);
                    }
                    if let Some(t) = transform {
                        let v = out[start..].to_string();
                        out.truncate(start);
                        out.push_str(&apply_transform(&v, t));
                        self.transforms.insert(*index, t.clone());
                    }
                    if record {
                        self.stops
                            .entry(*index)
                            .or_default()
                            .push((start, out.len()));
                    }
                }
                Node::Variable {
                    name,
                    default,
                    transform,
                } => match (self.vars)(name) {
                    Some(v) if !v.is_empty() || default.is_none() => {
                        let v = transform
                            .as_ref()
                            .map_or(v.clone(), |t| apply_transform(&v, t));
                        out.push_str(&v);
                    }
                    Some(_) => self.render(default.as_deref().unwrap_or(&[]), out, record),
                    None => {
                        if let Some(d) = default {
                            self.render(d, out, record);
                        } else {
                            // Unknown variable: its name becomes a placeholder (VS Code).
                            let start = out.len();
                            out.push_str(name);
                            if record {
                                self.stops
                                    .insert(self.next_unknown, vec![(start, out.len())]);
                                self.next_unknown += 1;
                            }
                        }
                    }
                },
            }
        }
    }
}

/// Expand parsed nodes: variables resolved with `vars` (`None` = unknown variable).
pub(crate) fn expand(nodes: &[Node], vars: &dyn Fn(&str) -> Option<String>) -> Expanded {
    let mut e = Expander {
        vars,
        defs: BTreeMap::new(),
        choices: BTreeMap::new(),
        transforms: BTreeMap::new(),
        stops: BTreeMap::new(),
        next_unknown: 1_000_000,
    };
    e.collect(nodes);
    let mut text = String::new();
    e.render(nodes, &mut text, true);
    let final_stop = e
        .stops
        .remove(&0)
        .unwrap_or_else(|| vec![(text.len(), text.len())]);
    let mut stops: Vec<Stop> = e
        .stops
        .into_iter()
        .map(|(index, mut ranges)| {
            ranges.sort_unstable();
            Stop {
                index,
                ranges,
                choices: e.choices.get(&index).cloned().unwrap_or_default(),
            }
        })
        .collect();
    stops.push(Stop {
        index: 0,
        ranges: final_stop,
        choices: Vec::new(),
    });
    Expanded { text, stops }
}

/// Prefix every continuation line with `lead`, shifting the stops to match.
pub(crate) fn indent(e: &mut Expanded, lead: &str) {
    if lead.is_empty() || !e.text.contains('\n') {
        return;
    }
    let breaks: Vec<usize> = e.text.match_indices('\n').map(|(i, _)| i).collect();
    let shift = |p: usize| p + breaks.iter().filter(|&&b| b < p).count() * lead.len();
    for s in &mut e.stops {
        for r in &mut s.ranges {
            *r = (shift(r.0), shift(r.1));
        }
    }
    e.text = e.text.replace('\n', &format!("\n{lead}"));
}

// ---------------------------------------------------------------------------
// Variables
// ---------------------------------------------------------------------------

/// What the variable resolver knows about the insertion site.
pub(crate) struct VarCtx<'a> {
    pub(crate) uri: &'a str,
    pub(crate) src: &'a str,
    /// The replaced range (its text is `TM_SELECTED_TEXT`).
    pub(crate) from: usize,
    pub(crate) to: usize,
    pub(crate) line_comment: Option<&'a str>,
}

/// UTC civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// The local time zone's UTC offset (seconds) at `unix`, from the TZif database
/// (`$TZ`, else `/etc/localtime`); 0 (UTC) when unavailable.
fn local_offset(unix: i64) -> i64 {
    thread_local!(static ZONE: Option<Vec<u8>> = {
        let tz = std::env::var("TZ").ok().map(|t| t.trim_start_matches(':').to_string());
        let path = match tz.as_deref() {
            Some(t) if t.starts_with('/') => t.to_string(),
            Some(t) if !t.is_empty() => format!("/usr/share/zoneinfo/{t}"),
            _ => "/etc/localtime".to_string(),
        };
        std::fs::read(path).ok()
    });
    ZONE.with(|z| z.as_deref().and_then(|b| tzif_offset(b, unix)).unwrap_or(0))
}

/// The UTC offset at `unix` in a TZif file (RFC 8536): the version-1 (32-bit) block, or
/// the 64-bit block of version 2+ files.
fn tzif_offset(b: &[u8], unix: i64) -> Option<i64> {
    let be32 = |i: usize| -> Option<i64> {
        Some(i64::from(i32::from_be_bytes(
            b.get(i..i + 4)?.try_into().ok()?,
        )))
    };
    let be64 =
        |i: usize| -> Option<i64> { Some(i64::from_be_bytes(b.get(i..i + 8)?.try_into().ok()?)) };
    let header = |at: usize| -> Option<[usize; 6]> {
        if b.get(at..at + 4)? != b"TZif" {
            return None;
        }
        let mut c = [0usize; 6];
        for (k, v) in c.iter_mut().enumerate() {
            *v = be32(at + 20 + 4 * k)? as usize;
        }
        Some(c) // isut, isstd, leap, time, type, char
    };
    let c1 = header(0)?;
    let v1_len = c1[3] * 5 + c1[4] * 6 + c1[5] + c1[2] * 8 + c1[1] + c1[0];
    // Version 2+: skip the v1 block, use the 64-bit one.
    let (at, c, tsz) = match b.get(4) {
        Some(b'2'..=b'9') => {
            let at = 44 + v1_len;
            (at, header(at)?, 8)
        }
        _ => (0, c1, 4),
    };
    let times = at + 44;
    let idxs = times + c[3] * tsz;
    let types = idxs + c[3];
    let off_of = |t: usize| be32(types + t * 6);
    if c[4] == 0 {
        return None;
    }
    let mut ty = None;
    for i in 0..c[3] {
        let t = if tsz == 8 {
            be64(times + i * 8)?
        } else {
            be32(times + i * 4)?
        };
        if t > unix {
            break;
        }
        ty = Some(*b.get(idxs + i)? as usize);
    }
    off_of(ty.unwrap_or(0).min(c[4] - 1))
}

fn rand64() -> u64 {
    use std::cell::Cell;
    thread_local!(static S: Cell<u64> = const { Cell::new(0) });
    S.with(|s| {
        let mut x = s.get();
        if x == 0 {
            x = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0x9E37_79B9_7F4A_7C15, |d| d.as_nanos() as u64)
                | 1;
        }
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        x
    })
}

/// Resolve a snippet variable (`None` = unknown).
pub(crate) fn variable(name: &str, cx: &VarCtx) -> Option<String> {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    const DAYS: [&str; 7] = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    let path = cx
        .uri
        .strip_prefix("file://")
        .unwrap_or(cx.uri)
        .replace("%20", " ");
    let file = path.rsplit('/').next().unwrap_or(&path).to_string();
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    // Date/time variables are local time (VS Code); CURRENT_SECONDS_UNIX stays UTC.
    let secs = unix + local_offset(unix);
    let (y, mo, d) = civil(secs.div_euclid(86_400));
    let tod = secs.rem_euclid(86_400);
    let wd = (secs.div_euclid(86_400) + 4).rem_euclid(7) as usize;
    let ls = cx.src[..cx.from].rfind('\n').map_or(0, |i| i + 1);
    let le = cx.src[cx.from..]
        .find('\n')
        .map_or(cx.src.len(), |i| cx.from + i);
    let line = cx.src[..cx.from].matches('\n').count();
    Some(match name {
        "TM_SELECTED_TEXT" => cx.src[cx.from..cx.to].to_string(),
        "TM_CURRENT_LINE" => cx.src[ls..le].to_string(),
        "TM_CURRENT_WORD" => crate::view::word_range_at(cx.src, cx.from)
            .map_or(String::new(), |(a, b)| cx.src[a..b].to_string()),
        "TM_LINE_INDEX" => line.to_string(),
        "TM_LINE_NUMBER" => (line + 1).to_string(),
        "TM_FILENAME" => file,
        "TM_FILENAME_BASE" => file.split('.').next().unwrap_or("").to_string(),
        "TM_DIRECTORY" => path
            .rsplit_once('/')
            .map_or(String::new(), |(d, _)| d.to_string()),
        "TM_FILEPATH" => path.clone(),
        "RELATIVE_FILEPATH" => file,
        "CLIPBOARD" | "WORKSPACE_NAME" | "WORKSPACE_FOLDER" => String::new(),
        "CURSOR_INDEX" => "0".into(),
        "CURSOR_NUMBER" => "1".into(),
        "CURRENT_YEAR" => y.to_string(),
        "CURRENT_YEAR_SHORT" => format!("{:02}", y.rem_euclid(100)),
        "CURRENT_MONTH" => format!("{mo:02}"),
        "CURRENT_MONTH_NAME" => MONTHS[mo as usize - 1].into(),
        "CURRENT_MONTH_NAME_SHORT" => MONTHS[mo as usize - 1][..3].into(),
        "CURRENT_DATE" => format!("{d:02}"),
        "CURRENT_DAY_NAME" => DAYS[wd].into(),
        "CURRENT_DAY_NAME_SHORT" => DAYS[wd][..3].into(),
        "CURRENT_HOUR" => format!("{:02}", tod / 3600),
        "CURRENT_MINUTE" => format!("{:02}", tod / 60 % 60),
        "CURRENT_SECOND" => format!("{:02}", tod % 60),
        "CURRENT_SECONDS_UNIX" => unix.to_string(),
        "CURRENT_TIMEZONE_OFFSET" => "Z".into(),
        "RANDOM" => format!("{:06}", rand64() % 1_000_000),
        "RANDOM_HEX" => format!("{:06x}", rand64() & 0xFF_FFFF),
        "UUID" => {
            let (a, b) = (rand64(), rand64());
            format!(
                "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
                a >> 32,
                (a >> 16) & 0xFFFF,
                a & 0xFFF,
                (b >> 48) & 0x3FFF | 0x8000,
                b & 0xFFFF_FFFF_FFFF
            )
        }
        "LINE_COMMENT" => cx.line_comment.unwrap_or("//").to_string(),
        "BLOCK_COMMENT_START" => "/*".into(),
        "BLOCK_COMMENT_END" => "*/".into(),
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Live session
// ---------------------------------------------------------------------------

/// A live snippet: its stops (absolute byte ranges, kept in sync with edits) and which one
/// is active.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SnippetSession {
    pub(crate) stops: Vec<Stop>,
    pub(crate) current: usize,
    /// The document length after the last mapped edit (detects unmapped edits).
    pub(crate) doc_len: usize,
}

impl SnippetSession {
    /// Start a session for `e` inserted at byte `at` (`None` if it has no stop but `$0`).
    pub(crate) fn start(e: &Expanded, at: usize, doc_len: usize) -> Option<Self> {
        if e.stops.len() < 2 {
            return None;
        }
        let stops = e
            .stops
            .iter()
            .map(|s| Stop {
                index: s.index,
                ranges: s.ranges.iter().map(|&(a, b)| (a + at, b + at)).collect(),
                choices: s.choices.clone(),
            })
            .collect();
        Some(SnippetSession {
            stops,
            current: 0,
            doc_len,
        })
    }
    /// Whether the active stop is the final one (`$0`).
    pub(crate) fn at_final(&self) -> bool {
        self.current + 1 >= self.stops.len()
    }
    /// The active stop.
    pub(crate) fn stop(&self) -> &Stop {
        &self.stops[self.current.min(self.stops.len() - 1)]
    }
    /// Map every range across an edit: the active stop grows with typing at its edges; the
    /// others do not absorb edits at their boundaries.
    pub(crate) fn map(&mut self, cs: &ChangeSet) {
        let cur = self.current;
        for (k, s) in self.stops.iter_mut().enumerate() {
            for r in &mut s.ranges {
                let (a, b) = if k == cur {
                    (cs.map(r.0, Assoc::Before), cs.map(r.1, Assoc::After))
                } else {
                    (cs.map(r.0, Assoc::After), cs.map(r.1, Assoc::Before))
                };
                *r = (a, b.max(a));
            }
        }
    }
    /// The byte span covering every stop.
    pub(crate) fn span(&self) -> (usize, usize) {
        let it = self.stops.iter().flat_map(|s| s.ranges.iter());
        let a = it.clone().map(|r| r.0).min().unwrap_or(0);
        let b = it.map(|r| r.1).max().unwrap_or(0);
        (a, b)
    }
}

// ---------------------------------------------------------------------------
// Editor glue
// ---------------------------------------------------------------------------

/// Where a snippet is being inserted from.
pub(crate) struct InsertOpts<'a> {
    pub(crate) uri: &'a str,
    pub(crate) line_comment: Option<&'a str>,
    /// Give continuation lines the insertion line's indentation (VS Code
    /// `insertTextMode: adjustIndentation`).
    pub(crate) adjust_indent: bool,
}

/// The selection for a stop: every range selected (mirrors become extra cursors).
pub(crate) fn selection_for(stop: &Stop) -> Selections {
    Selections::new(
        stop.ranges
            .iter()
            .map(|&(a, b)| Selection::range(a, b))
            .collect(),
        0,
    )
}

/// Replace `from..to` with the expanded snippet `body` (plus `extra` edits elsewhere, e.g.
/// auto-imports, in the same undo step), select the first stop and start a session.
#[allow(clippy::too_many_arguments)]
pub(crate) fn insert(
    state: Signal<EditorState>,
    history: Signal<Rc<RefCell<History>>>,
    code: Signal<String>,
    snippet: Signal<Option<SnippetSession>>,
    from: usize,
    to: usize,
    body: &str,
    extra: Vec<Change>,
    o: &InsertOpts,
) {
    let src = state.peek().text();
    let vars = |n: &str| {
        variable(
            n,
            &VarCtx {
                uri: o.uri,
                src: &src,
                from,
                to,
                line_comment: o.line_comment,
            },
        )
    };
    let mut e = expand(&parse(body), &vars);
    if o.adjust_indent {
        let ls = src[..from].rfind('\n').map_or(0, |i| i + 1);
        let lead: String = src[ls..from]
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        indent(&mut e, &lead);
    }
    let mut shift = 0isize;
    let mut delta = e.text.len() as isize - (to - from) as isize;
    let mut changes = vec![Change {
        from,
        to,
        insert: e.text.clone(),
    }];
    for c in extra {
        if c.to <= from {
            shift += c.insert.len() as isize - (c.to - c.from) as isize;
        } else if c.from < to {
            continue; // overlaps the snippet: skip
        }
        delta += c.insert.len() as isize - (c.to - c.from) as isize;
        changes.push(c);
    }
    let at = (from as isize + shift) as usize;
    let new_len = (src.len() as isize + delta) as usize;
    let session = SnippetSession::start(&e, at, new_len);
    let sel = match &session {
        Some(s) => selection_for(s.stop()),
        None => {
            let f = e.stops.last().map_or(e.text.len(), |s| s.ranges[0].0);
            Selections::single(Selection::caret(at + f))
        }
    };
    dispatch(
        state,
        history,
        code,
        Transaction::change_and_select(ChangeSet::from_changes(changes), sel),
        Coalesce::Never,
    );
    clear_recorded(); // the session is already in post-insert coordinates
    snippet.set(session);
}

/// After a key: map the session through the edits it caused (`log`), and end it when an
/// unmapped edit happened or the caret left the snippet.
pub(crate) fn after_key(
    state: Signal<EditorState>,
    snippet: Signal<Option<SnippetSession>>,
    log: &[ChangeSet],
) {
    let Some(mut s) = snippet.peek() else { return };
    let st = state.peek();
    let mut expected = s.doc_len as isize;
    for cs in log {
        s.map(cs);
        expected += cs
            .changes()
            .iter()
            .map(|c| c.insert.len() as isize - (c.to - c.from) as isize)
            .sum::<isize>();
    }
    let (a, b) = s.span();
    let head = st.primary().head;
    if expected != st.len() as isize || head < a || head > b {
        snippet.set(None);
        return;
    }
    s.doc_len = st.len();
    if log.is_empty() {
        return; // nothing moved
    }
    snippet.set(Some(s));
}

/// Tab / Shift+Tab: activate the next (`forward`) or previous stop. Reaching `$0` ends the
/// session with the caret there. Returns the stop entered (for choice lists).
pub(crate) fn jump(
    state: Signal<EditorState>,
    snippet: Signal<Option<SnippetSession>>,
    forward: bool,
) -> Option<Stop> {
    let mut s = snippet.peek()?;
    if forward {
        s.current += 1;
    } else if s.current > 0 {
        s.current -= 1;
    }
    let stop = s.stop().clone();
    let cur = state.peek();
    state.set(EditorState {
        doc: cur.doc.clone(),
        selection: selection_for(&stop),
    });
    if s.at_final() {
        snippet.set(None);
    } else {
        snippet.set(Some(s));
    }
    Some(stop)
}

#[cfg(test)]
mod tests {
    #[test]
    fn tzif_offsets_follow_the_transitions() {
        // A v1 TZif: types UTC+1 (0) and UTC+2 (1); one transition to +2 at t=1000.
        let mut b = b"TZif".to_vec();
        b.extend([0u8; 16]);
        for n in [0u32, 0, 0, 1, 2, 4] {
            b.extend(n.to_be_bytes());
        }
        b.extend(1000i32.to_be_bytes());
        b.push(1);
        b.extend(3600i32.to_be_bytes());
        b.extend([0, 0]);
        b.extend(7200i32.to_be_bytes());
        b.extend([1, 0]);
        b.extend(*b"CET\0");
        assert_eq!(super::tzif_offset(&b, 10), Some(3600));
        assert_eq!(super::tzif_offset(&b, 1000), Some(7200));
        assert_eq!(super::tzif_offset(&b, 99_999), Some(7200));
        assert_eq!(super::tzif_offset(b"nope", 0), None);
        // Real zone files (version 2+, 64-bit block) when the system has them.
        let now = 1_790_000_000; // 2026
        if let Ok(z) = std::fs::read("/usr/share/zoneinfo/Asia/Tokyo") {
            assert_eq!(super::tzif_offset(&z, now), Some(9 * 3600));
        }
        if let Ok(z) = std::fs::read("/usr/share/zoneinfo/Europe/Berlin") {
            assert_eq!(super::tzif_offset(&z, 1_784_000_000), Some(2 * 3600)); // July: CEST
            assert_eq!(super::tzif_offset(&z, 1_798_000_000), Some(3600)); // December: CET
        }
    }

    use super::*;

    fn ex(body: &str) -> Expanded {
        expand(&parse(body), &|n| match n {
            "TM_FILENAME" => Some("main.rs".into()),
            "EMPTY" => Some(String::new()),
            _ => None,
        })
    }

    fn texts(e: &Expanded) -> Vec<(u32, Vec<&str>)> {
        e.stops
            .iter()
            .map(|s| {
                (
                    s.index,
                    s.ranges.iter().map(|&(a, b)| &e.text[a..b]).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn snippet_grammar_parses_and_expands() {
        // Tab stops, placeholders, final stop.
        let e = ex("for ${1:item} in ${2:iter} {\n\t$0\n}");
        assert_eq!(e.text, "for item in iter {\n\t\n}");
        assert_eq!(
            texts(&e),
            vec![(1, vec!["item"]), (2, vec!["iter"]), (0, vec![""])]
        );
        // Mirrors share the defining placeholder's text.
        let e = ex("<${1:div}>$0</$1>");
        assert_eq!(e.text, "<div></div>");
        assert_eq!(texts(&e)[0], (1, vec!["div", "div"]));
        // Nested placeholders.
        let e = ex("${1:foo(${2:bar})}");
        assert_eq!(e.text, "foo(bar)");
        assert_eq!(texts(&e)[..2], [(1, vec!["foo(bar)"]), (2, vec!["bar"])]);
        // Choices: the first is inserted.
        let e = ex("let x = ${1|one,two\\,2,three|};");
        assert_eq!(e.text, "let x = one;");
        assert_eq!(e.stops[0].choices, vec!["one", "two,2", "three"]);
        // No $0: the final stop is the end.
        let e = ex("a$1b");
        assert_eq!(e.stops.last().unwrap().ranges, vec![(2, 2)]);
        // Variables: known, default, unknown (becomes a placeholder), empty with default.
        let e = ex("// $TM_FILENAME ${NOPE:def} ${EMPTY:fallback} $UNKNOWN");
        assert_eq!(e.text, "// main.rs def fallback UNKNOWN");
        assert_eq!(texts(&e)[0].1, vec!["UNKNOWN"]);
        // Transforms.
        let e = ex("${TM_FILENAME/(.*)\\.rs/${1:/upcase}/}");
        assert_eq!(e.text, "MAIN");
        let e = ex("${TM_FILENAME/[a-z]/X/g}");
        assert_eq!(e.text, "XXXX.XX");
        // Escapes and malformed constructs stay literal.
        let e = ex("\\$1 costs \\} $ and ${ and ${1:x");
        assert_eq!(e.text, "$1 costs } $ and ${ and ${1:x");
    }

    #[test]
    fn variables_resolve_from_the_insertion_site() {
        let src = "fn main() {\n    hello_world\n}";
        let at = src.find("hello").unwrap();
        let cx = VarCtx {
            uri: "file:///w/src/main.rs",
            src,
            from: at,
            to: at + 5,
            line_comment: Some("//"),
        };
        let v = |n: &str| variable(n, &cx).unwrap();
        assert_eq!(v("TM_FILENAME"), "main.rs");
        assert_eq!(v("TM_FILENAME_BASE"), "main");
        assert_eq!(v("TM_DIRECTORY"), "/w/src");
        assert_eq!(v("TM_FILEPATH"), "/w/src/main.rs");
        assert_eq!(v("TM_SELECTED_TEXT"), "hello");
        assert_eq!(v("TM_CURRENT_LINE"), "    hello_world");
        assert_eq!(v("TM_CURRENT_WORD"), "hello_world");
        assert_eq!(v("TM_LINE_INDEX"), "1");
        assert_eq!(v("TM_LINE_NUMBER"), "2");
        assert_eq!(v("LINE_COMMENT"), "//");
        assert!(v("CURRENT_YEAR").parse::<i32>().unwrap() >= 2024);
        assert_eq!(v("CURRENT_MONTH").len(), 2);
        assert_eq!(v("UUID").len(), 36);
        assert_eq!(v("RANDOM_HEX").len(), 6);
        assert!(variable("NOT_A_VARIABLE", &cx).is_none());
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(19_723), (2024, 1, 1));
    }

    #[test]
    fn session_maps_ranges_through_edits() {
        let e = ex("<${1:div}>${2:x}</$1>$0");
        let mut s = SnippetSession::start(&e, 10, 100).unwrap();
        assert_eq!(s.stop().ranges, vec![(11, 14), (18, 21)]);
        // Typing "span" over both mirrors (one change set, two edits).
        let cs = ChangeSet::from_changes(vec![
            crate::edit::Change {
                from: 11,
                to: 14,
                insert: "span".into(),
            },
            crate::edit::Change {
                from: 18,
                to: 21,
                insert: "span".into(),
            },
        ]);
        s.map(&cs);
        assert_eq!(s.stop().ranges, vec![(11, 15), (19, 23)]);
        // Stop 2 ("x" at 15..16 originally) shifted by the first edit, not absorbed.
        assert_eq!(s.stops[1].ranges, vec![(16, 17)]);
        assert_eq!(s.span(), (11, 24));
        s.current = 2;
        assert!(s.at_final());
        // A snippet with only $0 needs no session.
        assert!(SnippetSession::start(&ex("abc$0"), 0, 3).is_none());
        // Indentation shifts the stops.
        let mut e = ex("{\n\t${1:a}\n}");
        indent(&mut e, "  ");
        assert_eq!(e.text, "{\n  \ta\n  }");
        assert_eq!(&e.text[e.stops[0].ranges[0].0..e.stops[0].ranges[0].1], "a");
    }
}
