//! Language-service UI: the parts of the editor driven by a [`LanguageService`] — hover,
//! signature help, navigation and peek, references, document highlights, rename,
//! diagnostics, code actions and formatting.
//!
//! [`Lsp`] bundles the service, the document URI and the view signals; every request goes
//! through a per-feature [`RequestGate`] **and** a document-version check, so a reply that
//! arrives after a newer request or an edit is dropped. Replies may arrive synchronously
//! (the [`ProviderService`](crate::language::ProviderService) adapter does) or later on the
//! UI thread.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use pebbles::prelude::*;
use ropey::Rope;

use crate::edit::{EditorState, History};
use crate::language::{self as lsp, LanguageService, RequestGate};
use crate::view::Frame;
use crate::view::chrome::with_alpha;

/// A diagnostic resolved to byte offsets in the current text.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Diag {
    pub(crate) from: usize,
    pub(crate) to: usize,
    pub(crate) d: lsp::LspDiagnostic,
}

impl Diag {
    pub(crate) fn has(&self, tag: lsp::DiagnosticTag) -> bool {
        self.d.tags.contains(&tag)
    }
}

/// The shown hover: where it is anchored and its content sections.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HoverView {
    /// The byte the hover was requested at.
    pub(crate) byte: usize,
    /// The byte range it applies to (moving within it keeps the hover).
    pub(crate) range: (usize, usize),
    /// The pointer position for a mouse hover; `None` when shown from the keyboard.
    pub(crate) pointer: Option<(f64, f64)>,
    /// Diagnostics under the position (shown first).
    pub(crate) diags: Vec<lsp::LspDiagnostic>,
    /// The hover content, split into sections (Markdown `---` rules separate them).
    pub(crate) sections: Vec<lsp::MarkupContent>,
}

/// The shown signature help and the overload being viewed.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SigView {
    pub(crate) help: lsp::SignatureHelp,
    pub(crate) active: usize,
}

/// Which navigation request to make.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Nav {
    Definition,
    Declaration,
    TypeDefinition,
    Implementation,
    References,
}

impl Nav {
    fn title(self) -> &'static str {
        match self {
            Nav::Definition => "Definition",
            Nav::Declaration => "Declaration",
            Nav::TypeDefinition => "Type Definition",
            Nav::Implementation => "Implementations",
            Nav::References => "References",
        }
    }
}

/// An open peek view: the locations shown inline below `line`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PeekView {
    pub(crate) line: usize,
    pub(crate) title: String,
    pub(crate) locs: Vec<lsp::Location>,
    pub(crate) sel: usize,
}

/// The inline rename box over `from..to`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RenameView {
    pub(crate) from: usize,
    pub(crate) to: usize,
}

/// The rename preview: the workspace edit and which files are accepted.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenamePreview {
    pub(crate) edit: lsp::WorkspaceEdit,
    pub(crate) accepted: Vec<bool>,
}

impl RenamePreview {
    /// The edit restricted to the accepted files.
    pub(crate) fn accepted_edit(&self) -> lsp::WorkspaceEdit {
        lsp::WorkspaceEdit {
            changes: self
                .edit
                .changes
                .iter()
                .zip(&self.accepted)
                .filter(|(_, a)| **a)
                .map(|(c, _)| c.clone())
                .collect(),
        }
    }
}

/// Non-reactive per-editor state (gates, caches, the document version counter).
#[derive(Default)]
pub(crate) struct Store {
    pub(crate) hover_gate: RequestGate,
    pub(crate) sig_gate: RequestGate,
    pub(crate) nav_gate: RequestGate,
    pub(crate) link_gate: RequestGate,
    pub(crate) hl_gate: RequestGate,
    pub(crate) rename_gate: RequestGate,
    /// The (caret, version) of the last document-highlight request.
    hl_at: Cell<Option<(usize, u64)>>,
    /// Hover replies per (byte, document version).
    pub(crate) hover_cache: RefCell<HashMap<(usize, u64), Option<lsp::Hover>>>,
    ver: Cell<u64>,
    last_doc: RefCell<Option<Rope>>,
}

/// The view signals, created once per render in a fixed order (hook positions).
#[derive(Clone, Copy)]
pub(crate) struct Hooks {
    pub(crate) hover: Signal<Option<HoverView>>,
    /// The pointer is over the hover widget (it stays while it is).
    pub(crate) hover_sticky: Signal<bool>,
    pub(crate) sig: Signal<Option<SigView>>,
    pub(crate) peek: Signal<Option<PeekView>>,
    /// The Ctrl+hover link: the word range with a definition under the pointer.
    pub(crate) link: Signal<Option<(usize, usize)>>,
    /// The service's document highlights for the caret (`None` = use the word fallback).
    pub(crate) highlights: Signal<Option<Vec<(usize, usize, lsp::DocumentHighlightKind)>>>,
    pub(crate) rename: Signal<Option<RenameView>>,
    pub(crate) rename_draft: Signal<String>,
    pub(crate) rename_preview: Signal<Option<RenamePreview>>,
    /// The open problem peek (F8): the diagnostic's start byte and message.
    pub(crate) problem: Signal<Option<(usize, String)>>,
    pub(crate) store: Signal<Rc<Store>>,
}

/// Create the hook signals (call unconditionally, once per render).
pub(crate) fn hooks() -> Hooks {
    Hooks {
        hover: create_signal(None),
        hover_sticky: create_signal(false),
        sig: create_signal(None),
        peek: create_signal(None),
        link: create_signal(None),
        highlights: create_signal(None),
        rename: create_signal(None),
        rename_draft: create_signal(String::new()),
        rename_preview: create_signal(None),
        problem: create_signal(None),
        store: create_signal(Rc::new(Store::default())),
    }
}

/// The language-service controller for one editor (cheap to clone).
#[derive(Clone)]
pub(crate) struct Lsp {
    pub(crate) svc: Option<Rc<dyn LanguageService>>,
    pub(crate) uri: Rc<str>,
    pub(crate) h: Hooks,
    pub(crate) store: Rc<Store>,
    pub(crate) state: Signal<EditorState>,
    pub(crate) history: Signal<Rc<RefCell<History>>>,
    pub(crate) code: Signal<String>,
    pub(crate) goal: Signal<usize>,
    pub(crate) hover_delay: f64,
    pub(crate) diag_old: Option<crate::providers::Diagnostics>,
    pub(crate) diag_lsp: Option<Signal<Vec<lsp::LspDiagnostic>>>,
    pub(crate) on_open_link: Option<Rc<dyn Fn(&str)>>,
    /// The editor's focus (returned to after the rename box / preview closes).
    pub(crate) focus: pebbles::core::focus::FocusNode,
    pub(crate) read_only: bool,
}

/// Map the old byte-range diagnostics onto the LSP form.
fn old_severity(s: crate::providers::Severity) -> lsp::DiagnosticSeverity {
    use crate::providers::Severity as S;
    use lsp::DiagnosticSeverity as L;
    match s {
        S::Error => L::Error,
        S::Warning => L::Warning,
        S::Info => L::Information,
        S::Hint => L::Hint,
    }
}

