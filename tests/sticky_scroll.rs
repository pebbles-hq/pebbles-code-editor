//! Sticky scroll draws its pinned headers exactly like the real lines (VS Code
//! `editor.stickyScroll`): the line number right-aligned in a gutter-width cell, the text at
//! the text column's x (indentation and horizontal scroll included), and a click reveals
//! the line. Driven through the headless harness.

use pebbles::core::create_root_signal;
use pebbles::prelude::*;
use pebbles::render::{RenderParagraph, RenderScroll, ScrollUnit};
use pebbles_code_editor::{code_editor, lang};
use pebbles_testing::Harness;

const H: f64 = 300.0;

fn doc() -> String {
    let mut s = String::from("impl Workspace {\n    pub fn record_from(&self, from: Loc) {\n");
    for i in 0..60 {
        s.push_str(&format!("        let v{i} = {i};\n"));
    }
    s.push_str("    }\n    pub fn other(&self) {\n        let tail = 0;\n    }\n}\n");
    s
}

fn mount(
    text: &str,
    cfg: impl FnOnce(pebbles_code_editor::CodeEditor) -> pebbles_code_editor::CodeEditor,
) -> Harness {
    pebbles::core::keyboard::set_modifiers(false, false, false, false);
    let code = create_root_signal(text.to_string());
    let mut h = Harness::new().window(800.0, H);
    h.mount_widget(cfg(code_editor(code)
        .language(lang::by_name("rust"))
        .height(H)
        .sticky_scroll(true)
        .autofocus()));
    h.frames(3);
    h
}

const LINE: f64 = 13.5 * 1.6;

fn vtop(h: &Harness) -> f64 {
    h.find_all::<RenderScroll>()
        .into_iter()
        .map(|id| h.object::<RenderScroll>(id))
        .filter(|s| s.axis == Axis::Vertical)
        .map(|s| s.offset)
        .fold(0.0, f64::max)
}

fn notch(h: &mut Harness, lines: f64) {
    h.wheel(
        Offset::new(300.0, 150.0),
        Offset::new(0.0, lines * 100.0),
        ScrollUnit::Lines(Offset::new(0.0, lines)),
    );
    h.settle();
}

/// Window-space rect (x, y, right) of every paragraph whose text is exactly `t`.
fn at(h: &Harness, t: &str) -> Vec<(f64, f64, f64)> {
    h.find_all_text(t)
        .into_iter()
        .map(|id| {
            let o = h.ui.render_tree().absolute_offset(id);
            (o.x, o.y, o.x + h.size_of(id).width)
        })
        .collect()
}

/// The pinned copy of `t`: the one painted in the header band (y < `rows` lines).
fn pinned(h: &Harness, t: &str, rows: usize) -> (f64, f64, f64) {
    at(h, t)
        .into_iter()
        .find(|p| p.1 >= -0.5 && p.1 < rows as f64 * LINE - 0.5)
        .unwrap_or_else(|| {
            dump(h);
            panic!("no pinned header {t:?}")
        })
}

/// The x of any real (scrolled) code line below the header band.
fn real_text_x(h: &Harness, t: &str) -> (f64, f64, f64) {
    at(h, t)
        .into_iter()
        .find(|p| p.1 > 3.0 * LINE)
        .unwrap_or_else(|| {
            dump(h);
            panic!("no real line {t:?}")
        })
}

fn dump(h: &Harness) {
    let tree = h.ui.render_tree();
    for id in h.find_all::<RenderParagraph>() {
        let o = tree.absolute_offset(id);
        let p = h.object::<RenderParagraph>(id);
        eprintln!("{:?} @ {:.1},{:.1}", p.text, o.x, o.y);
    }
}

const IMPL: &str = "impl Workspace {";
const FN: &str = "    pub fn record_from(&self, from: Loc) {";

/// The owner's report: headers were CENTRED with no line numbers. They sit at the text
/// column's x (the paragraph includes the indentation, like the real lines) with their
/// numbers right-aligned in the gutter column, exactly as the real lines'.
fn assert_aligned(h: &Harness) {
    let real = real_text_x(h, "        let v40 = 40;");
    let i = pinned(h, IMPL, 2);
    let f = pinned(h, FN, 2);
    assert!(
        (i.0 - real.0).abs() < 0.5,
        "impl header x {} = text column {}",
        i.0,
        real.0
    );
    assert!(
        (f.0 - real.0).abs() < 0.5,
        "fn header x {} = text column {}",
        f.0,
        real.0
    );
    assert!(
        i.1.abs() < 0.5 && (f.1 - LINE).abs() < 0.5,
        "rows stacked: {} {}",
        i.1,
        f.1
    );
    // Line numbers 1 and 2, right edges flush with a real number's (right-aligned).
    let n1 = pinned(h, "1", 2);
    let n2 = pinned(h, "2", 2);
    let real_n = real_text_x(h, "43");
    assert!(
        (n1.2 - real_n.2).abs() < 0.5 && (n2.2 - real_n.2).abs() < 0.5,
        "numbers right-aligned"
    );
    assert!(n1.2 < real.0, "numbers in the gutter, left of the text");
}

#[test]
fn headers_sit_on_the_text_column_with_their_line_numbers() {
    let mut h = mount(&doc(), |c| c);
    notch(&mut h, 10.0);
    assert!(vtop(&h) > 100.0);
    assert_aligned(&h);
}

