//! Language-service UI driven by a fake [`LanguageService`] through the headless harness:
//! hover, signature help, navigation + peek, references, document highlights, rename,
//! diagnostics, code actions and formatting.

use std::cell::RefCell;
use std::rc::Rc;

use pebbles::core::create_root_signal;
use pebbles::prelude::*;
use pebbles_code_editor::code_editor;
use pebbles_code_editor::language::*;
use pebbles_testing::Harness;

/// A references list the host was asked to show: (origin, locations).
type Shown = (Location, Vec<Location>);

type Pending<T> = RefCell<Vec<(Position, Reply<T>)>>;

/// A language service whose replies are held until the test delivers them.
#[derive(Default)]
struct Fake {
    hovers: Pending<Option<Hover>>,
    sigs: Pending<Option<SignatureHelp>>,
    sig_cx: RefCell<Vec<SignatureHelpContext>>,
    /// Locations answered (synchronously) for definition / implementation / references.
    defs: RefCell<Vec<Location>>,
    impls: RefCell<Vec<Location>>,
    /// Other documents' text (for peek previews).
    files: RefCell<Vec<(String, String)>>,
    opened: RefCell<Vec<Location>>,
    refs: RefCell<Vec<Location>>,
    /// Host references view: `Some(shown)` records the call and answers it.
    refs_view: RefCell<Option<Vec<Shown>>>,
    /// Document highlights; `None` declines the request.
    highlights: RefCell<Option<Vec<DocumentHighlight>>>,
    hl_requests: RefCell<Vec<Position>>,
    /// `prepareRename`: `None` declines (word fallback).
    prepare: RefCell<Option<Option<Range>>>,
    rename_edit: RefCell<Option<WorkspaceEdit>>,
    renamed_to: RefCell<Vec<String>>,
    applied: RefCell<Vec<WorkspaceEdit>>,
    actions: RefCell<Vec<CodeAction>>,
    action_cx: RefCell<Vec<CodeActionContext>>,
    commands: RefCell<Vec<String>>,
    /// Formatting replies: whole document, range (records the range), on type.
    fmt: RefCell<Vec<TextEdit>>,
    range_fmt: RefCell<Option<Vec<TextEdit>>>,
    fmt_ranges: RefCell<Vec<Range>>,
    type_fmt: RefCell<Vec<(String, Position, FormattingOptions)>>,
    /// Document symbols (`None` declines).
    symbols: RefCell<Option<DocumentSymbolResponse>>,
    symbol_requests: RefCell<usize>,
    /// Workspace symbols (answered for every query; the queries are recorded).
    ws: RefCell<Vec<SymbolInformation>>,
    ws_queries: RefCell<Vec<String>>,
}

