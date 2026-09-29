//! Page-space transforms of scene nodes.
//!
//! Scene bounds live in their parent's frame; paint turns each node by
//! its own flip then rotation about its aggregate centre, and children
//! inherit that. Canvas logic that must land where nodes render — the
//! drop flow, selection-handle hit-tests and resizes — replays the same
//! chain.

use crate::layout_scene::{LayoutScene, SceneNode};
use crate::{Point2D, Rect};
use glam::{DAffine2, DVec2};
use op_editor_core::editor_ui_state::{CanvasOverlayLine, CanvasOverlayRect};

/// The node's own paint transform (`paint_node_inner` order).
pub(crate) fn own_transform(node: &SceneNode) -> DAffine2 {
    own_transform_about(node, centre(node.aggregate_bounds()))
}

/// `node`'s own flip / rotation about an arbitrary pivot — a resize
/// moves the centre the node turns about.
pub(crate) fn own_transform_about(node: &SceneNode, pivot: DVec2) -> DAffine2 {
    if !node.flip_x && !node.flip_y && node.rotation.abs() <= f32::EPSILON {
        return DAffine2::IDENTITY;
    }
    DAffine2::from_translation(pivot)
        * DAffine2::from_mat2(own_linear(node))
        * DAffine2::from_translation(-pivot)
}

/// Linear part of the own transform: flip after rotation.
pub(crate) fn own_linear(node: &SceneNode) -> glam::DMat2 {
    own_flip(node) * glam::DMat2::from_angle(node.rotation as f64)
}

/// Own flip as a linear map — the part of `own_transform` a reparent
/// keeps on the node.
pub(super) fn own_flip(node: &SceneNode) -> glam::DMat2 {
    glam::DMat2::from_diagonal(DVec2::new(
        if node.flip_x { -1.0 } else { 1.0 },
        if node.flip_y { -1.0 } else { 1.0 },
    ))
}

/// The node `id` plus the transform from its parent's frame to the page.
pub(crate) fn find_with_parent_to_page<'a>(
    nodes: &'a [SceneNode],
    id: &str,
) -> Option<(&'a SceneNode, DAffine2)> {
    fn walk<'a>(
        nodes: &'a [SceneNode],
        id: &str,
        to_page: DAffine2,
    ) -> Option<(&'a SceneNode, DAffine2)> {
        for node in nodes {
            if node.id == id {
                return Some((node, to_page));
            }
            if let Some(found) = walk(&node.children, id, to_page * own_transform(node)) {
                return Some(found);
            }
        }
        None
    }
    walk(nodes, id, DAffine2::IDENTITY)
}

/// Transform from node `id`'s own frame (where its bounds, anchors and
/// glyphs are laid out) to the page, on the active page.
fn node_to_page(scene: &LayoutScene, id: &str) -> Option<DAffine2> {
    let (node, parent_to_page) = find_with_parent_to_page(&scene.active_page()?.children, id)?;
    Some(parent_to_page * own_transform(node))
}

/// Where a point laid out in node `id`'s own frame renders on the page.
pub fn node_point_on_page(scene: &LayoutScene, id: &str, p: Point2D) -> Option<Point2D> {
    let q = node_to_page(scene, id)?.transform_point2(DVec2::new(p.x as f64, p.y as f64));
    Some(to_point(q))
}

/// A page point in node `id`'s own frame — undoing its own and every
/// ancestor's flip / rotation, the inverse of [`node_point_on_page`].
pub fn page_point_in_node(scene: &LayoutScene, id: &str, p: Point2D) -> Option<Point2D> {
    let q = node_to_page(scene, id)?
        .inverse()
        .transform_point2(DVec2::new(p.x as f64, p.y as f64));
    Some(to_point(q))
}

/// Whether node `id` renders mirrored (an odd number of flips on it and
/// its ancestors): its own rotation then turns the other way on screen.
pub fn node_renders_mirrored(scene: &LayoutScene, id: &str) -> bool {
    node_to_page(scene, id).is_some_and(|t| t.matrix2.determinant() < 0.0)
}

pub(crate) fn centre(rect: Rect) -> DVec2 {
    DVec2::new(
        (rect.origin.x + rect.size.x / 2.0) as f64,
        (rect.origin.y + rect.size.y / 2.0) as f64,
    )
}

pub(crate) fn to_point(p: DVec2) -> Point2D {
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
