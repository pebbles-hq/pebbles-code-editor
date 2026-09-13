//! The **view** domain: the widget tree the editor renders each frame.
//!
//! [`render_editor`] is the component. It owns the reactive state (document, history, scroll,
//! blink, token cache), computes the read-model + the virtualized visible-line window into a
//! [`Frame`], then assembles the layers and panels. Each panel is split by concern into a
//! sibling module — [`overlays`] (the grid layers), [`gutter`], [`minimap`], [`sticky`] — and
//! shared chrome helpers live in [`chrome`]. This module keeps only the orchestration:
//! reactive setup, the read-model, the mouse gestures, and the final assembly.

mod chrome;
mod completion;
mod extensions;
mod gutter;
mod minimap;
mod overlays;
mod search;
mod sticky;

use std::cell::RefCell;
use std::rc::Rc;

use pebbles::prelude::*;
use pebbles::render::ScrollHandle;
use ropey::Rope;

use crate::brackets::DEFAULT_BRACKETS;
use crate::commands::{EditCfg, apply_key, dispatch};
use crate::config::Props;
use crate::edit::{ChangeSet, Coalesce, EditorState, History, Selection, Selections, Transaction};
use crate::extensions::{Decoration, GutterMark, Snapshot};
use crate::geometry::*;
use crate::highlight::merge_tokens;
use crate::lang::Token;
use crate::theme::EditorTheme;

use chrome::status_bar;

/// The per-frame read-model shared by the view builders: geometry, the resolved document
/// snapshot, the selections, and the virtualized visible-line window. Pure values computed
/// once per render — no signals, so the builders are pure functions of it.
pub(crate) struct Frame<'a> {
    pub(crate) p: &'a Props,
    pub(crate) theme: &'a EditorTheme,
    pub(crate) src: &'a str,
    pub(crate) tokens: &'a [Token],
    pub(crate) sels: &'a Selections,
    /// Primary caret head byte, and whether the primary range has a selection.
    pub(crate) pcc: usize,
    pub(crate) has_primary_sel: bool,
    /// Primary caret line / column (for the current-line band + gutter active row).
    pub(crate) cl: usize,
    // grid geometry
    pub(crate) fs: f64,
    pub(crate) lh: f64,
    /// The configured monospace family + extra letter spacing (folded into `advance`).
    pub(crate) font_family: &'a str,
    pub(crate) letter_spacing: f64,
    pub(crate) line_px: f64,
    pub(crate) advance: f64,
    pub(crate) pad_l: f64,
    pub(crate) pad_t: f64,
    // document + window
    pub(crate) line_count: usize,
    pub(crate) content_h: f64,
    pub(crate) first_line: usize,
    pub(crate) last_line: usize,
    pub(crate) caret_on: bool,
    /// Whether the editor currently holds keyboard focus (drives active vs inactive selection).
    pub(crate) focused: bool,
    /// Highlight other occurrences of the selected word (off while the find bar is open).
    pub(crate) highlight_word_matches: bool,
    /// Gutter markers contributed by extensions (rendered as colored dots).
    pub(crate) ext_gutter_marks: &'a [GutterMark],
    /// The fold display-map (buffer line ↔ display row) + the foldable regions for the gutter.
    pub(crate) disp: &'a crate::fold::DisplayMap,
    pub(crate) fold_regions: &'a [(usize, usize)],
    pub(crate) folds: Signal<std::collections::BTreeSet<usize>>,
}

impl Frame<'_> {
    /// The (x, y) px of a buffer `(line, col)` in visual-row space — accounts for folding
    /// (collapsed rows) and soft wrap (a column past the wrap point drops to the next row).
    pub(crate) fn xy(&self, line: usize, col: usize) -> (f64, f64) {
        let (row, xcol) = self.disp.place(line, col);
        (self.pad_l + xcol as f64 * self.advance, self.pad_t + row as f64 * self.line_px)
    }
    /// The y (px) of a display row index.
    pub(crate) fn row_y(&self, row: usize) -> f64 {
        self.pad_t + row as f64 * self.line_px
    }
    /// The y (px) of a buffer line's FIRST visual row.
    pub(crate) fn y_of(&self, line: usize) -> f64 {
        self.xy(line, 0).1
    }
    /// The visual rows `(row_index, Row)` that make up `line` (one unless soft-wrapped).
    pub(crate) fn segments(&self, line: usize) -> Vec<(usize, crate::fold::Row)> {
        let mut out = Vec::new();
        let mut i = self.disp.place(line, 0).0;
        while i < self.disp.rows() {
            let r = self.disp.row_at(i);
            if r.line != line {
                break;
            }
            out.push((i, r));
            i += 1;
        }
        out
    }
    /// Whether `line` is inside the rendered window AND not collapsed by a fold.
    pub(crate) fn line_visible(&self, line: usize) -> bool {
        (self.first_line..=self.last_line).contains(&line) && !self.disp.is_hidden(line)
    }
}