impl LanguageService for Fake {
    fn document_symbols(&self, _doc: &DocRef, reply: Reply<DocumentSymbolResponse>) -> bool {
        *self.symbol_requests.borrow_mut() += 1;
        match self.symbols.borrow().clone() {
            Some(r) => {
                reply(r);
                true
            }
            None => false,
        }
    }
    fn workspace_symbols(&self, query: &str, reply: Reply<Vec<SymbolInformation>>) -> bool {
        self.ws_queries.borrow_mut().push(query.to_string());
        let q = query.to_lowercase();
        reply(
            self.ws
                .borrow()
                .iter()
                .filter(|s| s.name.to_lowercase().contains(&q))
                .cloned()
                .collect(),
        );
        true
    }
    fn hover(&self, _doc: &DocRef, pos: Position, reply: Reply<Option<Hover>>) -> bool {
        self.hovers.borrow_mut().push((pos, reply));
        true
    }
    fn definition(&self, _doc: &DocRef, _pos: Position, reply: Reply<Vec<Location>>) -> bool {
        reply(self.defs.borrow().clone());
        true
    }
    fn implementation(&self, _doc: &DocRef, _pos: Position, reply: Reply<Vec<Location>>) -> bool {
        reply(self.impls.borrow().clone());
        true
    }
    fn references(
        &self,
        _doc: &DocRef,
        _pos: Position,
        include_declaration: bool,
        reply: Reply<Vec<Location>>,
    ) -> bool {
        assert!(include_declaration);
        reply(self.refs.borrow().clone());
        true
    }
    fn show_references(&self, origin: &Location, locations: &[Location]) -> bool {
        match &mut *self.refs_view.borrow_mut() {
            Some(v) => {
                v.push((origin.clone(), locations.to_vec()));
                true
            }
            None => false,
        }
    }
    fn document_highlight(
        &self,
        _doc: &DocRef,
        pos: Position,
        reply: Reply<Vec<DocumentHighlight>>,
    ) -> bool {
        self.hl_requests.borrow_mut().push(pos);
        match self.highlights.borrow().clone() {
            Some(v) => {
                reply(v);
                true
            }
            None => false,
        }
    }
    fn prepare_rename(&self, _doc: &DocRef, _pos: Position, reply: Reply<Option<Range>>) -> bool {
        match *self.prepare.borrow() {
            Some(r) => {
                reply(r);
                true
            }
            None => false,
        }
    }
    fn rename(
        &self,
        _doc: &DocRef,
        _pos: Position,
        new_name: &str,
        reply: Reply<Option<WorkspaceEdit>>,
    ) -> bool {
        self.renamed_to.borrow_mut().push(new_name.to_string());
        reply(self.rename_edit.borrow().clone());
        true
    }
    fn code_actions(
        &self,
        _doc: &DocRef,
        _range: Range,
        cx: CodeActionContext,
        reply: Reply<Vec<CodeAction>>,
    ) -> bool {
        let only = cx.only.clone();
        self.action_cx.borrow_mut().push(cx);
        reply(
            self.actions
                .borrow()
                .iter()
                .filter(|a| only.is_empty() || a.kind.as_ref().is_some_and(|k| only.contains(k)))
                .cloned()
                .collect(),
        );
        true
    }
    fn resolve_code_action(&self, action: CodeAction, reply: Reply<CodeAction>) -> bool {
        let mut a = action;
        a.edit = Some(WorkspaceEdit {
            changes: vec![(
                "file:///w/main.rs".into(),
                vec![edit(0, 0, 0, "// resolved\n")],
            )],
        });
        reply(a);
        true
    }
    fn on_type_formatting_trigger_characters(&self) -> Vec<String> {
        vec!["}".into()]
    }
    fn formatting(
        &self,
        _doc: &DocRef,
        _options: FormattingOptions,
        reply: Reply<Vec<TextEdit>>,
    ) -> bool {
        reply(self.fmt.borrow().clone());
        true
    }
    fn range_formatting(
        &self,
        _doc: &DocRef,
        range: Range,
        _options: FormattingOptions,
        reply: Reply<Vec<TextEdit>>,
    ) -> bool {
        let Some(e) = self.range_fmt.borrow().clone() else {
            return false;
        };
        self.fmt_ranges.borrow_mut().push(range);
        reply(e);
        true
    }
    fn on_type_formatting(
        &self,
        _doc: &DocRef,
        pos: Position,
        ch: &str,
        options: FormattingOptions,
        reply: Reply<Vec<TextEdit>>,
    ) -> bool {
        self.type_fmt
            .borrow_mut()
            .push((ch.to_string(), pos, options));
        reply(vec![edit(0, 0, 0, "    ")]);
        true
    }
    fn execute_command(&self, command: &LspCommand) -> bool {
        self.commands.borrow_mut().push(command.command.clone());
        true
    }
    fn apply_workspace_edit(&self, edit: &WorkspaceEdit) -> bool {
        self.applied.borrow_mut().push(edit.clone());
        true
    }
    fn read_text(&self, uri: &str) -> Option<String> {
        self.files
            .borrow()
            .iter()
            .find(|(u, _)| u == uri)
            .map(|(_, t)| t.clone())
    }
    fn open_location(&self, loc: &Location) -> bool {
        self.opened.borrow_mut().push(loc.clone());
        true
    }
    fn signature_help(
        &self,
        _doc: &DocRef,
        pos: Position,
        cx: SignatureHelpContext,
        reply: Reply<Option<SignatureHelp>>,
    ) -> bool {
        self.sig_cx.borrow_mut().push(cx);
        self.sigs.borrow_mut().push((pos, reply));
        true
    }
}

fn take<T>(p: &Pending<T>) -> (Position, Reply<T>) {
    p.borrow_mut().remove(0)
}

fn mount(
    text: &str,
    fake: Rc<Fake>,
    cfg: impl FnOnce(pebbles_code_editor::CodeEditor) -> pebbles_code_editor::CodeEditor,
) -> (Harness, Signal<String>) {
    let code = create_root_signal(String::from(text));
    let mut h = Harness::new().window(900.0, 500.0);
    h.mount_widget(cfg(code_editor(code)
        .language_service(fake)
        .document_uri("file:///w/main.rs")
        .word_based_suggestions(false)
        .builtin_snippets(false)
        .height(400.0)
        .autofocus()));
    h.frames(3);
    (h, code)
}

#[test]
fn hover_dwells_caches_renders_markdown_sections_and_shows_by_keyboard() {
    let diags = create_root_signal(vec![LspDiagnostic::new(
        Range::new(Position::new(0, 0), Position::new(0, 2)),
        DiagnosticSeverity::Warning,
        "unused function",
    )]);
    let fake = Rc::new(Fake::default());
    let (mut h, code) = mount("fn main() {\n    let value = 1;\n}\n", fake.clone(), |e| {
        e.lsp_diagnostics(diags)
    });
    // The pointer dwells: nothing is requested on the move itself, one request after the delay.
    let at = Offset::new(150.0, 40.0);
    h.hover(at);
    assert!(
        fake.hovers.borrow().is_empty(),
        "no request before the dwell delay"
    );
    h.advance(0.4);
    assert_eq!(fake.hovers.borrow().len(), 1);
    let (_, r) = take(&fake.hovers);
    r(Some(Hover {
        contents: MarkupContent::markdown(
            "```rust\nlet value: i32\n```\n---\nThe **value** binding.",
        ),
        range: None,
    }));
    h.frames(2);
    assert!(h.find_text("let value: i32"), "code section");
    assert!(h.find_text("The value binding."), "markdown section");
    // Escape hides it; dwelling at the same spot again is served from the cache.
    h.key(KeyInput::Escape);
    h.frames(1);
    assert!(!h.find_text("The value binding."));
    h.hover(Offset::new(150.5, 40.0));
    h.advance(0.4);
    assert!(
        fake.hovers.borrow().is_empty(),
        "cached per position + version"
    );
    assert!(h.find_text("The value binding."));
    h.key(KeyInput::Escape);
    // Ctrl+K Ctrl+I shows the hover at the caret (here over a diagnostic, shown first).
    h.press("Ctrl+K");
    h.press("Ctrl+I");
    let (pos, r) = take(&fake.hovers);
    assert_eq!(pos, Position::new(0, 0));
    r(Some(Hover {
        contents: MarkupContent::plain("fn main()"),
        range: None,
    }));
    h.frames(2);
    assert!(h.find_text("fn main()"));
    assert!(h.find_text("warning unused function"), "diagnostic section");
    // A reply that arrives after an edit is stale and dropped.
    h.key(KeyInput::Escape);
    h.key(KeyInput::Move {
        motion: Motion::Right,
        extend: false,
    });
    h.press("Ctrl+K");
    h.press("Ctrl+I");
    h.key(KeyInput::Insert("x".into()));
    let (_, r) = take(&fake.hovers);
    r(Some(Hover {
        contents: MarkupContent::plain("stale hover"),
        range: None,
    }));
    h.frames(2);
    assert!(!h.find_text("stale hover"));
    assert!(code.get().starts_with("fxn"));
}

