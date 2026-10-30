//! The autocomplete popup: session state, the trigger/accept logic, snippet expansion, and
//! the popup widget. The provider (dev-supplied) returns candidates; this module filters
//! them by the typed prefix, drives selection, and applies the accepted insert (expanding
//! snippet tabstops). Keyboard navigation is wired in [`super`]'s key handler.

use std::cell::RefCell;
use std::rc::Rc;

use pebbles::prelude::*;

use crate::commands::dispatch;
use crate::edit::{
    Change, ChangeSet, Coalesce, EditorState, History, Selection, Selections, Transaction,
};
use crate::geometry::{col_of, is_word, prev_char};
use crate::language::{self as lsp, LanguageService, ProviderService, RequestGate};
use crate::providers::{CompletionContext, CompletionItem, CompletionKind, CompletionProvider};

/// A live completion session: every item the source returned (`all`), the filtered + ranked
/// view (`items`, with the matched label chars in `hits`), the selection, and the byte where
/// the typed word starts (accepting replaces `anchor..caret` unless the item has its own
/// edit range).
#[derive(Clone)]
pub(crate) struct Session {
    pub(crate) all: Rc<Vec<lsp::CompletionItem>>,
    pub(crate) items: Vec<lsp::CompletionItem>,
    pub(crate) hits: Vec<Vec<usize>>,
    pub(crate) selected: usize,
    pub(crate) anchor: usize,
    /// The source said the list is incomplete: further typing re-queries.
    pub(crate) incomplete: bool,
}

impl Session {
    /// Move the highlight (wrapping).
    pub(crate) fn move_by(&mut self, delta: isize) {
        let n = self.items.len() as isize;
        if n > 0 {
            self.selected = (((self.selected as isize + delta) % n + n) % n) as usize;
        }
    }
    /// The highlighted item.
    pub(crate) fn current(&self) -> Option<&lsp::CompletionItem> {
        self.items.get(self.selected)
    }
}

/// VS Code-style fuzzy match of `pattern` against `word`: every pattern char must appear in
/// order (case-insensitively); the first one must land on a strong position (the word start,
/// after a separator, or a camelCase hump). Contiguous runs, word starts, camelCase humps and
/// exact case score bonuses; gaps cost a little. Returns the score and the matched char
/// indices of `word`, or `None` when it does not match.
pub(crate) fn fuzzy_score(pattern: &str, word: &str) -> Option<(i32, Vec<usize>)> {
    const NEG: i32 = i32::MIN / 4;
    let p: Vec<char> = pattern.chars().collect();
    let w: Vec<char> = word.chars().take(255).collect();
    if p.is_empty() {
        return Some((0, Vec::new()));
    }
    let (n, m) = (p.len(), w.len());
    if n > m {
        return None;
    }
    let strong = |j: usize| {
        j == 0
            || !w[j - 1].is_alphanumeric()
            || (w[j].is_uppercase() && w[j - 1].is_lowercase())
            || (w[j].is_ascii_digit() && !w[j - 1].is_ascii_digit())
    };
    let same = |a: char, b: char| a == b || a.to_lowercase().eq(b.to_lowercase());
    let mut dp = vec![vec![NEG; m]; n];
    let mut from = vec![vec![usize::MAX; m]; n];
    for i in 0..n {
        // Best `dp[i-1][k]` over k < j-1 (a gap), maintained as j advances.
        let mut gap_best = (NEG, usize::MAX);
        for j in i..m {
            if i > 0 && j >= 2 && dp[i - 1][j - 2] > gap_best.0 {
                gap_best = (dp[i - 1][j - 2], j - 2);
            }
            if !same(p[i], w[j]) {
                continue;
            }
            let mut bonus = 1;
            if p[i] == w[j] {
                bonus += 1;
            }
            if j == 0 {
                bonus += 8;
            } else if strong(j) {
                bonus += 6;
            }
            if i == 0 {
                if strong(j) {
                    dp[0][j] = bonus;
                }
                continue;
            }
            let contiguous = if j >= 1 && dp[i - 1][j - 1] > NEG {
                dp[i - 1][j - 1] + 5
            } else {
                NEG
            };
            let gapped = if gap_best.0 > NEG {
                gap_best.0 - 1
            } else {
                NEG
            };
            if contiguous >= gapped && contiguous > NEG {
                dp[i][j] = contiguous + bonus;
                from[i][j] = j - 1;
            } else if gapped > NEG {
                dp[i][j] = gapped + bonus;
                from[i][j] = gap_best.1;
            }
        }
    }
    let (mut j, score) = dp[n - 1]
        .iter()
        .enumerate()
        .filter(|(_, s)| **s > NEG)
        .max_by_key(|(j, s)| (**s, std::cmp::Reverse(*j)))
        .map(|(j, s)| (j, *s))?;
    let mut hits = vec![0; n];
    for i in (0..n).rev() {
        hits[i] = j;
        j = from[i][j];
    }
    Some((score, hits))
}

