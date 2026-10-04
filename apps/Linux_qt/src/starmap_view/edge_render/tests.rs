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

// ─────────────────────────────────────────────────────────────────────
// 评论 5976758184: 正式边端点 = 形状真实边界交点（Node 矩形 / Embed 圆周），
// 不再靠固定 42px 内缩猜边界。
// ─────────────────────────────────────────────────────────────────────

/// 圆求交：从圆心朝目标方向，交点必须落在圆周上。
#[test]
fn boundary_point_on_circle_is_on_the_circumference() {
    let embed = EndpointShape {
        x: 0.0,
        y: 0.0,
        width: 200.0,
        height: 200.0,
        is_embed: true,
    };
    let right = endpoint_boundary_point(&embed, (100.0, 100.0), (1.0, 0.0));
    assert!(
        (right.0 - 200.0).abs() < 0.01 && (right.1 - 100.0).abs() < 0.01,
        "右侧圆周交点: {:?}",
        right
    );
    let diag = endpoint_boundary_point(&embed, (100.0, 100.0), (0.6, 0.8));
    assert!(
        (diag.0 - 160.0).abs() < 0.01 && (diag.1 - 180.0).abs() < 0.01,
        "斜向圆周交点: {:?}",
        diag
    );
    // 偏移线（双向边）：交点仍在圆周上。
    let shifted = endpoint_boundary_point(&embed, (100.0, 112.0), (1.0, 0.0));
    let d = ((shifted.0 - 100.0).powi(2) + (shifted.1 - 100.0).powi(2)).sqrt();
    assert!((d - 100.0).abs() < 0.01, "偏移线交点必须贴圆周, d={}", d);
}

/// 矩形求交：从矩形中心朝目标方向，交点落在矩形边上。
#[test]
fn boundary_point_on_rect_is_on_the_rect_edge() {
    let node = EndpointShape {
        x: 0.0,
        y: 0.0,
        width: 150.0,
        height: 60.0,
        is_embed: false,
    };
    let right = endpoint_boundary_point(&node, (75.0, 30.0), (1.0, 0.0));
    assert!(
        (right.0 - 150.0).abs() < 0.01 && (right.1 - 30.0).abs() < 0.01,
        "右侧矩形边: {:?}",
        right
    );
    let up = endpoint_boundary_point(&node, (75.0, 30.0), (0.0, -1.0));
    assert!(
        (up.0 - 75.0).abs() < 0.01 && up.1.abs() < 0.01,
        "上侧矩形边: {:?}",
        up
    );
}

/// 端到端：本地 Node → 深路径 Embed 端点。起点在矩形边、终点贴在圆周上，
/// 固定 42px 内缩的旧行为（x ≈ 458）会扎进 200 圆内 58px。
#[test]
fn edge_to_embed_endpoint_lands_on_circle_boundary() {
    use writer_core::starmap::types::reference::StarMapPathSegment;

    let graph = StarMapGraph {
        starmap_id: "map_1".to_string(),
        nodes: vec![node("note_1", 0.0)],
        edges: vec![StarMapEdge {
            id: "edge_1".to_string(),
            from: StarMapTargetPath {
                starmap_id: "map_1".to_string(),
                segments: vec![],
                target: StarMapTargetDetail::Node {
                    node_id: "note_1".to_string(),
                },
            },
            to: StarMapTargetPath {
                starmap_id: "map_1".to_string(),
                segments: vec![StarMapPathSegment::EnterEmbed {
                    instance_id: "e1".to_string(),
                }],
                target: StarMapTargetDetail::Node {
                    node_id: "child_note".to_string(),
                },
            },
            kind: StarMapEdgeKind::RelatedTo,
            label: None,
            payload: None,
            created_at: 0,
            updated_at: 0,
        }],
        ..StarMapGraph::default()
    };
    let layout = layout_with(&[("note_1", 0.0)]);
    let embed_rects = vec![StarMapEmbedSceneRect {
        instance_id: "e1".to_string(),
        x: 400.0,
        y: 0.0,
        width: 200.0,
        height: 200.0,
    }];
    let batch = compute_edge_renders_from_paths(
        &graph.edges,
        &graph,
        &layout,
        &embed_rects,
        &EdgeRenderParams::default(),
    );
    assert!(batch.diagnostics.is_empty(), "{:?}", batch.diagnostics);
    assert_eq!(batch.renders.len(), 1);
    let r = &batch.renders[0];
    assert!(
        (r.start_x - 150.0).abs() < 0.01,
        "起点必须落在 note 右边界, start_x={}",
        r.start_x
    );
    let d = ((r.end_x - 500.0).powi(2) + (r.end_y - 100.0).powi(2)).sqrt();
    assert!(
        (d - 100.0).abs() < 0.05,
        "终点必须贴在 200 正圆的圆周上, d={} (end=({},{}))",
        d,
        r.end_x,
        r.end_y
    );
    assert!(
        r.end_x < 420.0,
        "终点不能深入圆内（旧 42px 内缩会停在 ~458）, end_x={}",
        r.end_x
    );
}

/// 旧 Portal 端点（本地路径 + 归一显示几何）：可见身份是正圆，
/// 端点同样必须贴在圆周上。
#[test]
fn portal_endpoint_uses_normalized_circle_when_available() {
    let graph = graph_with_legacy_portal_node();
    let layout = layout_with(&[("note_1", 0.0), ("portal_1", 400.0)]);
    let embed_rects = vec![StarMapEmbedSceneRect {
        instance_id: "legacy-portal:portal_1".to_string(),
        x: 400.0,
        y: 0.0,
        width: 200.0,
        height: 200.0,
    }];
    let batch = compute_edge_renders_from_paths(
        &graph.edges,
        &graph,
        &layout,
        &embed_rects,
        &EdgeRenderParams::default(),
    );
    assert!(batch.diagnostics.is_empty(), "{:?}", batch.diagnostics);
    assert_eq!(batch.renders.len(), 1);
    let r = &batch.renders[0];
    let d = ((r.end_x - 500.0).powi(2) + (r.end_y - 100.0).powi(2)).sqrt();
    assert!(
        (d - 100.0).abs() < 0.05,
        "portal 端点的可见边界是圆周, d={} (end=({},{}))",
        d,
        r.end_x,
        r.end_y
    );
}