fn overloads(active_signature: Option<u32>, active_parameter: u32) -> SignatureHelp {
    let p = |l: &str, d: Option<&str>| ParameterInformation {
        label: ParameterLabel::Simple(l.into()),
        documentation: d.map(MarkupContent::markdown),
    };
    SignatureHelp {
        signatures: vec![
            SignatureInformation {
                label: "f(a: i32, b: i32)".into(),
                documentation: Some(MarkupContent::markdown("Adds **two** numbers.")),
                parameters: vec![p("a: i32", Some("The *first* operand.")), p("b: i32", None)],
                active_parameter: None,
            },
            SignatureInformation {
                label: "f(s: &str)".into(),
                documentation: None,
                parameters: vec![p("s: &str", None)],
                active_parameter: None,
            },
        ],
        active_signature,
        active_parameter: Some(active_parameter),
    }
}

#[test]
fn signature_help_triggers_retriggers_cycles_overloads_and_closes() {
    let fake = Rc::new(Fake::default());
    let (mut h, code) = mount("f\n", fake.clone(), |e| e);
    h.key(KeyInput::Move {
        motion: Motion::Right,
        extend: false,
    });
    h.key(KeyInput::Insert("(".into()));
    {
        let cx = fake.sig_cx.borrow();
        assert_eq!(cx.len(), 1);
        assert_eq!(
            cx[0].trigger_kind,
            SignatureHelpTriggerKind::TriggerCharacter
        );
        assert_eq!(cx[0].trigger_character.as_deref(), Some("("));
        assert!(!cx[0].is_retrigger);
    }
    let (pos, r) = take(&fake.sigs);
    assert_eq!(pos, Position::new(0, 2));
    r(Some(overloads(None, 0)));
    h.frames(2);
    assert!(
        h.find_text("\u{2039} 1/2 \u{203a}  f(a: i32, b: i32)"),
        "overload counter + label"
    );
    assert!(h.find_text("The first operand."), "active parameter docs");
    assert!(h.find_text("Adds two numbers."), "signature docs");
    // Alt+Down shows the next overload (instead of moving the line).
    let before = code.get();
    h.press("Alt+Down");
    assert!(h.find_text("\u{2039} 2/2 \u{203a}  f(s: &str)"));
    assert_eq!(code.get(), before, "the line did not move");
    h.press("Alt+Down");
    assert!(
        h.find_text("\u{2039} 1/2 \u{203a}  f(a: i32, b: i32)"),
        "wraps around"
    );
    h.press("Alt+Up");
    // Typing while open retriggers with the active help (the picked overload).
    h.key(KeyInput::Insert("x".into()));
    {
        let cx = fake.sig_cx.borrow();
        let last = cx.last().unwrap();
        assert_eq!(last.trigger_kind, SignatureHelpTriggerKind::ContentChange);
        assert!(last.is_retrigger);
        assert_eq!(
            last.active.as_ref().and_then(|a| a.active_signature),
            Some(1)
        );
    }
    let (_, r) = take(&fake.sigs);
    r(Some(overloads(None, 0)));
    h.frames(1);
    assert!(
        h.find_text("\u{2039} 2/2 \u{203a}  f(s: &str)"),
        "picked overload kept"
    );
    // `,` is a trigger character; an empty reply closes the help.
    h.key(KeyInput::Insert(",".into()));
    assert_eq!(
        fake.sig_cx
            .borrow()
            .last()
            .unwrap()
            .trigger_character
            .as_deref(),
        Some(",")
    );
    let (_, r) = take(&fake.sigs);
    r(None);
    h.frames(1);
    assert!(!h.find_text("\u{2039} 2/2 \u{203a}  f(s: &str)"));
    // Ctrl+Shift+Space invokes it; Escape closes it.
    h.press("Ctrl+Shift+Space");
    assert_eq!(
        fake.sig_cx.borrow().last().unwrap().trigger_kind,
        SignatureHelpTriggerKind::Invoked
    );
    let (_, r) = take(&fake.sigs);
    r(Some(overloads(Some(0), 1)));
    h.frames(1);
    assert!(h.find_text("\u{2039} 1/2 \u{203a}  f(a: i32, b: i32)"));
    h.key(KeyInput::Escape);
    assert!(!h.find_text("\u{2039} 1/2 \u{203a}  f(a: i32, b: i32)"));
    assert!(fake.sigs.borrow().is_empty());
}

