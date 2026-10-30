//! Inlay hints and code lenses from the language service.
//!
//! Inlay hints are requested for the visible line range and re-requested when the viewport or
//! the document version changes; they render as compact labels at their position (padding
//! honoured), each label part clickable (reveal its location or run its command; a hint with
//! `data` and nothing to act on is resolved first) with its tooltip. Code lenses are requested
//! per document version, unresolved ones resolved, and drawn on a row above their line;
//! clicking one runs its command through the host. Both have toggle commands.
//!
//! Hints occupy space like VS Code's injected text: each reserves whole grid columns (see
//! [`slots`]) in the display map, so the rest of the line shifts right and the caret,
//! selection, hit-testing and soft wrap all map around them.

use std::rc::Rc;

use pebbles::prelude::*;

use crate::language::{self as lsp};
use crate::view::Frame;
use crate::view::lsp::Lsp;

/// Where one hint sits: `(line, col, width_cols, x_offset_cols)` — the offset stacks hints
/// that share a column.
pub(crate) type Slot = (usize, usize, usize, usize);

/// Every hint's reserved slot, for both the host-provided and the language-service hints
/// (same order as their source lists), plus the display map's per-line widths.
#[derive(Default)]
pub(crate) struct InlaySlots {
    pub(crate) props: Vec<Option<Slot>>,
    pub(crate) lsp: Vec<Option<Slot>>,
    pub(crate) map: Rc<crate::fold::Inlays>,
}

/// The whole grid columns a hint label of `chars` chars occupies (drawn at 0.9 of the font
/// size inside a chip with 2px side padding), plus half a column per LSP padding side.
pub(crate) fn hint_cols(chars: usize, pads: usize) -> usize {
    // Chars render at 0.9x the cell; 4px of chip padding is ~0.5 col at common sizes.
    let w = chars as f64 * 0.9 + 0.5 + pads as f64 * 0.5;
    (w.ceil() as usize).max(1)
}

/// Lay out every hint (host hints first, then language-service hints, stable per column).
pub(crate) fn slots(
    src: &str,
    li: &crate::lines::LineIndex,
    props: &[crate::providers::InlayHint],
    hints: &[lsp::InlayHint],
) -> InlaySlots {
    let mut map = crate::fold::Inlays::new();
    let mut used: std::collections::HashMap<(usize, usize), usize> = Default::default();
    let mut place = |at: usize, w: usize| -> Slot {
        let at = at.min(src.len());
        let line = li.line_of(src, at);
        let col = li.col_of(src, at);
        let off = used.entry((line, col)).or_insert(0);
        let slot = (line, col, w, *off);
        *off += w;
        map.entry(line).or_default().push((col, w));
        slot
    };
    let props = props
        .iter()
        .map(|h| Some(place(h.at, hint_cols(h.label.chars().count(), 0))))
        .collect();
    let lsp = hints
        .iter()
        .map(|h| {
            let chars: usize = h.parts().iter().map(|p| p.value.chars().count()).sum();
            let pads = h.padding_left as usize + h.padding_right as usize;
            Some(place(
                lsp::position_to_byte(src, h.position),
                hint_cols(chars, pads),
            ))
        })
        .collect();
    for v in map.values_mut() {
        v.sort_by_key(|(c, _)| *c);
    }
    InlaySlots {
        props,
        lsp,
        map: Rc::new(map),
    }
}

impl Lsp {
    /// Whether inlay hints are shown (the toggle over the configured default).
    pub(crate) fn inlays_on(&self) -> bool {
        self.h.inlay_on.get().unwrap_or(self.inlay_default)
    }
    /// Whether code lenses are shown.
    pub(crate) fn lenses_on(&self) -> bool {
        self.h.lens_on.get().unwrap_or(self.lens_default)
    }
    /// Toggle inlay hints.
    pub(crate) fn toggle_inlays(&self) -> bool {
        let on = self.h.inlay_on.peek().unwrap_or(self.inlay_default);
        self.h.inlay_on.set(Some(!on));
        self.store.inlay_key.set(None);
        true
    }
    /// Toggle code lenses.
    pub(crate) fn toggle_lenses(&self) -> bool {
        let on = self.h.lens_on.peek().unwrap_or(self.lens_default);
        self.h.lens_on.set(Some(!on));
        self.store.lens_ver.set(None);
        true
    }

