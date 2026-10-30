//! In-text decorations that mirror VS Code's editor surface: word-occurrence highlights,
//! the active indent guide, bracket-pair guides, Unicode highlighting + control-character
//! symbols, link underlines, colour swatches, the folded-region badge and linked-editing
//! outlines. Pure builders over the [`Frame`], like [`super::overlays`].

use pebbles::prelude::*;

use crate::geometry::line_end;
use crate::scan_text::{UnicodeKind, control_symbol, find_colors, find_links, unicode_marks};
use crate::view::Frame;
use crate::view::chrome::with_alpha;

fn rect(x: f64, y: f64, w: f64, h: f64, color: Color) -> AnyWidget {
    Positioned::new(
        container()
            .width(w.max(1.0))
            .height(h.max(1.0))
            .decoration(BoxDecoration::new().color(color)),
    )
    .left(x)
    .top(y)
    .into_widget()
}

fn outline(x: f64, y: f64, w: f64, h: f64, color: Color) -> AnyWidget {
    Positioned::new(
        container().width(w.max(1.0)).height(h.max(1.0)).decoration(
            BoxDecoration::new()
                .border(Border::new(color, 1.0))
                .radius(BorderRadius::all(2.0)),
        ),
    )
    .left(x)
    .top(y)
    .into_widget()
}

/// Word-occurrence highlight (VS Code `editor.occurrencesHighlight`): with a bare caret in a
/// word, box every whole-word occurrence of it in the visible window.
pub(crate) fn occurrences(f: &Frame, layers: &mut Vec<AnyWidget>) {
    if !f.p.occurrences_highlight || !f.highlight_word_matches || f.sels.is_multi() {
        return;
    }
    let p = f.sels.primary();
    if !p.is_empty() {
        return;
    }
    let (a, b) = crate::geometry::word_at(f.src, p.head);
    if b <= a {
        return;
    }
    let word = &f.src[a..b];
    let lo = f.li.line_start_of(f.first_line);
    let hi = line_end(f.src, f.li.line_start_of(f.last_line));
    let opts = crate::search::Options {
        case_sensitive: true,
        whole_word: true,
        regex: false,
    };
    let Some(win) = f.src.get(lo..hi) else { return };
    let color = with_alpha(f.theme.gutter_active_fg, 0.16);
    for (s, _) in crate::search::find_all(win, word, &opts) {
        let at = lo + s;
        let line = f.li.line_of(f.src, at);
        if !f.line_visible(line) {
            continue;
        }
        let (x, y) = f.xy(line, f.li.col_of(f.src, at));
        layers.push(rect(
            x,
            y,
            word.chars().count() as f64 * f.advance,
            f.line_px,
            color,
        ));
    }
}

/// The active indent guide: the guide of the innermost fold block containing the caret,
/// drawn brighter over its body lines (VS Code `editor.guides.highlightActiveIndentation`).
pub(crate) fn active_indent_guide(f: &Frame, layers: &mut Vec<AnyWidget>) {
    if !f.p.indent_guides {
        return;
    }
    let regions = f.li.regions();
    let Some((head, last)) = crate::fold::region_at(regions, f.cl) else {
        return;
    };
    let hs = f.li.line_start_of(head);
    let col = f.src[hs..line_end(f.src, hs)]
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .count();
    for line in (head + 1).max(f.first_line)..=last.min(f.last_line) {
        if f.disp.is_hidden(line) {
            continue;
        }
        layers.push(rect(
            f.pad_l + col as f64 * f.advance,
            f.y_of(line),
            1.0,
            f.line_px,
            f.theme.indent_guide_active,
        ));
    }
}