fn loc(uri: &str, line: u32, col: u32) -> Location {
    Location {
        uri: uri.into(),
        range: Range::new(Position::new(line, col), Position::new(line, col + 3)),
    }
}

#[test]
fn navigation_reveals_peeks_and_opens_other_files() {
    let fake = Rc::new(Fake::default());
    fake.files.borrow_mut().push((
        "file:///w/lib.rs".into(),
        "// lib\n\npub fn helper() {}\n".into(),
    ));
    let (mut h, code) = mount(
        "use lib;\nfn main() {\n    helper();\n}\n",
        fake.clone(),
        |e| e,
    );
    // F12 with one same-document result moves the caret there.
    *fake.defs.borrow_mut() = vec![loc("file:///w/main.rs", 1, 3)];
    h.press("F12");
    h.key(KeyInput::Insert("X".into()));
    assert_eq!(code.get(), "use lib;\nfn Xmain() {\n    helper();\n}\n");
    h.key(KeyInput::Undo);
    // A result in another document goes through the host's open_location.
    *fake.defs.borrow_mut() = vec![loc("file:///w/lib.rs", 2, 7)];
    h.press("F12");
    assert_eq!(fake.opened.borrow().len(), 1);
    assert_eq!(fake.opened.borrow()[0].uri, "file:///w/lib.rs");
    // Alt+F12 peeks even a single result: an inline preview read through read_text.
    h.press("Alt+F12");
    h.frames(2);
    assert!(h.find_text("Definition"), "peek title");
    assert!(
        h.find_text("pub fn helper() {}"),
        "embedded preview of the other file"
    );
    h.key(KeyInput::Escape);
    h.frames(1);
    assert!(!h.find_text("pub fn helper() {}"));
    // Several implementations (Ctrl+F12) open the peek with a file list; Down + Enter opens
    // the second one.
    *fake.impls.borrow_mut() = vec![
        loc("file:///w/main.rs", 2, 4),
        loc("file:///w/lib.rs", 2, 7),
    ];
    h.press("Ctrl+F12");
    h.frames(2);
    assert!(h.find_text("Implementations"));
    assert!(h.find_text("2 results"));
    assert!(h.find_text("main.rs:3"), "file list entry");
    assert!(h.find_text("lib.rs:3"), "file list entry");
    assert!(
        h.find_text("    helper();"),
        "preview of the selected result"
    );
    h.key(KeyInput::Move {
        motion: Motion::Down,
        extend: false,
    });
    h.frames(1);
    assert!(
        h.find_text("pub fn helper() {}"),
        "preview follows the selection"
    );
    h.key(KeyInput::Enter);
    h.frames(1);
    assert_eq!(fake.opened.borrow().len(), 2);
    assert!(!h.find_text("2 results"), "opening closes the peek");
    assert_eq!(
        code.get(),
        "use lib;\nfn main() {\n    helper();\n}\n",
        "Enter did not edit"
    );
}

#[test]
fn ctrl_hover_underlines_through_the_service_and_ctrl_click_navigates() {
    let fake = Rc::new(Fake::default());
    *fake.defs.borrow_mut() = vec![loc("file:///w/main.rs", 0, 0)];
    let (mut h, code) = mount("alpha beta\n", fake.clone(), |e| e.gutter(false));
    pebbles::core::keyboard::set_modifiers(false, false, false, false);
    let at = Offset::new(14.0 + 7.5 * 8.4, 18.0);
    h.hover(at);
    let plain = h.render_node_count();
    pebbles::core::keyboard::set_modifiers(false, true, false, false);
    h.hover(Offset::new(at.x + 1.0, at.y));
    h.frames(1);
    assert!(
        h.render_node_count() > plain,
        "underline under the Ctrl+hovered symbol"
    );
    h.click(Offset::new(at.x + 1.0, at.y));
    pebbles::core::keyboard::set_modifiers(false, false, false, false);
    h.key(KeyInput::Insert("X".into()));
    assert_eq!(
        code.get(),
        "Xalpha beta\n",
        "Ctrl+click went to the definition"
    );
}

#[test]
fn references_peek_and_host_view() {
    let fake = Rc::new(Fake::default());
    *fake.refs.borrow_mut() = vec![
        loc("file:///w/main.rs", 0, 0),
        loc("file:///w/main.rs", 2, 0),
        loc("file:///w/lib.rs", 1, 4),
    ];
    let (mut h, _code) = mount("foo();\nbar();\nfoo();\n", fake.clone(), |e| e);
    h.press("Shift+F12");
    h.frames(2);
    assert!(h.find_text("References"));
    assert!(h.find_text("3 results"));
    h.key(KeyInput::Escape);
    // Shift+Alt+F12 hands them to the host's references view (no peek)...
    *fake.refs_view.borrow_mut() = Some(Vec::new());
    h.press("Shift+Alt+F12");
    h.frames(1);
    {
        let shown = fake.refs_view.borrow();
        let shown = shown.as_ref().unwrap();
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].0.uri, "file:///w/main.rs");
        assert_eq!(shown[0].1.len(), 3);
    }
    assert!(!h.find_text("3 results"));
    // ...or falls back to the peek when the host has no such view.
    *fake.refs_view.borrow_mut() = None;
    h.press("Shift+Alt+F12");
    h.frames(1);
    assert!(h.find_text("3 results"));
}

