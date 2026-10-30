//! The line-number gutter. Virtualized to match the text: only the visible display rows are
//! built, each absolutely positioned at its row (`f.y_of`) so folding collapses rows and the
//! scroll range stays exact. Foldable lines get a clickable ▾/▸ fold arrow. The gutter stays
//! fixed while the content scrolls horizontally, so it wraps `content` in a row to its right.

use std::collections::HashMap;

use pebbles::prelude::*;

use crate::view::Frame;

/// Wrap `content` with the line-number gutter on its left (or return it unchanged when the
/// gutter is disabled).
pub(crate) fn wrap(f: &Frame, content: AnyWidget) -> AnyWidget {
    if !f.p.gutter {
        return content;
    }
    let digits = f.line_count.to_string().len().max(2);
    let arrow_w = 14.0;
    // The breakpoint area (glyph margin), when the host binds breakpoints.
    let bp_w = glyph_margin_w(f.p, f.line_px);
    let gutter_w = digits as f64 * f.advance + 22.0 + arrow_w + bp_w;
    let breakpoints = f.p.breakpoints.map(|s| (s, s.get()));

    // The most-severe diagnostic / extension marker per line (drives the gutter dot).
    let mut marks: HashMap<usize, Color> = HashMap::new();
    {
        use crate::language::DiagnosticSeverity as S;
        let rank = |s: S| match s {
            S::Error => 0,
            S::Warning => 1,
            S::Information => 2,
            S::Hint => 3,
        };
        let mut best: HashMap<usize, S> = HashMap::new();
        for d in f.diags.iter() {
            let line = f.li.line_of(f.src, d.from.min(f.src.len()));
            let sev = d.d.severity();
            if best.get(&line).is_none_or(|b| rank(sev) < rank(*b)) {
                best.insert(line, sev);
            }
        }
        for (line, sev) in best {
            marks.insert(line, crate::view::lsp::severity_color(f.theme, sev));
        }
    }
    for m in f.ext_gutter_marks {
        marks.insert(m.line, m.color);
    }
    // Foldable head lines + which are currently folded.
    let folded = f.folds.get();

    let mut nums: Vec<AnyWidget> = Vec::new();
    for n in f.first_line..=f.last_line {
        if f.disp.is_hidden(n) {
            continue;
        }
        let y = f.y_of(n);
        // Git change bar at the far left (VS Code dirty diff): added / modified bars,
        // a deleted-lines marker; click to peek the change.
        if let Some(d) = f.diff
            && let Some(kind) = d.kind_at(n)
        {
            let hunk = d
                .hunks
                .iter()
                .position(|h| h.cur.contains(&n) || (h.cur.is_empty() && h.cur.start == n))
                .unwrap_or(0);
            let (color, h, top) = match kind {
                crate::diff::ChangeKind::Added => (f.theme.change_added, f.line_px, y),
                crate::diff::ChangeKind::Modified => (f.theme.change_modified, f.line_px, y),
                crate::diff::ChangeKind::Deleted => (f.theme.change_deleted, 4.0, y - 2.0),
            };
            let peek = f.change_peek;
            nums.push(
                Positioned::new(
                    GestureDetector::new(
                        container()
                            .width(3.0)
                            .height(h)
                            .decoration(BoxDecoration::new().color(color)),
                    )
                    .cursor(Cursor::Pointer)
                    .on_tap(action(move || {
                        peek.set(if peek.peek() == Some(hunk) {
                            None
                        } else {
                            Some(hunk)
                        });
                    })),
                )
                .left(0.0)
                .top(top.max(0.0))
                .into_widget(),
            );
        }
        if let Some((sig, set)) = &breakpoints {
            let sig = *sig;
            let on = set.contains(&n);
            let d = (f.line_px * 0.6).min(14.0);
            nums.push(
                Positioned::new(
                    GestureDetector::new(
                        container()
                            .width(bp_w)
                            .height(f.line_px)
                            .alignment(Alignment::CENTER)
                            .child(
                                container().width(d).height(d).decoration(
                                    BoxDecoration::new()
                                        .color(if on {
                                            f.theme.breakpoint
                                        } else {
                                            Color::TRANSPARENT
                                        })
                                        .radius(BorderRadius::all(d / 2.0)),
                                ),
                            ),
                    )
                    .cursor(Cursor::Pointer)
                    .on_tap(action(move || {
                        sig.update(|s| {
                            if !s.remove(&n) {
                                s.insert(n);
                            }
                        });
                    })),
                )
                .left(4.0)
                .top(y)
                .into_widget(),
            );
        }
        if let Some(&color) = marks.get(&n) {
            nums.push(
                Positioned::new(
                    container().width(4.0).height(f.line_px * 0.6).decoration(
                        BoxDecoration::new()
                            .color(color)
                            .radius(BorderRadius::all(2.0)),
                    ),
                )
                .left(5.0 + bp_w)
                .top(y + f.line_px * 0.2)
                .into_widget(),
            );
        }
        // Fold arrow for foldable head lines (▾ open, ▸ folded), toggling this line's fold.
        if f.li.is_fold_head(n) {
            let is_folded = folded.contains(&n);
            let folds = f.folds;
            let arrow = GestureDetector::new(
                container()
                    .width(arrow_w)
                    .height(f.line_px)
                    .alignment(Alignment::CENTER)
                    .child(
                        icon(if is_folded {
                            IconKind::ChevronRight
                        } else {
                            IconKind::ChevronDown
                        })
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
                Positioned::new(arrow)
                    .left(gutter_w - arrow_w)
                    .top(y)
                    .into_widget(),
            );
        }
        let active = n == f.cl;
        nums.push(
            Positioned::new(
                container()
                    .width(gutter_w - arrow_w - 6.0 - bp_w)
                    .height(f.line_px)
                    .alignment(Alignment::CENTER_RIGHT)
                    .child(
                        text(match f.p.line_numbers {
                            crate::config::LineNumbers::On => (n + 1).to_string(),
                            crate::config::LineNumbers::Relative if n == f.cl => {
                                (n + 1).to_string()
                            }
                            crate::config::LineNumbers::Relative => n.abs_diff(f.cl).to_string(),
                            crate::config::LineNumbers::Off => String::new(),
                        })
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
            .left(bp_w)
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

/// Width of the breakpoint area (0 when breakpoints are not bound).
pub(crate) fn glyph_margin_w(p: &crate::config::Props, line_px: f64) -> f64 {
    if p.breakpoints.is_some() {
        line_px.clamp(14.0, 20.0)
    } else {
        0.0
    }
}
