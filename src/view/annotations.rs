//! Host annotations: end-of-line "after text" ([`LineAnnotation`], GitLens' trailing blame)
//! with its hover card, and the host gutter column ([`GutterAnnotations`], blame view).

use pebbles::prelude::*;

use crate::annotations::{GutterAnnotations, LineAnnotation};
use crate::view::Frame;
use crate::view::chrome::with_alpha;

/// Columns between a line's last character and its annotation (GitLens' trailing margin).
const AFTER_GAP_COLS: f64 = 3.0;

/// Timer id of the hover card's delayed close.
fn close_timer(hover: Signal<Option<usize>>) -> u64 {
    hover.raw_id() ^ 0x414e_4e4f_5400_0000
}

/// Keep the card of `line` open (cancel a pending close).
fn hold(hover: Signal<Option<usize>>, line: usize) {
    pebbles::core::animation::clear_timeout(close_timer(hover));
    if hover.peek() != Some(line) {
        hover.set(Some(line));
    }
}

/// Close the card shortly unless the pointer comes back (trigger ↔ card moves).
fn release(hover: Signal<Option<usize>>) {
    // The editor may have unmounted by then (its signals disposed).
    pebbles::core::animation::set_timeout(close_timer(hover), 0.3, move || {
        if hover.try_peek().flatten().is_some() {
            hover.set(None);
        }
    });
}

/// VS Code's `editor.inlineValuesBackground` (#ffc80033).
const INLINE_VALUE_BG: Color = Color::from_rgba8(0xff, 0xc8, 0x00, 0x33);

/// The after-text layer: each visible annotation at its line's end, and the hover card of
/// the hovered one (below the line, owning the pointer over its area).
pub(crate) fn after_text_layer(
    f: &Frame,
    anns: &[LineAnnotation],
    hover: Signal<Option<usize>>,
) -> AnyWidget {
    let mut kids: Vec<AnyWidget> = Vec::new();
    let open = hover.get();
    for a in anns {
        if a.line >= f.line_count || !f.line_visible(a.line) || a.text.is_empty() {
            continue;
        }
        let len = f.li.char_lens().get(a.line).copied().unwrap_or(0);
        let (x, y) = f.xy(a.line, len);
        let x = x + AFTER_GAP_COLS * f.advance;
        let line = a.line;
        let label = text(a.text.clone())
            .size(f.fs as f32)
            .line_height(f.lh as f32)
            .font_family(f.font_family)
            .color(if a.inline_value {
                // `editor.inlineValuesForeground`: #ffffff80 dark / #00000080 light.
                crate::view::chrome::with_alpha(f.theme.foreground, 0.5)
            } else {
                f.theme.trailing_annotation
            });
        let cell = || {
            let c = container().height(f.line_px);
            if a.inline_value {
                // `editor.inlineValuesBackground` (#ffc80033).
                c.padding(EdgeInsets::symmetric(2.0, 0.0))
                    .decoration(BoxDecoration::new().color(INLINE_VALUE_BG))
            } else {
                c
            }
        };
        let trigger: AnyWidget = if a.hover.is_some() {
            GestureDetector::new(cell().child(label))
                .on_hover_enter(action(move || hold(hover, line)))
                .on_hover_exit(action(move || release(hover)))
                .into_widget()
        } else {
            cell().child(label).into_widget()
        };
        kids.push(
            Positioned::new(semantics(
                SemanticsRole::Label,
                if a.inline_value {
                    format!("Inline value {}", a.text)
                } else {
                    format!("Annotation {}", a.text)
                },
                trigger,
            ))
            .left(x)
            .top(y)
            .into_widget(),
        );
        if open == Some(line)
            && let Some(h) = &a.hover
        {
            kids.push(
                Positioned::new(card(f, h, hover, line))
                    .left(x)
                    .top(y + f.line_px)
                    .into_widget(),
            );
        }
    }
    stack(kids)
        .fit(StackFit::Expand)
        .alignment(Alignment::TOP_LEFT)
        .into_widget()
}