#[test]
fn document_highlights_follow_the_caret_with_word_fallback() {
    let text = "foo x foo y foo\n";
    let count = |hl: Option<Vec<DocumentHighlight>>| {
        let fake = Rc::new(Fake::default());
        *fake.highlights.borrow_mut() = hl;
        let (mut h, _code) = mount(text, fake.clone(), |e| e);
        h.key(KeyInput::Move {
            motion: Motion::Right,
            extend: false,
        });
        h.frames(2);
        assert_eq!(
            fake.hl_requests.borrow().last(),
            Some(&Position::new(0, 1)),
            "requested at the moved caret"
        );
        h.render_node_count()
    };
    let r = |a: u32| Range::new(Position::new(0, a), Position::new(0, a + 3));
    let all = count(Some(vec![
        DocumentHighlight {
            range: r(0),
            kind: DocumentHighlightKind::Write,
        },
        DocumentHighlight {
            range: r(6),
            kind: DocumentHighlightKind::Read,
        },
        DocumentHighlight {
            range: r(12),
            kind: DocumentHighlightKind::Text,
        },
    ]));
    let one = count(Some(vec![DocumentHighlight {
        range: r(0),
        kind: DocumentHighlightKind::Write,
    }]));
    let fallback = count(None);
    assert!(all > one, "service highlights drawn ({all} vs {one})");
    assert!(
        fallback > one,
        "declined: the word occurrences are drawn ({fallback})"
    );
}

/// Click the centre of the widget whose accessible label is `label`.
fn tap_label(h: &mut Harness, label: &str) {
    let node = h
        .semantics_tree()
        .into_iter()
        .find(|n| n.props.label == label)
        .unwrap_or_else(|| panic!("no widget labelled {label:?}"));
    let b = node.bounds;
    h.click(Offset::new(b.x0 + b.width() / 2.0, b.y0 + b.height() / 2.0));
    h.frames(1);
}

fn edit(line: u32, col: u32, len: u32, new: &str) -> TextEdit {
    TextEdit::new(
        Range::new(Position::new(line, col), Position::new(line, col + len)),
        new,
    )
}

#[test]
fn rename_box_applies_one_undo_step_and_previews_cross_file_edits() {
    let fake = Rc::new(Fake::default());
    fake.files
        .borrow_mut()
        .push(("file:///w/lib.rs".into(), "use main::foo;\n".into()));
    let (mut h, code) = mount("let foo = 1;\nfoo + foo;\n", fake.clone(), |e| e);
    let original = code.get();
    for _ in 0..5 {
        h.key(KeyInput::Move {
            motion: Motion::Right,
            extend: false,
        });
    }
    // prepareRename gives the range; the box opens over it with the name selected.
    *fake.prepare.borrow_mut() = Some(Some(Range::new(Position::new(0, 4), Position::new(0, 7))));
    *fake.rename_edit.borrow_mut() = Some(WorkspaceEdit {
        changes: vec![(
            "file:///w/main.rs".into(),
            vec![
                edit(0, 4, 3, "bar"),
                edit(1, 0, 3, "bar"),
                edit(1, 6, 3, "bar"),
            ],
        )],
    });
    h.press("F2");
    h.frames(2);
    assert!(h.find_text("Enter to rename, Shift+Enter to preview"));
    for c in "bar".chars() {
        h.key(KeyInput::Insert(c.to_string()));
    }
    h.key(KeyInput::Enter);
    h.frames(1);
    assert_eq!(*fake.renamed_to.borrow(), vec!["bar".to_string()]);
    assert_eq!(code.get(), "let bar = 1;\nbar + bar;\n");
    assert!(
        fake.applied.borrow().is_empty(),
        "same-document edits applied locally"
    );
    assert!(
        !h.find_text("Enter to rename, Shift+Enter to preview"),
        "box closed"
    );
    h.key(KeyInput::Undo);
    assert_eq!(code.get(), original, "one undo step");
    // Edits across files open the preview; unticking a file leaves it out.
    *fake.prepare.borrow_mut() = None; // declined: the word at the caret
    *fake.rename_edit.borrow_mut() = Some(WorkspaceEdit {
        changes: vec![
            ("file:///w/main.rs".into(), vec![edit(0, 4, 3, "baz")]),
            ("file:///w/lib.rs".into(), vec![edit(0, 10, 3, "baz")]),
        ],
    });
    h.press("F2");
    h.frames(2);
    for c in "baz".chars() {
        h.key(KeyInput::Insert(c.to_string()));
    }
    h.key(KeyInput::Enter);
    h.frames(2);
    assert!(h.find_text("Rename: 2 edits in 2 files"));
    assert!(
        h.find_text("1: foo \u{2192} baz"),
        "each edit listed with its old text"
    );
    tap_label(&mut h, "lib.rs (1 edits)");
    tap_label(&mut h, "Apply");
    assert_eq!(code.get(), "let baz = 1;\nfoo + foo;\n");
    assert!(
        fake.applied.borrow().is_empty(),
        "the discarded file is not sent"
    );
    h.key(KeyInput::Undo);
    // Accept everything: the other file goes through the host's apply_workspace_edit.
    h.press("F2");
    h.frames(2);
    for c in "baz".chars() {
        h.key(KeyInput::Insert(c.to_string()));
    }
    h.key(KeyInput::Enter);
    h.frames(2);
    tap_label(&mut h, "Apply");
    assert_eq!(code.get(), "let baz = 1;\nfoo + foo;\n");
    let applied = fake.applied.borrow();
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].changes.len(), 1);
    assert_eq!(applied[0].changes[0].0, "file:///w/lib.rs");
}

