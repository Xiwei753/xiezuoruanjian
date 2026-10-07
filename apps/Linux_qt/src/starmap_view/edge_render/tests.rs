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

// ─────────────────────────────────────────────────────────────────────
// 评论 5977278030: 拉线候选边预览复用正式 edge renderer。
//
// 已有反向边时正式边会进入双向模式并整体垂直偏移 12 world；
// 预览必须调用同一份 renderer（候选边临时追加进宿主图边表），
// 而不是在 QML 里再抄一份中线/偏移，否则松手瞬间会整体平移。
// ─────────────────────────────────────────────────────────────────────

fn local_path(node_id: &str) -> StarMapTargetPath {
    StarMapTargetPath {
        starmap_id: "map_1".to_string(),
        segments: vec![],
        target: StarMapTargetDetail::Node {
            node_id: node_id.to_string(),
        },
    }
}

fn local_edge(id: &str, from: &str, to: &str) -> StarMapEdge {
    StarMapEdge {
        id: id.to_string(),
        from: local_path(from),
        to: local_path(to),
        kind: StarMapEdgeKind::RelatedTo,
        label: None,
        payload: None,
        created_at: 0,
        updated_at: 0,
    }
}

/// 两个普通节点 A(0,0) / B(400,0)，layout 都是 150×60。
fn graph_with_two_nodes(edges: Vec<StarMapEdge>) -> StarMapGraph {
    StarMapGraph {
        starmap_id: "map_1".to_string(),
        nodes: vec![node("note_a", 0.0), node("note_b", 400.0)],
        edges,
        ..StarMapGraph::default()
    }
}

fn assert_render_matches(actual: &EdgeRender, expected: &EdgeRender, what: &str) {
    for (name, a, b) in [
        ("start_x", actual.start_x, expected.start_x),
        ("start_y", actual.start_y, expected.start_y),
        ("end_x", actual.end_x, expected.end_x),
        ("end_y", actual.end_y, expected.end_y),
    ] {
        assert!(
            (a - b).abs() < 0.01,
            "{what}: {name} 预览/正式必须一致, actual={a}, expected={b}"
        );
    }
}

/// 已有 A→B 时从 B 拉到 A：候选边预览 = 两条正式边都在时的 B→A
/// （双向模式 + 垂直偏移），不是未偏移中线。
#[test]
fn prospective_preview_matches_formal_edge_when_reverse_exists() {
    let layout = layout_with(&[("note_a", 0.0), ("note_b", 400.0)]);
    let params = EdgeRenderParams::default();
    let graph = graph_with_two_nodes(vec![local_edge("edge_ab", "note_a", "note_b")]);

    // 正式：提交 B→A 之后两条边都在。
    let mut formal_graph = graph.clone();
    formal_graph
        .edges
        .push(local_edge("edge_ba", "note_b", "note_a"));
    let batch =
        compute_edge_renders_from_paths(&formal_graph.edges, &formal_graph, &layout, &[], &params);
    assert!(batch.diagnostics.is_empty(), "{:?}", batch.diagnostics);
    let formal_ba = batch
        .renders
        .iter()
        .find(|r| r.edge_id == "edge_ba")
        .expect("正式 B→A 必须渲染");

    // 预览：只把候选边临时追加进现有边表。
    let preview = compute_prospective_edge_render(
        &local_path("note_b"),
        &local_path("note_a"),
        &graph,
        &layout,
        &[],
        &params,
    )
    .expect("候选边预览必须可解析");

    assert!(
        preview.has_bidirectional,
        "已有反向边时候选边必须进入双向模式"
    );
    assert_render_matches(&preview, formal_ba, "已有反向边");
    assert!(
        (preview.start_y - 18.0).abs() < 0.01 && (preview.end_y - 18.0).abs() < 0.01,
        "偏移后的 B→A 应画在 y=18，实际 y=({},{})",
        preview.start_y,
        preview.end_y
    );
    assert!(
        (preview.start_y - 30.0).abs() > 1.0,
        "预览不得停在中线 y=30（旧实现松手会跳 12 world）"
    );
}

/// 没有反向边时候选边预览 = 未偏移中线（不能无条件加偏移）。
#[test]
fn prospective_preview_stays_on_midline_without_reverse_edge() {
    let layout = layout_with(&[("note_a", 0.0), ("note_b", 400.0)]);
    let params = EdgeRenderParams::default();
    let graph = graph_with_two_nodes(vec![]);

    let preview = compute_prospective_edge_render(
        &local_path("note_b"),
        &local_path("note_a"),
        &graph,
        &layout,
        &[],
        &params,
    )
    .expect("候选边预览必须可解析");
    assert!(!preview.has_bidirectional);
    assert!(
        (preview.start_y - 30.0).abs() < 0.01 && (preview.end_y - 30.0).abs() < 0.01,
        "无反向边时预览应贴中线 y=30, 实际 y=({},{})",
        preview.start_y,
        preview.end_y
    );
}

