//! Running [`EditorCommand`]s against the live editor: pure document operations from
//! [`crate::ops`] are dispatched through the single mutation path; view-state commands
//! (folding, wrap, zoom, scrolling, find navigation, cursor history, smart select) act on the
//! view's signals here.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use pebbles::prelude::*;

use crate::actions::EditorCommand as C;
use crate::commands::dispatch;
use crate::edit::{Coalesce, EditorState, History, Selection, Selections, Transaction};
use crate::geometry::*;
use crate::ops::{self, Case, OpCfg};

/// Selection history for Cursor Undo / Cursor Redo (soft undo). Records every selection
/// change that leaves the text untouched; an edit clears it.
#[derive(Default)]
pub(crate) struct CursorHistory {
    pub(crate) undo: Vec<Selections>,
    pub(crate) redo: Vec<Selections>,
    /// The last observed state (to diff the next one against).
    pub(crate) last: Option<EditorState>,
    /// Set while a cursor-undo applies, so it is not re-recorded.
    pub(crate) applying: bool,
}

impl CursorHistory {
    /// Observe a new state.
    pub(crate) fn observe(&mut self, st: &EditorState) {
        if self.applying {
            self.applying = false;
            self.last = Some(st.clone());
            return;
        }
        if let Some(prev) = &self.last {
            if prev.doc.len_bytes() != st.doc.len_bytes() || prev.doc != st.doc {
                self.undo.clear();
                self.redo.clear();
            } else if prev.selection != st.selection {
                self.undo.push(prev.selection.clone());
                if self.undo.len() > 200 {
                    self.undo.remove(0);
                }
                self.redo.clear();
            }
        }
        self.last = Some(st.clone());
    }
}

/// Everything a command may touch (all cheap clones / `Copy` signals).
#[derive(Clone)]
pub(crate) struct Env {
    pub(crate) state: Signal<EditorState>,
    pub(crate) history: Signal<Rc<RefCell<History>>>,
    pub(crate) code: Signal<String>,
    pub(crate) goal: Signal<usize>,
    pub(crate) folds: Signal<BTreeSet<usize>>,
    pub(crate) wrap: Signal<Option<bool>>,
    pub(crate) soft_wrap_default: bool,
    pub(crate) zoom: Signal<f64>,
    pub(crate) cursor_hist: Signal<Rc<RefCell<CursorHistory>>>,
    pub(crate) smart_stack: Signal<Vec<Selections>>,
    #[allow(clippy::type_complexity)]
    pub(crate) syntax: Signal<Rc<RefCell<Option<(String, Box<dyn crate::lang::SyntaxSession>)>>>>,
    pub(crate) find: crate::view::search::State,
    pub(crate) palette: crate::view::extensions::Palette,
    pub(crate) vscroll: crate::view::VScroll,
    pub(crate) scroll_top: Signal<f64>,
    pub(crate) height: Option<f64>,
    pub(crate) line_px: f64,
    pub(crate) read_only: bool,
    pub(crate) diff_base: Option<Signal<String>>,
    pub(crate) ops: Rc<OpCfg>,
    pub(crate) line_cache: Signal<Rc<RefCell<crate::lines::LineCache>>>,
    pub(crate) inline: Signal<Option<crate::view::InlineGhost>>,
    pub(crate) tab_focus: Signal<bool>,
    pub(crate) overtype: Signal<bool>,
    /// The language name (for Emmet).
    pub(crate) lang_name: String,
    pub(crate) inline_provider: Option<crate::providers::InlineSuggestionProvider>,
    /// The live snippet session (Emmet expansions start one).
    pub(crate) snippet: Signal<Option<crate::snippet::SnippetSession>>,
    /// The document URI (snippet variables).
    pub(crate) doc_uri: String,
    /// The language-service UI (hover, navigation, rename, …).
    pub(crate) lsp: crate::view::lsp::Lsp,
}

