//! Inspector / overlay state types: shared-picker purposes, canvas
//! overlay geometry, the layer context menu + page rename drafts, the
//! variables-row and effect-param focus enums, and the preview device
//! kind.
//!
//! Split out of the `editor_ui_state` spine (800-line file ceiling);
//! every type is re-exported from there, so import paths are unchanged.

use crate::ui_draft::LayerContextTarget;

/// Surface that opened the shared searchable font picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingFontSurface {
    Prompt,
    Settings,
}

/// The shared font picker serves both the selected text node and missing-font
/// replacement rows. Keeping the purpose explicit prevents a modal picker
/// click from accidentally writing the current canvas selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontPickerPurpose {
    PropertyText,
    MissingFont {
        row: usize,
        surface: MissingFontSurface,
    },
}

/// Inspector property whose options are shown in the shared compositing
/// picker. Fill blend carries its authored fill-list index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompositingPickerTarget {
    NodeBlend,
    NodeMask,
    FillBlend(usize),
}

/// Document-space rectangle used by transient canvas overlays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasOverlayRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// Radians, clockwise about the rect centre — the rendered angle of
    /// the node the overlay outlines, including its ancestors'.
    pub rotation: f64,
}

impl CanvasOverlayRect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self {
            x,
            y,
            w,
            h,
            rotation: 0.0,
        }
    }
}

/// Document-space line segment used by transient canvas overlays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasOverlayLine {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

impl CanvasOverlayLine {
    pub fn new(x1: f64, y1: f64, x2: f64, y2: f64) -> Self {
        Self { x1, y1, x2, y2 }
    }
}

/// Drag-time drop preview for canvas node dragging. View-only,
/// transient, and excluded from history/file persistence.
#[derive(Debug, Clone, PartialEq)]
pub struct CanvasDropIndicator {
    /// Dropped bounds of the dragged node, in document space.
    pub ghost: CanvasOverlayRect,
    /// Target container bounds, when the cursor is inside one.
    pub target: Option<CanvasOverlayRect>,
    /// Flex insertion line, when the target container auto-layouts.
    pub insertion: Option<CanvasOverlayLine>,
}

/// Right-click context-menu state.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerContextMenuState {
    pub target: LayerContextTarget,
    pub anchor_x: f32,
    pub anchor_y: f32,
    /// Hovered row index for the menu paint; `None` = no row hovered.
    pub menu: jian_widgets::components::menu::MenuState,
}

/// Inline-rename state for a page row (double-click → rename).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageRenameState {
    pub page_index: usize,
    pub draft: String,
}

/// Editor focus for a variable row cell in the VariablesPanel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableRowFocus {
    Name(usize),
    Number(usize),
    String(usize),
    NumberCell {
        row: usize,
        variant: usize,
    },
    StringCell {
        row: usize,
        variant: usize,
    },
    /// Inline hex editing of a Color cell under one variant column
    /// (TS `variable-row.tsx` ColorCell hex `<input>`). The draft is
    /// committed only when it parses as a full `#rrggbb`, mirroring
    /// TS's `/^#[0-9a-fA-F]{6}$/` gate.
    ColorCell {
        row: usize,
        variant: usize,
    },
}

/// Keyboard focus on an effect-parameter value (the Effects
/// section's editable X / Y / Blur / Spread / Radius numbers).
/// `effect` is the index of the effect on the selected node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectParamFocus {
    pub effect: usize,
    pub field: crate::EffectField,
}

/// Which device frame the Canvas Preview presents. `Canvas` is the
/// free-canvas preview (today's behavior); inference never picks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewDeviceKind {
    Phone,
    Desktop,
    Canvas,
}
