//! Canvas node-drag flow shared by the native and web widget hosts.
//!
//! The two hosts' `widget_host/canvas_select_drag.rs` and
//! `widget_host/node_drag.rs` twins used to carry this whole
//! read-plan / mutate / preview pipeline as a verbatim copy-paste run.
//! Everything below reads the layout scene and mutates `EditorState`
//! only, so it stays wasm32-clean; the hosts keep the platform tail
//! (`refresh_layout_scene`, `scene_cache.invalidate`, `mark_dirty`,
//! layout transitions, drag-state bookkeeping) in thin wrappers.

use op_editor_core::drag_mutators::DragDropTarget;
use op_editor_core::editor_ui_state::CanvasDropIndicator;
use op_editor_core::{EditorState, NodeId};

use crate::layout_scene::LayoutScene;
use crate::Rect;

pub use super::drag_flow_index::{
    build_canvas_drop_index, plan_drag_commit_indexed, CanvasDropIndex,
};

/// Read-phase summary of one dragged node — collected before any
/// mutation so no document / scene borrow survives into the mutators.
pub struct DragCommitPlan {
    /// Where the node should land, or `None` when the drop is a no-op.
    pub target: Option<DragDropTarget>,
    /// Unrotated bounds the node occupies at the drop point, in the
    /// target parent's frame (page space for a page-root drop).
    pub dropped_bounds: Rect,
    /// Own rotation (degrees) that keeps the rendered angle under the
    /// target's rotated ancestors; `None` leaves it untouched.
    pub dropped_rotation: Option<f64>,
    /// The same pose re-expressed as a page root — the live preview
    /// lifts a node out of its parent before it enters another.
    pub(super) page_bounds: Rect,
    pub(super) page_rotation: Option<f64>,
    /// Ghost / target / insertion overlay for the live drop preview.
    pub indicator: Option<CanvasDropIndicator>,
}

/// Result of one live-preview step on a single-selection drag.
pub struct LiveDragPreview {
    /// The preview reordered / reparented the tree.
    pub mutated: bool,
    /// Bounds the drag overlay should ghost at, when the node stayed
    /// inside its current auto-layout parent.
    pub overlay_bounds: Option<Rect>,
    /// Scene snapshot captured only when the preview is about to mutate
    /// the document. Hosts use it to animate sibling reflow.
    pub before_scene: Option<LayoutScene>,
}

/// Read phase — gather everything the commit needs as owned data.
///
/// `total_dx` / `total_dy` are the gesture's net doc-space travel since
/// the press; flex children never doc-translate during a drag, so that
/// delta is the only record of where the user dropped them.
pub fn plan_drag_commit(
    state: &EditorState,
    scene: &LayoutScene,
    id: &NodeId,
    total_dx: f64,
    total_dy: f64,
    excluded_ids: &[NodeId],
) -> Option<DragCommitPlan> {
    let index = build_canvas_drop_index(state, scene, id, excluded_ids)?;
    plan_drag_commit_indexed(scene, &index, total_dx, total_dy)
}

/// Mutation phase — apply the planned reparent / reorder.
pub fn apply_drag_commit(state: &mut EditorState, id: &NodeId, plan: DragCommitPlan) -> bool {
    let Some(target) = plan.target else {
        return false;
    };
    let bounds = plan.dropped_bounds;
    state.move_node_to_drop_target(
        id,
        target,
        bounds.origin.x as f64,
        bounds.origin.y as f64,
        bounds.size.x as f64,
        bounds.size.y as f64,
        plan.dropped_rotation,
    )
}

/// Release commit for the whole selection: a node dropped into another
/// container reparents there; a node dropped outside every container
/// becomes a page root; a child dropped within an auto-layout parent
/// re-inserts at the midpoint-derived sibling index. Free-layout
/// children were already translated live.
///
/// Returns `true` when the tree changed — the caller then invalidates
/// its scene cache (the drag snapshot already consumed this gesture's
/// document revision) and repaints.
pub fn commit_node_drag(
    state: &mut EditorState,
    scene: &LayoutScene,
    total_dx: f64,
    total_dy: f64,
    excluded_ids: &[NodeId],
) -> bool {
    let ids = state.selection.set.clone();
    let mut mutated = false;
    for id in &ids {
        let Some(plan) = plan_drag_commit(state, scene, id, total_dx, total_dy, excluded_ids)
        else {
            continue;
        };
        mutated |= apply_drag_commit(state, id, plan);
    }
    mutated
}

