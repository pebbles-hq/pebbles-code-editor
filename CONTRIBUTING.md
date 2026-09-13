# Contributing to pebbles-code-editor

Thanks for helping! This crate is the embeddable code-editor widget for the
[Pebbles](https://github.com/pebbles-hq/pebbles) GUI framework — a custom editing engine you
can drop into an app and grow an IDE on. It's organized by concept so you can find things by
name and land changes with confidence.

## Setup

You need a recent stable Rust (edition 2024; `rust-version = 1.90`). The crate depends on
`pebbles` as a git dependency, so the first build fetches and compiles the framework (and its
`wgpu`/`vello` stack) — expect a few minutes the first time, fast after that.

```sh
git clone https://github.com/pebbles-hq/pebbles-code-editor
cd pebbles-code-editor
cargo build
cargo run -p demo   # the full IDE demo
```

## Where the code lives

`lib.rs` is a thin hub: crate docs, the module tree, and the public re-exports. See
[`ARCHITECTURE.md`](ARCHITECTURE.md) for the full map and the layering rules. The essentials:

| Path | What's in it |
|---|---|
| `src/edit.rs` | The text model: rope document, invertible `ChangeSet`/`Transaction`, `Selections`, undo `History` — the single mutation path. No widgets. |
| `src/commands.rs` | The controller: `KeyInput` → `Transaction` → `dispatch` (multi-cursor edits, motions, auto-close, comment toggle, indent). Owns no widgets. |
| `src/geometry.rs` `brackets.rs` `highlight.rs` | Pure helpers: byte↔line/col + grid math, bracket matching, tokens→themed spans. |
| `src/lang/` | The `Language` trait + `Token`/`TokenKind` and the bundled grammars (one scanner per module). Panic-free on any input. |
| `src/providers.rs` | IntelliSense data types + the provider callback signatures (completion/hover/signature/diagnostics/inlay/definition/format). |
| `src/fold.rs` | The visual-row `DisplayMap` behind **folding + soft wrap** (and block-widget gap rows). Pure. |
| `src/diff.rs` `src/collab.rs` | LCS line diff for `.diff_base`; the `Edit` delta + remap for `.on_edit`. Pure. |
| `src/theme.rs` | `EditorTheme` (chrome slots) + separable `HighlightStyle`/`TokenStyle`. |
| `src/extensions.rs` | The `Extension` plugin model (decorations, gutter, block widgets, commands, keymap, config facets, theme, hooks). |
| `src/config.rs` | The public `code_editor()` builder + `CodeEditor` config + resolved `Props`. The front door. |
| `src/view/` | The only place that builds widgets: `render_editor` orchestration + `overlays`/`gutter`/`minimap`/`sticky`/`completion`/`search`/`extensions`/`chrome`. |

## Conventions

- **One concern per file.** If a file starts mixing concerns, split it.
- **Pure where possible.** `edit`/`geometry`/`highlight`/`lang`/`fold`/`diff`/`collab` are pure
  functions of text — unit-test them next to the code.
- **The view owns widgets; nothing else does.** Everything else returns data.
- **Never panic on bad input.** The buffer is always full of half-typed, invalid source.
- **Signals live in `render_editor`.** Any per-widget local state must be its own
  `component_props` — never a `create_signal` in a plain helper (it churns the parent's hook
  order). See `ARCHITECTURE.md`.
- **Every user-facing feature must be shown in `examples/demo`** (a toggle, a sample provider, a
  plugin) — a feature that isn't in the demo isn't done.

## Before you push

The same gates CI runs:

```sh
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace
```

A good change also adds/updates: a test (`tests/editing.rs` for behavior, `tests/robustness.rs`
for edge cases, or a `#[cfg(test)]` unit test for pure modules), the demo, and `CHANGELOG.md`.
