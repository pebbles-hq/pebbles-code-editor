//! A showcase app for `pebbles-code-editor`. A language dropdown + feature toggles on top,
//! one editor below wired with **every** capability so you can try them live:
//!
//! - Editing: type, select, arrows, word/line/doc motions, Copy/Cut/Paste, undo/redo, Tab.
//! - Multi-cursor: Alt-click to add a caret, Ctrl+D to add the next occurrence,
//!   Shift+Alt-drag for a column selection, double/triple-click for word/line, Esc collapses.
//! - Rendering: virtualization, minimap, sticky scroll, indent guides, whitespace, rulers,
//!   horizontal scroll, bracket matching, caret blink (toggle the switches).
//! - Theming: light/dark chrome, a syntax palette with bold keywords + italic comments,
//!   "Relaxed spacing" (line-height + letter-spacing), and theme-as-extension (the Plugins
//!   toggle bolds strings + recolors the caret by composing over the base theme).
//! - Search: Ctrl+F find (incremental, regex/case/whole-word), Ctrl+H replace / replace-all,
//!   match highlighting, and other-occurrence highlighting when you select a word.
//! - Language: highlighting, auto-close, comment toggle (Ctrl+/), smart indent.
//! - IntelliSense (dev-supplied here): completion (Ctrl+Space or type), snippets, hover,
//!   signature help (type `(`), diagnostics (TODO/FIXME/unwrap), inlay hints, go-to-def (F12),
//!   format (Shift+Alt+F).
//! - Extensions/plugins (toggle "Plugins"): decorations (TODO highlight), gutter markers
//!   (TODO bookmark dot), a read-only region (line 1 is locked), a command palette (Ctrl+P →
//!   "Uppercase Selection" / "Wrap Selection in println!"), a configurable keybinding (Ctrl+U
//!   → uppercase), a config facet (2-space indent), and focus/click event hooks (logged to
//!   stdout).
//!
//! Run it: `cargo run -p demo`
//! Headless screenshot: `SHOT=1200:820:/tmp/editor.rgba cargo run -p demo`

use std::rc::Rc;

use pebbles::prelude::*;
use pebbles_code_editor::{
    Command, CompletionContext, CompletionItem, CompletionKind, CompletionProvider, ConfigPatch,
    DefinitionProvider, Decoration, Diagnostic, EditorTheme, Extension, FormatProvider, GutterMark,
    Hover, HoverProvider, InlayHint, Severity, SignatureHelp, SignatureProvider, code_editor,
    extension, lang, lang::Language,
};

mod capture;
mod samples;

use samples::SAMPLES;

fn lang_for(name: &str) -> Box<dyn Language> {
    match name {
        "Rust" | "Rust · long" => Box::new(lang::Rust),
        "TypeScript" => Box::new(lang::typescript()),
        "JavaScript" => Box::new(lang::javascript()),
        "Python" => Box::new(lang::Python),
        "Go" => Box::new(lang::go()),
        "C" => Box::new(lang::c_lang()),
        "Java" => Box::new(lang::java()),
        "JSON" => Box::new(lang::Json),
        _ => Box::new(lang::Plain),
    }
}

// ---------------------------------------------------------------------------
// Sample IntelliSense providers — a real app would back these with a language server.
// ---------------------------------------------------------------------------

const KEYWORDS: &[&str] = &[
    "fn", "let", "mut", "match", "if", "else", "for", "while", "loop", "struct", "enum", "impl",
    "trait", "pub", "mod", "use", "return", "self", "Self", "where", "async", "await", "const",
];

/// Distinct identifiers already in the document (3+ chars) — "words in file" completion.
fn doc_words(src: &str) -> Vec<String> {
    let mut set = std::collections::BTreeSet::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, set: &mut std::collections::BTreeSet<String>| {
        if cur.len() >= 3 && !cur.chars().next().unwrap().is_numeric() {
            set.insert(std::mem::take(cur));
        } else {
            cur.clear();
        }
    };
    for ch in src.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            cur.push(ch);
        } else {
            flush(&mut cur, &mut set);
        }
    }
    flush(&mut cur, &mut set);
    set.into_iter().take(60).collect()
}

