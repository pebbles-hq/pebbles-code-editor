# pebbles-code-editor

An embeddable, syntax-highlighting **code editor** widget for the
[Pebbles](https://github.com/pebbles-hq/pebbles) GUI framework — built to be the
foundation an IDE or code-first tool can grow on.

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
        .height(420.0)
}
```

## What's here (v0.1)

- **Syntax highlighting** via a pluggable [`Language`] layer. **Rust** and **JSON** are
  bundled; add a grammar by implementing one trait method.
- **Line-number gutter** and editor chrome (a status bar with the filename, language,
  and line count).
- **Themes** (`EditorTheme`) — `dark` and `light` bundled, or build your own by filling
  the struct (an IDE would expose a picker over these).
- **Monospace, grid-aligned, scrollable** rendering. The buffer lives in the `Signal`
  you pass in, so programmatic edits re-render live.

## Extending it — add a language

A highlighter is just a function from source to tagged byte ranges:

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
language is highlighted the moment you return its tokens. The bundled scanners are
hand-written, dependency-free, and **panic-free on any input** (an editor is full of
half-typed, invalid source).

## Run the sample

```sh
cargo run -p demo
# headless screenshot (no display needed):
SHOT=1100:760:/tmp/editor.rgba cargo run -p demo
```

## Roadmap — toward CodeMirror-grade

v0.1 is the **highlighted render pane** every editor is built around, plus the
pluggable-language + theme system. The next milestones:

1. **Live in-place editing** (caret, selection, keystrokes over the *highlighted* text).
   This needs a small enhancement in the framework's text engine: the editable
   (`RenderTextField`) currently paints a single color, so a highlighted overlay can't
   align to it. The fix is to let the editable paint **styled runs** — then this widget
   feeds it the same tokens it colors with today and editing + highlighting become one
   layer. (Tracked upstream in `pebbles`.)
2. Current-line highlight, bracket matching, active-line gutter.
3. Search / replace, code folding, multiple selections.
4. A completion / diagnostics surface an LSP client can drive.

## License

MIT OR Apache-2.0.
