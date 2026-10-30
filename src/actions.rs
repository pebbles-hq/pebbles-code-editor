//! The editor's **command list** — every built-in editor command with its VS Code id, a
//! human title (shown in the command palette) and its default keybinding(s), Linux/Windows
//! VS Code defaults. Keybindings are remappable through [`Keymap`](crate::Keymap)
//! (`bind_command` / `unbind_command`); every command is also runnable from the palette.
//!
//! Chords use the pebbles shortcut grammar (`"Ctrl+Shift+K"`, `"Alt+Up"`); a space separates
//! the strokes of a multi-stroke chord (`"Ctrl+K Ctrl+0"`). The editor resolves them on the
//! raw key event *before* the framework maps the key to an editing intent, so `Alt+Up` moves
//! lines instead of the caret.

use pebbles::core::shortcuts::{self, Mods};
use pebbles::prelude::ShortcutKey;

macro_rules! commands {
    ($( $(#[$doc:meta])* $var:ident => $id:literal, $title:literal, [$($key:literal),*]; )*) => {
        /// A built-in editor command (see the [module docs](self)).
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        pub enum EditorCommand {
            $( $(#[$doc])* $var, )*
        }

        impl EditorCommand {
            /// Every command, in palette order.
            pub const ALL: &'static [EditorCommand] = &[$(EditorCommand::$var,)*];

            /// The VS Code command id (`"editor.action.moveLinesUpAction"`).
            pub fn id(self) -> &'static str {
                match self { $(EditorCommand::$var => $id,)* }
            }

            /// The palette title.
            pub fn title(self) -> &'static str {
                match self { $(EditorCommand::$var => $title,)* }
            }

            /// The default keybindings (VS Code Linux/Windows defaults).
            pub fn default_keys(self) -> &'static [&'static str] {
                match self { $(EditorCommand::$var => &[$($key),*],)* }
            }

            /// Look a command up by its VS Code id.
            pub fn from_id(id: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|c| c.id() == id)
            }
        }
    };
}

