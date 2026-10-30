//! Document symbols: the outline (kept fresh per document version and published to the
//! host's outline signal), Go to Symbol in File (Ctrl+Shift+O, the palette's `@` mode, `@:`
//! grouped by kind, live preview while moving) and Go to Symbol in Workspace (Ctrl+T, the
//! palette's `#` mode, results opened through the host).

use std::rc::Rc;

use pebbles::prelude::*;

use crate::edit::{EditorState, Selection, Selections};
use crate::extensions::Command;
use crate::language::{self as lsp};
use crate::view::lsp::Lsp;

/// One Go-to-Symbol entry: the palette title and the byte ranges (name, whole symbol).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SymbolEntry {
    pub(crate) title: String,
    pub(crate) name: (usize, usize),
    pub(crate) range: (usize, usize),
}

/// The Go-to-Symbol entries for an `@…` query over `symbols` in `text`: filtered fuzzily by
/// name (best first) or, with `@:`, grouped by kind (the first entry of a group carries the
/// group label, e.g. `functions (3)`, as VS Code shows it).
pub(crate) fn symbol_entries(
    symbols: &[lsp::DocumentSymbol],
    text: &str,
    query: &str,
) -> Vec<SymbolEntry> {
    let q = query.strip_prefix('@').unwrap_or(query);
    let (grouped, q) = match q.strip_prefix(':') {
        Some(rest) => (true, rest.trim()),
        None => (false, q.trim()),
    };
    let mut hits: Vec<(i32, usize, usize, &lsp::DocumentSymbol)> = lsp::flatten_symbols(symbols)
        .into_iter()
        .enumerate()
        .filter_map(|(i, (d, s))| {
            crate::view::completion::fuzzy_score(q, &s.name).map(|(sc, _)| (sc, i, d, s))
        })
        .collect();
    if grouped {
        hits.sort_by_key(|&(_, i, _, s)| (s.kind.to_lsp(), i));
    } else if !q.is_empty() {
        hits.sort_by_key(|&(sc, i, _, _)| (std::cmp::Reverse(sc), i));
    }
    let mut out = Vec::new();
    for (n, &(_, _, depth, s)) in hits.iter().enumerate() {
        let indent = if grouped || !q.is_empty() { 0 } else { depth };
        let mut title = format!("{}{}", "  ".repeat(indent), s.name);
        if let Some(d) = &s.detail {
            title.push_str(&format!("  {d}"));
        }
        if grouped && (n == 0 || hits[n - 1].3.kind != s.kind) {
            let count = hits.iter().filter(|h| h.3.kind == s.kind).count();
            title.push_str(&format!("    {} ({count})", s.kind.category()));
        }
        out.push(SymbolEntry {
            title,
            name: lsp::range_to_bytes(text, s.selection_range),
            range: lsp::range_to_bytes(text, s.range),
        });
    }
    out
}

impl Lsp {
    /// Ask for the document symbols when the document changed since the last request; the
    /// reply updates the outline (and the host's outline signal).
    pub(crate) fn request_symbols(&self) {
        let Some(svc) = self.svc.clone() else { return };
        let ver = self.version();
        if self.store.sym_ver.get() == Some(ver) {
            return;
        }
        self.store.sym_ver.set(Some(ver));
        let (sig, host) = (self.h.symbols, self.outline);
        let reply = self.guard(
            &self.store.sym_gate,
            move |r: lsp::DocumentSymbolResponse| {
                let v = r.into_nested();
                if let Some(h) = host {
                    h.set(v.clone());
                }
                sig.set(Rc::new(v));
            },
        );
        self.with_doc(|doc, _| svc.document_symbols(doc, reply));
    }

    /// Ctrl+Shift+O: open the palette in `@` mode (remembering the selection so Escape
    /// restores it after the live preview moved it).
    pub(crate) fn open_go_to_symbol(&self, pal: crate::view::extensions::Palette) -> bool {
        if self.svc.is_none() {
            return false;
        }
        self.request_symbols();
        self.h
            .sym_restore
            .set(Some(self.state.peek().selection.clone()));
        pal.query.set("@".to_string());
        pal.sel.set(0);
        pal.open.set(true);
        pebbles::core::focus::set_focus(None);
        true
    }

    /// Ctrl+T: open the palette in `#` mode (workspace symbols).
    pub(crate) fn open_workspace_symbols(&self, pal: crate::view::extensions::Palette) -> bool {
        if self.svc.is_none() {
            return false;
        }
        pal.query.set("#".to_string());
        pal.sel.set(0);
        pal.open.set(true);
        pebbles::core::focus::set_focus(None);
        true
    }

