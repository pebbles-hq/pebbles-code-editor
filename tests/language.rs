//! Language-service driven features through the headless harness: LSP completion (trigger
//! characters, async + stale replies, fuzzy ranking, docs pane, commit characters,
//! auto-import edits in one undo step, commands, incomplete re-query) and snippet sessions.

use std::cell::RefCell;
use std::rc::Rc;

use pebbles::core::create_root_signal;
use pebbles::prelude::*;
use pebbles_code_editor::code_editor;
use pebbles_code_editor::language::*;
use pebbles_testing::Harness;

type Pending = Rc<RefCell<Vec<(CompletionContext, Reply<Option<CompletionList>>)>>>;

/// A language service whose completion replies are held until the test delivers them.
#[derive(Default)]
struct Lsp {
    pending: Pending,
    items: Vec<CompletionItem>,
    incomplete: bool,
    commands: Rc<RefCell<Vec<String>>>,
    /// When set, `completionItem/resolve` is supported and its replies are held here.
    resolves: Option<Rc<RefCell<Vec<(CompletionItem, Reply<CompletionItem>)>>>>,
}

impl LanguageService for Lsp {
    fn completion_trigger_characters(&self) -> Vec<String> {
        vec![".".into()]
    }
    fn completion(
        &self,
        _doc: &DocRef,
        _pos: Position,
        cx: CompletionContext,
        reply: Reply<Option<CompletionList>>,
    ) -> bool {
        self.pending.borrow_mut().push((cx, reply));
        true
    }
    fn resolve_completion(&self, item: CompletionItem, reply: Reply<CompletionItem>) -> bool {
        match &self.resolves {
            Some(r) => {
                r.borrow_mut().push((item, reply));
                true
            }
            None => false,
        }
    }
    fn execute_command(&self, command: &LspCommand) -> bool {
        self.commands.borrow_mut().push(command.command.clone());
        true
    }
}

impl Lsp {
    fn deliver(&self, h: &mut Harness, which: usize) {
        let (_, r) = self.pending.borrow_mut().remove(which);
        r(Some(CompletionList {
            is_incomplete: self.incomplete,
            items: self.items.clone(),
        }));
        h.frames(2);
    }
}

fn typ(h: &mut Harness, s: &str) {
    for c in s.chars() {
        h.key(KeyInput::Insert(c.to_string()));
    }
    h.frames(1);
}

fn mount(code: Signal<String>, lsp: Rc<Lsp>) -> Harness {
    let mut h = Harness::new().window(900.0, 500.0);
    h.mount_widget(
        code_editor(code)
            .language_service(lsp)
            .document_uri("file:///w/main.rs")
            .word_based_suggestions(false)
            .builtin_snippets(false)
            .height(400.0)
            .autofocus(),
    );
    h.frames(3);
    h
}

#[test]
fn trigger_character_async_reply_docs_pane_and_auto_import() {
    let code = create_root_signal(String::from("\nlet v = x"));
    let mut len = CompletionItem::new("len", CompletionItemKind::Method)
        .detail("fn len(&self) -> usize")
        .documentation("Returns the **length** of `self`.\n\n```rust\nlet n = v.len();\n```");
    len.additional_text_edits = vec![TextEdit::new(
        Range::new(Position::new(0, 0), Position::new(0, 0)),
        "use std::vec::Vec;",
    )];
    len.command = Some(LspCommand {
        title: "Trigger parameter hints".into(),
        command: "editor.action.triggerParameterHints".into(),
        arguments: vec![],
    });
    let lsp = Rc::new(Lsp {
        items: vec![
            CompletionItem::new("last", CompletionItemKind::Method),
            len,
            CompletionItem::new("capacity", CompletionItemKind::Method),
        ],
        ..Default::default()
    });
    let mut h = mount(code, lsp.clone());
    h.key(KeyInput::Move {
        motion: Motion::DocEnd,
        extend: false,
    });
    typ(&mut h, ".");
    // `.` is a trigger character: one request, marked as such, still pending.
    assert_eq!(lsp.pending.borrow().len(), 1);
    assert_eq!(
        lsp.pending.borrow()[0].0.trigger_kind,
        CompletionTriggerKind::TriggerCharacter
    );
    assert_eq!(
        lsp.pending.borrow()[0].0.trigger_character.as_deref(),
        Some(".")
    );
    assert!(!h.find_text("capacity"), "no popup before the reply");
    lsp.deliver(&mut h, 0);
    assert!(h.find_text("capacity"), "popup opens on the async reply");
    // Typing filters the open list locally (no re-query): "le" fuzzy-matches only "len".
    typ(&mut h, "le");
    assert!(
        lsp.pending.borrow().is_empty(),
        "complete list is not re-queried"
    );
    assert!(!h.find_text("capacity"));
    // The details line and the Markdown documentation render in the side pane.
    assert!(h.find_text("fn len(&self) -> usize"), "detail line");
    assert!(
        h.find_text("Returns the length of self."),
        "markdown paragraph"
    );
    assert!(h.find_text("let n = v.len();"), "highlighted code block");
    h.key(KeyInput::Enter);
    h.frames(1);
    assert_eq!(code.get(), "use std::vec::Vec;\nlet v = x.len");
    assert_eq!(
        *lsp.commands.borrow(),
        vec!["editor.action.triggerParameterHints".to_string()]
    );
    // Main edit + auto-import are one undo step.
    h.key(KeyInput::Undo);
    h.frames(1);
    assert_eq!(code.get(), "\nlet v = x.le");
}

