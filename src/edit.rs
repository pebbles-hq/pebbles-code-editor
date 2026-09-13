//! The text model & editing core: an immutable-style [`EditorState`] (a rope document plus a
//! [`Selection`]) mutated only through [`Transaction`]s built from an invertible, mappable
//! [`ChangeSet`], with an undo/redo [`History`].
//!
//! This is the load-bearing layer the rest of the editor is built on:
//!
//! - **Rope document** — [`ropey::Rope`] gives O(log n) splice + line indexing on large files.
//! - **`ChangeSet`** — a set of replacements that is **invertible** (→ undo/redo is a stack of
//!   inverses) and **mappable** (→ carets, selections, and decorations survive edits).
//! - **`Transaction`** — a change plus an optional new selection; `EditorState::apply` returns
//!   the next state and the inverse transaction.
//! - **`History`** — undo/redo with typing coalescing.
//!
//! All offsets are **byte** offsets into the document (UTF-8, char-boundary safe), matching
//! the rest of the editor.

use ropey::Rope;

/// A caret + selection over the document, as byte offsets. `anchor == head` is a bare caret.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    /// The fixed end of the selection (where it started).
    pub anchor: usize,
    /// The moving end — the caret.
    pub head: usize,
}

impl Selection {
    /// A collapsed selection (a caret) at `pos`.
    pub fn caret(pos: usize) -> Self {
        Selection {
            anchor: pos,
            head: pos,
        }
    }
    /// A selection from `anchor` to `head`.
    pub fn range(anchor: usize, head: usize) -> Self {
        Selection { anchor, head }
    }
    /// Whether the selection is empty (a caret with nothing selected).
    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
    /// The lower offset of the selection.
    pub fn min(&self) -> usize {
        self.anchor.min(self.head)
    }
    /// The upper offset of the selection.
    pub fn max(&self) -> usize {
        self.anchor.max(self.head)
    }
    /// Clamp both ends into `0..=len` (e.g. after an external document swap).
    pub fn clamped(&self, len: usize) -> Self {
        Selection {
            anchor: self.anchor.min(len),
            head: self.head.min(len),
        }
    }
}

/// Which side a mapped position sticks to when it sits exactly at an edit boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Assoc {
    /// Stay before inserted text (bias toward the start of an edit).
    Before,
    /// Move after inserted text (bias toward the end of an edit) — the caret default.
    After,
}

/// A single replacement: replace bytes `from..to` with `insert`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// Start byte offset (inclusive).
    pub from: usize,
    /// End byte offset (exclusive); `from == to` is a pure insertion.
    pub to: usize,
    /// The replacement text; empty is a pure deletion.
    pub insert: String,
}

/// An ordered set of non-overlapping [`Change`]s applied atomically to one document.
///
/// A change set is **invertible** ([`ChangeSet::invert`]) and **mappable**
/// ([`ChangeSet::map`]), which is what makes undo/redo and edit-surviving decorations work.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangeSet {
    /// Sorted by `from`, non-overlapping.
    changes: Vec<Change>,
}

impl ChangeSet {
    /// An empty change set (a no-op).
    pub fn new() -> Self {
        ChangeSet {
            changes: Vec::new(),
        }
    }

    /// A change set that replaces `from..to` with `insert`.
    pub fn replace(from: usize, to: usize, insert: impl Into<String>) -> Self {
        let (from, to) = (from.min(to), from.max(to));
        ChangeSet {
            changes: vec![Change {
                from,
                to,
                insert: insert.into(),
            }],
        }
    }

    /// A change set that inserts `text` at `at`.
    pub fn insert(at: usize, text: impl Into<String>) -> Self {
        Self::replace(at, at, text)
    }

    /// A change set that deletes `from..to`.
    pub fn delete(from: usize, to: usize) -> Self {
        Self::replace(from, to, "")
    }

