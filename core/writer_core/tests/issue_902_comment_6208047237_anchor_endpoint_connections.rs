//! Issue #902 评论 6208047237 — 以 **Anchor** 为端点的跨画布连线与删除保护。
//!
//! #902 实现路线第 2 节明确跨画布边的端点目标可以是 `Node / Anchor / Starmap`。
//! 此前几轮已覆盖 Node 端点（父/子、兄弟、多父图菱形、整张子画布）与 Starmap 端点，
//! 但 `Anchor` 端点的真实语义尚未经 `WriterCoreApi` 验证。本测试构造
//! `R → Embed(eB) → B`，在 B 的节点 b1 上挂锚点 a1，并创建一条存于 R、
//! 经 `[eB]` 落在该 Anchor 的跨画布边：
//!
//! 1. 唯一真实 Edge 写进最长公共宿主图 R，`to.segments = [eB]`、`to.target`
//!    是 Anchor；B 不写假边。
//! 2. 在 B 反查能命中该边：canvas 查询与 `Some(Anchor)` 查询都命中，而
//!    `Some(Node(b1))` 查询不得把 Anchor 端点误当成 Node 端点。
//! 3. 删除锚点 a1 会破坏外图 R 持有的路径依赖（`AnchorTarget`），必须被
//!    拒绝并说明 `external dependencies would dangle`；删除原始边后允许移除锚点。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tempfile::TempDir;
use writer_core::api::types::{
    StarMapAnchorDto, StarMapAnchorRoleDto, StarMapAnchorTargetDto, StarMapEdgeKindDto,
    StarMapEndpointSelectionDto, StarMapNavigationRouteDto, StarMapNodeDto, StarMapNodeKindDto,
    StarMapNodePatchDto, StarMapPathSegmentDto, StarMapPointDto, StarMapTargetDetailDto,
};
use writer_core::api::WriterCoreApi;

fn make_api() -> (TempDir, WriterCoreApi) {
    let tmp = tempfile::tempdir().unwrap();
    let app_data_root = tmp.path().to_path_buf();
    let projects_root = app_data_root.join("projects");
    std::fs::create_dir_all(&projects_root).unwrap();
    let api = WriterCoreApi::new(&app_data_root, &projects_root);
    (tmp, api)
}

fn point(x: f64, y: f64) -> StarMapPointDto {
    StarMapPointDto { x, y }
}

fn node(id: &str) -> StarMapNodeDto {
    StarMapNodeDto {
        id: id.to_string(),
        title: id.to_string(),
        kind: StarMapNodeKindDto::Note,
        payload: None,
        tags: vec![],
        content: Default::default(),
        anchors: vec![],
        portal: None,
        position: point(0.0, 0.0),
        style: Default::default(),
        provenance: Default::default(),
        created_at: 0,
        updated_at: 0,
    }
}

fn node_target(id: &str) -> StarMapTargetDetailDto {
    StarMapTargetDetailDto {
        kind: "node".to_string(),
        node_id: Some(id.to_string()),
        ..Default::default()
    }
}

fn anchor_target(node_id: &str, anchor_id: &str) -> StarMapTargetDetailDto {
    StarMapTargetDetailDto {
        kind: "anchor".to_string(),
        node_id: Some(node_id.to_string()),
        anchor_id: Some(anchor_id.to_string()),
        ..Default::default()
    }
}

fn anchor(anchor_id: &str, starmap_id: &str) -> StarMapAnchorDto {
    StarMapAnchorDto {
        anchor_id: anchor_id.to_string(),
        target: StarMapAnchorTargetDto {
            kind: "starmap".to_string(),
            starmap_id: Some(starmap_id.to_string()),
            ..Default::default()
        },
        label: None,
        role: StarMapAnchorRoleDto::Destination,
    }
}

fn patch_anchors(anchors: Vec<StarMapAnchorDto>) -> StarMapNodePatchDto {
    StarMapNodePatchDto {
        title: None,
        kind: None,
        payload: None,
        tags: None,
        content: None,
        anchors: Some(anchors),
        portal: None,
        position: None,
        style: None,
        provenance: None,
    }
}

fn route(root: &str, segments: Vec<StarMapPathSegmentDto>) -> StarMapNavigationRouteDto {
    StarMapNavigationRouteDto {
        root_starmap_id: root.to_string(),
        segments,
    }
}

fn enter_embed(instance_id: &str) -> StarMapPathSegmentDto {
    StarMapPathSegmentDto {
        kind: "enterEmbed".to_string(),
        instance_id: Some(instance_id.to_string()),
        node_id: None,
    }
}

fn selection(
    route: StarMapNavigationRouteDto,
    target: StarMapTargetDetailDto,
) -> StarMapEndpointSelectionDto {
    StarMapEndpointSelectionDto { route, target }
}

