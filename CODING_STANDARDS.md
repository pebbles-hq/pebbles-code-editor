# Coding Standards — Pebbles, pebbles-code-editor, Gravel

**Status: MANDATORY.** Every agent and human working on these repos must follow this document. A change that breaks a **MUST** rule is not done, even if it works and its feature tests pass. Reviewers reject it.

Why it exists: on 2026-11-06/07 the release build of Gravel was slower than VS Code. Each rule below comes from a real bug we found and fixed:
- whole-window rebuilds of 53 ms;
- frames drawn while idle, up to 80 a second;
- a `git` process started on every caret move;
- every extension manifest re-parsed from disk for each explorer row;
- a leak on every file open and close;
- 20,000 folders watched;
- three copies of the icon theme.

The bar is **faster than VS Code and Zed, with far less memory**. Gravel is a Rust app; being slow or memory-hungry is a defect.

Words: **MUST / MUST NOT** = hard rule. **SHOULD** = default; deviate only with a written reason in the commit message.

---

## 0. The budgets (what "performant" means)

Measured in **release** builds. CPU-side numbers come from `tests/perf_budget.rs` (Xvfb + lavapipe); memory comes from the owner's real GPU. A change MUST NOT push any of these over budget:

| Metric | Budget | Now (2026-11-07) |
|---|---|---|
| Keystroke → paint, p95 (5k-line file) | **< 8 ms** | ~7 ms |
| Scroll frame, p99 | **< 4 ms** | ~3 ms |
| Hover / pointer move frame | **< 2 ms** | ~1 ms |
| Open a 5k-line file → first paint | **< 30 ms** | 25 ms |
| Gravel UI-thread work from launch to settled | **< 200 ms** | ~65 ms |
| Frames drawn while idle | **0** | 0 |
| Gravel-thread wakeups while idle (60 s) | **0** | 0 |
| Memory growth per open/close cycle | **< 100 B** | ~50 B |
| Interactive soak growth (memory_soak) | **< 3 MB** | < 1 MB |
| Any single UI-thread task | **< 8 ms** (PEBBLES_DEV warns) | — |
| New feature's idle memory once used | SHOULD be **< 5 MB**, and released when its view closes | — |

Every new feature MUST state its effect on these in its commit message. "No change" counts as a statement.

---

## 1. The render / build path is sacred

Component bodies (`#[component]` functions and anything they call while building) run on the UI thread, sometimes many times per second.

1.1 **MUST NOT** do any of these in a build/render/paint path, including helpers called from one:
- file-system calls of any kind (`fs::*`, `read_dir`, `metadata`, `exists`, canonicalize);
- starting a process (`Command::new`, `git`, `docker`);
- parsing (JSON, TOML, YAML, manifests, grammars, tree-sitter queries);
- reading settings files;
- network access;
- locking a mutex another thread holds for long;
- sleeping.

> Bugs this prevented: `git rev-parse` on every caret move; every extension manifest parsed per explorer row; `.editorconfig` and breadcrumb folder reads on every render.

1.2 **MUST** compute expensive values once and cache them, keyed by what they depend on, and invalidate them on the event that changes them: a file-watch event, a settings change, an install or uninstall. Never cache them "per render".

1.3 **MUST NOT** clone large data per render: document text, file lists, JSON trees, theme or icon maps, git status. Share it via `Rc`/`Arc` and hand out references or cheap handles.

> Bug: each tab and Quick Open icon lookup copied the whole Material icon theme.

1.4 **MUST NOT** allocate per row per frame in lists: no `format!`, `String`, `Vec` or closure-heavy menu building for every visible row on every frame. Build strings when the data changes, not when it's drawn.

1.5 **MUST NOT** build a context menu (or any popup content) until it opens. Rows hold a lightweight "on right-click" handler; the menu items are created in that handler.

> Bug: the gutter built 44 per-line menus per keystroke and scroll frame; each explorer row built its full menu per render.