    /// Build from raw changes, sorting and rejecting overlaps (overlapping/out-of-order
    /// changes are dropped, so the set stays well-formed rather than corrupting the doc).
    pub fn from_changes(mut changes: Vec<Change>) -> Self {
        changes.sort_by_key(|c| c.from);
        let mut out: Vec<Change> = Vec::with_capacity(changes.len());
        let mut last_end = 0usize;
        for c in changes {
            let from = c.from.min(c.to);
            let to = c.from.max(c.to);
            if from < last_end {
                continue; // overlaps a previous change — drop it
            }
            last_end = to;
            out.push(Change {
                from,
                to,
                insert: c.insert,
            });
        }
        ChangeSet { changes: out }
    }

    /// Whether this set makes no changes.
    pub fn is_empty(&self) -> bool {
        self.changes
            .iter()
            .all(|c| c.from == c.to && c.insert.is_empty())
    }

    /// The individual changes, in document order.
    pub fn changes(&self) -> &[Change] {
        &self.changes
    }

    /// Apply the changes to `doc` (right-to-left, so earlier offsets stay valid).
    pub fn apply(&self, doc: &mut Rope) {
        for c in self.changes.iter().rev() {
            let from = c.from.min(doc.len_bytes());
            let to = c.to.min(doc.len_bytes());
            let cf = doc.byte_to_char(from);
            let ct = doc.byte_to_char(to);
            if ct > cf {
                doc.remove(cf..ct);
            }
            if !c.insert.is_empty() {
                doc.insert(cf, &c.insert);
            }
        }
    }

    /// The inverse of this set against the document it was built for (`before`) — applying
    /// the inverse to the *post-change* document restores `before`. This is undo.
    pub fn invert(&self, before: &Rope) -> ChangeSet {
        let mut inv = Vec::with_capacity(self.changes.len());
        let mut delta: isize = 0; // net length change from earlier edits
        for c in &self.changes {
            let from = c.from.min(before.len_bytes());
            let to = c.to.min(before.len_bytes());
            let deleted = before.byte_slice(from..to).to_string();
            let ilen = c.insert.len();
            // In the post-change doc this change occupies [new_from, new_from+ilen);
            // inverting replaces that span with the text it deleted.
            let new_from = (from as isize + delta) as usize;
            inv.push(Change {
                from: new_from,
                to: new_from + ilen,
                insert: deleted,
            });
            delta += ilen as isize - (to as isize - from as isize);
        }
        ChangeSet { changes: inv }
    }

    /// Map a byte position from before this change set to after it. `assoc` decides which way
    /// a position at an exact edit boundary moves (the caret uses [`Assoc::After`]).
    pub fn map(&self, pos: usize, assoc: Assoc) -> usize {
        let after = matches!(assoc, Assoc::After);
        let mut result = pos as isize;
        for c in &self.changes {
            let ins = c.insert.len() as isize;
            let del = (c.to - c.from) as isize;
            if pos < c.from {
                break; // this change (and all later ones) are entirely after `pos`
            } else if pos > c.to {
                result += ins - del; // this change is entirely before `pos`
            } else if pos == c.from && c.from == c.to {
                // A pure insertion exactly at `pos`: `After` moves past it, `Before` stays.
                if after {
                    result += ins;
                }
                break;
            } else if pos == c.from && !after {
                break; // stick before the start of a replacement/deletion
            } else {
                // Inside a replacement/deletion (or at its far edge): collapse onto the new
                // text — its end (`After`, or at the end edge) or its start (`Before`).
                let base = if pos == c.to || after { ins } else { 0 };
                result += base - (pos as isize - c.from as isize);
                break;
            }
        }
        result.max(0) as usize
    }
}

/// One or more [`Selection`] ranges with a designated **primary** — the multi-cursor model.
/// Ranges are kept sorted by position and non-overlapping (overlapping/touching ranges, and
/// duplicate carets, merge). A single caret is the common case (`Selections::single`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selections {
    ranges: Vec<Selection>,
    primary: usize,
}

