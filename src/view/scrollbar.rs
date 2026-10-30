//! The editor's scrollbars (VS Code `editor.scrollbar`).
//!
//! The vertical bar is the Pebbles scroll view's own bar, styled from the theme; the text
//! keeps clear of its strip ([`SIZE`]) so it never paints under the bar and never loses a
//! click to it. The horizontal bar is drawn here: the horizontally scrolling text is the
//! CONTENT of the vertical viewport, so a scroll-view bar would sit under the last line of
//! the document instead of at the bottom of the viewport. This one is pinned to the
//! viewport's bottom edge and drives the horizontal scroll handle, with the same geometry
//! and desktop semantics as the Pebbles bar: grabbing the thumb never jumps it, a track
//! click centres the thumb there, a drag keeps following outside the bar, hover/active
//! colours.

use pebbles::prelude::*;
use pebbles::render::{ScrollHandle, ScrollbarStyle};

use crate::theme::EditorTheme;

/// The strip either bar occupies across its axis (VS Code's 14 px scrollbar).
pub(crate) const SIZE: f64 = 14.0;

/// The bar style from the theme (`scrollbarSlider.*` colours, square ends).
pub(crate) fn style(theme: &EditorTheme) -> ScrollbarStyle {
    ScrollbarStyle {
        thickness: SIZE - 4.0,
        margin: 2.0,
        min_thumb: 20.0,
        thumb_color: theme.scrollbar,
        thumb_hover_color: theme.scrollbar_active,
        thumb_active_color: theme.scrollbar_active,
        track_color: pebbles::foundation::palette::TRANSPARENT,
        radius: 0.0,
        ..ScrollbarStyle::default()
    }
}

/// State the horizontal bar keeps across renders.
#[derive(Clone, Copy)]
pub(crate) struct HBarState {
    /// Pointer over the bar.
    pub(crate) hover: Signal<bool>,
    /// While dragging: where along the thumb it was grabbed.
    pub(crate) grab: Signal<Option<f64>>,
}

/// The horizontal bar for `content_w` px of text in a `view_w` px viewport scrolled to
/// `scroll_left`, or `None` when nothing overflows.
pub(crate) fn horizontal(
    theme: &EditorTheme,
    hscroll: ScrollHandle,
    scroll_left: Signal<f64>,
    content_w: f64,
    view_w: f64,
    st: HBarState,
) -> Option<AnyWidget> {
    if view_w <= 0.0 || content_w <= view_w + 0.5 {
        return None;
    }
    let sb = style(theme);
    let off = scroll_left.get();
    let (_, _, start, len) = sb.geometry(view_w, content_w, off);
    let color = if st.grab.get().is_some() {
        sb.thumb_active_color
    } else if st.hover.get() {
        sb.thumb_hover_color
    } else {
        sb.thumb_color
    };
    let painter = move |cv: &mut Canvas<'_>| {
        let h = cv.size().height;
        cv.fill_rect(
            Rect::new(start, sb.margin, start + len, h - sb.margin),
            color,
        );
    };
    let apply = std::rc::Rc::new(move |x: f64| {
        let grab = st.grab.peek().unwrap_or(len / 2.0);
        let t = sb.offset_for_thumb(view_w, content_w, x - grab);
        hscroll.scroll_to(t);
        scroll_left.set(t);
    });
    let a0 = apply.clone();
    let press = std::rc::Rc::new(move |x: f64| {
        // Measured from the live offset: a press right after a scroll grabs the thumb where
        // it is now, not where this render drew it.
        let (_, _, s, l) = sb.geometry(view_w, content_w, scroll_left.peek());
        st.grab
            .set(Some(if x >= s && x <= s + l { x - s } else { l / 2.0 }));
        a0(x);
    });
    let (p1, p2, a1) = (press.clone(), press, apply);
    Some(
        GestureDetector::new(canvas(painter).height(SIZE))
            .on_pointer_down(action_event(move |e| p1(e.position.x)))
            .on_pan_start(action_event(move |e| {
                if st.grab.peek().is_none() {
                    p2(e.position.x);
                }
            }))
            .on_pan_update(action_event(move |e| a1(e.position.x)))
            .on_pan_end(action_event(move |_| st.grab.set(None)))
            .on_pointer_up(action_event(move |_| st.grab.set(None)))
            .on_hover_move(action_event(move |_| {
                if !st.hover.peek() {
                    st.hover.set(true);
                }
            }))
            .on_hover_exit(action(move || st.hover.set(false)))
            .into_widget(),
    )
}
