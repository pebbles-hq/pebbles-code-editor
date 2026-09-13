//! The line-number gutter. Virtualized to match the text: only the visible display rows are
//! built, each absolutely positioned at its row (`f.y_of`) so folding collapses rows and the
//! scroll range stays exact. Foldable lines get a clickable ▾/▸ fold arrow. The gutter stays
//! fixed while the content scrolls horizontally, so it wraps `content` in a row to its right.

use std::collections::{HashMap, HashSet};

use pebbles::prelude::*;

use crate::geometry::line_of;
use crate::view::Frame;
use crate::view::overlays::severity_color;

/// Wrap `content` with the line-number gutter on its left (or return it unchanged when the
/// gutter is disabled).
pub(crate) fn wrap(f: &Frame, content: AnyWidget) -> AnyWidget {
    if !f.p.gutter {
        return content;
    }
    let digits = f.line_count.to_string().len().max(2);
    let arrow_w = 14.0;
    let gutter_w = digits as f64 * f.advance + 22.0 + arrow_w;

    // The most-severe diagnostic / extension marker per line (drives the gutter dot).
    let mut marks: HashMap<usize, Color> = HashMap::new();
    if let Some(sig) = f.p.diagnostics {
        for d in sig.get() {
            marks.insert(line_of(f.src, d.range.0), severity_color(f.theme, d.severity));
        }
    }
    for m in f.ext_gutter_marks {
        marks.insert(m.line, m.color);
    }
    // Foldable head lines + which are currently folded.
    let heads: HashSet<usize> = f.fold_regions.iter().map(|&(h, _)| h).collect();
    let folded = f.folds.get();

    let mut nums: Vec<AnyWidget> = Vec::new();
    for n in f.first_line..=f.last_line {
        if f.disp.is_hidden(n) {
            continue;
        }
        let y = f.y_of(n);
        if let Some(&color) = marks.get(&n) {
            nums.push(
                Positioned::new(
                    container()
                        .width(4.0)
                        .height(f.line_px * 0.6)
                        .decoration(BoxDecoration::new().color(color).radius(BorderRadius::all(2.0))),
                )
                .left(2.0)
                .top(y + f.line_px * 0.2)
                .into_widget(),
            );
        }
        // Fold arrow for foldable head lines (▾ open, ▸ folded), toggling this line's fold.
        if heads.contains(&n) {
            let is_folded = folded.contains(&n);
            let folds = f.folds;
            let arrow = GestureDetector::new(
                container()
                    .width(arrow_w)
                    .height(f.line_px)
                    .alignment(Alignment::CENTER)
                    .child(
                        icon(if is_folded { IconKind::ChevronRight } else { IconKind::ChevronDown })
                            .size((f.fs * 0.95).max(12.0))
                            .color(f.theme.gutter_fg),
                    ),
            )
            .cursor(Cursor::Pointer)
            .on_tap(action(move || {
                folds.update(|s| {
                    if !s.remove(&n) {
                        s.insert(n);
                    }
                });
            }));
            nums.push(
                Positioned::new(arrow).left(gutter_w - arrow_w).top(y).into_widget(),
            );
        }
        let active = n == f.cl;
        nums.push(
            Positioned::new(
                container()
                    .width(gutter_w - arrow_w - 6.0)
                    .height(f.line_px)
                    .alignment(Alignment::CENTER_RIGHT)
                    .child(
                        text((n + 1).to_string())
                            .size(f.fs as f32)
                            .line_height(f.lh as f32)
                            .font_family(f.font_family)
                            .color(if active {
                                f.theme.gutter_active_fg
                            } else {
                                f.theme.gutter_fg
                            }),
                    ),
            )
            .left(0.0)
            .top(y)
            .into_widget(),
        );
    }
    let gutter_col = container()
        .width(gutter_w)
        .height(f.content_h)
        .decoration(BoxDecoration::new().color(f.theme.gutter_bg))
        .child(stack(nums).alignment(Alignment::TOP_LEFT))
        .into_widget();
    row(children![gutter_col, expanded(content)])
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .into_widget()
}
