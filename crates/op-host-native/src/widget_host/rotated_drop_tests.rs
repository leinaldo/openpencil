//! Canvas drag-into-container with rotated sources, targets and
//! ancestors: the drop preview, the drop hit-test, the reparent commit
//! and smart-guide snapping must all agree with the rendered geometry,
//! and the layer tree must show the new parent as soon as the drop lands.
//!
//! Geometry: zoom 1 / pan 0, chat minimized, fixtures at doc x ≥ 420 so
//! presses land on the canvas. `world()` replays the scene's ancestor
//! rotation chain, which is what paint shows.

use super::WidgetHostNative;
use op_editor_core::editor_ui_state::CanvasOverlayRect;
use op_editor_core::{NodeId, PenNodeExt as _};
use op_editor_ui::layout_scene::SceneNode;

const W: f32 = 1440.0;
const H: f32 = 900.0;
const DEG30: f64 = std::f64::consts::PI / 6.0;

fn host_with(json: &str) -> WidgetHostNative {
    let doc = jian_ops_schema::load_str(json)
        .expect("fixture JSON parses")
        .value;
    let mut state = op_editor_core::EditorState::from_document(doc);
    state.chat.minimize();
    let mut host = WidgetHostNative::new();
    *host.editor_state_mut() = state;
    host.mark_paint_dirty_for_test();
    host
}

fn screen(host: &WidgetHostNative, p: (f64, f64)) -> (f32, f32) {
    let (cx, cy, _, _) = host.canvas_region(W, H);
    (cx + p.0 as f32, cy + p.1 as f32)
}

/// Press at `from`, cross the drag threshold, then move to each point.
fn drag_through(host: &mut WidgetHostNative, from: (f64, f64), points: &[(f64, f64)]) {
    let (x, y) = screen(host, from);
    host.apply_press(x, y, W, H);
    host.apply_cursor_move(x, y - 8.0);
    for &p in points {
        let (x, y) = screen(host, p);
        host.apply_cursor_move(x, y);
    }
}

fn release(host: &mut WidgetHostNative) {
    let _ = host.apply_release_with_viewport(W, H);
}

fn chain<'a>(nodes: &'a [SceneNode], id: &str, out: &mut Vec<&'a SceneNode>) -> bool {
    for node in nodes {
        out.push(node);
        if node.id == id || chain(&node.children, id, out) {
            return true;
        }
        out.pop();
    }
    false
}

fn centre_of(node: &SceneNode) -> (f64, f64) {
    let b = node.aggregate_bounds();
    (
        (b.origin.x + b.size.x / 2.0) as f64,
        (b.origin.y + b.size.y / 2.0) as f64,
    )
}

fn rotate_about(p: (f64, f64), c: (f64, f64), a: f64) -> (f64, f64) {
    let (dx, dy) = (p.0 - c.0, p.1 - c.1);
    (
        c.0 + dx * a.cos() - dy * a.sin(),
        c.1 + dx * a.sin() + dy * a.cos(),
    )
}

/// Rendered centre and rendered angle (degrees) of `id`.
fn world(host: &mut WidgetHostNative, id: &str) -> ((f64, f64), f64) {
    host.refresh_layout_scene();
    let page = host.layout_scene.active_page().expect("active page");
    let mut nodes = Vec::new();
    assert!(
        chain(&page.children, id, &mut nodes),
        "{id} is in the scene"
    );
    let mut p = centre_of(nodes.last().unwrap());
    let mut angle = 0.0;
    for node in nodes.iter().rev() {
        p = rotate_about(p, centre_of(node), node.rotation as f64);
        angle += (node.rotation as f64).to_degrees();
    }
    (p, angle)
}

fn parent(host: &WidgetHostNative, id: &str) -> Option<NodeId> {
    op_editor_core::drag_mutators::parent_of(
        host.editor_state().active_children(),
        &NodeId::new(id),
    )
}

fn node_xy(host: &WidgetHostNative, id: &str) -> (f64, f64) {
    let node =
        op_editor_core::walkers::find_node(host.editor_state().active_children(), &NodeId::new(id))
            .expect("node present");
    (node.base().x.unwrap_or(0.0), node.base().y.unwrap_or(0.0))
}

fn target(host: &WidgetHostNative) -> Option<CanvasOverlayRect> {
    host.editor_state()
        .editor_ui
        .canvas_drop_indicator
        .as_ref()
        .and_then(|indicator| indicator.target)
}

fn rect_centre(r: &CanvasOverlayRect) -> (f64, f64) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