fn diag(line: u32, a: u32, b: u32, sev: DiagnosticSeverity, msg: &str) -> LspDiagnostic {
    LspDiagnostic::new(
        Range::new(Position::new(line, a), Position::new(line, b)),
        sev,
        msg,
    )
}

#[test]
fn diagnostics_render_by_severity_count_navigate_and_hover() {
    let mut err = diag(
        0,
        4,
        7,
        DiagnosticSeverity::Error,
        "cannot find value `foo`",
    );
    err.code = Some("E0425".into());
    err.code_href = Some("https://doc.rust-lang.org/error_codes/E0425.html".into());
    err.source = Some("rustc".into());
    err.related_information = vec![DiagnosticRelatedInformation {
        location: loc("file:///w/lib.rs", 2, 4),
        message: "defined here".into(),
    }];
    let mut unused = diag(1, 4, 5, DiagnosticSeverity::Warning, "unused variable");
    let mut old = diag(2, 0, 3, DiagnosticSeverity::Warning, "deprecated fn");
    let hint = diag(3, 0, 1, DiagnosticSeverity::Hint, "consider");
    let info = diag(3, 0, 2, DiagnosticSeverity::Information, "note");
    let text = "let foo = 1;\nlet x = 2;\nold();\nok\n";
    let nodes = |diags: Vec<LspDiagnostic>| {
        let sig = create_root_signal(diags);
        let fake = Rc::new(Fake::default());
        let (mut h, _) = mount(text, fake, |e| e.lsp_diagnostics(sig));
        h.frames(1);
        h.render_node_count()
    };
    let plain = nodes(vec![
        err.clone(),
        unused.clone(),
        old.clone(),
        info.clone(),
        hint.clone(),
    ]);
    unused.tags = vec![DiagnosticTag::Unnecessary];
    old.tags = vec![DiagnosticTag::Deprecated];
    let tagged = nodes(vec![
        err.clone(),
        unused.clone(),
        old.clone(),
        info.clone(),
        hint.clone(),
    ]);
    assert!(
        tagged > plain,
        "faded + struck-through layers ({plain} -> {tagged})"
    );

    let sig = create_root_signal(vec![err, unused, old, info, hint]);
    let counts = create_root_signal(ProblemCounts::default());
    let opened = Rc::new(RefCell::new(Vec::<String>::new()));
    let o2 = opened.clone();
    let fake = Rc::new(Fake::default());
    let (mut h, _) = mount(text, fake, |e| {
        e.lsp_diagnostics(sig)
            .problem_counts(counts)
            .on_open_link(move |u| o2.borrow_mut().push(u.to_string()))
    });
    assert_eq!(
        counts.get(),
        ProblemCounts {
            errors: 1,
            warnings: 2,
            infos: 1,
            hints: 1
        }
    );
    // F8 walks the problems from the caret with an inline peek; Shift+F8 goes back.
    h.press("F8");
    h.frames(2);
    assert!(h.find_text("error cannot find value `foo`  rustc"));
    assert!(h.find_text("1 of 5 problems"));
    assert!(h.find_text("(E0425)"), "code shown");
    assert!(
        h.find_text("lib.rs(3, 5): defined here"),
        "related information"
    );
    h.press("F8");
    h.frames(1);
    assert!(h.find_text("2 of 5 problems"));
    h.press("Shift+F8");
    h.frames(1);
    assert!(h.find_text("1 of 5 problems"));
    h.key(KeyInput::Escape);
    h.frames(1);
    assert!(!h.find_text("1 of 5 problems"));
    // The caret is on the error: the keyboard hover shows its message (no service hover).
    h.press("Ctrl+K");
    h.press("Ctrl+I");
    h.frames(1);
    assert!(
        h.find_text("error cannot find value `foo`  rustc"),
        "hover shows the message"
    );
    // Counts follow the published diagnostics.
    sig.set(Vec::new());
    h.frames(2);
    assert_eq!(counts.get(), ProblemCounts::default());
    h.press("F8");
    h.frames(1);
    assert!(!h.find_text("1 of 5 problems"));
    let _ = opened;
}