commands! {
    /// Add a cursor on the line above every cursor.
    AddCursorAbove => "editor.action.insertCursorAbove", "Add Cursor Above", ["Ctrl+Alt+Up", "Ctrl+Shift+Up"];
    /// Add a cursor on the line below every cursor.
    AddCursorBelow => "editor.action.insertCursorBelow", "Add Cursor Below", ["Ctrl+Alt+Down", "Ctrl+Shift+Down"];
    /// Select every occurrence of the selection / word.
    SelectAllOccurrences => "editor.action.selectHighlights", "Select All Occurrences of Find Match", ["Ctrl+Shift+L", "Ctrl+F2"];
    /// Move the last added selection to the next occurrence.
    MoveSelectionToNextFindMatch => "editor.action.moveSelectionToNextFindMatch", "Move Last Selection to Next Find Match", ["Ctrl+K Ctrl+D"];
    /// Put a cursor at the end of every selected line.
    AddCursorsToLineEnds => "editor.action.insertCursorAtEndOfEachLineSelected", "Add Cursors to Line Ends", ["Shift+Alt+I"];
    /// Undo the last cursor/selection change (soft undo).
    CursorUndo => "cursorUndo", "Cursor Undo", ["Ctrl+U"];
    /// Redo a cursor/selection change undone by Cursor Undo.
    CursorRedo => "cursorRedo", "Cursor Redo", [];
    /// Column (box) select up.
    ColumnSelectUp => "cursorColumnSelectUp", "Column Select Up", ["Ctrl+Shift+Alt+Up"];
    /// Column (box) select down.
    ColumnSelectDown => "cursorColumnSelectDown", "Column Select Down", ["Ctrl+Shift+Alt+Down"];
    /// Column (box) select left.
    ColumnSelectLeft => "cursorColumnSelectLeft", "Column Select Left", ["Ctrl+Shift+Alt+Left"];
    /// Column (box) select right.
    ColumnSelectRight => "cursorColumnSelectRight", "Column Select Right", ["Ctrl+Shift+Alt+Right"];
    /// Select the current line (repeat to extend).
    ExpandLineSelection => "expandLineSelection", "Expand Line Selection", ["Ctrl+L"];
    /// Jump to the matching bracket.
    JumpToBracket => "editor.action.jumpToBracket", "Go to Bracket", ["Ctrl+Shift+\\"];
    /// Select to (and including) the enclosing brackets.
    SelectToBracket => "editor.action.selectToBracket", "Select to Bracket", [];
    /// Move the selected lines up.
    MoveLinesUp => "editor.action.moveLinesUpAction", "Move Line Up", ["Alt+Up"];
    /// Move the selected lines down.
    MoveLinesDown => "editor.action.moveLinesDownAction", "Move Line Down", ["Alt+Down"];
    /// Copy the selected lines above.
    CopyLinesUp => "editor.action.copyLinesUpAction", "Copy Line Up", ["Shift+Alt+Up"];
    /// Copy the selected lines below.
    CopyLinesDown => "editor.action.copyLinesDownAction", "Copy Line Down", ["Shift+Alt+Down"];
    /// Delete the selected lines.
    DeleteLines => "editor.action.deleteLines", "Delete Line", ["Ctrl+Shift+K"];
    /// Join the selected lines (or the caret line with the next).
    JoinLines => "editor.action.joinLines", "Join Lines", ["Ctrl+J"];
    /// Duplicate the selection (or line).
    DuplicateSelection => "editor.action.duplicateSelection", "Duplicate Selection", [];
    /// Sort lines ascending.
    SortLinesAscending => "editor.action.sortLinesAscending", "Sort Lines Ascending", [];
    /// Sort lines descending.
    SortLinesDescending => "editor.action.sortLinesDescending", "Sort Lines Descending", [];
    /// Transpose the characters around the caret.
    TransposeLetters => "editor.action.transposeLetters", "Transpose Characters around the Cursor", [];
    /// Insert a line below and move to it.
    InsertLineBelow => "editor.action.insertLineAfter", "Insert Line Below", ["Ctrl+Enter"];
    /// Insert a line above and move to it.
    InsertLineAbove => "editor.action.insertLineBefore", "Insert Line Above", ["Ctrl+Shift+Enter"];
    /// Indent the selected lines.
    IndentLines => "editor.action.indentLines", "Indent Line", ["Ctrl+]"];
    /// Outdent the selected lines.
    OutdentLines => "editor.action.outdentLines", "Outdent Line", ["Ctrl+["];
    /// Toggle line comments.
    ToggleLineComment => "editor.action.commentLine", "Toggle Line Comment", [];
    /// Add line comments.
    AddLineComment => "editor.action.addCommentLine", "Add Line Comment", ["Ctrl+K Ctrl+C"];
    /// Remove line comments.
    RemoveLineComment => "editor.action.removeCommentLine", "Remove Line Comment", ["Ctrl+K Ctrl+U"];
    /// Toggle a block comment.
    ToggleBlockComment => "editor.action.blockComment", "Toggle Block Comment", ["Shift+Alt+A"];
    /// Upper-case the selection.
    TransformToUppercase => "editor.action.transformToUppercase", "Transform to Uppercase", [];
    /// Lower-case the selection.
    TransformToLowercase => "editor.action.transformToLowercase", "Transform to Lowercase", [];
    /// Title-case the selection.
    TransformToTitlecase => "editor.action.transformToTitlecase", "Transform to Title Case", [];
    /// snake_case the selection.
    TransformToSnakecase => "editor.action.transformToSnakecase", "Transform to Snake Case", [];
    /// kebab-case the selection.
    TransformToKebabcase => "editor.action.transformToKebabcase", "Transform to Kebab Case", [];
    /// camelCase the selection.
    TransformToCamelcase => "editor.action.transformToCamelcase", "Transform to Camel Case", [];
    /// Remove trailing whitespace.
    TrimTrailingWhitespace => "editor.action.trimTrailingWhitespace", "Trim Trailing Whitespace", ["Ctrl+K Ctrl+X"];
    /// Ensure a final newline.
    InsertFinalNewline => "editor.action.insertFinalNewline", "Insert Final Newline", [];
    /// Convert indentation to spaces.
    IndentationToSpaces => "editor.action.indentationToSpaces", "Convert Indentation to Spaces", [];
    /// Convert indentation to tabs.
    IndentationToTabs => "editor.action.indentationToTabs", "Convert Indentation to Tabs", [];
    /// Change all line endings to LF.
    ChangeEolToLf => "workbench.action.editor.changeEOL.lf", "Change End of Line Sequence to LF", [];
    /// Change all line endings to CRLF.
    ChangeEolToCrlf => "workbench.action.editor.changeEOL.crlf", "Change End of Line Sequence to CRLF", [];
    /// Move to the previous subword (camelCase / snake_case part).
    CursorWordPartLeft => "cursorWordPartLeft", "Cursor Word Part Left", ["Ctrl+Alt+Left"];
    /// Move to the next subword.
    CursorWordPartRight => "cursorWordPartRight", "Cursor Word Part Right", ["Ctrl+Alt+Right"];
    /// Select to the previous subword.
    CursorWordPartLeftSelect => "cursorWordPartLeftSelect", "Cursor Word Part Left Select", [];
    /// Select to the next subword.
    CursorWordPartRightSelect => "cursorWordPartRightSelect", "Cursor Word Part Right Select", [];
    /// Delete to the previous subword.
    DeleteWordPartLeft => "deleteWordPartLeft", "Delete Word Part Left", ["Ctrl+Alt+Backspace"];
    /// Delete to the next subword.
    DeleteWordPartRight => "deleteWordPartRight", "Delete Word Part Right", ["Ctrl+Alt+Delete"];
    /// Move the cursor one page up.
    CursorPageUp => "cursorPageUp", "Cursor Page Up", ["PageUp"];
    /// Move the cursor one page down.
    CursorPageDown => "cursorPageDown", "Cursor Page Down", ["PageDown"];
    /// Select one page up.
    CursorPageUpSelect => "cursorPageUpSelect", "Cursor Page Up Select", ["Shift+PageUp"];
    /// Select one page down.
    CursorPageDownSelect => "cursorPageDownSelect", "Cursor Page Down Select", ["Shift+PageDown"];
    /// Scroll the view one line up (the caret stays).
    ScrollLineUp => "scrollLineUp", "Scroll Line Up", ["Ctrl+Up"];
    /// Scroll the view one line down.
    ScrollLineDown => "scrollLineDown", "Scroll Line Down", ["Ctrl+Down"];
    /// Scroll the view one page up.
    ScrollPageUp => "scrollPageUp", "Scroll Page Up", ["Alt+PageUp"];
    /// Scroll the view one page down.
    ScrollPageDown => "scrollPageDown", "Scroll Page Down", ["Alt+PageDown"];
    /// Fold the innermost region at the cursor.
    Fold => "editor.fold", "Fold", ["Ctrl+Shift+["];
    /// Unfold the region at the cursor.
    Unfold => "editor.unfold", "Unfold", ["Ctrl+Shift+]"];
    /// Toggle the fold at the cursor.
    ToggleFold => "editor.toggleFold", "Toggle Fold", ["Ctrl+K Ctrl+L"];
    /// Fold the region at the cursor and every region inside it.
    FoldRecursively => "editor.foldRecursively", "Fold Recursively", ["Ctrl+K Ctrl+["];
    /// Unfold the region at the cursor and every region inside it.
    UnfoldRecursively => "editor.unfoldRecursively", "Unfold Recursively", ["Ctrl+K Ctrl+]"];
    /// Fold every region.
    FoldAll => "editor.foldAll", "Fold All", ["Ctrl+K Ctrl+0"];
    /// Unfold every region.
    UnfoldAll => "editor.unfoldAll", "Unfold All", ["Ctrl+K Ctrl+J"];
    /// Fold every block comment.
    FoldAllBlockComments => "editor.foldAllBlockComments", "Fold All Block Comments", ["Ctrl+K Ctrl+/"];
    /// Fold every `#region` marker region.
    FoldAllMarkerRegions => "editor.foldAllMarkerRegions", "Fold All Regions", ["Ctrl+K Ctrl+8"];
    /// Unfold every `#region` marker region.
    UnfoldAllMarkerRegions => "editor.unfoldAllMarkerRegions", "Unfold All Regions", ["Ctrl+K Ctrl+9"];
    /// Fold all regions except the one containing the cursor.
    FoldAllExcept => "editor.foldAllExcept", "Fold All Except Selected", ["Ctrl+K Ctrl+-"];
    /// Fold level 1.
    FoldLevel1 => "editor.foldLevel1", "Fold Level 1", ["Ctrl+K Ctrl+1"];
    /// Fold level 2.
    FoldLevel2 => "editor.foldLevel2", "Fold Level 2", ["Ctrl+K Ctrl+2"];
    /// Fold level 3.
    FoldLevel3 => "editor.foldLevel3", "Fold Level 3", ["Ctrl+K Ctrl+3"];
    /// Fold level 4.
    FoldLevel4 => "editor.foldLevel4", "Fold Level 4", ["Ctrl+K Ctrl+4"];
    /// Fold level 5.
    FoldLevel5 => "editor.foldLevel5", "Fold Level 5", ["Ctrl+K Ctrl+5"];
    /// Fold level 6.
    FoldLevel6 => "editor.foldLevel6", "Fold Level 6", ["Ctrl+K Ctrl+6"];
    /// Fold level 7.
    FoldLevel7 => "editor.foldLevel7", "Fold Level 7", ["Ctrl+K Ctrl+7"];
    /// Toggle soft wrap.
    ToggleWordWrap => "editor.action.toggleWordWrap", "View: Toggle Word Wrap", ["Alt+Z"];
    /// Increase the editor font size.
    FontZoomIn => "editor.action.fontZoomIn", "Editor Font Zoom In", ["Ctrl+="];
    /// Decrease the editor font size.
    FontZoomOut => "editor.action.fontZoomOut", "Editor Font Zoom Out", ["Ctrl+-"];
    /// Reset the editor font size.
    FontZoomReset => "editor.action.fontZoomReset", "Editor Font Zoom Reset", ["Ctrl+0"];
    /// Go to a line (opens the Go to Line prompt).
    GoToLine => "workbench.action.gotoLine", "Go to Line/Column...", ["Ctrl+G"];
    /// Find the next match of the find widget's query.
    FindNext => "editor.action.nextMatchFindAction", "Find Next", ["F3"];
    /// Find the previous match.
    FindPrevious => "editor.action.previousMatchFindAction", "Find Previous", ["Shift+F3"];
    /// Go to the next change against the change baseline (git).
    NextChange => "editor.action.dirtydiff.next", "Go to Next Change", ["Alt+F3"];
    /// Go to the previous change.
    PreviousChange => "editor.action.dirtydiff.previous", "Go to Previous Change", ["Shift+Alt+F3"];
    /// Expand the selection to the enclosing syntax node.
    ExpandSelection => "editor.action.smartSelect.expand", "Expand Selection", ["Shift+Alt+Right"];
    /// Shrink the selection back.
    ShrinkSelection => "editor.action.smartSelect.shrink", "Shrink Selection", ["Shift+Alt+Left"];
}