impl Selections {
    /// A single selection/caret.
    pub fn single(sel: Selection) -> Self {
        Selections {
            ranges: vec![sel],
            primary: 0,
        }
    }

    /// Build from ranges with `primary` the index of the main one, normalizing (sort + merge
    /// overlaps). Empty input degenerates to a caret at 0.
    pub fn new(ranges: Vec<Selection>, primary: usize) -> Self {
        if ranges.is_empty() {
            return Selections::single(Selection::caret(0));
        }
        let marker = ranges[primary.min(ranges.len() - 1)];
        let mut sorted = ranges;
        sorted.sort_by_key(|s| (s.min(), s.max()));
        let mut merged: Vec<Selection> = Vec::with_capacity(sorted.len());
        for r in sorted {
            match merged.last_mut() {
                Some(last) if r.min() <= last.max() => {
                    // Overlapping/touching → union into one forward range.
                    *last = Selection::range(last.min().min(r.min()), last.max().max(r.max()));
                }
                _ => merged.push(r),
            }
        }
        // The primary is whichever merged range now contains the old primary's head.
        let ph = marker.head;
        let primary = merged
            .iter()
            .position(|s| s.min() <= ph && ph <= s.max())
            .unwrap_or(merged.len() - 1);
        Selections {
            ranges: merged,
            primary,
        }
    }

    /// The primary (main) selection — the one status/current-line/autoscroll follow.
    pub fn primary(&self) -> Selection {
        self.ranges[self.primary]
    }

    /// The index of the primary range within [`ranges`](Self::ranges).
    pub fn primary_index(&self) -> usize {
        self.primary
    }

    /// All selection ranges, sorted by position.
    pub fn ranges(&self) -> &[Selection] {
        &self.ranges
    }

    /// The number of ranges (cursors).
    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    /// Whether there is exactly one caret with no selected text.
    pub fn is_empty(&self) -> bool {
        self.ranges.len() == 1 && self.ranges[0].is_empty()
    }

    /// Whether there is more than one cursor.
    pub fn is_multi(&self) -> bool {
        self.ranges.len() > 1
    }

    /// Add `sel` as the new primary range (merging overlaps).
    pub fn pushed(&self, sel: Selection) -> Selections {
        let mut r = self.ranges.clone();
        r.push(sel);
        let n = r.len();
        Selections::new(r, n - 1)
    }

    /// Collapse to just the primary caret (Escape / a plain click).
    pub fn collapsed(&self) -> Selections {
        Selections::single(self.primary())
    }

    /// Map every range forward through a change set (carets survive an edit).
    pub fn mapped(&self, cs: &ChangeSet) -> Selections {
        let ranges = self
            .ranges
            .iter()
            .map(|s| Selection {
                anchor: cs.map(s.anchor, Assoc::After),
                head: cs.map(s.head, Assoc::After),
            })
            .collect();
        Selections::new(ranges, self.primary)
    }

    /// Clamp every range into `0..=len`.
    pub fn clamped(&self, len: usize) -> Selections {
        let ranges = self.ranges.iter().map(|s| s.clamped(len)).collect();
        Selections::new(ranges, self.primary)
    }
}

impl From<Selection> for Selections {
    fn from(s: Selection) -> Self {
        Selections::single(s)
    }
}

/// The editor's document + selection. Treated as immutable: [`EditorState::apply`] returns a
/// new state rather than mutating in place.
#[derive(Clone)]
pub struct EditorState {
    /// The document rope.
    pub doc: Rope,
    /// The current caret(s) / selection(s).
    pub selection: Selections,
}

impl EditorState {
    /// A fresh state holding `text` with a single caret at the start.
    pub fn new(text: &str) -> Self {
        EditorState {
            doc: Rope::from_str(text),
            selection: Selections::single(Selection::caret(0)),
        }
    }