/// Filter + rank `all` against the text typed since each item's word start.
fn filter(
    all: &[lsp::CompletionItem],
    src: &str,
    anchor: usize,
    caret: usize,
) -> (Vec<lsp::CompletionItem>, Vec<Vec<usize>>) {
    let mut scored: Vec<(i32, &lsp::CompletionItem, Vec<usize>)> = all
        .iter()
        .filter_map(|it| {
            let start = item_start(it, src).unwrap_or(anchor).min(caret);
            let typed = src.get(start..caret).unwrap_or("");
            let (score, hits) = fuzzy_score(typed, it.filter_key())?;
            // Highlight on the label (only when the filter key *is* the label).
            let hits = if it.filter_text.is_none() {
                hits
            } else {
                Vec::new()
            };
            Some((score, it, hits))
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.sort_key().cmp(b.1.sort_key()))
            .then_with(|| a.1.label.cmp(&b.1.label))
    });
    scored.into_iter().map(|(_, it, h)| (it.clone(), h)).unzip()
}

/// The start byte of an item's own edit range (when it has one on the caret line).
fn item_start(it: &lsp::CompletionItem, src: &str) -> Option<usize> {
    let r = match it.text_edit.as_ref()? {
        lsp::CompletionTextEdit::Edit(e) => e.range,
        lsp::CompletionTextEdit::InsertReplace { replace, .. } => *replace,
    };
    Some(lsp::range_to_bytes(src, r).0)
}

/// Open (or close) the popup over `all` for the current caret.
fn open(
    state: Signal<EditorState>,
    session: Signal<Option<Session>>,
    anchor: usize,
    all: Vec<lsp::CompletionItem>,
    incomplete: bool,
) {
    let st = state.peek();
    let src = st.text();
    let caret = st.primary().head;
    if caret < anchor || all.is_empty() {
        session.set(None);
        return;
    }
    let all = Rc::new(all);
    let (items, hits) = filter(&all, &src, anchor, caret);
    let selected = if src[anchor..caret].is_empty() {
        items.iter().position(|i| i.preselect).unwrap_or(0)
    } else {
        0
    };
    session.set((!items.is_empty()).then_some(Session {
        all,
        items,
        hits,
        selected,
        anchor,
        incomplete,
    }));
}

/// Re-filter the open session after typing (no re-query). Closes it when the caret left the
/// word or nothing matches.
pub(crate) fn refilter(state: Signal<EditorState>, session: Signal<Option<Session>>) {
    let Some(s) = session.peek() else { return };
    open(state, session, s.anchor, (*s.all).clone(), s.incomplete);
}