/// Whether `cmd` edits the document (vetoed in read-only mode).
fn is_edit(cmd: C) -> bool {
    matches!(
        cmd,
        C::MoveLinesUp
            | C::MoveLinesDown
            | C::CopyLinesUp
            | C::CopyLinesDown
            | C::DeleteLines
            | C::JoinLines
            | C::DuplicateSelection
            | C::SortLinesAscending
            | C::SortLinesDescending
            | C::TransposeLetters
            | C::InsertLineBelow
            | C::InsertLineAbove
            | C::IndentLines
            | C::OutdentLines
            | C::ToggleLineComment
            | C::AddLineComment
            | C::RemoveLineComment
            | C::ToggleBlockComment
            | C::TransformToUppercase
            | C::TransformToLowercase
            | C::TransformToTitlecase
            | C::TransformToSnakecase
            | C::TransformToKebabcase
            | C::TransformToCamelcase
            | C::TrimTrailingWhitespace
            | C::InsertFinalNewline
            | C::IndentationToSpaces
            | C::IndentationToTabs
            | C::ChangeEolToLf
            | C::ChangeEolToCrlf
            | C::DeleteWordPartLeft
            | C::DeleteWordPartRight
    )
}

/// Run `cmd`. Returns whether it applied (a `false` lets the key fall through).
pub(crate) fn run(cmd: C, env: &Env) -> bool {
    if env.read_only && is_edit(cmd) {
        return false;
    }
    let st = env.state.peek();
    let src = st.text();
    let sels = st.selection.clamped(&st.doc);
    let cfg = &*env.ops;
    let edit = |tx: Option<Transaction>| -> bool {
        let Some(tx) = tx else { return true };
        if tx.is_edit() {
            dispatch(env.state, env.history, env.code, tx, Coalesce::Never);
        } else {
            let cur = env.state.peek();
            let (next, _) = cur.apply(&tx);
            env.state.set(next);
            env.history.peek().borrow_mut().break_group();
        }
        let now = env.state.peek();
        env.goal.set(col_of(&now.text(), now.primary().head));
        true
    };
    let select = |sel: Selections| edit(Some(Transaction::select(sel)));
    let caret_line = line_of(&src, sels.primary().head);
    match cmd {
        C::ShowHover => {
            env.lsp.show_hover_at_caret();
            true
        }
        C::GoToDefinition => env.lsp.navigate(crate::view::lsp::Nav::Definition, false),
        C::GoToDeclaration => env.lsp.navigate(crate::view::lsp::Nav::Declaration, false),
        C::GoToTypeDefinition => env
            .lsp
            .navigate(crate::view::lsp::Nav::TypeDefinition, false),
        C::GoToImplementation => env
            .lsp
            .navigate(crate::view::lsp::Nav::Implementation, false),
        C::PeekDefinition => env.lsp.navigate(crate::view::lsp::Nav::Definition, true),
        C::PeekImplementation => env
            .lsp
            .navigate(crate::view::lsp::Nav::Implementation, true),
        C::FindReferences => env.lsp.navigate(crate::view::lsp::Nav::References, true),
        C::ShowReferences => env.lsp.show_references(),
        C::Rename => env.lsp.start_rename(),
        C::FormatDocument => env.lsp.format_document(),
        C::FormatSelection => env.lsp.format_selection(),
        C::QuickFix => env.lsp.quick_fix(),
        C::AutoFix => env.lsp.auto_fix(),
        C::NextProblem => env.lsp.next_problem(true),
        C::PreviousProblem => env.lsp.next_problem(false),
        C::GoToSymbol => env.lsp.open_go_to_symbol(env.palette),
        C::ShowAllSymbols => env.lsp.open_workspace_symbols(env.palette),
        C::ToggleInlayHints => env.lsp.toggle_inlays(),
        C::ToggleCodeLens => env.lsp.toggle_lenses(),
        C::FocusBreadcrumbs => env.lsp.focus_breadcrumbs(false),
        C::FocusAndSelectBreadcrumbs => env.lsp.focus_breadcrumbs(true),
        C::TriggerParameterHints => {
            env.lsp
                .request_signature(crate::language::SignatureHelpTriggerKind::Invoked, None);
            true
        }
        C::AddCursorAbove => edit(ops::add_cursor_vertical(&src, &sels, false)),
        C::AddCursorBelow => edit(ops::add_cursor_vertical(&src, &sels, true)),
        C::SelectAllOccurrences => edit(ops::select_all_occurrences(&src, &sels)),
        C::MoveSelectionToNextFindMatch => {
            let p = sels.primary();
            if p.is_empty() {
                return edit(ops::select_all_occurrences(&src, &sels).map(|_| {
                    let (s, e) = word_at(&src, p.head);
                    Transaction::select(Selections::single(Selection::range(s, e)))
                }));
            }
            let needle = &src[p.min()..p.max()];
            let next = find_from(&src, needle, p.max()).or_else(|| find_from(&src, needle, 0));
            match next {
                Some(s) => {
                    let mut r: Vec<Selection> =
                        sels.ranges().iter().copied().filter(|x| *x != p).collect();
                    r.push(Selection::range(s, s + needle.len()));
                    let n = r.len();
                    select(Selections::new(r, n - 1))
                }
                None => true,
            }
        }
        C::AddCursorsToLineEnds => edit(ops::cursors_at_line_ends(&src, &sels)),
        C::CursorUndo | C::CursorRedo => {
            let cell = env.cursor_hist.peek();
            let mut h = cell.borrow_mut();
            // Catch up with a selection change whose effect has not flushed yet.
            h.observe(&st);
            let prev = if cmd == C::CursorUndo {
                h.undo.pop()
            } else {
                h.redo.pop()
            };
            let Some(prev) = prev else { return true };
            if cmd == C::CursorUndo {
                h.redo.push(sels.clone());
            } else {
                h.undo.push(sels.clone());
            }
            h.applying = true;
            drop(h);
            env.state.set(EditorState {
                doc: st.doc.clone(),
                selection: prev.clamped(&st.doc),
            });
            true
        }
        C::ColumnSelectUp => edit(ops::column_select(&src, &sels, -1, 0)),
        C::ColumnSelectDown => edit(ops::column_select(&src, &sels, 1, 0)),
        C::ColumnSelectLeft => edit(ops::column_select(&src, &sels, 0, -1)),
        C::ColumnSelectRight => edit(ops::column_select(&src, &sels, 0, 1)),
        C::ExpandLineSelection => edit(ops::expand_line_selection(&src, &sels)),
        C::JumpToBracket => edit(ops::jump_to_bracket(&src, &sels, cfg)),
        C::SelectToBracket => edit(ops::select_to_bracket(&src, &sels, cfg)),
        C::MoveLinesUp => edit(ops::move_lines(&src, &sels, true)),
        C::MoveLinesDown => edit(ops::move_lines(&src, &sels, false)),
        C::CopyLinesUp => edit(ops::copy_lines(&src, &sels, true)),
        C::CopyLinesDown => edit(ops::copy_lines(&src, &sels, false)),
        C::DeleteLines => edit(ops::delete_lines(&src, &sels)),
        C::JoinLines => edit(ops::join_lines(&src, &sels)),
        C::DuplicateSelection => edit(ops::duplicate_selection(&src, &sels)),
        C::SortLinesAscending => edit(ops::sort_lines(&src, &sels, false)),
        C::SortLinesDescending => edit(ops::sort_lines(&src, &sels, true)),
        C::TransposeLetters => edit(ops::transpose(&src, &sels)),
        C::InsertLineBelow => edit(ops::insert_line(&src, &sels, false)),
        C::InsertLineAbove => edit(ops::insert_line(&src, &sels, true)),
        C::IndentLines => edit(ops::indent_lines(&src, &sels, cfg, false)),
        C::OutdentLines => edit(ops::indent_lines(&src, &sels, cfg, true)),
        C::ToggleLineComment | C::AddLineComment | C::RemoveLineComment => {
            let mode = match cmd {
                C::AddLineComment => Some(true),
                C::RemoveLineComment => Some(false),
                _ => None,
            };
            match cfg.line_comment.as_deref() {
                Some(p) => edit(ops::line_comment(&src, &sels, p, mode)),
                // No line comment: VS Code falls back to a block comment.
                None => edit(ops::toggle_block_comment(&src, &sels, cfg)),
            }
        }
        C::ToggleBlockComment => edit(ops::toggle_block_comment(&src, &sels, cfg)),
        C::TransformToUppercase => edit(ops::transform_case(&src, &sels, Case::Upper)),
        C::TransformToLowercase => edit(ops::transform_case(&src, &sels, Case::Lower)),
        C::TransformToTitlecase => edit(ops::transform_case(&src, &sels, Case::Title)),
        C::TransformToSnakecase => edit(ops::transform_case(&src, &sels, Case::Snake)),
        C::TransformToKebabcase => edit(ops::transform_case(&src, &sels, Case::Kebab)),
        C::TransformToCamelcase => edit(ops::transform_case(&src, &sels, Case::Camel)),
        C::TrimTrailingWhitespace => edit(ops::trim_trailing_whitespace(&src, &sels)),
        C::InsertFinalNewline => edit(ops::insert_final_newline(&src, &sels)),
        C::IndentationToSpaces => edit(ops::convert_indentation(&src, true, cfg.tab_size)),
        C::IndentationToTabs => edit(ops::convert_indentation(&src, false, cfg.tab_size)),
        C::ChangeEolToLf => edit(ops::convert_line_endings(&src, ops::LineEnding::Lf)),
        C::ChangeEolToCrlf => edit(ops::convert_line_endings(&src, ops::LineEnding::CrLf)),
        C::CursorWordPartLeft
        | C::CursorWordPartRight
        | C::CursorWordPartLeftSelect
        | C::CursorWordPartRightSelect => {
            let right = matches!(cmd, C::CursorWordPartRight | C::CursorWordPartRightSelect);
            let extend = matches!(
                cmd,
                C::CursorWordPartLeftSelect | C::CursorWordPartRightSelect
            );
            let r: Vec<Selection> = sels
                .ranges()
                .iter()
                .map(|r| {
                    let t = if right {
                        ops::next_subword(&src, r.head)
                    } else {
                        ops::prev_subword(&src, r.head)
                    };
                    Selection::range(if extend { r.anchor } else { t }, t)
                })
                .collect();
            select(Selections::new(r, sels.primary_index()))
        }
        C::DeleteWordPartLeft | C::DeleteWordPartRight => {
            let right = cmd == C::DeleteWordPartRight;
            let changes: Vec<crate::edit::Change> = sels
                .ranges()
                .iter()
                .map(|r| {
                    let (a, b) = if !r.is_empty() {
                        (r.min(), r.max())
                    } else if right {
                        (r.head, ops::next_subword(&src, r.head))
                    } else {
                        (ops::prev_subword(&src, r.head), r.head)
                    };
                    crate::edit::Change {
                        from: a,
                        to: b,
                        insert: String::new(),
                    }
                })
                .collect();
            let cs = crate::edit::ChangeSet::from_changes(changes);
            let sel = sels.mapped(&cs);
            edit(Some(Transaction::change_and_select(cs, sel)))
        }
        C::CursorPageUp | C::CursorPageDown | C::CursorPageUpSelect | C::CursorPageDownSelect => {
            let rows = env.height.map_or(20, |h| {
                ((h / env.line_px).floor() as usize)
                    .saturating_sub(1)
                    .max(1)
            });
            let down = matches!(cmd, C::CursorPageDown | C::CursorPageDownSelect);
            let extend = matches!(cmd, C::CursorPageUpSelect | C::CursorPageDownSelect);
            let col = env.goal.peek();
            let n = src.split('\n').count();
            let r: Vec<Selection> = sels
                .ranges()
                .iter()
                .map(|r| {
                    let l = line_of(&src, r.head);
                    let t = if down {
                        (l + rows).min(n - 1)
                    } else {
                        l.saturating_sub(rows)
                    };
                    let b = byte_at(&src, t, col);
                    Selection::range(if extend { r.anchor } else { b }, b)
                })
                .collect();
            let ok = select(Selections::new(r, sels.primary_index()));
            env.goal.set(col);
            ok
        }
        C::ScrollLineUp | C::ScrollLineDown | C::ScrollPageUp | C::ScrollPageDown => {
            let Some(h) = env.height else { return false };
            let step = match cmd {
                C::ScrollLineUp => -env.line_px,
                C::ScrollLineDown => env.line_px,
                C::ScrollPageUp => -h,
                _ => h,
            };
            let t = (env.scroll_top.peek() + step).max(0.0);
            env.vscroll.to(t, env.scroll_top);
            true
        }
        C::Fold
        | C::Unfold
        | C::ToggleFold
        | C::FoldRecursively
        | C::UnfoldRecursively
        | C::FoldAll
        | C::UnfoldAll
        | C::FoldAllBlockComments
        | C::FoldAllMarkerRegions
        | C::UnfoldAllMarkerRegions
        | C::FoldAllExcept
        | C::FoldSelection
        | C::RemoveManualFolds
        | C::FoldLevel1
        | C::FoldLevel2
        | C::FoldLevel3
        | C::FoldLevel4
        | C::FoldLevel5
        | C::FoldLevel6
        | C::FoldLevel7 => {
            run_fold(cmd, env, &src, caret_line, &sels);
            true
        }
        C::ToggleWordWrap => {
            let cur = env.wrap.peek().unwrap_or(env.soft_wrap_default);
            env.wrap.set(Some(!cur));
            true
        }
        C::FontZoomIn => {
            env.zoom.set((env.zoom.peek() + 1.0).min(40.0));
            true
        }
        C::FontZoomOut => {
            env.zoom.set((env.zoom.peek() - 1.0).max(-8.0));
            true
        }
        C::FontZoomReset => {
            env.zoom.set(0.0);
            true
        }
        C::GoToLine => {
            env.palette.query.set(":".to_string());
            env.palette.sel.set(0);
            env.palette.open.set(true);
            pebbles::core::focus::set_focus(None);
            true
        }
        C::FindNext | C::FindPrevious => {
            if env.find.query.peek().is_empty() {
                let p = sels.primary();
                let q = if p.is_empty() {
                    let (s, e) = word_at(&src, p.head);
                    src[s..e].to_string()
                } else {
                    src[p.min()..p.max()].to_string()
                };
                if q.is_empty() {
                    return true;
                }
                env.find.query.set(q);
            }
            let matches =
                crate::search::find_all(&src, &env.find.query.peek(), &env.find.options());
            let caret = if cmd == C::FindNext {
                sels.primary().max()
            } else {
                sels.primary().min()
            };
            if let Some(i) = ops::next_match(&matches, caret, cmd == C::FindNext) {
                env.find.idx.set(i);
                let (s, e) = matches[i];
                select(Selections::single(Selection::range(s, e)));
            }
            true
        }
        C::NextChange | C::PreviousChange => {
            let Some(base) = env.diff_base else {
                return false;
            };
            let d = crate::diff::diff_lines(&base.peek(), &src);
            let hunks = ops::change_hunks(&d);
            let target = if cmd == C::NextChange {
                hunks
                    .iter()
                    .copied()
                    .find(|&l| l > caret_line)
                    .or(hunks.first().copied())
            } else {
                hunks
                    .iter()
                    .copied()
                    .rev()
                    .find(|&l| l < caret_line)
                    .or(hunks.last().copied())
            };
            if let Some(l) = target {
                select(Selections::single(Selection::caret(line_start_of(&src, l))));
            }
            true
        }
        C::InlineSuggestNext | C::InlineSuggestPrevious => {
            let Some(mut g) = env.inline.peek() else {
                return false;
            };
            let n = g.items.len().max(1);
            g.idx = if cmd == C::InlineSuggestNext {
                (g.idx + 1) % n
            } else {
                (g.idx + n - 1) % n
            };
            env.inline.set(Some(g));
            true
        }
        C::EmmetExpand => emmet_expand(env),
        C::ToggleTabFocusMode => {
            env.tab_focus.set(!env.tab_focus.peek());
            true
        }
        C::ToggleOvertype => {
            env.overtype.set(!env.overtype.peek());
            true
        }
        C::InlineSuggestTrigger => {
            let Some(ip) = &env.inline_provider else {
                return false;
            };
            let at = sels.primary().head;
            let items: Vec<String> = ip(&src, at).into_iter().filter(|s| !s.is_empty()).collect();
            env.inline
                .set((!items.is_empty()).then_some(crate::view::InlineGhost { at, items, idx: 0 }));
            true
        }
        C::ExpandSelection if env.lsp.expand_selection(env.smart_stack) => true,
        C::ExpandSelection => {
            let ranges: Vec<Selection> = sels
                .ranges()
                .iter()
                .map(|r| {
                    let (a, b) = (r.min(), r.max());
                    let mut ladder = {
                        let cell = env.syntax.peek();
                        let mut slot = cell.borrow_mut();
                        match slot.as_mut() {
                            Some((_, s)) => {
                                s.update(&src);
                                s.enclosing_nodes(a, b)
                            }
                            None => Vec::new(),
                        }
                    };
                    ladder.extend(ops::fallback_ladder(&src, &cfg.brackets, a, b));
                    match ops::expand_to(&ladder, a, b) {
                        Some((s, e)) => Selection::range(s, e),
                        None => *r,
                    }
                })
                .collect();
            let next = Selections::new(ranges, sels.primary_index());
            if next != sels {
                env.smart_stack.update(|v| v.push(sels.clone()));
            }
            select(next)
        }
        C::ShrinkSelection => {
            let mut stack = env.smart_stack.peek();
            match stack.pop() {
                Some(prev) => {
                    env.smart_stack.set(stack);
                    select(prev.clamped(&st.doc))
                }
                None => true,
            }
        }
    }
}

