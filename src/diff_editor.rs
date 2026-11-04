//! The diff editor (VS Code's diff editor): the base ("original") and current ("modified")
//! texts side by side, line-aligned with filler rows and scrolled together, or a single
//! inline view. Removed lines and words are red, inserted words green; the modified side
//! keeps the change bars with next/previous change (Alt+F5 / Alt+F3) and per-hunk revert
//! (click a bar → peek → Revert), plus optional per-hunk gutter buttons
//! ([`DiffEditor::hunk_actions`]), whitespace-insensitive comparison and collapsed
//! unchanged regions. The modified side is editable (unless
//! [`DiffEditor::read_only`]) and re-diffs live while typing.

use std::rc::Rc;

use pebbles::prelude::*;

use crate::config::{CodeEditor, code_editor};

/// Configure each side's [`CodeEditor`] (language, theme, fonts, …).
pub type DiffSideConfig = Rc<dyn Fn(CodeEditor) -> CodeEditor>;

/// A diff editor between `original` (read-only) and `modified` (editable).
#[derive(Clone)]
pub struct DiffEditor {
    original: Signal<String>,
    modified: Signal<String>,
    side_by_side: bool,
    height: f64,
    configure: Option<DiffSideConfig>,
    ignore_whitespace: bool,
    collapse_unchanged: bool,
    read_only: bool,
    hunk_actions: Vec<crate::config::DiffHunkAction>,
}

/// A side-by-side diff editor of `modified` against `original`.
pub fn diff_editor(original: Signal<String>, modified: Signal<String>) -> DiffEditor {
    DiffEditor {
        original,
        modified,
        side_by_side: true,
        height: 400.0,
        configure: None,
        ignore_whitespace: false,
        collapse_unchanged: false,
        read_only: false,
        hunk_actions: Vec::new(),
    }
}

impl DiffEditor {
    /// Side by side (default) or a single inline view (`false`).
    pub fn side_by_side(mut self, v: bool) -> Self {
        self.side_by_side = v;
        self
    }
    /// Ignore leading/trailing whitespace changes (`diffEditor.ignoreTrimWhitespace`).
    pub fn ignore_whitespace(mut self, v: bool) -> Self {
        self.ignore_whitespace = v;
        self
    }
    /// Collapse unchanged regions into expandable "N hidden lines" bars
    /// (`diffEditor.hideUnchangedRegions.enabled`).
    pub fn collapse_unchanged(mut self, v: bool) -> Self {
        self.collapse_unchanged = v;
        self
    }
    /// Make the modified side read-only (e.g. comparing two revisions).
    pub fn read_only(mut self, v: bool) -> Self {
        self.read_only = v;
        self
    }
    /// Per-hunk gutter buttons on the modified side (Stage / Revert / Unstage Change).
    pub fn hunk_actions(mut self, actions: Vec<crate::config::DiffHunkAction>) -> Self {
        self.hunk_actions = actions;
        self
    }
    /// The viewport height of the editors.
    pub fn height(mut self, h: f64) -> Self {
        self.height = h;
        self
    }
    /// Apply `f` to each side's editor (language, theme, font, …).
    pub fn configure(mut self, f: impl Fn(CodeEditor) -> CodeEditor + 'static) -> Self {
        self.configure = Some(Rc::new(f));
        self
    }
}

fn render_diff(d: &DiffEditor) -> AnyWidget {
    let link = create_signal(0.0_f64);
    let cfg = |e: CodeEditor| match &d.configure {
        Some(f) => f(e),
        None => e,
    };
    let open_regions = create_signal(std::collections::BTreeSet::<usize>::new());
    let diff_opts = |e: CodeEditor| {
        let e = e.diff_ignore_whitespace(d.ignore_whitespace);
        if d.collapse_unchanged {
            e.diff_collapse_unchanged(open_regions)
        } else {
            e
        }
    };
    let mut modified = diff_opts(cfg(code_editor(d.modified)))
        .height(d.height)
        .soft_wrap(false)
        .diff_base(d.original)
        .diff_hunk_actions(d.hunk_actions.clone());
    if d.read_only {
        modified = modified.read_only(true);
    }
    if !d.side_by_side {
        return modified.inline_diff(true).into_widget();
    }
    let original = diff_opts(cfg(code_editor(d.original)))
        .height(d.height)
        .soft_wrap(false)
        .read_only(true)
        .minimap(false)
        .diff_original_of(d.modified)
        .diff_align(true)
        .scroll_link(link);
    let modified = modified.diff_align(true).scroll_link(link);
    // A 1px divider between the panes in the theme's border colour (VS Code's
    // `diffEditor.border`), so the two sides read as separate panes.
    let divider = original.effective_theme().border;
    row(children![
        expanded(original),
        container().width(1.0).height(d.height).color(divider),
        expanded(modified),
    ])
    .cross_axis_alignment(CrossAxisAlignment::Start)
    .into_widget()
}

impl IntoWidget for DiffEditor {
    fn into_widget(self) -> AnyWidget {
        component_props(render_diff, self).into_widget()
    }
}