fn assert_near(actual: (f64, f64), expected: (f64, f64), what: &str) {
    assert!(
        (actual.0 - expected.0).abs() < 0.5 && (actual.1 - expected.1).abs() < 0.5,
        "{what}: expected {expected:?}, got {actual:?}"
    );
}

fn assert_angle(actual_deg: f64, expected_deg: f64, what: &str) {
    let diff = (actual_deg - expected_deg).rem_euclid(360.0);
    assert!(
        !(0.1..=359.9).contains(&diff),
        "{what}: expected {expected_deg}°, got {actual_deg}°"
    );
}

/// B: free frame (460,100 260×160) rotated 30° about (590,180).
/// A: 60×60 rect centred at (590,450).
const B_ROT30: &str = r##"{"version":"1.0.0","children":[
  {"type":"frame","id":"B","x":460,"y":100,"width":260,"height":160,"rotation":30,"layout":"none","children":[]},
  {"type":"rectangle","id":"A","x":560,"y":420,"width":60,"height":60}]}"##;

/// C: frame (420,80 300×220) rotated 30° about (570,190) holding an
/// unrotated frame B whose local centre (635,150) renders at ≈(646.3,187.9).
const B_IN_ROTATED_C: &str = r##"{"version":"1.0.0","children":[
  {"type":"frame","id":"C","x":420,"y":80,"width":300,"height":220,"rotation":30,"layout":"none","children":[
    {"type":"frame","id":"B","x":150,"y":20,"width":130,"height":100,"layout":"none","children":[]}]},
  {"type":"rectangle","id":"A","x":560,"y":420,"width":60,"height":60}]}"##;
const B_IN_C_WORLD_CENTRE: (f64, f64) = (646.29, 187.86);

/// Horizontal auto-layout B (440,100 280×120) rotated 30°.
const FLEX_B_ROT30: &str = r##"{"version":"1.0.0","children":[
  {"type":"frame","id":"B","x":440,"y":100,"width":280,"height":120,"rotation":30,"layout":"horizontal","gap":12,"padding":[16,16,16,16],"children":[
    {"type":"rectangle","id":"c1","width":50,"height":50},
    {"type":"rectangle","id":"c2","width":50,"height":50}]},
  {"type":"rectangle","id":"A","x":565,"y":420,"width":50,"height":50}]}"##;

#[test]
fn drop_target_highlight_carries_the_target_rotation() {
    let mut host = host_with(B_ROT30);
    drag_through(&mut host, (590.0, 450.0), &[(590.0, 180.0)]);
    let target = target(&host).expect("rotated B is the drop target");
    assert_near(rect_centre(&target), (590.0, 180.0), "highlight centre");
    assert!((target.w - 260.0).abs() < 0.01 && (target.h - 160.0).abs() < 0.01);
    assert!(
        (target.rotation - DEG30).abs() < 1e-3,
        "highlight must rotate with B; got {} rad",
        target.rotation
    );
}

#[test]
fn drop_target_highlight_replays_the_rotated_ancestor_chain() {
    let mut host = host_with(B_IN_ROTATED_C);
    drag_through(&mut host, (590.0, 450.0), &[B_IN_C_WORLD_CENTRE]);
    let target = target(&host).expect("B inside rotated C is the drop target");
    assert!((target.w - 130.0).abs() < 0.01 && (target.h - 100.0).abs() < 0.01);
    assert_near(
        rect_centre(&target),
        B_IN_C_WORLD_CENTRE,
        "highlight centre",
    );
    assert!(
        (target.rotation - DEG30).abs() < 1e-3,
        "got {} rad",
        target.rotation
    );
}

#[test]
fn flex_insertion_line_rotates_with_its_container() {
    let mut host = host_with(FLEX_B_ROT30);
    drag_through(&mut host, (590.0, 445.0), &[(580.0, 160.0)]);
    let indicator = host
        .editor_state()
        .editor_ui
        .canvas_drop_indicator
        .clone()
        .expect("drop preview");
    let target = indicator.target.expect("flex B is the drop target");
    assert!(
        (target.rotation - DEG30).abs() < 1e-3,
        "got {} rad",
        target.rotation
    );
    let line = indicator.insertion.expect("insertion line");
    let (dx, dy) = (line.x2 - line.x1, line.y2 - line.y1);
    let len = (dx * dx + dy * dy).sqrt();
    // A horizontal row's insertion line is vertical in the row's frame.
    let expected = (-DEG30.sin(), DEG30.cos());
    assert!(
        (dx / len - expected.0).abs() < 1e-3 && (dy / len - expected.1).abs() < 1e-3,
        "insertion line direction {:?}, expected {expected:?}",
        (dx / len, dy / len)
    );
}

