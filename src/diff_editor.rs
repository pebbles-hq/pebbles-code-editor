//! The diff editor (VS Code's diff editor): the base ("original") and current ("modified")
//! texts side by side, line-aligned with filler rows and scrolled together, or a single
//! inline view. Removed lines and words are red, inserted words green; the modified side
//! keeps the change bars with next/previous change (Alt+F3) and per-hunk revert (click a
//! bar → peek → Revert).

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
}

/// A side-by-side diff editor of `modified` against `original`.
pub fn diff_editor(original: Signal<String>, modified: Signal<String>) -> DiffEditor {
    DiffEditor {
        original,
        modified,
        side_by_side: true,
        height: 400.0,
        configure: None,
    }
}

impl DiffEditor {
    /// Side by side (default) or a single inline view (`false`).
    pub fn side_by_side(mut self, v: bool) -> Self {
        self.side_by_side = v;
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
    let modified = cfg(code_editor(d.modified))
        .height(d.height)
        .soft_wrap(false)
        .diff_base(d.original);
    if !d.side_by_side {
        return modified.inline_diff(true).into_widget();
    }
    let original = cfg(code_editor(d.original))
        .height(d.height)
        .soft_wrap(false)
        .read_only(true)
        .minimap(false)
        .diff_original_of(d.modified)
        .diff_align(true)
        .scroll_link(link);
    let modified = modified.diff_align(true).scroll_link(link);
    row(children![
        expanded(original),
        container().width(1.0).height(d.height),
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