    /// Ask for the inlay hints of lines `first..=last` when the range or the document changed.
    pub(crate) fn request_inlays(&self, first: usize, last: usize) {
        let Some(svc) = self.svc.clone() else { return };
        if !self.h.inlay_on.peek().unwrap_or(self.inlay_default) {
            return;
        }
        let key = (self.version(), first, last);
        if self.store.inlay_key.get() == Some(key) {
            return;
        }
        self.store.inlay_key.set(Some(key));
        let sig = self.h.inlays;
        let reply = self.guard(&self.store.inlay_gate, move |v: Vec<lsp::InlayHint>| {
            sig.set(Rc::new(v));
        });
        let range = lsp::Range::new(
            lsp::Position::new(first as u32, 0),
            lsp::Position::new(last as u32 + 1, 0),
        );
        self.with_doc(|doc, _| svc.inlay_hints(doc, range, reply));
    }

    /// Ask for the code lenses when the document changed; unresolved lenses are resolved.
    pub(crate) fn request_lenses(&self) {
        let Some(svc) = self.svc.clone() else { return };
        if !self.h.lens_on.peek().unwrap_or(self.lens_default) {
            return;
        }
        let ver = self.version();
        if self.store.lens_ver.get() == Some(ver) {
            return;
        }
        self.store.lens_ver.set(Some(ver));
        let me = self.clone();
        let reply = self.guard(&self.store.lens_gate, move |v: Vec<lsp::CodeLens>| {
            me.h.lenses.set(Rc::new(v.clone()));
            let Some(svc) = me.svc.clone() else { return };
            for (i, lens) in v.into_iter().enumerate() {
                if lens.command.is_some() {
                    continue;
                }
                let me2 = me.clone();
                let at = me.version();
                svc.resolve_code_lens(
                    lens,
                    Box::new(move |l: lsp::CodeLens| {
                        if me2.version() != at {
                            return;
                        }
                        let mut cur = (*me2.h.lenses.peek()).clone();
                        if let Some(slot) = cur.get_mut(i) {
                            *slot = l;
                            me2.h.lenses.set(Rc::new(cur));
                        }
                    }),
                );
            }
        });
        self.with_doc(|doc, _| svc.code_lens(doc, reply));
    }

    /// Act on a clicked label part: reveal its location / run its command; when the hint
    /// carries neither, resolve it first.
    fn click_part(&self, hint: lsp::InlayHint, part: usize) {
        let act = move |me: &Lsp, h: &lsp::InlayHint| {
            let Some(p) = h.parts().get(part).cloned() else {
                return;
            };
            if let Some(loc) = &p.location {
                me.go_to(loc);
            }
            if let (Some(c), Some(s)) = (&p.command, &me.svc) {
                s.execute_command(c);
            }
        };
        let parts = hint.parts();
        let has = parts
            .get(part)
            .is_some_and(|p| p.location.is_some() || p.command.is_some());
        if has || hint.data.is_none() {
            act(self, &hint);
            return;
        }
        let Some(svc) = self.svc.clone() else { return };
        let me = self.clone();
        let at = self.version();
        svc.resolve_inlay_hint(
            hint,
            Box::new(move |h: lsp::InlayHint| {
                if me.version() == at {
                    act(&me, &h);
                }
            }),
        );
    }

