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

type Pending<T> = RefCell<Vec<(Position, Reply<T>)>>;

/// A language service whose replies are held until the test delivers them.
#[derive(Default)]
struct Fake {
    hovers: Pending<Option<Hover>>,
    sigs: Pending<Option<SignatureHelp>>,
    sig_cx: RefCell<Vec<SignatureHelpContext>>,
}

impl LanguageService for Fake {
    fn hover(&self, _doc: &DocRef, pos: Position, reply: Reply<Option<Hover>>) -> bool {
        self.hovers.borrow_mut().push((pos, reply));
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