/// Recompute the drop indicator for the anchor node without mutating
/// the tree (the multi-selection preview path).
pub fn refresh_drop_indicator(
    state: &mut EditorState,
    scene: &LayoutScene,
    total_dx: f64,
    total_dy: f64,
    excluded_ids: &[NodeId],
) {
    let id = state.selection.anchor.clone();
    let next = if id.is_real() {
        plan_drag_commit(state, scene, &id, total_dx, total_dy, excluded_ids)
            .and_then(|plan| plan.indicator)
    } else {
        None
    };
    if state.editor_ui.canvas_drop_indicator != next {
        state.editor_ui.canvas_drop_indicator = next;
    }
}

/// Single-selection live preview: reorder inside the current
/// auto-layout parent as the cursor travels, lift a node out of its
/// parent as soon as it leaves, and otherwise only paint the indicator.
///
/// `None` means the node has no plan at all — the indicator was
/// cleared and the caller must leave its overlay bounds untouched.
pub fn apply_live_drag_preview(
    state: &mut EditorState,
    scene: &LayoutScene,
    id: &NodeId,
    total_dx: f64,
    total_dy: f64,
    excluded_ids: &[NodeId],
) -> Option<LiveDragPreview> {
    let Some(plan) = plan_drag_commit(state, scene, id, total_dx, total_dy, excluded_ids) else {
        state.editor_ui.canvas_drop_indicator = None;
        return None;
    };
    let (current_parent, current_index) =
        op_editor_core::walkers::find_parent_and_index(state.active_children(), id)?;
    Some(apply_live_drag_preview_plan(
        state,
        scene,
        id,
        current_parent,
        current_index,
        plan,
    ))
}

/// Single-selection live preview backed by a gesture-scoped drop index.
pub fn apply_live_drag_preview_indexed(
    state: &mut EditorState,
    scene: &LayoutScene,
    index: &CanvasDropIndex,
    id: &NodeId,
    total_dx: f64,
    total_dy: f64,
) -> Option<LiveDragPreview> {
    let Some(plan) = plan_drag_commit_indexed(scene, index, total_dx, total_dy) else {
        state.editor_ui.canvas_drop_indicator = None;
        return None;
    };
    Some(apply_live_drag_preview_plan(
        state,
        scene,
        id,
        index.current_parent().cloned(),
        index.current_index(),
        plan,
    ))
}

fn apply_live_drag_preview_plan(
    state: &mut EditorState,
    scene: &LayoutScene,
    id: &NodeId,
    current_parent: Option<NodeId>,
    current_index: usize,
    plan: DragCommitPlan,
) -> LiveDragPreview {
    let bounds = plan.dropped_bounds;
    let planned_indicator = plan.indicator.clone();
    let mut indicator = None;
    let mut mutated = false;
    let mut overlay_bounds = None;
    let mut before_scene = None;
    if let Some(target) = plan.target.clone() {
        match target {
            DragDropTarget::Container {
                ref parent_id,
                index,
                ..
            } if current_parent.as_ref() == Some(parent_id) => {
                overlay_bounds = Some(bounds);
                if current_index != index {
                    before_scene = Some(scene.clone());
                    mutated |= apply_drag_commit(state, id, plan);
                }
            }
            DragDropTarget::PageRoot { .. } if current_parent.is_some() => {
                before_scene = Some(scene.clone());
                mutated |= apply_drag_commit(state, id, plan);
            }
            DragDropTarget::Container { .. } if current_parent.is_some() => {
                before_scene = Some(scene.clone());
                let lifted = plan.page_bounds;
                mutated |= state.move_node_to_drop_target(
                    id,
                    DragDropTarget::PageRoot { index: 0 },
                    lifted.origin.x as f64,
                    lifted.origin.y as f64,
                    lifted.size.x as f64,
                    lifted.size.y as f64,
                    plan.page_rotation,
                );
                indicator = planned_indicator;
            }
            _ => {
                indicator = planned_indicator;
            }
        }
    }
    if state.editor_ui.canvas_drop_indicator != indicator {
        state.editor_ui.canvas_drop_indicator = indicator;
    }
    LiveDragPreview {
        mutated,
        overlay_bounds,
        before_scene,
    }
}