    /// The primary caret / selection (the common single-cursor accessor).
    pub fn primary(&self) -> Selection {
        self.selection.primary()
    }

    /// The document as a `String` (O(n) — used for rendering / plain-text output).
    pub fn text(&self) -> String {
        self.doc.to_string()
    }

    /// The document length in bytes.
    pub fn len(&self) -> usize {
        self.doc.len_bytes()
    }

    /// Whether the document is empty.
    pub fn is_empty(&self) -> bool {
        self.doc.len_bytes() == 0
    }

    /// Apply `tx`, returning the next state **and** the inverse transaction (which, applied to
    /// the next state, restores this one — the undo entry).
    pub fn apply(&self, tx: &Transaction) -> (EditorState, Transaction) {
        let inverse_changes = tx.changes.invert(&self.doc);
        let mut doc = self.doc.clone();
        tx.changes.apply(&mut doc);
        let len = doc.len_bytes();

        // The selection after: an explicit one from the tx, else map the old one through.
        let selection = tx
            .selection
            .clone()
            .unwrap_or_else(|| self.selection.mapped(&tx.changes))
            .clamped(len);

        let next = EditorState { doc, selection };
        let inverse = Transaction {
            changes: inverse_changes,
            selection: Some(self.selection.clone()),
        };
        (next, inverse)
    }
}

/// A change to the document plus an optional resulting selection. The only way to mutate an
/// [`EditorState`].
#[derive(Clone, Debug, Default)]
pub struct Transaction {
    /// The edit.
    pub changes: ChangeSet,
    /// Where to put the selection(s) afterward; `None` maps the old selection through.
    pub selection: Option<Selections>,
}

impl Transaction {
    /// A transaction from a change set (selection mapped through the change).
    pub fn change(changes: ChangeSet) -> Self {
        Transaction {
            changes,
            selection: None,
        }
    }
    /// A transaction from a change set with an explicit resulting selection (a single
    /// [`Selection`] or a [`Selections`] set).
    pub fn change_and_select(changes: ChangeSet, selection: impl Into<Selections>) -> Self {
        Transaction {
            changes,
            selection: Some(selection.into()),
        }
    }
    /// A selection-only transaction (moves the caret(s), edits nothing).
    pub fn select(selection: impl Into<Selections>) -> Self {
        Transaction {
            changes: ChangeSet::new(),
            selection: Some(selection.into()),
        }
    }
    /// Whether this transaction edits the document (vs. a pure selection change).
    pub fn is_edit(&self) -> bool {
        !self.changes.is_empty()
    }
}

/// How a recorded edit may merge with the previous one for undo grouping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coalesce {
    /// Never merge — this edit is its own undo step (paste, replace, indent, newline…).
    Never,
    /// Contiguous single-character typing merges into one step.
    Typing,
    /// Contiguous backspacing merges into one step.
    Deleting,
}

struct Revision {
    inverse: Transaction,
    kind: Coalesce,
    /// Caret position after the edit — used to detect contiguous typing/deleting.
    caret_after: usize,
}

/// The undo/redo stack. Records the inverse of every applied edit; contiguous typing and
/// backspacing coalesce into single undo steps.
#[derive(Default)]
pub struct History {
    undo: Vec<Revision>,
    redo: Vec<Revision>,
}

impl History {
    /// A fresh, empty history.
    pub fn new() -> Self {
        History::default()
    }

    /// Record the `inverse` of an edit that produced `caret_after`, coalescing with the
    /// previous revision when `kind` allows and the edits are contiguous. Any edit clears the
    /// redo stack.
    pub fn record(&mut self, inverse: Transaction, kind: Coalesce, caret_after: usize) {
        self.redo.clear();
        if kind != Coalesce::Never
            && let Some(top) = self.undo.last_mut()
            && top.kind == kind
            && can_merge(kind, top.caret_after, &inverse)
        {
            merge_inverse(kind, &mut top.inverse, inverse);
            top.caret_after = caret_after;
            return;
        }
        self.undo.push(Revision {
            inverse,
            kind,
            caret_after,
        });
    }