/// The editor component: wires reactive state, computes the [`Frame`], and assembles the
/// gutter, scrolling content (with overlays), sticky headers, minimap, and chrome.
pub(crate) fn render_editor(p: &Props) -> AnyWidget {
    let code = p.code;
    // The document + selection live in one immutable-style state, mutated only by dispatching
    // transactions (see the `edit` module). `code` (the public `Signal<String>`) is kept in
    // sync so callers still bind to a plain string.
    let state = create_signal(EditorState::new(&code.peek()));
    let history = create_signal(Rc::new(RefCell::new(History::new())));
    let goal = create_signal(0usize); // preserved column for Up/Down
    let blink_stamp = create_signal(0.0_f64); // loop time of the last edit/move — caret solid then
    let drag_anchor = create_signal::<Option<Offset>>(None); // pointer-down pos for column select
    // Text drag-and-drop: the source range armed on a press inside the selection, the live
    // drop byte during the drag, and whether an actual drag (vs a click) happened.
    let text_drag = create_signal::<Option<(usize, usize)>>(None);
    let drop_byte = create_signal::<Option<usize>>(None);
    let dragged = create_signal(false);
    let scroll_top = create_signal(0.0_f64); // live vertical scroll offset (drives virtualization)
    // Tokenization cache: (source, tokens). Re-tokenizes only when the text changes, so
    // scrolling (which re-renders the window) never re-parses the document.
    #[allow(clippy::type_complexity)]
    let token_cache: Signal<Rc<RefCell<(String, Rc<Vec<Token>>)>>> =
        create_signal(Rc::new(RefCell::new((String::new(), Rc::new(Vec::new())))));
    // Imperative scroll handles — drive caret-into-view autoscroll (only used when a fixed
    // `height` makes the editor a scroll viewport). Held in signals so they survive renders.
    let scroll = create_signal(ScrollHandle::new()).peek(); // vertical
    let hscroll = create_signal(ScrollHandle::new()).peek(); // horizontal
    let completion = create_signal::<Option<completion::Session>>(None); // autocomplete popup
    let hover_pos = create_signal::<Option<Offset>>(None); // pointer pos for hover tooltip
    let sig_help = create_signal::<Option<crate::providers::SignatureHelp>>(None); // signature help
    let preedit = create_signal(String::new()); // IME composition (preedit) text, shown at caret
    // Fold state: use the caller-bound signal (persists across mounts / tabs) if given, else a
    // private one. `create_signal` runs unconditionally so the hook order never churns.
    let internal_folds = create_signal::<std::collections::BTreeSet<usize>>(std::collections::BTreeSet::new());
    let folds = p.folds.unwrap_or(internal_folds);
    // The live wrap-column width, shared with the (once-created) autoscroll effect via interior
    // mutability so it can map the caret to its display row without a stale capture.
    let wrap_hold = create_signal(Rc::new(std::cell::Cell::new(0usize)));
    // Find/replace state.
    let find = search::State {
        open: create_signal(0u8),
        query: create_signal(String::new()),
        replace: create_signal(String::new()),
        case: create_signal(false),
        word: create_signal(false),
        regex: create_signal(false),
        idx: create_signal(0usize),
    };
    let palette = extensions::Palette {
        open: create_signal(false),
        query: create_signal(String::new()),
        sel: create_signal(0usize),
    };
    let focus = create_focus();

    // Two-way binding + collaboration stream. On any `code` change: if it came from OUTSIDE
    // (a remote/loader edit — `code != state.text()`), rebuild the doc and REMAP the carets
    // through the delta so a collaborator's edit never yanks the local cursor; then report the
    // minimal `Edit` to `on_edit` (with `remote` set). Local keystrokes keep `code ==
    // state.text()`, so `remote` is false and no rebuild happens.
    let on_edit = p.on_edit.clone();
    let prev_code = create_signal(code.peek());
    create_effect(move || {
        let new = code.get();
        let old = prev_code.peek();
        if new == old {
            return;
        }
        let delta = crate::collab::diff(&old, &new);
        let remote = new != state.peek().text();
        if remote {
            let cur = state.peek();
            let sel = match &delta {
                Some(e) => {
                    let mapped: Vec<Selection> = cur
                        .selection
                        .ranges()
                        .iter()
                        .map(|r| {
                            Selection::range(crate::collab::map_pos(r.anchor, e), crate::collab::map_pos(r.head, e))
                        })
                        .collect();
                    Selections::new(mapped, cur.selection.primary_index()).clamped(new.len())
                }
                None => cur.selection.clamped(new.len()),
            };
            state.set(EditorState { doc: Rope::from_str(&new), selection: sel });
        }
        if let (Some(cb), Some(e)) = (&on_edit, &delta) {
            cb(e, remote);
        }
        prev_code.set(new);
    });

    // Grab focus on mount if requested — but only ONCE, so it never steals focus back from a
    // child input (the find bar's query field) on later re-renders. The hook is created
    // UNCONDITIONALLY (the condition lives inside the effect) so the hook order never churns.
    let did_autofocus = create_signal(false);
    let want_focus = p.autofocus;
    create_effect(move || {
        if want_focus && !did_autofocus.peek() {
            focus.request_focus();
            did_autofocus.set(true);
        }
    });

    // Restore a saved scroll offset once on mount (persist/restore view state). Again the
    // signal + effect are UNCONDITIONAL (gated inside), so a per-render change in
    // `initial_scroll` can't shift this component's hook positions.
    let did_restore = create_signal(false);
    let init_scroll = p.initial_scroll;
    let scroll_restore = scroll.clone();
    create_effect(move || {
        if init_scroll > 0.0 && !did_restore.peek() {
            scroll_restore.scroll_to(init_scroll);
            scroll_top.set(init_scroll);
            did_restore.set(true);
        }
    });

    // Register the key handler (semantic edit commands from the framework).
    let read_only = p.read_only;
    let indent_unit: String = if p.insert_spaces {
        " ".repeat(p.tab_size)
    } else {
        "\t".to_string()
    };
    // Language-derived editing config (owned, so it can travel into the 'static handler).
    let cfg = Rc::new(EditCfg {
        tab: indent_unit.clone(),
        line_comment: p.language.as_ref().and_then(|l| l.line_comment().map(String::from)),
        brackets: p
            .language
            .as_ref()
            .map(|l| l.brackets().to_vec())
            .unwrap_or_else(|| DEFAULT_BRACKETS.to_vec()),
        auto_close: p.auto_close,
    });
    {
        let cfg = cfg.clone();
        let provider = p.completion.clone();
        let definition = p.definition.clone();
        let format = p.format.clone();
        let sig_provider = p.signature.clone();
        let ro_exts = p.extensions.clone(); // for read-only-range veto
        // Register as a *code* editor so the shell routes Tab/Shift+Tab here (indent/outdent)
        // instead of moving focus. When the completion popup is open it intercepts navigation
        // keys; otherwise keys flow to the command engine, and typing re-queries completion.
        focus.register_code_editor(Rc::new(move |k: KeyInput| {
            // IME composition: a Preedit shows the in-progress (underlined) text at the caret;
            // it is NOT committed until an Insert (from Ime::Commit). Any other key ends the
            // composition (the commit Insert clears it here, then inserts the committed text).
            if let KeyInput::Preedit(s) = &k {
                preedit.set(s.clone());
                return;
            }
            if !preedit.peek().is_empty() {
                preedit.set(String::new());
            }
            if completion.peek().is_some() {
                match k {
                    KeyInput::Move { motion: Motion::Down, .. } => {
                        let mut s = completion.peek();
                        if let Some(sess) = &mut s {
                            sess.move_by(1);
                        }
                        completion.set(s);
                        return;
                    }
                    KeyInput::Move { motion: Motion::Up, .. } => {
                        let mut s = completion.peek();
                        if let Some(sess) = &mut s {
                            sess.move_by(-1);
                        }
                        completion.set(s);
                        return;
                    }
                    KeyInput::Enter | KeyInput::Indent => {
                        completion::accept(state, history, code, goal, completion);
                        return;
                    }
                    KeyInput::Escape => {
                        completion.set(None);
                        return;
                    }
                    _ => {}
                }
            }
            match &k {
                // Find / replace: open the bar (it autofocuses its query field). Escape (while
                // the editor is focused) closes it.
                KeyInput::Find => {
                    find.open.set(1);
                    // Release focus so the bar's query field (autofocus) can take it.
                    pebbles::core::focus::set_focus(None);
                    return;
                }
                KeyInput::Replace => {
                    find.open.set(2);
                    pebbles::core::focus::set_focus(None);
                    return;
                }
                KeyInput::Escape if find.open.peek() > 0 => {
                    find.open.set(0);
                    return;
                }
                KeyInput::CommandPalette => {
                    palette.open.set(true);
                    pebbles::core::focus::set_focus(None); // let the palette field autofocus
                    return;
                }
                KeyInput::Escape if palette.open.peek() => {
                    palette.open.set(false);
                    return;
                }
                KeyInput::TriggerCompletion => {
                    if let Some(pv) = &provider {
                        completion::trigger(state, completion, pv, true);
                    }
                    return;
                }
                // F12: jump the caret to the provider's target (autoscroll follows).
                KeyInput::GoToDefinition => {
                    if let Some(dp) = &definition {
                        let cur = state.peek();
                        if let Some(target) = dp(&cur.text(), cur.primary().head) {
                            state.set(EditorState {
                                doc: cur.doc.clone(),
                                selection: Selections::single(Selection::caret(target.min(cur.len()))),
                            });
                            history.peek().borrow_mut().break_group();
                        }
                    }
                    return;
                }
                // Shift+Alt+F: replace the document with the formatter's output (one undo step).
                KeyInput::Format => {
                    if let Some(fp) = &format {
                        let cur = state.peek();
                        let old = cur.text();
                        let new = fp(&old);
                        if new != old {
                            let caret = cur.primary().head.min(new.len());
                            dispatch(
                                state,
                                history,
                                code,
                                Transaction::change_and_select(
                                    ChangeSet::replace(0, old.len(), new),
                                    Selections::single(Selection::caret(caret)),
                                ),
                                Coalesce::Never,
                            );
                        }
                    }
                    return;
                }
                _ => {}
            }
            // Decide the completion follow-up from the key *before* it's consumed: typing an
            // identifier char re-queries; backspace re-queries only while open; anything else
            // dismisses.
            let follow = match &k {
                KeyInput::Insert(s)
                    if !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_') =>
                {
                    1
                }
                KeyInput::Backspace if completion.peek().is_some() => 1,
                _ => 0,
            };
            // Signature help follows the same "before consume" rule: `(`/`,` opens it, `)`/Esc
            // closes it. Evaluated before `k` is moved into the command engine.
            let sig_action = match &k {
                KeyInput::Insert(s) if s == "(" || s == "," => 1,
                KeyInput::Insert(s) if s == ")" => 2,
                KeyInput::Escape => 2,
                _ => 0,
            };
            // Read-only veto: if an editing key would touch a range an extension marked
            // read-only, drop it (checked against the primary selection/caret).
            let editing = matches!(
                &k,
                KeyInput::Insert(_)
                    | KeyInput::Backspace
                    | KeyInput::Delete
                    | KeyInput::DeleteWordBack
                    | KeyInput::DeleteWordForward
                    | KeyInput::Enter
                    | KeyInput::Indent
                    | KeyInput::Outdent
                    | KeyInput::Paste
                    | KeyInput::Cut
                    | KeyInput::ToggleComment
            );
            if editing && !ro_exts.is_empty() {
                let st = state.peek();
                let text = st.text();
                let pr = st.primary();
                let (a, b) = (pr.min(), pr.max());
                let snap = Snapshot { text: &text, caret: pr.head, selection: (a, b) };
                let blocked = ro_exts.iter().any(|e| {
                    e.read_only.as_ref().is_some_and(|f| {
                        f(&snap).iter().any(|&(lo, hi)| a <= hi && b >= lo)
                    })
                });
                if blocked {
                    return;
                }
            }
            // Transaction filter: let extensions transform or veto a single-caret insert (e.g.
            // expand tabs, upper-case on type). Multi-cursor edits bypass it.
            if let KeyInput::Insert(text) = &k
                && ro_exts.iter().any(|e| e.edit_filter.is_some())
            {
                let st = state.peek();
                let sels = st.selection.clamped(st.text().len());
                if sels.ranges().len() == 1 {
                    let src = st.text();
                    let pr = sels.primary();
                    let (a, b) = (pr.min(), pr.max());
                    let snap = Snapshot { text: &src, caret: pr.head, selection: (a, b) };
                    let mut edit = crate::collab::Edit { from: a, to: b, insert: text.clone() };
                    let mut vetoed = false;
                    for e in ro_exts.iter().filter_map(|e| e.edit_filter.as_ref()) {
                        match e(&snap, &edit) {
                            Some(new) => edit = new,
                            None => {
                                vetoed = true;
                                break;
                            }
                        }
                    }
                    if vetoed {
                        return;
                    }
                    if edit.from != a || edit.to != b || &edit.insert != text {
                        // The filter changed the edit — apply it directly (one undo step).
                        let caret = edit.from + edit.insert.len();
                        dispatch(
                            state,
                            history,
                            code,
                            Transaction::change_and_select(
                                ChangeSet::replace(edit.from, edit.to, edit.insert),
                                Selections::single(Selection::caret(caret)),
                            ),
                            Coalesce::Never,
                        );
                        goal.set(col_of(&state.peek().text(), state.peek().primary().head));
                        return;
                    }
                }
            }
            apply_key(k, state, history, code, goal, read_only, &cfg);
            if let Some(pv) = &provider {
                if follow == 1 {
                    completion::trigger(state, completion, pv, false);
                } else {
                    completion.set(None);
                }
            }
            match (sig_action, &sig_provider) {
                (1, Some(sp)) => {
                    let cur = state.peek();
                    sig_help.set(sp(&cur.text(), cur.primary().head));
                }
                (2, _) => sig_help.set(None),
                _ => {}
            }
        }));
    }

    // Extension event hooks (on_change / on_selection). The signal + effect are created
    // UNCONDITIONALLY (they no-op with no hooks) so toggling the extension set never churns
    // this component's hook order.
    let hook_exts = p.extensions.clone();
    let prev = create_signal(state.peek().text());
    create_effect(move || {
        let st = state.get();
        let text = st.text();
        let pr = st.primary();
        let snap = Snapshot {
            text: &text,
            caret: pr.head,
            selection: (pr.min(), pr.max()),
        };
        let changed = *prev.peek() != text;
        for e in &hook_exts {
            if changed && let Some(f) = &e.on_change {
                f(&snap);
            }
            if let Some(f) = &e.on_selection {
                f(&snap);
            }
        }
        if changed {
            prev.set(text);
        }
    });

    // Extension focus hook (unconditional, gated inside).
    let focus_exts = p.extensions.clone();
    create_effect(move || {
        let has = focus.is_focused();
        for e in &focus_exts {
            if let Some(f) = &e.on_focus {
                f(has);
            }
        }
    });

    // Extension keymap: bind each extension keybinding to its command while the editor is
    // focused (declined otherwise, so it falls through to other handlers / page scroll).
    for kb in p.extensions.iter().flat_map(|e| e.keys.iter()).cloned() {
        let key_exts = p.extensions.clone();
        create_shortcut_if(&kb.chord.clone(), move || {
            if !focus.is_focused() {
                return false;
            }
            let ctx = crate::extensions::EditContext { state, history, code, goal };
            crate::extensions::run_command_by_id(&key_exts, &kb.command, &ctx);
            true
        });
    }

    // ---- read model ----
    let st = state.get();
    let src = st.text();
    let fs = p.fs;
    let lh = p.lh;
    let line_px = fs * lh;
    // Letter spacing widens every cell; fold it into the advance so caret/click/selection math
    // stays exact and matches the rendered text (which gets the same letter_spacing).
    let advance = fs * p.advance_ratio + p.letter_spacing;
    let pad_l = 14.0;
    let pad_t = 10.0;
    let theme = &p.theme;

    // All cursors/selections; the primary drives the current-line band + status line.
    let sels = st.selection.clamped(src.len());
    let primary = sels.primary();
    let (pcc, has_primary_sel) = (primary.head, !primary.is_empty());
    let cl = line_of(&src, pcc);
    let cc = col_of(&src, pcc);
    let line_count = src.split('\n').count().max(1);
    let char_lens: Vec<usize> = src.split('\n').map(|l| l.chars().count()).collect();

    // ---- folding + soft wrap: buffer-line ↔ visual-row map ----
    // The whole view renders in VISUAL-ROW space (folded lines collapse, wrapped lines split).
    // Reading `folds` re-renders on fold/unfold. Soft wrap needs the measured content width; it
    // settles one frame after mount (use_bounds is a tracked signal), and is 0 until then.
    let bounds = use_bounds();
    let gutter_w_est = if p.gutter {
        line_count.to_string().len().max(2) as f64 * advance + 22.0 + 14.0
    } else {
        0.0
    };
    let wrap_cols = if p.soft_wrap && p.height.is_some() && bounds.width() > 1.0 {
        (((bounds.width() - gutter_w_est - pad_l * 2.0) / advance).floor() as isize).max(8) as usize
    } else {
        0
    };
    let fold_regions: std::rc::Rc<Vec<(usize, usize)>> = std::rc::Rc::new(crate::fold::foldable(&src));
    let disp = std::rc::Rc::new(crate::fold::DisplayMap::new(
        line_count,
        &folds.get(),
        &fold_regions,
        &char_lens,
        wrap_cols,
    ));
    let display_rows = disp.rows();
    let content_h = display_rows as f64 * line_px + pad_t * 2.0;
    wrap_hold.peek().set(wrap_cols); // publish to the autoscroll effect
    let focused = focus.is_focused();

    // ---- viewport virtualization (in display-row space) ----
    // With a fixed height we render only the rows in view (plus a small overscan), so a
    // 100k-line file costs the same as a screenful. `first_line`/`last_line` are the BUFFER
    // lines at the window edges; per-line loops iterate that buffer span and skip hidden lines.
    const OVERSCAN: usize = 4;
    let (first_line, last_line) = if let Some(h) = p.height {
        let top = scroll_top.get();
        let first_row =
            (((top - pad_t) / line_px).floor() as isize - OVERSCAN as isize).max(0) as usize;
        let rows = (h / line_px).ceil() as usize + OVERSCAN * 2;
        let last_row = (first_row + rows).min(display_rows.saturating_sub(1));
        (disp.line_at_row(first_row), disp.line_at_row(last_row))
    } else {
        (0, line_count - 1)
    };

    // Pointer → byte, fold/wrap-aware: the y maps to a VISUAL row, then to its buffer line +
    // column span. When nothing is folded or wrapped this is the plain identity mapping (fast
    // path). Copy closure (captures only signals + f64s), reused by every pointer handler.
    let p2b = move |text: &str, pos: Offset| -> usize {
        let row = (((pos.y - pad_t) / line_px).floor().max(0.0)) as usize;
        let xcol = (((pos.x - pad_l) / advance).round().max(0.0)) as usize;
        let nlines = text.split('\n').count().max(1);
        let fset = folds.peek();
        if wrap_cols == 0 && fset.is_empty() {
            return byte_at(text, row.min(nlines - 1), xcol);
        }
        let clens: Vec<usize> = text.split('\n').map(|l| l.chars().count()).collect();
        let regions = crate::fold::foldable(text);
        let map = crate::fold::DisplayMap::new(nlines, &fset, &regions, &clens, wrap_cols);
        let r = map.row_at(row);
        let col = (r.start + xcol).min(clens.get(r.line).copied().unwrap_or(0));
        byte_at(text, r.line, col)
    };

    // ---- caret blink ----
    // A 0.5s phase that ticks ONLY while focused, reset to solid on any edit or caret move
    // (the effect below), so the caret is steady while you work.
    let blink_loop = create_loop_while(focused, 0.5);
    create_effect(move || {
        let _ = state.get();
        blink_stamp.set(blink_loop.peek());
    });
    let caret_on = focused && (blink_loop.get() - blink_stamp.get()).rem_euclid(1.0) < 0.5;

    // ---- autoscroll ----
    // Keep the primary caret in view on every edit/move (only when a fixed height makes the
    // editor a scroll viewport). `scroll_top` (the signal the scroll view feeds) is the
    // source of truth; we set it optimistically so the virtualized window follows now, then
    // command the scroll view to match. Only moves when the caret is off-screen.
    let scroll_a = scroll.clone();
    let hscroll_a = hscroll.clone();
    if let Some(vh) = p.height {
        create_effect(move || {
            let st = state.get();
            let text = st.text();
            let head = st.primary().head;
            let line = line_of(&text, head);
            let col = col_of(&text, head);
            // The caret's DISPLAY row (fold/wrap-aware), so autoscroll targets the right place.
            let wc = wrap_hold.peek().get();
            let drow = if wc == 0 && folds.peek().is_empty() {
                line
            } else {
                let clens: Vec<usize> = text.split('\n').map(|l| l.chars().count()).collect();
                let regions = crate::fold::foldable(&text);
                let map = crate::fold::DisplayMap::new(
                    clens.len().max(1),
                    &folds.peek(),
                    &regions,
                    &clens,
                    wc,
                );
                map.place(line, col).0
            };
            let top = pad_t + drow as f64 * line_px;
            let bottom = top + line_px;
            let cur = scroll_top.peek();
            let margin = line_px;
            let target = if top < cur + margin {
                Some((top - margin).max(0.0))
            } else if bottom > cur + vh - margin {
                Some((bottom - vh + margin).max(0.0))
            } else {
                None
            };
            if let Some(t) = target {
                scroll_a.scroll_to(t);
                scroll_top.set(t.min((content_h - vh).max(0.0)));
            }
            let x = pad_l + col_of(&text, head) as f64 * advance;
            hscroll_a.ensure_visible(x, x + advance, advance * 4.0);
        });
    }

    // ---- tokens (cached lexical + semantic overlay) ----
    // Large-file mode: past a line threshold, tokenize ONLY the visible window (lazily, each
    // render) and rebase to absolute offsets, instead of the whole document — so a 100k-line
    // file re-highlights in screenful-sized work per edit/scroll. Tradeoff: a multi-line
    // construct (string/comment) that opens above the window may mis-color at the top edge.
    const LARGE_FILE_LINES: usize = 5000;
    let tokens: Rc<Vec<Token>> = if line_count > LARGE_FILE_LINES {
        let ls = line_start_of(&src, first_line);
        let le = line_end(&src, line_start_of(&src, last_line));
        let win = &src[ls..le];
        let toks: Vec<Token> = p
            .language
            .as_ref()
            .map(|l| l.highlight(win))
            .unwrap_or_default()
            .into_iter()
            .map(|t| Token { start: t.start + ls, ..t })
            .collect();
        Rc::new(toks)
    } else {
        let cell = token_cache.peek();
        let mut c = cell.borrow_mut();
        if c.0 != src {
            let toks = Rc::new(
                p.language
                    .as_ref()
                    .map(|l| l.highlight(&src))
                    .unwrap_or_default(),
            );
            *c = (src.clone(), toks);
        }
        c.1.clone()
    };
    let tokens: Rc<Vec<Token>> = match p.semantic {
        Some(sig) => {
            let sem = sig.get();
            if sem.is_empty() {
                tokens
            } else {
                Rc::new(merge_tokens(&tokens, &sem))
            }
        }
        None => tokens,
    };

    // ---- extension contributions for this frame ----
    // Decorations + gutter markers are recomputed from the current snapshot each render.
    let (ext_decos, ext_gutter_marks): (Vec<Decoration>, Vec<GutterMark>) = if p.extensions.is_empty()
    {
        (Vec::new(), Vec::new())
    } else {
        let snap = Snapshot {
            text: &src,
            caret: pcc,
            selection: (primary.min(), primary.max()),
        };
        let decos = p
            .extensions
            .iter()
            .filter_map(|e| e.decorations.as_ref())
            .flat_map(|f| f(&snap))
            .collect();
        let marks = p
            .extensions
            .iter()
            .filter_map(|e| e.gutter.as_ref())
            .flat_map(|f| f(&snap))
            .collect();
        (decos, marks)
    };

    // ---- the read-model the view builders share ----
    let frame = Frame {
        p,
        theme,
        src: &src,
        tokens: &tokens,
        sels: &sels,
        pcc,
        has_primary_sel,
        cl,
        fs,
        lh,
        font_family: &p.font_family,
        letter_spacing: p.letter_spacing,
        line_px,
        advance,
        pad_l,
        pad_t,
        line_count,
        content_h,
        first_line,
        last_line,
        caret_on,
        focused,
        highlight_word_matches: find.open.get() == 0,
        ext_gutter_marks: &ext_gutter_marks,
        disp: disp.as_ref(),
        fold_regions: fold_regions.as_slice(),
        folds,
    };

    // Overlay layers on the monospace grid (current-line, rulers, guides, selection, bracket
    // match, text, whitespace, carets), plus the completion popup when open.
    // Find matches for the current query/options (only while the bar is open) — highlighted
    // below and driven by the find bar's navigation/replace actions.
    let search_matches: Vec<(usize, usize)> = if find.open.get() > 0 {
        // Read the option signals so a toggle re-runs the search.
        let (q, _, _, _) = (find.query.get(), find.case.get(), find.word.get(), find.regex.get());
        if q.is_empty() {
            Vec::new()
        } else {
            crate::search::find_all(&src, &q, &find.options())
        }
    } else {
        Vec::new()
    };

    let mut layers = overlays::build(&frame);
    if !ext_decos.is_empty() {
        layers.extend(extensions::decoration_layers(&frame, &ext_decos));
    }
    if !search_matches.is_empty() {
        let cur = find.idx.peek().min(search_matches.len() - 1);
        layers.extend(search::match_layers(&frame, &search_matches, cur));
    }
    if let Some(sess) = completion.peek() {
        layers.push(completion::popup(&sess, &frame));
    }
    // Hover tooltip: query the provider at the pointer's byte and float a panel there.
    if let (Some(pos), Some(hp)) = (hover_pos.get(), &p.hover) {
        let byte = p2b(&src, pos);
        if let Some(h) = hp(&src, byte) {
            let tip = chrome::tooltip(
                text(h.contents)
                    .size((fs * 0.92) as f32)
                    .font_family(&p.font_family)
                    .color(theme.foreground)
                    .into_widget(),
                theme,
            );
            layers.push(
                Positioned::new(tip)
                    .left(pos.x + 4.0)
                    .top(pos.y + 18.0)
                    .into_widget(),
            );
        }
    }
    // Signature help: a panel just above the caret line while typing a call.
    if let Some(sig) = sig_help.get() {
        let tip = chrome::tooltip(
            text(sig.label.clone())
                .size((fs * 0.92) as f32)
                .font_family(&p.font_family)
                .color(theme.foreground)
                .into_widget(),
            theme,
        );
        let y = (frame.y_of(cl) - line_px - 4.0).max(0.0);
        layers.push(
            Positioned::new(tip)
                .left(pad_l + cc as f64 * advance)
                .top(y)
                .into_widget(),
        );
    }
    // IME composition: the in-progress preedit text, tinted + underlined at the caret.
    let pre = preedit.get();
    if !pre.is_empty() {
        let x = pad_l + cc as f64 * advance;
        let y = frame.y_of(cl);
        let w = (pre.chars().count() as f64 * advance).max(2.0);
        layers.push(
            Positioned::new(
                container()
                    .width(w)
                    .height(line_px)
                    .decoration(BoxDecoration::new().color(theme.selection)),
            )
            .left(x)
            .top(y)
            .into_widget(),
        );
        layers.push(
            Positioned::new(
                text(pre)
                    .size(fs as f32)
                    .line_height(lh as f32)
                    .font_family(&p.font_family)
                    .color(theme.foreground)
                    .underline(),
            )
            .left(x)
            .top(y)
            .into_widget(),
        );
    }
    // Touch selection handles: draggable knobs at the selection ends (opt-in). Dragging a
    // handle moves that endpoint (the other stays anchored).
    if p.selection_handles && has_primary_sel {
        let (lo, hi) = (primary.min(), primary.max());
        for (byte, is_end) in [(lo, false), (hi, true)] {
            let ln = line_of(&src, byte);
            if !frame.line_visible(ln) {
                continue;
            }
            let hx = pad_l + col_of(&src, byte) as f64 * advance - 7.0;
            let hy = frame.y_of(ln) + line_px - 3.0;
            // The OTHER endpoint stays fixed while this handle drags.
            let fixed = if is_end { lo } else { hi };
            let handle = GestureDetector::new(
                container()
                    .width(14.0)
                    .height(14.0)
                    .decoration(
                        BoxDecoration::new().color(theme.caret).radius(BorderRadius::all(7.0)),
                    ),
            )
            .cursor(Cursor::Pointer)
            .on_pan_update(action_event(move |e| {
                let cur = state.peek();
                let text = cur.text();
                let b = p2b(&text, e.position);
                state.set(EditorState {
                    doc: cur.doc.clone(),
                    selection: Selections::single(Selection::range(fixed, b)),
                });
                goal.set(col_of(&text, b));
            }));
            layers.push(Positioned::new(handle).left(hx).top(hy).into_widget());
        }
    }
    // Text drag-and-drop: a drop caret at the target byte while dragging a selection.
    if let Some(d) = drop_byte.get() {
        let d = d.min(src.len());
        let x = pad_l + col_of(&src, d) as f64 * advance;
        let y = frame.y_of(line_of(&src, d));
        layers.push(
            Positioned::new(
                container()
                    .width(2.0)
                    .height(line_px)
                    .decoration(BoxDecoration::new().color(theme.caret)),
            )
            .left(x)
            .top(y)
            .into_widget(),
        );
    }
    let grid = stack(layers)
        .fit(StackFit::Expand)
        .alignment(Alignment::TOP_LEFT);

    // ---- mouse: click to place caret, drag to select, Alt-click to add a cursor ----
    let hit = move |pos: Offset, extend: bool, add: bool| {
        let cur = state.peek();
        let text = cur.text();
        let b = p2b(&text, pos);
        let next = if add {
            cur.selection.pushed(Selection::caret(b))
        } else if extend {
            let anchor = cur.selection.primary().anchor.min(text.len());
            Selections::single(Selection::range(anchor, b))
        } else {
            Selections::single(Selection::caret(b))
        };
        state.set(EditorState {
            doc: cur.doc.clone(),
            selection: next,
        });
        goal.set(col_of(&text, b));
        // A click/drag ends any typing run, so the next keystroke starts a new undo step.
        history.peek().borrow_mut().break_group();
    };
    // If `pos` falls inside the primary (non-empty) selection, return that range — used to arm
    // a text drag-and-drop instead of starting a new selection.
    let sel_range_at = move |pos: Offset| -> Option<(usize, usize)> {
        let cur = state.peek();
        let pr = cur.selection.primary();
        let (a, b) = (pr.min(), pr.max());
        if a == b {
            return None;
        }
        let text = cur.text();
        let byte = p2b(&text, pos);
        (byte >= a && byte <= b).then_some((a, b))
    };
    // Drop the dragged range `(a,b)` at byte `dst` — move it (or copy with Ctrl held), as one
    // undo step, leaving the moved text selected. No-op if dropped inside the source.
    let drop_text = move |a: usize, b: usize, dst: usize, copy: bool| {
        let cur = state.peek();
        let text = cur.text();
        if b > text.len() || a >= b {
            return;
        }
        let slice = text[a..b].to_string();
        let (new, cs, ce) = if copy {
            let mut s = String::with_capacity(text.len() + slice.len());
            s.push_str(&text[..dst]);
            s.push_str(&slice);
            s.push_str(&text[dst..]);
            (s, dst, dst + slice.len())
        } else {
            if dst > a && dst < b {
                return; // dropped inside the source — nothing to do
            }
            let without = format!("{}{}", &text[..a], &text[b..]);
            let adj = if dst >= b { dst - (b - a) } else { dst };
            let mut s = String::with_capacity(text.len());
            s.push_str(&without[..adj]);
            s.push_str(&slice);
            s.push_str(&without[adj..]);
            (s, adj, adj + slice.len())
        };
        dispatch(
            state,
            history,
            code,
            Transaction::change_and_select(
                ChangeSet::replace(0, text.len(), new),
                Selections::single(Selection::range(cs, ce)),
            ),
            Coalesce::Never,
        );
        goal.set(col_of(&state.peek().text(), state.peek().primary().head));
    };
    // Double-click selects the word under the pointer (new single selection).
    let word_select = move |pos: Offset| {
        let cur = state.peek();
        let text = cur.text();
        let b = p2b(&text, pos);
        let (s, e) = word_at(&text, b);
        state.set(EditorState {
            doc: cur.doc.clone(),
            selection: Selections::single(Selection::range(s, e)),
        });
        goal.set(col_of(&text, e));
        history.peek().borrow_mut().break_group();
    };
    // Triple-click selects the whole line under the pointer (including its newline).
    let line_select = move |pos: Offset| {
        let cur = state.peek();
        let text = cur.text();
        let b = p2b(&text, pos);
        let s = line_start(&text, b);
        let le = line_end(&text, b);
        let e = if le < text.len() { next_char(&text, le) } else { le };
        state.set(EditorState {
            doc: cur.doc.clone(),
            selection: Selections::single(Selection::range(s, e)),
        });
        goal.set(col_of(&text, e));
        history.peek().borrow_mut().break_group();
    };
    // Column (rectangular) selection: Shift+Alt+drag makes one caret/range per row.
    let column_select = move |from: Offset, to: Offset| {
        let cur = state.peek();
        let text = cur.text();
        let (la, ca) = pos_to_grid(&text, from, pad_l, pad_t, advance, line_px);
        let (lb, cb) = pos_to_grid(&text, to, pad_l, pad_t, advance, line_px);
        let (l0, l1) = (la.min(lb), la.max(lb));
        let rows: Vec<Selection> = (l0..=l1)
            .map(|line| Selection::range(byte_at(&text, line, ca), byte_at(&text, line, cb)))
            .collect();
        let pi = if lb >= la { rows.len() - 1 } else { 0 };
        state.set(EditorState {
            doc: cur.doc.clone(),
            selection: Selections::new(rows, pi),
        });
        goal.set(cb);
        history.peek().borrow_mut().break_group();
    };

    // With a fixed height, bound the content to the widest line so it can overflow and scroll
    // horizontally (the gutter stays fixed, outside the horizontal scroll). Without a height
    // the editor grows to content. Soft wrap disables horizontal overflow (lines fit the width).
    let content_w = if wrap_cols > 0 {
        None
    } else {
        p.height.map(|_| {
            let max_cols = src.split('\n').map(|l| l.chars().count()).max().unwrap_or(0);
            pad_l * 2.0 + max_cols as f64 * advance
        })
    };
    let content_box = match content_w {
        Some(w) => container().width(w).height(content_h).child(grid),
        None => container().height(content_h).child(grid),
    };
    // Per-plugin pointer handlers: notify each extension's on_click with the byte under the
    // press. (The editor's own hit-testing above already placed the caret.)
    let click_exts = p.extensions.clone();
    let click_area = GestureDetector::new(content_box)
        .on_pointer_down(action_event(move |e| {
            focus.request_focus();
            let alt = pebbles::core::keyboard::alt_held();
            let shift = pebbles::core::keyboard::shift_held();
            drag_anchor.set(Some(e.position)); // remember where a drag begins
            dragged.set(false);
            if shift && alt {
                // Shift+Alt starts a column drag — don't place/add a caret on the press.
                text_drag.set(None);
            } else if !shift && !alt && sel_range_at(e.position).is_some() {
                // Press inside the selection: arm a text drag; keep the selection (a drag moves
                // it, a plain click collapses it — handled in on_pan_end / on_tap).
                text_drag.set(sel_range_at(e.position));
            } else {
                text_drag.set(None);
                hit(e.position, shift, alt);
            }
            if click_exts.iter().any(|x| x.on_click.is_some()) {
                let cur = state.peek();
                let text = cur.text();
                let b = p2b(&text, e.position);
                let pr = cur.primary();
                let snap = Snapshot {
                    text: &text,
                    caret: pr.head,
                    selection: (pr.min(), pr.max()),
                };
                for x in &click_exts {
                    if let Some(f) = &x.on_click {
                        f(&snap, b);
                    }
                }
            }
        }))
        .on_pan_start(action_event(move |e| drag_anchor.set(Some(e.position))))
        .on_pan_update(action_event(move |e| {
            let alt = pebbles::core::keyboard::alt_held();
            let shift = pebbles::core::keyboard::shift_held();
            if text_drag.peek().is_some() {
                // Dragging the selection: track the drop point (a drop caret renders at it).
                dragged.set(true);
                let text = state.peek().text();
                drop_byte.set(Some(p2b(&text, e.position)));
            } else if shift && alt {
                let from = drag_anchor.peek().unwrap_or(e.position);
                column_select(from, e.position);
            } else {
                hit(e.position, true, false);
            }
        }))
        .on_pan_end(action_event(move |e| {
            if let Some((a, b)) = text_drag.peek() {
                if dragged.peek() {
                    let text = state.peek().text();
                    let dst = p2b(&text, e.position);
                    let copy = pebbles::core::keyboard::ctrl_held()
                        || pebbles::core::keyboard::meta_held();
                    drop_text(a, b, dst, copy);
                } else {
                    hit(e.position, false, false); // a press+release with no real drag → collapse
                }
                text_drag.set(None);
                drop_byte.set(None);
                dragged.set(false);
            }
        }))
        .on_tap(action_event(move |e| {
            // A click inside the selection (no drag) collapses it to the caret.
            if text_drag.peek().is_some() {
                hit(e.position, false, false);
                text_drag.set(None);
                drop_byte.set(None);
            }
        }))
        .on_double_tap(action_event(move |e| word_select(e.position)))
        .on_triple_tap(action_event(move |e| line_select(e.position)))
        .on_hover_move(action_event(move |e| hover_pos.set(Some(e.position))))
        .on_hover_exit(action(move || hover_pos.set(None)));

    // Horizontal scroll: wrap the content (not the gutter) so long lines scroll sideways
    // while the line numbers stay put. Only when the editor is a bounded viewport.
    let content_area: AnyWidget = if content_w.is_some() {
        SingleChildScrollView::horizontal(click_area)
            .controller(hscroll.clone())
            .into_widget()
    } else {
        click_area.into_widget()
    };

    // Gutter to the left of the (horizontally-scrolling) content.
    let body = gutter::wrap(&frame, content_area);

    // Right-click menu driving the same edit commands as the keyboard.
    let body: AnyWidget = if p.context_menu {
        let (c_cut, c_copy, c_paste, c_all) = (cfg.clone(), cfg.clone(), cfg.clone(), cfg.clone());
        context_menu(body)
            .item(menu_item("Cut").on_select(move || {
                apply_key(KeyInput::Cut, state, history, code, goal, read_only, &c_cut)
            }))
            .item(menu_item("Copy").on_select(move || {
                apply_key(KeyInput::Copy, state, history, code, goal, read_only, &c_copy)
            }))
            .item(menu_item("Paste").on_select(move || {
                apply_key(KeyInput::Paste, state, history, code, goal, read_only, &c_paste)
            }))
            .separator()
            .item(menu_item("Select All").on_select(move || {
                apply_key(KeyInput::SelectAll, state, history, code, goal, read_only, &c_all)
            }))
            .into_widget()
    } else {
        body
    };

    // With a fixed height the editor scrolls within a viewport (with sticky headers + an
    // optional minimap); without one it grows to its content (inline snippets).
    let code_area: AnyWidget = match p.height {
        Some(h) => {
            let on_scroll_cb = p.on_scroll.clone();
            let scroller = scroll_view(body)
                .controller(scroll.clone())
                // Re-render the visible window as the viewport scrolls, and report the offset so
                // the caller can persist/restore it (view state).
                .on_scroll(move |n| {
                    scroll_top.set(n.metrics.pixels);
                    if let Some(cb) = &on_scroll_cb {
                        cb(n.metrics.pixels);
                    }
                })
                .into_widget();
            let scroller = sticky::overlay(&frame, scroller, scroll_top);
            let inner: AnyWidget = match minimap::panel(&frame, scroll, scroll_top) {
                Some(mm) => row(children![expanded(scroller), mm])
                    .cross_axis_alignment(CrossAxisAlignment::Start)
                    .into_widget(),
                None => scroller,
            };
            container()
                .height(h)
                .decoration(BoxDecoration::new().color(theme.background))
                .child(inner)
                .into_widget()
        }
        None => container()
            .decoration(BoxDecoration::new().color(theme.background))
            .child(body)
            .into_widget(),
    };

    // Overlay the find/replace bar (top-right) when open.
    let code_area: AnyWidget = if find.open.get() > 0 {
        let bar = search::bar(find, search_matches, state, history, code, goal, focus, theme);
        stack(children![code_area, bar]).into_widget()
    } else {
        code_area
    };

    // Overlay the command palette (all extension commands) when open.
    let code_area: AnyWidget = if palette.open.get() {
        let commands: Vec<crate::extensions::Command> = p
            .extensions
            .iter()
            .flat_map(|e| e.commands.iter().cloned())
            .collect();
        let ctx = crate::extensions::EditContext { state, history, code, goal };
        let pal = extensions::palette(palette, commands, ctx, focus, theme);
        stack(children![code_area, pal]).into_widget()
    } else {
        code_area
    };

    // ---- chrome: optional title/status bar above the code area ----
    // The editor renders FLAT by design — just its background, no rounded corners and no
    // border. Any frame (radius, border, shadow, card) is the caller's to add by wrapping the
    // editor, so it drops cleanly into any design.
    let lang_name = p
        .language
        .as_ref()
        .map(|l| l.name().to_string())
        .unwrap_or_else(|| "Plain Text".into());
    let mut col: Vec<AnyWidget> = Vec::new();
    if let Some(t) = &p.title {
        col.push(status_bar(t, &lang_name, cl + 1, cc + 1, theme).into_widget());
        col.push(
            container()
                .height(1.0)
                .decoration(BoxDecoration::new().color(theme.border))
                .into_widget(),
        );
    }
    col.push(code_area);

    let body = container()
        .color(theme.background)
        .child(
            column(col)
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .main_axis_size(MainAxisSize::Min),
        );

    // Accessibility: expose the editor as a multiline text input with the document as its
    // value, so screen readers announce it (name + role + content).
    semantics(SemanticsRole::TextInput, p.a11y_label.clone(), body)
        .value(src.clone())
        .into_widget()
}