#[test]
fn accept_waits_briefly_for_resolve_to_bring_additional_edits() {
    let resolves = Rc::new(RefCell::new(Vec::new()));
    let lsp = Rc::new(Lsp {
        items: vec![CompletionItem::new("HashMap", CompletionItemKind::Struct)],
        resolves: Some(resolves.clone()),
        ..Default::default()
    });
    let code = create_root_signal(String::from("\n"));
    let mut h = mount(code, lsp.clone());
    h.key(KeyInput::Move {
        motion: Motion::DocEnd,
        extend: false,
    });
    typ(&mut h, "Ha");
    let n = lsp.pending.borrow().len();
    lsp.deliver(&mut h, n - 1);
    resolves.borrow_mut().clear(); // the popup's own resolve of the highlighted item
    h.key(KeyInput::Enter);
    assert_eq!(code.get(), "\nHa", "waiting for the resolve reply");
    let (item, reply) = resolves.borrow_mut().remove(0);
    let mut r = item;
    r.additional_text_edits = vec![TextEdit::new(
        Range::new(Position::new(0, 0), Position::new(0, 0)),
        "use std::collections::HashMap;",
    )];
    reply(r);
    h.frames(1);
    assert_eq!(code.get(), "use std::collections::HashMap;\nHashMap");
    h.key(KeyInput::Undo);
    assert_eq!(code.get(), "\nHa", "still one undo step");
    // No reply in time: the item applies as it is after the short timeout.
    typ(&mut h, "s");
    let n = lsp.pending.borrow().len();
    lsp.deliver(&mut h, n - 1);
    resolves.borrow_mut().clear();
    h.key(KeyInput::Enter);
    assert_eq!(code.get(), "\nHas");
    h.advance(0.3);
    assert_eq!(code.get(), "\nHashMap");
    // A late reply does not apply twice.
    let (item, reply) = resolves.borrow_mut().remove(0);
    reply(item);
    h.frames(1);
    assert_eq!(code.get(), "\nHashMap");
}

#[test]
fn stale_completion_replies_are_dropped() {
    let code = create_root_signal(String::new());
    let lsp = Rc::new(Lsp {
        items: vec![CompletionItem::new(
            "alpha_beta",
            CompletionItemKind::Variable,
        )],
        ..Default::default()
    });
    let mut h = mount(code, lsp.clone());
    typ(&mut h, "a"); // request 1
    typ(&mut h, "b"); // request 2 (the document changed: request 1 is stale)
    assert_eq!(lsp.pending.borrow().len(), 2);
    lsp.deliver(&mut h, 0);
    assert!(!h.find_text("alpha_beta"), "stale reply ignored");
    lsp.deliver(&mut h, 0);
    assert!(h.find_text("alpha_beta"), "current reply shown");
    h.key(KeyInput::Enter);
    h.frames(1);
    assert_eq!(code.get(), "alpha_beta");
}

#[test]
fn commit_characters_accept_then_type() {
    let code = create_root_signal(String::new());
    let mut item = CompletionItem::new("println", CompletionItemKind::Function);
    item.commit_characters = vec!["(".into()];
    let lsp = Rc::new(Lsp {
        items: vec![item],
        ..Default::default()
    });
    let mut h = mount(code, lsp.clone());
    typ(&mut h, "pl");
    let n = lsp.pending.borrow().len();
    lsp.deliver(&mut h, n - 1);
    typ(&mut h, "(");
    assert!(code.get().starts_with("println("), "{:?}", code.get());
}

