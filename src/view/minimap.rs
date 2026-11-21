//! The minimap (VS Code `editor.minimap`, `renderCharacters: false`, `showSlider:
//! mouseover`): a miniature of the code on the editor's background — every line is
//! `LINE_H` px tall and every character a `CHAR_W` px block in its token's colour (whitespace
//! left blank), so the shape of the code reads at a glance. Long files keep the scale and the
//! minimap scrolls with the editor (VS Code: the minimap's own top follows the scroll
//! fraction). Only the lines on screen are built, so huge files stay cheap. The viewport
//! slider is translucent and painted only while the pointer is over the minimap or dragging.
//! Click jumps (centres the viewport there); dragging the slider moves it proportionally.

use pebbles::prelude::*;
use pebbles::render::ScrollHandle;

use crate::lang::TokenKind;
use crate::view::Frame;
use crate::view::chrome::with_alpha;

const MM_W: f64 = 84.0;
/// One document line in the minimap (VS Code `minimap.scale: 1` renders 2 px per line).
pub(crate) const LINE_H: f64 = 2.0;
/// One character column.
pub(crate) const CHAR_W: f64 = 1.0;
/// Left inset of column 0.
pub(crate) const PAD_L: f64 = 2.0;
/// Block opacity over the editor background (VS Code draws blocks dimmed, not full ink).
const INK_ALPHA: f32 = 0.7;

/// State the minimap keeps across renders.
#[derive(Clone, Copy)]
pub(crate) struct MinimapState {
    /// Pointer over the minimap (the slider shows).
    pub(crate) hover: Signal<bool>,
    /// While dragging the slider: (pointer y, scroll top) at the press.
    pub(crate) grab: Signal<Option<(f64, f64)>>,
}

/// The minimap's geometry for a frame: where its own top is in minimap px, and the slider.
#[derive(Clone, Copy, Debug)]
struct Geom {
    /// Minimap scroll (px of minimap content above its top edge).
    top: f64,
    slider_y: f64,
    slider_h: f64,
    /// Scroll px per slider px while dragging.
    drag_ratio: f64,
}

