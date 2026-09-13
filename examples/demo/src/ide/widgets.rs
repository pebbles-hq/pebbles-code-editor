//! Small shared IDE chrome helpers: a section header and a hover-highlighting clickable row.

use pebbles::prelude::*;

/// An uppercase, muted section header (used in the sidebar panels).
pub fn section(title: &str) -> AnyWidget {
    let c = theme().colors;
    container()
        .padding(EdgeInsets::only(2.0, 0.0, 0.0, 6.0))
        .child(
            text(title.to_uppercase())
                .size(10.5)
                .weight(700.0)
                .letter_spacing(0.6)
                .color(c.muted_foreground),
        )
        .into_widget()
}

/// A clickable list row. Built on the framework's `pressable` (which owns its own hover/press
/// tint in its own component scope), so this stays a plain builder with no local signals —
/// safe to call any number of times per render. `active` rows keep an accent background.
pub fn list_row(active: bool, child: AnyWidget, on_tap: impl Fn() + 'static) -> AnyWidget {
    let c = theme().colors;
    let bg = if active {
        c.accent
    } else {
        Color::from_rgba8(0, 0, 0, 0)
    };
    pressable(
        container()
            .height(26.0)
            .padding(EdgeInsets::symmetric(8.0, 0.0))
            .decoration(
                BoxDecoration::new()
                    .color(bg)
                    .radius(BorderRadius::all(5.0)),
            )
            .alignment(Alignment::CENTER_LEFT)
            .child(child),
    )
    .radius(5.0)
    .on_tap(on_tap)
    .into_widget()
}