#[test]
fn incomplete_lists_requery_and_ctrl_space_opens_with_nothing_typed() {
    let code = create_root_signal(String::new());
    let lsp = Rc::new(Lsp {
        items: vec![CompletionItem::new("foo_bar", CompletionItemKind::Function)],
        incomplete: true,
        ..Default::default()
    });
    let mut h = mount(code, lsp.clone());
    h.key(KeyInput::TriggerCompletion);
    assert_eq!(lsp.pending.borrow().len(), 1);
    assert_eq!(
        lsp.pending.borrow()[0].0.trigger_kind,
        CompletionTriggerKind::Invoked
    );
    lsp.deliver(&mut h, 0);
    assert!(
        h.find_text("foo_bar"),
        "explicit request opens with an empty prefix"
    );
    typ(&mut h, "f");
    assert_eq!(lsp.pending.borrow().len(), 1, "incomplete list re-queried");
    assert_eq!(
        lsp.pending.borrow()[0].0.trigger_kind,
        CompletionTriggerKind::TriggerForIncompleteCompletions
    );
}

#[test]
fn fuzzy_ranking_and_sort_text_order_the_popup() {
    let code = create_root_signal(String::new());
    let mut z = CompletionItem::new("fbz", CompletionItemKind::Variable);
    z.sort_text = Some("0".into());
    let lsp = Rc::new(Lsp {
        items: vec![
            CompletionItem::new("fabric", CompletionItemKind::Variable),
            CompletionItem::new("fooBar", CompletionItemKind::Variable),
            CompletionItem::new("xfb", CompletionItemKind::Variable), // no strong first match
            z,
        ],
        ..Default::default()
    });
    let mut h = mount(code, lsp.clone());
    typ(&mut h, "fb");
    let n = lsp.pending.borrow().len();
    lsp.deliver(&mut h, n - 1);
    assert!(!h.find_text("xfb"), "first char must match a word start");
    // "fbz" (contiguous prefix) ranks first, then camelCase "fooBar" above "fabric".
    h.key(KeyInput::Enter);
    h.frames(1);
    assert_eq!(code.get(), "fbz");
    for _ in 0..3 {
        h.key(KeyInput::Backspace);
    }
    typ(&mut h, "fb");
    let n = lsp.pending.borrow().len();
    lsp.deliver(&mut h, n - 1);
    h.key(KeyInput::Move {
        motion: Motion::Down,
        extend: false,
    });
    h.key(KeyInput::Enter);
    h.frames(1);
    assert_eq!(code.get(), "fooBar");
}

#[test]
fn old_providers_still_complete_through_the_adapter() {
    use pebbles_code_editor::{
        CompletionContext as Cx, CompletionItem as Old, CompletionKind, CompletionProvider,
    };
    let code = create_root_signal(String::new());
    let p: CompletionProvider =
        Rc::new(|_: &Cx| vec![Old::new("println", CompletionKind::Function)]);
    let mut h = Harness::new().window(900.0, 500.0);
    h.mount_widget(code_editor(code).completion(p).height(300.0).autofocus());
    h.frames(3);
    typ(&mut h, "pn"); // fuzzy: p…n
    h.key(KeyInput::Enter);
    h.frames(1);
    assert_eq!(code.get(), "println");
}

fn snippet_editor(code: Signal<String>, snips: Vec<pebbles_code_editor::Snippet>) -> Harness {
    let mut h = Harness::new().window(900.0, 500.0);
    h.mount_widget(
        code_editor(code)
            .snippets(snips)
            .document_uri("file:///w/src/main.rs")
            .word_based_suggestions(false)
            .builtin_snippets(false)
            .height(400.0)
            .autofocus(),
    );
    h.frames(3);
    h
}

