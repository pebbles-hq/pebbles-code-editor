//! The editor's scrollbars behave like VS Code's (`editor.scrollbar`), driven through the
//! headless harness, which routes presses the way the shell does (a bar under the pointer
//! takes the press before the text).
//!
//! Layout used throughout: an 800×400 window, a 400 px editor without the gutter. The ONE
//! vertical bar is the 14 px strip x 786..800 at the right edge (the overview ruler drawn in
//! its track), so the text spans x 0..786; the horizontal bar is the strip y 386..400 over
//! x 0..786. With the minimap on, it sits between the text and the bar (x 702..786).

use std::collections::BTreeSet;

use pebbles::core::create_root_signal;
use pebbles::prelude::*;
use pebbles::render::{PointerButton, RenderScroll, ScrollUnit, ScrollbarStyle};
use pebbles_code_editor::{EditorController, code_editor, editor_controller, lang};
use pebbles_testing::Harness;

const LINE: f64 = 13.5 * 1.6;
const PAD: f64 = 10.0;
const H: f64 = 400.0;
const BAR_X: f64 = 793.0;
const HBAR_Y: f64 = 393.0;

struct Ed {
    h: Harness,
    ctrl: EditorController,
    folds: Signal<BTreeSet<usize>>,
}

fn lines(n: usize) -> String {
    (0..n)
        .map(|i| format!("let v{i} = {i};"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn mount(
    text: &str,
    cfg: impl FnOnce(pebbles_code_editor::CodeEditor) -> pebbles_code_editor::CodeEditor,
) -> Ed {
    pebbles::core::keyboard::set_modifiers(false, false, false, false);
    let code = create_root_signal(text.to_string());
    let folds = create_root_signal(BTreeSet::new());
    let ctrl = editor_controller();
    let mut h = Harness::new().window(800.0, H);
    h.mount_widget(cfg(code_editor(code)
        .language(lang::by_name("rust"))
        .height(H)
        .gutter(false)
        .folds(folds)
        .controller(ctrl)
        .autofocus()));
    h.frames(3);
    Ed { h, ctrl, folds }
}

impl Ed {
    fn view(&self, axis: Axis) -> &RenderScroll {
        let h = &self.h;
        let id = h
            .find_all::<RenderScroll>()
            .into_iter()
            .find(|&id| h.object::<RenderScroll>(id).axis == axis)
            .expect("scroll view");
        h.object::<RenderScroll>(id)
    }
    fn top(&self) -> f64 {
        self.view(Axis::Vertical).offset
    }
    fn left(&self) -> f64 {
        self.view(Axis::Horizontal).offset
    }
    /// (viewport, content) of a viewport.
    fn extent(&self, axis: Axis) -> (f64, f64) {
        let v = self.view(axis);
        (v.viewport_extent, v.viewport_extent + v.max_offset)
    }
    /// The bar geometry (track start, track len, thumb start, thumb len) for an axis.
    fn thumb(&self, axis: Axis) -> (f64, f64, f64, f64) {
        let (vp, content) = self.extent(axis);
        let off = self.view(axis).offset;
        bar().geometry(vp, content, off)
    }
}

fn bar() -> ScrollbarStyle {
    ScrollbarStyle {
        thickness: 10.0,
        margin: 2.0,
        min_thumb: 20.0,
        ..ScrollbarStyle::default()
    }
}

fn notch(e: &mut Ed, at: Offset, lines: f64) {
    e.h.wheel(
        at,
        Offset::new(0.0, lines * 100.0),
        ScrollUnit::Lines(Offset::new(0.0, lines)),
    );
    e.h.settle();
}

#[test]
fn thumb_measures_viewport_over_content_including_scroll_beyond_last_line() {
    let e = mount(&lines(500), |c| c);
    let (vp, content) = e.extent(Axis::Vertical);
    assert_eq!(vp, H);
    // VS Code scrollBeyondLastLine: the last line can scroll to the top of the viewport.
    let max = content - vp;
    let last_line_top = PAD + 499.0 * LINE;
    assert!(
        (max - last_line_top).abs() < 1.0,
        "max {max} vs last line top {last_line_top}"
    );
    let (_, track, _, len) = e.thumb(Axis::Vertical);
    let want = (track * vp / content).max(20.0);
    assert!(
        (len - want).abs() < 1e-6,
        "thumb = track × viewport / content (min 20): {len} vs {want}"
    );

    let off = mount(&lines(500), |c| c.scroll_beyond_last_line(false));
    let (_, content) = off.extent(Axis::Vertical);
    assert!(
        (content - (500.0 * LINE + 2.0 * PAD)).abs() < 1.0,
        "no beyond room when disabled"
    );
}

#[test]
fn wheel_notch_scrolls_three_lines_and_the_thumb_follows() {
    let mut e = mount(&lines(500), |c| c);
    notch(&mut e, Offset::new(200.0, 200.0), 1.0);
    assert!(
        (e.top() - 3.0 * LINE).abs() < 0.5,
        "one notch = 3 lines: {}",
        e.top()
    );
    let (ts, _, start, _) = e.thumb(Axis::Vertical);
    assert!(start > ts, "the thumb moved off the top");
    notch(&mut e, Offset::new(200.0, 200.0), -5.0);
    assert_eq!(e.top(), 0.0, "clamped at the top");
}

#[test]
fn smooth_scrolling_animates_the_wheel_and_lands_on_the_lines() {
    let mut e = mount(&lines(500), |c| c.smooth_scrolling(true));
    e.h.wheel(
        Offset::new(200.0, 200.0),
        Offset::new(0.0, 100.0),
        ScrollUnit::Lines(Offset::new(0.0, 1.0)),
    );
    e.h.tick(0.016);
    let mid = e.top();
    assert!(mid < 3.0 * LINE - 0.5, "animating, not jumped: {mid}");
    e.h.advance(0.5);
    assert!((e.top() - 3.0 * LINE).abs() < 0.5, "landed: {}", e.top());
}

#[test]
fn dragging_the_vertical_thumb_scrolls_proportionally_and_follows_outside_the_bar() {
    let mut e = mount(&lines(500), |c| c);
    let (_, track, start, len) = e.thumb(Axis::Vertical);
    let (vp, content) = e.extent(Axis::Vertical);
    let ratio = (content - vp) / (track - len);
    let y0 = start + len - 2.0; // near the thumb's bottom: grabbing must not jump
    e.h.button_down(Offset::new(BAR_X, y0), PointerButton::Primary);
    assert_eq!(e.top(), 0.0, "pressing the thumb does not move it");
    e.h.pointer_move(Offset::new(BAR_X, y0 + 50.0));
    assert!(
        (e.top() - 50.0 * ratio).abs() < 1.0,
        "proportional: {} vs {}",
        e.top(),
        50.0 * ratio
    );
    e.h.pointer_move(Offset::new(300.0, y0 + 100.0)); // far left of the bar, over the text
    assert!(
        (e.top() - 100.0 * ratio).abs() < 1.0,
        "follows outside the bar: {}",
        e.top()
    );
    e.h.button_up(Offset::new(300.0, y0 + 100.0), PointerButton::Primary);
    assert_eq!(
        e.ctrl.cursor().get(),
        (0, 0),
        "the drag never reached the text"
    );
}

#[test]
fn clicking_the_track_jumps_the_thumb_to_the_click() {
    let mut e = mount(&lines(500), |c| c);
    e.h.click(Offset::new(BAR_X, 300.0));
    let (_, _, start, len) = e.thumb(Axis::Vertical);
    assert!(
        (start + len / 2.0 - 300.0).abs() < 1.0,
        "thumb centred on the click: {}",
        start + len / 2.0
    );
    assert_eq!(e.ctrl.cursor().get(), (0, 0), "the caret did not move");
}

#[test]
fn bar_hover_state_and_arrow_cursor() {
    let mut e = mount(&lines(500), |c| c);
    e.h.hover(Offset::new(BAR_X, 100.0));
    assert!(
        e.h.ui.cursor_at(Offset::new(BAR_X, 100.0)).is_none(),
        "arrow, not the I-beam"
    );
    assert!(
        e.h.ui.cursor_at(Offset::new(300.0, 100.0)).is_some(),
        "the I-beam over the text"
    );
}

#[test]
fn text_stops_short_of_the_bar_and_keeps_its_clicks() {
    let mut e = mount(&lines(500), |c| c.soft_wrap(true));
    // Wrapped text fits the area left of the bar: no horizontal overflow at all.
    assert!(
        e.h.find_all::<RenderScroll>()
            .iter()
            .all(|&id| e.h.object::<RenderScroll>(id).axis == Axis::Vertical)
    );
    // A click just left of the bar strip lands on the text (line 3).
    e.h.click(Offset::new(768.0, PAD + 3.0 * LINE + LINE / 2.0));
    assert_eq!(
        e.ctrl.cursor().get().0,
        3,
        "caret placed by a click beside the bar"
    );
    assert_eq!(e.top(), 0.0, "and nothing scrolled");
}

#[test]
fn horizontal_bar_sits_at_the_viewport_bottom_and_drags_the_long_lines() {
    let long = format!("{}\n{}", "x".repeat(400), lines(499));
    let mut e = mount(&long, |c| c);
    let (vp, content) = e.extent(Axis::Horizontal);
    assert!(content > vp + 100.0, "long line overflows");
    let (_, track, start, len) = e.thumb(Axis::Horizontal);
    let ratio = (content - vp) / (track - len);
    let x0 = start + 3.0;
    e.h.drag(
        Offset::new(x0, HBAR_Y),
        Offset::new(x0 + 60.0, HBAR_Y - 200.0),
    );
    assert!(
        (e.left() - 60.0 * ratio).abs() < 2.0,
        "proportional: {} vs {}",
        e.left(),
        60.0 * ratio
    );
    assert_eq!(e.top(), 0.0, "the vertical offset is untouched");
    // A click on the horizontal track jumps there.
    e.h.click(Offset::new(400.0, HBAR_Y));
    let (_, _, start, len) = e.thumb(Axis::Horizontal);
    assert!(
        (start + len / 2.0 - 400.0).abs() < 2.0,
        "track click centres the thumb: {}",
        start + len / 2.0
    );
}

#[test]
fn shift_wheel_scrolls_horizontally() {
    let long = format!("{}\n{}", "x".repeat(400), lines(499));
    let mut e = mount(&long, |c| c);
    pebbles::core::keyboard::set_modifiers(true, false, false, false);
    e.h.wheel(
        Offset::new(200.0, 200.0),
        Offset::new(0.0, 100.0),
        ScrollUnit::Lines(Offset::new(0.0, 1.0)),
    );
    pebbles::core::keyboard::set_modifiers(false, false, false, false);
    e.h.settle();
    assert!(e.left() > 50.0, "Shift+wheel went sideways: {}", e.left());
    assert_eq!(e.top(), 0.0, "and not down");
}

#[test]
fn thumb_follows_resize_folding_and_wrap() {
    let blocks: String = (0..100)
        .map(|i| format!("fn f{i}() {{\n    a();\n    b();\n}}\n"))
        .collect();
    let mut e = mount(&blocks, |c| c);
    let (_, before) = e.extent(Axis::Vertical);
    e.folds.set((0..100).map(|i| i * 4).collect());
    e.h.frames(3);
    let (_, folded) = e.extent(Axis::Vertical);
    assert!(
        (before - folded - 300.0 * LINE).abs() < 1.0,
        "folding 300 rows shrinks the extent: {before} → {folded}"
    );

    let long = format!("{}\n{}", "x".repeat(400), lines(499));
    let mut e = mount(&long, |c| c);
    let (vp, content) = e.extent(Axis::Horizontal);
    e.h.resize(600.0, H);
    e.h.frames(3);
    let (vp2, content2) = e.extent(Axis::Horizontal);
    assert!(
        (vp - vp2 - 200.0).abs() < 1.0,
        "the viewport follows the window: {vp} → {vp2}"
    );
    assert!((content - content2).abs() < 1.0, "same content");
    let (_, track, _, len) = e.thumb(Axis::Horizontal);
    assert!(
        (len - track * vp2 / content2).abs() < 1e-6,
        "thumb re-measured"
    );

    let mut e = mount(&long, |c| c.soft_wrap(true));
    let (_, wrapped) = e.extent(Axis::Vertical);
    let plain = PAD + 499.0 * LINE + H; // unwrapped scroll extent with beyond room
    assert!(
        wrapped > plain + LINE,
        "wrapping the long line adds rows to the extent"
    );
    e.h.frames(1);
}

#[test]
fn overview_ruler_is_the_bar_track_and_a_click_there_centres_the_thumb() {
    // VS Code draws the overview ruler inside the scrollbar: a click on it is a track click.
    let mut e = mount(&lines(500), |c| c.overview_ruler(true));
    e.h.click(Offset::new(BAR_X, 200.0));
    let (_, _, start, len) = e.thumb(Axis::Vertical);
    assert!(
        (start + len / 2.0 - 200.0).abs() < 1.0,
        "thumb centred on the click: {}",
        start + len / 2.0
    );
}

#[test]
fn sticky_headers_leave_the_bar_reachable() {
    let body: String = (0..400).map(|i| format!("    let v{i} = {i};\n")).collect();
    let mut e = mount(&format!("fn main() {{\n{body}}}\n"), |c| {
        c.sticky_scroll(true)
    });
    notch(&mut e, Offset::new(200.0, 200.0), 30.0);
    assert!(e.top() > 0.0);
    // The sticky header covers the top rows of the text — not the bar beside it.
    e.h.click(Offset::new(BAR_X, 6.0));
    assert!(
        e.top() < 1.0,
        "track click at the top went to the top: {}",
        e.top()
    );
}

/// Gravel's layout: the minimap ON (84 px) and the overview ruler. VS Code draws ONE vertical
/// bar, at the far right edge, right of the minimap, with the ruler's marks in its track. The
/// scroll view's own bar (which sat at the text's right edge, left of the minimap) must not
/// exist: two bars driving one offset fought each other.
#[test]
fn minimap_on_one_bar_at_the_far_right_and_the_minimap_slider_drags() {
    let mut e = mount(&lines(500), |c| c.minimap(true));
    // No scroll-view bar anywhere (the editor owns the one vertical bar).
    for &id in &e.h.find_all::<RenderScroll>() {
        assert_eq!(
            e.h.object::<RenderScroll>(id).scrollbar.policy,
            pebbles::render::ScrollbarPolicy::Hidden,
            "a scroll view paints its own bar"
        );
    }
    // The old inner bar's strip (left of the minimap: x 688..702) is text now: a press there
    // places the caret and scrolls nothing.
    e.h.click(Offset::new(695.0, PAD + 3.0 * LINE + LINE / 2.0));
    assert_eq!(
        e.ctrl.cursor().get().0,
        3,
        "the strip left of the minimap is text"
    );
    assert_eq!(e.top(), 0.0);

    // Dragging the one bar (far right) scrolls proportionally; the thumb stays under the
    // pointer at every step (no jitter).
    let (_, track, start, len) = e.thumb(Axis::Vertical);
    let (vp, content) = e.extent(Axis::Vertical);
    let ratio = (content - vp) / (track - len);
    let y0 = start + len - 2.0;
    e.h.button_down(Offset::new(BAR_X, y0), PointerButton::Primary);
    assert_eq!(e.top(), 0.0, "pressing the thumb does not move it");
    let mut last = 0.0;
    for k in 1..=10 {
        let dy = k as f64 * 10.0;
        e.h.pointer_move(Offset::new(BAR_X, y0 + dy));
        let t = e.top();
        assert!(
            (t - dy * ratio).abs() < 1.0,
            "step {k}: {t} vs {}",
            dy * ratio
        );
        assert!(t > last, "monotonic while dragging down");
        last = t;
        let (_, _, s, _) = e.thumb(Axis::Vertical);
        assert!(
            (s - (start + dy)).abs() < 1.0,
            "thumb follows the pointer: {s}"
        );
    }
    e.h.button_up(Offset::new(BAR_X, y0 + 100.0), PointerButton::Primary);
    assert_eq!(
        e.ctrl.cursor().get().0,
        3,
        "the drag never reached the text"
    );

    // Clicking the minimap jumps there (centred), the bar follows.
    e.h.click(Offset::new(744.0, 200.0));
    let after = e.top();
    assert!(after > last + 1.0, "minimap click scrolled: {after}");
    e.h.frames(2);
    assert_eq!(e.top(), after, "stable after the jump (no bar fighting it)");
}

/// Minimap OFF: the one bar sits right of the text, at the editor's right edge.
#[test]
fn minimap_off_the_bar_sits_at_the_text_right_edge() {
    let mut e = mount(&lines(500), |c| c.minimap(false));
    e.h.click(Offset::new(BAR_X, 300.0));
    let (_, _, start, len) = e.thumb(Axis::Vertical);
    assert!(
        (start + len / 2.0 - 300.0).abs() < 1.0,
        "track click centres the thumb"
    );
    e.h.click(Offset::new(780.0, PAD + LINE / 2.0 + 0.0));
    assert!(e.top() > 0.0, "still scrolled; the click at x 780 was text");
}

/// Evidence capture (not a check): `SHOT_DIR=<dir> cargo test --test scrollbar -- --ignored
/// shot_` renders the editor (minimap on / off) to raw RGBA `<dir>/editor-{on,off}.rgba`.
#[test]
#[ignore]
fn shot_editor_scrollbar() {
    let Ok(dir) = std::env::var("SHOT_DIR") else {
        return;
    };
    for (name, mm) in [("on", true), ("off", false)] {
        let code = create_root_signal(lines(500));
        let w = code_editor(code)
            .language(lang::by_name("rust"))
            .height(300.0)
            .minimap(mm)
            .overview_ruler(true)
            .into_widget();
        let px =
            pebbles::shell::capture::capture_rgba(w, 480, 300, Color::from_rgb8(0x1e, 0x1e, 0x1e))
                .expect("capture");
        std::fs::write(format!("{dir}/editor-{name}.rgba"), px).unwrap();
    }
}