/// Word-based completion (VS Code `editor.wordBasedSuggestions`, used when no provider is
/// set): every distinct word of 2+ chars in the text around the caret (bounded to 32 KB, so cost is constant per keystroke),
/// excluding the word being typed; nearer words rank first.
pub(crate) fn word_provider() -> CompletionProvider {
    Rc::new(|cx: &CompletionContext| {
        const SPAN: usize = 16 * 1024;
        let src = cx.src;
        let mut lo = cx.cursor.saturating_sub(SPAN);
        while !src.is_char_boundary(lo) {
            lo -= 1;
        }
        let mut hi = (cx.cursor + SPAN).min(src.len());
        while !src.is_char_boundary(hi) {
            hi += 1;
        }
        let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        let mut start = None;
        for (i, c) in src[lo..hi]
            .char_indices()
            .chain(std::iter::once((hi - lo, ' ')))
        {
            let at = lo + i;
            if is_word(c) {
                start.get_or_insert(at);
            } else if let Some(s) = start.take() {
                let w = &src[s..at];
                // Skip the word being typed (it ends at the caret) and numbers.
                if w.chars().count() >= 2
                    && at != cx.cursor
                    && !w.chars().next().is_some_and(|c| c.is_ascii_digit())
                {
                    let d = s.abs_diff(cx.cursor);
                    let e = seen.entry(w).or_insert(d);
                    *e = (*e).min(d);
                }
            }
        }
        let mut words: Vec<(&str, usize)> = seen.into_iter().collect();
        words.sort_by_key(|&(w, d)| (d, w));
        words
            .into_iter()
            .take(200)
            .map(|(w, _)| CompletionItem::new(w, CompletionKind::Text))
            .collect()
    })
}

/// The identifier prefix start immediately before `caret`.
pub(crate) fn prefix_start(src: &str, caret: usize) -> usize {
    let mut b = caret;
    while b > 0 {
        let p = prev_char(src, b);
        if src[p..b].chars().next().is_some_and(is_word) {
            b = p;
        } else {
            break;
        }
    }
    b
}

/// The completion engine for one editor: the host [`LanguageService`] (if any), the local
/// source (the old provider or word-based suggestions, plus snippets) used when the host does
/// not support completion, the snippet source merged into host results, and the
/// stale-reply gates.
pub(crate) struct Completer {
    pub(crate) host: Option<Rc<dyn LanguageService>>,
    pub(crate) local: Option<CompletionProvider>,
    pub(crate) snippets: Option<CompletionProvider>,
    pub(crate) gate: RequestGate,
    pub(crate) resolve_gate: RequestGate,
    pub(crate) uri: String,
    pub(crate) triggers: Vec<String>,
    pub(crate) line_comment: Option<String>,
}

impl Completer {
    /// Whether any completion source exists.
    pub(crate) fn has_source(&self) -> bool {
        self.host.is_some() || self.local.is_some()
    }
    /// Whether typing `s` should open completion (a server trigger character).
    pub(crate) fn is_trigger(&self, s: &str) -> bool {
        self.triggers.iter().any(|t| t == s)
    }
    /// Request completions at the primary caret. `explicit` (Ctrl+Space, trigger chars,
    /// incomplete re-query) opens even with nothing typed.
    pub(crate) fn request(
        &self,
        state: Signal<EditorState>,
        session: Signal<Option<Session>>,
        explicit: bool,
        cx: lsp::CompletionContext,
    ) {
        let st = state.peek();
        let src = st.text();
        let caret = st.primary().head;
        let anchor = prefix_start(&src, caret);
        if anchor == caret && !explicit {
            self.gate.cancel();
            session.set(None);
            return;
        }
        let ctx = CompletionContext {
            src: &src,
            cursor: caret,
            prefix: &src[anchor..caret],
        };
        let extra: Vec<lsp::CompletionItem> = self
            .snippets
            .as_ref()
            .map(|p| p(&ctx).into_iter().map(Into::into).collect())
            .unwrap_or_default();
        let doc = lsp::DocRef {
            uri: &self.uri,
            text: &src,
            version: self.gate.version(),
        };
        let pos = lsp::byte_to_position(&src, caret);
        // One reply, handed to the host first and taken back if it declines.
        let deliver = move |list: Option<lsp::CompletionList>, extra: Vec<lsp::CompletionItem>| {
            let list = list.unwrap_or_default();
            let mut items = list.items;
            items.extend(extra);
            open(state, session, anchor, items, list.is_incomplete);
        };
        let slot: Rc<RefCell<Option<lsp::Reply<Option<lsp::CompletionList>>>>> =
            Rc::new(RefCell::new(None));
        if let Some(h) = &self.host {
            let extra = extra.clone();
            *slot.borrow_mut() = Some(self.gate.wrap(move |l| deliver(l, extra)));
            let s2 = slot.clone();
            let handed: lsp::Reply<Option<lsp::CompletionList>> = Box::new(move |l| {
                if let Some(r) = s2.borrow_mut().take() {
                    r(l);
                }
            });
            if h.completion(&doc, pos, cx.clone(), handed) {
                return;
            }
            slot.borrow_mut().take(); // declined: fall back to the local source
        }
        let local = ProviderService {
            completion: self.local.clone(),
            ..Default::default()
        };
        let reply = self.gate.wrap(move |l| deliver(l, Vec::new()));
        if !local.completion(&doc, pos, cx, reply) {
            session.set(None);
        }
    }

