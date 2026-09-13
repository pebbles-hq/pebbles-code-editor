//! Editor chrome: the small shared view helpers — the status bar, a full-width line band,
//! and an alpha tweak — used by the orchestrator and the overlay/sticky builders.

use pebbles::prelude::*;

use crate::theme::EditorTheme;

/// A full-viewport-width horizontal band at `top` (the current-line highlight).
pub(crate) fn band(top: f64, h: f64, color: Color) -> AnyWidget {
    Positioned::new(
        container()
            .height(h)
            .decoration(BoxDecoration::new().color(color)),
    )
    .left(0.0)
    .right(0.0)
    .top(top)
    .into_widget()
}

/// The bottom status bar: filename, language, and the primary caret's Ln/Col.
pub(crate) fn status_bar(
    title: &str,
    lang: &str,
    ln: usize,
    col: usize,
    theme: &EditorTheme,
) -> impl IntoWidget {
    container()
        .decoration(BoxDecoration::new().color(theme.gutter_bg))
        .padding(EdgeInsets::symmetric(14.0, 9.0))
        .child(
            row(children![
                icon(lucide::FILE_CODE)
                    .size(14.0)
                    .color(theme.gutter_active_fg),
                gap_w(8.0),
                text(title.to_string())
                    .size(12.5)
                    .weight(600.0)
                    .color(theme.foreground),
                spacer(),
                text(format!("{lang} · Ln {ln}, Col {col}"))
                    .size(11.5)
                    .color(theme.gutter_fg),
            ])
            .cross_axis_alignment(CrossAxisAlignment::Center),
        )
}

/// `c` with its alpha replaced by `a` (for translucent borders/dividers/overlays).
pub(crate) fn with_alpha(c: Color, a: f32) -> Color {
    let [r, g, b, _] = c.components;
    Color::new([r, g, b, a])
}
