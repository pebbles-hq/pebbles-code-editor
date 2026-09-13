# Architecture

`pebbles-code-editor` is organized **by domain**, the way Monaco and CodeMirror are — each
concern lives in its own module with a single, clear responsibility. `lib.rs` is a thin crate
root: module declarations and public re-exports, nothing else.

Contributions must keep this shape. When you add a feature, put it in the module that owns
that concern (or add a new domain module) — never grow one file into a grab-bag.

## Module map

```
src/
  lib.rs         Crate root — module wiring + public re-exports only. Keep it thin.

  # ── the document (model) ───────────────────────────────────────────────
  edit.rs        The text model & editing core: rope document, invertible + mappable
                 ChangeSet, immutable EditorState, Transaction (the single mutation path),
                 multi-cursor Selections, and undo/redo History. No widgets, no I/O.

  # ── language / syntax ──────────────────────────────────────────────────
  lang/
    mod.rs       The Language trait + Token/TokenKind, the structural SyntaxNode tree
                 (bracket_tree), Plain, and the grammar re-exports.
    scan.rs      Shared scanner primitives: a UTF-8-safe byte cursor + ident classifiers.
    rust.rs      The Rust highlighter.
    clike.rs     The C-family scanner (Grammar/CLike + the `grammar!` macro) — JS/TS/Go/
                 C/C++/C#/Java/Kotlin/Swift/PHP/Ruby/Shell/YAML/TOML.
    python.rs    The Python highlighter.
    json.rs      The JSON highlighter.
                 (All pure functions of source text; panic-free on any input.)

  # ── coordinate + text helpers (pure, shared) ───────────────────────────
  geometry.rs    Buffer navigation (byte ↔ line/column, word/line motions) and the
                 monospace-grid ↔ pixel mapping. Pure functions over a `&str` snapshot.
  brackets.rs    Default bracket pairs + nesting-aware match finding.
  highlight.rs   Token → themed rich-text spans; slice-to-window + semantic-overlay merges.

  # ── input (controller) ─────────────────────────────────────────────────
  commands.rs    The command engine: KeyInput → Transaction → dispatch. Multi-cursor edits,
                 motions, auto-close, comment toggle, smart indent. Owns no widgets.

  # ── IntelliSense providers (dev-supplied) ──────────────────────────────
  providers.rs   The provider trait/data types (completion, hover, signature, diagnostics,
                 inlay hints, definition, format). Data only — the app supplies the callbacks
                 and reactive signals; the editor renders the UI and wires the interactions.

  # ── search ─────────────────────────────────────────────────────────────
  search.rs      Pure find logic: match collection with case / whole-word / regex options
                 (the widget + replace actions live in view/search.rs).

  # ── configuration (public API) ─────────────────────────────────────────
  config.rs      The `code_editor()` builder + fluent `CodeEditor` config, and the resolved
                 `Props` the view consumes. The crate's front door.

  theme.rs       `EditorTheme` — every color slot, plus the bundled dark/light themes.

  # ── the view (rendering) ────────────────────────────────────────────────
  view/
    mod.rs       The `render_editor` component: reactive state, the read-model `Frame`, the
                 virtualized visible-line window, mouse gestures, and final assembly. Owns
                 orchestration only — each panel below is a pure(-ish) builder over `Frame`.
    overlays.rs  The stacked grid layers: current-line band, rulers, indent guides, selection
                 rects, bracket-match boxes, highlighted text, inlay hints, diagnostic
                 underlines, whitespace markers, carets.
    completion.rs The autocomplete popup: session state, trigger/accept, snippet expansion.
    search.rs    The find/replace bar widget + match-highlight overlay layers.
    gutter.rs    The virtualized line-number gutter (+ diagnostic marker dots).
    minimap.rs   The scaled document overview (one canvas node) + click/drag-to-scroll.
    sticky.rs    Sticky scroll — indentation-derived pinned scope headers.
    chrome.rs    Small shared view helpers: status bar, line band, tooltip, alpha tweak.
```

## Layering (dependencies point downward)

```
config ─┐
        ├─▶ view ─▶ overlays / gutter / minimap / sticky / chrome
        │            └─▶ highlight ─▶ lang
commands ┘            └─▶ brackets ─▶ geometry ─▶ edit
                      └─▶ commands / geometry / edit / theme
```

- **`edit`, `lang`, `geometry`, `theme`** are leaves — no dependency on the view or commands.
- **`highlight`, `brackets`** are pure transforms built on `lang`/`geometry`.
- **`commands`** is the controller (input → transaction); it never touches widgets.
- **`view`** is the only place that builds widgets; it reads `Props`, `EditorState`, and the
  pure helpers, and renders. Panels are split by concern and share the per-frame `Frame`
  read-model.
- **`config`** is the public surface; it produces `Props` and mounts `view::render_editor`.

## Conventions

- **One concern per file.** If a file starts mixing concerns, split it.
- **Pure where possible.** Geometry/highlight/brackets/lang are pure functions of text —
  easy to unit-test, no reactive state. Tests live next to the code they cover.
- **The view owns widgets; nothing else does.** Command/geometry/lang code returns data.
- **Signals live in `view::render_editor`;** builders take plain values (via `Frame`) or an
  explicit handle, so they stay simple and testable.
- **Never panic on bad input.** The editor is always full of half-typed, invalid source.
- **Every user-facing feature must be shown in `examples/demo`.** When you add a capability,
  wire it into the demo (a toggle, a sample provider, a legend entry) so it's visible and
  runnable (`cargo run -p demo`) — a feature that isn't in the demo isn't done.
