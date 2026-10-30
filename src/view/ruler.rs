//! The overview ruler (VS Code `editorOverviewRuler`): a narrow strip at the right edge of
//! the viewport mapping the WHOLE document onto its height, with marks for the cursor,
//! selections, find matches, diagnostics and git changes in three lanes. Click to scroll.

use pebbles::prelude::*;
use pebbles::render::ScrollHandle;

use crate::view::Frame;

const W: f64 = 14.0;

/// A mark: document line, colour, lane (0 left = changes, 1 centre = selections / matches,
/// 2 right = diagnostics, 3 = full width cursor).
pub(crate) type Mark = (usize, Color, u8);

/// Build the ruler for `marks`, or `None` when disabled / unbounded.
pub(crate) fn panel(
    f: &Frame,
    marks: Vec<Mark>,
    scroll: ScrollHandle,
    scroll_top: Signal<f64>,
) -> Option<AnyWidget> {
    let h = match (f.p.overview_ruler, f.p.height) {
        (true, Some(h)) => h,
        _ => return None,
    };
    let n = f.line_count.max(1) as f64;
    let ch = f.content_h;
    let vp = f.theme.scrollbar;
    let cur_top = scroll_top.get();
    let painter = move |cv: &mut Canvas<'_>| {
        let size = cv.size();
        let (w, hh) = (size.width, size.height);
        // Viewport band.
        let vy = (cur_top / ch) * hh;
        let vh = ((h / ch) * hh).max(4.0);
        cv.fill_rect(Rect::new(0.0, vy, w, (vy + vh).min(hh)), vp);
        let lane_w = w / 3.0;
        for &(line, color, lane) in &marks {
            let y = (line as f64 / n) * hh;
            let (x0, x1, th) = match lane {
                0 => (0.0, lane_w, 3.0),
                1 => (lane_w, 2.0 * lane_w, 3.0),
                2 => (2.0 * lane_w, w, 3.0),
                _ => (0.0, w, 2.0),
            };
            cv.fill_rect(Rect::new(x0, y, x1, (y + th).min(hh)), color);
        }
    };
    let jump = move |s: &ScrollHandle, pos: Offset| {
        let frac = (pos.y / h).clamp(0.0, 1.0);
        let t = (frac * ch - h / 2.0).max(0.0);
        s.scroll_to(t);
        scroll_top.set(t.min((ch - h).max(0.0)));
    };
    let (a, b) = (scroll.clone(), scroll);
    Some(
        GestureDetector::new(
            container()
                .width(W)
                .height(h)
                .decoration(BoxDecoration::new().color(f.theme.background))
                .child(canvas(painter).width(W).height(h)),
        )
        .on_pointer_down(action_event(move |e| jump(&a, e.position)))
        .on_pan_update(action_event(move |e| jump(&b, e.position)))
        .into_widget(),
    )
}

/// Collect the ruler marks for the frame.
pub(crate) fn marks(f: &Frame, search: &[(usize, usize)]) -> Vec<Mark> {
    let mut out: Vec<Mark> = Vec::new();
    if let Some(d) = f.diff {
        for h in &d.hunks {
            let c = match h.kind() {
                crate::diff::ChangeKind::Added => f.theme.change_added,
                crate::diff::ChangeKind::Modified => f.theme.change_modified,
                crate::diff::ChangeKind::Deleted => f.theme.change_deleted,
            };
            out.push((h.cur.start, c, 0));
        }
    }
    for &(s, _) in search.iter().take(5000) {
        out.push((f.li.line_of(f.src, s), f.theme.search_match_current, 1));
    }
    for r in f.sels.ranges().iter().filter(|r| !r.is_empty()) {
        out.push((f.li.line_of(f.src, r.min()), f.theme.selection, 1));
    }
    if let Some(sig) = f.p.diagnostics {
        for d in sig.get() {
            out.push((
                f.li.line_of(f.src, d.range.0),
                crate::view::overlays::severity_color(f.theme, d.severity),
                2,
            ));
        }
    }
    out.push((f.cl, f.theme.overview_cursor, 3));
    out
}