    /// Ask the host to resolve the highlighted item (lazy documentation / detail / edits).
    pub(crate) fn resolve_selected(&self, session: Signal<Option<Session>>) {
        let (Some(h), Some(s)) = (&self.host, session.peek()) else {
            return;
        };
        let Some(item) = s.current().cloned() else {
            return;
        };
        if item.documentation.is_some() && item.detail.is_some() {
            return;
        }
        let idx = s.selected;
        let label = item.label.clone();
        let reply = self
            .resolve_gate
            .wrap(move |resolved: lsp::CompletionItem| {
                let mut cur = session.peek();
                if let Some(c) = &mut cur
                    && c.items.get(idx).is_some_and(|i| i.label == label)
                {
                    c.items[idx] = resolved;
                    session.set(cur);
                }
            });
        h.resolve_completion(item, reply);
    }
}

/// Apply the selected item — its own edit range (or the typed word), the inserted text
/// (expanding snippets) and its `additionalTextEdits` (auto-imports) as **one** undo step —
/// close the popup, then run the item's command.
///
/// With `wait`, an item without `additionalTextEdits` is first resolved
/// (`completionItem/resolve`, which is where servers put auto-imports): it applies when the
/// reply arrives or after a short timeout, whichever comes first. A commit character does not
/// wait (the character is typed right after).
#[allow(clippy::too_many_arguments)]
pub(crate) fn accept(
    state: Signal<EditorState>,
    history: Signal<Rc<RefCell<History>>>,
    code: Signal<String>,
    goal: Signal<usize>,
    session: Signal<Option<Session>>,
    completer: &Rc<Completer>,
    snippet: Signal<Option<crate::snippet::SnippetSession>>,
    wait: bool,
) {
    let Some(s) = session.peek() else { return };
    let Some(item) = s.current().cloned() else {
        return;
    };
    session.set(None);
    completer.gate.cancel();
    let anchor = s.anchor;
    let apply = {
        let completer = completer.clone();
        let done = Rc::new(std::cell::Cell::new(false));
        move |it: lsp::CompletionItem| {
            if !done.replace(true) {
                apply_item(
                    state, history, code, goal, session, &completer, snippet, it, anchor,
                );
            }
        }
    };
    if wait
        && item.additional_text_edits.is_empty()
        && let Some(h) = completer.host.clone()
    {
        let apply = Rc::new(apply);
        let (a2, orig) = (apply.clone(), item.clone());
        let reply: lsp::Reply<lsp::CompletionItem> = Box::new(move |mut r: lsp::CompletionItem| {
            // Resolve must not change what is inserted; keep the original's edit if dropped.
            if r.text_edit.is_none() {
                r.text_edit = orig.text_edit.clone();
            }
            if r.insert_text.is_none() {
                r.insert_text = orig.insert_text.clone();
            }
            a2(r);
        });
        if h.resolve_completion(item.clone(), reply) {
            let fallback = item.clone();
            pebbles::core::animation::set_timeout(
                session.raw_id() ^ 0x5245_534f_4c56_4500,
                0.15,
                move || apply(fallback.clone()),
            );
            return;
        }
        apply(item);
        return;
    }
    apply(item);
}

