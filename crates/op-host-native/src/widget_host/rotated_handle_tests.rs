//! Selection chrome and in-place editing on rotated nodes: resize
//! cursors point along the rotated axes, resizes stretch along the
//! node's own axes with the opposite edge pinned on screen, and every
//! affordance of a node inside a rotated parent — handles, rotation
//! ring, arc handles, path anchors, text caret — is hit where it renders.

use super::{CursorHint, WidgetHostNative};
use op_editor_core::{NodeId, PenNodeExt as _};
use op_editor_ui::layout_scene::SceneNode;

const W: f32 = 1440.0;
const H: f32 = 900.0;

/// R: 120×80 at (500,200) turned 90° about (560,240). Its local
/// right-mid renders at (560,300), top-left at (600,180), left-mid at
/// (560,180).
const R_ROT90: &str = r##"{"version":"1.0.0","children":[
  {"type":"rectangle","id":"R","x":500,"y":200,"width":120,"height":80,"rotation":90}]}"##;

/// C: 300×300 at (400,100) turned 90° about (550,250), holding an
/// unrotated 100×60 K whose local right-mid (650,230) renders at
/// (570,350) and left-mid (550,230) at (570,250).
const K_IN_ROT90_C: &str = r##"{"version":"1.0.0","children":[
  {"type":"frame","id":"C","x":400,"y":100,"width":300,"height":300,"rotation":90,"layout":"none","children":[
    {"type":"rectangle","id":"K","x":150,"y":100,"width":100,"height":60}]}]}"##;

fn host_selecting(json: &str, id: &str) -> WidgetHostNative {
    let doc = jian_ops_schema::load_str(json)
        .expect("fixture JSON parses")
        .value;
    let mut state = op_editor_core::EditorState::from_document(doc);
    state.chat.minimize();
    state.set_single_selection(NodeId::new(id));
    let mut host = WidgetHostNative::new();
    *host.editor_state_mut() = state;
    host.mark_paint_dirty_for_test();
    host.refresh_layout_scene();
    host
}

fn screen(host: &WidgetHostNative, p: (f32, f32)) -> (f32, f32) {
    let (cx, cy, _, _) = host.canvas_region(W, H);
    (cx + p.0, cy + p.1)
}

fn cursor_at(host: &WidgetHostNative, p: (f32, f32)) -> CursorHint {
    let (x, y) = screen(host, p);
    host.cursor_hint(x, y, W, H)
}

fn drag_handle(host: &mut WidgetHostNative, from: (f32, f32), to: (f32, f32)) {
    let (x, y) = screen(host, from);
    host.apply_press(x, y, W, H);
    assert!(
        host.handle_drag.is_some(),
        "press at {from:?} grabs a handle"
    );
    let (x, y) = screen(host, to);
    host.apply_cursor_move(x, y);
    let _ = host.apply_release_with_viewport(W, H);
}

fn geometry(host: &WidgetHostNative, id: &str) -> (f64, f64, f64, f64) {
    let node =
        op_editor_core::walkers::find_node(host.editor_state().active_children(), &NodeId::new(id))
            .expect("node present");
    (
        node.base().x.unwrap_or(0.0),
        node.base().y.unwrap_or(0.0),
        node.width_px().unwrap_or(0.0),
        node.height_px().unwrap_or(0.0),
    )
}

fn assert_geometry(actual: (f64, f64, f64, f64), expected: (f64, f64, f64, f64)) {
    let close = |a: f64, b: f64| (a - b).abs() < 0.01;
    assert!(
        close(actual.0, expected.0)
            && close(actual.1, expected.1)
            && close(actual.2, expected.2)
            && close(actual.3, expected.3),
        "expected (x, y, w, h) {expected:?}, got {actual:?}"
    );
}

#[test]
fn edge_handle_cursor_follows_the_node_rotation() {
    let host = host_selecting(R_ROT90, "R");
    assert_eq!(cursor_at(&host, (560.0, 300.0)), CursorHint::ResizeNs);
}

#[test]
fn corner_handle_cursor_follows_the_node_rotation() {
    let host = host_selecting(R_ROT90, "R");
    assert_eq!(cursor_at(&host, (600.0, 180.0)), CursorHint::ResizeNesw);
}

#[test]
fn edge_handle_stretches_along_the_rotated_axis_and_pins_the_opposite_edge() {
    let mut host = host_selecting(R_ROT90, "R");
    // Right handle renders below R; dragging it 40 px down widens R.
    drag_handle(&mut host, (560.0, 300.0), (560.0, 340.0));
    // Width 160 about a centre that keeps the left edge at (560,180).
    assert_geometry(geometry(&host, "R"), (480.0, 220.0, 160.0, 80.0));
}

#[test]
fn handles_of_a_child_in_a_rotated_parent_follow_the_parent() {
    let mut host = host_selecting(K_IN_ROT90_C, "K");
    assert_eq!(cursor_at(&host, (570.0, 350.0)), CursorHint::ResizeNs);
    drag_handle(&mut host, (570.0, 350.0), (570.0, 380.0));
    assert_geometry(geometry(&host, "K"), (150.0, 100.0, 130.0, 60.0));
}

/// C (400,100 300×300) turned 90° about (550,250) around one child.
fn in_rot90_c(child: &str) -> String {
    format!(
        r##"{{"version":"1.0.0","children":[
  {{"type":"frame","id":"C","x":400,"y":100,"width":300,"height":300,"rotation":90,"layout":"none","children":[{child}]}}]}}"##
    )
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

