//! A sample app for `pebbles-code-editor` — a window with the editor loaded with some
//! Rust, plus a JSON panel, so you can see highlighting, the gutter, and editing.
//!
//! Run it: `cargo run -p demo`
//! Headless screenshot: `SHOT=1100:760:/tmp/editor.rgba cargo run -p demo`

use pebbles::prelude::*;
use pebbles_code_editor::{EditorTheme, code_editor, lang};

mod capture;

const SAMPLE_RS: &str = r#"use pebbles::prelude::*;

/// A counter — state is a signal, events are closures.
#[component]
fn counter(start: i64) -> impl IntoWidget {
    let count = create_signal(start);
    center(column(children![
        text(count.get().to_string()).size(48.0).bold(),
        button("Add one").on_pressed(move || {
            count.update(|n| *n += 1); // one line, no boilerplate
        }),
    ]))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Comments, "strings", numbers 42, macros, and Types all get colored.
    App::new(component(|| counter(0))).title("Counter").run()
}
"#;

const SAMPLE_JSON: &str = r#"{
  "name": "pebbles-code-editor",
  "version": "0.1.0",
  "editable": true,
  "languages": ["rust", "json"],
  "lines": 200
}
"#;

fn app() -> AnyWidget {
    let c = theme().colors;
    let rust = create_signal(String::from(SAMPLE_RS));
    let json = create_signal(String::from(SAMPLE_JSON));

    container()
        .color(c.background)
        .padding(EdgeInsets::all(28.0))
        .child(
            column(children![
                text("Pebbles Code Editor").size(22.0).bold().color(c.foreground),
                gap_h(4.0),
                text("Syntax highlighting · line-number gutter · real editing · pluggable languages")
                    .size(13.5)
                    .color(c.muted_foreground),
                gap_h(20.0),
                code_editor(rust)
                    .language(Box::new(lang::Rust))
                    .theme(EditorTheme::dark())
                    .title("main.rs")
                    .autofocus()
                    .height(430.0),
                gap_h(18.0),
                code_editor(json)
                    .language(Box::new(lang::Json))
                    .theme(EditorTheme::dark())
                    .title("package.json")
                    .height(150.0),
            ])
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .main_axis_size(MainAxisSize::Min),
        )
        .into_widget()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    Theme::dark().make_current();
    if let Ok(spec) = std::env::var("SHOT") {
        return capture::shot(&spec, app, theme().colors.background);
    }
    App::new(component(app)).title("Pebbles Code Editor").size(1100, 760).background(theme().colors.background).run()
}
