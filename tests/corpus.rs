//! "It must open any file, formatted properly or not." A corpus of hostile documents, each
//! mounted in a fully-featured editor (language, minimap, sticky scroll, soft wrap, diff
//! base, folds) and driven through paint, scroll, fold-all, select-all and editing. Bytes
//! that are not UTF-8 arrive the way the host decodes them (lossy / BOM-stripped / Latin-1),
//! so the corpus covers those decodings. Completing without a panic is the pass.
//!
//! The two largest inputs (a 10 MB single line, 200k lines) run with `--ignored` — they are
//! too slow for the opt-level-0 test build CI uses; scaled-down versions run by default.

use std::collections::BTreeSet;

use pebbles::core::{Ui, create_root_signal};
use pebbles::prelude::*;
use pebbles::render::{Scene, TextEnv};
use pebbles_code_editor::{EditorTheme, code_editor, lang};

const SIZE: Size = Size {
    width: 800.0,
    height: 500.0,
};

fn frame(ui: &mut Ui, env: &mut TextEnv) {
    ui.rebuild_if_dirty();
    ui.layout(env, SIZE);
    let mut scene = Scene::new();
    ui.paint(env, &mut scene);
}

fn key(ui: &mut Ui, env: &mut TextEnv, k: KeyInput) {
    ui.dispatch_key(k);
    frame(ui, env);
}

fn mv(ui: &mut Ui, env: &mut TextEnv, motion: Motion, extend: bool) {
    key(ui, env, KeyInput::Move { motion, extend });
}

/// Open `text` as `language`, then paint / scroll / fold / select-all / edit through it.
fn exercise(text: &str, language: &str) {
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    pebbles::core::keyboard::set_modifiers(false, false, false, false);
    let code = create_root_signal(text.to_string());
    let base = create_root_signal(format!(
        "{}\nchanged é\n",
        text.get(..text.len() / 2).unwrap_or("")
    ));
    let folds = create_root_signal(BTreeSet::new());
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(
        View::new(
            Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xFF),
            code_editor(code)
                .language(lang::by_name(language))
                .theme(EditorTheme::dark())
                .height(480.0)
                .minimap(true)
                .sticky_scroll(true)
                .soft_wrap(true)
                .diff_base(base)
                .folds(folds)
                .autofocus(),
        )
        .into_widget(),
    );
    for _ in 0..3 {
        frame(&mut ui, &mut env);
    }
    // Scroll down and back.
    for d in [800.0, 50_000.0, -1e9, 1e12] {
        ui.dispatch_scroll(Offset::new(200.0, 200.0), d);
        frame(&mut ui, &mut env);
    }
    // Fold every head line the editor would accept (and some it wouldn't).
    folds.set((0..200).chain([usize::MAX - 1, 1 << 40]).collect());
    frame(&mut ui, &mut env);
    key(&mut ui, &mut env, KeyInput::SelectAll);
    key(&mut ui, &mut env, KeyInput::Insert("é—🎉\n".to_string()));
    key(&mut ui, &mut env, KeyInput::Undo);
    folds.set(BTreeSet::new());
    mv(&mut ui, &mut env, Motion::DocEnd, false);
    for s in ["\"", "/*", "(", "<", "日", "\r\n", "\0"] {
        key(&mut ui, &mut env, KeyInput::Insert(s.to_string()));
    }
    mv(&mut ui, &mut env, Motion::DocStart, true);
    key(&mut ui, &mut env, KeyInput::Backspace);
    key(&mut ui, &mut env, KeyInput::Undo);
    for m in [
        Motion::WordRight,
        Motion::Down,
        Motion::LineEnd,
        Motion::Up,
        Motion::WordLeft,
    ] {
        mv(&mut ui, &mut env, m, true);
    }
    key(&mut ui, &mut env, KeyInput::Indent);
    key(&mut ui, &mut env, KeyInput::Enter);
    key(&mut ui, &mut env, KeyInput::Delete);
    ui.dispatch_pointer_down(Offset::new(300.0, 100.0));
    ui.dispatch_tap(Offset::new(300.0, 100.0));
    ui.dispatch_hover(Offset::new(780.0, 300.0)); // minimap
    frame(&mut ui, &mut env);
    // An external rewrite (file changed / truncated on disk while open).
    code.set(String::new());
    frame(&mut ui, &mut env);
    code.set(text.get(..text.len() / 3).unwrap_or("x").to_string());
    frame(&mut ui, &mut env);
}

