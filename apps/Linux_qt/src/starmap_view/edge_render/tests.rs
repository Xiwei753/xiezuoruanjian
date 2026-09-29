//! edge_render 单元测试。
//!
//! 本模块在 `edge_render` 内部（`#[cfg(test)] mod tests;`），可直接访问
//! `super` 的生产函数与类型。
//!
//! 覆盖：
//! - 边渲染命中测试（近/远）；
//! - Issue #801 评论 5895709352: 旧 portal Node 归一到 Embed 后的边锚点契约
//!   ——layout 覆盖 portal 节点 → 边正常渲染；layout 缺它 → 边因
//!   `LocalNodeMissing` 整条消失（QML `computeEdgeRenders()` 必须补几何的原因）。

use super::*;

fn make_render(id: &str, start: (f32, f32), end: (f32, f32)) -> EdgeRender {
    EdgeRender {
        edge_id: id.to_string(),
        from_cx: start.0,
        from_cy: start.1,
        to_cx: end.0,
        to_cy: end.1,
        start_x: start.0,
        start_y: start.1,
        end_x: end.0,
        end_y: end.1,
        offset_x: 0.0,
        offset_y: 0.0,
        arrow_tip_x: end.0,
        arrow_tip_y: end.1,
        arrow_left_x: end.0,
        arrow_left_y: end.1,
        arrow_right_x: end.0,
        arrow_right_y: end.1,
        label_x: (start.0 + end.0) / 2.0,
        label_y: (start.1 + end.1) / 2.0,
        label: None,
        has_bidirectional: false,
    }
}

#[test]
fn test_hit_test_edge_render_near() {
    let renders = vec![make_render("e1", (0.0, 0.0), (200.0, 0.0))];
    let hit = hit_test_edge_renders(100.0, 5.0, &renders);
    assert!(hit.is_some());
    assert_eq!(hit.unwrap(), "e1");
}

#[test]
fn test_hit_test_edge_render_far() {
    let renders = vec![make_render("e1", (0.0, 0.0), (200.0, 0.0))];
    let hit = hit_test_edge_renders(100.0, 20.0, &renders);
    assert!(hit.is_none());
}

// ─────────────────────────────────────────────────────────────────────
// Issue #801 评论 5895709352: 旧 portal Node 归一到 Embed 后的边锚点契约。
//
// QML 的 `StarMapGraphController.buildModels()` 把带 portal 的旧 Node 转成
// Embed 显示项，该节点不再出现在 nodesModel。但边端点解析仍按 node id 查
// layout（本地 Node 目标走 `node_layout_center`），所以 QML 的
// `computeEdgeRenders()` 必须把归一条目的几何补进 node layout。
// 下面两个测试固定这个契约：layout 覆盖 portal 节点 → 边正常渲染；
// layout 缺它 → 边因 LocalNodeMissing 整条消失。
// ─────────────────────────────────────────────────────────────────────

use crate::starmap_view::layout_types::{StarMapLayoutKind, StarMapLayoutNode};
use writer_core::starmap::semantic::StarMapPortal;
use writer_core::starmap::types::{
    StarMapEdge, StarMapEdgeKind, StarMapNode, StarMapNodeKind, StarMapPoint, StarMapTargetDetail,
};

fn node(id: &str, x: f32) -> StarMapNode {
    StarMapNode {
        id: id.to_string(),
        title: id.to_string(),
        kind: StarMapNodeKind::Note,
        payload: None,
        tags: vec![],
        content: Default::default(),
        anchors: vec![],
        portal: None,
        position: StarMapPoint { x, y: 0.0 },
        style: Default::default(),
        provenance: Default::default(),
        created_at: 0,
        updated_at: 0,
    }
}

/// 一张图：普通节点 `note_1`、带 portal 的旧节点 `portal_1`，
/// 以及一条本地边 `note_1 -> portal_1`。
fn graph_with_legacy_portal_node() -> StarMapGraph {
    let mut portal_node = node("portal_1", 400.0);
    portal_node.portal = Some(StarMapPortal {
        destination_starmap_id: "child_map".to_string(),
        destination_target: None,
    });
    let local_path = |node_id: &str| StarMapTargetPath {
        starmap_id: "map_1".to_string(),
        segments: vec![],
        target: StarMapTargetDetail::Node {
            node_id: node_id.to_string(),
        },
    };
    StarMapGraph {
        starmap_id: "map_1".to_string(),
        nodes: vec![node("note_1", 0.0), portal_node],
        edges: vec![StarMapEdge {
            id: "edge_1".to_string(),
            from: local_path("note_1"),
            to: local_path("portal_1"),
            kind: StarMapEdgeKind::RelatedTo,
            label: None,
            payload: None,
            created_at: 0,
            updated_at: 0,
        }],
        ..StarMapGraph::default()
    }
}

fn layout_with(entries: &[(&str, f32)]) -> StarMapLayout {
    StarMapLayout {
        kind: StarMapLayoutKind::Freeform,
        nodes: entries
            .iter()
            .map(|(id, x)| StarMapLayoutNode {
                node_id: (*id).to_string(),
                x: *x,
                y: 0.0,
                width: 150.0,
                height: 60.0,
                radius: 30.0,
                collapsed: false,
                z_index: 0,
                scale: 1.0,
                depth: 0.0,
                focus_weight: 1.0,
                orbit_group: None,
            })
            .collect(),
    }
}

#[test]
fn local_edge_to_normalized_portal_node_renders_when_layout_covers_it() {
    let graph = graph_with_legacy_portal_node();
    let layout = layout_with(&[("note_1", 0.0), ("portal_1", 400.0)]);
    let batch = compute_edge_renders_from_paths(
        &graph.edges,
        &graph,
        &layout,
        &[],
        &EdgeRenderParams::default(),
    );
    assert!(
        batch.diagnostics.is_empty(),
        "layout 覆盖旧 portal 节点时不应产生锚点诊断: {:?}",
        batch.diagnostics
    );
    assert_eq!(batch.renders.len(), 1, "指向旧 portal 节点的边必须渲染");
    assert_eq!(batch.renders[0].to_cx, 400.0 + 75.0);
}

#[test]
fn local_edge_to_normalized_portal_node_is_dropped_without_layout_geometry() {
    let graph = graph_with_legacy_portal_node();
    let layout = layout_with(&[("note_1", 0.0)]);
    let batch = compute_edge_renders_from_paths(
        &graph.edges,
        &graph,
        &layout,
        &[],
        &EdgeRenderParams::default(),
    );
    assert!(batch.renders.is_empty());
    assert_eq!(
        batch.diagnostics,
        vec![EdgeAnchorDiagnostic {
            edge_id: "edge_1".to_string(),
            endpoint: "to".to_string(),
            reason: EdgeAnchorDiagnosticReason::LocalNodeMissing,
        }],
        "layout 缺旧 portal 节点几何时，边会因 LocalNodeMissing 消失——\
         这就是 QML computeEdgeRenders() 必须补几何的原因"
    );
}
