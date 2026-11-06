//! Host text decorations (VS Code's `TextEditorDecorationType` options): background,
//! border, text colour, injected before / after text (which takes columns), a gutter
//! icon, an overview-ruler mark and a hover message; clearing them removes everything.
//! Checked on an offscreen GPU capture (skips without an adapter; lavapipe works).

use pebbles::core::create_root_signal;
use pebbles::prelude::*;
use pebbles::shell::capture::OffscreenCapture;
use pebbles_code_editor::{Decoration, EditorTheme, code_editor, extension};
use pebbles_testing::Harness;

const W: u32 = 800;
const H: u32 = 300;

const RED: Color = Color::from_rgb8(200, 0, 0);
const GREEN: Color = Color::from_rgb8(0, 255, 0);
const BLUE: Color = Color::from_rgb8(40, 40, 255);
const YELLOW: Color = Color::from_rgb8(255, 255, 0);
const ORANGE: Color = Color::from_rgb8(255, 128, 0);
const MAGENTA: Color = Color::from_rgb8(255, 0, 255);
const CYAN: Color = Color::from_rgb8(0, 255, 255);

/// The bounding box (x0, y0, x1, y1) of pixels within 40 of `c` (per channel) inside
/// `area`, and how many there are.
fn find(rgba: &[u8], c: Color, area: (u32, u32, u32, u32)) -> (usize, (u32, u32, u32, u32)) {
    let t = c.to_rgba8();
    let mut n = 0;
    let mut bb = (u32::MAX, u32::MAX, 0, 0);
    for y in area.1..area.3.min(H) {
        for x in area.0..area.2.min(W) {
            let i = ((y * W + x) * 4) as usize;
            let d = |a: u8, b: u8| (a as i32 - b as i32).abs() <= 40;
            if d(rgba[i], t.r) && d(rgba[i + 1], t.g) && d(rgba[i + 2], t.b) {
                n += 1;
                bb = (bb.0.min(x), bb.1.min(y), bb.2.max(x), bb.3.max(y));
            }
        }
    }
    (n, bb)
}

fn shot(h: &mut Harness, gpu: &mut OffscreenCapture, bg: Color) -> Vec<u8> {
    h.frames(2);
    let scene = h.draw();
    gpu.render_scene(&scene, W, H, bg).expect("render")
}

#[test]
fn decorations_paint_inject_text_mark_the_gutter_and_ruler_and_hover() {
    let Ok(mut gpu) = OffscreenCapture::new() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let theme = EditorTheme::dark();
    let bg = theme.background;
    let code = create_root_signal(String::from("say hello world\nsecond line\n"));
    let decos = create_root_signal(Vec::<Decoration>::new());
    let mut h = Harness::new()
        .window(f64::from(W), f64::from(H))
        .background(bg);
    h.mount_widget(
        code_editor(code)
            .theme(theme)
            .height(f64::from(H))
            .overview_ruler(true)
            .hover_delay(0.1)
            .extension(extension("decos").decorations(move |_| decos.get())),
    );
    h.frames(3);
    h.hover(Offset::new(400.0, 250.0));
    let all = (0, 0, W, H);
    let plain = shot(&mut h, &mut gpu, bg);
    for c in [RED, GREEN, YELLOW, ORANGE, MAGENTA, CYAN] {
        assert_eq!(find(&plain, c, all).0, 0, "{c:?} before decorating");
    }

    // "hello" (bytes 4..9) decorated with everything.
    decos.set(vec![
        Decoration::background((4, 9), RED)
            .border(GREEN)
            .color(BLUE)
            .before("<<", Some(YELLOW))
            .after(">>", Some(ORANGE))
            .overview_ruler(CYAN)
            .gutter_icon(|side| {
                container()
                    .width(side)
                    .height(side)
                    .color(MAGENTA)
                    .into_widget()
            })
            .hover("**Deco hover** message"),
    ]);
    let rgba = shot(&mut h, &mut gpu, bg);
    let (nr, red) = find(&rgba, RED, all);
    assert!(nr > 50, "background painted ({nr})");
    let (ng, green) = find(&rgba, GREEN, all);
    assert!(ng > 20, "border painted ({ng})");
    assert!(
        green.0 <= red.0 && green.2 >= red.2,
        "the border surrounds the background: {green:?} {red:?}"
    );
    let (ny, yellow) = find(&rgba, YELLOW, all);
    let (no, orange) = find(&rgba, ORANGE, all);
    assert!(ny > 5 && no > 5, "injected texts drawn ({ny}, {no})");
    assert!(
        yellow.2 < red.0 + 2 && orange.0 + 2 > red.2,
        "before text left of the range, after text right of it: {yellow:?} {red:?} {orange:?}"
    );
    assert!(
        yellow.1 >= red.1.saturating_sub(4) && yellow.3 <= red.3 + 4,
        "on the same row"
    );
    // The text of the range is drawn in the decoration's colour.
    let (nb, _) = find(&rgba, BLUE, red);
    assert!(nb > 5, "range text coloured ({nb})");
    let (nm, icon) = find(&rgba, MAGENTA, all);
    assert!(nm > 40, "gutter icon drawn ({nm})");
    assert!(icon.2 < red.0, "the icon is in the gutter: {icon:?}");
    assert!(
        icon.1 <= red.3 && icon.3 >= red.1,
        "on the decorated line: {icon:?} {red:?}"
    );
    let (nc, ruler) = find(&rgba, CYAN, all);
    assert!(nc > 3, "overview ruler mark ({nc})");
    assert!(
        ruler.0 > W - 20,
        "in the ruler at the right edge: {ruler:?}"
    );

    // Hovering the range shows its hover message.
    let mid = Offset::new(
        f64::from(red.0 + red.2) / 2.0,
        f64::from(red.1 + red.3) / 2.0,
    );
    h.hover(mid);
    h.frames(1);
    h.advance(0.3);
    h.frames(3);
    assert!(h.find_text("Deco hover message"), "hover message shown");

    // Clearing removes everything (VS Code: setDecorations(type, []) / dispose).
    h.hover(Offset::new(400.0, 250.0));
    h.advance(0.5);
    decos.set(Vec::new());
    let rgba = shot(&mut h, &mut gpu, bg);
    for c in [RED, GREEN, YELLOW, ORANGE, MAGENTA, CYAN] {
        assert_eq!(find(&rgba, c, all).0, 0, "{c:?} after clearing");
    }
    assert_eq!(
        code.peek(),
        "say hello world\nsecond line\n",
        "text untouched"
    );
}
