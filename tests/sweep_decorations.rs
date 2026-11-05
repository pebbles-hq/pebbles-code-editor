//! Gravel user-sweep regressions (gravel-bugs.md, "A lot of unwanted behaviour", 2026-11-05)
//! for in-text decorations, checked on an offscreen GPU capture (skips without an adapter;
//! lavapipe works):
//!
//! - a caret on an empty line boxed the character after it at the end of every line ending
//!   in punctuation (`word_at` off a word is that single character, and the occurrence
//!   highlight searched for it); VS Code highlights word occurrences only;
//! - a plain editor with a `diff_base` (Gravel's dirty-diff gutter) painted a full-width
//!   green band on every added line; VS Code paints that only in a diff editor.

use pebbles::core::create_root_signal;
use pebbles::prelude::*;
use pebbles::shell::capture::OffscreenCapture;
use pebbles_code_editor::{EditorTheme, code_editor};
use pebbles_testing::Harness;

const W: u32 = 800;
const H: u32 = 300;

/// The text area (left of the minimap / scrollbar / overview ruler) of rows `y0..y1`.
fn strip(rgba: &[u8], y0: usize, y1: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for y in y0..y1 {
        let i = y * W as usize * 4;
        out.extend_from_slice(&rgba[i..i + 680 * 4]);
    }
    out
}

fn render(
    gpu: &mut OffscreenCapture,
    code: &str,
    configure: impl Fn(pebbles_code_editor::CodeEditor) -> pebbles_code_editor::CodeEditor,
    click: Option<Offset>,
) -> Vec<u8> {
    let theme = EditorTheme::dark();
    let bg = theme.background;
    let code = create_root_signal(code.to_string());
    let mut h = Harness::new()
        .window(f64::from(W), f64::from(H))
        .background(bg);
    h.mount_widget(configure(
        code_editor(code)
            .theme(theme)
            .height(f64::from(H))
            .gutter(false)
            .minimap(false),
    ));
    h.frames(3);
    if let Some(at) = click {
        h.click(at);
    }
    // Park the pointer away from the text (no hover decorations).
    h.hover(Offset::new(f64::from(W) - 5.0, f64::from(H) - 5.0));
    h.frames(3);
    let scene = h.draw();
    gpu.render_scene(&scene, W, H, bg).expect("render")
}

#[test]
fn a_caret_on_an_empty_line_highlights_no_occurrences() {
    let Ok(mut gpu) = OffscreenCapture::new() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    // Line 0 ends in ';' — the character after it is the '\n' the empty-line caret sits on.
    let src = "let a = 1;\n\nlet b = 2;\nfoo();\n";
    // A click on the empty line 1 (real press / release), with and without the feature:
    // line 0 must look the same (the current-line band is on line 1 only).
    let click = Some(Offset::new(200.0, 28.0));
    let on = render(&mut gpu, src, |e| e.occurrences_highlight(true), click);
    let off = render(&mut gpu, src, |e| e.occurrences_highlight(false), click);
    assert!(
        strip(&on, 0, 16) == strip(&off, 0, 16),
        "an empty-line caret boxed line 0's line end"
    );
    // Lines 2 and 3 too (after the empty line's band).
    assert!(strip(&on, 44, 90) == strip(&off, 44, 90));
}

#[test]
fn a_plain_editor_with_a_diff_base_paints_no_added_line_band() {
    let Ok(mut gpu) = OffscreenCapture::new() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let src = "first\nadded line\nlast\n";
    let base = create_root_signal(String::from("first\nlast\n"));
    let plain = render(&mut gpu, src, |e| e, None);
    let with_base = render(&mut gpu, src, move |e| e.diff_base(base), None);
    assert!(
        strip(&plain, 0, 120) == strip(&with_base, 0, 120),
        "the dirty-diff base changed the text background (an added-line band)"
    );
    // The inline diff view still bands the added line.
    let inline = render(
        &mut gpu,
        src,
        move |e| e.diff_base(base).inline_diff(true),
        None,
    );
    assert!(strip(&plain, 0, 120) != strip(&inline, 0, 120));
}