/// Bracket-pair guide (VS Code `editor.guides.bracketPairs: active`): for the bracket pair
/// enclosing the caret, a vertical line at the closing bracket's column across the lines in
/// between, with a horizontal connector on the opener's line.
pub(crate) fn bracket_pair_guide(f: &Frame, layers: &mut Vec<AnyWidget>) {
    if !f.p.bracket_pair_guides {
        return;
    }
    let pairs =
        f.p.language
            .as_ref()
            .map(|l| l.brackets().to_vec())
            .unwrap_or_else(|| crate::brackets::DEFAULT_BRACKETS.to_vec());
    // Bound the scan to the visible window (+ margin) so a huge file stays cheap.
    let lo = f.li.line_start_of(f.first_line.saturating_sub(200));
    let hi = line_end(
        f.src,
        f.li.line_start_of((f.last_line + 200).min(f.line_count - 1)),
    );
    let Some(win) = f.src.get(lo..hi) else { return };
    let Some((o, c)) = crate::ops::enclosing_pair(win, &pairs, f.pcc.saturating_sub(lo)) else {
        return;
    };
    let (o, c) = (lo + o, lo + c);
    let (lo_line, hi_line) = (f.li.line_of(f.src, o), f.li.line_of(f.src, c));
    if hi_line <= lo_line + 1 && hi_line == lo_line {
        return;
    }
    let ccol = f.li.col_of(f.src, c);
    let ocol = f.li.col_of(f.src, o);
    let x = f.pad_l + ccol as f64 * f.advance + f.advance / 2.0;
    for line in (lo_line + 1).max(f.first_line)..hi_line.min(f.last_line + 1) {
        if f.disp.is_hidden(line) {
            continue;
        }
        layers.push(rect(x, f.y_of(line), 1.0, f.line_px, f.theme.bracket_guide));
    }
    if f.line_visible(lo_line) && ocol > ccol {
        let y = f.y_of(lo_line) + f.line_px - 1.0;
        layers.push(rect(
            x,
            y,
            (ocol - ccol) as f64 * f.advance,
            1.0,
            f.theme.bracket_guide,
        ));
    }
}

/// Unicode highlighting: box invisible and ambiguous characters, and draw control characters
/// as their symbols (`␀`, `␛`, …).
pub(crate) fn unicode(f: &Frame, layers: &mut Vec<AnyWidget>) {
    if !f.p.unicode_highlight {
        return;
    }
    for line in f.first_line..=f.last_line {
        if f.disp.is_hidden(line) {
            continue;
        }
        let ls = f.li.line_start_of(line);
        let text = &f.src[ls..line_end(f.src, ls)];
        if text.is_ascii() && !text.bytes().any(|b| b < 0x20 && b != b'\t' || b == 0x7F) {
            continue;
        }
        for (col, ch, kind) in unicode_marks(text) {
            let (x, y) = f.xy(line, col);
            match kind {
                UnicodeKind::Control => layers.push(
                    Positioned::new(
                        text_rich(vec![
                            span(control_symbol(ch).to_string())
                                .size(f.fs as f32)
                                .font_family(f.font_family)
                                .color(f.theme.unicode_highlight),
                        ])
                        .line_height(f.lh as f32),
                    )
                    .left(x)
                    .top(y)
                    .into_widget(),
                ),
                _ => layers.push(outline(
                    x,
                    y + 1.0,
                    f.advance,
                    f.line_px - 2.0,
                    f.theme.unicode_highlight,
                )),
            }
        }
    }
}

/// Underline links (Ctrl/Cmd+click opens them through the host's `on_open_link`).
pub(crate) fn links(f: &Frame, layers: &mut Vec<AnyWidget>) {
    if f.p.on_open_link.is_none() {
        return;
    }
    for line in f.first_line..=f.last_line {
        if f.disp.is_hidden(line) {
            continue;
        }
        let ls = f.li.line_start_of(line);
        let text = &f.src[ls..line_end(f.src, ls)];
        if !text.contains(':') {
            continue;
        }
        for (a, b) in find_links(text) {
            let ca = text[..a].chars().count();
            let n = text[a..b].chars().count();
            let (x, y) = f.xy(line, ca);
            layers.push(rect(
                x,
                y + f.line_px - 3.0,
                n as f64 * f.advance,
                1.0,
                f.theme.syntax.link.color,
            ));
        }
    }
}