    /// Whether there is anything to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    /// Whether there is anything to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Undo one step against `state`, returning the restored state (or `None` if empty).
    pub fn undo(&mut self, state: &EditorState) -> Option<EditorState> {
        let rev = self.undo.pop()?;
        let (next, redo_inverse) = state.apply(&rev.inverse);
        self.redo.push(Revision {
            inverse: redo_inverse,
            kind: Coalesce::Never,
            caret_after: next.primary().head,
        });
        Some(next)
    }

    /// Redo one step against `state`, returning the restored state (or `None` if empty).
    pub fn redo(&mut self, state: &EditorState) -> Option<EditorState> {
        let rev = self.redo.pop()?;
        let (next, undo_inverse) = state.apply(&rev.inverse);
        self.undo.push(Revision {
            inverse: undo_inverse,
            kind: Coalesce::Never,
            caret_after: next.primary().head,
        });
        Some(next)
    }

    /// Force the next edit to start a fresh undo step (e.g. on focus loss or caret jump).
    pub fn break_group(&mut self) {
        if let Some(top) = self.undo.last_mut() {
            top.kind = Coalesce::Never;
        }
    }
}

/// A new typing edit merges if its single inserted char sits right where the caret was.
fn can_merge(kind: Coalesce, prev_caret: usize, inverse: &Transaction) -> bool {
    let Some(c) = inverse.changes.changes().first() else {
        return false;
    };
    if inverse.changes.changes().len() != 1 {
        return false;
    }
    match kind {
        // Typing: the edit inserted 1 char at prev_caret, so its inverse deletes [prev,prev+n).
        Coalesce::Typing => c.insert.is_empty() && c.from == prev_caret,
        // Deleting (backspace): the edit removed the char before the caret, so its inverse
        // inserts at the new caret position (== the inverse's `from`, one char back).
        Coalesce::Deleting => c.from + c.insert.len() == prev_caret && !c.insert.is_empty(),
        Coalesce::Never => false,
    }
}

