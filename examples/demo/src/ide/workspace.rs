//! The IDE's in-memory project model: a fixed set of files (each with a live `Signal<String>`
//! buffer), the ordered list of open tabs, and the active file. All UI panels read/drive this.

use std::rc::Rc;

use pebbles::prelude::*;

/// One file in the project. `content` is the live buffer; `original` is the on-load text used
/// for the dirty (•) indicator.
pub struct FileEntry {
    pub path: String,
    pub name: String,
    pub lang: &'static str,
    pub content: Signal<String>,
    /// The last-saved text (dirty = `content != saved`); "Save" resets it to `content`.
    pub saved: Signal<String>,
}

/// The immutable project file set (content mutates through each entry's signal).
pub type Files = Rc<Vec<FileEntry>>;

/// The workspace: the file set plus the reactive open-tabs / active-file state. `Clone` (an
/// `Rc` + `Copy` signals), so it's cheap to hand to every panel.
#[derive(Clone)]
pub struct Workspace {
    pub files: Files,
    /// Open tab file-indices, in tab order.
    pub open: Signal<Vec<usize>>,
    /// The active file index (the one shown in the editor), if any.
    pub active: Signal<Option<usize>>,
}

impl Workspace {
    pub fn new(files: Files) -> Self {
        Workspace {
            files,
            open: create_signal(Vec::new()),
            active: create_signal(None),
        }
    }

    /// Open `idx` in a tab (adding it if new) and make it active.
    pub fn open_file(&self, idx: usize) {
        if idx >= self.files.len() {
            return;
        }
        self.open.update(|o| {
            if !o.contains(&idx) {
                o.push(idx);
            }
        });
        self.active.set(Some(idx));
    }

    /// Close `idx`'s tab; if it was active, fall back to the neighbouring tab.
    pub fn close_file(&self, idx: usize) {
        let pos = self.open.peek().iter().position(|&i| i == idx);
        self.open.update(|o| o.retain(|&i| i != idx));
        if self.active.peek() == Some(idx) {
            let open = self.open.peek();
            let next = pos
                .and_then(|p| open.get(p).or_else(|| open.get(p.wrapping_sub(1))))
                .or_else(|| open.last())
                .copied();
            self.active.set(next);
        }
    }

    /// Whether `idx`'s buffer differs from its last-saved text.
    pub fn is_dirty(&self, idx: usize) -> bool {
        self.files
            .get(idx)
            .map(|f| f.content.peek() != f.saved.peek())
            .unwrap_or(false)
    }

    /// "Save" `idx` (in-memory): reset its saved baseline to the current buffer, clearing dirty.
    pub fn save(&self, idx: usize) {
        if let Some(f) = self.files.get(idx) {
            f.saved.set(f.content.peek());
        }
    }
}
