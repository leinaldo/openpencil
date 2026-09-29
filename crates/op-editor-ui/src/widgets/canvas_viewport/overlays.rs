//! Canvas drop-target preview and the hand-segmented dashed-rect helper
//! used by the viewport overlays.
//!
//! Split out of `canvas_viewport.rs` to keep that spine under the
//! repository's 800-line cap.

use crate::theme::Theme;
use crate::widgets::PaintCx;
use crate::{Color, Point2D, Rect};
use op_editor_core::Viewport as DocViewport;
pub(super) fn paint_drop_indicator(
    cx: &mut PaintCx<'_>,
    canvas_rect: Rect,
    viewport: &DocViewport,
    theme: &Theme,
    indicator: &op_editor_core::editor_ui_state::CanvasDropIndicator,
    paint_ghost: bool,
) {
    let to_screen_rect = |r: op_editor_core::editor_ui_state::CanvasOverlayRect| {
        Rect::xywh(
            canvas_rect.origin.x + viewport.pan_x + r.x as f32 * viewport.zoom,
            canvas_rect.origin.y + viewport.pan_y + r.y as f32 * viewport.zoom,
            r.w as f32 * viewport.zoom,
            r.h as f32 * viewport.zoom,
        )
    };
    let primary = theme.primary;
    if let Some(target) = indicator.target {
        let rect = to_screen_rect(target);
        let fill = Color { a: 0.08, ..primary };
        let stroke = Color { a: 0.45, ..primary };
        let rotated = rotate_about_centre(cx, rect, target.rotation);
        cx.backend.fill_rect(rect, fill);
        cx.backend.stroke_rect(rect, stroke, 1.0);
        if rotated {
            cx.backend.restore();
        }
    }
    if paint_ghost {
        let ghost = to_screen_rect(indicator.ghost);
        let ghost_fill = Color { a: 0.10, ..primary };
        let ghost_stroke = Color { a: 0.85, ..primary };
        let rotated = rotate_about_centre(cx, ghost, indicator.ghost.rotation);
        cx.backend.fill_rect(ghost, ghost_fill);
        paint_dashed_rect(cx, ghost, ghost_stroke, 1.25);
        if rotated {
            cx.backend.restore();
        }
    }
    if let Some(line) = indicator.insertion {
        let from = Point2D::new(
            canvas_rect.origin.x + viewport.pan_x + line.x1 as f32 * viewport.zoom,
            canvas_rect.origin.y + viewport.pan_y + line.y1 as f32 * viewport.zoom,
        );
        let to = Point2D::new(
            canvas_rect.origin.x + viewport.pan_x + line.x2 as f32 * viewport.zoom,
            canvas_rect.origin.y + viewport.pan_y + line.y2 as f32 * viewport.zoom,
        );
        cx.backend.stroke_line(from, to, primary, 2.0);
    }
}

/// Push a rotation about `rect`'s centre; the caller restores when true.
fn rotate_about_centre(cx: &mut PaintCx<'_>, rect: Rect, radians: f64) -> bool {
    if radians.abs() <= f64::from(f32::EPSILON) {
        return false;
    }
    cx.backend.save();
    cx.backend.rotate(
        radians as f32,
        Point2D::new(
            rect.origin.x + rect.size.x / 2.0,
            rect.origin.y + rect.size.y / 2.0,
        ),
    );
    true
}

/// Stroke a dashed rectangle as 4 dashed edges (4 px on / 4 px off,
/// screen-space) — the `RenderBackend` trait has no path-effect
/// surface, so the dash is segmented by hand; backend-agnostic.
pub(crate) fn paint_dashed_rect(cx: &mut PaintCx<'_>, rect: Rect, color: Color, width: f32) {
    const ON: f32 = 4.0;
    const OFF: f32 = 4.0;
    let mut dash_line = |from: Point2D, to: Point2D| {
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        let len = (dx * dx + dy * dy).sqrt();
        if len <= f32::EPSILON {
            return;
        }
        let (ux, uy) = (dx / len, dy / len);
        let mut t = 0.0;
        while t < len {
            let end = (t + ON).min(len);
            cx.backend.stroke_line(
                Point2D::new(from.x + ux * t, from.y + uy * t),
                Point2D::new(from.x + ux * end, from.y + uy * end),
                color,
                width,
            );
            t = end + OFF;
        }
    };
    let tl = rect.origin;
    let tr = Point2D::new(rect.origin.x + rect.size.x, rect.origin.y);
    let br = Point2D::new(rect.origin.x + rect.size.x, rect.origin.y + rect.size.y);
    let bl = Point2D::new(rect.origin.x, rect.origin.y + rect.size.y);
    dash_line(tl, tr);
    dash_line(tr, br);
    dash_line(br, bl);
    dash_line(bl, tl);
}
