//! Project-wide search: a query field, then per-file grouped matches (file + line no. +
//! the matching line). Clicking a result opens that file.

use pebbles::prelude::*;

use super::widgets::{list_row, section};
use super::workspace::Workspace;

pub fn panel(ws: Workspace) -> AnyWidget {
    let c = theme().colors;
    let query = create_signal(String::new());

    let header = container().padding(EdgeInsets::all(12.0)).child(
        column(children![
            section("Search"),
            text_field()
                .placeholder("Search across files")
                .value(query.get())
                .on_changed(move |s| query.set(s.to_string())),
        ])
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .main_axis_size(MainAxisSize::Min),
    );

    let q = query.get();
    let mut groups: Vec<AnyWidget> = Vec::new();
    let mut total = 0usize;
    if q.len() >= 2 {
        let needle = q.to_lowercase();
        for (idx, f) in ws.files.iter().enumerate() {
            let content = f.content.peek();
            let hits: Vec<(usize, String)> = content
                .lines()
                .enumerate()
                .filter(|(_, l)| l.to_lowercase().contains(&needle))
                .take(8)
                .map(|(n, l)| (n + 1, l.trim().to_string()))
                .collect();
            if hits.is_empty() {
                continue;
            }
            total += hits.len();
            let ws_open = ws.clone();
            let mut rows: Vec<AnyWidget> = vec![
                container()
                    .padding(EdgeInsets::only(2.0, 8.0, 4.0, 2.0))
                    .child(
                        row(children![
                            icon(lucide::FILE).size(12.0).color(c.muted_foreground),
                            gap_w(6.0),
                            text(f.path.clone()).size(12.0).weight(600.0).color(c.foreground),
                        ])
                        .cross_axis_alignment(CrossAxisAlignment::Center),
                    )
                    .into_widget(),
            ];
            for (ln, line) in hits {
                let ws_row = ws_open.clone();
                let mut snippet = line;
                snippet.truncate(80);
                rows.push(list_row(
                    false,
                    row(children![
                        container()
                            .width(34.0)
                            .child(text(ln.to_string()).size(11.5).color(c.muted_foreground)),
                        text(snippet).size(12.0).color(c.muted_foreground),
                    ])
                    .cross_axis_alignment(CrossAxisAlignment::Center)
                    .into_widget(),
                    move || ws_row.open_file(idx),
                ));
            }
            groups.push(
                container()
                    .padding(EdgeInsets::symmetric(8.0, 4.0))
                    .child(column(rows).cross_axis_alignment(CrossAxisAlignment::Stretch))
                    .into_widget(),
            );
        }
    }

    let summary = if q.len() < 2 {
        "type at least 2 characters".to_string()
    } else if total == 0 {
        "no results".to_string()
    } else {
        format!("{total} result(s)")
    };

    column(children![
        header,
        container()
            .padding(EdgeInsets::symmetric(14.0, 0.0))
            .child(text(summary).size(11.5).color(c.muted_foreground)),
        gap_h(6.0),
        expanded(scroll_view(column(groups).cross_axis_alignment(CrossAxisAlignment::Stretch))),
    ])
    .cross_axis_alignment(CrossAxisAlignment::Stretch)
    .into_widget()
}
