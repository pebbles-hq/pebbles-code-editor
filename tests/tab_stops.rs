//! Tab stops (gravel-bugs.md S12): a tab advances to the next multiple of `editor.tabSize`
//! visual columns (VS Code), not one cell. Covered through real pointer / key events on the
//! headless harness, and on an offscreen GPU capture (skips without an adapter; lavapipe
//! works): a tab-indented document must draw exactly like the same document with each tab
//! expanded to spaces — caret, selection, indent guides, word highlights, find matches, soft
//! wrap, the minimap, sticky scroll and an inline diff — at tab sizes 2, 4 and 8.

use pebbles::core::create_root_signal;
use pebbles::prelude::*;
use pebbles::shell::capture::OffscreenCapture;
use pebbles_code_editor::{EditorTheme, code_editor, editor_controller};
use pebbles_testing::Harness;

const W: u32 = 800;
const H: u32 = 300;
/// The monospace grid with the gutter off (fs 13.5, lh 1.6, pad 14 / 10).
const ADV: f64 = 13.5 * 0.6;
const LINE: f64 = 13.5 * 1.6;
const PAD_L: f64 = 14.0;
const PAD_T: f64 = 10.0;

/// Window point of fractional visual column `v` on `line`.
fn at(line: usize, v: f64) -> Offset {
    Offset::new(PAD_L + v * ADV, PAD_T + line as f64 * LINE + LINE / 2.0)
}

/// `s` with every tab expanded to the spaces reaching its tab stop.
fn expand(s: &str, ts: usize) -> String {
    let mut out = String::new();
    for line in s.split('\n') {
        let mut v = 0;
        for c in line.chars() {
            if c == '\t' {
                let w = ts - v % ts;
                out.push_str(&" ".repeat(w));
                v += w;
            } else {
                out.push(c);
                v += 1;
            }
        }
        out.push('\n');
    }
    out.pop();
    out
}

fn mount(
    code: Signal<String>,
    cfg: impl FnOnce(pebbles_code_editor::CodeEditor) -> pebbles_code_editor::CodeEditor,
) -> Harness {
    pebbles::core::keyboard::set_modifiers(false, false, false, false);
    let theme = EditorTheme::dark();
    let mut h = Harness::new()
        .window(f64::from(W), f64::from(H))
        .background(theme.background);
    h.mount_widget(cfg(code_editor(code)
        .theme(theme)
        .height(f64::from(H))
        .gutter(false)
        .minimap(false)
        .detect_indentation(false)
        .autofocus()));
    h.frames(3);
    h
}

/// Mount `src` at tab size `ts`, run `act`, then type `Z` and return the text.
fn type_after(src: &str, ts: usize, act: impl FnOnce(&mut Harness)) -> String {
    let code = create_root_signal(src.to_string());
    let mut h = mount(code, |e| e.tab_size(ts));
    act(&mut h);
    h.key(KeyInput::Insert("Z".into()));
    h.frames(2);
    code.get()
}

#[test]
fn clicking_inside_a_tab_lands_on_its_nearer_side() {
    for ts in [2usize, 4, 8] {
        let t = ts as f64;
        // Left part of the tab: the caret goes before it.
        let got = type_after("\tx", ts, |h| h.click(at(0, t * 0.3)));
        assert_eq!(got, "Z\tx", "ts={ts}: click in the tab's left half");
        // Right part: after it (before `x`, which is drawn at the tab stop).
        let got = type_after("\tx", ts, |h| h.click(at(0, t * 0.7)));
        assert_eq!(got, "\tZx", "ts={ts}: click in the tab's right half");
        // The char after the tab is at the tab stop: a click on its right half is after it.
        let got = type_after("\tx", ts, |h| h.click(at(0, t + 0.8)));
        assert_eq!(got, "\txZ", "ts={ts}: click on the char after the tab");
        // Mixed: two chars, then a tab that reaches the next stop.
        let got = type_after("ab\tx", ts, |h| {
            h.click(at(0, (ts.max(3).next_multiple_of(ts)) as f64 + 0.2))
        });
        assert_eq!(
            got, "ab\tZx",
            "ts={ts}: the tab after `ab` ends at the next stop"
        );
    }
}

