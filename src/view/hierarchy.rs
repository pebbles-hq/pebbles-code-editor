//! Call hierarchy (Shift+Alt+H) and type hierarchy in an inline peek tree: the prepared
//! item is the root; expanding a node asks for its incoming / outgoing calls (supertypes /
//! subtypes); the direction toggles. Up / Down select, Right / Left expand / collapse, Enter
//! reveals the selected item (in another document through the host), Escape closes.

use pebbles::prelude::*;

use crate::language::{self as lsp};
use crate::view::chrome::with_alpha;
use crate::view::lsp::Lsp;

/// Which hierarchy and which direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HierKind {
    /// Calls into the item.
    Incoming,
    /// Calls out of the item.
    Outgoing,
    /// The item's supertypes.
    Supertypes,
    /// The item's subtypes.
    Subtypes,
}

impl HierKind {
    fn title(self) -> &'static str {
        match self {
            HierKind::Incoming => "Incoming Calls",
            HierKind::Outgoing => "Outgoing Calls",
            HierKind::Supertypes => "Supertypes",
            HierKind::Subtypes => "Subtypes",
        }
    }
    fn flipped(self) -> Self {
        match self {
            HierKind::Incoming => HierKind::Outgoing,
            HierKind::Outgoing => HierKind::Incoming,
            HierKind::Supertypes => HierKind::Subtypes,
            HierKind::Subtypes => HierKind::Supertypes,
        }
    }
}

/// A tree node: the item, its call sites, children once loaded, expanded or not.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HierNode {
    pub(crate) item: lsp::CallHierarchyItem,
    pub(crate) ranges: Vec<lsp::Range>,
    pub(crate) children: Option<Vec<HierNode>>,
    pub(crate) expanded: bool,
}

impl HierNode {
    fn new(item: lsp::CallHierarchyItem, ranges: Vec<lsp::Range>) -> Self {
        HierNode {
            item,
            ranges,
            children: None,
            expanded: false,
        }
    }
}

/// The open hierarchy peek.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HierView {
    pub(crate) line: usize,
    pub(crate) kind: HierKind,
    pub(crate) root: HierNode,
    /// The selected visible row.
    pub(crate) sel: usize,
}