/// Bytes decoded the ways a host might hand them to the editor.
fn decodings(bytes: &[u8]) -> Vec<String> {
    vec![
        String::from_utf8_lossy(bytes).into_owned(),
        bytes.iter().map(|&b| b as char).collect(), // Latin-1
    ]
}

fn small_corpus() -> Vec<(&'static str, String)> {
    let mut v: Vec<(&'static str, String)> = vec![
        ("empty", String::new()),
        ("nul", "a\0b\0\0\0c\n\0".repeat(50)),
        ("mixed-eol", "one\rtwo\r\nthree\nfour\r\r\n\n\r".repeat(100)),
        ("bom-utf8", "\u{feff}fn main() {}\n".to_string()),
        ("nested", format!("{}{}", "{[(".repeat(5000 / 3), ")]}".repeat(5000 / 3))),
        ("unclosed", "{[(<".repeat(5000)),
        ("closers", ")]}>".repeat(5000)),
        (
            "unicode",
            "café 日本語 🎉👨\u{200d}👩\u{200d}👧 e\u{0301}\u{0301} שלום مرحبا \u{202e}rtl\u{202c} \u{200b}\u{fe0f}\n"
                .repeat(40),
        ),
        ("long-line", "x = \"é—\" + 日本; ".repeat(20_000)),
        ("many-lines", "fn a() { let x = \"é\"; }\n".repeat(20_000)),
        ("toml-bad", "[package\nname = \"x\nversion = = 1\n[[bin]]]\n# — é\nk = { a = [1, 2,\n".to_string()),
        ("json-bad", "{\"a\": [1, 2, {\"b\": \"é—\n, }, ]] trailing".to_string()),
        ("yaml-bad", "a: [b, {c: d\n  - e\n\t- tab\n: :\n---\n...\n&a *a <<: *b\n".to_string()),
        ("md-bad", "# h\n```rust\nfn x( {\n> > quote\n| a | b\n|--\n[link](http://\n**bold\n".to_string()),
        ("html-bad", "<html><body><div <p>é</di v><!-- open <script>if (a < b) {</style>".to_string()),
    ];
    for (b, name) in [
        (&b"\xff\xfeh\0i\0\n\0"[..], "utf16le-bom"),
        (&b"\xfe\xff\0h\0i"[..], "utf16be-bom"),
        (
            &b"caf\xe9 na\xefve \x80\x81\xc3\x28 \xed\xa0\x80 \xf0\x28\x8c\x28"[..],
            "invalid-utf8",
        ),
    ] {
        for d in decodings(b) {
            v.push((name, d));
        }
    }
    v
}

#[test]
fn hostile_corpus_opens_paints_scrolls_folds_and_edits() {
    for (_name, text) in small_corpus() {
        exercise(&text, "rust");
    }
}

#[test]
fn malformed_documents_in_their_own_language() {
    for (name, language) in [
        ("toml-bad", "toml"),
        ("json-bad", "json"),
        ("yaml-bad", "yaml"),
        ("md-bad", "markdown"),
        ("html-bad", "html"),
        ("unicode", "markdown"),
        ("nested", "json"),
        ("mixed-eol", "python"),
    ] {
        let text = small_corpus()
            .into_iter()
            .find(|(n, _)| *n == name)
            .unwrap()
            .1;
        exercise(&text, language);
    }
}

/// Unterminated strings / comments / blocks, opened in every bundled grammar.
#[test]
fn unterminated_constructs_in_every_grammar() {
    let text = "\"never closed\n'x\n/* never closed\n<!-- open\n\"\"\"open\n`tpl ${\nr#\"raw\n$(echo \"\n#[a(\n{ [ (";
    for language in [
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
        "ruby",
        "php",
        "plain",
    ] {
        exercise(text, language);
    }
}

#[test]
#[ignore = "slow; run with `cargo test --profile fuzz --test corpus -- --ignored`"]
fn ten_megabyte_single_line() {
    exercise(&"a—é 日 {[(\"".repeat(10 * 1024 * 1024 / 20), "rust");
}

#[test]
#[ignore = "slow; run with `cargo test --profile fuzz --test corpus -- --ignored`"]
fn two_hundred_thousand_lines() {
    exercise(&"fn a() { let x = \"é\"; }\n".repeat(200_000), "rust");
}