/// The word range around `byte` (byte-safe).
fn word_range(src: &str, byte: usize) -> (usize, usize) {
    let is_w = |c: char| c.is_alphanumeric() || c == '_';
    let b = byte.min(src.len());
    let mut s = b;
    for (i, ch) in src[..b].char_indices().rev() {
        if is_w(ch) {
            s = i;
        } else {
            break;
        }
    }
    let mut e = b;
    for ch in src[b..].chars() {
        if is_w(ch) {
            e += ch.len_utf8();
        } else {
            break;
        }
    }
    (s, e)
}

fn completion_provider() -> CompletionProvider {
    Rc::new(|ctx: &CompletionContext| {
        let mut items: Vec<CompletionItem> = KEYWORDS
            .iter()
            .map(|k| CompletionItem::new(*k, CompletionKind::Keyword))
            .collect();
        items.push(
            CompletionItem::new("println", CompletionKind::Function)
                .insert("println!(\"$1\")$0")
                .detail("macro"),
        );
        items.push(
            CompletionItem::new("fn", CompletionKind::Snippet)
                .insert("fn ${1:name}($2) {\n    $0\n}")
                .detail("snippet"),
        );
        items.push(
            CompletionItem::new("for", CompletionKind::Snippet)
                .insert("for ${1:item} in ${2:iter} {\n    $0\n}")
                .detail("snippet"),
        );
        for w in doc_words(ctx.src) {
            items.push(CompletionItem::new(w, CompletionKind::Variable));
        }
        items
    })
}

fn hover_provider() -> HoverProvider {
    Rc::new(|src: &str, byte: usize| {
        let (s, e) = word_range(src, byte);
        if e <= s {
            return None;
        }
        let w = &src[s..e];
        Some(Hover {
            contents: format!(
                "{w}\n\nidentifier · demo hover\n(your provider would return real docs/types)"
            ),
            range: Some((s, e)),
        })
    })
}

fn signature_provider() -> SignatureProvider {
    Rc::new(|_src: &str, _byte: usize| {
        Some(SignatureHelp {
            label: "new(name: &str) -> Self".into(),
            params: vec!["name: &str".into()],
            active: Some(0),
        })
    })
}

fn definition_provider() -> DefinitionProvider {
    Rc::new(|src: &str, byte: usize| {
        let (s, e) = word_range(src, byte);
        if e.saturating_sub(s) < 2 {
            return None;
        }
        let w = &src[s..e];
        // Jump to another occurrence of the word (wraps to the first).
        src.match_indices(w)
            .map(|(i, _)| i)
            .find(|&i| i != s)
            .or_else(|| src.match_indices(w).map(|(i, _)| i).next())
    })
}

fn format_provider() -> FormatProvider {
    Rc::new(|src: &str| {
        let mut out: String = src.lines().map(|l| l.trim_end()).collect::<Vec<_>>().join("\n");
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out
    })
}

/// Diagnostics derived from the text: flag leftover TODO/FIXME and `unwrap`.
fn compute_diagnostics(src: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for pat in ["TODO", "FIXME"] {
        for (i, _) in src.match_indices(pat) {
            out.push(Diagnostic {
                range: (i, i + pat.len()),
                severity: Severity::Warning,
                message: format!("{pat} left in the code"),
            });
        }
    }
    for (i, _) in src.match_indices("unwrap") {
        out.push(Diagnostic {
            range: (i, i + 6),
            severity: Severity::Info,
            message: "consider handling the error".into(),
        });
    }
    out
}

