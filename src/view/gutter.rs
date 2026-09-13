//! The line-number gutter. Virtualized to match the text: only the visible line numbers are
//! built, each absolutely positioned at its row inside a full-height column so the scroll
//! range and vertical alignment stay exact. The gutter stays fixed while the content scrolls
//! horizontally, so it wraps `content` in a row to its right.

use std::collections::HashMap;

use pebbles::prelude::*;

use crate::MONO;
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
    let gutter_w = digits as f64 * f.advance + 22.0;
    // The most-severe diagnostic per line (drives the gutter marker dot).
    let mut marks: HashMap<usize, Color> = HashMap::new();
    if let Some(sig) = f.p.diagnostics {
        for d in sig.get() {
            let line = line_of(f.src, d.range.0);
            marks.insert(line, severity_color(f.theme, d.severity));
        }
    }
    let mut nums: Vec<AnyWidget> = Vec::new();
    for n in f.first_line..=f.last_line {
        if let Some(&color) = marks.get(&n) {
            nums.push(
                Positioned::new(
                    container()
                        .width(4.0)
                        .height(f.line_px * 0.6)
                        .decoration(BoxDecoration::new().color(color).radius(BorderRadius::all(2.0))),
                )
                .left(2.0)
                .top(f.pad_t + n as f64 * f.line_px + f.line_px * 0.2)
                .into_widget(),
            );
        }
        let active = n == f.cl;
        nums.push(
            Positioned::new(
                container()
                    .width(gutter_w - 8.0)
                    .height(f.line_px)
                    .alignment(Alignment::CENTER_RIGHT)
                    .child(
                        text((n + 1).to_string())
                            .size(f.fs as f32)
                            .line_height(f.lh as f32)
                            .font_family(MONO)
                            .color(if active {
                                f.theme.gutter_active_fg
                            } else {
                                f.theme.gutter_fg
                            }),
                    ),
            )
            .left(0.0)
            .top(f.pad_t + n as f64 * f.line_px)
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