#[test]
fn drop_ghost_carries_the_dragged_node_rotation() {
    let mut host = host_with(
        r##"{"version":"1.0.0","children":[
          {"type":"frame","id":"B","x":460,"y":100,"width":260,"height":160,"layout":"none","children":[]},
          {"type":"rectangle","id":"A","x":550,"y":420,"width":80,"height":50,"rotation":30}]}"##,
    );
    drag_through(&mut host, (590.0, 445.0), &[(590.0, 180.0)]);
    let indicator = host
        .editor_state()
        .editor_ui
        .canvas_drop_indicator
        .clone()
        .expect("drop preview");
    assert!(
        (indicator.ghost.rotation - DEG30).abs() < 1e-3,
        "ghost must rotate with A; got {} rad",
        indicator.ghost.rotation
    );
    assert_eq!(indicator.target.map(|t| t.rotation), Some(0.0));
}

#[test]
fn drop_inside_the_rotated_shape_but_outside_its_unrotated_rect_targets_it() {
    // Local (120, 70) of B renders at ≈(658.9, 300.6) — below B's
    // unrotated rect (y ≤ 260) yet inside the rotated body.
    let mut host = host_with(
        r##"{"version":"1.0.0","children":[
          {"type":"frame","id":"B","x":460,"y":100,"width":260,"height":160,"rotation":30,"layout":"none","children":[]},
          {"type":"rectangle","id":"A","x":800,"y":420,"width":40,"height":40}]}"##,
    );
    drag_through(&mut host, (820.0, 440.0), &[(658.9, 300.6)]);
    assert!(
        target(&host).is_some(),
        "the rotated body of B must accept the drop"
    );
    release(&mut host);
    assert_eq!(parent(&host, "A"), Some(NodeId::new("B")));
}

#[test]
fn drop_inside_the_unrotated_rect_but_outside_the_rotated_shape_ignores_it() {
    // (470, 110) sits in B's unrotated rect but outside the rotated body.
    let mut host = host_with(B_ROT30);
    drag_through(&mut host, (590.0, 450.0), &[(470.0, 110.0)]);
    assert!(
        target(&host).is_none(),
        "empty canvas beside rotated B is no target"
    );
    release(&mut host);
    assert_eq!(parent(&host, "A"), None);
}

#[test]
fn dropping_into_a_rotated_container_keeps_the_rendered_pose() {
    let mut host = host_with(
        r##"{"version":"1.0.0","children":[
          {"type":"frame","id":"B","x":480,"y":80,"width":220,"height":160,"rotation":45,"layout":"none","children":[]},
          {"type":"rectangle","id":"A","x":550,"y":420,"width":80,"height":50,"rotation":20}]}"##,
    );
    drag_through(&mut host, (590.0, 445.0), &[(610.0, 170.0)]);
    let (before_centre, before_angle) = world(&mut host, "A");
    assert_near(before_centre, (610.0, 170.0), "A follows the cursor");
    release(&mut host);
    assert_eq!(parent(&host, "A"), Some(NodeId::new("B")));
    let (after_centre, after_angle) = world(&mut host, "A");
    assert_near(after_centre, before_centre, "A must not jump on drop");
    assert_angle(after_angle, before_angle, "A must not spin on drop");
    assert_angle(after_angle, 20.0, "A keeps its on-canvas angle");
}

#[test]
fn dropping_into_a_container_under_a_rotated_parent_keeps_the_rendered_pose() {
    let mut host = host_with(B_IN_ROTATED_C);
    let drop = (B_IN_C_WORLD_CENTRE.0 + 10.0, B_IN_C_WORLD_CENTRE.1 + 5.0);
    drag_through(&mut host, (590.0, 450.0), &[drop]);
    // Smart guides may pull A a few px onto C's rotated box.
    let (before, _) = world(&mut host, "A");
    assert!((before.0 - drop.0).abs() <= 6.0 && (before.1 - drop.1).abs() <= 6.0);
    release(&mut host);
    assert_eq!(parent(&host, "A"), Some(NodeId::new("B")));
    let (centre, angle) = world(&mut host, "A");
    assert_near(centre, before, "A stays where it was dropped");
    assert_angle(angle, 0.0, "A keeps its upright on-canvas angle");
}

#[test]
fn dropping_into_a_rotated_flex_container_keeps_the_rendered_angle() {
    let mut host = host_with(FLEX_B_ROT30);
    drag_through(&mut host, (590.0, 445.0), &[(580.0, 160.0)]);
    release(&mut host);
    assert_eq!(parent(&host, "A"), Some(NodeId::new("B")));
    let (_, angle) = world(&mut host, "A");
    assert_angle(angle, 0.0, "flow placement may move A but must not spin it");
}