#[test]
fn up_and_down_keep_the_visual_column() {
    for ts in [2usize, 4, 8] {
        let sp = " ".repeat(ts);
        // After a tab (visual column ts) → Down lands after ts spaces.
        let got = type_after(&format!("\tab\n{sp}cd"), ts, |h| {
            h.click(at(0, ts as f64));
            h.key(KeyInput::Move {
                motion: Motion::Down,
                extend: false,
            });
        });
        assert_eq!(got, format!("\tab\n{sp}Zcd"), "ts={ts}: tab → spaces");
        // And back up: after ts spaces → after the tab.
        let got = type_after(&format!("\tab\n{sp}cd"), ts, |h| {
            h.click(at(1, ts as f64));
            h.key(KeyInput::Move {
                motion: Motion::Up,
                extend: false,
            });
        });
        assert_eq!(got, format!("\tZab\n{sp}cd"), "ts={ts}: spaces → tab");
    }
}

#[test]
fn shift_alt_drag_box_selects_visual_columns() {
    for ts in [2usize, 4, 8] {
        let sp = " ".repeat(ts);
        let src = format!("\txy\n{sp}xy\nab");
        let got = type_after(&src, ts, |h| {
            pebbles::core::keyboard::set_modifiers(true, false, true, false);
            h.drag(at(0, ts as f64 + 1.0), at(1, ts as f64 + 1.0));
            pebbles::core::keyboard::set_modifiers(false, false, false, false);
        });
        assert_eq!(got, format!("\txZy\n{sp}xZy\nab"), "ts={ts}");
    }
}

#[test]
fn the_controller_reports_the_visual_column() {
    for ts in [2usize, 4, 8] {
        let code = create_root_signal("\tx\n".to_string());
        let ctrl = editor_controller();
        let mut h = mount(code, |e| e.tab_size(ts).controller(ctrl));
        ctrl.set_cursor(0, 2);
        h.frames(3);
        assert_eq!(ctrl.cursor().peek(), (0, 2), "char column");
        assert_eq!(
            ctrl.visual_col().peek(),
            ts + 1,
            "VS Code `Col` - 1 at ts={ts}"
        );
    }
}

// ---------------------------------------------------------------------------
// Pixels
// ---------------------------------------------------------------------------

fn capture(
    gpu: &mut OffscreenCapture,
    src: &str,
    ts: usize,
    cfg: impl Fn(pebbles_code_editor::CodeEditor) -> pebbles_code_editor::CodeEditor,
    act: impl FnOnce(&mut Harness),
) -> Vec<u8> {
    let code = create_root_signal(src.to_string());
    let mut h = mount(code, |e| cfg(e.tab_size(ts)));
    act(&mut h);
    // Park the pointer away from the text (no hover decorations).
    h.hover(Offset::new(f64::from(W) - 5.0, f64::from(H) - 5.0));
    h.frames(3);
    let scene = h.draw();
    gpu.render_scene(&scene, W, H, EditorTheme::dark().background)
        .expect("render")
}

/// Pixels (of the whole window) differing by more than a rounding error.
fn differing(a: &[u8], b: &[u8]) -> usize {
    a.chunks(4)
        .zip(b.chunks(4))
        .filter(|(p, q)| p.iter().zip(q.iter()).any(|(x, y)| x.abs_diff(*y) > 24))
        .count()
}

const GO: &str = "package main\n\nfunc main() {\n\tfoo := 1\n\tif foo > 0 {\n\t\tbar(foo)\t// call\n\t}\n}\nx\ty\tfoo\n";

