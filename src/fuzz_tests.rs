//! Seeded, bounded fuzzing of every pure text scanner / offset path: random documents built
//! from hostile fragments (multi-byte, emoji/ZWJ, combining, RTL, NUL, CR/LF/CRLF, brackets,
//! unterminated strings/comments) and EVERY byte offset — including ones inside a char and
//! past the end. Completing without a panic is the pass. Deterministic (fixed seeds).

use crate::edit::{Selection, Selections};

const FRAGS: &[&str] = &[
    "a",
    "Zz",
    "_x",
    "fooBar",
    "foo_bar",
    " ",
    "\t",
    "\n",
    "\r\n",
    "\r",
    "\0",
    "\u{7f}",
    "é",
    "e\u{0301}",
    "—",
    "日本語",
    "🎉",
    "👩\u{200d}💻",
    "שלום",
    "مرحبا",
    "\u{200b}",
    "\u{feff}",
    "\u{fffd}",
    "(",
    ")",
    "[",
    "]",
    "{",
    "}",
    "<",
    ">",
    "</",
    "/>",
    "<div>",
    "\"",
    "'",
    "`",
    "\\",
    "/*",
    "*/",
    "//",
    "#",
    "--",
    "<!--",
    "-->",
    "${",
    "$1",
    "${1:x}",
    "#fff",
    "rgb(",
    "hsl(",
    "https://",
    "[a]",
    "= ",
    ": ",
    "- ",
    "> ",
    "```",
    "\"\"\"",
    "#region",
];

pub(crate) struct Rng(u64);
impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub(crate) fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    pub(crate) fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

pub(crate) fn random_doc(rng: &mut Rng, frags: usize) -> String {
    let mut s = String::new();
    for _ in 0..frags {
        s.push_str(FRAGS[rng.below(FRAGS.len())]);
    }
    s
}