#[test]
fn dragging_a_child_out_of_a_rotated_parent_keeps_it_under_the_cursor() {
    // A's local centre (470,245) renders at ≈(455.9,187.6), angled 30°.
    let mut host = host_with(
        r##"{"version":"1.0.0","children":[
          {"type":"frame","id":"C","x":420,"y":80,"width":300,"height":220,"rotation":30,"layout":"none","children":[
            {"type":"rectangle","id":"A","x":20,"y":140,"width":60,"height":50}]}]}"##,
    );
    host.editor_state_mut().editor_ui.entered_container = Some(NodeId::new("C"));
    let (start, start_angle) = world(&mut host, "A");
    assert_angle(start_angle, 30.0, "fixture");
    let press = (start.0, start.1);
    drag_through(&mut host, press, &[(press.0 + 20.0, press.1 + 10.0)]);
    assert_eq!(parent(&host, "A"), Some(NodeId::new("C")), "still inside C");
    let (inside, _) = world(&mut host, "A");
    assert_near(
        inside,
        (start.0 + 20.0, start.1 + 10.0),
        "A follows the cursor inside C",
    );

    let out = (press.0 + 444.0, press.1 + 292.0);
    let (x, y) = screen(&host, out);
    host.apply_cursor_move(x, y);
    assert_eq!(parent(&host, "A"), None, "leaving C lifts A to the page");
    let (lifted, lifted_angle) = world(&mut host, "A");
    assert_near(
        lifted,
        (start.0 + 444.0, start.1 + 292.0),
        "A follows the cursor out of C",
    );
    assert_angle(
        lifted_angle,
        30.0,
        "A keeps its on-canvas angle when lifted",
    );
    release(&mut host);
    let (dropped, dropped_angle) = world(&mut host, "A");
    assert_near(dropped, lifted, "release keeps A in place");
    assert_angle(dropped_angle, 30.0, "release keeps A's angle");
}

#[test]
fn smart_guides_snap_to_the_rotated_bounding_box() {
    // Rotated B's on-canvas box spans y 45.7..314.3 (unrotated: 100..260).
    let mut host = host_with(
        r##"{"version":"1.0.0","children":[
          {"type":"frame","id":"B","x":460,"y":100,"width":260,"height":160,"rotation":30,"layout":"none","children":[]},
          {"type":"rectangle","id":"A","x":800,"y":420,"width":60,"height":60}]}"##,
    );
    // Lands A's top edge at y 317 — 3 px below B's rotated bottom edge.
    drag_through(&mut host, (830.0, 450.0), &[(830.0, 347.0)]);
    let (_, y) = node_xy(&host, "A");
    assert!(
        (y - 314.28).abs() < 0.05,
        "A's top should snap to rotated B's bottom (≈314.28); got {y}"
    );
}

#[test]
fn passing_through_a_smart_guide_does_not_drift_the_dragged_node() {
    let mut host = host_with(
        r##"{"version":"1.0.0","children":[
          {"type":"rectangle","id":"X","x":400,"y":100,"width":100,"height":100},
          {"type":"rectangle","id":"A","x":700,"y":400,"width":50,"height":50}]}"##,
    );
    // Walk A upward 10 px at a time across X's top / centre / bottom
    // guides and stop where no guide is within reach (A top at y 60).
    let steps: Vec<(f64, f64)> = (1..=34).map(|i| (725.0, 425.0 - 10.0 * i as f64)).collect();
    drag_through(&mut host, (725.0, 425.0), &steps);
    assert_eq!(
        host.editor_state().editor_ui.active_guides.len(),
        0,
        "no guide at the end"
    );
    let (x, y) = node_xy(&host, "A");
    assert!(
        (x - 700.0).abs() < 1e-6 && (y - 60.0).abs() < 1e-6,
        "A must end exactly at press + travel (700, 60); got ({x}, {y})"
    );
}

fn layer_depth(host: &WidgetHostNative, id: &str) -> Option<usize> {
    host.layer_panel()
        .items
        .iter()
        .find(|item| item.node_id == NodeId::new(id))
        .map(|item| item.depth)
}

#[test]
fn dropping_into_a_container_refreshes_the_layer_tree_immediately() {
    let mut host = host_with(B_ROT30);
    drag_through(&mut host, (590.0, 450.0), &[(590.0, 180.0)]);
    // Paint caches the layer rows at the drag's latest revision.
    assert_eq!(layer_depth(&host, "A"), Some(0));
    release(&mut host);
    assert_eq!(parent(&host, "A"), Some(NodeId::new("B")));
    assert_eq!(
        layer_depth(&host, "A"),
        Some(1),
        "A must be listed under B right after the drop"
    );
}
