//! Selection / rotation / path / arc handle geometry and the host's
//! input hit-tests for [`super::CanvasViewport`].
//!
//! Split out of `canvas_viewport.rs` to keep that spine under the
//! repository's 800-line cap. Every public item is re-exported from the
//! spine so existing `canvas_viewport::…` paths keep resolving.

use super::super::scene_transform::{
    centre, find_with_parent_to_page, own_linear, own_transform, own_transform_about,
};
use crate::layout_scene::LayoutScene;
use crate::layout_scene::NodeKind;
use crate::layout_scene::{SceneAnchor, SceneNode};
use crate::util::resize_bounds;
use crate::{Point2D, Rect};
use glam::{DAffine2, DMat2, DVec2};
use op_editor_core::EditorState;
/// One of the 8 selection handles (corners + edge midpoints) the
/// selection overlay paints. Used by the host to dispatch resize
/// drags: each variant fixes the corresponding edge / corner of
/// the selected bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionHandle {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

impl SelectionHandle {
    /// Authored dimensions changed by this handle.
    pub fn resize_axes(self) -> op_editor_core::drag_mutators::ResizeAxes {
        use op_editor_core::drag_mutators::ResizeAxes;
        match (self.resizes_width(), self.resizes_height()) {
            (true, true) => ResizeAxes::Both,
            (true, false) => ResizeAxes::Width,
            (false, true) => ResizeAxes::Height,
            (false, false) => unreachable!("every selection handle resizes at least one axis"),
        }
    }

    /// Whether this handle authors the selected node's width.
    pub fn resizes_width(self) -> bool {
        !matches!(self, Self::Top | Self::Bottom)
    }

    /// Whether this handle authors the selected node's height.
    pub fn resizes_height(self) -> bool {
        !matches!(self, Self::Left | Self::Right)
    }

    /// Whether dragging this handle moves the selected node's left edge.
    pub fn moves_left_edge(self) -> bool {
        matches!(self, Self::Left | Self::TopLeft | Self::BottomLeft)
    }

    /// Whether dragging this handle moves the selected node's top edge.
    pub fn moves_top_edge(self) -> bool {
        matches!(self, Self::Top | Self::TopLeft | Self::TopRight)
    }
}

/// Radius (screen px) of the rotation ring that sits OUTSIDE the
/// 4 selection corners. Matches the TS `ROTATE_OUTER_RADIUS`.
const ROTATE_OUTER_RADIUS: f32 = 16.0;

/// Rotate `p` by `radians` (clockwise, screen y-down) about `center`.
/// Used by the host to un-rotate a cursor point into a rotated
/// node's local frame before hit-testing its handles.
pub fn rotate_point(p: Point2D, center: Point2D, radians: f32) -> Point2D {
    let (s, c) = radians.sin_cos();
    let dx = p.x - center.x;
    let dy = p.y - center.y;
    Point2D::new(center.x + dx * c - dy * s, center.y + dx * s + dy * c)
}

/// Screen-px offset of a "ghost" handle dot from its anchor when the
/// handle is unset — far enough from the anchor body to grab.
pub const PATH_HANDLE_GHOST_PX: f32 = 26.0;

/// Doc-space positions of a path anchor's incoming + outgoing bezier
/// control handles. An unset handle is given a "ghost" position
/// offset from the anchor (scaled to `zoom`) so the user can grab it
/// to create the handle. Returns `(handle_in, handle_out)`. Shared by
/// the overlay painter and the host's handle hit-test.
pub fn path_handle_positions(anchor: &SceneAnchor, zoom: f32) -> (Point2D, Point2D) {
    let ghost = PATH_HANDLE_GHOST_PX / zoom.max(0.0001);
    let hin = anchor
        .handle_in
        .unwrap_or(Point2D::new(anchor.pos.x - ghost, anchor.pos.y));
    let hout = anchor
        .handle_out
        .unwrap_or(Point2D::new(anchor.pos.x + ghost, anchor.pos.y));
    (hin, hout)
}

/// The three arc-edit handles on a selected Ellipse — start angle,
/// sweep (end) angle, and the donut inner-radius.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArcHandle {
    /// Perimeter handle at the arc's start angle.
    Start,
    /// Perimeter handle at the arc's end angle (start + sweep).
    Sweep,
    /// Radial handle controlling the donut inner-radius fraction.
    Inner,
}