/// Split Markdown hover content into sections at horizontal rules (`---`, `***`, `___`).
pub(crate) fn sections(c: &lsp::MarkupContent) -> Vec<lsp::MarkupContent> {
    if c.kind != lsp::MarkupKind::Markdown {
        return vec![c.clone()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut fence = false;
    for line in c.value.split('\n') {
        let t = line.trim();
        if t.starts_with("```") {
            fence = !fence;
        }
        if !fence && (t == "---" || t == "***" || t == "___") {
            if !cur.trim().is_empty() {
                out.push(lsp::MarkupContent::markdown(cur.trim_end()));
            }
            cur.clear();
            continue;
        }
        cur.push_str(line);
        cur.push('\n');
    }
    if !cur.trim().is_empty() {
        out.push(lsp::MarkupContent::markdown(cur.trim_end()));
    }
    out
}

impl Lsp {
    /// The current document version (bumped whenever the text differs from the last call).
    pub(crate) fn version(&self) -> u64 {
        let doc = self.state.peek().doc;
        let mut last = self.store.last_doc.borrow_mut();
        if last.as_ref() != Some(&doc) {
            self.store.ver.set(self.store.ver.get() + 1);
            *last = Some(doc);
        }
        self.store.ver.get()
    }

    /// A reply that runs `f` only while it is the latest request through `gate` and the
    /// document is unchanged since the request.
    pub(crate) fn guard<T: 'static>(
        &self,
        gate: &RequestGate,
        f: impl FnOnce(T) + 'static,
    ) -> lsp::Reply<T> {
        let at = self.version();
        let me = self.clone();
        gate.wrap(move |v: T| {
            if me.version() == at {
                f(v);
            }
        })
    }

    /// Run `f` with a [`lsp::DocRef`] for the current text and the text itself.
    pub(crate) fn with_doc<R>(&self, f: impl FnOnce(&lsp::DocRef, &str) -> R) -> R {
        let version = self.version();
        let text = self.state.peek().text();
        let doc = lsp::DocRef {
            uri: &self.uri,
            text: &text,
            version,
        };
        f(&doc, &text)
    }

    /// Every diagnostic (old byte-range ones and LSP ones), resolved to bytes in `src` and
    /// sorted by position. `track` reads the signals reactively (render) vs peeks.
    pub(crate) fn diagnostics(&self, src: &str, track: bool) -> Vec<Diag> {
        let mut out = Vec::new();
        if let Some(s) = self.diag_old {
            for d in if track { s.get() } else { s.peek() } {
                let (from, to) = (d.range.0.min(src.len()), d.range.1.min(src.len()));
                out.push(Diag {
                    from,
                    to: to.max(from),
                    d: lsp::LspDiagnostic::new(
                        lsp::bytes_to_range(src, from, to.max(from)),
                        old_severity(d.severity),
                        d.message.clone(),
                    ),
                });
            }
        }
        if let Some(s) = self.diag_lsp {
            for d in if track { s.get() } else { s.peek() } {
                let (from, to) = lsp::range_to_bytes(src, d.range);
                out.push(Diag {
                    from,
                    to: to.max(from),
                    d,
                });
            }
        }
        out.sort_by_key(|d| (d.from, d.to));
        out
    }

    // ---- hover ------------------------------------------------------------------------

    fn hover_timer(&self) -> u64 {
        self.h.hover.raw_id() ^ 0x484f_5645_5200_0000
    }

    /// The pointer moved over byte `byte` (at `pos`): keep the hover while inside its range,
    /// otherwise request one after the dwell delay.
    pub(crate) fn pointer_moved(&self, byte: usize, pos: Offset) {
        if self.h.hover_sticky.peek() {
            return;
        }
        if let Some(hv) = self.h.hover.peek()
            && hv.pointer.is_some()
            && hv.range.0 <= byte
            && byte <= hv.range.1
        {
            pebbles::core::animation::clear_timeout(self.hover_timer());
            return;
        }
        let me = self.clone();
        pebbles::core::animation::set_timeout(self.hover_timer(), self.hover_delay, move || {
            if !me.h.hover_sticky.peek() {
                me.request_hover(byte, Some((pos.x, pos.y)));
            }
        });
    }

    /// The pointer left the text: hide a mouse hover shortly unless the pointer is on it.
    pub(crate) fn pointer_left(&self) {
        let me = self.clone();
        pebbles::core::animation::set_timeout(self.hover_timer(), 0.25, move || {
            if !me.h.hover_sticky.peek() && me.h.hover.peek().is_some_and(|h| h.pointer.is_some()) {
                me.h.hover.set(None);
            }
        });
    }

    /// Hide the hover (and cancel a pending dwell / reply).
    pub(crate) fn hide_hover(&self) {
        pebbles::core::animation::clear_timeout(self.hover_timer());
        self.store.hover_gate.cancel();
        if self.h.hover.peek().is_some() {
            self.h.hover.set(None);
        }
        self.h.hover_sticky.set(false);
    }

    /// Show the hover for the primary caret (Ctrl+K Ctrl+I).
    pub(crate) fn show_hover_at_caret(&self) {
        let head = self.state.peek().primary().head;
        self.request_hover(head, None);
    }

    /// Request the hover at `byte`: cached per (byte, version); diagnostics under the
    /// position are always included.
    pub(crate) fn request_hover(&self, byte: usize, pointer: Option<(f64, f64)>) {
        let src = self.state.peek().text();
        let byte = byte.min(src.len());
        let diags: Vec<lsp::LspDiagnostic> = self
            .diagnostics(&src, false)
            .into_iter()
            .filter(|d| d.from <= byte && byte <= d.to.max(d.from + 1))
            .map(|d| d.d)
            .collect();
        let word = crate::view::word_range_at(&src, byte);
        let has_diags = !diags.is_empty();
        let me = self.clone();
        let show = move |h: Option<lsp::Hover>| {
            let src = me.state.peek().text();
            let mut range = word.unwrap_or((byte, byte));
            let mut secs = Vec::new();
            if let Some(h) = &h {
                if let Some(r) = h.range {
                    range = lsp::range_to_bytes(&src, r);
                }
                secs = sections(&h.contents);
                secs.retain(|s| !s.value.trim().is_empty());
            }
            if secs.is_empty() && diags.is_empty() {
                me.h.hover.set(None);
                return;
            }
            me.h.hover.set(Some(HoverView {
                byte,
                range: (range.0.min(byte), range.1.max(byte)),
                pointer,
                diags: diags.clone(),
                sections: secs,
            }));
        };
        let Some(svc) = self.svc.clone() else {
            show(None);
            return;
        };
        let key = (byte, self.version());
        let cached = self.store.hover_cache.borrow().get(&key).cloned();
        if let Some(h) = cached {
            show(h);
            return;
        }
        let store = self.store.clone();
        let show = Rc::new(show);
        let answered = Rc::new(Cell::new(false));
        let (s2, a2) = (show.clone(), answered.clone());
        let reply = self.guard(&self.store.hover_gate, move |h: Option<lsp::Hover>| {
            let mut c = store.hover_cache.borrow_mut();
            if c.len() > 256 {
                c.clear();
            }
            c.insert(key, h.clone());
            drop(c);
            a2.set(true);
            s2(h);
        });
        let asked =
            self.with_doc(|doc, text| svc.hover(doc, lsp::byte_to_position(text, byte), reply));
        // Declined, or still pending: show the diagnostics part now (the reply completes it).
        if !answered.get() && (!asked || has_diags) {
            show(None);
        }
    }

    /// The hover widget layer, if a hover is showing.
    pub(crate) fn hover_layer(&self, f: &Frame) -> Option<AnyWidget> {
        let hv = self.h.hover.get()?;
        let t = f.theme;
        let fs = f.fs * 0.92;
        let mut kids: Vec<AnyWidget> = Vec::new();
        let divider = || {
            container()
                .height(1.0)
                .margin(EdgeInsets::symmetric(0.0, 6.0))
                .decoration(BoxDecoration::new().color(t.border))
                .into_widget()
        };
        for d in &hv.diags {
            if !kids.is_empty() {
                kids.push(divider());
            }
            kids.push(self.diag_body(d, t, fs));
        }
        for s in &hv.sections {
            if !kids.is_empty() {
                kids.push(divider());
            }
            kids.push(match s.kind {
                lsp::MarkupKind::Markdown => {
                    crate::view::markdown::render(&s.value, t, fs, f.font_family)
                }
                lsp::MarkupKind::PlainText => text(s.value.clone())
                    .size(fs as f32)
                    .font_family(f.font_family)
                    .color(t.foreground)
                    .into_widget(),
            });
        }
        let line = f.li.line_of(f.src, hv.range.0.min(f.src.len()));
        let (x, _) = f.xy(line, f.li.col_of(f.src, hv.range.0.min(f.src.len())));
        let y = f.y_of(line) + f.line_px + 2.0;
        let x = match hv.pointer {
            Some((px, _)) => px.min(x.max(px - 40.0)).max(0.0),
            None => x,
        };
        let sticky = self.h.hover_sticky;
        let me = self.clone();
        let panel = container()
            .width(460.0)
            .decoration(
                BoxDecoration::new()
                    .color(t.overlay_bg)
                    .radius(BorderRadius::all(t.overlay_radius))
                    .border(Border::new(t.border, 1.0)),
            )
            .padding(EdgeInsets::symmetric(10.0, 7.0))
            .clip()
            .child(
                column(kids)
                    .cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .main_axis_size(MainAxisSize::Min),
            );
        let panel = GestureDetector::new(panel)
            .on_hover_move(action_event(move |_| {
                if !sticky.peek() {
                    sticky.set(true);
                }
            }))
            .on_hover_exit(action(move || {
                sticky.set(false);
                me.pointer_left();
            }));
        Some(Positioned::new(panel).left(x).top(y).into_widget())
    }

    /// A diagnostic as shown in the hover / problem peek: severity, message, code / source
    /// (the code is a link when the diagnostic has one) and related information.
    pub(crate) fn diag_body(
        &self,
        d: &lsp::LspDiagnostic,
        t: &crate::theme::EditorTheme,
        fs: f64,
    ) -> AnyWidget {
        let color = severity_color(t, d.severity());
        let mut rows: Vec<AnyWidget> = Vec::new();
        let mut msg = vec![
            span(format!("{} ", severity_label(d.severity())))
                .size(fs as f32)
                .bold()
                .color(color),
            span(d.message.clone()).size(fs as f32).color(t.foreground),
        ];
        if let Some(src) = &d.source {
            msg.push(span(format!("  {src}")).size(fs as f32).color(t.muted));
        }
        rows.push(text_rich(msg).into_widget());
        if let Some(code) = &d.code {
            let label = text(format!("({code})")).size((fs * 0.95) as f32).color(
                if d.code_href.is_some() {
                    t.accent
                } else {
                    t.muted
                },
            );
            let w: AnyWidget = match (&d.code_href, &self.on_open_link) {
                (Some(href), Some(open)) => {
                    let (href, open) = (href.clone(), open.clone());
                    GestureDetector::new(label.underline())
                        .cursor(Cursor::Pointer)
                        .on_tap(action(move || open(&href)))
                        .into_widget()
                }
                (Some(_), None) => label.underline().into_widget(),
                _ => label.into_widget(),
            };
            rows.push(w);
        }
        for r in &d.related_information {
            let file = r.location.uri.rsplit('/').next().unwrap_or(&r.location.uri);
            let label = text(format!(
                "{file}({}, {}): {}",
                r.location.range.start.line + 1,
                r.location.range.start.character + 1,
                r.message
            ))
            .size((fs * 0.95) as f32)
            .color(t.muted);
            let me = self.clone();
            let loc = r.location.clone();
            rows.push(
                GestureDetector::new(label)
                    .cursor(Cursor::Pointer)
                    .on_tap(action(move || me.go_to(&loc)))
                    .into_widget(),
            );
        }
        column(rows)
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .main_axis_size(MainAxisSize::Min)
            .into_widget()
    }

    // ---- signature help ---------------------------------------------------------------

    /// Whether typing `s` opens signature help (the service's trigger characters).
    pub(crate) fn is_sig_trigger(&self, s: &str) -> bool {
        self.svc
            .as_ref()
            .is_some_and(|v| v.signature_trigger_characters().iter().any(|t| t == s))
    }

    /// Request signature help at the caret. While help is showing, the request is a
    /// retrigger carrying the active help; an empty reply closes it.
    pub(crate) fn request_signature(
        &self,
        kind: lsp::SignatureHelpTriggerKind,
        ch: Option<String>,
    ) {
        let Some(svc) = self.svc.clone() else { return };
        let open = self.h.sig.peek();
        let cx = lsp::SignatureHelpContext {
            trigger_kind: kind,
            trigger_character: ch,
            is_retrigger: open.is_some(),
            active: open.as_ref().map(|o| {
                let mut h = o.help.clone();
                h.active_signature = Some(o.active as u32);
                h
            }),
        };
        let sig = self.h.sig;
        let reply = self.guard(
            &self.store.sig_gate,
            move |h: Option<lsp::SignatureHelp>| {
                match h.filter(|h| !h.signatures.is_empty()) {
                    Some(help) => {
                        let n = help.signatures.len();
                        // Keep the overload the user picked across retriggers (same overload set).
                        let active = match sig.peek() {
                            Some(o)
                                if o.help.signatures.len() == n
                                    && help.active_signature.is_none() =>
                            {
                                o.active
                            }
                            _ => help.active_signature.unwrap_or(0) as usize,
                        };
                        sig.set(Some(SigView {
                            active: active.min(n - 1),
                            help,
                        }));
                    }
                    None => sig.set(None),
                }
            },
        );
        let head = self.state.peek().primary().head;
        let asked = self.with_doc(|doc, text| {
            svc.signature_help(doc, lsp::byte_to_position(text, head), cx, reply)
        });
        if !asked {
            self.h.sig.set(None);
        }
    }

    /// After a key: a trigger character opens help; while open, any edit or caret move
    /// retriggers it (the server closes it by replying with nothing).
    pub(crate) fn signature_after_key(&self, typed: Option<&str>) {
        use lsp::SignatureHelpTriggerKind as K;
        match typed {
            Some(t) if self.is_sig_trigger(t) => {
                self.request_signature(K::TriggerCharacter, Some(t.to_string()));
            }
            _ if self.h.sig.peek().is_some() => self.request_signature(K::ContentChange, None),
            _ => {}
        }
    }

    /// Close signature help.
    pub(crate) fn close_signature(&self) {
        self.store.sig_gate.cancel();
        if self.h.sig.peek().is_some() {
            self.h.sig.set(None);
        }
    }

    /// Cycle the shown overload by `delta` (Alt+Up / Alt+Down). `false` when not applicable.
    pub(crate) fn cycle_signature(&self, delta: isize) -> bool {
        let Some(mut v) = self.h.sig.peek() else {
            return false;
        };
        let n = v.help.signatures.len();
        if n < 2 {
            return false;
        }
        v.active = (v.active as isize + delta).rem_euclid(n as isize) as usize;
        self.h.sig.set(Some(v));
        true
    }

    /// The signature-help panel, just above the caret line.
    pub(crate) fn sig_layer(&self, f: &Frame) -> Option<AnyWidget> {
        let v = self.h.sig.get()?;
        let sig = v.help.signatures.get(v.active)?;
        let t = f.theme;
        let fs = (f.fs * 0.92) as f32;
        let active_param = sig.active_parameter.or(v.help.active_parameter);
        let hl = active_param
            .and_then(|i| sig.parameters.get(i as usize))
            .and_then(|p| param_range(&sig.label, &p.label));
        let mut spans: Vec<TextSpan> = Vec::new();
        if v.help.signatures.len() > 1 {
            spans.push(
                span(format!(
                    "\u{2039} {}/{} \u{203a}  ",
                    v.active + 1,
                    v.help.signatures.len()
                ))
                .size(fs)
                .color(t.muted),
            );
        }
        let base = |s: &str| {
            span(s.to_string())
                .size(fs)
                .font_family(f.font_family)
                .color(t.foreground)
        };
        match hl {
            Some((a, b)) => {
                spans.push(base(&sig.label[..a]));
                spans.push(base(&sig.label[a..b]).bold().underline().color(t.accent));
                spans.push(base(&sig.label[b..]));
            }
            None => spans.push(base(&sig.label)),
        }
        let mut kids: Vec<AnyWidget> = vec![text_rich(spans).into_widget()];
        let docs = |m: &lsp::MarkupContent| match m.kind {
            lsp::MarkupKind::Markdown => {
                crate::view::markdown::render(&m.value, t, f.fs * 0.88, f.font_family)
            }
            lsp::MarkupKind::PlainText => text(m.value.clone())
                .size((f.fs * 0.88) as f32)
                .color(t.foreground)
                .into_widget(),
        };
        if let Some(d) = active_param
            .and_then(|i| sig.parameters.get(i as usize))
            .and_then(|p| p.documentation.as_ref())
        {
            kids.push(gap_h(4.0).into_widget());
            kids.push(docs(d));
        }
        if let Some(d) = &sig.documentation {
            kids.push(gap_h(4.0).into_widget());
            kids.push(docs(d));
        }
        let panel = container()
            .width(460.0)
            .decoration(
                BoxDecoration::new()
                    .color(t.overlay_bg)
                    .radius(BorderRadius::all(t.overlay_radius))
                    .border(Border::new(t.border, 1.0)),
            )
            .padding(EdgeInsets::symmetric(10.0, 6.0))
            .clip()
            .child(
                column(kids)
                    .cross_axis_alignment(CrossAxisAlignment::Start)
                    .main_axis_size(MainAxisSize::Min),
            );
        let (x, y) = f.xy(f.cl, f.li.col_of(f.src, f.pcc));
        let extra = if sig.documentation.is_some() {
            3.0
        } else {
            1.0
        };
        let top = (y - f.line_px * (1.0 + extra) - 6.0).max(0.0);
        Some(Positioned::new(panel).left(x).top(top).into_widget())
    }

    // ---- navigation + peek ---------------------------------------------------------------

    /// Navigate from the caret (F12, Ctrl+F12, …). One result is revealed (another document
    /// through the host's `open_location`); several open the peek view, as does `peek`.
    /// `false` when there is no service or it declines the request.
    pub(crate) fn navigate(&self, kind: Nav, peek: bool) -> bool {
        let head = self.state.peek().primary().head;
        self.navigate_at(head, kind, peek)
    }

    /// Navigate from byte `at` (Ctrl+click uses the pointer's byte).
    pub(crate) fn navigate_at(&self, at: usize, kind: Nav, peek: bool) -> bool {
        let Some(svc) = self.svc.clone() else {
            return false;
        };
        let line = crate::geometry::line_of(&self.state.peek().text(), at);
        let me = self.clone();
        let reply = self.guard(&self.store.nav_gate, move |locs: Vec<lsp::Location>| {
            me.show_locations(kind.title(), locs, peek, line);
        });
        self.with_doc(|doc, text| {
            let pos = lsp::byte_to_position(text, at);
            match kind {
                Nav::Definition => svc.definition(doc, pos, reply),
                Nav::Declaration => svc.declaration(doc, pos, reply),
                Nav::TypeDefinition => svc.type_definition(doc, pos, reply),
                Nav::Implementation => svc.implementation(doc, pos, reply),
                Nav::References => svc.references(doc, pos, true, reply),
            }
        })
    }

    /// Reveal one location, or peek several (or always, with `peek`).
    pub(crate) fn show_locations(
        &self,
        title: &str,
        locs: Vec<lsp::Location>,
        peek: bool,
        line: usize,
    ) {
        match locs.len() {
            0 => {}
            1 if !peek => self.go_to(&locs[0]),
            _ => self.h.peek.set(Some(PeekView {
                line,
                title: title.to_string(),
                locs,
                sel: 0,
            })),
        }
    }

    /// Close the peek view.
    pub(crate) fn close_peek(&self) {
        if self.h.peek.peek().is_some() {
            self.h.peek.set(None);
        }
    }

    /// Keys while the peek is open: Up/Down pick a result, Enter opens it, Escape closes.
    /// Returns whether the key was consumed.
    pub(crate) fn peek_key(&self, k: &KeyInput) -> bool {
        let Some(mut v) = self.h.peek.peek() else {
            return false;
        };
        match k {
            KeyInput::Escape => self.close_peek(),
            KeyInput::Enter => {
                self.close_peek();
                if let Some(l) = v.locs.get(v.sel) {
                    self.go_to(l);
                }
            }
            KeyInput::Move {
                motion: Motion::Down | Motion::Up,
                ..
            } => {
                let n = v.locs.len();
                let d = if matches!(
                    k,
                    KeyInput::Move {
                        motion: Motion::Down,
                        ..
                    }
                ) {
                    1
                } else {
                    n - 1
                };
                v.sel = (v.sel + d) % n;
                self.h.peek.set(Some(v));
            }
            _ => return false,
        }
        true
    }

    /// The text of `uri`: this document's, or the host's `read_text`.
    fn text_of(&self, uri: &str) -> Option<String> {
        if uri == &*self.uri {
            return Some(self.state.peek().text());
        }
        self.svc.as_ref()?.read_text(uri)
    }

    /// The peek view as a block widget below its line.
    pub(crate) fn peek_block(
        &self,
        theme: &crate::theme::EditorTheme,
        fs: f64,
        line_px: f64,
        mono: &str,
    ) -> Option<crate::extensions::BlockWidget> {
        const ROWS: usize = 10;
        let v = self.h.peek.get()?;
        let t = theme;
        let sel = v.sel.min(v.locs.len().saturating_sub(1));
        let loc = v.locs.get(sel)?.clone();
        let file = |u: &str| u.rsplit('/').next().unwrap_or(u).to_string();
        let peek = self.h.peek;
        let mut header: Vec<AnyWidget> = vec![
            t.ui_text(v.title.clone(), 0.0)
                .color(t.foreground)
                .into_widget(),
            gap_w(8.0).into_widget(),
            t.ui_text(
                format!("{} \u{2014} {}", file(&loc.uri), loc.range.start.line + 1),
                0.0,
            )
            .color(t.muted)
            .into_widget(),
            expanded(gap_w(1.0)).into_widget(),
        ];
        if v.locs.len() > 1 {
            header.push(
                t.ui_text(format!("{} results", v.locs.len()), 0.0)
                    .color(t.muted)
                    .into_widget(),
            );
            header.push(gap_w(6.0).into_widget());
        }
        header.push(
            icon_button(IconKind::Close)
                .size(16.0)
                .on_pressed(move || peek.set(None))
                .into_widget(),
        );
        // The embedded read-only preview around the target.
        let mut preview: Vec<AnyWidget> = Vec::new();
        match self.text_of(&loc.uri) {
            Some(src) => {
                let lines: Vec<&str> = src.split('\n').collect();
                let first = (loc.range.start.line as usize).saturating_sub(2);
                for n in first..(first + ROWS).min(lines.len()) {
                    let target =
                        n >= loc.range.start.line as usize && n <= loc.range.end.line as usize;
                    let bg = if target {
                        with_alpha(t.accent, 0.16)
                    } else {
                        Color::from_rgba8(0, 0, 0, 0)
                    };
                    preview.push(
                        container()
                            .height(line_px)
                            .decoration(BoxDecoration::new().color(bg))
                            .child(
                                row(vec![
                                    container()
                                        .width(fs * 3.2)
                                        .child(
                                            text(format!("{}", n + 1))
                                                .size((fs * 0.85) as f32)
                                                .font_family(mono)
                                                .color(t.muted),
                                        )
                                        .into_widget(),
                                    text(lines[n].to_string())
                                        .size(fs as f32)
                                        .font_family(mono)
                                        .color(t.foreground)
                                        .soft_wrap(false)
                                        .into_widget(),
                                ])
                                .cross_axis_alignment(CrossAxisAlignment::Center),
                            )
                            .into_widget(),
                    );
                }
            }
            None => preview.push(
                t.ui_text("(preview unavailable)", 0.0)
                    .color(t.muted)
                    .into_widget(),
            ),
        }
        let me = self.clone();
        let open = loc.clone();
        let preview = GestureDetector::new(
            container()
                .clip()
                .child(column(preview).main_axis_size(MainAxisSize::Min)),
        )
        .on_double_tap(action(move || {
            me.close_peek();
            me.go_to(&open);
        }));
        let mut body: Vec<AnyWidget> = vec![expanded(preview).into_widget()];
        if v.locs.len() > 1 {
            let mut list: Vec<AnyWidget> = Vec::new();
            for (i, l) in v.locs.iter().enumerate().take(ROWS) {
                let bg = if i == sel {
                    t.selection
                } else {
                    Color::from_rgba8(0, 0, 0, 0)
                };
                let (me, l2) = (self.clone(), l.clone());
                list.push(
                    GestureDetector::new(
                        container()
                            .height(line_px)
                            .padding(EdgeInsets::symmetric(6.0, 0.0))
                            .decoration(BoxDecoration::new().color(bg))
                            .alignment(Alignment::CENTER_LEFT)
                            .child(
                                t.ui_text(
                                    format!("{}:{}", file(&l.uri), l.range.start.line + 1),
                                    0.0,
                                )
                                .color(t.foreground),
                            ),
                    )
                    .cursor(Cursor::Pointer)
                    .on_tap(action(move || {
                        if let Some(mut v) = peek.peek() {
                            v.sel = i;
                            peek.set(Some(v));
                        }
                    }))
                    .on_double_tap(action(move || {
                        me.close_peek();
                        me.go_to(&l2);
                    }))
                    .into_widget(),
                );
            }
            body.push(
                container()
                    .width(220.0)
                    .decoration(BoxDecoration::new().border(Border::new(t.border, 1.0)))
                    .child(column(list).main_axis_size(MainAxisSize::Min))
                    .into_widget(),
            );
        }
        let height = line_px * (ROWS as f64 + 1.6) + 12.0;
        let panel = container()
            .height(height)
            .padding(EdgeInsets::all(6.0))
            .decoration(
                BoxDecoration::new()
                    .color(t.overlay_bg)
                    .border(Border::new(t.accent, 1.0)),
            )
            .child(
                column(vec![
                    row(header)
                        .cross_axis_alignment(CrossAxisAlignment::Center)
                        .into_widget(),
                    expanded(row(body).cross_axis_alignment(CrossAxisAlignment::Start))
                        .into_widget(),
                ])
                .cross_axis_alignment(CrossAxisAlignment::Stretch),
            );
        Some(crate::extensions::BlockWidget {
            line: v.line,
            height,
            widget: panel.into_widget(),
        })
    }

    /// Shift+Alt+F12: find references and hand them to the host's references view; the
    /// inline peek is the fallback when the host does not show them.
    pub(crate) fn show_references(&self) -> bool {
        let Some(svc) = self.svc.clone() else {
            return false;
        };
        let head = self.state.peek().primary().head;
        let (line, origin) = {
            let text = self.state.peek().text();
            let p = lsp::byte_to_position(&text, head);
            (
                crate::geometry::line_of(&text, head),
                lsp::Location {
                    uri: self.uri.to_string(),
                    range: lsp::Range::new(p, p),
                },
            )
        };
        let me = self.clone();
        let reply = self.guard(&self.store.nav_gate, move |locs: Vec<lsp::Location>| {
            if locs.is_empty() {
                return;
            }
            let shown = me
                .svc
                .as_ref()
                .is_some_and(|s| s.show_references(&origin, &locs));
            if !shown {
                me.show_locations(Nav::References.title(), locs, true, line);
            }
        });
        self.with_doc(|doc, text| {
            svc.references(doc, lsp::byte_to_position(text, head), true, reply)
        })
    }

    /// Ask for the document highlights at the caret (when it moved or the text changed).
    /// A declined request falls back to the word-occurrence highlight.
    pub(crate) fn request_highlights(&self) {
        let st = self.state.peek();
        let head = st.primary().head;
        let key = (head, self.version());
        if self.store.hl_at.get() == Some(key) {
            return;
        }
        self.store.hl_at.set(Some(key));
        let hl = self.h.highlights;
        let Some(svc) = self.svc.clone() else {
            return;
        };
        if st.selection.is_multi() || !st.primary().is_empty() {
            if hl.peek().is_some() {
                hl.set(None);
            }
            return;
        }
        let me = self.clone();
        let reply = self.guard(
            &self.store.hl_gate,
            move |v: Vec<lsp::DocumentHighlight>| {
                let text = me.state.peek().text();
                let list: Vec<_> = v
                    .iter()
                    .map(|h| {
                        let (a, b) = lsp::range_to_bytes(&text, h.range);
                        (a, b, h.kind)
                    })
                    .collect();
                hl.set(Some(list));
            },
        );
        let asked = self.with_doc(|doc, text| {
            svc.document_highlight(doc, lsp::byte_to_position(text, head), reply)
        });
        if !asked && hl.peek().is_some() {
            hl.set(None);
        }
    }

    // ---- diagnostics -------------------------------------------------------------------

    /// F8 / Shift+F8: move to the next / previous problem (wrapping) and show it inline.
    pub(crate) fn next_problem(&self, forward: bool) -> bool {
        let src = self.state.peek().text();
        let diags = self.diagnostics(&src, false);
        if diags.is_empty() {
            return false;
        }
        let head = self.state.peek().primary().head;
        // From the open problem when there is one, else from the caret.
        let open = self
            .h
            .problem
            .peek()
            .and_then(|(at, m)| diags.iter().position(|d| d.from == at && d.d.message == m));
        let i = match (open, forward) {
            (Some(i), true) => (i + 1) % diags.len(),
            (Some(i), false) => (i + diags.len() - 1) % diags.len(),
            (None, true) => diags.iter().position(|d| d.from > head).unwrap_or(0),
            (None, false) => diags
                .iter()
                .rposition(|d| d.from < head)
                .unwrap_or(diags.len() - 1),
        };
        let d = &diags[i];
        let cur = self.state.peek();
        self.state.set(EditorState {
            doc: cur.doc.clone(),
            selection: crate::edit::Selections::single(crate::edit::Selection::caret(d.from)),
        });
        self.goal.set(crate::geometry::col_of(&src, d.from));
        self.history.peek().borrow_mut().break_group();
        self.h.problem.set(Some((d.from, d.d.message.clone())));
        true
    }

    /// Close the problem peek.
    pub(crate) fn close_problem(&self) -> bool {
        if self.h.problem.peek().is_some() {
            self.h.problem.set(None);
            return true;
        }
        false
    }

    /// The problem peek (VS Code's marker navigation widget) below the problem's line.
    pub(crate) fn problem_block(
        &self,
        t: &crate::theme::EditorTheme,
        fs: f64,
        line_px: f64,
        src: &str,
        diags: &[Diag],
    ) -> Option<crate::extensions::BlockWidget> {
        let (at, msg) = self.h.problem.get()?;
        let i = diags
            .iter()
            .position(|d| d.from == at && d.d.message == msg)?;
        let d = &diags[i];
        let color = severity_color(t, d.d.severity());
        let (up, down, close) = (self.clone(), self.clone(), self.clone());
        let header = row(vec![
            expanded(self.diag_body(&d.d, t, fs * 0.92)).into_widget(),
            t.ui_text(format!("{} of {} problems", i + 1, diags.len()), 0.0)
                .color(t.muted)
                .into_widget(),
            gap_w(6.0).into_widget(),
            icon_button(IconKind::ChevronUp)
                .size(16.0)
                .on_pressed(move || {
                    up.next_problem(false);
                })
                .into_widget(),
            icon_button(IconKind::ChevronDown)
                .size(16.0)
                .on_pressed(move || {
                    down.next_problem(true);
                })
                .into_widget(),
            icon_button(IconKind::Close)
                .size(16.0)
                .on_pressed(move || {
                    close.close_problem();
                })
                .into_widget(),
        ])
        .cross_axis_alignment(CrossAxisAlignment::Start);
        let rows = 2 + d.d.related_information.len() + usize::from(d.d.code.is_some());
        let height = line_px * rows as f64 + 14.0;
        let panel = container()
            .height(height)
            .padding(EdgeInsets::all(6.0))
            .decoration(
                BoxDecoration::new()
                    .color(t.overlay_bg)
                    .border(Border::new(color, 1.0)),
            )
            .child(header);
        Some(crate::extensions::BlockWidget {
            line: crate::geometry::line_of(src, d.to.min(src.len())),
            height,
            widget: panel.into_widget(),
        })
    }

    // ---- edits ---------------------------------------------------------------------------

    /// Apply `edits` (LSP ranges in the current text) as ONE undo step; the cursors map
    /// through the change. Overlapping edits after the first are dropped.
    pub(crate) fn apply_text_edits(&self, edits: &[lsp::TextEdit]) -> bool {
        if edits.is_empty() || self.read_only {
            return false;
        }
        let text = self.state.peek().text();
        let mut changes: Vec<crate::edit::Change> = edits
            .iter()
            .map(|e| {
                let (from, to) = lsp::range_to_bytes(&text, e.range);
                crate::edit::Change {
                    from,
                    to: to.max(from),
                    insert: e.new_text.clone(),
                }
            })
            .collect();
        changes.sort_by_key(|c| (c.from, c.to));
        let mut kept: Vec<crate::edit::Change> = Vec::new();
        for c in changes {
            if kept.last().is_some_and(|l| c.from < l.to) {
                continue;
            }
            if c.from == c.to && c.insert.is_empty() {
                continue;
            }
            if text.get(c.from..c.to) == Some(c.insert.as_str()) {
                continue; // no-op
            }
            kept.push(c);
        }
        if kept.is_empty() {
            return false;
        }
        crate::commands::dispatch(
            self.state,
            self.history,
            self.code,
            crate::edit::Transaction::change(crate::edit::ChangeSet::from_changes(kept)),
            crate::edit::Coalesce::Never,
        );
        self.history.peek().borrow_mut().break_group();
        let now = self.state.peek();
        self.goal
            .set(crate::geometry::col_of(&now.text(), now.primary().head));
        true
    }

    /// Apply a workspace edit: this document's edits as one undo step, the rest through the
    /// host's `apply_workspace_edit`.
    pub(crate) fn apply_workspace_edit(&self, edit: &lsp::WorkspaceEdit) {
        let mut others = lsp::WorkspaceEdit::default();
        for (uri, edits) in &edit.changes {
            if **uri == *self.uri {
                self.apply_text_edits(edits);
            } else {
                others.changes.push((uri.clone(), edits.clone()));
            }
        }
        if !others.changes.is_empty()
            && let Some(svc) = &self.svc
        {
            svc.apply_workspace_edit(&others);
        }
    }

    // ---- rename ----------------------------------------------------------------------------

    /// F2: ask `prepareRename` (or take the word when unsupported) and open the inline box.
    pub(crate) fn start_rename(&self) -> bool {
        let Some(svc) = self.svc.clone() else {
            return false;
        };
        if self.read_only {
            return false;
        }
        let head = self.state.peek().primary().head;
        let me = self.clone();
        let reply = self.guard(&self.store.rename_gate, move |r: Option<lsp::Range>| {
            if let Some(r) = r {
                let text = me.state.peek().text();
                let (a, b) = lsp::range_to_bytes(&text, r);
                me.open_rename(a, b);
            }
        });
        let asked = self.with_doc(|doc, text| {
            svc.prepare_rename(doc, lsp::byte_to_position(text, head), reply)
        });
        if !asked {
            let text = self.state.peek().text();
            if let Some((a, b)) = crate::view::word_range_at(&text, head) {
                self.open_rename(a, b);
            }
        }
        true
    }

    fn open_rename(&self, from: usize, to: usize) {
        let text = self.state.peek().text();
        self.h
            .rename_draft
            .set(text.get(from..to).unwrap_or_default().to_string());
        self.h.rename.set(Some(RenameView { from, to }));
        pebbles::core::focus::set_focus(None); // let the rename field autofocus
    }

    /// Close the rename box (focus returns to the editor).
    pub(crate) fn cancel_rename(&self) {
        self.h.rename.set(None);
        self.focus.request_focus();
    }

    /// Submit the new name: apply this document's edits directly, or preview when the
    /// edit touches other files (or `preview` — Shift+Enter).
    pub(crate) fn submit_rename(&self, preview: bool) {
        let Some(rv) = self.h.rename.peek() else {
            return;
        };
        let name = self.h.rename_draft.peek();
        self.cancel_rename();
        let Some(svc) = self.svc.clone() else { return };
        let text = self.state.peek().text();
        if name.is_empty() || text.get(rv.from..rv.to) == Some(name.as_str()) {
            return;
        }
        let me = self.clone();
        let reply = self.guard(
            &self.store.rename_gate,
            move |e: Option<lsp::WorkspaceEdit>| {
                let Some(e) = e else { return };
                let only_here = e.changes.iter().all(|(u, _)| **u == *me.uri);
                if preview || !only_here {
                    let n = e.changes.len();
                    me.h.rename_preview.set(Some(RenamePreview {
                        edit: e,
                        accepted: vec![true; n],
                    }));
                } else {
                    me.apply_workspace_edit(&e);
                }
            },
        );
        self.with_doc(|doc, text| {
            svc.rename(doc, lsp::byte_to_position(text, rv.from), &name, reply)
        });
    }

    /// Apply the accepted files of the preview and close it.
    pub(crate) fn apply_preview(&self) {
        if let Some(p) = self.h.rename_preview.peek() {
            self.h.rename_preview.set(None);
            self.apply_workspace_edit(&p.accepted_edit());
        }
        self.focus.request_focus();
    }

    /// The inline rename box over the symbol.
    pub(crate) fn rename_layer(&self, f: &Frame) -> Option<AnyWidget> {
        let rv = self.h.rename.get()?;
        let line = f.li.line_of(f.src, rv.from.min(f.src.len()));
        let (x, y) = f.xy(line, f.li.col_of(f.src, rv.from.min(f.src.len())));
        let n = self.h.rename_draft.peek().chars().count();
        let (me, me2) = (self.clone(), self.clone());
        let field = text_field()
            .bind(self.h.rename_draft)
            .autofocus()
            .select_range(0, n)
            .width(240.0)
            .on_nav(move |k| match k {
                KeyInput::Escape => {
                    me.cancel_rename();
                    true
                }
                KeyInput::Enter => {
                    me.submit_rename(pebbles::core::keyboard::shift_held());
                    true
                }
                _ => false,
            })
            .on_submit(move |_| me2.submit_rename(pebbles::core::keyboard::shift_held()));
        let hint = f
            .theme
            .ui_text("Enter to rename, Shift+Enter to preview", -1.0)
            .color(f.theme.muted);
        let panel = container()
            .decoration(
                BoxDecoration::new()
                    .color(f.theme.overlay_bg)
                    .radius(BorderRadius::all(f.theme.overlay_radius))
                    .border(Border::new(f.theme.accent, 1.0)),
            )
            .padding(EdgeInsets::all(4.0))
            .child(
                column(vec![field.into_widget(), hint.into_widget()])
                    .main_axis_size(MainAxisSize::Min),
            );
        Some(
            Positioned::new(panel)
                .left((x - 4.0).max(0.0))
                .top((y - 4.0).max(0.0))
                .into_widget(),
        )
    }

    /// The rename preview dialog: every file with its edits, a checkbox per file, Apply /
    /// Discard.
    pub(crate) fn preview_layer(
        &self,
        t: &crate::theme::EditorTheme,
        mono: &str,
    ) -> Option<AnyWidget> {
        let pv = self.h.rename_preview.get()?;
        let total: usize = pv.edit.changes.iter().map(|(_, e)| e.len()).sum();
        let mut rows: Vec<AnyWidget> = vec![
            t.ui_text(
                format!("Rename: {total} edits in {} files", pv.edit.changes.len()),
                1.0,
            )
            .color(t.foreground)
            .into_widget(),
            gap_h(6.0).into_widget(),
        ];
        let sig = self.h.rename_preview;
        for (i, (uri, edits)) in pv.edit.changes.iter().enumerate() {
            let file = uri.rsplit('/').next().unwrap_or(uri).to_string();
            rows.push(
                checkbox(pv.accepted[i])
                    .label(format!("{file} ({} edits)", edits.len()))
                    .on_changed(move |v| {
                        if let Some(mut p) = sig.peek() {
                            p.accepted[i] = v;
                            sig.set(Some(p));
                        }
                    })
                    .into_widget(),
            );
            let src = self.text_of(uri);
            for e in edits.iter().take(8) {
                let old = src
                    .as_deref()
                    .map(|s| {
                        let (a, b) = lsp::range_to_bytes(s, e.range);
                        s[a..b].to_string()
                    })
                    .unwrap_or_default();
                rows.push(
                    container()
                        .padding(EdgeInsets::only(24.0, 0.0, 0.0, 0.0))
                        .child(
                            text(format!(
                                "{}: {old} \u{2192} {}",
                                e.range.start.line + 1,
                                e.new_text
                            ))
                            .size((t.ui_font_size) as f32)
                            .font_family(mono)
                            .color(if pv.accepted[i] {
                                t.foreground
                            } else {
                                t.muted
                            }),
                        )
                        .into_widget(),
                );
            }
        }
        let (a, d) = (self.clone(), self.clone());
        rows.push(gap_h(8.0).into_widget());
        rows.push(
            row(vec![
                expanded(gap_w(1.0)).into_widget(),
                button("Discard")
                    .size(ButtonSize::Sm)
                    .variant(ButtonVariant::Secondary)
                    .on_pressed(move || {
                        d.h.rename_preview.set(None);
                        d.focus.request_focus();
                    })
                    .into_widget(),
                gap_w(6.0).into_widget(),
                button("Apply")
                    .size(ButtonSize::Sm)
                    .on_pressed(move || a.apply_preview())
                    .into_widget(),
            ])
            .into_widget(),
        );
        let panel = container()
            .width(520.0)
            .decoration(
                BoxDecoration::new()
                    .color(t.overlay_bg)
                    .radius(BorderRadius::all(t.overlay_radius + 2.0))
                    .border(Border::new(t.border, 1.0)),
            )
            .padding(EdgeInsets::all(10.0))
            .child(
                column(rows)
                    .cross_axis_alignment(CrossAxisAlignment::Start)
                    .main_axis_size(MainAxisSize::Min),
            );
        Some(
            Positioned::new(align(Alignment::TOP_CENTER, panel))
                .left(0.0)
                .right(0.0)
                .top(40.0)
                .into_widget(),
        )
    }

    /// Ctrl/Cmd+hover over `byte`: ask for its definition and underline the word when there
    /// is one (the Ctrl+click target).
    pub(crate) fn request_link(&self, byte: usize) {
        if self
            .h
            .link
            .peek()
            .is_some_and(|(a, b)| a <= byte && byte <= b)
        {
            return;
        }
        let Some(svc) = self.svc.clone() else { return };
        let word = crate::view::word_range_at(&self.state.peek().text(), byte);
        let link = self.h.link;
        let Some(w) = word else {
            link.set(None);
            return;
        };
        let reply = self.guard(&self.store.link_gate, move |locs: Vec<lsp::Location>| {
            link.set((!locs.is_empty()).then_some(w));
        });
        let asked = self
            .with_doc(|doc, text| svc.definition(doc, lsp::byte_to_position(text, byte), reply));
        if !asked {
            link.set(None);
        }
    }

    /// Reveal `loc`: in this document move the caret to its start; elsewhere ask the host.
    pub(crate) fn go_to(&self, loc: &lsp::Location) {
        if *loc.uri == *self.uri {
            let cur = self.state.peek();
            let text = cur.text();
            let at = lsp::position_to_byte(&text, loc.range.start);
            self.state.set(EditorState {
                doc: cur.doc.clone(),
                selection: crate::edit::Selections::single(crate::edit::Selection::caret(at)),
            });
            self.goal.set(crate::geometry::col_of(&text, at));
            self.history.peek().borrow_mut().break_group();
        } else if let Some(svc) = &self.svc {
            svc.open_location(loc);
        }
    }
}

/// Count the problems by severity (for the host's status bar).
pub(crate) fn counts(diags: &[Diag]) -> lsp::ProblemCounts {
    use lsp::DiagnosticSeverity as S;
    let mut c = lsp::ProblemCounts::default();
    for d in diags {
        match d.d.severity() {
            S::Error => c.errors += 1,
            S::Warning => c.warnings += 1,
            S::Information => c.infos += 1,
            S::Hint => c.hints += 1,
        }
    }
    c
}

/// The byte range of a parameter label inside its signature label.
pub(crate) fn param_range(label: &str, p: &lsp::ParameterLabel) -> Option<(usize, usize)> {
    match p {
        lsp::ParameterLabel::Simple(s) if !s.is_empty() => {
            // Search after the opening paren so a parameter named like the function matches
            // the parameter, not the name.
            let from = label.find('(').map_or(0, |i| i + 1);
            label[from..]
                .find(s.as_str())
                .map(|i| (from + i, from + i + s.len()))
        }
        lsp::ParameterLabel::Simple(_) => None,
        lsp::ParameterLabel::Offsets(a, b) => {
            let at = |u: u32| -> usize {
                let mut units = 0u32;
                for (i, c) in label.char_indices() {
                    if units >= u {
                        return i;
                    }
                    units += c.len_utf16() as u32;
                }
                label.len()
            };
            let (x, y) = (at(*a), at(*b));
            (x < y).then_some((x, y))
        }
    }
}

/// The theme color for a diagnostic severity.
pub(crate) fn severity_color(t: &crate::theme::EditorTheme, s: lsp::DiagnosticSeverity) -> Color {
    use lsp::DiagnosticSeverity as S;
    match s {
        S::Error => t.diag_error,
        S::Warning => t.diag_warning,
        S::Information => t.diag_info,
        S::Hint => t.diag_hint,
    }
}

fn severity_label(s: lsp::DiagnosticSeverity) -> &'static str {
    use lsp::DiagnosticSeverity as S;
    match s {
        S::Error => "error",
        S::Warning => "warning",
        S::Information => "info",
        S::Hint => "hint",
    }
}

