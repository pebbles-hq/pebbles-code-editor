//! Editor settings: a `Copy` bundle of reactive toggles + a font-size, and the Settings panel
//! UI that drives them. Every editor pane reads these, so changes apply live everywhere.

use pebbles::prelude::*;

use super::widgets::section;

/// All editor settings as `Copy` signals (so the whole struct is `Copy`).
#[derive(Clone, Copy)]
pub struct Settings {
    pub minimap: Signal<bool>,
    pub sticky: Signal<bool>,
    pub guides: Signal<bool>,
    pub whitespace: Signal<bool>,
    pub ruler: Signal<bool>,
    pub light: Signal<bool>,
    pub plugins: Signal<bool>,
    pub relaxed: Signal<bool>,
    pub font_size: Signal<f64>,
}

impl Settings {
    pub fn new() -> Self {
        Settings {
            minimap: create_signal(true),
            sticky: create_signal(true),
            guides: create_signal(true),
            whitespace: create_signal(false),
            ruler: create_signal(false),
            light: create_signal(false),
            plugins: create_signal(true),
            relaxed: create_signal(false),
            font_size: create_signal(13.5),
        }
    }
}

/// The Settings sidebar panel: grouped switches + a font-size stepper.
pub fn panel(s: Settings) -> AnyWidget {
    let c = theme().colors;
    let sw = move |flag: Signal<bool>, label: &str, hint: &str| {
        row(children![
            column(children![
                text(label.to_string()).size(13.0).color(c.foreground),
                text(hint.to_string()).size(11.0).color(c.muted_foreground),
            ])
            .main_axis_size(MainAxisSize::Min),
            spacer(),
            switch(flag.get()).on_changed(move || flag.set(!flag.peek())),
        ])
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .into_widget()
    };

    let font = s.font_size;
    let font_row = row(children![
        column(children![
            text("Font size").size(13.0).color(c.foreground),
            text("editor text size (px)").size(11.0).color(c.muted_foreground),
        ])
        .main_axis_size(MainAxisSize::Min),
        spacer(),
        icon_button(IconKind::Minus)
            .size(15.0)
            .on_pressed(move || font.set((font.peek() - 0.5).max(9.0))),
        container()
            .padding(EdgeInsets::symmetric(8.0, 0.0))
            .child(text(format!("{:.1}", font.get())).size(12.5).color(c.foreground)),
        icon_button(IconKind::Plus)
            .size(15.0)
            .on_pressed(move || font.set((font.peek() + 0.5).min(28.0))),
    ])
    .cross_axis_alignment(CrossAxisAlignment::Center)
    .into_widget();

    let body = column(children![
        section("Appearance"),
        sw(s.light, "Light theme", "swap the color scheme"),
        gap_h(12.0),
        font_row,
        gap_h(20.0),
        section("Editor"),
        sw(s.minimap, "Minimap", "scaled document overview"),
        gap_h(12.0),
        sw(s.sticky, "Sticky scroll", "pin enclosing scopes"),
        gap_h(12.0),
        sw(s.guides, "Indent guides", "vertical indent lines"),
        gap_h(12.0),
        sw(s.whitespace, "Render whitespace", "dots + arrows"),
        gap_h(12.0),
        sw(s.ruler, "Ruler at 80", "print-margin line"),
        gap_h(12.0),
        sw(s.relaxed, "Relaxed spacing", "taller lines + tracking"),
        gap_h(20.0),
        section("Extensions"),
        sw(s.plugins, "Plugins", "TODO highlight, bookmarks, commands"),
    ])
    .cross_axis_alignment(CrossAxisAlignment::Stretch)
    .main_axis_size(MainAxisSize::Min);

    scroll_view(container().padding(EdgeInsets::all(14.0)).child(body)).into_widget()
}
