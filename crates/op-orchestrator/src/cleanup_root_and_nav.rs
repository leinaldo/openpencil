//! Light-mobile nav-surface repair, root-height-to-content adjustment (and
//! its height-preserving sink) plus the mobile-root / nav-surface predicates
//! and hex helpers they share.

use super::*;

/// Pass ②:移动端浅色 root 下的 nav surface 纠偏。弱模型常把
/// bottom nav / tab bar 套用成黑色安全模板,和当前浅色页面调性断裂。
/// TS 端只补"缺失 fill"的 nav;Rust cleanup 还需要兜住已写
/// safe-dark fill 的误生成。
pub(super) fn repair_light_mobile_nav_surfaces(sink: &mut dyn DocSink, root_id: &str) {
    let repairs: Vec<NavSurfaceRepair> = {
        let Some(root) = find_root(sink.state(), root_id) else {
            return;
        };
        if !is_light_mobile_root(root) {
            return;
        }
        let root_surface_hex = first_solid_fill_hex(root);
        let Some(children) = root.children() else {
            return;
        };
        children
            .iter()
            .filter_map(nav_surface_target)
            .filter_map(|nav| nav_surface_repair(nav, root_surface_hex))
            .collect()
    };

    for repair in repairs {
        sink.apply(EditorCommand::SetNodeFillHex {
            node_id: repair.node_id.clone(),
            hex: repair.fill_hex,
        });
    }
}

#[derive(Debug, Clone)]
pub(super) struct NavSurfaceRepair {
    node_id: NodeId,
    fill_hex: String,
}

/// A root whose own sizing states its height — explicit `fit_content`, a
/// mobile viewport, or a trailing-nav reflow screen. Cleanup and streaming
/// growth both leave that height alone.
pub(super) fn root_height_is_authored_contract(root: &PenNode) -> bool {
    root_has_explicit_fit_content_height(root)
        || has_explicit_mobile_viewport_contract(root)
        || crate::mobile_reflow::has_mobile_trailing_nav_reflow_contract(root)
}

/// Streaming counterpart of [`adjust_root_height_to_content`]: a numeric
/// page-root height is authoritative (content past it is clipped, not grown
/// around), so a section landing below the preset would stay hidden until
/// the finalize pass. Grow — never shrink — the page root holding `node_id`.
pub(crate) fn grow_page_root_to_content(sink: &mut dyn DocSink, node_id: &NodeId) {
    let Some(root_id) = sink
        .state()
        .active_children()
        .iter()
        .find(|root| {
            root.id_str() == node_id.as_str()
                || op_editor_core::walkers::descendant_contains(root, node_id)
        })
        .map(|root| root.id_str().to_string())
    else {
        return;
    };
    let preserved = find_root(sink.state(), &root_id).is_some_and(root_height_is_authored_contract);
    adjust_root_height_to_content(sink, &root_id, preserved);
}

/// Grow roots whose authored numeric height cannot contain the estimated
/// content. `preserve_root_height` is computed from an explicit sizing mode or
/// mobile-screen semantics; narrow geometry alone is never enough to freeze a
/// poster, component board, or narrow desktop artboard.
pub(super) fn adjust_root_height_to_content(
    sink: &mut dyn DocSink,
    root_id: &str,
    preserve_root_height: bool,
) {
    if preserve_root_height {
        return;
    }
    let (total, current_height) = {
        let Some(root) = find_root(sink.state(), root_id) else {
            return;
        };
        // `fit_content` is already an explicit authored sizing mode. Replacing
        // it with a measured number would be an intent-changing conversion,
        // independent of whether the artboard is mobile.
        if root_has_explicit_fit_content_height(root) {
            return;
        }
        let estimated = root_content_height(root);
        // The tree estimator intentionally has no font shaper, so wrapped
        // fit-content text can make the real jian layout a little taller.
        // Reconcile against that same resolved scene used by diagnostics.
        let resolved = crate::geometry_validation::resolved_node_height(sink.state(), root_id)
            .filter(|height| height.is_finite() && *height > 0.0)
            .map(|height| height.ceil() as i32);
        let required = match (estimated, resolved) {
            (Some(estimated), Some(resolved)) => Some(estimated.max(resolved)),
            (estimated, resolved) => estimated.or(resolved),
        };
        (required, root.height_px())
    };

    // Non-mobile roots only GROW a too-short fixed height to fit overflowing
    // content. Never shrink here — a desktop dashboard root's height is
    // `max(region heights)` on purpose.
    if let Some(height) = total.filter(|height| {
        current_height
            .map(|current| f64::from(*height) > current)
            .unwrap_or(true)
    }) {
        sink.apply(EditorCommand::UpdateNode {
            node_id: NodeId::new(root_id.to_string()),
            x: None,
            y: None,
            width: None,
            height: Some(height),
            name: None,
            fill_hex: None,
            page_id: None,
        });
    }
}