/// Doc-space positions of the three arc handles for an Ellipse
/// `SceneNode`. `None` for non-Ellipse kinds or a zero-size node.
/// Shared by the overlay painter and the host's arc-handle hit-test
/// so both agree on handle placement.
pub fn arc_handle_positions(node: &SceneNode) -> Option<[(ArcHandle, Point2D); 3]> {
    if !matches!(node.kind, NodeKind::Ellipse) {
        return None;
    }
    let b = node.bounds;
    if b.size.x <= 0.0 || b.size.y <= 0.0 {
        return None;
    }
    let cx = b.origin.x + b.size.x / 2.0;
    let cy = b.origin.y + b.size.y / 2.0;
    let rx = b.size.x / 2.0;
    let ry = b.size.y / 2.0;
    let start = node.arc_start_angle.unwrap_or(0.0);
    let sweep = node.arc_sweep_angle.unwrap_or(360.0);
    let inner = node.arc_inner_radius.unwrap_or(0.0).clamp(0.0, 1.0);
    let at = |deg: f32, scale: f32| -> Point2D {
        let a = deg.to_radians();
        Point2D::new(cx + rx * scale * a.cos(), cy + ry * scale * a.sin())
    };
    Some([
        (ArcHandle::Start, at(start, 1.0)),
        (ArcHandle::Sweep, at(start + sweep, 1.0)),
        (ArcHandle::Inner, at(start, inner)),
    ])
}

/// The single selected node with its unrotated bounds and the
/// transform from those bounds' frame to the page (its own and its
/// ancestors' flips / rotations) — how its handles render.
struct SelectedFrame {
    bounds: Rect,
    to_page: DAffine2,
}

/// `None` unless the selection is one resolved node with real area.
/// Shared by the handle hit-tests below — they only fire on
/// single-select.
fn selected_frame(scene: &LayoutScene, state: &EditorState) -> Option<SelectedFrame> {
    if state.selection_count() != 1 {
        return None;
    }
    let (node, parent_to_page) = find_with_parent_to_page(
        &scene.active_page()?.children,
        state.selection.anchor.as_str(),
    )?;
    let bounds = node.aggregate_bounds();
    if bounds.size.x <= 0.0 || bounds.size.y <= 0.0 {
        return None;
    }
    Some(SelectedFrame {
        bounds,
        to_page: parent_to_page * own_transform(node),
    })
}

/// Screen `point` in the selected node's unrotated document frame.
fn local_doc_point(
    frame: &SelectedFrame,
    canvas_rect: Rect,
    state: &EditorState,
    point: Point2D,
) -> Point2D {
    let doc = state.viewport.to_document(Point2D::new(
        point.x - canvas_rect.origin.x,
        point.y - canvas_rect.origin.y,
    ));
    let local = frame
        .to_page
        .inverse()
        .transform_point2(DVec2::new(doc.x as f64, doc.y as f64));
    Point2D::new(local.x as f32, local.y as f32)
}

/// Hit-test the rotation ring that sits just outside the four
/// corner handles. Returns the nearest corner (so the runner can
/// hint which way the rotation drag is anchored) or `None` if the
/// cursor isn't in a rotation zone.
///
/// The rotation zone is an annulus around each corner — beyond
/// the 6 px handle slop and inside the 16 px outer radius. Matches
/// the TS `hitTestRotation` logic.
///
/// INPUT path — reads the layout-resolved [`LayoutScene`] (selected
/// node geometry) + the editor's selection / viewport state.
pub fn rotation_corner_at_point(
    canvas_rect: Rect,
    scene: &LayoutScene,
    state: &EditorState,
    point: Point2D,
) -> Option<SelectionHandle> {
    // Rotation rings are only painted on single-select (the
    // multi-select overlay is outline-only), so gate the hit-test
    // to match — otherwise non-anchor "rotation zones" would
    // intercept clicks on dead air.
    let frame = selected_frame(scene, state)?;
    let local = local_doc_point(&frame, canvas_rect, state, point);
    let zoom = state.viewport.zoom.max(0.0001);
    let inner = 6.0 / zoom;
    let outer = ROTATE_OUTER_RADIUS / zoom;
    let b = frame.bounds;
    let (left, top) = (b.origin.x, b.origin.y);
    let (right, bottom) = (left + b.size.x, top + b.size.y);
    let corners = [
        (SelectionHandle::TopLeft, left, top),
        (SelectionHandle::TopRight, right, top),
        (SelectionHandle::BottomLeft, left, bottom),
        (SelectionHandle::BottomRight, right, bottom),
    ];
    for (kind, cx, cy) in corners {
        let dx = local.x - cx;
        let dy = local.y - cy;
        let dist = (dx * dx + dy * dy).sqrt();
        if dist > inner && dist <= outer {
            return Some(kind);
        }
    }
    None
}

