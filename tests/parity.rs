//! VS Code parity features driven through the headless harness: gutter change bars + peek +
//! revert/stage, the overview ruler, relative line numbers, linked tag editing, links,
//! colour picker, Unicode highlighting, cursor styles, folded badges.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use pebbles::core::create_root_signal;
use pebbles::prelude::*;
use pebbles_code_editor::{
    ChangeHunk, CursorBlinking, CursorStyle, LineNumbers, code_editor, editor_controller, lang,
};
use pebbles_testing::Harness;

const LINE: f64 = 13.5 * 1.6;
fn row_y(line: usize) -> f64 {
    10.0 + line as f64 * LINE + LINE / 2.0
}

fn tap_label(h: &mut Harness, label: &str) {
    let node = h
        .semantics_tree()
        .into_iter()
        .find(|n| n.props.label == label)
        .unwrap_or_else(|| panic!("no {label:?} in the semantics tree"));
    if std::env::var("DBG").is_ok() {
        for n in h.semantics_tree() {
            eprintln!("{:?} {:?} {:?}", n.props.role, n.props.label, n.bounds);
        }
    }
    let c = node.bounds.center();
    h.click(Offset::new(c.x, c.y));
}

#[test]
fn change_bars_peek_revert_and_stage() {
    let base = create_root_signal(String::from("a\nb\nc\nd"));
    let code = create_root_signal(String::from("a\nX\nc\nd\ne"));
    let staged: Rc<RefCell<Vec<(ChangeHunk, String)>>> = Rc::default();
    let st = staged.clone();
    let mut h = Harness::new().window(700.0, 400.0);
    h.mount_widget(
        code_editor(code)
            .height(380.0)
            .change_baseline(base)
            .on_stage_hunk(move |hk, text| st.borrow_mut().push((hk.clone(), text.to_string())))
            .autofocus(),
    );
    h.frames(3);
    // Click the modified bar on line 1 (far left of the gutter): the peek opens.
    h.tap(Offset::new(1.0, row_y(1)));
    h.frames(2);
    assert!(h.find_text("Modified · change 1 of 2"), "peek header");
    assert!(h.find_text("- b"), "base line shown");
    tap_label(&mut h, "Stage");
    h.frames(2);
    assert_eq!(staged.borrow().len(), 1);
    assert_eq!(staged.borrow()[0].1, "X");
    // Reopen and revert.
    h.tap(Offset::new(1.0, row_y(1)));
    h.frames(2);
    tap_label(&mut h, "Revert");
    h.frames(2);
    assert_eq!(code.get(), "a\nb\nc\nd\ne");
    // The added last line: revert removes it.
    h.tap(Offset::new(1.0, row_y(4)));
    h.frames(2);
    assert!(h.find_text("Added · change 1 of 1"));
    tap_label(&mut h, "Revert");
    h.frames(2);
    assert_eq!(code.get(), "a\nb\nc\nd");
}

#[test]
fn relative_line_numbers() {
    let code = create_root_signal(String::from("a\nb\nc\nd"));
    let ctrl = editor_controller();
    let mut h = Harness::new().window(500.0, 300.0);
    h.mount_widget(
        code_editor(code)
            .height(280.0)
            .line_numbers(LineNumbers::Relative)
            .controller(ctrl),
    );
    ctrl.set_cursor(2, 0);
    h.frames(3);
    // Caret line shows its absolute number (3); the others show distances 2, 1, 1.
    assert!(h.find_text("3"));
    assert_eq!(h.find_all_text("1").len(), 2);
    assert!(h.find_text("2"));
    assert!(!h.find_text("4"));
}

#[test]
fn linked_editing_renames_the_closing_tag() {
    let code = create_root_signal(String::from("<div>x</div>"));
    let ctrl = editor_controller();
    let mut h = Harness::new().window(500.0, 300.0);
    h.mount_widget(
        code_editor(code)
            .language(lang::by_name("html"))
            .height(280.0)
            .controller(ctrl)
            .autofocus(),
    );
    h.frames(2);
    ctrl.set_cursor(0, 4); // after "<div"
    h.frames(2);
    h.key(KeyInput::Backspace);
    h.key(KeyInput::Backspace);
    h.key(KeyInput::Backspace);
    for c in ["s", "p", "a", "n"] {
        h.key(KeyInput::Insert(c.into()));
    }
    assert_eq!(code.get(), "<span>x</span>");
    // One undo step per keystroke pair: undo restores both tags together.
    h.key(KeyInput::Undo);
    assert_eq!(code.get(), "<spa>x</spa>");
}

#[test]
fn ctrl_click_opens_links() {
    let code = create_root_signal(String::from("see https://example.com/x now"));
    let opened: Rc<RefCell<Vec<String>>> = Rc::default();
    let o = opened.clone();
    let mut h = Harness::new().window(700.0, 300.0);
    h.mount_widget(
        code_editor(code)
            .gutter(false)
            .height(280.0)
            .on_open_link(move |u| o.borrow_mut().push(u.to_string()))
            .autofocus(),
    );
    h.frames(2);
    pebbles::core::keyboard::set_modifiers(false, true, false, false);
    let adv = 13.5 * 0.6;
    h.click(Offset::new(14.0 + 10.0 * adv, row_y(0)));
    pebbles::core::keyboard::set_modifiers(false, false, false, false);
    assert_eq!(opened.borrow().as_slice(), ["https://example.com/x"]);
}