#[test]
fn code_actions_lightbulb_menu_preferred_resolve_and_auto_fix() {
    let fake = Rc::new(Fake::default());
    let import = CodeAction {
        title: "Import `foo`".into(),
        kind: Some("quickfix".into()),
        is_preferred: true,
        edit: Some(WorkspaceEdit {
            changes: vec![(
                "file:///w/main.rs".into(),
                vec![edit(0, 0, 0, "use foo;\n")],
            )],
        }),
        command: Some(LspCommand {
            title: "organize".into(),
            command: "rust.organize".into(),
            arguments: vec![],
        }),
        ..Default::default()
    };
    let extract = CodeAction {
        title: "Extract into function".into(),
        kind: Some("refactor.extract".into()),
        data: Some("lazy".into()),
        ..Default::default()
    };
    let disabled = CodeAction {
        title: "Inline".into(),
        kind: Some("refactor.inline".into()),
        disabled: Some("not a call".into()),
        ..Default::default()
    };
    *fake.actions.borrow_mut() = vec![extract.clone(), disabled, import];
    let diags = create_root_signal(vec![diag(
        0,
        0,
        3,
        DiagnosticSeverity::Error,
        "unresolved foo",
    )]);
    let (mut h, code) = mount("foo();\n", fake.clone(), |e| e.lsp_diagnostics(diags));
    // The caret sits on the diagnostic: the background request carries it, and the
    // lightbulb shows in the gutter.
    {
        let cx = fake.action_cx.borrow();
        let last = cx.last().expect("lightbulb request");
        assert_eq!(last.trigger_kind, CodeActionTriggerKind::Automatic);
        assert_eq!(last.diagnostics.len(), 1);
    }
    assert!(h.find_text("Show Code Actions"), "lightbulb");
    // Ctrl+. opens the menu: preferred first, then refactors; disabled ones say why.
    h.press("Ctrl+.");
    h.frames(2);
    assert_eq!(
        fake.action_cx.borrow().last().unwrap().trigger_kind,
        CodeActionTriggerKind::Invoked
    );
    assert!(h.find_text("Quick Fix: Import `foo` \u{2605}"));
    assert!(h.find_text("Refactor: Extract into function"));
    assert!(h.find_text("Refactor: Inline (disabled: not a call)"));
    // Enter runs the selected (preferred) action: its edit, then its command.
    h.key(KeyInput::Enter);
    h.frames(1);
    assert_eq!(code.get(), "use foo;\nfoo();\n");
    assert_eq!(*fake.commands.borrow(), vec!["rust.organize".to_string()]);
    assert!(
        !h.find_text("Refactor: Extract into function"),
        "menu closed"
    );
    h.key(KeyInput::Undo);
    // The lightbulb opens the same menu; an action without edit/command is resolved first.
    tap_label(&mut h, "Show Code Actions");
    h.frames(1);
    assert!(h.find_text("Refactor: Extract into function"));
    h.key(KeyInput::Move {
        motion: Motion::Down,
        extend: false,
    });
    h.key(KeyInput::Enter);
    h.frames(1);
    assert_eq!(
        code.get(),
        "// resolved\nfoo();\n",
        "codeAction/resolve supplied the edit"
    );
    h.key(KeyInput::Undo);
    // Shift+Alt+. applies the single preferred quick fix directly.
    h.press("Shift+Alt+.");
    h.frames(1);
    assert_eq!(code.get(), "use foo;\nfoo();\n");
    assert!(
        fake.action_cx
            .borrow()
            .iter()
            .any(|c| c.only == vec!["quickfix".to_string()]),
        "auto fix asks for quick fixes only"
    );
}

#[test]
fn formatting_document_selection_on_type_and_paste_keep_cursors_in_one_undo_step() {
    let fake = Rc::new(Fake::default());
    // Document formatting: two edits, one before each of two cursors.
    *fake.fmt.borrow_mut() = vec![edit(0, 0, 0, "  "), edit(1, 0, 0, "  ")];
    let (mut h, code) = mount("ab\ncd\n", fake.clone(), |e| {
        e.format_on_type(true).format_on_paste(true).tab_size(2)
    });
    h.key(KeyInput::Move {
        motion: Motion::Right,
        extend: false,
    });
    h.press("Ctrl+Alt+Down"); // a second cursor below
    h.press("Shift+Alt+F");
    assert_eq!(code.get(), "  ab\n  cd\n");
    h.key(KeyInput::Insert("X".into()));
    assert_eq!(
        code.get(),
        "  aXb\n  cXd\n",
        "both cursors mapped through the edits"
    );
    h.key(KeyInput::Undo);
    h.key(KeyInput::Undo);
    assert_eq!(code.get(), "ab\ncd\n", "the format was one undo step");
    h.key(KeyInput::Escape);
    // Format selection: the selected range goes to range formatting.
    *fake.range_fmt.borrow_mut() = Some(vec![edit(1, 0, 2, "CD")]);
    h.key(KeyInput::Move {
        motion: Motion::DocStart,
        extend: false,
    });
    h.key(KeyInput::Move {
        motion: Motion::Down,
        extend: false,
    });
    h.key(KeyInput::Move {
        motion: Motion::LineEnd,
        extend: true,
    });
    h.press("Ctrl+K");
    h.press("Ctrl+F");
    assert_eq!(code.get(), "ab\nCD\n");
    assert_eq!(
        fake.fmt_ranges.borrow().last(),
        Some(&Range::new(Position::new(1, 0), Position::new(1, 2)))
    );
    h.key(KeyInput::Undo);
    // On type: `}` is a trigger character; the format is its own undo step.
    h.key(KeyInput::Move {
        motion: Motion::DocEnd,
        extend: false,
    });
    h.key(KeyInput::Insert("}".into()));
    {
        let t = fake.type_fmt.borrow();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].0, "}");
        assert_eq!(t[0].1, Position::new(2, 1));
        assert_eq!(t[0].2.tab_size, 2);
    }
    assert_eq!(code.get(), "    ab\ncd\n}");
    h.key(KeyInput::Undo);
    assert_eq!(
        code.get(),
        "ab\ncd\n}",
        "undo removes only the on-type format"
    );
    // Paste: the pasted range is range-formatted.
    *fake.range_fmt.borrow_mut() = Some(vec![]);
    pebbles::core::clipboard::write("xy");
    h.key(KeyInput::Paste);
    assert_eq!(code.get(), "ab\ncd\n}xy");
    assert_eq!(
        fake.fmt_ranges.borrow().last(),
        Some(&Range::new(Position::new(2, 1), Position::new(2, 3))),
        "the pasted text's range"
    );
}