/// The visible rows: (path from the root, depth).
fn rows(root: &HierNode) -> Vec<(Vec<usize>, usize)> {
    fn walk(n: &HierNode, path: Vec<usize>, out: &mut Vec<(Vec<usize>, usize)>) {
        out.push((path.clone(), path.len()));
        if n.expanded
            && let Some(c) = &n.children
        {
            for (i, ch) in c.iter().enumerate() {
                let mut p = path.clone();
                p.push(i);
                walk(ch, p, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, Vec::new(), &mut out);
    out
}

fn node_at<'a>(root: &'a mut HierNode, path: &[usize]) -> Option<&'a mut HierNode> {
    let mut n = root;
    for &i in path {
        n = n.children.as_mut()?.get_mut(i)?;
    }
    Some(n)
}

impl Lsp {
    /// Shift+Alt+H (`call = true`) / Show Type Hierarchy: prepare at the caret and open the
    /// peek with the root expanded.
    pub(crate) fn show_hierarchy(&self, call: bool) -> bool {
        let Some(svc) = self.svc.clone() else {
            return false;
        };
        let head = self.state.peek().primary().head;
        let line = crate::geometry::line_of(&self.state.peek().text(), head);
        let me = self.clone();
        let reply = self.guard(
            &self.store.hier_gate,
            move |items: Vec<lsp::CallHierarchyItem>| {
                let Some(item) = items.into_iter().next() else {
                    return;
                };
                let kind = if call {
                    HierKind::Incoming
                } else {
                    HierKind::Supertypes
                };
                me.h.hier.set(Some(HierView {
                    line,
                    kind,
                    root: HierNode::new(item, Vec::new()),
                    sel: 0,
                }));
                me.expand_hier(Vec::new());
            },
        );
        self.with_doc(|doc, text| {
            let pos = lsp::byte_to_position(text, head);
            if call {
                svc.prepare_call_hierarchy(doc, pos, reply)
            } else {
                svc.prepare_type_hierarchy(doc, pos, reply)
            }
        })
    }

    /// Flip incoming / outgoing (supertypes / subtypes): the root is re-expanded.
    pub(crate) fn toggle_hier_direction(&self) -> bool {
        let Some(mut v) = self.h.hier.peek() else {
            return false;
        };
        v.kind = v.kind.flipped();
        v.root.children = None;
        v.root.expanded = false;
        v.sel = 0;
        self.h.hier.set(Some(v));
        self.expand_hier(Vec::new());
        true
    }

    /// Expand the node at `path`, loading its children on first expansion.
    pub(crate) fn expand_hier(&self, path: Vec<usize>) {
        let Some(mut v) = self.h.hier.peek() else {
            return;
        };
        let kind = v.kind;
        let Some(n) = node_at(&mut v.root, &path) else {
            return;
        };
        n.expanded = true;
        if n.children.is_some() {
            self.h.hier.set(Some(v));
            return;
        }
        let item = n.item.clone();
        self.h.hier.set(Some(v));
        let Some(svc) = self.svc.clone() else { return };
        let hier = self.h.hier;
        let fill = move |children: Vec<HierNode>| {
            let Some(mut v) = hier.peek() else { return };
            if v.kind != kind {
                return;
            }
            if let Some(n) = node_at(&mut v.root, &path) {
                n.children = Some(children);
                hier.set(Some(v));
            }
        };
        // Each node's request is independent (no shared gate), guarded by the version.
        let at = self.version();
        let me = self.clone();
        let ok = move || me.version() == at;
        match kind {
            HierKind::Incoming => {
                svc.incoming_calls(
                    &item,
                    Box::new(move |v: Vec<lsp::CallHierarchyIncomingCall>| {
                        if ok() {
                            fill(
                                v.into_iter()
                                    .map(|c| HierNode::new(c.from, c.from_ranges))
                                    .collect(),
                            );
                        }
                    }),
                );
            }
            HierKind::Outgoing => {
                svc.outgoing_calls(
                    &item,
                    Box::new(move |v: Vec<lsp::CallHierarchyOutgoingCall>| {
                        if ok() {
                            fill(
                                v.into_iter()
                                    .map(|c| HierNode::new(c.to, c.from_ranges))
                                    .collect(),
                            );
                        }
                    }),
                );
            }
            HierKind::Supertypes | HierKind::Subtypes => {
                let reply: lsp::Reply<Vec<lsp::TypeHierarchyItem>> =
                    Box::new(move |v: Vec<lsp::TypeHierarchyItem>| {
                        if ok() {
                            fill(
                                v.into_iter()
                                    .map(|i| HierNode::new(i, Vec::new()))
                                    .collect(),
                            );
                        }
                    });
                if kind == HierKind::Supertypes {
                    svc.supertypes(&item, reply);
                } else {
                    svc.subtypes(&item, reply);
                }
            }
        }
    }

    /// Reveal the node at `path` (its name).
    fn open_hier(&self, path: &[usize]) {
        let Some(mut v) = self.h.hier.peek() else {
            return;
        };
        let Some(n) = node_at(&mut v.root, path) else {
            return;
        };
        let at = n.item.selection_range.start;
        let loc = lsp::Location {
            uri: n.item.uri.clone(),
            range: lsp::Range::new(at, at),
        };
        self.h.hier.set(None);
        self.go_to(&loc);
    }

    /// Keys while the hierarchy peek is open. Returns whether consumed.
    pub(crate) fn hier_key(&self, k: &KeyInput) -> bool {
        let Some(mut v) = self.h.hier.peek() else {
            return false;
        };
        let visible = rows(&v.root);
        let sel = v.sel.min(visible.len().saturating_sub(1));
        let path = visible.get(sel).map(|r| r.0.clone()).unwrap_or_default();
        match k {
            KeyInput::Escape => self.h.hier.set(None),
            KeyInput::Enter => self.open_hier(&path),
            KeyInput::Move { motion, .. } => match motion {
                Motion::Down => {
                    v.sel = (sel + 1) % visible.len().max(1);
                    self.h.hier.set(Some(v));
                }
                Motion::Up => {
                    let n = visible.len().max(1);
                    v.sel = (sel + n - 1) % n;
                    self.h.hier.set(Some(v));
                }
                Motion::Right => self.expand_hier(path),
                Motion::Left => {
                    if let Some(n) = node_at(&mut v.root, &path)
                        && n.expanded
                    {
                        n.expanded = false;
                    } else if !path.is_empty() {
                        // Collapsed already: select the parent.
                        let parent = &path[..path.len() - 1];
                        v.sel = visible.iter().position(|r| r.0 == parent).unwrap_or(0);
                    }
                    self.h.hier.set(Some(v));
                }
                _ => return false,
            },
            _ => return false,
        }
        true
    }

    /// The hierarchy peek as a block widget below its line.
    pub(crate) fn hier_block(
        &self,
        t: &crate::theme::EditorTheme,
        line_px: f64,
    ) -> Option<crate::extensions::BlockWidget> {
        const ROWS: usize = 10;
        let v = self.h.hier.get()?;
        let visible = rows(&v.root);
        let sel = v.sel.min(visible.len().saturating_sub(1));
        let hier = self.h.hier;
        let me = self.clone();
        let dir_label = format!("Show {}", v.kind.flipped().title());
        let header = row(vec![
            t.ui_text(v.kind.title(), 0.0)
                .color(t.foreground)
                .into_widget(),
            gap_w(8.0).into_widget(),
            t.ui_text(v.root.item.name.clone(), 0.0)
                .color(t.muted)
                .into_widget(),
            expanded(gap_w(1.0)).into_widget(),
            button(dir_label)
                .size(ButtonSize::Sm)
                .on_pressed(move || {
                    me.toggle_hier_direction();
                    me.focus.request_focus();
                })
                .into_widget(),
            gap_w(6.0).into_widget(),
            icon_button(IconKind::Close)
                .icon_size(16.0)
                .on_pressed(move || hier.set(None))
                .into_widget(),
        ])
        .cross_axis_alignment(CrossAxisAlignment::Center);
        let first = sel.saturating_sub(ROWS - 1);
        let mut list: Vec<AnyWidget> = Vec::new();
        let mut root = v.root.clone();
        for (i, (path, depth)) in visible.iter().enumerate().skip(first).take(ROWS) {
            let Some(n) = node_at(&mut root, path) else {
                continue;
            };
            let chevron = match (&n.children, n.expanded) {
                (Some(c), _) if c.is_empty() => "  ",
                (_, true) => "\u{25BE} ",
                _ => "\u{25B8} ",
            };
            let mut label = format!("{}{}{}", "    ".repeat(*depth), chevron, n.item.name);
            if let Some(d) = &n.item.detail {
                label.push_str(&format!("  {d}"));
            }
            if !n.ranges.is_empty() && n.ranges.len() > 1 {
                label.push_str(&format!("  ({} calls)", n.ranges.len()));
            }
            let file = n.item.uri.rsplit('/').next().unwrap_or("").to_string();
            let bg = if i == sel {
                t.selection
            } else {
                Color::from_rgba8(0, 0, 0, 0)
            };
            let (me1, me2, p1, p2) = (self.clone(), self.clone(), path.clone(), path.clone());
            list.push(
                semantics(
                    SemanticsRole::Button,
                    format!("Hierarchy {}", n.item.name),
                    GestureDetector::new(
                        container()
                            .height(line_px)
                            .padding(EdgeInsets::symmetric(6.0, 0.0))
                            .decoration(BoxDecoration::new().color(bg))
                            .alignment(Alignment::CENTER_LEFT)
                            .child(
                                row(vec![
                                    t.ui_text(label, 0.0).color(t.foreground).into_widget(),
                                    gap_w(8.0).into_widget(),
                                    t.ui_text(file, -1.0).color(t.muted).into_widget(),
                                ])
                                .cross_axis_alignment(CrossAxisAlignment::Center),
                            ),
                    )
                    .cursor(Cursor::Pointer)
                    .on_tap(action(move || {
                        // Select the row and toggle its expansion.
                        if let Some(mut v) = me1.h.hier.peek() {
                            v.sel = i;
                            let open = node_at(&mut v.root, &p1).is_some_and(|n| n.expanded);
                            if open {
                                if let Some(n) = node_at(&mut v.root, &p1) {
                                    n.expanded = false;
                                }
                                me1.h.hier.set(Some(v));
                            } else {
                                me1.h.hier.set(Some(v));
                                me1.expand_hier(p1.clone());
                            }
                        }
                    }))
                    .on_double_tap(action(move || me2.open_hier(&p2))),
                )
                .into_widget(),
            );
        }
        let height = line_px * (ROWS as f64 + 1.6) + 12.0;
        let panel = container()
            .height(height)
            .padding(EdgeInsets::all(6.0))
            .decoration(
                BoxDecoration::new()
                    .color(t.overlay_bg)
                    .border(Border::new(with_alpha(t.accent, 1.0), 1.0)),
            )
            .child(
                column(vec![
                    header.into_widget(),
                    gap_h(4.0).into_widget(),
                    expanded(
                        container()
                            .clip()
                            .child(column(list).main_axis_size(MainAxisSize::Min)),
                    )
                    .into_widget(),
                ])
                .cross_axis_alignment(CrossAxisAlignment::Stretch),
            );
        Some(crate::extensions::BlockWidget {
            line: v.line,
            height,
            widget: panel.into_widget(),
        })
    }
}