    /// The current `@` entries.
    pub(crate) fn current_symbol_entries(&self, query: &str) -> Vec<SymbolEntry> {
        let text = self.state.peek().text();
        symbol_entries(&self.h.symbols.peek(), &text, query)
    }

    /// The `@` mode palette commands (accepting moves the caret to the symbol's name).
    pub(crate) fn symbol_commands(&self, query: &str) -> Vec<Command> {
        self.current_symbol_entries(query)
            .into_iter()
            .enumerate()
            .map(|(i, e)| {
                let me = self.clone();
                Command::new(format!("symbol.{i}"), e.title.clone(), move |_| {
                    me.h.sym_restore.set(None);
                    me.h.sym_preview.set(None);
                    me.select_range(e.name.0, e.name.0);
                })
            })
            .collect()
    }

    /// Live preview while moving through the `@` list: reveal the symbol (caret on its name)
    /// and highlight its whole range. Called from an effect on the palette state.
    pub(crate) fn preview_symbol(&self, open: bool, query: &str, sel: usize) {
        if !open || !query.starts_with('@') {
            if !open && let Some(prev) = self.h.sym_restore.peek() {
                // Closed without accepting: put the selection back.
                self.h.sym_restore.set(None);
                let cur = self.state.peek();
                self.state.set(EditorState {
                    doc: cur.doc.clone(),
                    selection: prev.clamped(&cur.doc),
                });
            }
            if self.h.sym_preview.peek().is_some() {
                self.h.sym_preview.set(None);
            }
            return;
        }
        let entries = self.current_symbol_entries(query);
        let Some(e) = entries.get(sel.min(entries.len().saturating_sub(1))) else {
            if self.h.sym_preview.peek().is_some() {
                self.h.sym_preview.set(None);
            }
            return;
        };
        if self.h.sym_preview.peek() != Some(e.range) {
            self.h.sym_preview.set(Some(e.range));
            self.select_range(e.name.0, e.name.0);
        }
    }

    fn select_range(&self, from: usize, head: usize) {
        let cur = self.state.peek();
        let text = cur.text();
        self.state.set(EditorState {
            doc: cur.doc.clone(),
            selection: Selections::single(Selection::range(from, head)),
        });
        self.goal.set(crate::geometry::col_of(&text, head));
        self.history.peek().borrow_mut().break_group();
    }

    /// Request workspace symbols for a `#…` query (once per distinct query).
    pub(crate) fn request_workspace_symbols(&self, query: &str) {
        let Some(svc) = self.svc.clone() else { return };
        let q = query.strip_prefix('#').unwrap_or(query).trim().to_string();
        if self.store.ws_query.borrow().as_deref() == Some(q.as_str()) {
            return;
        }
        *self.store.ws_query.borrow_mut() = Some(q.clone());
        let sig = self.h.ws_symbols;
        let reply = self
            .store
            .ws_gate
            .wrap(move |v: Vec<lsp::SymbolInformation>| sig.set(v));
        if !svc.workspace_symbols(&q, reply) {
            sig.set(Vec::new());
        }
    }

    /// Forget the last workspace query (the palette closed).
    pub(crate) fn reset_workspace_symbols(&self) {
        *self.store.ws_query.borrow_mut() = None;
    }

    /// The `#` mode palette commands (accepting reveals the symbol, in another document
    /// through the host's `open_location`).
    pub(crate) fn workspace_symbol_commands(&self) -> Vec<Command> {
        self.h
            .ws_symbols
            .peek()
            .into_iter()
            .enumerate()
            .map(|(i, s)| {
                let me = self.clone();
                let file = s.location.uri.rsplit('/').next().unwrap_or("").to_string();
                let mut title = s.name.clone();
                if let Some(c) = &s.container_name {
                    title.push_str(&format!("  {c}"));
                }
                title.push_str(&format!("  \u{2014} {file}"));
                Command::new(format!("workspaceSymbol.{i}"), title, move |_| {
                    let mut loc = s.location.clone();
                    loc.range.end = loc.range.start;
                    me.go_to(&loc);
                })
            })
            .collect()
    }

    /// The live-preview range highlight (whole symbol, line backgrounds).
    pub(crate) fn preview_layers(&self, f: &crate::view::Frame, layers: &mut Vec<AnyWidget>) {
        let Some((a, b)) = self.h.sym_preview.get() else {
            return;
        };
        let (la, lb) = (f.li.line_of(f.src, a), f.li.line_of(f.src, b));
        let color = crate::view::chrome::with_alpha(f.theme.accent, 0.14);
        for line in la.max(f.first_line)..=lb.min(f.last_line) {
            if !f.line_visible(line) {
                continue;
            }
            for (row, _) in f.segments(line) {
                layers.push(crate::view::chrome::band(f.row_y(row), f.line_px, color));
            }
        }
    }
}
