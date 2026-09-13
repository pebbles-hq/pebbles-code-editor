//! The minimap: a scaled overview of the WHOLE document drawn in a single canvas node (so it
//! stays cheap on huge files) — one faint bar per line (indent-offset, length-scaled) with a
//! translucent viewport indicator. Click/drag scrolls the editor. Bounded viewport only.

use pebbles::prelude::*;
use pebbles::render::ScrollHandle;

use crate::view::Frame;
use crate::view::chrome::with_alpha;

const MM_W: f64 = 84.0;

/// Build the minimap panel, or `None` when the minimap is off or the editor is unbounded.
pub(crate) fn panel(f: &Frame, scroll: ScrollHandle, scroll_top: Signal<f64>) -> Option<AnyWidget> {
    let h = match (f.p.minimap, f.p.height) {
        (true, Some(h)) => h,
        _ => return None,
    };
    // Per-line (indent, length) in columns — the shape the canvas draws.
    let metrics: Vec<(f32, f32)> = f
        .src
        .split('\n')
        .map(|l| {
            let indent = l.chars().take_while(|c| *c == ' ' || *c == '\t').count() as f32;
            (indent, l.chars().count() as f32)
        })
        .collect();
    let cur_top = scroll_top.get();
    let ch = f.content_h;
    let ink = with_alpha(f.theme.foreground, 0.45);
    let vp = with_alpha(f.theme.gutter_active_fg, 0.16);
    let painter = move |cv: &mut Canvas<'_>| {
        let size = cv.size();
        let (mm_w, mm_h) = (size.width, size.height);
        let n = metrics.len().max(1);
        let rows = (mm_h.floor() as usize).clamp(1, n);
        let char_w = (mm_w - 6.0) / 90.0; // ~90 columns across
        for row in 0..rows {
            let li = row * n / rows;
            let (indent, len) = metrics.get(li).copied().unwrap_or((0.0, 0.0));
            if len <= 0.0 {
                continue;
            }
            let y = row as f64 * mm_h / rows as f64;
            let x0 = 3.0 + indent as f64 * char_w;
            let x1 = (x0 + (len - indent).max(0.0) as f64 * char_w).min(mm_w - 3.0);
            if x1 > x0 {
                cv.fill_rect(Rect::new(x0, y, x1, y + 1.5), ink);
            }
        }
        // viewport indicator
        let vy = (cur_top / ch) * mm_h;
        let vh = (h / ch) * mm_h;
        cv.fill_rect(Rect::new(0.0, vy, mm_w, (vy + vh).min(mm_h)), vp);
    };
    // Click/drag on the minimap centers the viewport on that fraction of the doc, nudging the
    // offset signal so the virtualized window follows now (the scroll view's `on_scroll` then
    // confirms the exact clamped offset).
    let jump = move |scroll_mm: &ScrollHandle, pos: Offset| {
        let frac = (pos.y / h).clamp(0.0, 1.0);
        let t = (frac * ch - h / 2.0).max(0.0);
        scroll_mm.scroll_to(t);
        scroll_top.set(t.min((ch - h).max(0.0)));
    };
    let (down_scroll, pan_scroll) = (scroll.clone(), scroll);
    Some(
        GestureDetector::new(
            container()
                .width(MM_W)
                .height(h)
                .decoration(BoxDecoration::new().color(f.theme.gutter_bg))
                .child(canvas(painter).width(MM_W).height(h)),
        )
        .on_pointer_down(action_event(move |e| jump(&down_scroll, e.position)))
        .on_pan_update(action_event(move |e| jump(&pan_scroll, e.position)))
        .into_widget(),
    )
}
