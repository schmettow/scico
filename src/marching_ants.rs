//! The animated frame around passages marked for the next question.
//!
//! The editor owns the text layout; [`EditorState::range_to_bounds`] turns a
//! marked byte range into window coordinates. The overlay subtracts its own
//! origin and paints a moving dashed rectangle on top of the editor, so the
//! frame follows scrolling, wrapping, and edits without touching the editor.

use std::{ops::Range, time::Duration};

use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, Bounds, ElementId, Entity, Hsla, IntoElement,
    ParentElement as _, Pixels, Point, Styled as _, Window, canvas, component::input::EditorState,
    fill, point, px, size,
};

/// Length of one moving dash, in pixels.
const DASH: f32 = 6.0;
/// Gap between dashes, in pixels.
const GAP: f32 = 4.0;
/// One dash plus one gap.
const PERIOD: f32 = DASH + GAP;
/// Thickness of the frame, in pixels.
const THICKNESS: f32 = 2.0;
/// Padding between the selection bounds and the frame.
const PADDING: f32 = 3.0;

/// An absolutely positioned overlay drawing a marching-ant frame around each
/// marked byte `range`.
///
/// Place it in a relative container over the editor. Ranges whose lines are
/// scrolled out return no bounds and are skipped. The animation stops when the
/// caller stops rendering the overlay (the marked ranges are consumed by the
/// next prompt).
pub fn marked_section_overlay(
    editor: Entity<EditorState>,
    ranges: Vec<Range<usize>>,
    color: Hsla,
) -> AnyElement {
    gpui_kit::div()
        .absolute()
        .inset_0()
        .with_animation(
            ElementId::Name("marked-section-ants".into()),
            Animation::new(Duration::from_millis(650)).repeat(),
            move |this, delta| {
                let editor = editor.clone();
                let ranges = ranges.clone();
                this.child(
                    canvas(
                        |_, _, _| (),
                        move |bounds, _, window, cx| {
                            for range in &ranges {
                                let Some(selection) = editor.read(cx).range_to_bounds(range) else {
                                    continue;
                                };
                                let frame = (selection - bounds.origin).dilate(px(PADDING));
                                paint_frame(window, frame, color, delta);
                            }
                        },
                    )
                    .size_full(),
                )
            },
        )
        .into_any_element()
}

fn paint_frame(window: &mut Window, frame: Bounds<Pixels>, color: Hsla, phase: f32) {
    window.paint_quad(fill(frame, color.opacity(0.04)));

    let width = f32::from(frame.size.width);
    let height = f32::from(frame.size.height);
    let offset = phase * PERIOD;
    let right = frame.origin.x + frame.size.width;
    let bottom = frame.origin.y + frame.size.height;

    paint_edge(window, frame.origin, true, false, width, offset, color);
    paint_edge(
        window,
        point(right, frame.origin.y),
        false,
        false,
        height,
        offset + width,
        color,
    );
    paint_edge(
        window,
        point(right, bottom),
        true,
        true,
        width,
        offset + width + height,
        color,
    );
    paint_edge(
        window,
        point(frame.origin.x, bottom),
        false,
        true,
        height,
        offset + 2.0 * width + height,
        color,
    );
}

/// Paint dashes along one edge. `reverse` draws from the opposite corner so
/// the pattern travels around the rectangle in one direction.
fn paint_edge(
    window: &mut Window,
    origin: Point<Pixels>,
    horizontal: bool,
    reverse: bool,
    length: f32,
    offset: f32,
    color: Hsla,
) {
    for (from, to) in segments(length, offset) {
        let span = to - from;
        let position = if reverse { length - to } else { from };
        let (origin, size) = if horizontal {
            (
                point(origin.x + px(position), origin.y),
                size(px(span), px(THICKNESS)),
            )
        } else {
            (
                point(origin.x, origin.y + px(position)),
                size(px(THICKNESS), px(span)),
            )
        };
        window.paint_quad(fill(Bounds::new(origin, size), color));
    }
}

/// The visible dash spans on an edge of `length`, shifted by `offset`.
fn segments(length: f32, offset: f32) -> impl Iterator<Item = (f32, f32)> {
    let mut start = -offset.rem_euclid(PERIOD);
    std::iter::from_fn(move || {
        if start >= length {
            return None;
        }
        let from = start.max(0.0);
        let to = (start + DASH).min(length);
        start += PERIOD;
        (to > from).then_some((from, to))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashes_stay_within_the_edge_and_advance() {
        let first: Vec<_> = segments(30.0, 0.0).collect();
        assert!(!first.is_empty());
        assert!(
            first
                .iter()
                .all(|(from, to)| *from >= 0.0 && *to <= 30.0 && from < to)
        );

        let moved: Vec<_> = segments(30.0, PERIOD / 2.0).collect();
        assert_ne!(first, moved, "a phase shift must move the dashes");
    }

    #[test]
    fn an_empty_edge_paints_nothing() {
        assert_eq!(segments(0.0, 3.0).count(), 0);
    }
}
