//! The overview ruler (VS Code `editorOverviewRuler`): a narrow strip at the right edge of
//! the viewport mapping the WHOLE document onto its height, with marks for the cursor,
//! selections, find matches, diagnostics and git changes in three lanes. Drawn in the
//! vertical scrollbar's track ([`crate::view::scrollbar::vertical`]), as VS Code does.

use pebbles::prelude::*;

use crate::view::Frame;
use crate::view::scrollbar::TrackPainter;

/// A mark: document line, colour, lane (0 left = changes, 1 centre = selections / matches,
/// 2 right = diagnostics, 3 = full width cursor).
pub(crate) type Mark = (usize, Color, u8);

/// The ruler's marks as a painter for the vertical scrollbar's track (VS Code draws the
/// overview ruler inside the scrollbar, behind the thumb), or `None` when disabled.
pub(crate) fn track(f: &Frame, marks: Vec<Mark>, extent: f64) -> Option<TrackPainter> {
    let h = match (f.p.overview_ruler, f.p.height) {
        (true, Some(h)) => h,
        _ => return None,
    };
    let n = f.line_count.max(1) as f64;
    // Marks map the whole scroll extent (text + scroll-beyond room), like the thumb.
    let text_frac = (f.content_h / extent.max(h)).min(1.0);
    Some(Box::new(move |cv: &mut Canvas<'_>| {
        let size = cv.size();
        let (w, hh) = (size.width, size.height);
        let lane_w = w / 3.0;
        for &(line, color, lane) in &marks {
            let y = (line as f64 / n) * text_frac * hh;
            let (x0, x1, th) = match lane {
                0 => (0.0, lane_w, 3.0),
                1 => (lane_w, 2.0 * lane_w, 3.0),
                2 => (2.0 * lane_w, w, 3.0),
                _ => (0.0, w, 2.0),
            };
            cv.fill_rect(Rect::new(x0, y, x1, (y + th).min(hh)), color);
        }
    }))
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
    for d in f.diags.iter() {
        out.push((
            f.li.line_of(f.src, d.from.min(f.src.len())),
            crate::view::lsp::severity_color(f.theme, d.d.severity()),
            2,
        ));
    }
    out.push((f.cl, f.theme.overview_cursor, 3));
    out
}
