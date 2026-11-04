//! Host-contributed annotations and menu items: end-of-line "after text" annotations
//! (GitLens inline blame, error lens), a host gutter column (blame view), and items merged
//! into the editor's right-click menu (VS Code's `editor/context` menu contribution point).

use std::rc::Rc;

use pebbles::prelude::Color;

/// An action button in an annotation's hover card.
#[derive(Clone)]
pub struct AnnotationAction {
    /// The button label (e.g. "Open Commit in Graph").
    pub label: String,
    /// Run when the button is pressed (the card then closes).
    pub run: Rc<dyn Fn()>,
}

impl AnnotationAction {
    /// An action labelled `label` running `run`.
    pub fn new(label: impl Into<String>, run: impl Fn() + 'static) -> Self {
        AnnotationAction {
            label: label.into(),
            run: Rc::new(run),
        }
    }
}

/// The hover card of a [`LineAnnotation`]: a bold title, body lines and action buttons.
#[derive(Clone, Default)]
pub struct AnnotationHover {
    /// The header (e.g. "Ada Lovelace, 3 days ago (Mon, Nov 2 2026 10:12)").
    pub title: String,
    /// Body lines (e.g. the SHA and the full commit message).
    pub body: Vec<String>,
    /// Buttons along the card's bottom.
    pub actions: Vec<AnnotationAction>,
}

/// An end-of-line "after text" annotation: faded text after the last character of `line`
/// (VS Code's `after` decoration / GitLens' trailing blame). It takes no space in the
/// layout (the text does not reflow) and hovering it shows `hover`.
#[derive(Clone)]
pub struct LineAnnotation {
    /// The 0-based buffer line.
    pub line: usize,
    /// The annotation text.
    pub text: String,
    /// The card shown while the annotation is hovered.
    pub hover: Option<AnnotationHover>,
}

impl LineAnnotation {
    /// `text` after `line`.
    pub fn new(line: usize, text: impl Into<String>) -> Self {
        LineAnnotation {
            line,
            text: text.into(),
            hover: None,
        }
    }
    /// Attach a hover card.
    pub fn hover(mut self, hover: AnnotationHover) -> Self {
        self.hover = Some(hover);
        self
    }
}

/// One line's cell in a host gutter column ([`GutterAnnotations`]).
#[derive(Clone)]
pub struct GutterAnnotation {
    /// The 0-based buffer line.
    pub line: usize,
    /// The cell text (empty for continuation lines of a group).
    pub text: String,
    /// The secondary, right-aligned text (e.g. the relative age).
    pub detail: String,
    /// The bar at the cell's left edge (e.g. an age heatmap colour).
    pub bar: Color,
    /// The first line of a group (a hairline separates groups).
    pub group_start: bool,
    /// Run on click.
    pub on_click: Option<Rc<dyn Fn()>>,
}

/// A host gutter column left of the line numbers (VS Code's / GitLens' blame annotations).
#[derive(Clone)]
pub struct GutterAnnotations {
    /// The column width in logical px.
    pub width: f64,
    /// The cells (any order; lines without a cell are blank).
    pub items: Vec<GutterAnnotation>,
}

/// A breakpoint in the glyph margin ([`CodeEditor::breakpoint_marks`]): VS Code's
/// breakpoint decorations. The glyph follows the kind — a dot (plain), a dot with a bar
/// (conditional / hit count), a diamond (logpoint) — drawn grey when disabled and hollow
/// when the debugger could not verify it.
///
/// [`CodeEditor::breakpoint_marks`]: crate::CodeEditor::breakpoint_marks
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BreakpointMark {
    /// Stop only when this expression is true.
    pub condition: Option<String>,
    /// Stop only when the hit count satisfies this (`5`, `>3`, `%2`).
    pub hit_condition: Option<String>,
    /// A logpoint: print this message instead of stopping.
    pub log_message: Option<String>,
    /// Off: kept but not sent to the debugger (drawn grey).
    pub enabled: bool,
    /// The debugger confirmed it (`true` while no session runs).
    pub verified: bool,
    /// Why it is unverified / what the debugger said (hover text).
    pub message: Option<String>,
}

impl Default for BreakpointMark {
    fn default() -> Self {
        BreakpointMark {
            condition: None,
            hit_condition: None,
            log_message: None,
            enabled: true,
            verified: true,
            message: None,
        }
    }
}

impl BreakpointMark {
    /// A logpoint.
    pub fn is_log(&self) -> bool {
        self.log_message.is_some()
    }
    /// A conditional (or hit-count) breakpoint.
    pub fn is_conditional(&self) -> bool {
        self.condition.is_some() || self.hit_condition.is_some()
    }
}

/// A line of the call stack shown in the editor ([`CodeEditor::stack_frame_lines`]): the
/// top frame (where execution is paused) is highlighted yellow with an arrow, an outer
/// frame selected in the call stack green.
///
/// [`CodeEditor::stack_frame_lines`]: crate::CodeEditor::stack_frame_lines
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StackFrameLine {
    /// The 0-based line.
    pub line: usize,
    /// The top frame (the current execution point).
    pub top: bool,
}

/// Items for the glyph margin's right-click menu on a line (`CodeEditor::breakpoint_menu`).
pub type BreakpointMenu = Rc<dyn Fn(usize) -> Vec<EditorMenuItem>>;