/// Anchor 端点：真实边落宿主、反查可命中、被引用锚点删除受保护。
#[test]
fn cross_canvas_edge_to_anchor_blocks_anchor_removal_until_edge_removed() {
    let (_tmp, api) = make_api();

    let r = api.create_starmap("根图", "", None).unwrap().starmap_id;
    let b_child = api
        .create_starmap_child_embed(&r, "子图", point(0.0, 0.0))
        .unwrap();
    let b = b_child.starmap.starmap_id.clone();
    let eb = b_child.embed.instance_id.clone();

    api.add_starmap_node(&r, node("r1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&b, node("b1"), 0.0, 0.0).unwrap();

    // 在 b1 上挂锚点 a1，目标指向子画布 B 自身（合法 starmap 锚点）。
    let updated = api
        .update_starmap_node(&b, "b1", patch_anchors(vec![anchor("a1", &b)]))
        .unwrap();
    assert_eq!(updated.anchors.len(), 1, "锚点应写入 b1");
    assert_eq!(updated.anchors[0].anchor_id, "a1");

    // 存于 R 的跨画布边：from = R/[]/Node(r1)，to = R/[eB]/Anchor(b1, a1)。
    let created = api
        .create_cross_canvas_edge(
            selection(route(&r, vec![]), node_target("r1")),
            selection(route(&r, vec![enter_embed(&eb)]), anchor_target("b1", "a1")),
            StarMapEdgeKindDto::References,
            None,
            None,
        )
        .unwrap();

    assert_eq!(
        created.owner_starmap_id, r,
        "两端公共前缀为空，宿主是根图 R"
    );
    assert_eq!(
        created.edge.to.segments,
        vec![enter_embed(&eb)],
        "穿过子画布入口的进入路径必须写入 to.segments"
    );
    assert_eq!(
        created.edge.to.target,
        anchor_target("b1", "a1"),
        "to.target 必须保留 Anchor 端点身份"
    );

    // 真实边只落在 R，B 不写假边。
    let r_graph = api.get_starmap_graph(&r).unwrap();
    assert_eq!(r_graph.edges.len(), 1, "宿主图必须只写入一条 Edge");
    assert_eq!(r_graph.edges[0].id, created.edge.id);
    assert!(
        api.get_starmap_graph(&b).unwrap().edges.is_empty(),
        "子画布不应写入伪造的本地 Edge"
    );

    // B 画布反查（canvas）：存于祖先图 R、落在当前画布的边必须出现。
    let b_canvas = api
        .list_starmap_connections(route(&r, vec![enter_embed(&eb)]), None)
        .unwrap();
    assert!(
        b_canvas.iter().any(|c| c.edge_id == created.edge.id),
        "B 画布应反查到存于 R 的 Anchor 端点边"
    );

    // 按 Anchor 精确反查：应命中。
    let b_anchor = api
        .list_starmap_connections(
            route(&r, vec![enter_embed(&eb)]),
            Some(anchor_target("b1", "a1")),
        )
        .unwrap();
    assert!(
        b_anchor.iter().any(|c| c.edge_id == created.edge.id),
        "按 Anchor target 应命中该边"
    );

    // 按 Node(b1) 查询：Anchor 端点不应被误当成 Node 端点。
    let b_node = api
        .list_starmap_connections(route(&r, vec![enter_embed(&eb)]), Some(node_target("b1")))
        .unwrap();
    assert!(
        b_node.iter().all(|c| c.edge_id != created.edge.id),
        "Anchor 端点不应被 Node 端点查询命中"
    );

    // 题干：外图边指向这个锚点 → 移除锚点必须被拒绝。
    let error = api
        .update_starmap_node(&b, "b1", patch_anchors(vec![]))
        .expect_err("被外图边引用的锚点不得被移除");
    let message = format!("{error:?}");
    assert!(
        message.contains("external dependencies would dangle"),
        "必须以外部依赖悬空为由拒绝；实际: {message}"
    );
    assert!(
        message.contains("AnchorTarget(anchor=a1)"),
        "必须指出是锚点 a1 的外部依赖；实际: {message}"
    );

    // 被拒绝的更新不得实际移除锚点。
    let b_graph = api.get_starmap_graph(&b).unwrap();
    let b1 = b_graph
        .nodes
        .iter()
        .find(|n| n.id == "b1")
        .expect("b1 必须仍在 B");
    assert!(
        b1.anchors.iter().any(|a| a.anchor_id == "a1"),
        "被拒绝后 b1 必须仍保留锚点 a1"
    );

    // 删除 R 中的原始边，解除对 a1 的外部引用。
    assert!(
        api.delete_starmap_edge(&r, &created.edge.id).unwrap(),
        "删除宿主图中的跨画布边应返回 true"
    );
    assert!(api.get_starmap_graph(&r).unwrap().edges.is_empty());

    // 解除外部引用后，允许移除锚点。
    let after = api
        .update_starmap_node(&b, "b1", patch_anchors(vec![]))
        .unwrap();
    assert!(after.anchors.is_empty(), "解除外部引用后应允许移除锚点");
}
