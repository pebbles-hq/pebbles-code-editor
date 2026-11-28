//! The minimap is a miniature of the code like VS Code's (`renderCharacters: false`,
//! `showSlider: mouseover`): every line 2 px tall, every character a 1 px block in its
//! token's colour on the editor background, and the viewport slider painted only while the
//! pointer is over the minimap. Checked on an offscreen GPU capture (skips without an
//! adapter; lavapipe works).
//!
//! Layout: an 800×400 window, a 400 px editor without the gutter; the minimap spans
//! x 702..786 (column 0 at x 704), the vertical bar x 786..800.

use pebbles::core::create_root_signal;
use pebbles::prelude::*;
use pebbles::shell::capture::OffscreenCapture;
use pebbles_code_editor::{EditorTheme, code_editor, lang};
use pebbles_testing::Harness;

const COL0: f64 = 704.0;
const W: u32 = 800;
const H: u32 = 400;

fn px(rgba: &[u8], x: f64, y: f64) -> [u8; 3] {
    let i = ((y as usize) * W as usize + x as usize) * 4;
    [rgba[i], rgba[i + 1], rgba[i + 2]]
}

fn shot(h: &mut Harness, gpu: &mut OffscreenCapture, bg: Color) -> Vec<u8> {
    h.frames(2);
    let scene = h.draw();
    gpu.render_scene(&scene, W, H, bg).expect("render")
}

#[test]
fn minimap_is_a_coloured_miniature_and_the_slider_shows_on_hover() {
    let Ok(mut gpu) = OffscreenCapture::new() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let theme = EditorTheme::dark();
    let bg = theme.background;
    let b = bg.to_rgba8();
    let bg3 = [b.r, b.g, b.b];
    // Line 0: a keyword then a plain name; line 1: 60 plain columns; line 2: empty;
    // line 3: 4 spaces of indent then text; then filler so the doc is longer than a screen.
    let mut src = String::from("pub hello\n");
    src.push_str(&"x".repeat(60));
    src.push_str("\n\n    yy\n");
    for i in 0..200 {
        src.push_str(&format!("let v{i} = {i};\n"));
    }
    let code = create_root_signal(src);
    let mut h = Harness::new()
        .window(f64::from(W), f64::from(H))
        .background(bg);
    h.mount_widget(
        code_editor(code)
            .language(lang::by_name("rust"))
            .theme(theme.clone())
            .height(f64::from(H))
            .gutter(false)
            .minimap(true),
    );
    h.frames(3);
    // Park the pointer over the text so nothing is hovered.
    h.hover(Offset::new(300.0, 300.0));
    let rgba = shot(&mut h, &mut gpu, bg);

    // Pixels follow line lengths: line 1 is inked at column 50, line 0 (9 columns) is not.
    let y = |line: f64| line * 2.0 + 1.0;
    assert_ne!(
        px(&rgba, COL0 + 50.5, y(1.0)),
        bg3,
        "60-column line inked at col 50"
    );
    assert_eq!(
        px(&rgba, COL0 + 50.5, y(0.0)),
        bg3,
        "9-column line blank at col 50"
    );
    // Whitespace stays blank (the indent of line 3, the space in line 0, the empty line 2).
    assert_eq!(px(&rgba, COL0 + 1.5, y(3.0)), bg3, "indent blank");
    assert_ne!(
        px(&rgba, COL0 + 4.5, y(3.0)),
        bg3,
        "text after the indent inked"
    );
    assert_eq!(px(&rgba, COL0 + 3.5, y(0.0)), bg3, "space blank");
    assert_eq!(px(&rgba, COL0 + 10.5, y(2.0)), bg3, "empty line blank");
    // Colours follow tokens: the keyword `pub` differs from the plain identifier `hello`.
    let kw = px(&rgba, COL0 + 1.5, y(0.0));
    let plain = px(&rgba, COL0 + 6.5, y(0.0));
    assert_ne!(kw, bg3);
    assert_ne!(kw, plain, "keyword block coloured unlike plain text");
    // The panel is the editor background, not a grey slab, and the slider is hidden.
    let empty = (COL0 + 70.5, 300.0);
    assert_eq!(px(&rgba, empty.0, 20.0), bg3, "slider hidden without hover");
    assert_eq!(px(&rgba, empty.0, empty.1), bg3);

    // Hovering the minimap shows the translucent slider over the viewport rows only.
    h.hover(Offset::new(COL0 + 40.0, 300.0));
    let rgba = shot(&mut h, &mut gpu, bg);
    let slid = px(&rgba, empty.0, 20.0);
    assert_ne!(slid, bg3, "slider painted on hover");
    assert_eq!(px(&rgba, empty.0, 300.0), bg3, "below the slider untouched");
    // Translucent: the code under it still shows through (line 1 at col 50 differs from
    // the bare slider colour).
    assert_ne!(px(&rgba, COL0 + 50.5, y(1.0)), slid);

    // Leaving hides it again.
    h.hover(Offset::new(300.0, 300.0));
    let rgba = shot(&mut h, &mut gpu, bg);
    assert_eq!(px(&rgba, empty.0, 20.0), bg3, "slider hidden after leaving");
}