/// Try `a` with the reply; if it declines, hand the (unused) reply to `b`.
fn either<T: 'static>(
    reply: lsp::Reply<T>,
    a: impl FnOnce(lsp::Reply<T>) -> bool,
    b: impl FnOnce(lsp::Reply<T>) -> bool,
) -> bool {
    let slot = Rc::new(RefCell::new(Some(reply)));
    let s = slot.clone();
    let handed: lsp::Reply<T> = Box::new(move |v| {
        if let Some(r) = s.borrow_mut().take() {
            r(v);
        }
    });
    if a(handed) {
        return true;
    }
    let r = slot.borrow_mut().take();
    match r {
        Some(r) => b(r),
        None => true,
    }
}

/// The host's service with the old synchronous providers as a fallback for the requests the
/// service declines.
struct Chained {
    main: Rc<dyn LanguageService>,
    old: lsp::ProviderService,
}

#[rustfmt::skip]
impl LanguageService for Chained {
    fn completion_trigger_characters(&self) -> Vec<String> { self.main.completion_trigger_characters() }
    fn signature_trigger_characters(&self) -> Vec<String> { self.main.signature_trigger_characters() }
    fn on_type_formatting_trigger_characters(&self) -> Vec<String> { self.main.on_type_formatting_trigger_characters() }
    fn completion(&self, d: &lsp::DocRef, p: lsp::Position, c: lsp::CompletionContext, r: lsp::Reply<Option<lsp::CompletionList>>) -> bool { self.main.completion(d, p, c, r) }
    fn resolve_completion(&self, i: lsp::CompletionItem, r: lsp::Reply<lsp::CompletionItem>) -> bool { self.main.resolve_completion(i, r) }
    fn hover(&self, d: &lsp::DocRef, p: lsp::Position, r: lsp::Reply<Option<lsp::Hover>>) -> bool {
        either(r, |r| self.main.hover(d, p, r), |r| self.old.hover(d, p, r))
    }
    fn signature_help(&self, d: &lsp::DocRef, p: lsp::Position, c: lsp::SignatureHelpContext, r: lsp::Reply<Option<lsp::SignatureHelp>>) -> bool {
        let c2 = c.clone();
        either(r, |r| self.main.signature_help(d, p, c, r), |r| self.old.signature_help(d, p, c2, r))
    }
    fn definition(&self, d: &lsp::DocRef, p: lsp::Position, r: lsp::Reply<Vec<lsp::Location>>) -> bool {
        either(r, |r| self.main.definition(d, p, r), |r| self.old.definition(d, p, r))
    }
    fn declaration(&self, d: &lsp::DocRef, p: lsp::Position, r: lsp::Reply<Vec<lsp::Location>>) -> bool { self.main.declaration(d, p, r) }
    fn type_definition(&self, d: &lsp::DocRef, p: lsp::Position, r: lsp::Reply<Vec<lsp::Location>>) -> bool { self.main.type_definition(d, p, r) }
    fn implementation(&self, d: &lsp::DocRef, p: lsp::Position, r: lsp::Reply<Vec<lsp::Location>>) -> bool { self.main.implementation(d, p, r) }
    fn references(&self, d: &lsp::DocRef, p: lsp::Position, i: bool, r: lsp::Reply<Vec<lsp::Location>>) -> bool { self.main.references(d, p, i, r) }
    fn document_highlight(&self, d: &lsp::DocRef, p: lsp::Position, r: lsp::Reply<Vec<lsp::DocumentHighlight>>) -> bool { self.main.document_highlight(d, p, r) }
    fn document_symbols(&self, d: &lsp::DocRef, r: lsp::Reply<Vec<lsp::DocumentSymbol>>) -> bool { self.main.document_symbols(d, r) }
    fn workspace_symbols(&self, q: &str, r: lsp::Reply<Vec<lsp::SymbolInformation>>) -> bool { self.main.workspace_symbols(q, r) }
    fn code_actions(&self, d: &lsp::DocRef, g: lsp::Range, c: lsp::CodeActionContext, r: lsp::Reply<Vec<lsp::CodeAction>>) -> bool { self.main.code_actions(d, g, c, r) }
    fn resolve_code_action(&self, a: lsp::CodeAction, r: lsp::Reply<lsp::CodeAction>) -> bool { self.main.resolve_code_action(a, r) }
    fn code_lens(&self, d: &lsp::DocRef, r: lsp::Reply<Vec<lsp::CodeLens>>) -> bool { self.main.code_lens(d, r) }
    fn resolve_code_lens(&self, l: lsp::CodeLens, r: lsp::Reply<lsp::CodeLens>) -> bool { self.main.resolve_code_lens(l, r) }
    fn formatting(&self, d: &lsp::DocRef, o: lsp::FormattingOptions, r: lsp::Reply<Vec<lsp::TextEdit>>) -> bool {
        either(r, |r| self.main.formatting(d, o, r), |r| self.old.formatting(d, o, r))
    }
    fn range_formatting(&self, d: &lsp::DocRef, g: lsp::Range, o: lsp::FormattingOptions, r: lsp::Reply<Vec<lsp::TextEdit>>) -> bool { self.main.range_formatting(d, g, o, r) }
    fn on_type_formatting(&self, d: &lsp::DocRef, p: lsp::Position, ch: &str, o: lsp::FormattingOptions, r: lsp::Reply<Vec<lsp::TextEdit>>) -> bool { self.main.on_type_formatting(d, p, ch, o, r) }
    fn prepare_rename(&self, d: &lsp::DocRef, p: lsp::Position, r: lsp::Reply<Option<lsp::Range>>) -> bool { self.main.prepare_rename(d, p, r) }
    fn rename(&self, d: &lsp::DocRef, p: lsp::Position, n: &str, r: lsp::Reply<Option<lsp::WorkspaceEdit>>) -> bool { self.main.rename(d, p, n, r) }
    fn inlay_hints(&self, d: &lsp::DocRef, g: lsp::Range, r: lsp::Reply<Vec<lsp::InlayHint>>) -> bool { self.main.inlay_hints(d, g, r) }
    fn prepare_call_hierarchy(&self, d: &lsp::DocRef, p: lsp::Position, r: lsp::Reply<Vec<lsp::CallHierarchyItem>>) -> bool { self.main.prepare_call_hierarchy(d, p, r) }
    fn incoming_calls(&self, i: &lsp::CallHierarchyItem, r: lsp::Reply<Vec<lsp::CallHierarchyIncomingCall>>) -> bool { self.main.incoming_calls(i, r) }
    fn outgoing_calls(&self, i: &lsp::CallHierarchyItem, r: lsp::Reply<Vec<lsp::CallHierarchyOutgoingCall>>) -> bool { self.main.outgoing_calls(i, r) }
    fn prepare_type_hierarchy(&self, d: &lsp::DocRef, p: lsp::Position, r: lsp::Reply<Vec<lsp::TypeHierarchyItem>>) -> bool { self.main.prepare_type_hierarchy(d, p, r) }
    fn supertypes(&self, i: &lsp::TypeHierarchyItem, r: lsp::Reply<Vec<lsp::TypeHierarchyItem>>) -> bool { self.main.supertypes(i, r) }
    fn subtypes(&self, i: &lsp::TypeHierarchyItem, r: lsp::Reply<Vec<lsp::TypeHierarchyItem>>) -> bool { self.main.subtypes(i, r) }
    fn open_location(&self, l: &lsp::Location) -> bool { self.main.open_location(l) }
    fn show_references(&self, o: &lsp::Location, l: &[lsp::Location]) -> bool { self.main.show_references(o, l) }
    fn read_text(&self, u: &str) -> Option<String> { self.main.read_text(u) }
    fn apply_workspace_edit(&self, e: &lsp::WorkspaceEdit) -> bool { self.main.apply_workspace_edit(e) }
    fn execute_command(&self, c: &lsp::LspCommand) -> bool { self.main.execute_command(c) }
}