/// Apply `item` at `anchor` (see [`accept`]).
#[allow(clippy::too_many_arguments)]
fn apply_item(
    state: Signal<EditorState>,
    history: Signal<Rc<RefCell<History>>>,
    code: Signal<String>,
    goal: Signal<usize>,
    session: Signal<Option<Session>>,
    completer: &Completer,
    snippet: Signal<Option<crate::snippet::SnippetSession>>,
    item: lsp::CompletionItem,
    anchor: usize,
) {
    let st = state.peek();
    let src = st.text();
    let caret = st.primary().head;
    let (a, b) = match &item.text_edit {
        Some(lsp::CompletionTextEdit::Edit(e)) => lsp::range_to_bytes(&src, e.range),
        Some(lsp::CompletionTextEdit::InsertReplace { replace, .. }) => {
            lsp::range_to_bytes(&src, *replace)
        }
        None => (anchor, caret),
    };
    // The range was computed when the list arrived; cover what was typed since.
    let (a, b) = (a.min(caret), b.max(caret));
    let raw = item.new_text();
    let extra: Vec<Change> = item
        .additional_text_edits
        .iter()
        .map(|e| {
            let (x, y) = lsp::range_to_bytes(&src, e.range);
            Change {
                from: x,
                to: y,
                insert: e.new_text.clone(),
            }
        })
        .collect();
    if item.insert_text_format == lsp::InsertTextFormat::Snippet {
        crate::snippet::insert(
            state,
            history,
            code,
            snippet,
            a,
            b,
            raw,
            extra,
            &crate::snippet::InsertOpts {
                uri: &completer.uri,
                line_comment: completer.line_comment.as_deref(),
                adjust_indent: true,
            },
        );
        if let Some(sn) = snippet.peek() {
            open_choices(session, sn.stop());
        }
    } else {
        let mut changes = vec![Change {
            from: a,
            to: b,
            insert: raw.to_string(),
        }];
        let mut shift = 0isize;
        for c in extra {
            if c.to <= a {
                shift += c.insert.len() as isize - (c.to - c.from) as isize;
            } else if c.from < b {
                continue; // overlaps the main edit: invalid per LSP, skip
            }
            changes.push(c);
        }
        let at = (a as isize + shift) as usize + raw.len();
        dispatch(
            state,
            history,
            code,
            Transaction::change_and_select(
                ChangeSet::from_changes(changes),
                Selections::single(Selection::caret(at)),
            ),
            Coalesce::Never,
        );
    }
    goal.set(col_of(&state.peek().text(), state.peek().primary().head));
    if let (Some(cmd), Some(h)) = (&item.command, &completer.host) {
        h.execute_command(cmd);
    }
}

/// Offer a snippet choice stop's options (`${1|a,b|}`) as a pick list over the stop.
pub(crate) fn open_choices(session: Signal<Option<Session>>, stop: &crate::snippet::Stop) {
    if stop.choices.len() < 2 {
        return;
    }
    let items: Vec<lsp::CompletionItem> = stop
        .choices
        .iter()
        .map(|c| lsp::CompletionItem::new(c.clone(), lsp::CompletionItemKind::Value))
        .collect();
    session.set(Some(Session {
        all: Rc::new(items.clone()),
        hits: vec![Vec::new(); items.len()],
        items,
        selected: 0,
        anchor: stop.ranges[0].0,
        incomplete: false,
    }));
}