/// 端点无法在宿主图定位时返回诊断，不静默给出错误几何。
#[test]
fn prospective_preview_reports_diagnostic_for_missing_endpoint() {
    let layout = layout_with(&[("note_a", 0.0)]);
    let params = EdgeRenderParams::default();
    let graph = graph_with_two_nodes(vec![]);

    let err = compute_prospective_edge_render(
        &local_path("note_a"),
        &local_path("note_missing"),
        &graph,
        &layout,
        &[],
        &params,
    )
    .expect_err("缺失端点必须返回诊断");
    assert_eq!(err.edge_id, PROSPECTIVE_EDGE_ID);
    assert_eq!(err.endpoint, "to");
    assert_eq!(err.reason, EdgeAnchorDiagnosticReason::LocalNodeMissing);
}

// ─────────────────────────────────────────────────────────────────────
// 评论 5977879544: 画出来的箭头也是边 —— 命中必须覆盖箭头三角形
// ─────────────────────────────────────────────────────────────────────

/// 箭头内部、同时离箭杆 > threshold 的点，必须在很小的 threshold 下仍然命中。
/// 只测箭杆的旧实现会漏掉这些点（"点在箭头上却点不中"）。
#[test]
fn hit_test_edge_render_covers_arrow_triangle() {
    let mut r = make_render("e1", (0.0, 0.0), (100.0, 0.0));
    // 真实箭头几何：tip=(100,0)，长 10、半角 30° → left=(90,5), right=(90,-5)
    r.arrow_tip_x = 100.0;
    r.arrow_tip_y = 0.0;
    r.arrow_left_x = 90.0;
    r.arrow_left_y = 5.0;
    r.arrow_right_x = 90.0;
    r.arrow_right_y = -5.0;
    let renders = std::slice::from_ref(&r);

    // (91.5,2.5)：三角形内部，且到三条箭头边和箭杆都 > 1.0 world。
    let shaft_only = point_to_segment_distance(91.5, 2.5, 0.0, 0.0, 100.0, 0.0);
    assert!(
        shaft_only > 1.0,
        "该点必须离箭杆 > threshold, got {}",
        shaft_only
    );
    assert!(
        !point_in_triangle(91.5, 2.5, 100.0, 0.0, 90.0, 5.0, 90.0, -5.0) == false,
        "前提：该点确实在箭头三角形内部"
    );
    assert_eq!(
        hit_test_edge_renders_with_threshold(91.5, 2.5, renders, 1.0).as_deref(),
        Some("e1"),
        "点在箭头内部时必须命中（只测箭杆会漏）"
    );

    // (89.4,3.0)：三角形外侧，但距箭头底边 0.6 world < threshold —— 统一 slop 同样命中。
    assert!(
        point_to_segment_distance(89.4, 3.0, 90.0, -5.0, 90.0, 5.0) < 1.0,
        "前提：该点离箭头底边 < threshold"
    );
    assert_eq!(
        hit_test_edge_renders_with_threshold(89.4, 3.0, renders, 1.0).as_deref(),
        Some("e1"),
        "箭头边附近的点必须命中"
    );

    // (50,3)：离箭杆 3 world、离箭头很远 —— 不得命中。
    assert_eq!(
        hit_test_edge_renders_with_threshold(50.0, 3.0, renders, 1.0),
        None,
        "没有画出来的空域不能被算进命中"
    );
}

/// 退化箭头（三个顶点重合）不能让整个平面都算命中。
#[test]
fn hit_test_edge_render_degenerate_arrow_does_not_cover_plane() {
    let renders = vec![make_render("e1", (0.0, 0.0), (200.0, 0.0))];
    // make_render 的箭头三点都等于终点 (200,0)。
    assert_eq!(
        hit_test_edge_renders_with_threshold(100.0, 20.0, &renders, 10.0),
        None,
        "退化箭头不得覆盖平面"
    );
    assert_eq!(
        hit_test_edge_renders_with_threshold(200.0, 5.0, &renders, 10.0).as_deref(),
        Some("e1"),
        "终点附近仍在 threshold 内命中"
    );
}