#[test]
fn headers_align_with_the_minimap_on_and_with_soft_wrap() {
    let mut h = mount(&doc(), |c| c.minimap(true));
    notch(&mut h, 10.0);
    assert_aligned(&h);
    // Soft wrap: a long header stays ONE row (VS Code clips it), the next header right under.
    let long = doc().replace(
        "from: Loc)",
        &format!("from: Loc, {})", "x: u8, ".repeat(40)),
    );
    let fn_long = format!(
        "    pub fn record_from(&self, from: Loc, {}) {{",
        "x: u8, ".repeat(40)
    );
    let mut h = mount(&long, |c| c.soft_wrap(true));
    notch(&mut h, 10.0);
    let i = pinned(&h, IMPL, 2);
    let f = pinned(&h, &fn_long, 2);
    assert!(
        (f.1 - i.1 - LINE).abs() < 0.5,
        "one row per header under wrap"
    );
    assert!((i.0 - f.0).abs() < 0.5);
}

#[test]
fn headers_follow_the_horizontal_scroll() {
    let wide = doc().replace(
        "let v30 = 30;",
        &format!("let v30 = 30; // {}", "w".repeat(300)),
    );
    let mut h = mount(&wide, |c| c);
    notch(&mut h, 10.0);
    let before = pinned(&h, FN, 2).0;
    pebbles::core::keyboard::set_modifiers(true, false, false, false);
    h.wheel(
        Offset::new(300.0, 200.0),
        Offset::new(0.0, 100.0),
        ScrollUnit::Lines(Offset::new(0.0, 1.0)),
    );
    pebbles::core::keyboard::set_modifiers(false, false, false, false);
    h.settle();
    let after = pinned(&h, FN, 2).0;
    let real = real_text_x(&h, "        let v31 = 31;").0;
    assert!(
        after < before - 20.0,
        "header scrolled sideways: {before} -> {after}"
    );
    assert!(
        (after - real).abs() < 0.5,
        "with the text: {after} vs {real}"
    );
    // ... while the numbers stay put in the gutter.
    assert!(pinned(&h, "2", 2).2 < before);
}

#[test]
fn headers_with_a_fold_above_pick_the_right_scopes() {
    // fn a() is folded (30 lines collapse to one row); scroll into fn b().
    let mut s = String::from("impl Workspace {\n    fn a() {\n");
    for i in 0..30 {
        s.push_str(&format!("        let a{i} = {i};\n"));
    }
    s.push_str("    }\n    fn b() {\n");
    for i in 0..60 {
        s.push_str(&format!("        let b{i} = {i};\n"));
    }
    s.push_str("    }\n}\n");
    let folds = create_root_signal(std::collections::BTreeSet::from([1usize]));
    let mut h = mount(&s, |c| c.folds(folds));
    notch(&mut h, 8.0);
    pinned(&h, IMPL, 2);
    pinned(&h, "    fn b() {", 2);
    pinned(&h, "34", 2); // fn b()'s real number (line 34), not a folded-away line
}

#[test]
fn clicking_a_header_reveals_its_line() {
    let mut h = mount(&doc(), |c| c);
    notch(&mut h, 10.0);
    // Click the fn header (row 1): the real line 2 lands where the header was.
    h.click(Offset::new(300.0, LINE * 1.5));
    h.settle();
    let t = vtop(&h);
    // y_of(line 1) = pad_t + LINE; header row 1 → scroll = pad_t.
    assert!((t - 10.0).abs() < 1.0, "scrolled to the fn line: {t}");
    let real = at(&h, FN).into_iter().map(|p| p.1).collect::<Vec<_>>();
    assert!(
        real.iter().any(|y| (y - LINE).abs() < 1.0),
        "fn line at row 1: {real:?}"
    );
}

/// Evidence: `SHOT_DIR=… cargo test --test sticky_scroll shot_ -- --ignored` writes the top of
/// a scrolled editor (minimap on, like Gravel) as raw RGBA.
#[test]
#[ignore]
fn shot_sticky_scroll() {
    let Ok(dir) = std::env::var("SHOT_DIR") else {
        return;
    };
    let mut s = String::from("use crate::nav::Loc;\n\nimpl Workspace {\n");
    s.push_str("    pub fn record_from(&self, from: crate::nav::Loc) {\n");
    for i in 0..40 {
        s.push_str(&format!(
            "        let step_{i} = self.nav.borrow().peek({i});\n"
        ));
    }
    s.push_str("    }\n}\n");
    let bg = Color::from_rgb8(0xff, 0xff, 0xff);
    let code = create_root_signal(s);
    let mut h = Harness::new().window(1100.0, 160.0).background(bg);
    h.mount_widget(
        code_editor(code)
            .language(lang::by_name("rust"))
            .height(160.0)
            .minimap(true)
            .sticky_scroll(true),
    );
    h.frames(3);
    h.wheel(
        Offset::new(300.0, 80.0),
        Offset::new(0.0, 700.0),
        ScrollUnit::Lines(Offset::new(0.0, 7.0)),
    );
    h.settle();
    let scene = h.draw();
    let px = pebbles::shell::capture::OffscreenCapture::new()
        .expect("gpu")
        .render_scene(&scene, 1100, 160, bg)
        .expect("render");
    std::fs::write(format!("{dir}/sticky.rgba"), px).unwrap();
}