/// The tabler icon and color for a completion kind (VS Code's symbol colors, from the theme).
fn kind_icon(
    kind: Option<lsp::CompletionItemKind>,
    theme: &crate::theme::EditorTheme,
) -> (IconData, Color) {
    use crate::lang::TokenKind as T;
    use lsp::CompletionItemKind as K;
    let Some(kind) = kind else {
        return (tabler::ABC, theme.muted);
    };
    let (icon, tk) = match kind {
        K::Text => (tabler::ABC, None),
        K::Method => (tabler::CUBE, Some(T::Function)),
        K::Function => (tabler::MATH_FUNCTION, Some(T::Function)),
        K::Constructor => (tabler::TOOL, Some(T::Function)),
        K::Field => (tabler::TAG, Some(T::Property)),
        K::Variable => (tabler::VARIABLE, Some(T::Property)),
        K::Class => (tabler::BOX, Some(T::Type)),
        K::Interface => (tabler::PLUG, Some(T::Type)),
        K::Module => (tabler::PACKAGE, Some(T::Namespace)),
        K::Property => (tabler::KEY, Some(T::Property)),
        K::Unit => (tabler::RULER, Some(T::Constant)),
        K::Value => (tabler::NUMBERS, Some(T::Constant)),
        K::Enum => (tabler::LIST_NUMBERS, Some(T::Type)),
        K::Keyword => (tabler::CODE, Some(T::Keyword)),
        K::Snippet => (tabler::BRACKETS_CONTAIN, None),
        K::Color => (tabler::PALETTE, Some(T::Constant)),
        K::File => (tabler::FILE, Some(T::Namespace)),
        K::Reference => (tabler::ARROW_FORWARD_UP, Some(T::Property)),
        K::Folder => (tabler::FOLDER, Some(T::Namespace)),
        K::EnumMember => (tabler::SQUARE_LETTER_E, Some(T::Constant)),
        K::Constant => (tabler::SQUARE_LETTER_C, Some(T::Constant)),
        K::Struct => (tabler::SITEMAP, Some(T::Type)),
        K::Event => (tabler::BOLT, Some(T::Type)),
        K::Operator => (tabler::PLUS_MINUS, Some(T::Keyword)),
        K::TypeParameter => (tabler::LETTER_T, Some(T::Type)),
    };
    (icon, tk.map_or(theme.foreground, |k| theme.color(k)))
}

