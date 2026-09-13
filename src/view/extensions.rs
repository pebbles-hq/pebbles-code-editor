//! View support for [`crate::extensions`]: rendering extension **decorations** (background /
//! underline / line-background layers) and the **command palette** (a searchable overlay
//! that runs any registered [`Command`]).

use pebbles::prelude::*;

use crate::MONO;
use crate::extensions::{Command, Decoration, EditContext};
use crate::geometry::{col_of, line_char_len, line_of};
use crate::view::Frame;

/// Build overlay layers for `decos`, clipped to the visible window (line backgrounds first,
/// then backgrounds, then underlines).
pub(crate) fn decoration_layers(f: &Frame, decos: &[Decoration]) -> Vec<AnyWidget> {
    let mut layers = Vec::new();
    // Line backgrounds (behind everything).
    for d in decos {
        if let Some(bg) = d.style.line_background {
            let (la, lb) = (line_of(f.src, d.range.0), line_of(f.src, d.range.1));
            for line in la.max(f.first_line)..=lb.min(f.last_line) {
                layers.push(
                    Positioned::new(
                        container()
                            .height(f.line_px)
                            .decoration(BoxDecoration::new().color(bg)),
                    )
                    .left(0.0)
                    .right(0.0)
                    .top(f.pad_t + line as f64 * f.line_px)
                    .into_widget(),
                );
            }
        }
    }
    // Range backgrounds + underlines.
    for d in decos {
        for line in
            line_of(f.src, d.range.0).max(f.first_line)..=line_of(f.src, d.range.1).min(f.last_line)
        {
            let (la, lb) = (line_of(f.src, d.range.0), line_of(f.src, d.range.1));
            let start_col = if line == la {
                col_of(f.src, d.range.0)
            } else {
                0
            };
            let end_col = if line == lb {
                col_of(f.src, d.range.1)
            } else {
                line_char_len(f.src, line)
            };
            let x = f.pad_l + start_col as f64 * f.advance;
            let w = (end_col.saturating_sub(start_col).max(1) as f64 * f.advance).max(2.0);
            let y = f.pad_t + line as f64 * f.line_px;
            if let Some(bg) = d.style.background {
                layers.push(
                    Positioned::new(
                        container().width(w).height(f.line_px).decoration(
                            BoxDecoration::new()
                                .color(bg)
                                .radius(BorderRadius::all(2.0)),
                        ),
                    )
                    .left(x)
                    .top(y)
                    .into_widget(),
                );
            }
            if let Some(u) = d.style.underline {
                layers.push(
                    Positioned::new(
                        container()
                            .width(w)
                            .height(2.0)
                            .decoration(BoxDecoration::new().color(u)),
                    )
                    .left(x)
                    .top(y + f.line_px - 2.0)
                    .into_widget(),
                );
            }
        }
    }
    layers
}

// ---------------------------------------------------------------------------
// Command palette
// ---------------------------------------------------------------------------

/// The command-palette reactive state, owned by `view::render_editor`.
#[derive(Clone, Copy)]
pub(crate) struct Palette {
    pub(crate) open: Signal<bool>,
    pub(crate) query: Signal<String>,
    pub(crate) sel: Signal<usize>,
}

/// Commands whose title contains the (lowercased) query, in order.
fn filtered(commands: &[Command], query: &str) -> Vec<Command> {
    if query.is_empty() {
        return commands.to_vec();
    }
    let q = query.to_lowercase();
    commands
        .iter()
        .filter(|c| c.title().to_lowercase().contains(&q))
        .cloned()
        .collect()
}

/// Run the selected command (if any) and close the palette.
pub(crate) fn run_selected(
    pal: Palette,
    commands: &[Command],
    ctx: EditContext,
    focus: pebbles::core::focus::FocusNode,
) {
    let matches = filtered(commands, &pal.query.peek());
    if let Some(cmd) = matches.get(pal.sel.peek().min(matches.len().saturating_sub(1))) {
        (cmd.run)(&ctx);
    }
    pal.open.set(false);
    focus.request_focus();
}

/// The command-palette overlay: a centered, searchable list of commands.
pub(crate) fn palette(
    pal: Palette,
    commands: Vec<Command>,
    ctx: EditContext,
    focus: pebbles::core::focus::FocusNode,
    theme: &crate::theme::EditorTheme,
) -> AnyWidget {
    let matches = filtered(&commands, &pal.query.peek());
    let sel = if matches.is_empty() {
        0
    } else {
        pal.sel.peek().min(matches.len() - 1)
    };

    let field = text_field()
        .placeholder("Run a command…")
        .autofocus()
        .on_changed(move |s| {
            pal.query.set(s.to_string());
            pal.sel.set(0);
        })
        .on_submit(move |_| run_selected(pal, &commands, ctx, focus));

    let mut rows: Vec<AnyWidget> = vec![field.into_widget(), gap_h(6.0).into_widget()];
    for (i, cmd) in matches.iter().take(10).enumerate() {
        let bg = if i == sel {
            theme.selection
        } else {
            Color::from_rgba8(0, 0, 0, 0)
        };
        rows.push(
            container()
                .height(26.0)
                .padding(EdgeInsets::symmetric(8.0, 0.0))
                .decoration(
                    BoxDecoration::new()
                        .color(bg)
                        .radius(BorderRadius::all(4.0)),
                )
                .alignment(Alignment::CENTER_LEFT)
                .child(
                    text(cmd.title().to_string())
                        .size(13.0)
                        .font_family(MONO)
                        .color(theme.foreground),
                )
                .into_widget(),
        );
    }
    if matches.is_empty() {
        rows.push(
            text("no matching commands")
                .size(12.5)
                .color(theme.muted)
                .into_widget(),
        );
    }

    let panel = container()
        .width(520.0)
        .decoration(
            BoxDecoration::new()
                .color(theme.overlay_bg)
                .radius(BorderRadius::all(10.0))
                .border(Border::new(theme.border, 1.0)),
        )
        .padding(EdgeInsets::all(10.0))
        .child(column(rows).main_axis_size(MainAxisSize::Min));
    // Centered near the top of the editor.
    Positioned::new(align(Alignment::TOP_CENTER, panel))
        .left(0.0)
        .right(0.0)
        .top(40.0)
        .into_widget()
}
