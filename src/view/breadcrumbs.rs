//! Breadcrumbs (`CodeEditor::breadcrumbs`): the host's path segments followed by the chain
//! of document symbols containing the caret. Each segment opens a dropdown (a palette) of
//! its siblings — and, for a symbol, its children; Ctrl+Shift+; focuses the bar
//! (Left / Right move, Enter / Down open, Escape leaves), Ctrl+Shift+. focuses it and opens
//! the last segment's dropdown (VS Code `breadcrumbs.focus` / `breadcrumbs.focusAndSelect`).

use pebbles::prelude::*;

use crate::config::BreadcrumbSegment;
use crate::extensions::Command;
use crate::language::{self as lsp};
use crate::view::lsp::Lsp;

impl Lsp {
    /// The symbol chain at the caret (outermost first).
    fn crumb_chain(&self) -> Vec<lsp::DocumentSymbol> {
        let syms = self.h.symbols.peek();
        let st = self.state.peek();
        let text = st.text();
        let pos = lsp::byte_to_position(&text, st.primary().head);
        lsp::symbol_path(&syms, pos).into_iter().cloned().collect()
    }

    /// The segment labels: path, then symbols.
    pub(crate) fn crumb_labels(&self) -> Vec<String> {
        self.crumbs
            .path
            .iter()
            .map(|s| s.label.clone())
            .chain(self.crumb_chain().into_iter().map(|s| s.name))
            .collect()
    }

    /// Open segment `i`'s dropdown, its current entry preselected.
    pub(crate) fn open_crumb(&self, i: usize) {
        let (cmds, sel) = self.crumb_commands(i);
        if cmds.is_empty() {
            return;
        }
        self.h.crumb_open.set(Some(i));
        self.h.crumb_menu.query.set(String::new());
        self.h.crumb_menu.sel.set(sel);
        self.h.crumb_menu.open.set(true);
        pebbles::core::focus::set_focus(None);
    }

    /// Ctrl+Shift+; (`open = false`) / Ctrl+Shift+. (`open = true`).
    pub(crate) fn focus_breadcrumbs(&self, open: bool) -> bool {
        if !self.crumbs.enabled {
            return false;
        }
        let n = self.crumb_labels().len();
        if n == 0 {
            return false;
        }
        self.h.crumb_focus.set(Some(n - 1));
        if open {
            self.open_crumb(n - 1);
        }
        true
    }

    /// A key while the bar is focused. Returns whether it was consumed.
    pub(crate) fn crumb_key(&self, key: &str) -> bool {
        let Some(i) = self.h.crumb_focus.peek() else {
            return false;
        };
        let n = self.crumb_labels().len();
        match key {
            "Left" => self.h.crumb_focus.set(Some(i.saturating_sub(1))),
            "Right" => self
                .h
                .crumb_focus
                .set(Some((i + 1).min(n.saturating_sub(1)))),
            "Enter" | "Down" | "Space" => self.open_crumb(i.min(n.saturating_sub(1))),
            "Escape" => self.h.crumb_focus.set(None),
            _ => {
                self.h.crumb_focus.set(None);
                return false;
            }
        }
        true
    }

    /// The dropdown entries of segment `i` and the index of the current one.
    pub(crate) fn crumb_commands(&self, i: usize) -> (Vec<Command>, usize) {
        let path = &self.crumbs.path;
        if let Some(seg) = path.get(i) {
            let pick = self.crumbs.on_pick.clone();
            let sel = seg
                .siblings
                .iter()
                .position(|s| *s == seg.label)
                .unwrap_or(0);
            let cmds = seg
                .siblings
                .iter()
                .enumerate()
                .map(|(k, name)| {
                    let (pick, name2, me) = (pick.clone(), name.clone(), self.clone());
                    Command::new(format!("breadcrumb.{i}.{k}"), name.clone(), move |_| {
                        me.h.crumb_focus.set(None);
                        if let Some(p) = &pick {
                            p(i, &name2);
                        }
                    })
                })
                .collect();
            return (cmds, sel);
        }
        let d = i - path.len();
        let chain = self.crumb_chain();
        let syms = self.h.symbols.peek();
        let level: &[lsp::DocumentSymbol] = match d {
            0 => &syms,
            _ => match chain.get(d - 1) {
                Some(p) => &p.children,
                None => return (Vec::new(), 0),
            },
        };
        let mut cmds = Vec::new();
        let mut sel = 0;
        for s in level {
            let current = chain.get(d) == Some(s);
            if current {
                sel = cmds.len();
            }
            cmds.push(self.reveal_command(s, 0));
            if current {
                cmds.extend(s.children.iter().map(|c| self.reveal_command(c, 1)));
            }
        }
        (cmds, sel)
    }

    fn reveal_command(&self, s: &lsp::DocumentSymbol, indent: usize) -> Command {
        let me = self.clone();
        let at = s.selection_range.start;
        Command::new(
            format!("breadcrumb.symbol.{}", s.name),
            format!("{}{}", "  ".repeat(indent), s.name),
            move |_| {
                me.h.crumb_focus.set(None);
                let uri = me.uri.to_string();
                me.go_to(&lsp::Location {
                    uri,
                    range: lsp::Range::new(at, at),
                });
            },
        )
    }

    /// The commands for the open dropdown.
    pub(crate) fn open_crumb_commands(&self) -> Vec<Command> {
        match self.h.crumb_open.peek() {
            Some(i) => self.crumb_commands(i).0,
            None => Vec::new(),
        }
    }

    /// The breadcrumbs bar.
    pub(crate) fn breadcrumb_bar(&self, t: &crate::theme::EditorTheme) -> AnyWidget {
        let _ = self.h.symbols.get();
        let focus = self.h.crumb_focus.get();
        let labels = self.crumb_labels();
        let mut items: Vec<AnyWidget> = Vec::new();
        for (i, label) in labels.iter().enumerate() {
            if i > 0 {
                items.push(t.ui_text(" \u{203A} ", 0.0).color(t.muted).into_widget());
            }
            let bg = if focus == Some(i) {
                t.selection
            } else {
                Color::from_rgba8(0, 0, 0, 0)
            };
            let me = self.clone();
            items.push(
                semantics(
                    SemanticsRole::Button,
                    format!("Breadcrumb {label}"),
                    GestureDetector::new(
                        container()
                            .padding(EdgeInsets::symmetric(3.0, 1.0))
                            .decoration(
                                BoxDecoration::new()
                                    .color(bg)
                                    .radius(BorderRadius::all(3.0)),
                            )
                            .child(t.ui_text(label.clone(), 0.0).color(t.foreground)),
                    )
                    .cursor(Cursor::Pointer)
                    .on_tap(action(move || {
                        me.h.crumb_focus.set(Some(i));
                        me.open_crumb(i);
                    })),
                )
                .into_widget(),
            );
        }
        container()
            .height(22.0)
            .padding(EdgeInsets::symmetric(10.0, 0.0))
            .decoration(BoxDecoration::new().color(t.background))
            .alignment(Alignment::CENTER_LEFT)
            .child(row(items).cross_axis_alignment(CrossAxisAlignment::Center))
            .into_widget()
    }
}

/// Host breadcrumb config carried by the controller.
#[derive(Clone, Default)]
pub(crate) struct Crumbs {
    pub(crate) enabled: bool,
    pub(crate) path: std::rc::Rc<Vec<BreadcrumbSegment>>,
    pub(crate) on_pick: Option<crate::config::BreadcrumbPick>,
}