/// The effective service for an editor: the host's [`LanguageService`] with the old
/// synchronous providers (hover, signature help, definition, format) as fallback, or just
/// the providers adapted, or none.
pub(crate) fn effective_service(p: &crate::config::Props) -> Option<Rc<dyn LanguageService>> {
    let old = lsp::ProviderService {
        completion: None,
        hover: p.hover.clone(),
        signature: p.signature.clone(),
        definition: p.definition.clone(),
        format: p.format.clone(),
    };
    let any_old = old.hover.is_some()
        || old.signature.is_some()
        || old.definition.is_some()
        || old.format.is_some();
    match (p.language_service.clone(), any_old) {
        (Some(main), true) => Some(Rc::new(Chained { main, old })),
        (Some(main), false) => Some(main),
        (None, true) => Some(Rc::new(old)),
        (None, false) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameter_labels_resolve_by_text_and_utf16_offsets() {
        let l = "f(f: i32, é: u8)";
        assert_eq!(
            param_range(l, &lsp::ParameterLabel::Simple("f: i32".into())),
            Some((2, 8))
        );
        // UTF-16 offsets: 'é' is one unit but two bytes.
        assert_eq!(
            param_range(l, &lsp::ParameterLabel::Offsets(10, 15)),
            Some((10, 16))
        );
        assert_eq!(
            param_range(l, &lsp::ParameterLabel::Simple("zz".into())),
            None
        );
    }

    #[test]
    fn markdown_hover_splits_into_sections_at_rules_outside_fences() {
        let c =
            lsp::MarkupContent::markdown("```rust\nfn f()\n---\n```\n---\nDocs here.\n***\nMore");
        let s = sections(&c);
        assert_eq!(s.len(), 3);
        assert!(s[0].value.contains("---"), "a rule inside a fence stays");
        assert_eq!(s[1].value, "Docs here.");
        assert_eq!(s[2].value, "More");
        let p = lsp::MarkupContent::plain("a\n---\nb");
        assert_eq!(sections(&p).len(), 1);
    }
}
