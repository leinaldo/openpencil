//! Page-space transforms for the canvas drop flow.
//!
//! Scene bounds live in their parent's frame; paint turns each node by
//! its own flip then rotation about its aggregate centre, and children
//! inherit that. The drop preview, the drop hit-test and the reparent
//! commit replay the same chain so they land where the nodes render.

use crate::layout_scene::SceneNode;
use crate::{Point2D, Rect};
use glam::{DAffine2, DVec2};
use op_editor_core::editor_ui_state::{CanvasOverlayLine, CanvasOverlayRect};

/// The node's own paint transform (`paint_node_inner` order).
pub(super) fn own_transform(node: &SceneNode) -> DAffine2 {
    if !node.flip_x && !node.flip_y && node.rotation.abs() <= f32::EPSILON {
        return DAffine2::IDENTITY;
    }
    let pivot = centre(node.aggregate_bounds());
    let flip = DVec2::new(
        if node.flip_x { -1.0 } else { 1.0 },
        if node.flip_y { -1.0 } else { 1.0 },
    );
    DAffine2::from_translation(pivot)
        * DAffine2::from_scale(flip)
        * DAffine2::from_angle(node.rotation as f64)
        * DAffine2::from_translation(-pivot)
}

/// Own flip as a linear map — the part of `own_transform` a reparent
/// keeps on the node.
pub(super) fn own_flip(node: &SceneNode) -> glam::DMat2 {
    glam::DMat2::from_diagonal(DVec2::new(
        if node.flip_x { -1.0 } else { 1.0 },
        if node.flip_y { -1.0 } else { 1.0 },
    ))
}

pub(super) fn centre(rect: Rect) -> DVec2 {
    DVec2::new(
        (rect.origin.x + rect.size.x / 2.0) as f64,
        (rect.origin.y + rect.size.y / 2.0) as f64,
    )
}

pub(super) fn to_point(p: DVec2) -> Point2D {
    Point2D::new(p.x as f32, p.y as f32)
}

/// Rendered angle (radians, clockwise) of the local x axis.
pub(super) fn angle(m: glam::DMat2) -> f64 {
    m.x_axis.y.atan2(m.x_axis.x)
}

/// `rect` (in `to_page`'s source frame) as it renders on the page.
pub(super) fn page_overlay_rect(rect: Rect, to_page: DAffine2) -> CanvasOverlayRect {
    let c = to_page.transform_point2(centre(rect));
    let (w, h) = (rect.size.x as f64, rect.size.y as f64);
    CanvasOverlayRect {
        rotation: angle(to_page.matrix2),
        ..CanvasOverlayRect::new(c.x - w / 2.0, c.y - h / 2.0, w, h)
    }
}

pub(super) fn page_overlay_line(line: CanvasOverlayLine, to_page: DAffine2) -> CanvasOverlayLine {
    let a = to_page.transform_point2(DVec2::new(line.x1, line.y1));
    let b = to_page.transform_point2(DVec2::new(line.x2, line.y2));
    CanvasOverlayLine::new(a.x, a.y, b.x, b.y)
}

/// Unrotated `size` rect centred on `c`.
pub(super) fn rect_around(c: DVec2, size: Point2D) -> Rect {
    Rect::xywh(
        c.x as f32 - size.x / 2.0,
        c.y as f32 - size.y / 2.0,
        size.x,
        size.y,
    )
}

/// Own rotation (degrees) that keeps `node`'s rendered angle — currently
/// `node_to_page` — once its parent frame becomes `frame_to_page`.
pub(super) fn own_rotation_in(
    node: &SceneNode,
    node_to_page: DAffine2,
    frame_to_page: DAffine2,
) -> f64 {
    // ponytail: a frame whose flip parity differs from the source can't be
    // matched by rotation alone; the x axis is kept and the node mirrors.
    let m = own_flip(node) * frame_to_page.matrix2.inverse() * node_to_page.matrix2;
    // Scene angles are f32; round off the round-trip noise.
    (angle(m).to_degrees() * 1e4).round() / 1e4
}