1.6 Something that is only *displayed* MUST NOT trigger work elsewhere. Rendering a status, label or icon is read-only.

---

## 2. Reactivity: update only what changed

Pebbles uses fine-grained signals (SolidJS-style) inside Flutter-style components. A signal read inside a component subscribes **that component** to it. Read signals at the lowest component that needs them.

2.1 **MUST** read a signal in the smallest component that displays it. Reading `caret`, `scroll`, `hover`, `window size` or `git status` high up (the workbench, the editor pane, a list) re-renders everything under it.

> Bug: the workbench regions were plain function calls, so opening a file rebuilt the whole window: 53 ms for 448 objects.

2.2 **MUST** split big views into components with clear props, and wrap stable regions with `component_memo` (it skips re-rendering when props are equal, like React's `memo`). Props MUST be cheap to compare: ids, `Rc` handles, small values. Never a whole `Vec` that is rebuilt each time.

2.3 Lists **MUST** use keyed, memoized row components, so that one changed row re-renders one row. A hover on one row MUST NOT re-render its siblings or its parent subtree.

2.4 **MUST NOT** subscribe to window size just to read the device pixel ratio. Use `device_pixel_ratio()`, not `media_query()`.

> Bug: every icon re-rendered on window changes.

2.5 Bookkeeping writes that no view displays (counters, last-seen markers, caches) **MUST** use `Signal::set_untracked` or plain fields, so they don't schedule re-renders.

2.6 Effects **MUST** have narrow dependencies. Use `untrack` for values read but not depended on. An effect MUST NOT write a signal it also reads, which causes loops.

2.7 Derived values **SHOULD** be memos (`create_memo`), not recomputed in each consumer.

2.8 With `PEBBLES_DEV=1`, no component may re-render on 120 consecutive frames. Pebbles warns; fix the cause.

---

## 3. Threads, async work and the UI thread

3.1 Any work that can take more than ~1 ms, or touches disk, processes or network, **MUST** run off the UI thread on Pebbles' **shared worker pool** (`spawn` / `spawn_after`). Results come back to signals.

3.2 **MUST NOT** call `std::thread::spawn` per task, per tick or per item. Long-lived dedicated threads are allowed only for blocking streams (a PTY reader, an inotify fd, a `docker events` / `docker logs` reader), MUST be named, and MUST exit when their owner is dropped.

> Bug: the git watcher started a new thread every 300 ms.

3.3 **MUST NOT** sleep inside a pool task to wait. Use `spawn_after(delay, …)`.

3.4 Heavy one-off work on open MUST be deferred or moved off-thread: tree-sitter query compilation, first full parse of large files, theme and icon parsing. Show the content first (e.g. unhighlighted), then refine.

> Bug: compiling the Rust query (55 ms) plus the first parse (52 ms) on the UI thread when a file opened.

3.5 Results from a stale request **MUST** be dropped: version-stamp requests, so that an old parse, git status or docker list can't overwrite a newer one.

> Bug: an out-of-order git status read caused the flaky git_sidebar test, and was also a real UI bug.

3.6 Duplicate in-flight requests for the same thing **MUST** be coalesced into one.

> Bug: each file open ran `git status` ×2 plus `for-each-ref`; now it's a single `git show`.

---

## 4. Idle means idle

4.1 With nothing changing, the app **MUST** draw **0 frames** and Gravel's threads **MUST** have **0 wakeups**. Verify with `PEBBLES_TRACE_WAKE=1` and `/proc/<pid>/task/*/status` context-switch counts.

4.2 **MUST NOT** poll: no repeating timers that check for changes (files, git, docker, processes). Use OS notifications instead: inotify through `src/fswatch.rs`, an event stream like `docker events`, or a child process's output.

> Bug: a git watcher every 300 ms and an explorer poll every 2 s. Replacing them with inotify cut idle wakeups from 3,259 to 122 a minute.

4.3 Background task results **MUST NOT** force a frame unless they changed something visible.

4.4 Animations and blinking **MUST** use timers at their real period (`create_blink`: 500 ms), never a per-frame loop.

> Bug: the caret blink drew ~80 frames a second.

4.5 Watchers and streams for a view **MUST** stop when the view is hidden or closed, and start again lazily when it's shown.

4.6 File watching **MUST** skip what doesn't matter: git-ignored folders, `target/`, `node_modules/`, `.git/objects/`, `vendor/`. Store paths once, shared.

> Bug: 20,000 watched folders, each path held three times.

---

## 5. Memory

5.1 **Lazy by default.** Nothing feature-specific loads at startup. It loads on first use: the Quick Open file index, grammars, the extension host / wasmtime, icon themes beyond the active one, Docker data, Git graph data.

> Bug: a 50,000-file index built at folder open (now built on first Quick Open).

5.2 Every cache, log, history and stream buffer **MUST** be bounded: a size cap or LRU, plus eviction. State the bound in a comment. Examples:
- text shape/layout caches: evict from 1,024 entries, hard cap 2,048;
- git/docker output logs: 5,000 lines;
- terminal scrollback: capped;
- log tabs: capped.

> Bug: text caches grew to 32k entries per map, ~40 MB after one scroll-through.

5.3 Per-document and per-view state **MUST** be freed when the document or view closes: signals, caches, watchers, LSP and extension sync maps, decorations. Add a regression check to `memory_soak`.

> Bug: every closed file left ~20 signals plus a full copy of its text behind.

5.4 Cleanups **MUST** be registered on **every** render with `create_cleanup`, because the runtime drops a component's cleanups each time it re-renders. Registering only on the first render leaks.

> Bug: `render_drag_target` leaked ~750 B per re-render.

5.5 Parse, convert, then drop. **MUST NOT** keep a parsed `serde_json::Value` or manifest tree once the compact structure is built. Keep compact maps and interned strings, and `shrink_to_fit` retained strings and vecs.

> Bug: the icon theme JSON was held three times.

5.6 **MUST NOT** add heavy dependencies without a measured reason and the owner's OK: HTTP clients, async runtimes like tokio, big UI or parsing crates. Prefer the system CLI (`git`, `docker`) plus a light parser, and reuse crates already in `Cargo.lock`. A dependency in a hot path or at startup must be justified with numbers.

5.7 GPU resources (atlases, render targets) **SHOULD** start small and grow on demand.

---

## 6. External tools (git, docker, language servers, debug adapters, CLIs)

6.1 Run them as subprocesses on a worker, never on the UI thread. Use machine-readable output (`--format '{{json .}}'`, `--porcelain=v2 -z`, `--json`) and parse it on the worker.

6.2 Each call **MUST** be cheap, specific and coalesced. Ask for exactly what the view needs: one repo, one file, one container. Never "refresh everything" on a small event.

6.3 Long-running output (logs, builds, pulls, `events`) **MUST** be streamed. Batch it into ~8 ms chunks before updating the UI (the terminal's pattern), cap what's kept, and kill the process when the consumer goes away.

6.4 Errors surface as a toast plus an output-channel line, never a panic. Missing tools give a friendly empty state, and Gravel **MUST NOT** install anything without the owner's explicit yes.

6.5 Credentials **MUST NOT** be stored or logged. Logins happen in a terminal tab, typed by the user.

---

## 7. Docker view — specific bar

On top of everything above (see `gravel-docker-requirements.md`):
- **No `docker` call before the Docker view is first opened.** Startup cost: 0.
- Live updates come from **one** `docker events` stream, only while the view is visible. No polling, no refresh timers.
- Lists: keyed and memoized rows, grouped by compose project; one event updates one row or one list, never the whole view.
- Log tabs follow with batched appends and a capped buffer; closing the tab kills `docker logs`.
- Inspect output is pretty-printed once, on the worker, into a read-only tab.
- Hidden view: zero processes, zero threads, zero wakeups. Closing the view frees its lists.
- **Required tests:** a fake `docker` on PATH (including a fake `events` stream), plus perf/idle assertions in `perf_budget` (opening the Docker view stays within the hover/open budgets; hidden-view idle has 0 wakeups) and a memory_soak phase (open and close the Docker view and log tabs repeatedly; growth under budget).

---

## 8. Tests, measurement and proof

8.1 Measure in **release**. Debug-build timings don't count.

8.2 Every performance-relevant change **MUST** keep these green, run in release:
- `cargo test --release --test perf_budget`
- `cargo test --release --test memory_soak`

New features add their own budget assertions there: render counts, frame times, idle wakeups, memory growth.

8.3 Use the tools. Don't guess:

| Variable | What it does |
|---|---|
| `PEBBLES_DEV=1` | Warnings for frames over budget, the UI thread blocked over 8 ms, and every-frame re-renders |
| `PEBBLES_REBUILD_PROFILE=1` | The costliest components per frame, with source lines |
| `PEBBLES_FRAME_JSON=<file>` | A per-frame timeline: rebuild, layout, encode and GPU timings |
| `PEBBLES_TRACE_WAKE=1` | What woke an idle window |
| `PEBBLES_MEM_TRACE=1` | Memory by subsystem, plus a runtime census of signals, timers and tasks |
| `GRAVEL_STARTUP_TRACE=1` | Start-up timestamps |
| `dhat-heap` (Gravel feature) | A heap profile |
| `PERF_BUDGET_JSON=<file>` | Benchmark output as JSON |

8.4 Commit messages for features **MUST** include before/after numbers for the budgets they touch, or say "no measurable change: <how checked>".

8.5 Any manual run of the Gravel binary **MUST** be memory-capped:
`systemd-run --user --scope -p MemoryMax=1500M -p MemorySwapMax=0 …`
Run it on Xvfb + lavapipe, never on the owner's display. Tests **MUST NOT** touch the real `~/.gravel` (`src/home.rs` enforces this).

8.6 Run only the affected test files plus `perf_budget` and `memory_soak`. Never the full suite unless the owner asks. All cargo goes through `~/.cache/phasec/safe-cargo`.

---

## 9. Code-review checklist (paste into the commit/PR and tick)

```
Perf/memory checklist (documentations/coding-standards.md)
[ ] No IO / process spawn / parsing / settings read in any render path (§1.1)
[ ] Expensive values cached + invalidated by events, not per render (§1.2)
[ ] No large clones or per-row allocations per frame; menus built on open (§1.3–1.5)
[ ] Signals read in the smallest component; regions memoized; rows keyed (§2.1–2.3)
[ ] Bookkeeping writes untracked; effects have narrow deps (§2.5–2.6)
[ ] Slow work on the worker pool; no per-task threads; no sleeps; stale results dropped; duplicates coalesced (§3)
[ ] Idle: 0 frames, 0 wakeups; no polling; watchers stop when hidden (§4)
[ ] Lazy loading; every cache/log/buffer bounded; state freed on close; cleanups registered every render (§5)
[ ] No new heavy dependency (or owner OK + numbers) (§5.6)
[ ] External tools: worker, JSON output, specific, streamed + batched + capped (§6)
[ ] perf_budget + memory_soak green in release; numbers in the commit message (§8)
```

A change that can't tick a box MUST explain why in the commit message, or it isn't done.

---

## 10. General quality (non-performance)

- Match the surrounding code: naming, comment density, idioms. Follow industry semantics (Flutter/Skia/CSS for Pebbles; VS Code for Gravel UX).
- Fix framework bugs **upstream** in Pebbles or the editor, not with app-side workarounds.
- No dead UI: every button, menu item and command must work, with a test that clicks it through real hit-testing. Remove what doesn't work.
- Tests use real pointer and key events, temp repos and workspaces, and fakes for external tools. Never the network.
- `cargo fmt` clean, no new warnings.
- Commit as `Reyco Seguma <segumareyco@gmail.com>`. Push as xreyc and verify with an authenticated `git ls-remote`. See `AGENTS.md`.