/// Hit-test the 8 selection handles around the currently-selected
/// node. Returns the handle at `point` (a small slop around each
/// handle center counts) or `None` if no selection / no handle.
///
/// `canvas_rect` is the on-screen rect the canvas widget paints
/// into (same value passed to `CanvasViewport::paint`). The cursor is
/// taken into the node's own frame through the same flip / rotation
/// chain paint replays, so a handle the user clicks is the handle they
/// see.
///
/// INPUT path — reads the layout-resolved [`LayoutScene`] + the
/// editor's selection / viewport state (see [`rotation_corner_at_point`]).
pub fn selection_handle_at_point(
    canvas_rect: Rect,
    scene: &LayoutScene,
    state: &EditorState,
    point: Point2D,
) -> Option<SelectionHandle> {
    // Handles are only painted on single-select (the multi-select
    // overlay is outline-only — Figma parity), so gate the hit-
    // test to match. Otherwise the "anchor's handles" would hit-
    // test even though no handles are visible anywhere.
    let frame = selected_frame(scene, state)?;
    let local = local_doc_point(&frame, canvas_rect, state, point);
    let slop = 6.0 / state.viewport.zoom.max(0.0001);
    let b = frame.bounds;
    let (left, top) = (b.origin.x, b.origin.y);
    let (right, bottom) = (left + b.size.x, top + b.size.y);
    let (mid_x, mid_y) = ((left + right) / 2.0, (top + bottom) / 2.0);
    let anchors = [
        (SelectionHandle::TopLeft, left, top),
        (SelectionHandle::Top, mid_x, top),
        (SelectionHandle::TopRight, right, top),
        (SelectionHandle::Right, right, mid_y),
        (SelectionHandle::BottomRight, right, bottom),
        (SelectionHandle::Bottom, mid_x, bottom),
        (SelectionHandle::BottomLeft, left, bottom),
        (SelectionHandle::Left, left, mid_y),
    ];
    for (kind, hx, hy) in anchors {
        if (local.x - hx).abs() <= slop && (local.y - hy).abs() <= slop {
            return Some(kind);
        }
    }
    None
}

/// Screen direction (radians, clockwise from +x) the selected node's
/// `handle` pushes outward once its own and its ancestors' flips /
/// rotations apply — what the resize cursor should point along.
pub fn selection_handle_screen_angle(
    scene: &LayoutScene,
    state: &EditorState,
    handle: SelectionHandle,
) -> Option<f32> {
    let frame = selected_frame(scene, state)?;
    let (x, y) = match handle {
        SelectionHandle::Right => (1.0, 0.0),
        SelectionHandle::BottomRight => (1.0, 1.0),
        SelectionHandle::Bottom => (0.0, 1.0),
        SelectionHandle::BottomLeft => (-1.0, 1.0),
        SelectionHandle::Left => (-1.0, 0.0),
        SelectionHandle::TopLeft => (-1.0, -1.0),
        SelectionHandle::Top => (0.0, -1.0),
        SelectionHandle::TopRight => (1.0, -1.0),
    };
    let d = frame.to_page.matrix2 * DVec2::new(x, y);
    Some(d.y.atan2(d.x) as f32)
}

/// [`resize_bounds`] for a page-space drag of node `id`'s `handle`: the
/// travel is measured along the node's own (rotated / flipped) axes and
/// the opposite edge or corner stays where it renders. `start` is the
/// node's unrotated bounds in its parent frame at press time.
pub fn resize_bounds_on_page(
    scene: &LayoutScene,
    id: &str,
    start: Rect,
    handle: SelectionHandle,
    page_dx: f32,
    page_dy: f32,
) -> Rect {
    let found = scene
        .active_page()
        .and_then(|page| find_with_parent_to_page(&page.children, id));
    let Some((node, parent_to_page)) = found else {
        return resize_bounds(start, handle, page_dx, page_dy);
    };
    let linear = parent_to_page.matrix2 * own_linear(node);
    if linear == DMat2::IDENTITY {
        return resize_bounds(start, handle, page_dx, page_dy);
    }
    let local = linear.inverse() * DVec2::new(page_dx as f64, page_dy as f64);
    let resized = resize_bounds(start, handle, local.x as f32, local.y as f32);
    // The node turns about its centre, which the resize moves; shift the
    // rect so the point opposite the handle keeps its rendered spot.
    let pinned = pinned_point(start, handle);
    let before = own_transform_about(node, centre(start)).transform_point2(pinned);
    let after = own_transform_about(node, centre(resized)).transform_point2(pinned);
    let shift = before - after;
    Rect::xywh(
        resized.origin.x + shift.x as f32,
        resized.origin.y + shift.y as f32,
        resized.size.x,
        resized.size.y,
    )
}

/// The point of `rect` a drag of `handle` leaves in place: the opposite
/// corner, or the midpoint of the opposite edge.
fn pinned_point(rect: Rect, handle: SelectionHandle) -> DVec2 {
    let (left, top) = (rect.origin.x as f64, rect.origin.y as f64);
    let (right, bottom) = (left + rect.size.x as f64, top + rect.size.y as f64);
    let x = if handle.moves_left_edge() {
        right
    } else if handle.resizes_width() {
        left
    } else {
        (left + right) / 2.0
    };
    let y = if handle.moves_top_edge() {
        bottom
    } else if handle.resizes_height() {
        top
    } else {
        (top + bottom) / 2.0
    };
    DVec2::new(x, y)
}