/// One parsed keystroke.
pub(crate) type Stroke = (Mods, ShortcutKey);

/// Parse a (possibly multi-stroke) chord. `None` on a grammar error.
pub(crate) fn parse_chord(chord: &str) -> Option<Vec<Stroke>> {
    chord
        .split_whitespace()
        .map(|s| shortcuts::parse(s).ok())
        .collect::<Option<Vec<_>>>()
        .filter(|v| !v.is_empty())
}

/// The resolved key table: every (chord → command) after applying the host [`Keymap`]
/// overrides to the defaults.
pub(crate) fn key_table(km: &crate::Keymap) -> Vec<(Vec<Stroke>, EditorCommand)> {
    let mut out = Vec::new();
    for &cmd in EditorCommand::ALL {
        if km.unbound.contains(&cmd) {
            continue;
        }
        for k in cmd.default_keys() {
            if let Some(s) = parse_chord(k) {
                out.push((s, cmd));
            }
        }
    }
    for (chord, cmd) in &km.command_bindings {
        if let Some(s) = parse_chord(chord) {
            // A host binding on a chord shadows any default on the same chord.
            out.retain(|(k, _)| *k != s);
            out.push((s, *cmd));
        }
    }
    out
}

/// What a keystroke resolved to.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Resolved {
    /// Run this command.
    Run(EditorCommand),
    /// The stroke is a prefix of a multi-stroke chord — wait for the next.
    Pending,
    /// No binding.
    None,
}