/// The fold commands (they edit the fold set, never the text).
fn run_fold(cmd: C, env: &Env, src: &str, line: usize, sels: &Selections) {
    // Manual folding ranges: update the cache's set first so `regions` below includes them.
    let sel_lines: Vec<(usize, usize)> = sels
        .ranges()
        .iter()
        .map(|r| (line_of(src, r.min()), line_of(src, r.max())))
        .collect();
    let mut removed: Vec<usize> = Vec::new();
    if matches!(cmd, C::FoldSelection | C::RemoveManualFolds) {
        let cache = env.line_cache.peek();
        let mut lc = cache.borrow_mut();
        let mut manual = lc.sync(src).manual().to_vec();
        if cmd == C::FoldSelection {
            for &(h, l) in sel_lines.iter().filter(|(h, l)| l > h) {
                manual.retain(|m| m.0 != h);
                manual.push((h, l));
            }
        } else {
            manual.retain(|m| {
                let hit = sel_lines.iter().any(|&(h, l)| m.0 <= l && h <= m.1);
                if hit {
                    removed.push(m.0);
                }
                !hit
            });
        }
        lc.set_manual(manual);
    }
    let regions = env
        .line_cache
        .peek()
        .borrow_mut()
        .sync(src)
        .regions()
        .as_ref()
        .clone();
    let mut f = env.folds.peek();
    let at = crate::fold::region_at(&regions, line);
    match cmd {
        C::Fold => {
            // Fold the innermost unfolded region at the cursor (repeat folds outward).
            let mut cands: Vec<(usize, usize)> = regions
                .iter()
                .copied()
                .filter(|r| r.0 <= line && line <= r.1 && !f.contains(&r.0))
                .collect();
            cands.sort_by_key(|r| r.1 - r.0);
            if let Some(r) = cands.first() {
                f.insert(r.0);
            }
        }
        C::Unfold => {
            if let Some(r) = regions
                .iter()
                .filter(|r| r.0 <= line && line <= r.1 && f.contains(&r.0))
                .min_by_key(|r| r.1 - r.0)
            {
                f.remove(&r.0);
            }
        }
        C::ToggleFold => {
            if let Some(r) = at
                && !f.remove(&r.0)
            {
                f.insert(r.0);
            }
        }
        C::FoldRecursively | C::UnfoldRecursively => {
            if let Some(r) = at {
                for h in crate::fold::nested_heads(&regions, r) {
                    if cmd == C::FoldRecursively {
                        f.insert(h);
                    } else {
                        f.remove(&h);
                    }
                }
            }
        }
        C::FoldAll => f.extend(regions.iter().map(|r| r.0)),
        C::UnfoldAll => f.clear(),
        C::FoldAllBlockComments => {
            for r in &regions {
                let ls = line_start_of(src, r.0);
                let t = src[ls..line_end(src, ls)].trim_start();
                if t.starts_with("/*") || t.starts_with("<!--") || t.starts_with("\"\"\"") {
                    f.insert(r.0);
                }
            }
        }
        C::FoldAllMarkerRegions | C::UnfoldAllMarkerRegions => {
            for r in &regions {
                let ls = line_start_of(src, r.0);
                if crate::lines::marker_of(&src[ls..line_end(src, ls)]) > 0 {
                    if cmd == C::FoldAllMarkerRegions {
                        f.insert(r.0);
                    } else {
                        f.remove(&r.0);
                    }
                }
            }
        }
        C::FoldSelection => {
            f.extend(sel_lines.iter().filter(|(h, l)| l > h).map(|r| r.0));
        }
        C::RemoveManualFolds => {
            for h in &removed {
                f.remove(h);
            }
        }
        C::FoldAllExcept => {
            let keep: Vec<usize> = sels.ranges().iter().map(|r| line_of(src, r.head)).collect();
            for r in &regions {
                if !keep.iter().any(|&l| r.0 <= l && l <= r.1) {
                    f.insert(r.0);
                }
            }
        }
        _ => {
            let level = match cmd {
                C::FoldLevel1 => 1,
                C::FoldLevel2 => 2,
                C::FoldLevel3 => 3,
                C::FoldLevel4 => 4,
                C::FoldLevel5 => 5,
                C::FoldLevel6 => 6,
                _ => 7,
            };
            // VS Code: fold every region at the level, except those containing a cursor.
            for h in crate::fold::heads_at_level(&regions, level) {
                let r = regions.iter().find(|r| r.0 == h).copied().unwrap_or((h, h));
                if !sels
                    .ranges()
                    .iter()
                    .any(|s| (r.0 + 1..=r.1).contains(&line_of(src, s.head)))
                {
                    f.insert(h);
                }
            }
        }
    }
    // A fold that hides a caret moves the caret to the fold head.
    let hidden = |l: usize| {
        regions
            .iter()
            .any(|r| f.contains(&r.0) && r.0 < l && l <= r.1)
    };
    if sels.ranges().iter().any(|r| hidden(line_of(src, r.head))) {
        let r: Vec<Selection> = sels
            .ranges()
            .iter()
            .map(|r| {
                let mut l = line_of(src, r.head);
                while hidden(l) {
                    l = regions
                        .iter()
                        .filter(|x| f.contains(&x.0) && x.0 < l && l <= x.1)
                        .map(|x| x.0)
                        .min()
                        .unwrap_or(l.saturating_sub(1));
                }
                if l == line_of(src, r.head) {
                    *r
                } else {
                    Selection::caret(line_end(src, line_start_of(src, l)))
                }
            })
            .collect();
        env.state.set(EditorState {
            doc: env.state.peek().doc.clone(),
            selection: Selections::new(r, sels.primary_index()),
        });
    }
    env.folds.set(f);
}

/// Expand the Emmet abbreviation before the primary caret; false when there is none.
pub(crate) fn emmet_expand(env: &Env) -> bool {
    let st = env.state.peek();
    let pr = st.primary();
    if !pr.is_empty() || st.selection.is_multi() {
        return false;
    }
    let src = st.text();
    let Some((start, body)) =
        crate::emmet::expand_at_snippet(&src, pr.head, &env.lang_name, &env.ops.tab)
    else {
        return false;
    };
    crate::snippet::insert(
        env.state,
        env.history,
        env.code,
        env.snippet,
        start,
        pr.head,
        &body,
        Vec::new(),
        &crate::snippet::InsertOpts {
            uri: &env.doc_uri,
            line_comment: None,
            adjust_indent: false,
        },
    );
    true
}
