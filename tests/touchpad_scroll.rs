//! Touchpad (pixel-delta) scrolling: a stream of small `ScrollUnit::Pixels` deltas — a
//! two-finger swipe followed by the OS's kinetic momentum, which just keeps sending
//! deltas — scrolls the editor 1:1 in logical px and never snaps back. Line deltas (a
//! mouse wheel notch) still scroll by lines.

use pebbles::core::create_root_signal;
use pebbles::prelude::*;
use pebbles::render::{RenderScroll, ScrollUnit};
use pebbles_code_editor::code_editor;
use pebbles_testing::Harness;

const H: f64 = 400.0;

fn lines(n: usize) -> String {
    (0..n)
        .map(|i| format!("let v{i} = {i};"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn mount(smooth: bool, initial: f64) -> Harness {
    pebbles::core::keyboard::set_modifiers(false, false, false, false);
    let code = create_root_signal(lines(2000));
    let mut h = Harness::new().window(800.0, H);
    h.mount_widget(
        code_editor(code)
            .language(pebbles_code_editor::lang::by_name("rust"))
            .height(H)
            .smooth_scrolling(smooth)
            .initial_scroll(initial)
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

/// A swipe of `n` pixel deltas of `step` px, one per frame (no settling in between, as a
/// real touchpad delivers them), then a decaying momentum tail.
fn swipe(h: &mut Harness, step: f64, n: usize) -> f64 {
    let at = Offset::new(300.0, 200.0);
    let mut sent = 0.0;
    let mut prev = top(h);
    for _ in 0..n {
        h.wheel(at, Offset::new(0.0, step), ScrollUnit::Pixels);
        sent += step;
        let now = top(h);
        assert!(
            (now - prev) * step.signum() >= -0.01,
            "snapped back: {prev} -> {now}"
        );
        prev = now;
    }
    let mut v = step;
    while v.abs() > 0.5 {
        v *= 0.9;
        h.wheel(at, Offset::new(0.0, v), ScrollUnit::Pixels);
        sent += v;
        let now = top(h);
        assert!(
            (now - prev) * step.signum() >= -0.01,
            "momentum snapped back: {prev} -> {now}"
        );
        prev = now;
    }
    h.settle();
    sent
}

fn check(smooth: bool, initial: f64) {
    let mut h = mount(smooth, initial);
    let start = top(&h);
    let sent = swipe(&mut h, 6.0, 60);
    let after = top(&h);
    assert!(
        (after - start - sent).abs() < 1.0,
        "pixel deltas scroll 1:1: start {start} sent {sent} got {after}"
    );
    // Back up a bit, then down again: still no jump.
    let back = swipe(&mut h, -4.0, 20);
    assert!(
        (top(&h) - (after + back)).abs() < 1.0,
        "reverse swipe 1:1, got {}",
        top(&h)
    );
    // A wheel notch afterwards scrolls by lines from where the touchpad left it.
    let before = top(&h);
    h.wheel(
        Offset::new(300.0, 200.0),
        Offset::new(0.0, 60.0),
        ScrollUnit::Lines(Offset::new(0.0, 1.0)),
    );
    h.settle();
    assert!(
        top(&h) > before + 10.0,
        "wheel continues from the touchpad position: {before} -> {}",
        top(&h)
    );
}

#[test]
fn touchpad_scroll_is_one_to_one_without_smooth_scrolling() {
    check(false, 0.0);
}

#[test]
fn touchpad_scroll_is_one_to_one_with_smooth_scrolling() {
    check(true, 0.0);
}

#[test]
fn touchpad_scroll_after_a_restored_offset_does_not_jump_back() {
    check(true, 300.0);
    check(false, 300.0);
}

#[test]
fn touchpad_scroll_after_a_wheel_notch_does_not_jump_back() {
    for smooth in [false, true] {
        let mut h = mount(smooth, 0.0);
        h.wheel(
            Offset::new(300.0, 200.0),
            Offset::new(0.0, 60.0),
            ScrollUnit::Lines(Offset::new(0.0, 1.0)),
        );
        h.settle();
        let start = top(&h);
        assert!(start > 10.0);
        let sent = swipe(&mut h, 5.0, 30);
        assert!(
            (top(&h) - start - sent).abs() < 1.0,
            "smooth={smooth}: {start}+{sent} got {}",
            top(&h)
        );
    }
}

/// X11 (XInput2 smooth scrolling) reports a touchpad as FRACTIONAL line deltas, many per
/// frame: they too must accumulate and never snap back.
#[test]
fn fractional_line_deltas_accumulate_without_snapping_back() {
    for smooth in [false, true] {
        let mut h = mount(smooth, 0.0);
        let at = Offset::new(300.0, 200.0);
        let mut prev = top(&h);
        for i in 0..120 {
            let l = 0.05;
            h.wheel(
                at,
                Offset::new(0.0, l * 20.0),
                ScrollUnit::Lines(Offset::new(0.0, l)),
            );
            if i % 3 == 0 {
                h.frame();
            }
            let now = top(&h);
            assert!(
                now >= prev - 0.01,
                "smooth={smooth} step {i}: snapped back {prev} -> {now}"
            );
            prev = now;
        }
        h.settle();
        assert!(
            top(&h) > 100.0,
            "smooth={smooth}: 6 lines' worth scrolled only {}",
            top(&h)
        );
    }
}

/// The owner's report: a long touchpad swipe (with big momentum bursts) runs fine for a
/// while, then — well before the end of the file — jumps back to the top. Swipe the whole
/// document and far past its end: the offset must only ever grow, and end pinned at max.
#[test]
fn long_touchpad_swipe_to_the_end_never_jumps_back_to_the_top() {
    for smooth in [false, true] {
        for beyond in [false, true] {
            pebbles::core::keyboard::set_modifiers(false, false, false, false);
            let code = create_root_signal(lines(3000));
            let mut h = Harness::new().window(800.0, H);
            h.mount_widget(
                code_editor(code)
                    .language(pebbles_code_editor::lang::by_name("rust"))
                    .height(H)
                    .smooth_scrolling(smooth)
                    .scroll_beyond_last_line(beyond)
                    .minimap(true)
                    .on_scroll(|_| {}),
            );
            h.frames(3);
            let at = Offset::new(300.0, 200.0);
            let mut prev = top(&h);
            for i in 0..2000 {
                // Swipes of growing size plus momentum bursts up to 400 px per event.
                let d = [8.0, 30.0, 120.0, 400.0, 2.5][i % 5];
                h.wheel(at, Offset::new(0.0, d), ScrollUnit::Pixels);
                let now = top(&h);
                assert!(
                    now >= prev - 0.01,
                    "smooth={smooth} beyond={beyond} event {i}: jumped back {prev} -> {now}"
                );
                prev = now;
            }
            h.settle();
            let id = h
                .find_all::<RenderScroll>()
                .into_iter()
                .find(|&id| h.object::<RenderScroll>(id).axis == Axis::Vertical)
                .unwrap();
            let s = h.object::<RenderScroll>(id);
            assert!(s.max_offset > 3000.0 * 10.0, "max {}", s.max_offset);
            assert!(
                (s.offset - s.max_offset).abs() < 1.0,
                "smooth={smooth} beyond={beyond}: ends at {} of {}",
                s.offset,
                s.max_offset
            );
        }
    }
}

/// Root cause of the owner's "jumps back to the top": once a scope header (sticky scroll)
/// or the horizontal bar appeared, the editor wrapped its scroll view in a stack — a new
/// tree position, so the scroll view was REMOUNTED at offset 0. Scroll a file whose first
/// line opens a scope (and that has one long line further down), ticking the springs as
/// the shell does, with the caret at 1:1: the offset must only grow.
#[test]
fn sticky_header_or_hbar_appearing_does_not_remount_the_scroll_view() {
    for smooth in [false, true] {
        pebbles::core::keyboard::set_modifiers(false, false, false, false);
        let mut src = String::from("fn main() {\n");
        for i in 0..1500 {
            if i == 300 {
                src.push_str(&format!("    let long = \"{}\";\n", "x".repeat(400)));
            }
            src.push_str(&format!("    let v{i} = {i};\n"));
        }
        src.push_str("}\n");
        let code = create_root_signal(src);
        let mut h = Harness::new().window(800.0, H);
        h.mount_widget(
            code_editor(code)
                .language(pebbles_code_editor::lang::by_name("rust"))
                .height(H)
                .smooth_scrolling(smooth)
                .sticky_scroll(true)
                .on_scroll(|_| {}),
        );
        h.frames(3);
        let at = Offset::new(300.0, 200.0);
        let mut prev = top(&h);
        for i in 0..900 {
            let d = [6.0, 12.0, 30.0, 3.0][i % 4];
            h.wheel(at, Offset::new(0.0, d), ScrollUnit::Pixels);
            h.tick(1.0 / 60.0);
            let now = top(&h);
            assert!(
                now >= prev - 0.01,
                "smooth={smooth} event {i}: jumped back {prev} -> {now}"
            );
            prev = now;
        }
        h.settle();
        assert!(top(&h) > 9000.0, "smooth={smooth}: ended at {}", top(&h));
    }
}
