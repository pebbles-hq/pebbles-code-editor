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

/// Non-reactive per-editor state (gates, caches, the document version counter).
#[derive(Default)]
pub(crate) struct Store {
    pub(crate) hover_gate: RequestGate,
    pub(crate) sig_gate: RequestGate,
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
    pub(crate) store: Signal<Rc<Store>>,
}

/// Create the hook signals (call unconditionally, once per render).
pub(crate) fn hooks() -> Hooks {
    Hooks {
        hover: create_signal(None),
        hover_sticky: create_signal(false),
        sig: create_signal(None),
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
        let slot = Rc::new(RefCell::new(Some(show)));
        let s2 = slot.clone();
        let reply = self.guard(&self.store.hover_gate, move |h: Option<lsp::Hover>| {
            let mut c = store.hover_cache.borrow_mut();
            if c.len() > 256 {
                c.clear();
            }
            c.insert(key, h.clone());
            drop(c);
            if let Some(f) = s2.borrow_mut().take() {
                f(h);
            }
        });
        let asked =
            self.with_doc(|doc, text| svc.hover(doc, lsp::byte_to_position(text, byte), reply));
        if !asked && let Some(f) = slot.borrow_mut().take() {
            f(None);
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
            kids.push(self.diag_body(d, f, fs));
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
    pub(crate) fn diag_body(&self, d: &lsp::LspDiagnostic, f: &Frame, fs: f64) -> AnyWidget {
        let t = f.theme;
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
