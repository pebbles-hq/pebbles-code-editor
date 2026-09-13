//! A full **IDE** built on Pebbles + `pebbles-code-editor` — a reference for what the
//! framework can build, and the live testbed for every editor feature.
//!
//! What's in it:
//! - **Activity bar** (Explorer / Search / Settings) + a Problems-panel toggle.
//! - **File explorer** (the framework's `FileExplorer`: tree, rename, drag, context menus)
//!   over an in-memory polyglot project.
//! - **Editor tabs** with a dirty (•) indicator and close (×).
//! - **The code editor**, fully wired: syntax highlighting, multi-cursor, minimap, sticky
//!   scroll, completion, hover, signature help, diagnostics, inlay hints, go-to-def, format,
//!   search/replace, the command palette, and the sample plugin bundle.
//! - **Project search**, a **Settings** panel, a **Problems** dock, a **menu bar**
//!   (File / Edit / View), and a **status bar**.
//! - **Per-tab view state**: each file's scroll offset is persisted and restored on tab switch
//!   (via the editor's `.on_scroll()` / `.initial_scroll()` — §10 view state).
//!
//! Run it: `cargo run -p demo`
//! Headless screenshot: `SHOT=1400:900:/tmp/ide.rgba cargo run -p demo`
//! (optional `SHOT_KEYS=find|replace|palette` drives keys before the shot).

use pebbles::prelude::*;

mod capture;
mod ide;
mod samples;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    Theme::dark().make_current();
    if let Ok(spec) = std::env::var("SHOT") {
        return capture::shot(
            &spec,
            || ide::ide().into_widget(),
            theme().colors.background,
        );
    }
    App::new(component(ide::ide))
        .title("Pebbles IDE")
        .size(1400, 900)
        .background(theme().colors.background)
        .run()
}