/// The popup: the ranked list (kind icon, label with the matched chars emphasized, label
/// details, right-aligned detail) and, beside it, the details + documentation pane for the
/// highlighted item (Markdown rendered by [`super::markdown`]).
pub(crate) fn popup(s: &Session, f: &crate::view::Frame) -> AnyWidget {
    const ROWS: usize = 12;
    let (theme, fs, line_px) = (f.theme, f.fs, f.line_px);
    let line = f.li.line_of(f.src, s.anchor);
    let col = f.li.col_of(f.src, s.anchor);
    let x = f.pad_l + col as f64 * f.advance;
    let y = f.pad_t + (line + 1) as f64 * line_px + 2.0;
    let first = s.selected.saturating_sub(ROWS - 1);
    let mut rows: Vec<AnyWidget> = Vec::new();
    for (i, it) in s.items.iter().enumerate().skip(first).take(ROWS) {
        let selected = i == s.selected;
        let bg = if selected {
            theme.selection
        } else {
            Color::from_rgba8(0, 0, 0, 0)
        };
        let (ic, icol) = kind_icon(it.kind, theme);
        let hits = s.hits.get(i).cloned().unwrap_or_default();
        let mut label: Vec<TextSpan> = it
            .label
            .chars()
            .enumerate()
            .map(|(k, c)| {
                let mut sp = span(c.to_string())
                    .font_family(f.font_family)
                    .size(fs as f32)
                    .color(theme.foreground);
                if hits.contains(&k) {
                    sp = sp.bold().color(theme.accent);
                }
                if it.deprecated {
                    sp = sp.strikethrough();
                }
                sp
            })
            .collect();
        if let Some(d) = it.label_details.as_ref().and_then(|d| d.detail.clone()) {
            label.push(
                span(d)
                    .font_family(f.font_family)
                    .size(fs as f32)
                    .color(theme.muted),
            );
        }
        let mut cells: Vec<AnyWidget> = vec![
            icon(ic).size(fs * 1.05).color(icol).into_widget(),
            gap_w(6.0).into_widget(),
            text_rich(label).soft_wrap(false).into_widget(),
        ];
        let right = it
            .label_details
            .as_ref()
            .and_then(|d| d.description.clone())
            .or_else(|| it.detail.clone());
        if let Some(d) = right {
            cells.push(spacer().into_widget());
            cells.push(gap_w(10.0).into_widget());
            cells.push(
                text(d.lines().next().unwrap_or("").to_string())
                    .size((fs * 0.85) as f32)
                    .font_family(f.font_family)
                    .color(theme.muted)
                    .into_widget(),
            );
        }
        rows.push(
            container()
                .height(line_px)
                .padding(EdgeInsets::symmetric(8.0, 0.0))
                .decoration(BoxDecoration::new().color(bg))
                .clip()
                .child(row(cells).cross_axis_alignment(CrossAxisAlignment::Center))
                .into_widget(),
        );
    }
    let deco = || {
        BoxDecoration::new()
            .color(theme.overlay_bg)
            .radius(BorderRadius::all(theme.overlay_radius))
            .border(Border::new(theme.border, 1.0))
    };
    let list = container()
        .width(340.0)
        .decoration(deco())
        .clip()
        .child(column(rows).main_axis_size(MainAxisSize::Min));
    let mut parts: Vec<AnyWidget> = vec![list.into_widget()];
    if let Some(it) = s.current()
        && (it.detail.is_some() || it.documentation.is_some())
    {
        let mut kids: Vec<AnyWidget> = Vec::new();
        if let Some(d) = &it.detail {
            kids.push(
                text_rich(super::markdown::code_spans(
                    d,
                    f.p.language.as_ref().map_or("", |l| l.name()),
                    theme,
                    (fs * 0.92) as f32,
                    f.font_family,
                ))
                .into_widget(),
            );
        }
        if let Some(doc) = &it.documentation {
            if !kids.is_empty() {
                kids.push(gap_h(6.0).into_widget());
            }
            kids.push(match doc.kind {
                lsp::MarkupKind::Markdown => {
                    super::markdown::render(&doc.value, theme, fs * 0.92, f.font_family)
                }
                lsp::MarkupKind::PlainText => text(doc.value.clone())
                    .size((fs * 0.92) as f32)
                    .color(theme.foreground)
                    .into_widget(),
            });
        }
        parts.push(gap_w(4.0).into_widget());
        parts.push(
            container()
                .width(320.0)
                .padding(EdgeInsets::symmetric(10.0, 8.0))
                .decoration(deco())
                .clip()
                .child(
                    column(kids)
                        .cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .main_axis_size(MainAxisSize::Min),
                )
                .into_widget(),
        );
    }
    Positioned::new(
        row(parts)
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .main_axis_size(MainAxisSize::Min),
    )
    .left(x)
    .top(y)
    .into_widget()
}

#[cfg(test)]
mod fuzzy_tests {
    use super::fuzzy_score;

    fn score(p: &str, w: &str) -> i32 {
        fuzzy_score(p, w).map_or(i32::MIN, |s| s.0)
    }

    #[test]
    fn fuzzy_scoring_rewards_contiguous_camel_case_and_word_starts() {
        // Matches in order, case-insensitively; returns the matched char indices.
        assert_eq!(fuzzy_score("gw", "getWidth").unwrap().1, vec![0, 3]);
        assert_eq!(fuzzy_score("", "x").unwrap().0, 0);
        // The first pattern char must land on a strong position.
        assert!(fuzzy_score("idt", "getWidth").is_none());
        assert!(fuzzy_score("wid", "getWidth").is_some()); // camelCase hump
        assert!(fuzzy_score("val", "max_value").is_some()); // after a separator
        assert!(fuzzy_score("ab", "a").is_none());
        // Contiguous prefix > camelCase humps > scattered.
        assert!(score("fb", "fbz") > score("fb", "fooBar"));
        assert!(score("fb", "fooBar") > score("fb", "fabric"));
        // Exact case wins a tie.
        assert!(score("Foo", "Foo") > score("Foo", "foo"));
        // Word start beats mid-word.
        assert!(score("con", "console") > score("con", "my_icon_conf"));
    }
}
