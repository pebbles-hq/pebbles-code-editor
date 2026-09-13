//! The line-number gutter. Virtualized to match the text: only the visible line numbers are
//! built, each absolutely positioned at its row inside a full-height column so the scroll
//! range and vertical alignment stay exact. The gutter stays fixed while the content scrolls
//! horizontally, so it wraps `content` in a row to its right.

use pebbles::prelude::*;

use crate::MONO;
use crate::view::Frame;

/// Wrap `content` with the line-number gutter on its left (or return it unchanged when the
/// gutter is disabled).
pub(crate) fn wrap(f: &Frame, content: AnyWidget) -> AnyWidget {
    if !f.p.gutter {
        return content;
    }
    let digits = f.line_count.to_string().len().max(2);
    let gutter_w = digits as f64 * f.advance + 22.0;
    let mut nums: Vec<AnyWidget> = Vec::new();
    for n in f.first_line..=f.last_line {
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