/// The hover card: title, body lines, action buttons.
fn card(
    f: &Frame,
    h: &crate::annotations::AnnotationHover,
    hover: Signal<Option<usize>>,
    line: usize,
) -> AnyWidget {
    let ui = f.theme.ui_font_size;
    let mut rows: Vec<AnyWidget> = vec![
        semantics(
            SemanticsRole::Label,
            format!("Annotation hover {}", h.title),
            text(h.title.clone())
                .size(ui as f32)
                .bold()
                .color(f.theme.foreground),
        )
        .into_widget(),
    ];
    for b in &h.body {
        rows.push(
            semantics(
                SemanticsRole::Label,
                format!("Annotation hover line {b}"),
                text(b.clone()).size(ui as f32).color(if b.is_empty() {
                    f.theme.muted
                } else {
                    f.theme.foreground
                }),
            )
            .into_widget(),
        );
    }
    if !h.actions.is_empty() {
        let mut btns: Vec<AnyWidget> = Vec::new();
        for (i, a) in h.actions.iter().enumerate() {
            if i > 0 {
                btns.push(gap_w(6.0).into_widget());
            }
            let run = a.run.clone();
            btns.push(
                button(a.label.clone())
                    .size(ButtonSize::Sm)
                    .variant(ButtonVariant::Secondary)
                    .on_pressed(move || {
                        hover.set(None);
                        run();
                    })
                    .into_widget(),
            );
        }
        rows.push(gap_h(4.0).into_widget());
        rows.push(row(btns).main_axis_size(MainAxisSize::Min).into_widget());
    }
    let body = container()
        .padding(EdgeInsets::all(8.0))
        .decoration(
            BoxDecoration::new()
                .color(f.theme.overlay_bg)
                .border(Border::new(f.theme.border, 1.0))
                .radius(BorderRadius::all(f.theme.overlay_radius.min(6.0))),
        )
        .child(
            column(rows)
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .main_axis_size(MainAxisSize::Min),
        );
    let card = GestureDetector::new(body)
        .on_hover_enter(action(move || hold(hover, line)))
        .on_hover_exit(action(move || release(hover)));
    crate::view::popup_shield(f, card)
}

/// The host gutter column's cells over the frame's window: an age bar at the left edge, the
/// text, the right-aligned detail, and a hairline above each group's first line.
pub(crate) fn gutter_cells(f: &Frame, col: &GutterAnnotations) -> AnyWidget {
    let mut kids: Vec<AnyWidget> = Vec::new();
    let w = col.width;
    let small = (f.fs * 0.85).max(9.0) as f32;
    for c in &col.items {
        if c.line >= f.line_count || !f.line_visible(c.line) {
            continue;
        }
        let y = f.y_of(c.line);
        let cell = container()
            .width(w)
            .height(f.line_px)
            .padding(EdgeInsets::only(8.0, 0.0, 6.0, 0.0))
            .alignment(Alignment::CENTER_LEFT)
            .child(
                row(children![
                    expanded(
                        text(c.text.clone())
                            .size(small)
                            .font_family(f.font_family)
                            .color(f.theme.foreground)
                    ),
                    text(c.detail.clone())
                        .size(small)
                        .font_family(f.font_family)
                        .color(f.theme.muted),
                ])
                .cross_axis_alignment(CrossAxisAlignment::Center),
            );
        let cell: AnyWidget = match &c.on_click {
            Some(run) => {
                let run = run.clone();
                GestureDetector::new(cell)
                    .cursor(pebbles::widgets::action_cursor())
                    .on_tap(action(move || run()))
                    .into_widget()
            }
            None => cell.into_widget(),
        };
        let label = if c.text.is_empty() {
            format!("Blame line {}", c.line + 1)
        } else {
            format!("Blame line {} {} {}", c.line + 1, c.text, c.detail)
        };
        kids.push(
            Positioned::new(semantics(SemanticsRole::Button, label, cell))
                .left(0.0)
                .top(y)
                .into_widget(),
        );
        kids.push(
            Positioned::new(
                container()
                    .width(3.0)
                    .height(f.line_px)
                    .decoration(BoxDecoration::new().color(c.bar)),
            )
            .left(0.0)
            .top(y)
            .into_widget(),
        );
        if c.group_start && c.line > 0 {
            kids.push(
                Positioned::new(
                    container()
                        .width(w)
                        .height(1.0)
                        .decoration(BoxDecoration::new().color(with_alpha(f.theme.border, 0.8))),
                )
                .left(0.0)
                .top(y)
                .into_widget(),
            );
        }
    }
    stack(kids).alignment(Alignment::TOP_LEFT).into_widget()
}