#[test]
fn colour_swatch_opens_picker_and_applies() {
    let code = create_root_signal(String::from("a { color: #ff0000; }"));
    let mut h = Harness::new().window(700.0, 400.0);
    h.mount_widget(
        code_editor(code)
            .language(lang::by_name("css"))
            .gutter(false)
            .height(380.0),
    );
    h.frames(2);
    // The swatch sits just left of the literal (column 11).
    let adv = 13.5 * 0.6;
    let sz = (LINE * 0.55).min(adv * 1.2);
    h.tap(Offset::new(14.0 + 11.0 * adv - sz / 2.0 - 1.0, row_y(0)));
    h.frames(2);
    // The picker's hex field: focus it, replace the value, submit.
    let field = h
        .semantics_tree()
        .into_iter()
        .find(|n| n.props.value.as_deref() == Some("#ff0000"))
        .expect("picker hex field");
    let c = field.bounds.center();
    h.click(Offset::new(c.x, c.y));
    h.key(KeyInput::SelectAll);
    h.key(KeyInput::Insert("#00ff00".into()));
    h.key(KeyInput::Enter);
    h.frames(2);
    assert_eq!(code.get(), "a { color: #00ff00; }");
}

#[test]
fn folding_shows_a_line_count_badge() {
    let code = create_root_signal(String::from("fn a() {\n    x\n    y\n}\n"));
    let folds = create_root_signal(BTreeSet::from([0usize]));
    let mut h = Harness::new().window(500.0, 300.0);
    h.mount_widget(code_editor(code).height(280.0).folds(folds));
    h.frames(3);
    assert!(h.find_text("⋯ 2 lines"));
}

#[test]
fn cursor_styles_and_blinking_render() {
    for style in [
        CursorStyle::Line,
        CursorStyle::LineThin,
        CursorStyle::Block,
        CursorStyle::BlockOutline,
        CursorStyle::Underline,
        CursorStyle::UnderlineThin,
    ] {
        for blink in [
            CursorBlinking::Blink,
            CursorBlinking::Smooth,
            CursorBlinking::Phase,
            CursorBlinking::Expand,
            CursorBlinking::Solid,
        ] {
            let code = create_root_signal(String::from("abc\n\u{200B}x\u{1}"));
            let mut h = Harness::new().window(400.0, 200.0);
            h.mount_widget(
                code_editor(code)
                    .height(180.0)
                    .cursor_style(style)
                    .cursor_blinking(blink)
                    .smooth_caret(true)
                    .autofocus(),
            );
            h.frames(2);
            h.key(KeyInput::Move {
                motion: Motion::Right,
                extend: false,
            });
            h.frames(2);
        }
    }
}

#[test]
fn control_characters_render_as_symbols() {
    let code = create_root_signal(String::from("a\u{1}b"));
    let mut h = Harness::new().window(400.0, 200.0);
    h.mount_widget(code_editor(code).height(180.0));
    h.frames(2);
    assert!(h.find_text("␁"));
}

#[test]
fn hovering_the_fold_badge_previews_the_hidden_lines() {
    let code = create_root_signal(String::from("fn a() {\n    hidden_x\n    hidden_y\n}\n"));
    let folds = create_root_signal(BTreeSet::from([0usize]));
    let mut h = Harness::new().window(500.0, 300.0);
    h.mount_widget(View::new(
        Color::from_rgba8(255, 255, 255, 255),
        pebbles::widgets::overlay::OverlayHost::wrap(code_editor(code).height(280.0).folds(folds)),
    ));
    h.frames(3);
    assert!(!h.find_text("    hidden_x"));
    // Sweep the first line until the pointer is over the badge and the preview opens.
    let mut shown = false;
    for (x, y) in (0..400).step_by(6).flat_map(|x| [(x, 8), (x, 14), (x, 20)]) {
        h.hover(Offset::new(x as f64, y as f64));
        h.advance(0.8);
        h.frames(2);
        if h.find_text("    hidden_x") && h.find_text("    hidden_y") {
            shown = true;
            break;
        }
    }
    assert!(shown, "fold preview tooltip lists the hidden lines");
    assert!(h.find_text("⋯ 2 lines"));
}

#[test]
fn breakpoint_margin_toggles_and_breakpoints_follow_edits() {
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    let code = create_root_signal(String::from("a\nb\nc\n"));
    let bps = create_root_signal(BTreeSet::from([2usize]));
    let mut h = Harness::new().window(500.0, 300.0);
    h.mount_widget(code_editor(code).height(280.0).breakpoints(bps).autofocus());
    h.frames(3);
    // Clicking down the glyph margin toggles a breakpoint on some line; a second click
    // on the same spot toggles it off again.
    let mut hit = None;
    for y in (2..80).step_by(3) {
        let at = Offset::new(8.0, y as f64);
        h.click(at);
        if bps.get() != BTreeSet::from([2usize]) {
            hit = Some(at);
            break;
        }
    }
    let at = hit.expect("a click in the margin toggles a breakpoint");
    let toggled = bps.get();
    h.click(at);
    assert_eq!(bps.get(), BTreeSet::from([2usize]), "{toggled:?}");
    bps.set(BTreeSet::from([1, 2]));
    h.frames(1);
    // Inserting a line at the top shifts both down.
    code.set(String::from("x\na\nb\nc\n"));
    h.frames(2);
    assert_eq!(bps.get(), BTreeSet::from([2, 3]));
    // Deleting the line holding a breakpoint collapses it onto the edit line.
    code.set(String::from("x\na\nc\n"));
    h.frames(2);
    assert_eq!(bps.get(), BTreeSet::from([2]));
}
