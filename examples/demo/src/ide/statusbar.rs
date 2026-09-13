//! The bottom status bar: a (mock) git branch, the active file's language + dirty state, the
//! problem count, indentation, encoding, and the plugin state.

use pebbles::prelude::*;

use super::providers;
use super::settings::Settings;
use super::workspace::Workspace;

pub fn bar(ws: Workspace, s: Settings) -> AnyWidget {
    let c = theme().colors;
    let seg = |ic: pebbles::render::IconData, label: String| {
        row(children![
            icon(ic).size(12.0).color(c.primary_foreground),
            gap_w(5.0),
            text(label).size(11.5).color(c.primary_foreground),
        ])
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .into_widget()
    };
    let txt = |label: String| {
        text(label)
            .size(11.5)
            .color(c.primary_foreground)
            .into_widget()
    };

    let (lang, problems, dirty) = match ws.active.get() {
        Some(idx) => {
            let f = &ws.files[idx];
            let n = providers::compute_diagnostics(&f.content.peek()).len();
            (f.lang.to_string(), n, ws.is_dirty(idx))
        }
        None => ("—".to_string(), 0, false),
    };

    container()
        .height(24.0)
        .decoration(BoxDecoration::new().color(c.primary))
        .padding(EdgeInsets::symmetric(12.0, 0.0))
        .child(
            row(children![
                seg(lucide::GIT_BRANCH, "main".to_string()),
                gap_w(16.0),
                seg(lucide::TRIANGLE_ALERT, problems.to_string()),
                spacer(),
                txt(if dirty {
                    "● unsaved".to_string()
                } else {
                    "saved".to_string()
                }),
                gap_w(16.0),
                txt("Spaces: 4".to_string()),
                gap_w(16.0),
                txt("UTF-8".to_string()),
                gap_w(16.0),
                txt("LF".to_string()),
                gap_w(16.0),
                txt(lang),
                gap_w(16.0),
                txt(if s.plugins.get() {
                    "Plugins: on".to_string()
                } else {
                    "Plugins: off".to_string()
                }),
            ])
            .cross_axis_alignment(CrossAxisAlignment::Center),
        )
        .into_widget()
}