/// Merge `next` (a newer edit's inverse) into `top` (the accumulated inverse of the group).
fn merge_inverse(kind: Coalesce, top: &mut Transaction, next: Transaction) {
    let (Some(t), Some(n)) = (
        top.changes.changes().first(),
        next.changes.changes().first(),
    ) else {
        return;
    };
    match kind {
        // Both inverses are deletions; the group's inverse deletes the combined range.
        Coalesce::Typing => {
            let merged = Change {
                from: t.from,
                to: t.to + (n.to - n.from),
                insert: String::new(),
            };
            top.changes = ChangeSet::from_changes(vec![merged]);
        }
        // Both inverses are insertions; prepend the newer deleted char to the group's insert.
        Coalesce::Deleting => {
            let mut text = n.insert.clone();
            text.push_str(&t.insert);
            let merged = Change {
                from: n.from,
                to: n.from,
                insert: text,
            };
            top.changes = ChangeSet::from_changes(vec![merged]);
        }
        Coalesce::Never => {}
    }
    // Keep the group's original selection-before (top.selection).
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(state: &EditorState) -> String {
        state.text()
    }

    #[test]
    fn changeset_apply_insert_delete_replace() {
        let mut d = Rope::from_str("hello world");
        ChangeSet::insert(5, ",").apply(&mut d);
        assert_eq!(d.to_string(), "hello, world");
        ChangeSet::delete(0, 5).apply(&mut d);
        assert_eq!(d.to_string(), ", world");
        ChangeSet::replace(2, 7, "there").apply(&mut d);
        assert_eq!(d.to_string(), ", there");
    }

    #[test]
    fn changeset_invert_round_trips() {
        let before = Rope::from_str("abcdef");
        let cs = ChangeSet::replace(2, 4, "XYZ"); // "abXYZef"
        let mut after = before.clone();
        cs.apply(&mut after);
        assert_eq!(after.to_string(), "abXYZef");
        let inv = cs.invert(&before);
        inv.apply(&mut after);
        assert_eq!(after.to_string(), "abcdef");
    }

    #[test]
    fn changeset_map_positions() {
        // Insert 3 chars at 2: positions >= 2 shift right by 3; < 2 unchanged.
        let cs = ChangeSet::insert(2, "xxx");
        assert_eq!(cs.map(0, Assoc::After), 0);
        assert_eq!(cs.map(2, Assoc::After), 5);
        assert_eq!(cs.map(2, Assoc::Before), 2);
        assert_eq!(cs.map(5, Assoc::After), 8);
        // Delete 2..5: inside collapses to 2; after shifts left by 3.
        let del = ChangeSet::delete(2, 5);
        assert_eq!(del.map(1, Assoc::After), 1);
        assert_eq!(del.map(3, Assoc::After), 2);
        assert_eq!(del.map(6, Assoc::After), 3);
    }

    #[test]
    fn state_apply_moves_caret_and_inverts() {
        let st = EditorState::new("fn main() {}");
        let tx = Transaction::change(ChangeSet::insert(0, "pub "));
        let (next, inv) = st.apply(&tx);
        assert_eq!(s(&next), "pub fn main() {}");
        // caret (was 0) mapped After the insert → 4
        assert_eq!(next.primary().head, 4);
        // inverse restores
        let (back, _) = next.apply(&inv);
        assert_eq!(s(&back), "fn main() {}");
    }

    #[test]
    fn undo_redo_round_trip() {
        let mut st = EditorState::new("");
        let mut hist = History::new();
        for (i, ch) in "abc".chars().enumerate() {
            let tx = Transaction::change_and_select(
                ChangeSet::insert(i, ch.to_string()),
                Selection::caret(i + 1),
            );
            let (next, inv) = st.apply(&tx);
            hist.record(inv, Coalesce::Typing, next.primary().head);
            st = next;
        }
        assert_eq!(s(&st), "abc");
        // One coalesced typing group → a single undo restores the whole word.
        st = hist.undo(&st).unwrap();
        assert_eq!(s(&st), "");
        st = hist.redo(&st).unwrap();
        assert_eq!(s(&st), "abc");
        assert_eq!(st.primary().head, 3);
    }

    #[test]
    fn typing_then_delete_are_separate_undo_steps() {
        let mut st = EditorState::new("");
        let mut hist = History::new();
        // type "hi"
        for (i, ch) in "hi".chars().enumerate() {
            let tx = Transaction::change_and_select(
                ChangeSet::insert(i, ch.to_string()),
                Selection::caret(i + 1),
            );
            let (n, inv) = st.apply(&tx);
            hist.record(inv, Coalesce::Typing, n.primary().head);
            st = n;
        }
        // backspace once (own group)
        let tx = Transaction::change_and_select(ChangeSet::delete(1, 2), Selection::caret(1));
        let (n, inv) = st.apply(&tx);
        hist.record(inv, Coalesce::Deleting, n.primary().head);
        st = n;
        assert_eq!(s(&st), "h");
        // undo the delete → "hi"; undo the typing → ""
        st = hist.undo(&st).unwrap();
        assert_eq!(s(&st), "hi");
        st = hist.undo(&st).unwrap();
        assert_eq!(s(&st), "");
        assert!(!hist.can_undo());
    }

    #[test]
    fn edit_clears_redo() {
        let mut st = EditorState::new("x");
        let mut hist = History::new();
        let (n, inv) = st.apply(&Transaction::change_and_select(
            ChangeSet::insert(1, "y"),
            Selection::caret(2),
        ));
        hist.record(inv, Coalesce::Never, 2);
        st = n;
        st = hist.undo(&st).unwrap();
        assert!(hist.can_redo());
        // a new edit clears redo
        let (n, inv) = st.apply(&Transaction::change_and_select(
            ChangeSet::insert(1, "z"),
            Selection::caret(2),
        ));
        hist.record(inv, Coalesce::Never, 2);
        let _ = n;
        assert!(!hist.can_redo());
    }

    #[test]
    fn multibyte_is_char_boundary_safe() {
        let st = EditorState::new("héllo"); // é is 2 bytes
        let tx = Transaction::change(ChangeSet::insert(st.len(), " 🌍"));
        let (next, inv) = st.apply(&tx);
        assert_eq!(s(&next), "héllo 🌍");
        let (back, _) = next.apply(&inv);
        assert_eq!(s(&back), "héllo");
    }

    #[test]
    fn selections_sort_and_merge() {
        // Out-of-order ranges are sorted; overlapping/touching ones merge.
        let sels = Selections::new(
            vec![
                Selection::range(10, 6), // 6..10
                Selection::caret(2),
                Selection::range(4, 8), // overlaps 6..10 → merges to 2? no: 4..8 & 6..10 → 4..10
            ],
            0,
        );
        assert_eq!(sels.len(), 2);
        assert_eq!(sels.ranges()[0], Selection::caret(2));
        assert_eq!((sels.ranges()[1].min(), sels.ranges()[1].max()), (4, 10));
    }

    #[test]
    fn selections_primary_tracks_through_merge() {
        // The primary was the range at head 8; after merge it points at the merged range.
        let sels = Selections::new(
            vec![
                Selection::caret(0),
                Selection::range(8, 4),
                Selection::range(6, 9),
            ],
            1, // primary = the (8,4) range, head at 8
        );
        assert_eq!(sels.len(), 2);
        assert!(sels.primary().min() <= 8 && sels.primary().max() >= 8);
    }

    #[test]
    fn selections_pushed_and_collapsed() {
        let one = Selections::single(Selection::caret(0));
        let two = one.pushed(Selection::caret(5));
        assert_eq!(two.len(), 2);
        assert!(two.is_multi());
        // The newly pushed range becomes primary.
        assert_eq!(two.primary().head, 5);
    }

    #[test]
    fn selections_map_across_edit() {
        // Two carets at 2 and 6; insert "xx" at 0 shifts both right by 2.
        let sels = Selections::new(vec![Selection::caret(2), Selection::caret(6)], 0);
        let cs = ChangeSet::insert(0, "xx");
        let mapped = sels.mapped(&cs);
        assert_eq!(mapped.ranges()[0].head, 4);
        assert_eq!(mapped.ranges()[1].head, 8);
    }

    #[test]
    fn multi_cursor_edit_round_trips() {
        // Two carets (at 0 and after "b"); a two-change set inserts "X" at both.
        let st = EditorState {
            doc: Rope::from_str("ab"),
            selection: Selections::new(vec![Selection::caret(0), Selection::caret(1)], 0),
        };
        let cs = ChangeSet::from_changes(vec![
            Change {
                from: 0,
                to: 0,
                insert: "X".into(),
            },
            Change {
                from: 1,
                to: 1,
                insert: "X".into(),
            },
        ]);
        let tx = Transaction::change_and_select(
            cs,
            Selections::new(vec![Selection::caret(1), Selection::caret(3)], 0),
        );
        let (next, inv) = st.apply(&tx);
        assert_eq!(s(&next), "XaXb");
        // Inverse restores the document and the original multi-cursor selection.
        let (back, _) = next.apply(&inv);
        assert_eq!(s(&back), "ab");
        assert_eq!(back.selection.len(), 2);
    }

    #[test]
    fn selections_clamped_to_doc() {
        let sels = Selections::new(vec![Selection::range(2, 20)], 0);
        let c = sels.clamped(5);
        assert_eq!((c.primary().min(), c.primary().max()), (2, 5));
    }
}
