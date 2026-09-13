# Changelog

All notable changes to `pebbles-code-editor` are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[semantic versioning](https://semver.org/) (pre-1.0: any `0.x` release may break).

## [Unreleased]

The crate is feature-complete for its scope and used within the Pebbles ecosystem via a
git dependency; it is not yet published to crates.io.

### Editing
- Rope-backed document with an invertible/mappable `ChangeSet` → `Transaction` → `dispatch`
  mutation path; undo/redo with typing/deleting coalescing.
- Multi-cursor & selections (Alt-click add, Ctrl/Cmd+D add-next, Shift+Alt column select,
  double/triple-click word/line), with every edit and motion applied to all cursors.
- Word/line/doc motions with a preserved goal column; system clipboard cut/copy/paste;
  right-click context menu; read-only mode.
- Auto-close brackets/quotes, comment toggle (Ctrl+/), Tab/Shift+Tab indent-outdent, and
  language-aware auto-indent (open a block between a pair, dedent a closing bracket).
- Drag-and-drop text (move, or copy with Ctrl/Cmd); IME composition (CJK / dead keys).

### Rendering
- Monospace-grid layout with exact caret/click math, a line-number gutter, current-line
  highlight, blinking caret, indent guides, whitespace rendering, and print-margin rulers.
- Viewport virtualization (renders only the visible rows) + large-file mode (window-only
  tokenization past 5k lines), a minimap, and sticky scope headers.
- **Code folding** and **soft wrap** on a shared visual-row display model.
- Horizontal scroll (disabled while wrapping); configurable font family / size / line-height /
  letter-spacing.

### Language & IntelliSense
- Pluggable `Language` highlighting (17 bundled grammars + Plain) with memoized, panic-free,
  error-tolerant scanning; a coarse structural tree for tooling; bracket matching.
- Provider hooks: completion popup (+ snippets), hover, signature help, diagnostics, inlay
  hints, go-to-definition, and document formatting; plus a semantic-token overlay.

### Extensibility
- A composable `Extension`: decorations (mark/line background), gutter markers, block widgets,
  read-only ranges, an edit-transform filter, commands + a configurable keymap + command
  palette, mergeable config facets, theme transforms, and change/selection/focus/click hooks.

### Theming
- Separable syntax palette (`HighlightStyle`, `TokenStyle` with weight/italic/underline) and a
  full chrome-slot `EditorTheme`; light + dark bundled; theme-as-extension composition.

### Collaboration & view state
- OT/CRDT-ready `on_edit` delta stream with remote-edit caret remapping (`src/collab.rs`).
- Inline diff against a base text (`diff_base`, git-gutter style).
- Persist/restore scroll and fold state; screen-reader `TextInput` semantics; opt-in touch
  selection handles.

### Quality
- No panics on malformed input (empty/huge/multi-byte/degenerate) — guarded by
  `tests/robustness.rs`; every public item is documented (`#![deny(missing_docs)]`); the crate
  is `#![forbid(unsafe_code)]`.
- CI enforces `fmt`, `clippy -D warnings`, tests, and `doc -D warnings`.
- The `examples/demo` crate is a full IDE (explorer, tabs, search, settings, problems, menu +
  status bars) built on the editor — the live testbed and a reference for building an IDE.
