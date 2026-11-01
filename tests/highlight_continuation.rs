//! A budgeted [`SyntaxSession`] that stops short reports `needs_more()`, and the editor
//! keeps scheduling frames until highlighting completes — no user input involved.

use std::cell::Cell;
use std::rc::Rc;

use pebbles_code_editor::{Language, SyntaxSession, Token, TokenKind, code_editor};
use pebbles_testing::Harness;

/// Highlights at most one more line per `highlight_range` call (a stand-in for a
/// time-budgeted tokenizer) and reports `needs_more()` until every line is done.
struct Budgeted {
    done_lines: Rc<Cell<usize>>,
    calls: Rc<Cell<usize>>,
    total: Rc<Cell<usize>>,
}

impl SyntaxSession for Budgeted {
    fn update(&mut self, src: &str) {
        self.total.set(src.lines().count());
    }
    fn highlight_range(&mut self, src: &str, _range: std::ops::Range<usize>) -> Vec<Token> {
        self.calls.set(self.calls.get() + 1);
        let n = (self.done_lines.get() + 1).min(self.total.get());
        self.done_lines.set(n);
        let mut out = Vec::new();
        let mut at = 0;
        for line in src.split_inclusive('\n').take(n) {
            out.push(Token {
                start: at,
                len: line.trim_end().len(),
                kind: TokenKind::Keyword,
                scope: 0,
            });
            at += line.len();
        }
        out
    }
    fn needs_more(&self) -> bool {
        self.done_lines.get() < self.total.get()
    }
}

struct Lang(Rc<Cell<usize>>, Rc<Cell<usize>>, Rc<Cell<usize>>);

impl Language for Lang {
    fn name(&self) -> &str {
        "budgeted"
    }
    fn highlight(&self, _src: &str) -> Vec<Token> {
        Vec::new()
    }
    fn session(&self) -> Option<Box<dyn SyntaxSession>> {
        Some(Box::new(Budgeted {
            done_lines: self.0.clone(),
            calls: self.1.clone(),
            total: self.2.clone(),
        }))
    }
}

#[test]
fn needs_more_schedules_frames_until_highlighting_completes() {
    let done = Rc::new(Cell::new(0));
    let calls = Rc::new(Cell::new(0));
    let total = Rc::new(Cell::new(usize::MAX));
    let code = pebbles::core::create_root_signal("a\nb\nc\nd\ne\nf\n".to_string());
    let mut h = Harness::new().window(600.0, 400.0);
    h.mount_widget(
        code_editor(code)
            .language(Box::new(Lang(done.clone(), calls.clone(), total.clone())))
            .height(300.0),
    );
    h.frames(1);
    assert!(done.get() < 6, "the budget stops short on the first frame");
    // Only timers + frames from here: no key, pointer or scroll input.
    for _ in 0..20 {
        h.tick(0.016);
    }
    assert_eq!(done.get(), 6, "continuation frames finished every line");
    let settled = calls.get();
    for _ in 0..5 {
        h.tick(0.016);
    }
    assert_eq!(
        calls.get(),
        settled,
        "no more continuation once needs_more() is false"
    );
}