/// Inlay hints: a `: _` type hint after each `let [mut] <name>`.
fn compute_inlays(src: &str) -> Vec<InlayHint> {
    let mut out = Vec::new();
    for (i, _) in src.match_indices("let ") {
        let mut start = i + 4;
        if src[start..].starts_with("mut ") {
            start += 4;
        }
        let mut e = start;
        for ch in src[start..].chars() {
            if ch.is_alphanumeric() || ch == '_' {
                e += ch.len_utf8();
            } else {
                break;
            }
        }
        if e > start {
            out.push(InlayHint {
                at: e,
                label: ": _".into(),
            });
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Extensions / plugins (§7)
// ---------------------------------------------------------------------------

/// A set of sample extensions, each a self-contained plugin:
/// - a decoration plugin (translucent highlight on every `TODO`),
/// - a gutter-marker plugin (a bookmark dot on every line with a `TODO`),
/// - a read-only plugin (the first line is a locked "header" — edits there are vetoed),
/// - a command plugin (two commands runnable from the palette with Ctrl+P).
fn sample_extensions() -> Vec<Extension> {
    // Highlight every `TODO` — composes with the diagnostics underline on the same range.
    let highlight = extension("todo-highlighter").decorations(|snap| {
        snap.text
            .match_indices("TODO")
            .map(|(i, _)| {
                Decoration::background((i, i + 4), Color::from_rgba8(255, 190, 60, 55))
            })
            .collect()
    });
    // A bookmark dot in the gutter on every line containing a `TODO`.
    let bookmarks = extension("todo-bookmarks").gutter_markers(|snap| {
        snap.text
            .match_indices("TODO")
            .map(|(i, _)| GutterMark {
                line: snap.text[..i].bytes().filter(|&b| b == b'\n').count(),
                color: Color::from_rgba8(255, 190, 60, 255),
            })
            .collect()
    });
    // The first line is a locked header — any edit that touches it is vetoed.
    let guard = extension("first-line-guard").read_only_ranges(|snap| {
        let end = snap.text.find('\n').unwrap_or(snap.text.len());
        vec![(0, end)]
    });
    // Two commands (Ctrl+P palette): uppercase the selection, wrap it in println!.
    // "Uppercase" is also bound to a key (Ctrl+U) — the configurable-keymap surface.
    let commands = extension("edit-commands")
        .command(Command::new("edit.upper", "Uppercase Selection", |ctx| {
            let (a, b) = ctx.selection();
            if a != b {
                let up = ctx.text()[a..b].to_uppercase();
                ctx.replace(a, b, up);
            }
        }))
        .command(Command::new(
            "edit.wrap-println",
            "Wrap Selection in println!",
            |ctx| {
                let (a, b) = ctx.selection();
                let inner = ctx.text()[a..b].to_string();
                ctx.replace(a, b, format!("println!(\"{{}}\", {inner});"));
            },
        ))
        .keybinding("Ctrl+U", "edit.upper")
        // A config facet: this plugin prefers a 2-space indent (mergeable settings).
        .config(ConfigPatch {
            tab_size: Some(2),
            insert_spaces: Some(true),
            ..Default::default()
        })
        // Report focus + clicks to stdout so the event hooks are observable when you run it.
        .on_focus(|has| println!("[plugin] editor focus: {has}"))
        .on_click(|_snap, byte| println!("[plugin] clicked at byte {byte}"));
    // "Theme as an extension": compose over the base theme (bolder strings, brighter caret)
    // without replacing it — demonstrates §8 theming-via-extension.
    let theming = extension("accent-tweaks").theme(|mut t| {
        t.syntax.string = t.syntax.string.bold();
        t.caret = Color::from_rgba8(0xFF, 0xC0, 0x66, 0xFF);
        t
    });
    vec![highlight, bookmarks, guard, commands, theming]
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

fn app() -> AnyWidget {
    let c = theme().colors;
    let sel = create_signal(0usize);
    let code = create_signal(String::from(SAMPLES[0].src));

    // Feature toggles (defaults show off the rendering features immediately).
    let minimap = create_signal(true);
    let sticky = create_signal(true);
    let guides = create_signal(true);
    let whitespace = create_signal(false);
    let ruler = create_signal(false);
    let light = create_signal(false);
    let plugins = create_signal(true);
    let relaxed = create_signal(false);

    // Provider data derived reactively from the current text.
    let diagnostics = create_signal(compute_diagnostics(&code.peek()));
    let inlays = create_signal(compute_inlays(&code.peek()));
    create_effect(move || {
        let src = code.get();
        diagnostics.set(compute_diagnostics(&src));
        inlays.set(compute_inlays(&src));
    });

    // Dropdown: the bundled per-language samples + a long generated Rust file.
    let mut names: Vec<String> = SAMPLES.iter().map(|s| s.name.to_string()).collect();
    names.push("Rust · long".to_string());
    let big_idx = SAMPLES.len();
    let dropdown = select(names).value(sel.get()).on_changed(move |idx, _name| {
        sel.set(idx);
        code.set(if idx == big_idx {
            samples::big()
        } else {
            String::from(SAMPLES[idx].src)
        });
    });

    let i = sel.get();
    let (lang_name, title) = if i == big_idx {
        ("Rust · long", "processors.rs")
    } else {
        (SAMPLES[i].name, SAMPLES[i].file)
    };

    // A small labeled switch bound to a bool signal.
    let sw = |flag: Signal<bool>, label: &str| {
        switch(flag.get())
            .label(label)
            .on_changed(move || flag.set(!flag.peek()))
            .into_widget()
    };

    let editor_h = (media_query().size.height - 220.0).max(300.0);

    container()
        .color(c.background)
        .padding(EdgeInsets::all(24.0))
        .child(
            column(children![
                // ---- header ----
                row(children![
                    column(children![
                        text("Pebbles Code Editor").size(22.0).bold().color(c.foreground),
                        gap_h(3.0),
                        text("A custom editing engine — multi-cursor, virtualized, with IntelliSense hooks.")
                            .size(13.0)
                            .color(c.muted_foreground),
                    ])
                    .cross_axis_alignment(CrossAxisAlignment::Start)
                    .main_axis_size(MainAxisSize::Min),
                    spacer(),
                    text("Language").size(12.5).weight(600.0).color(c.muted_foreground),
                    gap_w(10.0),
                    dropdown,
                ])
                .cross_axis_alignment(CrossAxisAlignment::Center),
                gap_h(14.0),
                // ---- feature toggles ----
                row(children![
                    sw(minimap, "Minimap"),
                    gap_w(14.0),
                    sw(sticky, "Sticky scroll"),
                    gap_w(14.0),
                    sw(guides, "Indent guides"),
                    gap_w(14.0),
                    sw(whitespace, "Whitespace"),
                    gap_w(14.0),
                    sw(ruler, "Ruler @ 80"),
                    gap_w(14.0),
                    sw(light, "Light theme"),
                    gap_w(14.0),
                    sw(plugins, "Plugins"),
                    gap_w(14.0),
                    sw(relaxed, "Relaxed spacing"),
                ])
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .main_axis_size(MainAxisSize::Min),
                gap_h(14.0),
                // ---- the editor, wired with everything ----
                code_editor(code)
                    .language(lang_for(lang_name))
                    .theme(if light.get() { EditorTheme::light() } else { EditorTheme::dark() })
                    .title(title)
                    .height(editor_h)
                    .line_height(if relaxed.get() { 2.0 } else { 1.6 })
                    .letter_spacing(if relaxed.get() { 1.2 } else { 0.0 })
                    .autofocus()
                    .minimap(minimap.get())
                    .sticky_scroll(sticky.get())
                    .indent_guides(guides.get())
                    .render_whitespace(whitespace.get())
                    .rulers(if ruler.get() { vec![80usize] } else { Vec::new() })
                    .completion(completion_provider())
                    .hover(hover_provider())
                    .signature_help(signature_provider())
                    .diagnostics(diagnostics)
                    .inlay_hints(inlays)
                    .definition(definition_provider())
                    .format(format_provider())
                    .extensions(if plugins.get() { sample_extensions() } else { Vec::new() }),
                gap_h(12.0),
                // ---- keybindings legend ----
                text(
                    "Ctrl+F find · Ctrl+H replace · Ctrl+Space complete · Tab/Enter accept · \
                     Ctrl+/ comment · Ctrl+D add-next · Alt+click multi-cursor · Shift+Alt+drag column · \
                     dbl/triple-click word/line · F12 go-to-def · Shift+Alt+F format · hover for docs · ( for signature · \
                     Ctrl+P command palette · Ctrl+U uppercase (plugin keybind) · \
                     plugins: TODO highlight + gutter bookmark, line 1 read-only, 2-space indent",
                )
                .size(11.5)
                .color(c.muted_foreground),
            ])
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .main_axis_size(MainAxisSize::Min),
        )
        .into_widget()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    Theme::dark().make_current();
    if let Ok(spec) = std::env::var("SHOT") {
        return capture::shot(&spec, app, theme().colors.background);
    }
    App::new(component(app))
        .title("Pebbles Code Editor")
        .size(1200, 820)
        .background(theme().colors.background)
        .run()
}
