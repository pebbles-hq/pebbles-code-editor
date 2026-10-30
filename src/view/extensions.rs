//! View support for [`crate::extensions`]: rendering extension **decorations** (background /
//! underline / line-background layers) and the **command palette** (a searchable overlay
//! that runs any registered [`Command`]).

use pebbles::prelude::*;

use crate::MONO;
use crate::extensions::{Command, Decoration, EditContext};
use crate::view::Frame;

/// Build overlay layers for `decos`, clipped to the visible window (line backgrounds first,
/// then backgrounds, then underlines).
pub(crate) fn decoration_layers(f: &Frame, decos: &[Decoration]) -> Vec<AnyWidget> {
    let mut layers = Vec::new();
    // Line backgrounds (behind everything).
    for d in decos {
        if let Some(bg) = d.style.line_background {
            let (la, lb) = (
                f.li.line_of(f.src, d.range.0),
                f.li.line_of(f.src, d.range.1),
            );
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
        for line in f.li.line_of(f.src, d.range.0).max(f.first_line)
            ..=f.li.line_of(f.src, d.range.1).min(f.last_line)
        {
            let (la, lb) = (
                f.li.line_of(f.src, d.range.0),
                f.li.line_of(f.src, d.range.1),
            );
            let start_col = if line == la {
                f.li.col_of(f.src, d.range.0)
            } else {
                0
            };
            let end_col = if line == lb {
                f.li.col_of(f.src, d.range.1)
            } else {
                f.li.line_char_len(line)
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

/// Parse a Go-to-Line query (`:12`, `:12:5`, 1-based) into a 0-based `(line, col)`.
pub(crate) fn parse_goto(q: &str) -> Option<(usize, usize)> {
    let rest = q.strip_prefix(':')?;
    let mut it = rest.split([':', ',']);
    let line: usize = it.next()?.trim().parse().ok()?;
    let col: usize = it.next().and_then(|c| c.trim().parse().ok()).unwrap_or(1);
    Some((line.saturating_sub(1), col.saturating_sub(1)))
}

/// Run the selected command (if any) and close the palette.
pub(crate) fn run_selected(
    pal: Palette,
    commands: &[Command],
    ctx: EditContext,
    focus: pebbles::core::focus::FocusNode,
) {
    let q = pal.query.peek();
    if q.starts_with(':') {
        if let Some((line, col)) = parse_goto(&q) {
            let text = ctx.text();
            let n = text.split('\n').count();
            let b = crate::geometry::byte_at(&text, line.min(n.saturating_sub(1)), col);
            ctx.select(b, b);
        }
        pal.open.set(false);
        focus.request_focus();
        return;
    }
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
    let goto = pal.query.peek().starts_with(':');
    let matches = if goto {
        Vec::new()
    } else {
        filtered(&commands, &pal.query.peek())
    };
    let sel = if matches.is_empty() {
        0
    } else {
        pal.sel.peek().min(matches.len() - 1)
    };

    let field = text_field()
        .placeholder("Run a command…  (type : to go to a line)")
        .bind(pal.query)
        .autofocus()
        .on_changed(move |_| {
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
                        .radius(BorderRadius::all(theme.overlay_radius / 2.0)),
                )
                .alignment(Alignment::CENTER_LEFT)
                .child(
                    text(cmd.title().to_string())
                        .size((theme.ui_font_size + 0.5) as f32)
                        .font_family(
                            theme
                                .ui_font_family
                                .clone()
                                .unwrap_or_else(|| MONO.to_string()),
                        )
                        .color(theme.foreground),
                )
                .into_widget(),
        );
    }
    if goto {
        let q = pal.query.peek();
        let label = match parse_goto(&q) {
            Some((l, c)) => format!("Go to line {}, column {}", l + 1, c + 1),
            None => "Type a line number (and :column) to go to".to_string(),
        };
        rows.push(
            theme
                .ui_text(label, 0.0)
                .color(theme.foreground)
                .into_widget(),
        );
    } else if matches.is_empty() {
        rows.push(
            theme
                .ui_text("no matching commands", 0.0)
                .color(theme.muted)
                .into_widget(),
        );
    }

    let panel = container()
        .width(520.0)
        .decoration(
            BoxDecoration::new()
                .color(theme.overlay_bg)
                .radius(BorderRadius::all(theme.overlay_radius + 2.0))
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
