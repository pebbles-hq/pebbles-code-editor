//! Instrumentation for performance tests: how often the editor's top-level component
//! re-rendered (its whole tree). Scrolling and the caret blink must not move this.

use std::cell::Cell;

thread_local! {
    static RENDERS: Cell<u64> = const { Cell::new(0) };
}

/// Total `render_editor` runs on this thread (cumulative; diff it around a window).
pub fn render_count() -> u64 {
    RENDERS.with(Cell::get)
}

pub(crate) fn bump_render() {
    RENDERS.with(|c| c.set(c.get() + 1));
}

thread_local! {
    static SLICES: std::cell::RefCell<Vec<(&'static str, u64, f64)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Per-slice (name, renders, total ms) since the last [`reset_slices`].
pub fn slice_stats() -> Vec<(&'static str, u64, f64)> {
    SLICES.with(|s| s.borrow().clone())
}

/// Zero the slice counters.
pub fn reset_slices() {
    SLICES.with(|s| s.borrow_mut().clear());
}

pub(crate) fn record_slice(name: &'static str, ms: f64) {
    SLICES.with(|s| {
        let mut s = s.borrow_mut();
        match s.iter_mut().find(|e| e.0 == name) {
            Some(e) => {
                e.1 += 1;
                e.2 += ms;
            }
            None => s.push((name, 1, ms)),
        }
    });
}
