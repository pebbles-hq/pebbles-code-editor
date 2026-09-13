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

/// A clickable list row that highlights on hover (and, when `active`, stays highlighted).
/// `child` is the row content; `on_tap` fires on press.
pub fn list_row(active: bool, child: AnyWidget, on_tap: impl Fn() + 'static) -> AnyWidget {
    let c = theme().colors;
    let hovered = create_signal(false);
    let bg = if active {
        c.accent
    } else if hovered.get() {
        c.muted
    } else {
        Color::from_rgba8(0, 0, 0, 0)
    };
    GestureDetector::new(
        container()
            .height(26.0)
            .padding(EdgeInsets::symmetric(8.0, 0.0))
            .decoration(BoxDecoration::new().color(bg).radius(BorderRadius::all(5.0)))
            .alignment(Alignment::CENTER_LEFT)
            .child(child),
    )
    .cursor(Cursor::Pointer)
    .on_tap(action(on_tap))
    .on_hover_enter(action(move || hovered.set(true)))
    .on_hover_exit(action(move || hovered.set(false)))
    .into_widget()
}