/// What the editor knows when its context menu is built or an item runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MenuContext {
    /// The primary caret `(line, column)` (0-based; columns in chars).
    pub caret: (usize, usize),
    /// The primary selection's lines `(first, last)` — `None` for a bare caret. A selection
    /// ending at a line start does not include that line.
    pub selected_lines: Option<(usize, usize)>,
    /// The primary selection's text.
    pub selected_text: String,
    /// The editor is read-only.
    pub read_only: bool,
}

/// An item contributed to the editor's context menu. Items are sorted into `group`s
/// (VS Code's `group` strings: `"navigation"` first, the others by name, a separator
/// between groups) and by `order` within a group. The editor's own items sit in
/// `"9_cutcopypaste"` (Cut, Copy, Paste: orders 1–3) and `"9z_select"` (Select All).
#[derive(Clone)]
pub struct EditorMenuItem {
    /// The group the item belongs to.
    pub group: String,
    /// The order within the group.
    pub order: i32,
    /// The row label.
    pub label: String,
    /// A keyboard-shortcut hint.
    pub shortcut: Option<String>,
    /// Dimmed and not selectable when `false`.
    pub enabled: bool,
    /// Run with the editor's state when chosen.
    pub run: Rc<dyn Fn(&MenuContext)>,
}

impl EditorMenuItem {
    /// An item in `group` labelled `label`.
    pub fn new(
        group: impl Into<String>,
        label: impl Into<String>,
        run: impl Fn(&MenuContext) + 'static,
    ) -> Self {
        EditorMenuItem {
            group: group.into(),
            order: 0,
            label: label.into(),
            shortcut: None,
            enabled: true,
            run: Rc::new(run),
        }
    }
    /// The order within the group.
    pub fn order(mut self, order: i32) -> Self {
        self.order = order;
        self
    }
    /// A keyboard-shortcut hint.
    pub fn shortcut(mut self, s: impl Into<String>) -> Self {
        self.shortcut = Some(s.into());
        self
    }
    /// Enable or disable the item.
    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }
}

/// The host's menu contribution (`CodeEditor::context_menu_items`): called with the editor's
/// state when the menu is built.
pub type MenuProvider = Rc<dyn Fn(&MenuContext) -> Vec<EditorMenuItem>>;

/// Sort `items` into menu rows: groups (`"navigation"` first, then by name), items by
/// `order` (stable), `None` marking a separator between groups.
pub fn arrange(mut items: Vec<EditorMenuItem>) -> Vec<Option<EditorMenuItem>> {
    let key = |g: &str| (g != "navigation", g.to_string());
    items.sort_by(|a, b| {
        key(&a.group)
            .cmp(&key(&b.group))
            .then(a.order.cmp(&b.order))
    });
    let mut out = Vec::new();
    let mut last: Option<String> = None;
    for it in items {
        if last.as_ref().is_some_and(|g| *g != it.group) {
            out.push(None);
        }
        last = Some(it.group.clone());
        out.push(Some(it));
    }
    out
}

/// The [`MenuContext`] of `text` with the primary selection `(anchor, head)` bytes.
pub(crate) fn menu_context(text: &str, anchor: usize, head: usize, read_only: bool) -> MenuContext {
    let (a, b) = (
        anchor.min(head).min(text.len()),
        anchor.max(head).min(text.len()),
    );
    let line = |at: usize| text[..at].matches('\n').count();
    let col = |at: usize| {
        let ls = text[..at].rfind('\n').map_or(0, |i| i + 1);
        text[ls..at].chars().count()
    };
    let head = head.min(text.len());
    let selected_lines = (a != b).then(|| {
        let end = if text.as_bytes().get(b - 1) == Some(&b'\n') && b > a {
            b - 1
        } else {
            b
        };
        (line(a), line(end.max(a)))
    });
    MenuContext {
        caret: (line(head), col(head)),
        selected_lines,
        selected_text: text.get(a..b).unwrap_or_default().to_string(),
        read_only,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrange_groups_navigation_first_and_separates() {
        let it = |g: &str, l: &str, o: i32| EditorMenuItem::new(g, l, |_| {}).order(o);
        let rows = arrange(vec![
            it("9_cutcopypaste", "Copy", 2),
            it("2_git", "Open Changes", 1),
            it("9_cutcopypaste", "Cut", 1),
            it("navigation", "Go to Definition", 1),
        ]);
        let labels: Vec<String> = rows
            .iter()
            .map(|r| r.as_ref().map_or("--".into(), |i| i.label.clone()))
            .collect();
        assert_eq!(
            labels,
            [
                "Go to Definition",
                "--",
                "Open Changes",
                "--",
                "Cut",
                "Copy"
            ]
        );
    }

    #[test]
    fn menu_context_lines_and_caret() {
        let t = "ab\ncd\nef\n";
        let c = menu_context(t, 3, 3, false);
        assert_eq!(c.caret, (1, 0));
        assert_eq!(c.selected_lines, None);
        // A selection ending at a line start excludes that line.
        let c = menu_context(t, 0, 6, true);
        assert_eq!(c.selected_lines, Some((0, 1)));
        assert_eq!(c.selected_text, "ab\ncd\n");
        assert!(c.read_only);
        let c = menu_context(t, 7, 1, false);
        assert_eq!(c.selected_lines, Some((0, 2)));
        assert_eq!(c.caret, (0, 1));
    }
}