/// Where a point laid out in `id`'s frame renders (rotation-only chain).
fn on_page(host: &WidgetHostNative, id: &str, local: (f32, f32)) -> (f32, f32) {
    let page = host.layout_scene.active_page().expect("page");
    let mut nodes = Vec::new();
    assert!(chain(&page.children, id, &mut nodes), "{id} in scene");
    let mut p = local;
    for node in nodes.iter().rev() {
        let b = node.aggregate_bounds();
        let c = (b.origin.x + b.size.x / 2.0, b.origin.y + b.size.y / 2.0);
        let (s, k) = node.rotation.sin_cos();
        let (dx, dy) = (p.0 - c.0, p.1 - c.1);
        p = (c.0 + dx * k - dy * s, c.1 + dx * s + dy * k);
    }
    p
}

fn rendered_angle(host: &mut WidgetHostNative, id: &str) -> f32 {
    host.refresh_layout_scene();
    let page = host.layout_scene.active_page().expect("page");
    let mut nodes = Vec::new();
    assert!(chain(&page.children, id, &mut nodes));
    nodes.iter().map(|n| n.rotation.to_degrees()).sum()
}

fn local_bounds(host: &WidgetHostNative, id: &str) -> op_editor_ui::Rect {
    host.layout_scene
        .active_page()
        .and_then(|page| page.find(id))
        .expect("node in scene")
        .aggregate_bounds()
}

#[test]
fn rotation_ring_turns_a_child_of_a_rotated_parent_about_its_rendered_centre() {
    let mut host = host_selecting(K_IN_ROT90_C, "K");
    let b = local_bounds(&host, "K");
    let centre = on_page(
        &host,
        "K",
        (b.origin.x + b.size.x / 2.0, b.origin.y + b.size.y / 2.0),
    );
    // Just outside K's top-right corner, inside the rotation ring.
    let grab = on_page(&host, "K", (b.origin.x + b.size.x + 8.0, b.origin.y - 8.0));
    let turn = 30f32.to_radians();
    let (dx, dy) = (grab.0 - centre.0, grab.1 - centre.1);
    let swept = (
        centre.0 + dx * turn.cos() - dy * turn.sin(),
        centre.1 + dx * turn.sin() + dy * turn.cos(),
    );
    let (x, y) = screen(&host, grab);
    host.apply_press(x, y, W, H);
    assert!(
        host.rotate_drag.is_some(),
        "press lands on the rotation ring"
    );
    let (x, y) = screen(&host, swept);
    host.apply_cursor_move(x, y);
    let _ = host.apply_release_with_viewport(W, H);
    let angle = rendered_angle(&mut host, "K");
    assert!(
        (angle - 120.0).abs() < 0.1,
        "a 30° sweep about K's rendered centre turns it from 90° to 120°; got {angle}°"
    );
}

#[test]
fn arc_handle_of_an_ellipse_in_a_rotated_parent_is_hit_where_it_renders() {
    let doc = in_rot90_c(r#"{"type":"ellipse","id":"E","x":150,"y":100,"width":100,"height":60}"#);
    let host = host_selecting(&doc, "E");
    let b = local_bounds(&host, "E");
    // A full ellipse stacks its start and sweep handles at angle 0 — the
    // local right-mid; the sweep one paints on top and wins the hit.
    let start = on_page(
        &host,
        "E",
        (b.origin.x + b.size.x, b.origin.y + b.size.y / 2.0),
    );
    let (x, y) = screen(&host, start);
    assert_eq!(
        host.arc_handle_hit(x, y, W, H),
        Some(("E".to_string(), op_editor_ui::widgets::ArcHandle::Sweep))
    );
}

#[test]
fn path_anchor_in_a_rotated_parent_is_hit_where_it_renders() {
    let doc = in_rot90_c(
        r#"{"type":"path","id":"P","x":150,"y":100,"anchors":[{"x":0,"y":0},{"x":50,"y":25}]}"#,
    );
    let host = host_selecting(&doc, "P");
    let anchor = host
        .layout_scene
        .active_page()
        .and_then(|page| page.find("P"))
        .and_then(|node| node.path_anchors.get(1).map(|a| a.pos))
        .expect("second anchor in scene");
    let on_screen = on_page(&host, "P", (anchor.x, anchor.y));
    let (x, y) = screen(&host, on_screen);
    let hit = host.path_anchor_hit(x, y, W, H);
    assert!(
        matches!(hit, Some((ref id, 1, super::AnchorDragTarget::Anchor)) if id == "P"),
        "anchor 1 of P should be hit at its rendered spot; got {hit:?}"
    );
}

#[test]
fn text_caret_in_a_rotated_parent_lands_where_the_glyphs_render() {
    let doc = in_rot90_c(
        r#"{"type":"text","id":"T","x":150,"y":100,"width":120,"height":60,"content":"hello\nworld","fontSize":20,"lineHeight":1}"#,
    );
    let mut host = host_selecting(&doc, "T");
    assert!(host.editor_state_mut().start_text_edit(NodeId::new("T")));
    host.mark_paint_dirty_for_test();
    host.refresh_layout_scene();
    let b = local_bounds(&host, "T");
    // Far right of the first line: the caret goes after "hello".
    let end_of_line_one = on_page(&host, "T", (b.origin.x + b.size.x - 2.0, b.origin.y + 10.0));
    let (x, y) = screen(&host, end_of_line_one);
    host.apply_press(x, y, W, H);
    let _ = host.apply_release_with_viewport(W, H);
    let caret = host
        .editor_state()
        .ui
        .text_editing
        .as_ref()
        .map(|_| host.editor_state().ui.text_edit_input.caret());
    assert_eq!(caret, Some(5), "caret lands after \"hello\"");
}
