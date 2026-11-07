//! The diff editor's word wrap (VS Code's "Toggle Word Wrap" in a diff): side by side, a
//! long changed line wraps on its side and filler rows on the other keep the following
//! lines aligned. Checked on an offscreen GPU capture (skips without an adapter; lavapipe
//! works): a marker line painted magenta on both sides sits on the same row.

use pebbles::core::create_root_signal;
use pebbles::prelude::*;
use pebbles::shell::capture::OffscreenCapture;
use pebbles_code_editor::{Decoration, EditorTheme, diff_editor, extension};
use pebbles_testing::Harness;

const W: u32 = 900;
const H: u32 = 400;
const MAGENTA: Color = Color::from_rgb8(255, 0, 255);

/// The (top, bottom) rows of magenta pixels in columns `x0..x1`.
fn band(rgba: &[u8], x0: u32, x1: u32) -> Option<(u32, u32)> {
    let mut out: Option<(u32, u32)> = None;
    for y in 0..H {
        for x in x0..x1 {
            let i = ((y * W + x) * 4) as usize;
            if rgba[i] > 230 && rgba[i + 1] < 30 && rgba[i + 2] > 230 {
                out = Some(out.map_or((y, y), |(a, _)| (a, y)));
                break;
            }
        }
    }
    out
}

fn marker() -> pebbles_code_editor::Extension {
    extension("marker").decorations(|snap| {
        snap.text
            .find("MARK")
            .map(|a| vec![Decoration::line((a, a + 4), MAGENTA)])
            .unwrap_or_default()
    })
}

fn render(wrap: bool, gpu: &mut OffscreenCapture) -> (Option<(u32, u32)>, Option<(u32, u32)>) {
    let theme = EditorTheme::dark();
    let bg = theme.background;
    let base = create_root_signal(String::from("first\nshort\nMARK\nlast\n"));
    let long = "word ".repeat(60);
    let modified = create_root_signal(format!("first\n{long}\nMARK\nlast\n"));
    let mut h = Harness::new()
        .window(f64::from(W), f64::from(H))
        .background(bg);
    let t = theme.clone();
    h.mount_widget(
        container().width(f64::from(W)).height(f64::from(H)).child(
            diff_editor(base, modified)
                .height(f64::from(H))
                .word_wrap(wrap)
                .configure(move |e| e.theme(t.clone()).extension(marker())),
        ),
    );
    h.frames(4);
    let scene = h.draw();
    let rgba = gpu.render_scene(&scene, W, H, bg).expect("render");
    (band(&rgba, 0, W / 2 - 10), band(&rgba, W / 2 + 10, W))
}

#[test]
fn wrapped_lines_stay_aligned_side_by_side() {
    let Ok(mut gpu) = OffscreenCapture::new() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let (l0, r0) = render(false, &mut gpu);
    let (l0, r0) = (l0.expect("left marker"), r0.expect("right marker"));
    assert_eq!(l0, r0, "aligned without wrap");
    let (l1, r1) = render(true, &mut gpu);
    let (l1, r1) = (l1.expect("left marker"), r1.expect("right marker"));
    let row = l0.1 - l0.0 + 1;
    assert!(
        r1.0 >= r0.0 + 2 * row,
        "the long line wraps onto more rows: {r1:?} vs {r0:?}"
    );
    assert!(
        l1.0.abs_diff(r1.0) <= 1,
        "filler rows keep the marker aligned: {l1:?} {r1:?}"
    );
}
