//! The editor's scrollbars (VS Code `editor.scrollbar`).
//!
//! ONE vertical bar, drawn here at the editor's far right edge (right of the minimap when it
//! is on), with the overview ruler's marks painted in its track behind the thumb — VS Code's
//! layout. The vertical scroll view's own bar is hidden: it sat at the text's right edge
//! (left of the minimap), so two bars drove one offset. The bar is a sibling of the text, so
//! the text reserves no strip for it. The horizontal bar is drawn here too: the horizontally scrolling text is the
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

/// State a bar keeps across renders.
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

/// Painter for the overview-ruler marks drawn in the vertical bar's track.
pub(crate) type TrackPainter = Box<dyn Fn(&mut Canvas<'_>)>;

/// The vertical bar (VS Code layout: the editor's right edge) for a `view_h` px viewport over
/// `content_h` px of scroll extent at `scroll_top`, driving `vscroll`. `track` paints the
/// overview ruler behind the thumb. Same desktop semantics as the horizontal bar.
#[allow(clippy::too_many_arguments)]
pub(crate) fn vertical(
    theme: &EditorTheme,
    vscroll: ScrollHandle,
    scroll_top: Signal<f64>,
    content_h: f64,
    view_h: f64,
    st: HBarState,
    track: Option<TrackPainter>,
    background: Color,
) -> AnyWidget {
    let sb = style(theme);
    let content_h = content_h.max(view_h);
    let overflow = content_h > view_h + 0.5;
    let off = scroll_top.get();
    let (_, _, start, len) = sb.geometry(view_h, content_h, off);
    let color = if st.grab.get().is_some() {
        sb.thumb_active_color
    } else if st.hover.get() {
        sb.thumb_hover_color
    } else {
        sb.thumb_color
    };
    let painter = move |cv: &mut Canvas<'_>| {
        let w = cv.size().width;
        if overflow {
            cv.fill_rect(
                Rect::new(sb.margin, start, w - sb.margin, start + len),
                color,
            );
        }
    };
    // The overview ruler is the track: its own canvas behind the thumb's.
    let thumb = canvas(painter).width(SIZE).height(view_h).into_widget();
    let face: AnyWidget = match track {
        Some(t) => stack(vec![
            canvas(move |cv: &mut Canvas<'_>| t(cv))
                .width(SIZE)
                .height(view_h)
                .into_widget(),
            thumb,
        ])
        .into_widget(),
        None => thumb,
    };
    let apply = std::rc::Rc::new(move |y: f64| {
        if !overflow {
            return;
        }
        let grab = st.grab.peek().unwrap_or(len / 2.0);
        let t = sb.offset_for_thumb(view_h, content_h, y - grab);
        vscroll.scroll_to(t);
        scroll_top.set(t);
    });
    let a0 = apply.clone();
    let press = std::rc::Rc::new(move |y: f64| {
        let (_, _, s, l) = sb.geometry(view_h, content_h, scroll_top.peek());
        st.grab
            .set(Some(if y >= s && y <= s + l { y - s } else { l / 2.0 }));
        a0(y);
    });
    let (p1, p2, a1, a2) = (press.clone(), press, apply.clone(), apply);
    GestureDetector::new(
        container()
            .width(SIZE)
            .height(view_h)
            .decoration(BoxDecoration::new().color(background))
            .child(face),
    )
    .on_pointer_down(action_event(move |e| p1(e.position.y)))
    .on_pan_start(action_event(move |e| {
        if st.grab.peek().is_none() {
            p2(e.position.y);
        }
    }))
    .on_pan_update(action_event(move |e| a1(e.position.y)))
    // Captured, no slop (Flutter `Listener.onPointerMove`): the thumb follows the pointer
    // from the first pixel, even outside the bar. Positions are absolute, so the pan update
    // repeating it is harmless.
    .on_pointer_move(action_event(move |e| {
        if st.grab.peek().is_some() {
            a2(e.position.y);
        }
    }))
    .on_pan_end(action_event(move |_| st.grab.set(None)))
    .on_pointer_up(action_event(move |_| st.grab.set(None)))
    .on_hover_move(action_event(move |_| {
        if !st.hover.peek() {
            st.hover.set(true);
        }
    }))
    .on_hover_exit(action(move || st.hover.set(false)))
    .into_widget()
}
