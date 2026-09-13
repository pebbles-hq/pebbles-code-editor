//! The bottom "Problems" dock: the active file's diagnostics (severity icon + message + line),
//! recomputed from the buffer.

use pebbles::prelude::*;

use super::providers;
use super::workspace::Workspace;
use pebbles_code_editor::Severity;

pub fn dock(ws: Workspace) -> AnyWidget {
    let c = theme().colors;

    let (content, diags) = match ws.active.get() {
        Some(idx) => {
            let src = ws.files[idx].content.peek();
            let d = providers::compute_diagnostics(&src);
            (src, d)
        }
        None => (String::new(), Vec::new()),
    };

    let header = container()
        .padding(EdgeInsets::symmetric(12.0, 8.0))
        .decoration(BoxDecoration::new().color(c.muted))
        .child(
            row(children![
                text("PROBLEMS")
                    .size(10.5)
                    .weight(700.0)
                    .letter_spacing(0.6)
                    .color(c.muted_foreground),
                gap_w(8.0),
                container()
                    .padding(EdgeInsets::symmetric(6.0, 1.0))
                    .decoration(BoxDecoration::new().color(c.border).radius(BorderRadius::all(8.0)))
                    .child(text(diags.len().to_string()).size(10.5).color(c.foreground)),
            ])
            .cross_axis_alignment(CrossAxisAlignment::Center),
        );

    let mut rows: Vec<AnyWidget> = Vec::new();
    if diags.is_empty() {
        rows.push(
            container()
                .padding(EdgeInsets::all(12.0))
                .child(text("No problems detected.").size(12.0).color(c.muted_foreground))
                .into_widget(),
        );
    } else {
        for d in &diags {
            let (col, kind) = match d.severity {
                Severity::Error => (c.destructive, IconKind::Close),
                Severity::Warning => (c.warning, IconKind::Warning),
                Severity::Info => (c.primary, IconKind::Info),
                Severity::Hint => (c.muted_foreground, IconKind::Dot),
            };
            let line = content[..d.range.0.min(content.len())]
                .bytes()
                .filter(|&b| b == b'\n')
                .count()
                + 1;
            rows.push(
                container()
                    .padding(EdgeInsets::symmetric(12.0, 4.0))
                    .child(
                        row(children![
                            icon(kind).size(13.0).color(col),
                            gap_w(8.0),
                            text(d.message.clone()).size(12.0).color(c.foreground),
                            gap_w(8.0),
                            text(format!("[Ln {line}]")).size(11.5).color(c.muted_foreground),
                        ])
                        .cross_axis_alignment(CrossAxisAlignment::Center),
                    )
                    .into_widget(),
            );
        }
    }

    column(children![
        header,
        expanded(scroll_view(column(rows).cross_axis_alignment(CrossAxisAlignment::Stretch))),
    ])
    .cross_axis_alignment(CrossAxisAlignment::Stretch)
    .into_widget()
}
