# Changelog

All notable changes to `pebbles-code-editor` are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[semantic versioning](https://semver.org/) (pre-1.0: any `0.x` release may break).

## [Unreleased]

### Added — VS Code editor parity (phase 1)
- **Syntax:** incremental tree-sitter highlighting (Rust, TOML, JSON, Markdown, JS, TS/TSX,
  Python, Go, C, C++, HTML, CSS, YAML, Shell, SQL) with injections (Markdown code blocks,
  `<script>` / `<style>`), viewport-limited queries, syntax-based folding.
- **Commands:** the VS Code editor command list (`EditorCommand`, ids + default chords incl.
  multi-stroke `Ctrl+K …`), remappable via `Keymap`, all in the palette: line operations,
  multi-cursor, column select, comments, case transforms, subword motion, smart Home, cursor
  undo, fold levels / `#region` markers / fold selection (`Ctrl+K Ctrl+,`), word-wrap toggle,
  font zoom, Go to Line, next/previous change, smart select, overtype (`Insert`), tab focus
  mode (`Ctrl+M`), inline-suggestion cycling, Emmet expand.
- **Find/replace:** find in selection, preserve case, regex capture groups, select all
  matches, query seeding (the seeded query is selected), match count, replace all.
- **Surface:** git change bars with inline peek (revert / stage) and Alt+F3 navigation,
  overview ruler, occurrence + selection highlights, bracket-pair guides, active indent guide,
  Unicode / control-character highlighting, clickable links, colour swatches with picker,
  linked editing of tag pairs, cursor styles + blink modes + smooth caret, relative line
  numbers, folded-region badge with a hover preview of the hidden lines, wrap indent,
  long-line virtualization, Ctrl+hover underline of the Ctrl+click target.
- **Gutter:** `.breakpoints(signal)` glyph margin; folds and breakpoints now follow edits that
  add or remove lines.
- **Suggestions:** word-based completion when no provider is set
  (`.word_based_suggestions`), suggestion preview (`.suggest_preview`), inline ghost-text
  suggestions (`.inline_suggestions`, Tab / Escape / Alt+] / Alt+[ / Alt+\), built-in and
  user snippets (`.snippets`, `builtin_snippets`), Emmet for HTML/CSS (`.emmet`).
- **Diff editor:** `diff_editor(original, modified)` — side-by-side (line-aligned, scroll-
  linked) or inline (`.side_by_side(false)` / `CodeEditor::inline_diff`), word-level diff.
- **Other:** `.format_on_paste`, `.smooth_scrolling`, `EditorTheme::high_contrast_dark()` /
  `high_contrast_light()` (WCAG AAA).

### Fixed
- The find field re-requested focus on every rebuild; it now autofocuses only when opened.

### Fixed — editor state follows the bound document; Added — `EditorController`
The editor built its state from `code` only on first mount, so binding a *different*
`Signal<String>` (switching files) kept the previous document's carets/undo/scroll and stale
closures. It is now keyed by document identity: a different signal, or a changed
`.document_id(u64)` (for a reused signal), remounts it with fresh state.
New `editor_controller()` + `.controller(ctrl)`: `set_cursor(line, col)`, `select(anchor, head)`,
`scroll_to(line)`, `focus()` and a reactive `cursor()` (0-based, char columns). Requests are
applied on the next render, so "switch file + open at line" in one handler lands on the new
document.

### Added — overridable built-in keybindings (`Keymap`)
`code_editor(..).keymap(Keymap::new().disable(BuiltinCommand::CommandPalette).bind("F1",
BuiltinCommand::CommandPalette))`. A disabled built-in is declined so the key bubbles to the
app's shortcuts; intents the editor can't serve (completion / go-to-definition / format with no
provider) bubble too. **Changed:** following pebbles (VS Code chords), the editor palette opens
on Ctrl+Shift+P; Ctrl+P is no longer an editor key and reaches the app (Quick Open).

### Changed — the framework renders with wgpu only
Requires pebbles ≥ `5a9fdbd` (wgpu is the only renderer and cosmic-text the only text stack).
The renderer-selection features of this crate (and its demo) are gone: depend on
it with default features. Nothing else changes for users; the demo capture uses the public offscreen API.

**Migration:** none for users of default features.

### Changed — demo screenshots use the framework's offscreen renderer
`SHOT=` now calls the framework's `pebbles::shell::capture::capture_rgba` (settled frame,
system fonts — same as the app); the demo no longer drives a renderer itself.

### Changed — the editor follows the app's light/dark theme by default
Without `.theme(..)`, `code_editor` now uses the new `EditorTheme::from_theme()` — the
light or dark editor theme matching the current Pebbles theme — resolved in the parent's
build, so `ThemeMode::System`, `set_theme_mode` and `toggle_theme` switch it live. It was
always `EditorTheme::dark()`. **Migration:** call `.theme(EditorTheme::dark())` to keep the
previous always-dark editor.

### Changed — demo uses the framework's ThemeMode
The IDE demo no longer pins `Theme::dark()` or keeps its own editor-only "light" flag: the
window follows the system setting, and the Settings switch / View menu "Light theme" set
the app's `ThemeMode`, so chrome and editor switch together. Text takes the theme's
ambient colour instead of hard-coded `.color(foreground)`. `SHOT=` pins dark so docs
screenshots are unchanged.

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