/// Geometry validation may grow a slightly overflowing fixed-height frame.
/// That is correct for cards and ordinary content frames, but not for an
/// explicit `fit_content` root or a semantically-authored mobile viewport.
/// Filter only height writes targeting that root while forwarding every
/// descendant repair unchanged.
pub(super) struct PreserveRootHeightSink<'a> {
    pub(super) inner: &'a mut dyn DocSink,
    pub(super) root_id: &'a str,
}

impl DocSink for PreserveRootHeightSink<'_> {
    fn state(&self) -> &EditorState {
        self.inner.state()
    }

    fn apply(&mut self, cmd: EditorCommand) -> bool {
        let rewrites_root_height = match &cmd {
            EditorCommand::UpdateNode {
                node_id,
                height: Some(_),
                ..
            } => node_id.as_str() == self.root_id,
            EditorCommand::SetNodeLayoutProp {
                node_id, property, ..
            } => node_id.as_str() == self.root_id && property == "height",
            _ => false,
        };
        if rewrites_root_height {
            return false;
        }
        self.inner.apply(cmd)
    }

    fn begin_undo_batch(&mut self) {
        self.inner.begin_undo_batch();
    }

    fn end_undo_batch(&mut self) {
        self.inner.end_undo_batch();
    }
}

pub(super) fn is_light_mobile_root(root: &PenNode) -> bool {
    let width = root.width_px().unwrap_or(f64::INFINITY);
    let height = root.height_px().unwrap_or(0.0);
    if width > 480.0 || height < 500.0 {
        return false;
    }
    first_solid_fill_hex(root)
        .and_then(relative_luminance)
        .map(|luminance| luminance >= 0.5)
        .unwrap_or(false)
}

pub(super) fn is_mobile_root(root: &PenNode) -> bool {
    let width = root.width_px().unwrap_or(f64::INFINITY);
    // `fit_content` roots resolve no pixel height — treat "unresolved" as
    // mobile when the width says so, or every bottom-nav normalize pass
    // (dedupe / distribute / anchor) silently skips exactly the shape the
    // agentic loop produces (measured: GLM-5.2 root 390×fit_content kept a
    // crooked nav because none of the passes ran).
    let tall_enough = root.height_px().is_none_or(|h| h >= 500.0);
    width <= 480.0 && tall_enough
}

pub(super) fn root_has_explicit_fit_content_height(root: &PenNode) -> bool {
    let height = match root {
        PenNode::Frame(node) => node.container.height.as_ref(),
        PenNode::Group(node) => node.container.height.as_ref(),
        PenNode::Rectangle(node) => node.container.height.as_ref(),
        _ => None,
    };
    matches!(
        height,
        Some(SizingBehavior::Keyword(SizingKeyword::FitContent))
    )
}