/// Render the tabbed `GO` and its space expansion with the same config + actions; they must
/// match pixel for pixel, and differ from the same tabbed text at another tab size.
fn assert_tabs_draw_like_spaces(
    name: &str,
    src: &str,
    cfg: impl Fn(pebbles_code_editor::CodeEditor) -> pebbles_code_editor::CodeEditor + Copy,
    act: impl Fn(&mut Harness, usize) + Copy,
) {
    let Ok(mut gpu) = OffscreenCapture::new() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut shots = Vec::new();
    for ts in [2usize, 4, 8] {
        let tabs = capture(&mut gpu, src, ts, cfg, |h| act(h, ts));
        let spaces = capture(&mut gpu, &expand(src, ts), ts, cfg, |h| act(h, ts));
        let d = differing(&tabs, &spaces);
        assert!(
            d == 0,
            "{name}, ts={ts}: {d} pixels differ from the space expansion"
        );
        shots.push(tabs);
    }
    assert!(
        differing(&shots[0], &shots[2]) > 50,
        "{name}: tab size 2 and 8 drew the same (tabs ignored the tab size)"
    );
}

#[test]
fn caret_and_current_line_on_a_tabbed_line() {
    // A click on `bar` (line 5, after two tabs).
    assert_tabs_draw_like_spaces(
        "caret",
        GO,
        |e| e,
        |h, ts| {
            h.click(at(5, (2 * ts) as f64 + 1.2));
        },
    );
}

#[test]
fn caret_x_is_at_the_tab_stop() {
    let Ok(mut gpu) = OffscreenCapture::new() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let c = EditorTheme::dark().caret.to_rgba8();
    let caret = [c.r, c.g, c.b];
    for ts in [2usize, 4, 8] {
        // A click just right of the tab's middle puts the caret after it, at the tab stop.
        let px = capture(
            &mut gpu,
            "\tx\n",
            ts,
            |e| e,
            |h| {
                h.click(at(0, ts as f64 * 0.75));
            },
        );
        let y = (PAD_T + LINE / 2.0) as usize;
        let row = &px[y * W as usize * 4..(y + 1) * W as usize * 4];
        let x = row
            .chunks(4)
            .position(|p| {
                p.iter()
                    .zip(caret.iter())
                    .take(3)
                    .all(|(a, b)| a.abs_diff(*b) < 40)
            })
            .expect("a caret pixel on line 0");
        let want = PAD_L + ts as f64 * ADV;
        assert!(
            (x as f64 - want).abs() <= 2.0,
            "ts={ts}: caret at x={x}, expected the tab stop at {want}"
        );
    }
}

#[test]
fn selection_rectangles_on_tabbed_lines() {
    assert_tabs_draw_like_spaces(
        "select all",
        GO,
        |e| e,
        |h, _| {
            h.click(at(0, 1.0));
            h.press("Ctrl+A");
        },
    );
    // A drag from inside line 3's tab to inside line 5's second tab.
    assert_tabs_draw_like_spaces(
        "drag selection",
        GO,
        |e| e,
        |h, ts| {
            h.drag(at(3, ts as f64 + 2.0), at(5, (2 * ts) as f64 + 3.0));
        },
    );
}

#[test]
fn indent_guides_whitespace_and_occurrences() {
    // Word highlight of `foo` (on tabbed lines) + guides at tab stops.
    assert_tabs_draw_like_spaces(
        "guides + occurrences",
        GO,
        |e| e.indent_guides(true).occurrences_highlight(true),
        |h, ts| {
            h.click(at(3, ts as f64 + 1.0));
        },
    );
}

