//! Expand Selection through the language service's `textDocument/selectionRange`
//! (Shift+Alt+Right); the syntax tree is the fallback when the service declines. Shrink
//! Selection pops the same stack either way.

use pebbles::prelude::*;

use crate::edit::{EditorState, Selection, Selections};
use crate::language::{self as lsp};
use crate::view::lsp::Lsp;

impl Lsp {
    /// Ask the service for selection ranges at every cursor and grow each selection to the
    /// smallest range strictly containing it. Returns `false` when the service declines.
    pub(crate) fn expand_selection(&self, stack: Signal<Vec<Selections>>) -> bool {
        let Some(svc) = self.svc.clone() else {
            return false;
        };
        let st = self.state.peek();
        let sels = st.selection.clamped(&st.doc);
        let text = st.text();
        let positions: Vec<lsp::Position> = sels
            .ranges()
            .iter()
            .map(|r| lsp::byte_to_position(&text, r.head))
            .collect();
        let me = self.clone();
        let reply = self.guard(
            &self.store.select_gate,
            move |chains: Vec<lsp::SelectionRange>| {
                let st = me.state.peek();
                let sels = st.selection.clamped(&st.doc);
                let text = st.text();
                let grown: Vec<Selection> = sels
                    .ranges()
                    .iter()
                    .zip(chains.iter().map(Some).chain(std::iter::repeat(None)))
                    .map(|(r, chain)| {
                        let (a, b) = (r.min(), r.max());
                        chain
                            .and_then(|c| {
                                c.ranges()
                                    .into_iter()
                                    .map(|g| lsp::range_to_bytes(&text, g))
                                    .find(|&(s, e)| s <= a && b <= e && (s, e) != (a, b))
                            })
                            .map_or(*r, |(s, e)| Selection::range(s, e))
                    })
                    .collect();
                let next = Selections::new(grown, sels.primary_index());
                if next != sels {
                    stack.update(|v| v.push(sels.clone()));
                    me.state.set(EditorState {
                        doc: st.doc.clone(),
                        selection: next,
                    });
                    me.history.peek().borrow_mut().break_group();
                }
            },
        );
        self.with_doc(|doc, _| svc.selection_range(doc, &positions, reply))
    }
}
