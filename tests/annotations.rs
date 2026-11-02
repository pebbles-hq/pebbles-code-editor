//! Host annotations and menu contributions (headless): end-of-line "after text" with its
//! hover card, the host gutter column, and host items merged into the right-click menu.

use std::cell::RefCell;
use std::rc::Rc;

use pebbles::core::create_root_signal;
use pebbles::prelude::*;
use pebbles_code_editor::{
    AnnotationAction, AnnotationHover, EditorMenuItem, GutterAnnotation, GutterAnnotations,
    LineAnnotation, code_editor,
};
use pebbles_testing::Harness;

fn labels(h: &Harness) -> Vec<String> {
    h.semantics_tree()
        .into_iter()
        .map(|n| n.props.label)
        .filter(|l| !l.is_empty())
        .collect()
}

fn center(h: &Harness, pred: impl Fn(&str) -> bool) -> Offset {
    let n = h
        .semantics_tree()
        .into_iter()
        .rfind(|n| pred(&n.props.label))
        .unwrap_or_else(|| panic!("no such node; labels: {:?}", labels(h)));
    let b = n.bounds;
    Offset::new((b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0)
}

#[test]
fn after_text_annotation_shows_and_hover_card_runs_actions() {
    let code = create_root_signal(String::from("fn main() {\n    let x = 1;\n}\n"));
    let anns = create_root_signal(Vec::<LineAnnotation>::new());
    let ran = Rc::new(RefCell::new(0));
    let mut h = Harness::new().window(800.0, 300.0);
    h.mount_widget(code_editor(code).height(300.0).line_annotations(anns));
    h.frames(3);
    assert!(!labels(&h).iter().any(|l| l.starts_with("Annotation")));

    let r = ran.clone();
    anns.set(vec![
        LineAnnotation::new(1, "Ada, 3 days ago • Add x").hover(AnnotationHover {
            title: "Ada Lovelace, 3 days ago".into(),
            body: vec!["Commit abc".into(), String::new(), "Add x".into()],
            actions: vec![AnnotationAction::new("Copy SHA", move || {
                *r.borrow_mut() += 1
            })],
        }),
    ]);
    h.frames(2);
    assert!(
        labels(&h)
            .iter()
            .any(|l| l == "Annotation Ada, 3 days ago • Add x"),
        "{:?}",
        labels(&h)
    );
    // It sits after the line's text (to the right of "    let x = 1;").
    let at = center(&h, |l| l.starts_with("Annotation Ada"));
    let line1 = h.find_all_text("    let x = 1;");
    assert!(!line1.is_empty());

    // Hovering it opens the card; the action runs and closes it.
    h.hover(at);
    h.frames(2);
    let ls = labels(&h);
    assert!(
        ls.iter()
            .any(|l| l == "Annotation hover Ada Lovelace, 3 days ago"),
        "{ls:?}"
    );
    assert!(ls.iter().any(|l| l == "Annotation hover line Commit abc"));
    let btn = center(&h, |l| l == "Copy SHA");
    h.click(btn);
    h.frames(2);
    assert_eq!(*ran.borrow(), 1);
    assert!(
        !labels(&h)
            .iter()
            .any(|l| l.starts_with("Annotation hover "))
    );
}

#[test]
fn gutter_column_cells_render_and_click() {
    let code = create_root_signal(String::from("a\nb\nc\n"));
    let col = create_root_signal(None::<GutterAnnotations>);
    let hit = Rc::new(RefCell::new(None::<usize>));
    let mut h = Harness::new().window(800.0, 300.0);
    h.mount_widget(code_editor(code).height(300.0).gutter_annotations(col));
    h.frames(3);
    assert!(!labels(&h).iter().any(|l| l.starts_with("Blame line")));
    let items = (0..3)
        .map(|line| {
            let hit = hit.clone();
            GutterAnnotation {
                line,
                text: if line == 0 {
                    "Ada".into()
                } else {
                    String::new()
                },
                detail: if line == 0 {
                    "3 days ago".into()
                } else {
                    String::new()
                },
                bar: Color::from_rgb8(0xf6, 0x6a, 0x0a),
                group_start: line == 0,
                on_click: Some(Rc::new(move || *hit.borrow_mut() = Some(line))),
            }
        })
        .collect();
    col.set(Some(GutterAnnotations {
        width: 200.0,
        items,
    }));
    h.frames(3);
    let ls = labels(&h);
    assert!(
        ls.iter().any(|l| l == "Blame line 1 Ada 3 days ago"),
        "{ls:?}"
    );
    assert!(ls.iter().any(|l| l == "Blame line 2"));
    let at = center(&h, |l| l == "Blame line 2");
    // The column is left of the line numbers.
    assert!(at.x < 200.0);
    h.click(at);
    h.frames(2);
    assert_eq!(*hit.borrow(), Some(1));
    col.set(None);
    h.frames(3);
    assert!(!labels(&h).iter().any(|l| l.starts_with("Blame line")));
}

#[test]
fn host_context_menu_items_merge_with_the_editors() {
    let code = create_root_signal(String::from("one\ntwo\nthree\n"));
    let got = Rc::new(RefCell::new(None));
    let g = got.clone();
    let mut h = Harness::new().window(800.0, 400.0);
    let editor = code_editor(code)
        .height(400.0)
        .context_menu_items(move |_ctx| {
            let g = g.clone();
            vec![
                EditorMenuItem::new("1_git", "View Line History", move |c| {
                    *g.borrow_mut() = Some(c.caret)
                }),
                EditorMenuItem::new("9_cutcopypaste", "Copy Relative Path", |_| {}).order(10),
            ]
        });
    // The menu opens in the overlay layer.
    h.mount_widget(View::new(
        Color::from_rgb8(255, 255, 255),
        pebbles::widgets::OverlayHost::wrap(editor.into_widget()),
    ));
    h.frames(3);
    let at = Offset::new(300.0, 30.0);
    h.click(at);
    assert!(h.ui.dispatch_secondary_tap_down(at));
    h.ui.dispatch_secondary_tap_up(at);
    h.ui.dispatch_secondary_tap(at);
    h.frames(3);
    for item in [
        "View Line History",
        "Cut",
        "Copy",
        "Paste",
        "Copy Relative Path",
        "Select All",
    ] {
        assert!(h.find_text(item), "{item} in the menu; {:?}", labels(&h));
    }
    let at = center(&h, |l| l == "View Line History");
    h.click(at);
    h.frames(2);
    assert!(got.borrow().is_some(), "the host item ran");
}