/// Colour decorators: a small swatch just before every colour literal (VS Code
/// `editor.colorDecorators`); clicking it opens the picker (see `view::color_picker`).
pub(crate) fn color_swatches(
    f: &Frame,
    layers: &mut Vec<AnyWidget>,
    picker: Signal<Option<usize>>,
) {
    if !f.p.color_decorators {
        return;
    }
    for line in f.first_line..=f.last_line {
        if f.disp.is_hidden(line) {
            continue;
        }
        let ls = f.li.line_start_of(line);
        let text = &f.src[ls..line_end(f.src, ls)];
        if !text.contains('#') && !text.contains("rgb") && !text.contains("hsl") {
            continue;
        }
        for c in find_colors(text) {
            let col = text[..c.start].chars().count();
            let (x, y) = f.xy(line, col);
            let sz = (f.line_px * 0.55).min(f.advance * 1.2);
            let color = Color::from_rgba8(c.rgba[0], c.rgba[1], c.rgba[2], c.rgba[3]);
            let at = ls + c.start;
            let swatch = GestureDetector::new(
                container().width(sz).height(sz).decoration(
                    BoxDecoration::new()
                        .color(color)
                        .border(Border::new(f.theme.border, 1.0)),
                ),
            )
            .cursor(Cursor::Pointer)
            .on_tap(action(move || picker.set(Some(at))));
            layers.push(
                Positioned::new(swatch)
                    .left(x - sz - 1.0)
                    .top(y + (f.line_px - sz) / 2.0)
                    .into_widget(),
            );
        }
    }
}

/// The folded-region badge: `⋯ N lines` after a folded head line; clicking it unfolds.
pub(crate) fn fold_badges(f: &Frame, layers: &mut Vec<AnyWidget>) {
    let folded = f.folds.get();
    if folded.is_empty() {
        return;
    }
    let regions = f.li.regions();
    for &head in folded.iter() {
        if head < f.first_line || head > f.last_line || f.disp.is_hidden(head) {
            continue;
        }
        let n = crate::fold::hidden_count(regions, head);
        if n == 0 {
            continue;
        }
        let len = f.li.char_lens().get(head).copied().unwrap_or(0);
        let (x, y) = f.xy(head, len);
        let folds = f.folds;
        let badge = GestureDetector::new(
            container()
                .padding(EdgeInsets::symmetric(4.0, 0.0))
                .height(f.line_px - 4.0)
                .alignment(Alignment::CENTER)
                .decoration(
                    BoxDecoration::new()
                        .color(f.theme.fold_badge)
                        .radius(BorderRadius::all(3.0)),
                )
                .child(
                    text(format!("⋯ {n} line{}", if n == 1 { "" } else { "s" }))
                        .size((f.fs * 0.85) as f32)
                        .font_family(f.font_family)
                        .color(f.theme.muted),
                ),
        )
        .cursor(Cursor::Pointer)
        .on_tap(action(move || {
            folds.update(|s| {
                s.remove(&head);
            });
        }));
        layers.push(
            Positioned::new(badge)
                .left(x + f.advance)
                .top(y + 2.0)
                .into_widget(),
        );
    }
}

/// Linked-editing outlines: when the caret is in a markup tag name, outline it and its
/// partner (they are edited together).
pub(crate) fn linked_editing(f: &Frame, layers: &mut Vec<AnyWidget>) {
    if !f.linked_editing_active || f.sels.is_multi() {
        return;
    }
    let Some((a, b)) = crate::scan_text::linked_tag_names(f.src, f.pcc) else {
        return;
    };
    for (s, e) in [a, b] {
        let line = f.li.line_of(f.src, s);
        if !f.line_visible(line) {
            continue;
        }
        let (x, y) = f.xy(line, f.li.col_of(f.src, s));
        let w = f.src[s..e].chars().count().max(1) as f64 * f.advance;
        layers.push(outline(x, y, w, f.line_px, f.theme.linked_editing));
    }
}

