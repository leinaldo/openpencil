//! Selection-overlay tests — single/multi select chrome, dimension capsule, node-drag suppression and drop indicator.
//!
//! Split out of `canvas_viewport_tests.rs` to keep every file under
//! the repository's 800-line cap. Shared fixtures (`RecordingBackend`,
//! scene builders, transform-replay helpers) stay in that spine.

use super::*;

#[test]
fn single_selection_overlay_omits_name_and_paints_dimensions() {
    let _guard = crate::agent_indicator_test_support::lock();
    op_editor_core::agent_indicators::clear();
    let mut state = EditorState::new();
    state.doc.children = vec![named_rect_node("n2", "Schedule Card 1")];
    state.set_single_selection(op_editor_core::NodeId::new("n2"));
    let scene = sample_scene();
    let viewport = CanvasViewport::from_editor(&state, &scene);
    let mut backend = RecordingBackend::default();
    {
        let mut cx = PaintCx {
            backend: &mut backend,
        };
        viewport.paint(&mut cx, Rect::xywh(0.0, 0.0, 800.0, 600.0));
    }

    assert!(!backend.texts.iter().any(|text| text == "Schedule Card 1"));
    assert!(
        backend
            .texts
            .iter()
            .all(|text| !text.starts_with("Selected:")),
        "single-select overlay must not paint a count label"
    );
    assert!(
        backend.texts.iter().any(|text| text == "120 × 40"),
        "single-select overlay should paint bottom dimensions; texts: {:?}",
        backend.texts
    );
}

#[test]
fn single_selection_dimension_label_uses_active_color() {
    let _guard = crate::agent_indicator_test_support::lock();
    op_editor_core::agent_indicators::clear();
    let mut state = EditorState::new();
    state.doc.children = vec![named_rect_node("n2", "Schedule Card 1")];
    state.set_single_selection(op_editor_core::NodeId::new("n2"));
    let scene = sample_scene();
    let viewport = CanvasViewport::from_editor(&state, &scene);
    let mut backend = RecordingBackend::default();
    {
        let mut cx = PaintCx {
            backend: &mut backend,
        };
        viewport.paint(&mut cx, Rect::xywh(0.0, 0.0, 800.0, 600.0));
    }

    let color = backend
        .texts
        .iter()
        .zip(backend.text_colors.iter())
        .find_map(|(text, color)| (text == "120 × 40").then_some(*color))
        .expect("selected dimensions label should be painted");
    assert_eq!(color, viewport.theme.primary.to_jian());
}

#[test]
fn active_node_drag_hides_selection_chrome_and_dimension_label() {
    let _guard = crate::agent_indicator_test_support::lock();
    op_editor_core::agent_indicators::clear();
    let mut state = EditorState::new();
    state.doc.children = vec![named_rect_node("n2", "Schedule Card 1")];
    state.set_single_selection(op_editor_core::NodeId::new("n2"));
    let scene = sample_scene();
    let mut viewport = CanvasViewport::from_editor(&state, &scene);
    viewport.node_drag_active = true;
    let mut backend = RecordingBackend::default();
    {
        let mut cx = PaintCx {
            backend: &mut backend,
        };
        viewport.paint(&mut cx, Rect::xywh(0.0, 0.0, 800.0, 600.0));
    }

    assert!(
        backend.texts.iter().all(|text| text != "120 × 40"),
        "dragging should not paint the selected-size capsule; texts: {:?}",
        backend.texts
    );
    assert_eq!(
        backend.strokes, 1,
        "dragging should hide selection outlines/handles, leaving only the frame stroke"
    );
}