fn geom(lines: usize, mm_h: f64, view_h: f64, extent: f64, line_px: f64, scroll: f64) -> Geom {
    let total = lines as f64 * LINE_H;
    let max_scroll = (extent - view_h).max(0.0);
    let frac = if max_scroll > 0.0 {
        (scroll / max_scroll).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let top = if total > mm_h {
        frac * (total - mm_h)
    } else {
        0.0
    };
    let per = LINE_H / line_px.max(1.0);
    let slider_h = (view_h * per).max(LINE_H);
    let slider_y = scroll * per - top;
    let room = (total.min(mm_h) - slider_h).max(1.0);
    let drag_ratio = if max_scroll > 0.0 {
        max_scroll / room
    } else {
        0.0
    };
    Geom {
        top,
        slider_y,
        slider_h,
        drag_ratio,
    }
}

thread_local! {
    /// Minimap blocks per line `(x0, x1, colour)` for one (text, theme, columns), with
    /// whether tokens covered the line when they were computed.
    #[allow(clippy::type_complexity)]
    static MM_LINES: std::cell::RefCell<(
        (usize, usize, usize),
        std::collections::HashMap<usize, (bool, Vec<(f64, f64, Color)>)>,
    )> = std::cell::RefCell::new(((0, 0, 0), std::collections::HashMap::new()));
}

/// One line's same-coloured runs of non-whitespace `(x0, x1, colour)`.
fn line_blocks(f: &Frame, text: &str, start: usize, max_cols: usize, fg: Color) -> Vec<(f64, f64, Color)> {
    let tokens = f.tokens;
    let mut ti = tokens.partition_point(|t| t.start + t.len <= start);
    let mut out = Vec::new();
    let mut run: Option<(usize, Color)> = None;
    let mut col = 0usize;
    let mut byte = start;
    let flush = |run: &mut Option<(usize, Color)>, end: usize, out: &mut Vec<(f64, f64, Color)>| {
        if let Some((c0, color)) = run.take() {
            out.push((PAD_L + c0 as f64 * CHAR_W, PAD_L + end as f64 * CHAR_W, with_alpha(color, INK_ALPHA)));
        }
    };
    let ts = f.li.tab_size().max(1);
    for chr in text.chars() {
        if col >= max_cols {
            break;
        }
        let width = if chr == '\t' { ts - col % ts } else { 1 };
        if chr.is_whitespace() {
            flush(&mut run, col, &mut out);
        } else {
            while ti < tokens.len() && tokens[ti].start + tokens[ti].len <= byte {
                ti += 1;
            }
            let (kind, scope) = match tokens.get(ti) {
                Some(t) if t.start <= byte => (t.kind, t.scope),
                _ => (TokenKind::Plain, 0),
            };
            let color = if kind == TokenKind::Plain && scope == 0 { fg } else { f.theme.style_token(kind, scope).color };
            match run {
                Some((_, c)) if c == color => {}
                _ => {
                    flush(&mut run, col, &mut out);
                    run = Some((col, color));
                }
            }
        }
        col += width;
        byte += chr.len_utf8();
    }
    flush(&mut run, col.min(max_cols), &mut out);
    out
}

/// Build the minimap panel, or `None` when the minimap is off or the editor is unbounded.
pub(crate) fn panel(
    f: &Frame,
    scroll: ScrollHandle,
    scroll_top: Signal<f64>,
    extent: f64,
    st: MinimapState,
) -> Option<AnyWidget> {
    let h = match (f.p.minimap, f.p.height) {
        (true, Some(h)) => h,
        _ => return None,
    };
    let line_px = f.line_px;
    let n = f.li.line_count().max(1);
    let cur_top = scroll_top.get();
    // The slider maps the same scroll extent as the scrollbar (text + scroll-beyond room).
    let ch = extent.max(h);
    let g = geom(n, h, h, ch, line_px, cur_top);

    // The blocks of the lines on screen: (x0, y, x1, colour), one per same-coloured run of
    // non-whitespace characters.
    let max_cols = ((MM_W - PAD_L) / CHAR_W).floor() as usize;
    let first = (g.top / LINE_H).floor() as usize;
    let last = (((g.top + h) / LINE_H).ceil() as usize).min(n);
    let fg = f.theme.foreground;
    let tokens = f.tokens;
    // The blocks depend on the first line, the text and its tokens — not on the pixel
    // offset, which changes every scroll frame: cache them (one char-by-char pass over the
    // ~350 lines on screen cost ~2 ms per frame) and only translate.
    // Per-line block cache: a line's blocks depend on its text and its tokens, not on the
    // scroll offset — scrolling recomputes only lines it hasn't seen (one char-by-char pass
    // over the ~350 lines on screen cost ~2 ms per window move). Cleared by a new text,
    // theme or width; a line first seen without tokens is redone once they cover it.
    let text_id = f.src.as_ptr() as usize ^ f.src.len();
    let theme_id = std::ptr::from_ref(f.theme) as usize;
    let (tok_lo, tok_hi) = match (tokens.first(), tokens.last()) {
        (Some(a), Some(b)) => (a.start, b.start + b.len),
        _ => (usize::MAX, 0),
    };
    let blocks: Vec<(f64, f64, f64, Color)> = MM_LINES.with(|c| {
        let mut c = c.borrow_mut();
        if c.0 != (text_id, theme_id, max_cols) {
            *c = ((text_id, theme_id, max_cols), std::collections::HashMap::new());
        }
        let mut out = Vec::new();
        for line in first..last {
            let start = f.li.line_start_of(line);
            let text = f.src[start..].split('\n').next().unwrap_or("");
            let covered = start >= tok_lo && start + text.len() <= tok_hi;
            let fresh = match c.1.get(&line) {
                Some((had_tokens, _)) => *had_tokens || !covered,
                None => false,
            };
            if !fresh {
                c.1.insert(line, (covered, line_blocks(f, text, start, max_cols, fg)));
            }
            let y = (line - first) as f64 * LINE_H;
            out.extend(c.1[&line].1.iter().map(|&(x0, x1, col)| (x0, y, x1, col)));
        }
        out
    });
    let dy = first as f64 * LINE_H - g.top;

    let show_slider = st.hover.get() || st.grab.get().is_some();
    let slider = if st.grab.get().is_some() {
        with_alpha(f.theme.scrollbar_active, 0.5)
    } else {
        with_alpha(f.theme.scrollbar, 0.5)
    };
    let painter = move |cv: &mut Canvas<'_>| {
        for &(x0, y, x1, c) in blocks.iter() {
            let y = y + dy;
            cv.fill_rect(Rect::new(x0, y, x1, y + LINE_H), c);
        }
        if show_slider {
            let w = cv.size().width;
            cv.fill_rect(
                Rect::new(0.0, g.slider_y, w, (g.slider_y + g.slider_h).min(h)),
                slider,
            );
        }
    };
    let max_scroll = (ch - h).max(0.0);
    let set = std::rc::Rc::new(move |s: &ScrollHandle, t: f64| {
        let t = t.clamp(0.0, max_scroll);
        s.scroll_to(t);
        scroll_top.set(t);
    });
    // Press: on the slider it grabs it; elsewhere it centres the viewport on that line.
    let (set_d, set_p) = (set.clone(), set);
    let down = move |s: &ScrollHandle, y: f64| {
        let top = scroll_top.peek();
        let g = geom(n, h, h, ch, line_px, top);
        if y >= g.slider_y && y <= g.slider_y + g.slider_h {
            st.grab.set(Some((y, top)));
        } else {
            let line = (y + g.top) / LINE_H;
            let t = line * line_px - h / 2.0;
            set_d(s, t);
            st.grab.set(Some((y, t.clamp(0.0, max_scroll))));
        }
    };
    let drag = move |s: &ScrollHandle, y: f64| {
        if let Some((y0, t0)) = st.grab.peek() {
            set_p(s, t0 + (y - y0) * g.drag_ratio);
        }
    };
    let (down_scroll, pan_scroll) = (scroll.clone(), scroll);
    Some(
        GestureDetector::new(
            container()
                .width(MM_W)
                .height(h)
                .decoration(BoxDecoration::new().color(f.theme.background))
                .child(canvas(painter).width(MM_W).height(h)),
        )
        .on_pointer_down(action_event(move |e| down(&down_scroll, e.position.y)))
        .on_pan_update(action_event(move |e| drag(&pan_scroll, e.position.y)))
        .on_pan_end(action_event(move |_| st.grab.set(None)))
        .on_pointer_up(action_event(move |_| st.grab.set(None)))
        .on_hover_move(action_event(move |_| {
            if !st.hover.peek() {
                st.hover.set(true);
            }
        }))
        .on_hover_exit(action(move || st.hover.set(false)))
        .into_widget(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_doc_keeps_scale_and_slider_tracks_scroll() {
        // 15 lines at 2 px: the minimap does not scroll; the slider is the viewport scaled.
        let g = geom(15, 600.0, 600.0, 600.0, 20.0, 0.0);
        assert_eq!(g.top, 0.0);
        assert_eq!(g.slider_h, 60.0);
        assert_eq!(g.slider_y, 0.0);
    }

    #[test]
    fn long_doc_scrolls_the_minimap_proportionally() {
        // 1000 lines = 2000 px of minimap in a 400 px panel; editor 20 px lines.
        let (lines, h, lp) = (1000, 400.0, 20.0);
        let extent = lines as f64 * lp;
        let end = geom(lines, h, h, extent, lp, extent - h);
        assert!((end.top - 1600.0).abs() < 1e-6, "{end:?}");
        assert!((end.slider_y + end.slider_h - h).abs() < 1e-6, "{end:?}");
        let mid = geom(lines, h, h, extent, lp, (extent - h) / 2.0);
        assert!((mid.top - 800.0).abs() < 1e-6);
        // Dragging the slider across its whole room covers the whole scroll range.
        assert!(((h - end.slider_h) * end.drag_ratio - (extent - h)).abs() < 1e-6);
    }
}
