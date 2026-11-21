//! Fine-grained re-rendering: scrolling, the caret blink and pointer moves must not
//! re-render the editor component (its whole tree). Only the small slices that read the
//! scroll row / blink phase re-render — the visible rows once per window chunk.

use pebbles::core::create_root_signal;
use pebbles::prelude::*;
use pebbles::render::{RenderScroll, ScrollUnit};
use pebbles_code_editor::{code_editor, debug};
use pebbles_testing::Harness;

const H: f64 = 400.0;

fn mount(minimap: bool) -> Harness {
    pebbles::core::keyboard::set_modifiers(false, false, false, false);
    let src: String = (0..3000)
        .map(|i| format!("fn f{i}() {{\n    let v = {i};\n}}\n"))
        .collect();
    let code = create_root_signal(src);
    let mut h = Harness::new().window(900.0, H);
    h.mount_widget(
        code_editor(code)
            .language(pebbles_code_editor::lang::by_name("rust"))
            .height(H)
            .minimap(minimap)
            .sticky_scroll(true)
            .on_scroll(|_| {}),
    );
    h.frames(3);
    h
}

fn top(h: &Harness) -> f64 {
    let id = h
        .find_all::<RenderScroll>()
        .into_iter()
        .find(|&id| h.object::<RenderScroll>(id).axis == Axis::Vertical)
        .expect("vertical scroll view");
    h.object::<RenderScroll>(id).offset
}

fn slice_renders(name: &str) -> u64 {
    debug::slice_stats()
        .into_iter()
        .find(|e| e.0 == name)
        .map_or(0, |e| e.1)
}

#[test]
fn scrolling_never_rerenders_the_editor_component() {
    for minimap in [false, true] {
        let mut h = mount(minimap);
        let at = Offset::new(300.0, 200.0);
        h.tap(at); // focused: the caret blinks (the worst case)
        h.settle();
        let start = top(&h);
        let r0 = debug::render_count();
        debug::reset_slices();
        // A touchpad stream (pixels) and wheel notches (lines), one frame each.
        for i in 0..200 {
            h.wheel(
                at,
                Offset::new(0.0, [3.0, 7.0, 12.0][i % 3]),
                ScrollUnit::Pixels,
            );
            h.tick(1.0 / 60.0);
        }
        for _ in 0..20 {
            h.wheel(
                at,
                Offset::new(0.0, 1.0),
                ScrollUnit::Lines(Offset::new(0.0, 1.0)),
            );
            h.tick(1.0 / 60.0);
        }
        h.settle();
        let moved = top(&h) - start;
        assert!(moved > 1000.0, "minimap={minimap}: scrolled only {moved}px");
        assert_eq!(
            debug::render_count() - r0,
            0,
            "minimap={minimap}: scrolling re-rendered the editor component"
        );
        // The visible rows re-render once per WINDOW_CHUNK rows, not per frame.
        let line_px = 13.5 * 1.6; // the default font size × line height
        let rows = moved / line_px;
        let row_renders = slice_renders("rows");
        assert!(
            (row_renders as f64) <= rows / 8.0 + 4.0,
            "minimap={minimap}: rows slice rendered {row_renders} times for {rows:.0} rows"
        );
        // Idle (blinking caret, 2 s): only the caret slice re-renders.
        let r1 = debug::render_count();
        debug::reset_slices();
        for _ in 0..120 {
            h.tick(1.0 / 60.0);
        }
        assert_eq!(
            debug::render_count() - r1,
            0,
            "the caret blink re-rendered the editor"
        );
        assert_eq!(
            slice_renders("rows"),
            0,
            "the caret blink re-rendered the rows"
        );
        // Pointer moves (no Ctrl) don't re-render it either.
        let r2 = debug::render_count();
        for i in 0..30 {
            h.hover(Offset::new(200.0 + i as f64 * 5.0, 150.0));
        }
        assert_eq!(
            debug::render_count() - r2,
            0,
            "a pointer move re-rendered the editor"
        );
    }
}

#[test]
fn a_keystroke_rerenders_the_editor_once() {
    let mut h = mount(true);
    h.tap(Offset::new(300.0, 100.0));
    h.settle();
    let r0 = debug::render_count();
    for c in ["a", "b", "c", "Space", "d"] {
        h.press(c);
        h.tick(1.0 / 60.0);
    }
    let n = debug::render_count() - r0;
    assert!(n <= 5, "5 keystrokes re-rendered the editor {n} times");
}
