# pebbles-code-editor

An embeddable, syntax-highlighting **code editor** widget for the
[Pebbles](https://github.com/pebbles-hq/pebbles) GUI framework — built to be the
foundation an IDE or code-first tool can grow on.

It's a **custom editing engine** — it owns the buffer, cursor, selection, layout, and
input rather than reusing the framework's text field. Code is monospace, so it renders
on a fixed grid, which
keeps caret placement and click hit-testing exact and cheap.

![demo](docs/demo.png)

```rust
use pebbles::prelude::*;
use pebbles_code_editor::{code_editor, lang::Rust, EditorTheme};

fn view() -> impl IntoWidget {
    let code = create_signal(String::from("fn main() {}\n"));
    code_editor(code)
        .language(Box::new(Rust))
        .theme(EditorTheme::dark())
        .title("main.rs")
        .autofocus()
        .height(420.0)
}
```

## Features

- **Real editing** — type, backspace / delete, word-delete (Ctrl+Backspace/Delete),
  Enter with **smart auto-indent**, arrows, word / line / document motions,
  **Shift-select**, Ctrl+A, and **Copy / Cut / Paste**. The buffer is the
  `Signal<String>` you pass in.
- **Mouse** — click to place the caret, drag to select.
- **Right-click menu** — Cut / Copy / Paste / Select All (toggle with `.context_menu`).
- **Syntax highlighting** via a pluggable [`Language`] layer. Bundled: **Rust,
  TypeScript, JavaScript, Python, Go, C, Java, JSON** (plus `Plain`). Add any grammar
  by implementing one trait method.
- **Gutter** with an active-line marker, **current-line highlight**, selection, caret,
  and a status bar (filename · language · line/col).
- **Themes** (`EditorTheme`) — `dark` and `light` bundled, or build your own.

## Configurable

Every knob is a builder on `code_editor(...)`:

| Builder | Default | What it does |
|---|---|---|
| `.language(Box::new(..))` | Plain | the highlighter |
| `.theme(EditorTheme)` | dark | colors |
| `.font_size(px)` | 13.5 | glyph size |
| `.tab_size(n)` / `.insert_spaces(bool)` | 4 / true | indent unit |
| `.gutter(bool)` | true | line-number column |
| `.current_line(bool)` | true | caret-line highlight |
| `.context_menu(bool)` | true | right-click menu |
| `.read_only(bool)` | false | navigable/selectable but not editable |
| `.autofocus()` | off | grab focus + show caret on mount |
| `.height(px)` | grow | fixed viewport (scrolls) vs. grow-to-content |
| `.title(name)` | none | filename in the status bar |

## Extending it — add a language

A highlighter is a function from source to tagged byte ranges:

```rust
use pebbles_code_editor::lang::{Language, Token, TokenKind};

struct Toml;
impl Language for Toml {
    fn name(&self) -> &str { "TOML" }
    fn highlight(&self, src: &str) -> Vec<Token> {
        // scan `src`, push Token { start, len, kind } for keywords, strings, …
        Vec::new()
    }
}
```

The editor maps each [`TokenKind`] to a color through the active theme, so a new
language highlights the moment you return its tokens. The bundled scanners are
hand-written, dependency-free, and **panic-free on any input** (an editor is full of
half-typed, invalid source).

## Run the sample — a full IDE

The `demo` crate is a **complete IDE** built on this editor + Pebbles, meant both as the
live testbed for every feature and as a reference for building an IDE on the framework: an
activity bar, a file explorer, project search, a settings panel, editor tabs with dirty
state, a Problems dock, a menu bar (File / Edit / View), and a status bar — around the
fully-wired code editor.

```sh
cargo run -p demo
# headless screenshot (no display needed):
SHOT=1400:900:/tmp/ide.rgba cargo run -p demo
```

Its source (`examples/demo/src/ide/`) is domain-split — `workspace` (state), `explorer`,
`tabs`, `editor_pane`, `search_panel`, `settings`, `problems`, `statusbar`, `providers` —
one concern per file, so it doubles as a worked example of structuring a Pebbles app.

## Status

Feature-complete for its scope and used within the Pebbles ecosystem via a git dependency;
**not yet published to crates.io** (pre-1.0 — any `0.x` release may break). The full feature
list is in [`CHANGELOG.md`](CHANGELOG.md); the design and layering are in
[`ARCHITECTURE.md`](ARCHITECTURE.md).

Shipped: multi-cursor editing with undo/redo, 17-language highlighting, folding, soft wrap,
search/replace, the full IntelliSense provider set (completion/hover/signature/diagnostics/
inlay/definition/format + snippets), an extension/plugin API, full theming, IME, drag-and-drop,
screen-reader semantics, OT/CRDT-ready collab hooks, large-file mode, inline diff, and
scroll/fold view-state persistence.

Known scope limits (larger, niche follow-ups): a side-by-side **merge** view (inline diff is
done), mid-line **inline** replace-widgets (block widgets are done), and a full bidi **caret**
on the monospace grid (bidi text already displays correctly).

## Quality

- **No `unsafe`** (`#![forbid(unsafe_code)]`) and **every public item documented**
  (`#![deny(missing_docs)]`).
- **Panic-free on malformed input** — guarded by [`tests/robustness.rs`]; ~100 unit + e2e tests.
- **CI** enforces `fmt`, `clippy -D warnings`, tests, and `doc -D warnings` on every push/PR.

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the module map, conventions, and the pre-push gates.

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