/// 端到端：正式边渲染出的箭头三角形同样可点（高倍缩放的 threshold=1 world）。
#[test]
fn formal_edge_arrow_is_clickable_at_small_threshold() {
    let graph = StarMapGraph {
        starmap_id: "map_1".to_string(),
        nodes: vec![node("note_1", 0.0), node("note_2", 400.0)],
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
                segments: vec![],
                target: StarMapTargetDetail::Node {
                    node_id: "note_2".to_string(),
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
    let layout = layout_with(&[("note_1", 0.0), ("note_2", 400.0)]);
    let batch = compute_edge_renders_from_paths(
        &graph.edges,
        &graph,
        &layout,
        &[],
        &EdgeRenderParams::default(),
    );
    assert!(batch.diagnostics.is_empty(), "{:?}", batch.diagnostics);
    assert_eq!(batch.renders.len(), 1);
    let r = &batch.renders[0];
    // 箭头内部一点：重心与上翼边中点的中点（严格在三角形内，且离箭杆 ≈1.25 world）。
    let cx = (r.arrow_tip_x + r.arrow_left_x + r.arrow_right_x) / 3.0;
    let cy = (r.arrow_tip_y + r.arrow_left_y + r.arrow_right_y) / 3.0;
    let inside_x = (cx + (r.arrow_tip_x + r.arrow_left_x) / 2.0) / 2.0;
    let inside_y = (cy + (r.arrow_tip_y + r.arrow_left_y) / 2.0) / 2.0;
    assert!(
        point_in_triangle(
            inside_x,
            inside_y,
            r.arrow_tip_x,
            r.arrow_tip_y,
            r.arrow_left_x,
            r.arrow_left_y,
            r.arrow_right_x,
            r.arrow_right_y
        ),
        "前提：取样点必须落在箭头三角形内部"
    );
    let shaft_only =
        point_to_segment_distance(inside_x, inside_y, r.start_x, r.start_y, r.end_x, r.end_y);
    assert!(
        shaft_only > 1.0,
        "前提：该点离箭杆 > 1 world（got {}）",
        shaft_only
    );
    assert_eq!(
        hit_test_edge_renders_with_threshold(inside_x, inside_y, &batch.renders, 1.0).as_deref(),
        Some("edge_1"),
        "正式边箭头内部在小 threshold 下也必须命中"
    );
}

/// JSON 全链路：QML 走的正是 compute_edge_renders_json → hit_test_edge_renders_json，
/// 箭头命中必须在 serde camelCase 边界上同样成立。
#[test]
fn json_bridge_hits_arrow_triangle_with_small_threshold() {
    use crate::starmap_view::bridge::{compute_edge_renders_json, hit_test_edge_renders_json};

    let graph = StarMapGraph {
        starmap_id: "map_1".to_string(),
        nodes: vec![node("note_1", 0.0), node("note_2", 400.0)],
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
                segments: vec![],
                target: StarMapTargetDetail::Node {
                    node_id: "note_2".to_string(),
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
    let nodes_json = r#"[
        {"id":"note_1","x":0.0,"y":0.0,"width":150.0,"height":60.0},
        {"id":"note_2","x":400.0,"y":0.0,"width":150.0,"height":60.0}
    ]"#;
    let envelope = compute_edge_renders_json(&graph, nodes_json, "[]");
    assert!(
        envelope.contains("\"arrowTipX\""),
        "render JSON 必须是 camelCase: {envelope}"
    );
    // QML 从 envelope 里取 data 数组后原样传给命中接口。
    let envelope_json: serde_json::Value =
        serde_json::from_str(&envelope).expect("compute_edge_renders_json 必须是合法 JSON");
    let renders_json = serde_json::to_string(
        envelope_json
            .get("data")
            .expect("envelope 必须带 data 字段"),
    )
    .expect("renders 序列化");

    // 箭头内部一点（离箭杆 ≈1.25 world），threshold 用放大后的 1 world。
    let hit = hit_test_edge_renders_json(renders_json.as_str(), 394.95, 31.25, 1.0);
    assert!(
        hit.contains("edge_1"),
        "JSON 全链路上箭头内部也必须可命中: {hit}"
    );

    // 同一 x、离箭杆 3 world 且离箭头 > 1 world：不得命中。
    let miss = hit_test_edge_renders_json(renders_json.as_str(), 380.0, 33.0, 1.0);
    assert!(
        miss.contains("null") || !miss.contains("edge_1"),
        "箭头覆盖之外不得命中: {miss}"
    );
}
