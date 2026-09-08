//! A sample app for `pebbles-code-editor` — a language **dropdown** on top and one
//! editor below, loaded with a sample per language so you can see the highlighting for
//! each. Editing works (type, select, arrows, Copy/Cut/Paste, right-click menu).
//!
//! Run it: `cargo run -p demo`
//! Headless screenshot: `SHOT=1100:780:/tmp/editor.rgba cargo run -p demo`

use pebbles::prelude::*;
use pebbles_code_editor::{EditorTheme, code_editor, lang, lang::Language};

mod capture;
mod samples;

use samples::SAMPLES;

fn lang_for(name: &str) -> Box<dyn Language> {
    match name {
        "Rust" => Box::new(lang::Rust),
        "TypeScript" => Box::new(lang::typescript()),
        "JavaScript" => Box::new(lang::javascript()),
        "Python" => Box::new(lang::Python),
        "Go" => Box::new(lang::go()),
        "C" => Box::new(lang::c_lang()),
        "Java" => Box::new(lang::java()),
        "JSON" => Box::new(lang::Json),
        _ => Box::new(lang::Plain),
    }
}

fn app() -> AnyWidget {
    let c = theme().colors;
    let sel = create_signal(0usize);
    let code = create_signal(String::from(SAMPLES[0].src));

    let names: Vec<String> = SAMPLES.iter().map(|s| s.name.to_string()).collect();
    let i = sel.get();
    let cur = &SAMPLES[i];

    let dropdown = select(names).value(i).on_changed(move |idx, _name| {
        sel.set(idx);
        code.set(String::from(SAMPLES[idx].src));
    });

    let editor_h = (media_query().size.height - 170.0).max(320.0);

    container()
        .color(c.background)
        .padding(EdgeInsets::all(28.0))
        .child(
            column(children![
                row(children![
                    column(children![
                        text("Pebbles Code Editor").size(22.0).bold().color(c.foreground),
                        gap_h(3.0),
                        text("A custom editing engine — highlighting for every language, editable.")
                            .size(13.0)
                            .color(c.muted_foreground),
                    ])
                    .cross_axis_alignment(CrossAxisAlignment::Start)
                    .main_axis_size(MainAxisSize::Min),
                    spacer(),
                    text("Language").size(12.5).weight(600.0).color(c.muted_foreground),
                    gap_w(10.0),
                    dropdown,
                ])
                .cross_axis_alignment(CrossAxisAlignment::Center),
                gap_h(18.0),
                code_editor(code)
                    .language(lang_for(cur.name))
                    .theme(EditorTheme::dark())
                    .title(cur.file)
                    .autofocus()
                    .height(editor_h),
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
    App::new(component(app)).title("Pebbles Code Editor").size(1100, 780).background(theme().colors.background).run()
}
