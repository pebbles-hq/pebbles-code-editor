//! The line-number gutter. Virtualized to match the text: only the visible display rows are
//! built, each absolutely positioned at its row (`f.y_of`) so folding collapses rows and the
//! scroll range stays exact. Foldable lines get a clickable ▾/▸ fold arrow. The gutter stays
//! fixed while the content scrolls horizontally, so it wraps `content` in a row to its right.

use std::collections::HashMap;

use pebbles::prelude::*;

use crate::view::Frame;

/// Wrap `content` with the line-number gutter on its left (or return it unchanged when the
/// gutter is disabled).
pub(crate) fn wrap(
    f: &Frame,
    content: AnyWidget,
    numbers: AnyWidget,
    host: Option<(f64, AnyWidget)>,
) -> AnyWidget {
    if !f.p.gutter {
        return content;
    }
    let gutter_col = container()
        .width(width(f))
        .height(f.content_h)
        .decoration(BoxDecoration::new().color(f.theme.gutter_bg))
        .child(numbers)
        .into_widget();
    let mut kids = Vec::new();
    // The host column (blame annotations) sits left of the line numbers.
    if let Some((w, cells)) = host {
        kids.push(
            container()
                .width(w)
                .height(f.content_h)
                .decoration(BoxDecoration::new().color(f.theme.gutter_bg))
                .child(cells)
                .into_widget(),
        );
    }
    kids.push(gutter_col);
    kids.push(expanded(content).into_widget());
    row(kids)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .into_widget()
}

/// The gutter's per-line cells (numbers, fold arrows, change bars, breakpoints, markers,
/// the lightbulb) over the frame's window — the body of the gutter's scroll slice.
pub(crate) fn numbers(f: &Frame) -> AnyWidget {
    if !f.p.gutter {
        return container().into_widget();
    }
    let (gutter_w, arrow_w, bp_w) = (width(f), ARROW_W, glyph_margin_w(f.p, f.line_px));
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
                    .cursor(pebbles::widgets::action_cursor())
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
                    .cursor(pebbles::widgets::action_cursor())
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
            .cursor(pebbles::widgets::action_cursor())
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
        nums.push(
            Positioned::new(number_cell(f, n))
                .left(bp_w)
                .top(y)
                .into_widget(),
        );
    }
    // Diff view: the per-hunk buttons (VS Code's diff editor gutter menu) at each change's
    // first line.
    if let (Some(d), Some(run)) = (f.diff, f.hunk_run.as_ref()) {
        let acts = &f.p.diff_hunk_actions;
        let sz = (f.line_px * 0.75).clamp(11.0, 16.0);
        for (hi, h) in d.hunks.iter().enumerate() {
            let n = h.cur.start.min(f.line_count.saturating_sub(1));
            if n < f.first_line || n > f.last_line || !f.line_visible(n) {
                continue;
            }
            let btns: Vec<AnyWidget> = acts
                .iter()
                .enumerate()
                .map(|(ai, a)| {
                    let run = run.clone();
                    semantics(
                        SemanticsRole::Button,
                        a.label.clone(),
                        tooltip(
                            a.label.clone(),
                            GestureDetector::new(
                                container()
                                    .width(HUNK_BTN_W)
                                    .height(f.line_px)
                                    .alignment(Alignment::CENTER)
                                    .child(icon(a.icon).size(sz).color(f.theme.foreground)),
                            )
                            .cursor(pebbles::widgets::action_cursor())
                            .on_tap(action(move || run(hi, ai))),
                        ),
                    )
                    .into_widget()
                })
                .collect();
            nums.push(
                Positioned::new(row(btns))
                    .left(4.0)
                    .top(f.y_of(n))
                    .into_widget(),
            );
        }
    }
    // The code-action lightbulb on the caret line (click opens the menu); the quick-fix
    // variant (an action fixing a diagnostic here) uses the warning color.
    if let Some((line, actions)) = f.lsp.h.bulb.get()
        && line < f.line_count
        && f.line_visible(line)
    {
        let fix = actions
            .iter()
            .any(|a| a.kind.as_deref().is_some_and(|k| k.starts_with("quickfix")));
        let color = if fix {
            f.theme.diag_warning
        } else {
            f.theme.muted
        };
        let l = f.lsp.clone();
        let sz = (f.line_px * 0.8).min(16.0);
        nums.push(
            Positioned::new(semantics(
                SemanticsRole::Button,
                "Show Code Actions",
                GestureDetector::new(icon(tabler::BULB).size(sz).color(color))
                    .cursor(pebbles::widgets::action_cursor())
                    .on_tap(action(move || l.open_menu(actions.clone()))),
            ))
            .left(1.0)
            .top(f.y_of(line) + (f.line_px - sz) / 2.0)
            .into_widget(),
        );
    }
    stack(nums).alignment(Alignment::TOP_LEFT).into_widget()
}

/// The fold-arrow column at the gutter's right edge.
pub(crate) const ARROW_W: f64 = 14.0;

/// The gutter's full width: line numbers (at least 2 digits), the fold arrows and the
/// breakpoint area (glyph margin) when bound. Sticky-scroll rows reuse it so their numbers
/// line up with the real gutter's.
pub(crate) fn width(f: &Frame) -> f64 {
    let digits = f.line_count.to_string().len().max(2);
    digits as f64 * f.advance + 22.0 + ARROW_W + glyph_margin_w(f.p, f.line_px)
}

/// The line-number cell of `line` (right-aligned, the gutter's text style); placed at
/// `left = glyph_margin_w`. Shared by the gutter and the sticky-scroll rows.
pub(crate) fn number_cell(f: &Frame, line: usize) -> AnyWidget {
    let active = line == f.cl;
    container()
        .width(width(f) - ARROW_W - 6.0 - glyph_margin_w(f.p, f.line_px))
        .height(f.line_px)
        .alignment(Alignment::CENTER_RIGHT)
        .child(
            text(match f.p.line_numbers {
                crate::config::LineNumbers::On => (line + 1).to_string(),
                crate::config::LineNumbers::Relative if line == f.cl => (line + 1).to_string(),
                crate::config::LineNumbers::Relative => line.abs_diff(f.cl).to_string(),
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
        )
        .into_widget()
}

/// Width of the host gutter column (blame annotations; 0 when off). Untracked: the editor
/// re-renders when the column toggles.
pub(crate) fn host_w(f: &Frame) -> f64 {
    f.p.gutter_annotations
        .and_then(|s| s.peek().map(|c| c.width))
        .unwrap_or(0.0)
}

/// Width of one diff hunk button in the gutter.
const HUNK_BTN_W: f64 = 18.0;

/// Width of the glyph margin: the breakpoint area (when bound) or a diff view's hunk
/// buttons (0 when neither).
pub(crate) fn glyph_margin_w(p: &crate::config::Props, line_px: f64) -> f64 {
    let bp = if p.breakpoints.is_some() {
        line_px.clamp(14.0, 20.0)
    } else {
        0.0
    };
    let hunk = if p.diff_base.is_some() && !p.diff_hunk_actions.is_empty() {
        p.diff_hunk_actions.len() as f64 * HUNK_BTN_W + 4.0
    } else {
        0.0
    };
    bp.max(hunk)
}