#[test]
fn active_node_drag_suppresses_focus_and_child_hover_outlines() {
    let _guard = crate::agent_indicator_test_support::lock();
    op_editor_core::agent_indicators::clear();
    let mut state = EditorState::new();
    state.doc.children = vec![named_frame_node("n1", "Frame")];
    state.set_single_selection(op_editor_core::NodeId::new("n2"));
    state.editor_ui.canvas_hover_node = Some(op_editor_core::NodeId::new("n1"));
    let scene = sample_scene();
    let mut viewport = CanvasViewport::from_editor(&state, &scene);
    viewport.node_drag_active = true;
    let mut backend = RecordingBackend::default();
    {
        let mut cx = PaintCx {
            backend: &mut backend,
        };
        viewport.paint(&mut cx, Rect::xywh(0.0, 0.0, 800.0, 600.0));
    }

    assert!(
        backend
            .stroke_colors
            .iter()
            .all(|color| *color != HOVER_OUTLINE_COLOR),
        "dragging should suppress both the focus outline and all direct-child hierarchy hints"
    );
    let frame_label_color = backend
        .texts
        .iter()
        .zip(backend.text_colors.iter())
        .find_map(|(text, color)| (text == "Frame").then_some(*color))
        .expect("root frame label should paint");
    assert_ne!(
        frame_label_color,
        viewport.theme.primary.to_jian(),
        "dragging should suppress the transient root-title hover tint too"
    );
}

#[test]
fn active_node_drag_overlay_paints_selected_node_at_absolute_drag_origin() {
    let _guard = crate::agent_indicator_test_support::lock();
    op_editor_core::agent_indicators::clear();
    let mut state = EditorState::new();
    state.doc.children = vec![named_rect_node("n2", "Schedule Card 1")];
    state.set_single_selection(op_editor_core::NodeId::new("n2"));
    let scene = sample_scene();
    let mut viewport = CanvasViewport::from_editor(&state, &scene);
    viewport.node_drag_active = true;
    viewport.node_drag_overlay = Some(CanvasNodeDragOverlay {
        node_id: "n2".to_string(),
        target_origin_doc: Point2D::new(100.0, 100.0),
    });
    let mut backend = RecordingBackend::default();
    {
        let mut cx = PaintCx {
            backend: &mut backend,
        };
        viewport.paint(&mut cx, Rect::xywh(0.0, 0.0, 800.0, 600.0));
    }

    assert!(
        !backend
            .fill_rects
            .iter()
            .any(|rect| *rect == Rect::xywh(60.0, 80.0, 120.0, 40.0)),
        "drag overlay should hide the selected node at its layout slot"
    );
    assert!(
        backend
            .fill_rects
            .iter()
            .any(|rect| *rect == Rect::xywh(100.0, 100.0, 120.0, 40.0)),
        "drag overlay should paint the selected node at the absolute cursor-following origin; fills: {:?}",
        backend.fill_rects
    );
}

#[test]
fn active_node_drag_drop_indicator_omits_dragged_ghost_rect() {
    let _guard = crate::agent_indicator_test_support::lock();
    op_editor_core::agent_indicators::clear();
    let mut state = EditorState::new();
    state.doc.children = vec![named_rect_node("n2", "Schedule Card 1")];
    state.set_single_selection(op_editor_core::NodeId::new("n2"));
    let scene = sample_scene();
    let mut viewport = CanvasViewport::from_editor(&state, &scene);
    viewport.node_drag_active = true;
    viewport.drop_indicator = Some(op_editor_core::editor_ui_state::CanvasDropIndicator {
        ghost: op_editor_core::editor_ui_state::CanvasOverlayRect::new(420.0, 30.0, 96.0, 48.0),
        target: Some(op_editor_core::editor_ui_state::CanvasOverlayRect::new(
            540.0, 30.0, 120.0, 80.0,
        )),
        insertion: Some(op_editor_core::editor_ui_state::CanvasOverlayLine::new(
            528.0, 24.0, 528.0, 116.0,
        )),
    });

    let mut backend = RecordingBackend::default();
    {
        let mut cx = PaintCx {
            backend: &mut backend,
        };
        viewport.paint(&mut cx, Rect::xywh(0.0, 0.0, 800.0, 600.0));
    }

    assert!(
        !backend
            .fill_rects
            .iter()
            .any(|rect| *rect == Rect::xywh(420.0, 30.0, 96.0, 48.0)),
        "dragging should not paint a ghost fill around the dragged element; fills: {:?}",
        backend.fill_rects
    );
    assert!(
        backend
            .fill_rects
            .iter()
            .any(|rect| *rect == Rect::xywh(540.0, 30.0, 120.0, 80.0)),
        "dragging should still paint the target container preview; fills: {:?}",
        backend.fill_rects
    );
}