/// The colour picker popover for the literal at `color_picker` (opened from a swatch): a hex
/// field plus preset swatches; applying rewrites the literal (one undo step).
pub(crate) fn color_picker(
    f: &Frame,
    layers: &mut Vec<AnyWidget>,
    state: Signal<crate::edit::EditorState>,
    history: Signal<std::rc::Rc<std::cell::RefCell<crate::edit::History>>>,
    code: Signal<String>,
    draft: Signal<String>,
) {
    let picker = f.color_picker;
    let Some(at) = picker.get() else { return };
    let line = f.li.line_of(f.src, at);
    let ls = f.li.line_start_of(line);
    let text = &f.src[ls..line_end(f.src, ls)];
    let Some(lit) = find_colors(text).into_iter().find(|c| ls + c.start == at) else {
        picker.set(None);
        return;
    };
    let (from, to) = (ls + lit.start, ls + lit.end);
    let original = text[lit.start..lit.end].to_string();
    if draft.peek().is_empty() {
        draft.set(crate::scan_text::to_hex(lit.rgba));
    }
    let apply = move |hex: String| {
        // Keep the literal's notation: hex stays hex, functional notations become rgba().
        let parsed = find_colors(&hex).into_iter().next();
        let Some(c) = parsed else { return };
        let new = if original.starts_with('#') {
            crate::scan_text::to_hex(c.rgba)
        } else {
            let [r, g, b, a] = c.rgba;
            if a == 255 {
                format!("rgb({r}, {g}, {b})")
            } else {
                format!("rgba({r}, {g}, {b}, {:.2})", a as f64 / 255.0)
            }
        };
        crate::commands::dispatch(
            state,
            history,
            code,
            crate::edit::Transaction::change(crate::edit::ChangeSet::replace(from, to, new)),
            crate::edit::Coalesce::Never,
        );
        picker.set(None);
        draft.set(String::new());
    };
    let presets: [[u8; 3]; 10] = [
        [0xF4, 0x43, 0x36],
        [0xE9, 0x1E, 0x63],
        [0x9C, 0x27, 0xB0],
        [0x3F, 0x51, 0xB5],
        [0x21, 0x96, 0xF3],
        [0x00, 0x96, 0x88],
        [0x4C, 0xAF, 0x50],
        [0xFF, 0xEB, 0x3B],
        [0xFF, 0x98, 0x00],
        [0x79, 0x55, 0x48],
    ];
    let mut sw: Vec<AnyWidget> = Vec::new();
    for c in presets {
        let hex = crate::scan_text::to_hex([c[0], c[1], c[2], 255]);
        let apply = apply.clone();
        sw.push(
            GestureDetector::new(
                container()
                    .width(16.0)
                    .height(16.0)
                    .margin(EdgeInsets::all(2.0))
                    .decoration(
                        BoxDecoration::new()
                            .color(Color::from_rgba8(c[0], c[1], c[2], 255))
                            .radius(BorderRadius::all(3.0)),
                    ),
            )
            .cursor(Cursor::Pointer)
            .on_tap(action(move || apply(hex.clone())))
            .into_widget(),
        );
    }
    let apply_field = apply.clone();
    let field = text_field()
        .bind(draft)
        .autofocus()
        .on_submit(move |v| apply_field(v.to_string()));
    let current = Color::from_rgba8(lit.rgba[0], lit.rgba[1], lit.rgba[2], lit.rgba[3]);
    let panel = container()
        .width(220.0)
        .padding(EdgeInsets::all(8.0))
        .decoration(
            BoxDecoration::new()
                .color(f.theme.overlay_bg)
                .radius(BorderRadius::all(f.theme.overlay_radius))
                .border(Border::new(f.theme.border, 1.0)),
        )
        .child(
            column(children![
                row(children![
                    container().width(22.0).height(22.0).decoration(
                        BoxDecoration::new()
                            .color(current)
                            .radius(BorderRadius::all(4.0))
                    ),
                    gap_w(6.0),
                    expanded(field),
                    icon_button(IconKind::Close).size(14.0).on_pressed(move || {
                        picker.set(None);
                        draft.set(String::new());
                    }),
                ])
                .cross_axis_alignment(CrossAxisAlignment::Center),
                gap_h(6.0),
                row(sw),
            ])
            .main_axis_size(MainAxisSize::Min),
        );
    let (x, y) = f.xy(line, text[..lit.start].chars().count());
    layers.push(
        Positioned::new(panel)
            .left(x)
            .top(y + f.line_px + 2.0)
            .into_widget(),
    );
}
