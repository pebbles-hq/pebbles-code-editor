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
    treesitter.rs Incremental tree-sitter highlighting (16 bundled grammars + injections),
                 viewport-limited queries, syntax fold ranges, smart-select node ladder.
                 (All pure functions of source text; panic-free on any input.)

  # ── coordinate + text helpers (pure, shared) ───────────────────────────
  geometry.rs    Buffer navigation (byte ↔ line/column, word/line motions) and the
                 monospace-grid ↔ pixel mapping. Pure functions over a `&str` snapshot.
  brackets.rs    Default bracket pairs + nesting-aware match finding.
  lines.rs       The maintained per-line index (line starts, char lengths, indentation,
                 fold regions incl. syntax + manual ranges), updated by edit delta; records
                 line shifts so folds / breakpoints follow edits.
  scan_text.rs   Pure in-text scanners: Unicode highlighting, links, colour literals,
                 linked-editing tag pairs.
  highlight.rs   Token → themed rich-text spans; slice-to-window + semantic-overlay merges.

  # ── input (controller) ─────────────────────────────────────────────────
  commands.rs    The command engine: KeyInput → Transaction → dispatch. Multi-cursor edits,
                 motions, auto-close, comment toggle, smart indent. Owns no widgets.
  actions.rs     The command list: every EditorCommand with its VS Code id, title and default
                 chord(s); chord parsing + multi-stroke resolution.
  keymap.rs      Host overrides: disable/rebind built-in intents and commands.
  ops.rs         Pure editing operations behind the command list (line ops, comments, case,
                 subword, smart paste, indentation detection, line endings).
  controller.rs  EditorController — host-driven caret / selection / scroll / commands.
  emmet.rs       Emmet abbreviation expansion for HTML and CSS (pure).
  snippets.rs    Built-in per-language snippets + user snippets, merged into completion.

  # ── IntelliSense providers (dev-supplied) ──────────────────────────────
  providers.rs   The provider trait/data types (completion, hover, signature, diagnostics,
                 inlay hints, definition, format). Data only — the app supplies the callbacks
                 and reactive signals; the editor renders the UI and wires the interactions.

  # ── search ─────────────────────────────────────────────────────────────
  search.rs      Pure find logic: match collection with case / whole-word / regex options
                 (the widget + replace actions live in view/search.rs).

  # ── folding + soft wrap (display model) ────────────────────────────────
  fold.rs        Indentation-derived foldable regions + the `DisplayMap`: the buffer-line ↔
                 visual-row mapping the whole view renders through. A folded line contributes
                 zero rows; a wrapped line contributes several. Pure, no widgets.

  # ── inline diff (public API) ───────────────────────────────────────────
  diff.rs        Bounded hunk line-diff against a base text (change bars, inline bands, hunk
                 revert) + the word-level token LCS used by the diff editor. Pure, no widgets.
  diff_editor.rs The public `diff_editor(original, modified)` widget: side-by-side (two
                 line-aligned, scroll-linked editors) or inline.

  # ── collaboration (public API) ─────────────────────────────────────────
  collab.rs      The `Edit` delta type + pure `diff(old,new)` / `map_pos` — the OT/CRDT-ready
                 change stream (`.on_edit`) and remote-caret remapping. No widgets, no I/O.

  # ── extensibility / plugins (public API) ───────────────────────────────
  extensions.rs  The plugin model: the composable `Extension` (decorations, gutter markers,
                 read-only ranges, commands, keybindings, a mergeable `ConfigPatch`, and
                 change/selection/focus/click hooks), the `Snapshot` handed to callbacks, and
                 the `EditContext` a `Command` uses to read/mutate. Data + builders only; the
                 view renders the contributions, registers the keymap, and shows the palette.

  # ── configuration (public API) ─────────────────────────────────────────
  config.rs      The `code_editor()` builder + fluent `CodeEditor` config, and the resolved
                 `Props` the view consumes. The crate's front door.

  theme.rs       `EditorTheme` (every chrome slot) + a separable `HighlightStyle` (a
                 `TokenStyle` = color + bold/italic/underline per `TokenKind`); bundled
                 dark/light for both. Compose one over a base via an extension `theme`
                 transform.

  # ── the view (rendering) ────────────────────────────────────────────────
  view/
    mod.rs       The `render_editor` component: reactive state, the read-model `Frame`, the
                 virtualized visible-line window, mouse gestures, and final assembly. Owns
                 orchestration only — each panel below is a pure(-ish) builder over `Frame`.
    overlays.rs  The stacked grid layers: current-line band, rulers, indent guides, selection
                 rects, bracket-match boxes, highlighted text, inlay hints, diagnostic
                 underlines, whitespace markers, carets.
    completion.rs The autocomplete popup: session state, trigger/accept, snippet expansion,
                 word-based suggestions.
    commands.rs  Runs EditorCommands against the live view (folding incl. manual ranges,
                 wrap, zoom, scrolling, find navigation, cursor history, inline suggestions,
                 overtype / tab-focus modes, Emmet).
    decor.rs     VS Code surface decorations: occurrences, active indent guide, bracket-pair
                 guides, Unicode marks, links, colour swatches + picker, fold badge with the
                 hidden-lines preview, linked-editing outlines.
    diffview.rs  Diff-editor contributions: removed-line bands, word highlights, filler rows,
                 inline removed rows.
    peek.rs      The change peek (click a change bar): base lines, Revert, Stage, prev/next.
    ruler.rs     The overview ruler in the scrollbar lane.
    search.rs    The find/replace bar widget + match-highlight overlay layers.
    extensions.rs Extension rendering: decoration overlay layers (background/underline/line)
                 + the command-palette overlay (Ctrl+Shift+P).
    gutter.rs    The virtualized gutter: breakpoint area, change bars, diagnostic & extension
                 dots, line numbers (absolute/relative), fold arrows.
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
- **Every user-facing feature must be shown in `examples/demo`.** The demo is a **full IDE**
  (`examples/demo/src/ide/` — explorer, tabs, search, settings, problems, menu/status bars
  around the editor); wire each new capability into it (a settings toggle, a sample provider,
  a plugin) so it's visible and runnable (`cargo run -p demo`) — a feature that isn't in the
  demo isn't done. The IDE is itself domain-split (one concern per module), mirroring this crate.