/// Whether a fixed root contains an explicitly-authored scroll viewport that
/// consumes its remaining height.
///
/// A mobile/app/screen name is not a viewport contract: ordinary generated app
/// pages must still grow when their content grows. Preservation requires a
/// direct fill-height, clipped child whose role/name explicitly says scroll or
/// viewport.
pub(super) fn has_explicit_mobile_viewport_contract(root: &PenNode) -> bool {
    let Some(width) = root.width_px() else {
        return false;
    };
    let Some(height) = root.height_px() else {
        return false;
    };
    if width > 480.0 || height < 500.0 {
        return false;
    }

    let Some(props) = viewport_contract_container_props(root) else {
        return false;
    };
    props.layout.as_ref() == Some(&LayoutMode::Vertical)
        && root.children().is_some_and(|children| {
            children.iter().any(|child| {
                let Some(child_props) = viewport_contract_container_props(child) else {
                    return false;
                };
                child_props.clip_content == Some(true)
                    && matches!(
                        child_props.height.as_ref(),
                        Some(SizingBehavior::Keyword(SizingKeyword::FillContainer))
                    )
                    && crate::mobile_reflow::is_explicit_scroll_viewport(child)
            })
        })
}

/// Container access used only by the explicit viewport sizing contract.
///
/// Keep `frame_container_props` frame-only: its callers intentionally gate
/// frame-specific cleanup (wrapper transparency, padding collapse, and similar
/// visual rewrites). Sizing contracts, however, are valid on every container
/// variant supported by the schema.
pub(super) fn viewport_contract_container_props(node: &PenNode) -> Option<&ContainerProps> {
    match node {
        PenNode::Frame(node) => Some(&node.container),
        PenNode::Group(node) => Some(&node.container),
        PenNode::Rectangle(node) => Some(&node.container),
        _ => None,
    }
}

pub(super) fn node_identity_haystack(node: &PenNode) -> String {
    [
        node.id_str(),
        node.base().name.as_deref().unwrap_or(""),
        node.base().role.as_deref().unwrap_or(""),
    ]
    .join(" ")
    .to_lowercase()
}

pub(super) fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

pub(super) fn nav_surface_target(child: &PenNode) -> Option<&PenNode> {
    if is_nav_surface(child) {
        return Some(child);
    }

    let role = child.base().role.as_deref().unwrap_or("");
    let children = child.children()?;
    if !role.eq_ignore_ascii_case("section") || children.len() != 1 {
        return None;
    }
    children.first().filter(|inner| is_nav_surface(inner))
}

pub(super) fn is_nav_surface(node: &PenNode) -> bool {
    let role = node.base().role.as_deref().unwrap_or("").to_lowercase();
    // Matches tree_heuristics::NAV_ROLES exactly. The TOP header roles
    // (`navbar` / `top-nav-bar` / `top-app-bar`) are deliberately EXCLUDED: on a
    // light mobile page the header is transparent (TS references), and re-filling
    // it with the root surface hex + a drop-shadow is exactly what re-boxed the
    // mobile header the user flagged. Only bottom navs / floating tab bars — which
    // float over scrolling content — need a surface to read against the page.
    if matches!(
        role.as_str(),
        "nav" | "tab-bar" | "bottom-tab-bar" | "tab-row"
    ) {
        return true;
    }

    let name = node.base().name.as_deref().unwrap_or("").to_lowercase();
    let id = node.id_str().to_lowercase();
    let hay = format!("{id} {name}");
    hay.contains("bottom nav")
        || hay.contains("bottom-nav")
        || hay.contains("bottom navigation")
        || hay.contains("bottom-navigation")
        || hay.contains("tab bar")
        || hay.contains("tab-bar")
        || hay.contains("bottom tab")
        || hay.contains("bottom-tab")
}