/// Scene ids the incremental drag patch may translate: exactly what
/// `translate_selected` moved in the document — editable nodes only
/// (locked / hidden are skipped there) and not flex-flow children
/// (positioned by their parent). Otherwise the scene would drift nodes
/// the doc never moved, then snap back on the release-time
/// reconversion.
pub fn drag_scene_translate_ids(state: &EditorState) -> Vec<String> {
    let children = state.active_children();
    state
        .selection
        .set
        .iter()
        .filter(|id| {
            state.is_editable(id) && !op_editor_core::walkers::is_flow_child_of_flex(children, id)
        })
        .map(|id| id.as_str().to_string())
        .collect()
}

/// Incremental scene patch for one page-space drag step: every id from
/// [`drag_scene_translate_ids`] moves by the step mapped into its
/// parent's frame, matching `EditorState::translate_selected_world`.
pub fn translate_drag_scene(
    scene: &mut LayoutScene,
    state: &EditorState,
    dx: f64,
    dy: f64,
) -> bool {
    let children = state.active_children();
    let mut groups: Vec<((f32, f32), Vec<String>)> = Vec::new();
    for id in drag_scene_translate_ids(state) {
        let (lx, ly) =
            op_editor_core::walkers::page_delta_to_local(children, &NodeId::new(&id), dx, dy)
                .unwrap_or((dx, dy));
        let key = (lx as f32, ly as f32);
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, ids)) => ids.push(id),
            None => groups.push((key, vec![id])),
        }
    }
    let mut moved = false;
    for ((lx, ly), ids) in groups {
        moved |= scene.translate_nodes(&ids, lx, ly);
    }
    moved
}

/// Layer-panel drop that changes `source`'s parent: the x / y (relative
/// to the new parent) and own rotation (degrees) that keep it rendering
/// where it does now, like a canvas drop. `None` when the parent stays.
/// `scene` is the pre-move layout.
pub fn layer_drop_pose(
    state: &EditorState,
    scene: &LayoutScene,
    source: &NodeId,
    anchor: &NodeId,
    position: super::DropPosition,
) -> Option<(f64, f64, f64)> {
    use super::scene_transform::{
        centre, find_with_parent_to_page, own_rotation_in, own_transform,
    };
    let children = state.active_children();
    let new_parent = match position {
        super::DropPosition::Into => Some(anchor.clone()),
        super::DropPosition::Before | super::DropPosition::After => {
            op_editor_core::drag_mutators::parent_of(children, anchor)
        }
    };
    if op_editor_core::drag_mutators::parent_of(children, source) == new_parent {
        return None;
    }
    let page = scene.active_page()?;
    let (node, parent_to_page) = find_with_parent_to_page(&page.children, source.as_str())?;
    let node_to_page = parent_to_page * own_transform(node);
    let (frame_to_page, origin) = match &new_parent {
        Some(id) => {
            let (parent, to_page) = find_with_parent_to_page(&page.children, id.as_str())?;
            (to_page * own_transform(parent), parent.bounds.origin)
        }
        None => (glam::DAffine2::IDENTITY, crate::Point2D::new(0.0, 0.0)),
    };
    let bounds = node.aggregate_bounds();
    let local = frame_to_page
        .inverse()
        .transform_point2(node_to_page.transform_point2(centre(bounds)));
    Some((
        local.x - (bounds.size.x / 2.0 + origin.x) as f64,
        local.y - (bounds.size.y / 2.0 + origin.y) as f64,
        own_rotation_in(node, node_to_page, frame_to_page),
    ))
}