#[test]
fn selected_root_frame_paints_one_name_label() {
    let _guard = crate::agent_indicator_test_support::lock();
    op_editor_core::agent_indicators::clear();
    let mut state = EditorState::new();
    state.doc.children = vec![named_frame_node("n1", "Frame")];
    state.set_single_selection(op_editor_core::NodeId::new("n1"));
    let scene = sample_scene();
    let viewport = CanvasViewport::from_editor(&state, &scene);
    let mut backend = RecordingBackend::default();
    {
        let mut cx = PaintCx {
            backend: &mut backend,
        };
        viewport.paint(&mut cx, Rect::xywh(0.0, 0.0, 800.0, 600.0));
    }

    let painted_count = backend.texts.iter().filter(|text| *text == "Frame").count();
    assert_eq!(
        painted_count, 1,
        "selected root frame should not draw both the root label and selection pill"
    );
    let position = backend
        .texts
        .iter()
        .zip(backend.text_positions.iter())
        .find_map(|(text, position)| (text == "Frame").then_some(*position))
        .expect("selected root frame label should be painted");
    assert_eq!(
        position.x, 40.0,
        "selected root frame label should use the plain root-title position, not pill padding"
    );
}

#[test]
fn multi_selection_overlay_omits_count_label_and_paints_union_dimensions() {
    let _guard = crate::agent_indicator_test_support::lock();
    op_editor_core::agent_indicators::clear();
    let mut state = EditorState::new();
    state.doc.children = vec![
        named_rect_node("n2", "Schedule Card 1"),
        named_rect_node("n3", "Schedule Card 2"),
    ];
    state.selection.set = vec![
        op_editor_core::NodeId::new("n2"),
        op_editor_core::NodeId::new("n3"),
    ];
    state.selection.anchor = op_editor_core::NodeId::new("n3");
    let scene = sample_scene();
    let viewport = CanvasViewport::from_editor(&state, &scene);
    let mut backend = RecordingBackend::default();
    {
        let mut cx = PaintCx {
            backend: &mut backend,
        };
        viewport.paint(&mut cx, Rect::xywh(0.0, 0.0, 800.0, 600.0));
    }

    assert!(!backend
        .texts
        .iter()
        .any(|text| text == "Selected: 2 objects"));
    assert!(
        backend.texts.iter().all(|text| text != "Schedule Card 1"),
        "multi-select overlay must not paint individual selected node names"
    );
    assert!(
        backend.texts.iter().any(|text| text == "200 × 60"),
        "multi-select overlay should paint union dimensions; texts: {:?}",
        backend.texts
    );
}

#[test]
fn rotated_drop_preview_paints_target_and_ghost_under_their_rotation() {
    use op_editor_core::editor_ui_state::{CanvasDropIndicator, CanvasOverlayRect};
    let _guard = crate::agent_indicator_test_support::lock();
    op_editor_core::agent_indicators::clear();
    let mut state = EditorState::new();
    state.doc.children = vec![named_rect_node("n2", "Schedule Card 1")];
    let scene = sample_scene();
    let mut viewport = CanvasViewport::from_editor(&state, &scene);
    viewport.drop_indicator = Some(CanvasDropIndicator {
        ghost: CanvasOverlayRect {
            rotation: 0.3,
            ..CanvasOverlayRect::new(420.0, 30.0, 96.0, 48.0)
        },
        target: Some(CanvasOverlayRect {
            rotation: 0.5,
            ..CanvasOverlayRect::new(540.0, 30.0, 120.0, 80.0)
        }),
        insertion: None,
    });

    let mut backend = RecordingBackend::default();
    {
        let mut cx = PaintCx {
            backend: &mut backend,
        };
        viewport.paint(&mut cx, Rect::xywh(0.0, 0.0, 800.0, 600.0));
    }

    let primary = viewport.theme.primary;
    assert_eq!(
        active_rotations_at_first_stroke(&backend, Color { a: 0.45, ..primary }),
        Some(vec![(0.5, Point2D::new(600.0, 70.0))]),
        "target outline rotates about the target centre"
    );
    assert_eq!(
        active_rotations_at_first_stroke(&backend, Color { a: 0.85, ..primary }),
        Some(vec![(0.3, Point2D::new(468.0, 54.0))]),
        "dashed ghost rotates about the ghost centre"
    );
}