/// The keystroke of a raw key event, normalized so a shifted punctuation key matches the
/// chord written with its unshifted character (`Ctrl+Shift+\\` arrives as `|` on a US
/// layout; the physical key decides).
pub(crate) fn stroke_of(ev: &pebbles::core::KeyEvent) -> Option<Stroke> {
    let key = ev.shortcut_key()?;
    let key = match key {
        ShortcutKey::Char(c) if ev.mods.shift && !c.is_alphanumeric() => {
            let unshifted = match ev.physical.code() {
                "Backslash" => Some('\\'),
                "BracketLeft" => Some('['),
                "BracketRight" => Some(']'),
                "Slash" => Some('/'),
                "Minus" => Some('-'),
                "Equal" => Some('='),
                "Comma" => Some(','),
                "Period" => Some('.'),
                "Semicolon" => Some(';'),
                "Quote" => Some('\''),
                "Backquote" => Some('`'),
                _ => None,
            };
            ShortcutKey::Char(unshifted.unwrap_or(c))
        }
        k => k,
    };
    Some((ev.mods, key))
}

/// Resolve `stroke` given the strokes already pending.
pub(crate) fn resolve(
    table: &[(Vec<Stroke>, EditorCommand)],
    pending: &[Stroke],
    stroke: &Stroke,
) -> Resolved {
    let mut seq: Vec<Stroke> = pending.to_vec();
    seq.push(stroke.clone());
    if let Some((_, c)) = table.iter().find(|(k, _)| *k == seq) {
        return Resolved::Run(*c);
    }
    if table
        .iter()
        .any(|(k, _)| k.len() > seq.len() && k.starts_with(&seq))
    {
        return Resolved::Pending;
    }
    Resolved::None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_chord_parses_and_ids_are_unique() {
        let mut ids = std::collections::HashSet::new();
        for &c in EditorCommand::ALL {
            assert!(ids.insert(c.id()), "duplicate id {}", c.id());
            for k in c.default_keys() {
                assert!(
                    parse_chord(k).is_some(),
                    "{k:?} for {} does not parse",
                    c.id()
                );
            }
            assert_eq!(EditorCommand::from_id(c.id()), Some(c));
        }
        // No two commands share a default chord.
        let t = key_table(&crate::Keymap::new());
        for (i, (k, c)) in t.iter().enumerate() {
            for (k2, c2) in t.iter().skip(i + 1) {
                assert!(k != k2 || c == c2, "{c:?} and {c2:?} share a chord");
            }
        }
    }

    #[test]
    fn resolve_single_and_multi_stroke() {
        let t = key_table(&crate::Keymap::new());
        let s = |c: &str| parse_chord(c).unwrap().remove(0);
        assert_eq!(
            resolve(&t, &[], &s("Alt+Up")),
            Resolved::Run(EditorCommand::MoveLinesUp)
        );
        assert_eq!(resolve(&t, &[], &s("Ctrl+K")), Resolved::Pending);
        assert_eq!(
            resolve(&t, &[s("Ctrl+K")], &s("Ctrl+0")),
            Resolved::Run(EditorCommand::FoldAll)
        );
        assert_eq!(resolve(&t, &[], &s("Ctrl+Q")), Resolved::None);
    }

    #[test]
    fn keymap_rebinds_and_unbinds_commands() {
        let km = crate::Keymap::new()
            .unbind_command(EditorCommand::MoveLinesUp)
            .bind_command("Ctrl+Shift+Up", EditorCommand::MoveLinesUp);
        let t = key_table(&km);
        let s = |c: &str| parse_chord(c).unwrap().remove(0);
        assert_eq!(resolve(&t, &[], &s("Alt+Up")), Resolved::None);
        assert_eq!(
            resolve(&t, &[], &s("Ctrl+Shift+Up")),
            Resolved::Run(EditorCommand::MoveLinesUp)
        );
    }
}