    /// The inlay-hint labels in the visible window.
    pub(crate) fn inlay_layers(&self, f: &Frame, layers: &mut Vec<AnyWidget>) {
        if !self.inlays_on() {
            return;
        }
        let hints = self.h.inlays.get();
        let fs = f.fs * 0.9;
        for (k, hint) in hints.iter().enumerate() {
            let Some(Some((line, col, w, off))) = f.inlay_slots.lsp.get(k).copied() else {
                continue;
            };
            if !f.line_visible(line) {
                continue;
            }
            let (x, y) = f.xy(line, col);
            let x = x + off as f64 * f.advance;
            let mut parts: Vec<AnyWidget> = Vec::new();
            for (i, p) in hint.parts().into_iter().enumerate() {
                let label = text(p.value.clone())
                    .size(fs as f32)
                    .line_height(f.lh as f32)
                    .font_family(f.font_family)
                    .color(f.theme.muted);
                let me = self.clone();
                let h = hint.clone();
                let tip = p
                    .tooltip
                    .as_ref()
                    .or(hint.tooltip.as_ref())
                    .map(|t| t.value.clone());
                let clickable = GestureDetector::new(label)
                    .cursor(Cursor::Pointer)
                    .on_tap(action(move || me.click_part(h.clone(), i)));
                let w = semantics(
                    SemanticsRole::Button,
                    format!("Inlay hint {}", p.value),
                    clickable,
                )
                .into_widget();
                parts.push(match tip {
                    Some(t) => tooltip(t, w).into_widget(),
                    None => w,
                });
            }
            let pad = f.advance * 0.5;
            let chip = container()
                .width(w as f64 * f.advance)
                .padding(EdgeInsets::only(
                    if hint.padding_left { pad } else { 0.0 },
                    0.0,
                    if hint.padding_right { pad } else { 0.0 },
                    0.0,
                ))
                .child(
                    container()
                        .padding(EdgeInsets::symmetric(2.0, 0.0))
                        .decoration(
                            BoxDecoration::new()
                                .color(crate::view::chrome::with_alpha(f.theme.muted, 0.12))
                                .radius(BorderRadius::all(3.0)),
                        )
                        .child(row(parts)),
                );
            layers.push(Positioned::new(chip).left(x).top(y).into_widget());
        }
    }

    /// The code lenses grouped by line, as (line, widget) rows.
    pub(crate) fn lens_rows(
        &self,
        t: &crate::theme::EditorTheme,
        fs: f64,
        src: &str,
    ) -> Vec<(usize, AnyWidget)> {
        if !self.lenses_on() {
            return Vec::new();
        }
        let lenses = self.h.lenses.get();
        let mut by_line: std::collections::BTreeMap<usize, Vec<lsp::CodeLens>> = Default::default();
        for l in lenses.iter() {
            let at = lsp::position_to_byte(src, l.range.start);
            by_line
                .entry(crate::geometry::line_of(src, at))
                .or_default()
                .push(l.clone());
        }
        let mut out = Vec::new();
        for (line, ls) in by_line {
            // Indent the lens to its line's indentation, as VS Code does.
            let ls_start = crate::geometry::line_start_of(src, line);
            let indent = src[ls_start..]
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .count();
            let mut items: Vec<AnyWidget> = vec![gap_w(indent as f64 * fs * 0.6).into_widget()];
            let resolved: Vec<_> = ls.into_iter().filter_map(|l| l.command).collect();
            if resolved.is_empty() {
                continue;
            }
            for (i, c) in resolved.into_iter().enumerate() {
                if i > 0 {
                    items.push(
                        text(" | ")
                            .size((fs * 0.85) as f32)
                            .color(t.muted)
                            .into_widget(),
                    );
                }
                let me = self.clone();
                let title = c.title.clone();
                items.push(
                    semantics(
                        SemanticsRole::Button,
                        format!("Code lens {title}"),
                        GestureDetector::new(
                            text(title.clone()).size((fs * 0.85) as f32).color(t.muted),
                        )
                        .cursor(Cursor::Pointer)
                        .on_tap(action(move || {
                            if let Some(s) = &me.svc {
                                s.execute_command(&c);
                            }
                        })),
                    )
                    .into_widget(),
                );
            }
            out.push((
                line,
                row(items)
                    .cross_axis_alignment(CrossAxisAlignment::Center)
                    .into_widget(),
            ));
        }
        out
    }
}