fn rng(l0: u32, c0: u32, l1: u32, c1: u32) -> Range {
    Range::new(Position::new(l0, c0), Position::new(l1, c1))
}

fn sym_info(
    name: &str,
    kind: SymbolKind,
    uri: &str,
    r: Range,
    container: Option<&str>,
) -> SymbolInformation {
    SymbolInformation {
        name: name.into(),
        kind,
        deprecated: false,
        location: Location {
            uri: uri.into(),
            range: r,
        },
        container_name: container.map(String::from),
    }
}

fn type_text(h: &mut Harness, s: &str) {
    for c in s.chars() {
        h.key(KeyInput::Insert(c.to_string()));
    }
    h.frames(1);
}

#[test]
fn document_symbols_outline_go_to_symbol_with_preview_and_workspace_symbols() {
    let text = "fn alpha() {}\nstruct Beta {\n    gamma: u8,\n}\nfn delta() {}\n";
    let fake = Rc::new(Fake::default());
    // A flat reply is nested by range containment.
    *fake.symbols.borrow_mut() = Some(DocumentSymbolResponse::Flat(vec![
        sym_info(
            "alpha",
            SymbolKind::Function,
            "file:///w/main.rs",
            rng(0, 0, 0, 13),
            None,
        ),
        sym_info(
            "Beta",
            SymbolKind::Struct,
            "file:///w/main.rs",
            rng(1, 0, 3, 1),
            None,
        ),
        sym_info(
            "gamma",
            SymbolKind::Field,
            "file:///w/main.rs",
            rng(2, 4, 2, 13),
            Some("Beta"),
        ),
        sym_info(
            "delta",
            SymbolKind::Function,
            "file:///w/main.rs",
            rng(4, 0, 4, 13),
            None,
        ),
    ]));
    let outline = create_root_signal(Vec::<DocumentSymbol>::new());
    let (mut h, code) = mount(text, fake.clone(), |e| e.outline(outline));
    let o = outline.get();
    assert_eq!(o.len(), 3, "outline published: {o:?}");
    assert_eq!(o[1].name, "Beta");
    assert_eq!(o[1].children[0].name, "gamma");
    let asked = *fake.symbol_requests.borrow();
    h.frames(2);
    assert_eq!(
        *fake.symbol_requests.borrow(),
        asked,
        "not re-requested without an edit"
    );

    // Ctrl+Shift+O lists every symbol (nested ones indented) in document order.
    h.press("Ctrl+Shift+O");
    h.frames(2);
    for n in ["alpha", "Beta", "  gamma  Beta", "delta"] {
        assert!(h.find_text(n), "{n} listed");
    }
    // Moving previews: the caret jumps to the symbol; Escape restores it.
    h.key(KeyInput::Move {
        motion: Motion::Down,
        extend: false,
    });
    h.key(KeyInput::Move {
        motion: Motion::Down,
        extend: false,
    });
    h.frames(2);
    h.key(KeyInput::Escape);
    h.frames(2);
    h.key(KeyInput::Insert("X".into()));
    assert!(
        code.get().starts_with("Xfn alpha"),
        "cancel restores: {}",
        code.get()
    );
    h.key(KeyInput::Undo);
    // `@:` groups by kind with the category label on each group's first entry.
    h.press("Ctrl+Shift+O");
    h.frames(2);
    type_text(&mut h, ":");
    assert!(h.find_text("alpha    functions (2)"));
    assert!(h.find_text("gamma  Beta    fields (1)"));
    h.key(KeyInput::Escape);
    h.frames(2);
    // Filtering then Enter reveals the symbol's name.
    h.press("Ctrl+Shift+O");
    h.frames(2);
    type_text(&mut h, "del");
    assert!(h.find_text("delta"));
    assert!(!h.find_text("alpha"));
    h.key(KeyInput::Enter);
    h.frames(2);
    h.key(KeyInput::Insert("X".into()));
    assert!(
        code.get().contains("\nXfn delta"),
        "accepted: {}",
        code.get()
    );
    h.key(KeyInput::Undo);

    // Ctrl+T queries the workspace through the service; accepting opens other files via the host.
    fake.ws.borrow_mut().push(sym_info(
        "Widget",
        SymbolKind::Class,
        "file:///w/ui.rs",
        rng(7, 4, 7, 10),
        Some("ui"),
    ));
    h.press("Ctrl+T");
    h.frames(2);
    type_text(&mut h, "wid");
    h.frames(1);
    assert_eq!(
        fake.ws_queries.borrow().last().map(String::as_str),
        Some("wid")
    );
    assert!(h.find_text("Widget  ui  \u{2014} ui.rs"));
    h.key(KeyInput::Enter);
    h.frames(1);
    let opened = fake.opened.borrow();
    assert_eq!(opened.last().unwrap().uri, "file:///w/ui.rs");
    assert_eq!(opened.last().unwrap().range.start, Position::new(7, 4));
}
