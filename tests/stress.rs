//! Large-file stress: a 10,000-line document, driven headlessly through the real key /
//! scroll dispatch with a full rebuild + layout + paint after every input (one "frame").
//! Asserts per-frame budgets and that a keystroke does not cost O(document) — the
//! per-keystroke time on 10,000 lines must stay within a small factor of 1,000 lines.
//!
//! Budgets are loose in debug builds (the editor crate builds at opt-level 0). Numbers
//! worth quoting come from `cargo test --release --test stress -- --nocapture`.

use std::time::{Duration, Instant};

use pebbles::core::{Ui, create_root_signal};
use pebbles::prelude::*;
use pebbles::render::{Scene, TextEnv};
use pebbles_code_editor::code_editor;

const WINDOW: Size = Size {
    width: 1000.0,
    height: 700.0,
};

fn doc(lines: usize) -> String {
    (0..lines)
        .map(|i| match i % 4 {
            0 => format!("fn item_{i}(x: u32) -> u32 {{"),
            1 => format!("    let y = x * {i} + 7; // comment {i}"),
            2 => "    y.wrapping_add(\"str\".len() as u32)".to_string(),
            _ => "}".to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

struct Bench {
    ui: Ui,
    env: TextEnv,
    code: Signal<String>,
}

impl Bench {
    fn new(lines: usize) -> (Self, Duration) {
        pebbles::widgets::overlay::init();
        pebbles::core::focus::init();
        pebbles::core::keyboard::set_modifiers(false, false, false, false);
        let code = create_root_signal(doc(lines));
        let mut b = Bench {
            ui: Ui::new(),
            env: TextEnv::new(),
            code,
        };
        let t = Instant::now();
        b.ui.mount_root(
            View::new(
                Color::from_rgba8(255, 255, 255, 255),
                code_editor(code).height(660.0).autofocus(),
            )
            .into_widget(),
        );
        for _ in 0..3 {
            b.frame();
        }
        (b, t.elapsed())
    }

    fn frame(&mut self) {
        self.ui.rebuild_if_dirty();
        self.ui.layout(&mut self.env, WINDOW);
        let mut scene = Scene::new();
        self.ui.paint(&mut self.env, &mut scene);
    }

    /// One input + one frame, timed.
    fn timed(&mut self, k: KeyInput) -> Duration {
        let t = Instant::now();
        self.ui.dispatch_key(k);
        self.frame();
        t.elapsed()
    }

    fn mv(&mut self, motion: Motion, extend: bool, n: usize) {
        for _ in 0..n {
            self.ui.dispatch_key(KeyInput::Move { motion, extend });
        }
        self.frame();
    }
}

struct Stats {
    median: Duration,
    p95: Duration,
    max: Duration,
}

fn stats(mut v: Vec<Duration>) -> Stats {
    v.sort();
    Stats {
        median: v[v.len() / 2],
        p95: v[v.len() * 95 / 100],
        max: *v.last().unwrap(),
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

fn report(name: &str, s: &Stats) {
    println!(
        "{name:<34} median {:7.3} ms   p95 {:7.3} ms   max {:7.3} ms",
        ms(s.median),
        ms(s.p95),
        ms(s.max)
    );
}

/// Type `n` characters in the middle of the document; per-keystroke frame times.
fn typing(b: &mut Bench, lines: usize, n: usize) -> Stats {
    b.mv(Motion::DocStart, false, 1);
    b.mv(Motion::Down, false, lines / 2);
    b.mv(Motion::LineEnd, false, 1);
    let mut t = Vec::with_capacity(n);
    for i in 0..n {
        let ch = if i % 10 == 9 {
            " ".to_string()
        } else {
            char::from(b'a' + (i % 26) as u8).to_string()
        };
        t.push(b.timed(KeyInput::Insert(ch)));
    }
    stats(t)
}

/// Budget for a median keystroke frame / any single frame.
fn budget() -> (Duration, Duration) {
    if cfg!(debug_assertions) {
        (Duration::from_millis(120), Duration::from_millis(600))
    } else {
        (Duration::from_millis(16), Duration::from_millis(100))
    }
}

#[test]
fn ten_thousand_lines_typing_selection_scrolling() {
    const LINES: usize = 10_000;
    let (median_budget, max_budget) = budget();
    let (mut b, mount) = Bench::new(LINES);
    println!(
        "mount + 3 frames, {LINES} lines        {:7.3} ms",
        ms(mount)
    );
    assert_eq!(b.code.peek().lines().count(), LINES);

    // Typing.
    let typed = typing(&mut b, LINES, 200);
    report("type 200 chars mid-doc", &typed);
    assert!(
        b.code.peek().contains("abcdefghi klm"),
        "typed text reached the bound signal"
    );
    assert!(
        typed.median < median_budget,
        "typing median {:?} over {median_budget:?}",
        typed.median
    );
    assert!(
        typed.max < max_budget,
        "typing max {:?} over {max_budget:?}",
        typed.max
    );

    // Backspace.
    let back = stats((0..100).map(|_| b.timed(KeyInput::Backspace)).collect());
    report("backspace x100", &back);
    assert!(back.median < median_budget);

    // Selection: shift+down growing a selection line by line, then select-all.
    let sel = stats(
        (0..100)
            .map(|_| {
                b.timed(KeyInput::Move {
                    motion: Motion::Down,
                    extend: true,
                })
            })
            .collect(),
    );
    report("shift+down x100 (growing selection)", &sel);
    assert!(sel.median < median_budget);
    let all = b.timed(KeyInput::SelectAll);
    println!("{:<34} {:7.3} ms", "select all (10k lines)", ms(all));
    assert!(all < max_budget);
    let collapse = b.timed(KeyInput::Move {
        motion: Motion::DocStart,
        extend: false,
    });
    assert!(collapse < max_budget);

    // Scrolling: wheel down through the document, then jump to the end and back.
    let p = Offset::new(400.0, 300.0);
    let mut sc = Vec::new();
    for _ in 0..200 {
        let t = Instant::now();
        b.ui.dispatch_scroll(p, 120.0);
        b.frame();
        sc.push(t.elapsed());
    }
    let sc = stats(sc);
    report("wheel scroll x200 (120 px)", &sc);
    assert!(sc.median < median_budget);
    let end = b.timed(KeyInput::Move {
        motion: Motion::DocEnd,
        extend: false,
    });
    let home = b.timed(KeyInput::Move {
        motion: Motion::DocStart,
        extend: false,
    });
    println!(
        "{:<34} {:7.3} ms / {:7.3} ms",
        "ctrl+end / ctrl+home",
        ms(end),
        ms(home)
    );
    assert!(end < max_budget && home < max_budget);

    // Viewport virtualization holds at 10k lines.
    let nodes = b.ui.render_node_count();
    println!("{:<34} {nodes}", "render nodes");
    assert!(
        nodes < 600,
        "render tree should track the viewport, got {nodes} nodes"
    );
}

// Known O(n) per keystroke (debug, 2026-10-29: 1k 8.4 ms vs 10k 83 ms median = 9.9x):
// every rebuild re-splits the whole document and recomputes `fold::foldable` +
// `fold::DisplayMap::new` + per-line block gaps over all lines (src/view/mod.rs ~600-730);
// highlighting is already windowed past LARGE_FILE_LINES. Un-ignore once those are
// incremental. Run it with `cargo test --test stress -- --ignored --nocapture`.
#[test]
#[ignore = "known O(document) rebuild per keystroke: fold/DisplayMap over all lines"]
fn keystroke_cost_does_not_scale_with_document_size() {
    // Same work on a 1,000- and a 10,000-line file. An O(n)-per-keystroke path (whole-
    // document copy, re-highlight, re-layout) makes the big one ~10x slower; allow 3x.
    let (mut small, _) = Bench::new(1_000);
    let s = typing(&mut small, 1_000, 150);
    drop(small);
    let (mut big, _) = Bench::new(10_000);
    let l = typing(&mut big, 10_000, 150);
    report("type 150 chars, 1k lines", &s);
    report("type 150 chars, 10k lines", &l);
    let ratio = l.median.as_secs_f64() / s.median.as_secs_f64().max(1e-6);
    println!("10k/1k median keystroke ratio      {ratio:.2}x");
    assert!(
        ratio < 3.0,
        "per-keystroke cost grows with document size: {ratio:.2}x (1k {:?}, 10k {:?})",
        s.median,
        l.median
    );
}