#[test]
fn snippet_session_mirrors_choices_tab_stops_and_final_stop() {
    use pebbles_code_editor::Snippet;
    let code = create_root_signal(String::new());
    let mut h = snippet_editor(
        code,
        vec![Snippet::new(
            "tag",
            "<${1:div}>${2|alpha,beta,gamma|}</$1>$0",
            "Tag",
        )],
    );
    typ(&mut h, "tag");
    h.key(KeyInput::Enter);
    h.frames(1);
    assert_eq!(code.get(), "<div>alpha</div>");
    // The placeholder is selected and its mirror has a cursor: typing renames both.
    typ(&mut h, "span");
    assert_eq!(code.get(), "<span>alpha</span>");
    // Tab: the choice stop, with its options offered.
    h.key(KeyInput::Indent);
    h.frames(1);
    assert!(h.find_text("gamma"), "choice list open");
    h.key(KeyInput::Move {
        motion: Motion::Down,
        extend: false,
    });
    h.key(KeyInput::Enter);
    h.frames(1);
    assert_eq!(code.get(), "<span>beta</span>");
    // Tab: the final stop ends the session with the caret after the snippet.
    h.key(KeyInput::Indent);
    typ(&mut h, "!");
    assert_eq!(code.get(), "<span>beta</span>!");
    // Session over: Tab indents again.
    h.key(KeyInput::Indent);
    h.frames(1);
    assert_ne!(code.get(), "<span>beta</span>!");
}

#[test]
fn inactive_snippet_placeholders_get_a_background_highlight() {
    use pebbles_code_editor::Snippet;
    let code = create_root_signal(String::new());
    let mut h = snippet_editor(
        code,
        vec![Snippet::new("pair", "${1:a} ${2:b} ${3:c}", "Pair")],
    );
    typ(&mut h, "pair");
    h.key(KeyInput::Enter);
    h.frames(1);
    assert_eq!(code.get(), "a b c");
    let live = h.render_node_count();
    h.key(KeyInput::Escape); // session over: no placeholder boxes
    h.frames(1);
    let done = h.render_node_count();
    assert!(
        live >= done + 3,
        "boxes for $2, $3 and $0 ({live} vs {done})"
    );
}

#[test]
fn snippet_shift_tab_nested_placeholders_and_escape() {
    use pebbles_code_editor::Snippet;
    let code = create_root_signal(String::new());
    let mut h = snippet_editor(
        code,
        vec![
            Snippet::new("call", "${1:foo(${2:bar})} ${3:baz}", "Call"),
            Snippet::new("pair", "${1:a} ${2:b}", "Pair"),
        ],
    );
    typ(&mut h, "call");
    h.key(KeyInput::Enter);
    h.frames(1);
    assert_eq!(code.get(), "foo(bar) baz");
    h.key(KeyInput::Indent); // nested $2
    typ(&mut h, "x");
    assert_eq!(code.get(), "foo(x) baz");
    h.key(KeyInput::Indent); // $3
    h.key(KeyInput::Outdent); // back to $2
    typ(&mut h, "y");
    assert_eq!(code.get(), "foo(y) baz");
    // Esc leaves the session: Tab no longer jumps (it indents).
    h.key(KeyInput::Escape);
    h.key(KeyInput::Indent);
    h.frames(1);
    assert_ne!(code.get(), "foo(y) baz");
    assert!(code.get().starts_with("foo(y"));
}

#[test]
fn lsp_snippet_completion_resolves_variables_and_selects_the_placeholder() {
    let code = create_root_signal(String::new());
    let lsp = Rc::new(Lsp {
        items: vec![
            CompletionItem::new("todo", CompletionItemKind::Snippet)
                .snippet("// ${TM_FILENAME}: ${1:what} ($CURRENT_YEAR)"),
        ],
        ..Default::default()
    });
    let mut h = mount(code, lsp.clone());
    typ(&mut h, "todo");
    let n = lsp.pending.borrow().len();
    lsp.deliver(&mut h, n - 1);
    h.key(KeyInput::Enter);
    h.frames(1);
    let year = code.get();
    assert!(year.starts_with("// main.rs: what ("), "{year}");
    typ(&mut h, "fix");
    assert!(
        code.get().starts_with("// main.rs: fix ("),
        "{}",
        code.get()
    );
}

#[test]
fn emmet_expansion_tabs_through_its_slots() {
    let code = create_root_signal(String::new());
    let mut h = Harness::new().window(900.0, 500.0);
    h.mount_widget(
        code_editor(code)
            .language(pebbles_code_editor::lang::by_name("html"))
            .emmet(true)
            .word_based_suggestions(false)
            .builtin_snippets(false)
            .height(400.0)
            .autofocus(),
    );
    h.frames(3);
    typ(&mut h, "a");
    h.key(KeyInput::Indent);
    h.frames(1);
    assert_eq!(code.get(), "<a href=\"\"></a>");
    typ(&mut h, "u");
    h.key(KeyInput::Indent);
    typ(&mut h, "t");
    assert_eq!(code.get(), "<a href=\"u\">t</a>");
}