pub(super) fn nav_surface_repair(
    nav: &PenNode,
    root_surface_hex: Option<&str>,
) -> Option<NavSurfaceRepair> {
    let solid = first_solid_fill_hex(nav);
    let has_paintable_fill = solid.map(|hex| !hex.trim().is_empty()).unwrap_or(false)
        || !matches!(first_fill_type(nav), FillType::Solid);
    let safe_dark = solid.map(is_safe_dark_hex).unwrap_or(false);
    let is_bottom_nav = is_bottom_nav_surface(nav);
    let fill_hex = root_surface_hex
        .filter(|hex| !hex.trim().is_empty())
        .unwrap_or("#FFFFFF");
    let default_white_on_tinted_root = is_bottom_nav
        && solid.map(is_default_white_surface_hex).unwrap_or(false)
        && !same_hex(solid.unwrap_or_default(), fill_hex)
        && !is_default_white_surface_hex(fill_hex);

    if has_paintable_fill && !safe_dark && !default_white_on_tinted_root {
        return None;
    }

    Some(NavSurfaceRepair {
        node_id: NodeId::new(nav.id_str().to_string()),
        fill_hex: fill_hex.to_string(),
    })
}

pub(super) fn is_default_white_surface_hex(hex: &str) -> bool {
    matches!(
        normalize_hex6(hex).as_deref(),
        Some("#FFFFFF" | "#F9FAFB" | "#F8FAFC")
    )
}

pub(super) fn same_hex(a: &str, b: &str) -> bool {
    normalize_hex6(a) == normalize_hex6(b)
}

pub(super) fn normalize_hex6(hex: &str) -> Option<String> {
    let trimmed = hex.trim();
    let body = trimmed.strip_prefix('#').unwrap_or(trimmed);
    if body.len() != 6 || !body.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("#{}", body.to_ascii_uppercase()))
}

pub(super) fn is_bottom_nav_surface(node: &PenNode) -> bool {
    let role = node.base().role.as_deref().unwrap_or("").to_lowercase();
    if role == "bottom-tab-bar" {
        return true;
    }
    let name = node.base().name.as_deref().unwrap_or("").to_lowercase();
    let id = node.id_str().to_lowercase();
    let hay = format!("{id} {name}");
    hay.contains("bottom nav")
        || hay.contains("bottom-nav")
        || hay.contains("bottom navigation")
        || hay.contains("bottom-navigation")
        || hay.contains("bottom tab")
        || hay.contains("bottom-tab")
}

pub(super) fn is_safe_dark_hex(hex: &str) -> bool {
    let Some((r, g, b)) = parse_hex_rgb(hex) else {
        return false;
    };
    let normalized = format!("#{r:02X}{g:02X}{b:02X}");
    matches!(
        normalized.as_str(),
        "#000000"
            | "#0A0A0A"
            | "#0F0F0F"
            | "#111111"
            | "#121212"
            | "#141414"
            | "#1A1A1A"
            | "#181818"
            | "#1C1C1C"
            | "#1E1E1E"
            | "#202020"
            | "#111827"
            | "#0F172A"
            | "#18181B"
            | "#1F2937"
    ) || relative_luminance_from_rgb(r, g, b) <= 0.035
}

pub(crate) fn relative_luminance(hex: &str) -> Option<f64> {
    let (r, g, b) = parse_hex_rgb(hex)?;
    Some(relative_luminance_from_rgb(r, g, b))
}

pub(super) fn relative_luminance_from_rgb(r: u8, g: u8, b: u8) -> f64 {
    (0.299 * f64::from(r) + 0.587 * f64::from(g) + 0.114 * f64::from(b)) / 255.0
}

pub(super) fn parse_hex_rgb(hex: &str) -> Option<(u8, u8, u8)> {
    // Delegates to the canonical op-util parser. This also fixes a panic:
    // the old copy byte-sliced without an is_ascii guard, so non-ASCII
    // input like "#é1" split a codepoint and panicked; it now returns None.
    const OPTS: op_util::hex_color::HexOptions = op_util::hex_color::HexOptions {
        require_hash: false,
        allow_rgb_shorthand: true,
        allow_rgba_shorthand: false,
        allow_alpha: true,
    };
    let [r, g, b, _] = op_util::hex_color::parse_hex_rgba8(hex, OPTS)?;
    Some((r, g, b))
}
