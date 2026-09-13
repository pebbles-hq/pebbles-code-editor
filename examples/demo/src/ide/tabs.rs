//! The editor tab strip: one tab per open file, with a dirty (•) indicator and a close (×).
//! Clicking a tab activates it; clicking × closes it.

use pebbles::prelude::*;

use super::workspace::Workspace;

pub fn bar(ws: Workspace) -> AnyWidget {
    let c = theme().colors;
    let open = ws.open.get();
    let active = ws.active.get();

    if open.is_empty() {
        return container()
            .height(38.0)
            .decoration(BoxDecoration::new().color(c.muted))
            .into_widget();
    }

    let mut tabs: Vec<AnyWidget> = Vec::new();
    for idx in open {
        let f = &ws.files[idx];
        let is_active = active == Some(idx);
        let dirty = ws.is_dirty(idx);
        let bg = if is_active { c.background } else { c.muted };
        let fg = if is_active {
            c.foreground
        } else {
            c.muted_foreground
        };

        // The label area (activates the tab).
        let ws_act = ws.clone();
        let label = GestureDetector::new(
            row(children![
                icon(lucide::FILE).size(13.0).color(fg),
                gap_w(7.0),
                text(f.name.clone()).size(12.5).color(fg),
            ])
            .cross_axis_alignment(CrossAxisAlignment::Center),
        )
        .cursor(Cursor::Pointer)
        .on_tap(action(move || ws_act.active.set(Some(idx))));

        // Dirty dot, else a close button.
        let ws_close = ws.clone();
        let trailer: AnyWidget = if dirty {
            container()
                .width(8.0)
                .height(8.0)
                .decoration(
                    BoxDecoration::new()
                        .color(fg)
                        .radius(BorderRadius::all(4.0)),
                )
                .into_widget()
        } else {
            icon_button(IconKind::Close)
                .size(13.0)
                .on_pressed(move || ws_close.close_file(idx))
                .into_widget()
        };

        // An active tab gets an accent top-border and the editor background.
        let top = if is_active { c.accent } else { c.border };
        tabs.push(
            container()
                .height(38.0)
                .padding(EdgeInsets::symmetric(12.0, 0.0))
                .decoration(BoxDecoration::new().color(bg).border(Border::only(
                    BorderSide::new(top, 2.0),
                    BorderSide::new(c.border, 1.0),
                    BorderSide::NONE,
                    BorderSide::NONE,
                )))
                .child(
                    row(children![label, gap_w(8.0), trailer])
                        .cross_axis_alignment(CrossAxisAlignment::Center),
                )
                .into_widget(),
        );
    }

    container()
        .height(38.0)
        .decoration(BoxDecoration::new().color(c.muted))
        .child(SingleChildScrollView::horizontal(
            row(tabs).cross_axis_alignment(CrossAxisAlignment::Stretch),
        ))
        .into_widget()
}