#[test]
fn whitespace_arrow_spans_the_tab() {
    let Ok(mut gpu) = OffscreenCapture::new() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let c = EditorTheme::dark().background.to_rgba8();
    let bg = [c.r, c.g, c.b];
    for ts in [4usize, 8] {
        let px = capture(
            &mut gpu,
            "\tx\n",
            ts,
            |e| e.render_whitespace(true).current_line(false),
            |_| {},
        );
        let ink = |x0: f64, x1: f64| -> usize {
            let mut n = 0;
            for y in PAD_T as usize..(PAD_T + LINE) as usize {
                for x in x0 as usize..x1 as usize {
                    let p = &px[(y * W as usize + x) * 4..][..3];
                    if p.iter().zip(bg.iter()).any(|(a, b)| a.abs_diff(*b) > 20) {
                        n += 1;
                    }
                }
            }
            n
        };
        // The arrow's shaft crosses the middle of the tab; its head ends at the tab stop.
        let mid = PAD_L + ts as f64 * ADV / 2.0;
        assert!(
            ink(mid - 2.0, mid + 2.0) > 0,
            "ts={ts}: no arrow across the tab"
        );
        assert!(
            ink(PAD_L + (ts as f64 - 1.0) * ADV, PAD_L + ts as f64 * ADV) > 0,
            "ts={ts}: no arrow head"
        );
    }
}

#[test]
fn find_highlights_on_tabbed_lines() {
    assert_tabs_draw_like_spaces(
        "find",
        GO,
        |e| e,
        |h, _| {
            h.click(at(0, 1.0));
            h.press("Ctrl+F");
            h.frames(2);
            for c in "foo".chars() {
                h.key(KeyInput::Insert(c.to_string()));
            }
            h.frames(2);
        },
    );
}

#[test]
fn soft_wrap_counts_tabs_at_their_width() {
    let long = format!("\t\t{}\n\tend\n", "word ".repeat(40));
    assert_tabs_draw_like_spaces(
        "soft wrap",
        &long,
        |e| e.soft_wrap(true),
        |h, ts| {
            h.click(at(1, ts as f64 + 1.0));
        },
    );
}

#[test]
fn minimap_draws_tabs_at_the_tab_size() {
    assert_tabs_draw_like_spaces("minimap", GO, |e| e.minimap(true), |_, _| {});
}

#[test]
fn sticky_scroll_headers_keep_tab_indentation() {
    let mut src = String::from("func main() {\n\tif ok {\n\t\tfor {\n");
    for i in 0..60 {
        src.push_str(&format!("\t\t\tv{i} := {i}\n"));
    }
    src.push_str("\t\t}\n\t}\n}\n");
    assert_tabs_draw_like_spaces(
        "sticky scroll",
        &src,
        |e| e.sticky_scroll(true),
        |h, _| {
            for _ in 0..3 {
                h.wheel(
                    Offset::new(300.0, 150.0),
                    Offset::new(0.0, 300.0),
                    pebbles::render::ScrollUnit::Lines(Offset::new(0.0, 3.0)),
                );
                h.settle();
            }
        },
    );
}

#[test]
fn inline_diff_rows_and_word_marks_on_tabbed_lines() {
    let Ok(_) = OffscreenCapture::new() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    // The base differs in a word on a tabbed line and has a removed tabbed line; each side
    // is compared against its own space expansion.
    let src = "func f() {\n\tfoo := 1\n\tbar(foo)\n}\n";
    let base = "func f() {\n\tfoo := 2\n\t\tgone()\n\tbar(foo)\n}\n";
    for ts in [2usize, 4, 8] {
        let Ok(mut gpu) = OffscreenCapture::new() else {
            return;
        };
        let b1 = create_root_signal(base.to_string());
        let b2 = create_root_signal(expand(base, ts));
        let tabs = capture(
            &mut gpu,
            src,
            ts,
            |e| e.diff_base(b1).inline_diff(true),
            |_| {},
        );
        let spaces = capture(
            &mut gpu,
            &expand(src, ts),
            ts,
            |e| e.diff_base(b2).inline_diff(true),
            |_| {},
        );
        let d = differing(&tabs, &spaces);
        assert!(d == 0, "inline diff, ts={ts}: {d} pixels differ");
    }
}
