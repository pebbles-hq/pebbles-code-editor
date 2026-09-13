//! The editor pane: for the active file, a fully-wired `code_editor` (language + all
//! providers + the current settings + the sample plugin bundle). Shows a welcome screen when
//! no file is open.

use pebbles::prelude::*;
use pebbles_code_editor::{
    Command, Decoration, EditorTheme, Extension, GutterMark, code_editor, extension,
};

use super::providers;
use super::settings::Settings;
use super::workspace::Workspace;

/// The sample plugin bundle (decorations, gutter markers, read-only region, commands +
/// keybinding + config facet + a theme tweak) — the same set the standalone showcase used.
fn sample_extensions() -> Vec<Extension> {
    let highlight = extension("todo-highlighter").decorations(|snap| {
        snap.text
            .match_indices("TODO")
            .map(|(i, _)| Decoration::background((i, i + 4), Color::from_rgba8(255, 190, 60, 55)))
            .collect()
    });
    let bookmarks = extension("todo-bookmarks").gutter_markers(|snap| {
        snap.text
            .match_indices("TODO")
            .map(|(i, _)| GutterMark {
                line: snap.text[..i].bytes().filter(|&b| b == b'\n').count(),
                color: Color::from_rgba8(255, 190, 60, 255),
            })
            .collect()
    });
    let commands = extension("edit-commands")
        .command(Command::new("edit.upper", "Uppercase Selection", |ctx| {
            let (a, b) = ctx.selection();
            if a != b {
                let up = ctx.text()[a..b].to_uppercase();
                ctx.replace(a, b, up);
            }
        }))
        .command(Command::new("edit.wrap-println", "Wrap Selection in println!", |ctx| {
            let (a, b) = ctx.selection();
            let inner = ctx.text()[a..b].to_string();
            ctx.replace(a, b, format!("println!(\"{{}}\", {inner});"));
        }))
        .keybinding("Ctrl+U", "edit.upper");
    let theming = extension("accent-tweaks").theme(|mut t| {
        t.syntax.string = t.syntax.string.bold();
        t
    });
    vec![highlight, bookmarks, commands, theming]
}

/// The editor pane for the active file (or a welcome screen). `height` is the pane's height so
/// the editor scrolls within it (enabling the minimap + sticky scroll).
pub fn pane(ws: Workspace, s: Settings, height: f64) -> AnyWidget {
    let c = theme().colors;
    let Some(idx) = ws.active.get() else {
        return welcome();
    };
    let f = &ws.files[idx];
    let code = f.content;
    let lang = f.lang;
    // Diagnostics + inlays are derived once per file in `ide()` (no per-render signals here).
    let diagnostics = f.diagnostics;
    let inlays = f.inlays;

    let relaxed = s.relaxed.get();
    let mut editor = code_editor(code)
        .language(providers::lang_for(lang))
        .theme(if s.light.get() { EditorTheme::light() } else { EditorTheme::dark() })
        .height(height.max(120.0))
        .font_size(s.font_size.get())
        .line_height(if relaxed { 2.0 } else { 1.6 })
        .letter_spacing(if relaxed { 1.2 } else { 0.0 })
        .autofocus()
        .minimap(s.minimap.get())
        .sticky_scroll(s.sticky.get())
        .indent_guides(s.guides.get())
        .render_whitespace(s.whitespace.get())
        .rulers(if s.ruler.get() { vec![80usize] } else { Vec::new() })
        .completion(providers::completion())
        .hover(providers::hover())
        .signature_help(providers::signature())
        .diagnostics(diagnostics)
        .inlay_hints(inlays)
        .definition(providers::definition())
        .format(providers::format());
    if s.plugins.get() {
        editor = editor.extensions(sample_extensions());
    }

    container()
        .color(c.background)
        .child(editor)
        .into_widget()
}

/// The empty-state shown when no file is open.
fn welcome() -> AnyWidget {
    let c = theme().colors;
    container()
        .color(c.background)
        .alignment(Alignment::CENTER)
        .child(
            column(children![
                icon(lucide::FILE_CODE).size(46.0).color(c.muted_foreground),
                gap_h(14.0),
                text("Pebbles IDE").size(20.0).bold().color(c.foreground),
                gap_h(4.0),
                text("Open a file from the Explorer to start editing.")
                    .size(13.0)
                    .color(c.muted_foreground),
            ])
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .main_axis_size(MainAxisSize::Min),
        )
        .into_widget()
}
