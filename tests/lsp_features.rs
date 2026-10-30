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
}

impl LanguageService for Fake {
    fn hover(&self, _doc: &DocRef, pos: Position, reply: Reply<Option<Hover>>) -> bool {
        self.hovers.borrow_mut().push((pos, reply));
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
