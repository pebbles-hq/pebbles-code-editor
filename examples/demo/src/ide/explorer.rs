//! The Explorer sidebar panel — built on the framework's `FileExplorer` (its own tree model,
//! selection, inline rename, drag-move and context menus). We seed a `FileTree` from the
//! workspace paths, then open a file whenever its row becomes active.

use std::collections::HashMap;
use std::rc::Rc;

use pebbles::prelude::*;

use super::workspace::Workspace;

/// Build a `FileExplorer` over the workspace files, returning it plus the node-id → file-index
/// map used to open the right buffer on activation.
fn build(ws: &Workspace) -> (FileExplorer, Rc<HashMap<u64, usize>>) {
    let mut tree = FileTree::new();
    let mut folders: HashMap<String, u64> = HashMap::new();
    let mut id_to_file: HashMap<u64, usize> = HashMap::new();

    for (idx, f) in ws.files.iter().enumerate() {
        let parts: Vec<&str> = f.path.split('/').collect();
        let mut parent: Option<u64> = None;
        let mut acc = String::new();
        for (i, part) in parts.iter().enumerate() {
            if i == parts.len() - 1 {
                let id = tree.insert(parent, FsKind::File, *part);
                id_to_file.insert(id, idx);
            } else {
                acc = if acc.is_empty() { part.to_string() } else { format!("{acc}/{part}") };
                let id = *folders
                    .entry(acc.clone())
                    .or_insert_with(|| tree.insert(parent, FsKind::Folder, *part));
                parent = Some(id);
            }
        }
    }

    let explorer = file_explorer(create_signal(tree));
    explorer.expand_all();
    (explorer, Rc::new(id_to_file))
}

/// The Explorer panel: a header, the explorer's own toolbar, and its tree — with an effect
/// that opens the file under the active row.
pub fn panel(ws: Workspace) -> AnyWidget {
    let c = theme().colors;
    let (explorer, id_to_file) = build(&ws);

    // Open the file whenever the active row changes to a file node.
    let active_row = explorer.active_row();
    let ws_open = ws.clone();
    create_effect(move || {
        if let Some(id) = active_row.get()
            && let Some(&idx) = id_to_file.get(&id)
        {
            ws_open.open_file(idx);
        }
    });

    let header = container()
        .padding(EdgeInsets::only(14.0, 12.0, 10.0, 6.0))
        .child(
            row(children![
                text("EXPLORER")
                    .size(10.5)
                    .weight(700.0)
                    .letter_spacing(0.6)
                    .color(c.muted_foreground),
                spacer(),
                explorer.toolbar(),
            ])
            .cross_axis_alignment(CrossAxisAlignment::Center),
        );

    column(children![
        header,
        expanded(scroll_view(
            container().padding(EdgeInsets::symmetric(6.0, 0.0)).child(explorer.tree())
        )),
    ])
    .cross_axis_alignment(CrossAxisAlignment::Stretch)
    .into_widget()
}