fn docs() -> Vec<String> {
    let mut v: Vec<String> = [
        "",
        "\n",
        "\r",
        "\r\n\r\n",
        "\0\0\0",
        "é",
        "🎉",
        "(((((",
        ")))))",
        "\"unterminated",
        "/* open",
        "<div><span>",
        "\u{feff}x",
        "a\rb\nc\r\nd",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let mut rng = Rng::new(7);
    for i in 0..120 {
        v.push(random_doc(&mut rng, 1 + i % 40));
    }
    v
}

fn sels(src: &str, rng: &mut Rng) -> Selections {
    // Deliberately unclamped / non-boundary offsets, then the realistic clamped variant.
    let n = src.len() + 3;
    let a = rng.below(n);
    let b = rng.below(n);
    Selections::new(
        vec![Selection::range(a, b), Selection::caret(rng.below(n))],
        0,
    )
}

fn boundary_sels(src: &str, rng: &mut Rng) -> Selections {
    let fix = |mut p: usize| {
        p = p.min(src.len());
        while !src.is_char_boundary(p) {
            p -= 1;
        }
        p
    };
    let n = src.len() + 1;
    Selections::new(
        vec![
            Selection::range(fix(rng.below(n)), fix(rng.below(n))),
            Selection::caret(fix(rng.below(n))),
        ],
        0,
    )
}

#[test]
fn every_scanner_survives_hostile_text_at_every_offset() {
    use crate::ops::{self, Case, LineEnding, OpCfg};
    let cfg = OpCfg {
        tab: "    ".into(),
        tab_size: 4,
        line_comment: Some("//".into()),
        block_comment: Some(("/*".into(), "*/".into())),
        brackets: vec![('(', ')'), ('[', ']'), ('{', '}')],
    };
    let mut rng = Rng::new(42);
    for src in docs() {
        let src = src.as_str();
        // Whole-document scanners.
        let _ = crate::lines::LineIndex::build(src);
        let _ = crate::lines::regions_of(src);
        let _ = crate::fold::foldable(src);
        let _ = ops::detect_indentation(src);
        let _ = ops::detect_line_ending(src);
        let _ = ops::convert_line_endings(src, LineEnding::CrLf);
        let _ = ops::reindent_paste(src, "  \t");
        let _ = crate::search::preserve_case(src, "repl");
        let _ = crate::search::preserve_case("Ab", src);
        let _ = crate::diff::diff_lines(src, "x\né\n");
        let _ = crate::diff::word_changes(src, "é 日本 x");
        let _ = crate::highlight::bracket_depth(src, &cfg.brackets);
        let _ = crate::snippet::parse(src);
        let _ = crate::emmet::expand_html(src, "px");
        let _ = crate::emmet::expand_css(src);
        for line in src.split('\n') {
            let _ = crate::scan_text::unicode_marks(line);
            let _ = crate::scan_text::find_links(line);
            let _ = crate::scan_text::find_colors(line);
            let _ = crate::lines::marker_of(line);
        }
        for q in ["é", "a", "\\b\\w+", "(", "日", "(?i)É", "$", "^", ""] {
            for regex in [false, true] {
                for case_sensitive in [false, true] {
                    let o = crate::search::Options {
                        case_sensitive,
                        whole_word: true,
                        regex,
                    };
                    let _ = crate::search::find_all(src, q, &o);
                    let o = crate::search::Options {
                        whole_word: false,
                        ..o
                    };
                    let _ = crate::search::find_all(src, q, &o);
                }
            }
        }
        // Every byte offset, plus past-the-end.
        let idx = crate::lines::LineIndex::build(src);
        for pos in 0..=src.len() + 2 {
            let _ = crate::emmet::abbreviation_start(src, pos);
            let _ = crate::scan_text::linked_tag_names(src, pos);
            let _ = crate::brackets::find_bracket_match(src, &cfg.brackets, pos);
            let _ = ops::next_subword(src, pos);
            let _ = ops::prev_subword(src, pos);
            let _ = ops::smart_home(src, pos);
            let _ = idx.line_of(src, pos);
            let _ = idx.col_of(src, pos);
            let _ = idx.byte_at(src, pos % (idx.line_count() + 2), pos);
        }
        // Line operations with boundary and non-boundary selections.
        for round in 0..6 {
            let s = if round % 2 == 0 {
                boundary_sels(src, &mut rng)
            } else {
                sels(src, &mut rng)
            };
            let _ = ops::move_lines(src, &s, round % 3 == 0);
            let _ = ops::copy_lines(src, &s, true);
            let _ = ops::delete_lines(src, &s);
            let _ = ops::join_lines(src, &s);
            let _ = ops::duplicate_selection(src, &s);
            let _ = ops::sort_lines(src, &s, false);
            let _ = ops::transpose(src, &s);
            let _ = ops::insert_line(src, &s, true);
            for c in [
                Case::Upper,
                Case::Lower,
                Case::Title,
                Case::Snake,
                Case::Kebab,
                Case::Camel,
            ] {
                let _ = ops::transform_case(src, &s, c);
            }
            let _ = ops::trim_trailing_whitespace(src, &s);
            let _ = ops::insert_final_newline(src, &s);
            let _ = ops::add_cursor_vertical(src, &s, true, 4);
            let _ = ops::add_cursor_vertical(src, &s, false, 4);
            let _ = ops::select_all_occurrences(src, &s);
            let _ = ops::cursors_at_line_ends(src, &s);
            let _ = ops::expand_line_selection(src, &s);
            let _ = ops::jump_to_bracket(src, &s, &cfg);
            let _ = ops::select_to_bracket(src, &s, &cfg);
        }
    }
}

/// Every bundled grammar (tree-sitter) and every hand-written scanner.
pub(crate) const LANGS: &[&str] = &[
    "rust",
    "toml",
    "json",
    "markdown",
    "javascript",
    "typescript",
    "tsx",
    "python",
    "go",
    "c",
    "cpp",
    "html",
    "css",
    "yaml",
    "bash",
    "sql",
    "java",
    "kotlin",
    "php",
    "ruby",
    "swift",
    "csharp",
    "plain",
];

/// Unterminated / malformed constructs per language family.
const MALFORMED: &[&str] = &[
    "\"never closed",
    "'x",
    "/* never closed",
    "<!-- open",
    "\"\"\"open",
    "`tpl ${",
    "[table\nkey = = \"v",
    "{\"a\": [1, 2,",
    "key: [a, {b: ",
    "# H\n```rust\nfn(",
    "<div <p>",
    "fn f( { [ ( }",
    "r#\"raw",
    "#[attr(",
    "$(echo \"",
    "SELECT 'x",
    "a: |\n  b\n\tc",
];

/// Seeded random edit loop per language (proptest-style, bounded): random hostile edits
/// at random (possibly mid-char, possibly past-the-end) offsets, re-highlighting through
/// both the whole-text API and the incremental session after each one.
#[test]
fn every_language_survives_a_random_edit_loop() {
    for (li, name) in LANGS.iter().enumerate() {
        let mut rng = Rng::new(1000 + li as u64);
        let langs: Vec<Box<dyn crate::lang::Language>> = vec![
            crate::lang::by_name(name),
            crate::lang::scanner_by_name(name),
        ];
        for language in langs {
            let mut src = MALFORMED.join("\n");
            let mut session = language.session();
            for step in 0..60 {
                // Edit at a random offset snapped to a boundary (the editor's invariant) …
                let at = crate::geometry::floor_char_boundary(&src, rng.below(src.len() + 4));
                let to = crate::geometry::floor_char_boundary(&src, at + rng.below(8));
                let k = rng.below(4);
                let ins = random_doc(&mut rng, k);
                src.replace_range(at..to.max(at), &ins);
                if step % 7 == 0 {
                    src.push_str(MALFORMED[rng.below(MALFORMED.len())]);
                }
                let _ = language.highlight(&src);
                let _ = language.tree(&src);
                if let Some(s) = session.as_mut() {
                    s.update(&src);
                    // … but query ranges are deliberately hostile.
                    let a = rng.below(src.len() + 10);
                    let b = rng.below(src.len() + 10);
                    let _ = s.highlight_range(&src, a.min(b)..a.max(b));
                    let _ = s.highlight_range(&src, b..a);
                    let _ = s.fold_ranges();
                    let _ = s.enclosing_nodes(a, b);
                }
                let toks = language.highlight(&src);
                let _ = crate::highlight::slice_tokens(
                    &toks,
                    rng.below(src.len() + 3),
                    rng.below(src.len() + 3),
                );
            }
        }
    }
}

/// Pathological shapes: deep nesting and a long single line, through every language.
#[test]
fn every_language_survives_deep_nesting_and_long_lines() {
    let deep = format!("{}{}", "([{".repeat(1700), "}])".repeat(1000));
    let long_line = "a = \"é\" + 日本 // 🎉 ".repeat(4000);
    let cr_mixed = "a\rb\r\nc\nd\r\r\n\0\u{feff}e".repeat(500);
    for name in LANGS {
        let language = crate::lang::by_name(name);
        for src in [&deep, &long_line, &cr_mixed] {
            let _ = language.highlight(src);
            let _ = language.tree(src);
            if let Some(mut s) = language.session() {
                s.update(src);
                let _ = s.highlight_range(src, 0..src.len());
                let _ = s.fold_ranges();
            }
            let idx = crate::lines::LineIndex::build(src);
            let _ = idx.max_cols();
            let _ = crate::ops::detect_indentation(src);
        }
    }
}

#[test]
fn delete_lines_with_carets_on_the_last_two_lines_does_not_underflow() {
    // Before: the last block's change started at the previous block's trailing newline,
    // overlapping it, and `from - removed` underflowed.
    let src = "a\nb";
    let sels = Selections::new(vec![Selection::caret(0), Selection::caret(2)], 0);
    let tx = crate::ops::delete_lines(src, &sels).expect("an edit");
    let _ = tx;
}

#[test]
fn offsets_inside_a_char_are_floored_not_panicked() {
    let s = "a—b日🎉";
    for p in 0..=s.len() + 3 {
        let f = crate::geometry::floor_char_boundary(s, p);
        assert!(s.is_char_boundary(f) && f <= p.min(s.len()));
        let _ = crate::view::word_range_at(s, p);
        let _ = crate::geometry::prev_char(s, p);
        let _ = crate::emmet::abbreviation_start(s, p);
        let _ = crate::ops::next_subword(s, p);
    }
}
