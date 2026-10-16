//! Issue #902 评论 6206822408 — 跨画布连线剩余的两种真实拓扑经 `WriterCoreApi` 验证。
//!
//! #902 要求"跨画布语义连线连接真实节点对象"，并给出根图 A 有 Embed `e1→B`、
//! `e2→C` 的典型例子（见实现路线第一条评论）：
//! - a→b：同层端点
//! - b→c：**兄弟画布**之间的连线
//!
//! 前几轮分别覆盖了父→子、同一子图多 Embed 实例、Portal 穿行、菱形多父图 Embed。
//! 本测试补齐两条此前只在领域/单元层验证、未走 `WriterCoreApi` 真实 Store 的路径：
//!
//! 1. **指向整张子画布（`Starmap` 端点）**：`to.target = Starmap`、route 经 `eB` 进入
//!    B。宿主是 A；`to.segments` 记录 `eB`。反查时依赖 `CanvasEndpoint` 事实，从子画布
//!    的 canvas / Starmap target 都能命中；按 `Node(b1)` 反查则不应命中（终点身份不同）。
//!    删除这条跨画布边后，反向依赖索引必须同步失效，子画布不再反查到它。
//! 2. **兄弟画布直连** `b→c`：宿主是最长公共前缀对应的 A，`from/to.segments` 分别是
//!    各自的 Embed 实例；B、C 画布都只看到与自己这一端相关的那一次连线；根画布 A 因为
//!    两端都不落在此层而不显示；按对端对象反查不会串用另一条进入路径。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tempfile::TempDir;
use writer_core::api::types::{
    StarMapEdgeKindDto, StarMapEndpointSelectionDto, StarMapNavigationRouteDto, StarMapNodeDto,
    StarMapNodeKindDto, StarMapPathSegmentDto, StarMapPointDto, StarMapTargetDetailDto,
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

fn starmap_target() -> StarMapTargetDetailDto {
    StarMapTargetDetailDto {
        kind: "starmap".to_string(),
        ..Default::default()
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

/// 指向整张子画布（Starmap 端点）：宿主正确、子画布能反查、删除后索引同步失效。
#[test]
fn edge_to_whole_child_canvas_is_queryable_and_delete_clears_reverse_index() {
    let (_tmp, api) = make_api();

    let root = api.create_starmap("根图", "", None).unwrap().starmap_id;
    let child = api
        .create_starmap_child_embed(&root, "子图", point(0.0, 0.0))
        .unwrap();
    let child_id = child.starmap.starmap_id.clone();
    let instance_id = child.embed.instance_id.clone();

    api.add_starmap_node(&root, node("a1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&child_id, node("b1"), 0.0, 0.0)
        .unwrap();

    // from：根画布节点；to：整张子画布（Starmap 端点）。
    let from = selection(route(&root, vec![]), node_target("a1"));
    let to = selection(
        route(&root, vec![enter_embed(&instance_id)]),
        starmap_target(),
    );

    let created = api
        .create_cross_canvas_edge(from, to, StarMapEdgeKindDto::References, None, None)
        .unwrap();

    assert_eq!(created.owner_starmap_id, root);
    assert_eq!(created.edge.from.starmap_id, root);
    assert!(created.edge.from.segments.is_empty());
    assert_eq!(created.edge.from.target, node_target("a1"));
    assert_eq!(created.edge.to.starmap_id, root);
    assert_eq!(
        created.edge.to.segments,
        vec![enter_embed(&instance_id)],
        "指向整张子画布的穿越路径必须写进 to.segments"
    );
    assert_eq!(
        created.edge.to.target,
        starmap_target(),
        "to 端终点身份是整张画布，不是某个节点"
    );

    // 只在宿主图 A 写一条真实 Edge；子画布不写假边。
    let root_graph = api.get_starmap_graph(&root).unwrap();
    assert_eq!(root_graph.edges.len(), 1);
    assert_eq!(root_graph.edges[0].id, created.edge.id);
    assert!(api.get_starmap_graph(&child_id).unwrap().edges.is_empty());

    let child_route = route(&root, vec![enter_embed(&instance_id)]);

    // 子画布 canvas 反查：依赖 CanvasEndpoint 事实，不能漏掉 Starmap 终点的边。
    let child_canvas = api
        .list_starmap_connections(child_route.clone(), None)
        .unwrap();
    assert!(
        child_canvas.iter().any(|c| c.edge_id == created.edge.id),
        "子画布应能看到指向整张画布的跨层边"
    );

    // 按 Starmap 终点精确反查命中。
    let child_starmap = api
        .list_starmap_connections(child_route.clone(), Some(starmap_target()))
        .unwrap();
    assert!(
        child_starmap.iter().any(|c| c.edge_id == created.edge.id),
        "按 Starmap target 应命中整张画布终点的边"
    );

    // 按 Node 终点反查不应命中：终点身份不同，不能因为落在同一画布就串。
    let child_node = api
        .list_starmap_connections(child_route.clone(), Some(node_target("b1")))
        .unwrap();
    assert!(
        child_node.iter().all(|c| c.edge_id != created.edge.id),
        "终点是整张画布的边不应在按 Node(b1) 反查时出现"
    );

    // 删除跨画布边后，反向依赖索引必须同步失效。
    assert!(
        api.delete_starmap_edge(&root, &created.edge.id).unwrap(),
        "删除宿主图中的跨画布边应返回 true"
    );
    assert!(api.get_starmap_graph(&root).unwrap().edges.is_empty());

    let after_delete = api.list_starmap_connections(child_route, None).unwrap();
    assert!(
        after_delete.iter().all(|c| c.edge_id != created.edge.id),
        "删除后子画布不应再反查到已删除的跨画布边"
    );
}

/// 兄弟画布直连：宿主是公共前缀对应的 A，各端只落在自己的进入画布。
#[test]
fn sibling_canvas_edge_is_hosted_by_root_and_isolated_per_instance() {
    let (_tmp, api) = make_api();

    let a = api.create_starmap("A", "", None).unwrap().starmap_id;
    let b_child = api
        .create_starmap_child_embed(&a, "B", point(0.0, 0.0))
        .unwrap();
    let b = b_child.starmap.starmap_id.clone();
    let eb = b_child.embed.instance_id.clone();
    let c_child = api
        .create_starmap_child_embed(&a, "C", point(0.0, 0.0))
        .unwrap();
    let c = c_child.starmap.starmap_id.clone();
    let ec = c_child.embed.instance_id.clone();

    api.add_starmap_node(&a, node("a1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&b, node("b1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&c, node("c1"), 0.0, 0.0).unwrap();

    let via_b = route(&a, vec![enter_embed(&eb)]);
    let via_c = route(&a, vec![enter_embed(&ec)]);

    let created = api
        .create_cross_canvas_edge(
            selection(via_b.clone(), node_target("b1")),
            selection(via_c.clone(), node_target("c1")),
            StarMapEdgeKindDto::RelatedTo,
            None,
            None,
        )
        .unwrap();

    assert_eq!(created.owner_starmap_id, a, "兄弟画布连线宿主是公共前缀 A");
    assert_eq!(created.edge.from.segments, vec![enter_embed(&eb)]);
    assert_eq!(created.edge.to.segments, vec![enter_embed(&ec)]);

    // 只有 A 写一条真实 Edge，两个兄弟画布都不写假边。
    let a_graph = api.get_starmap_graph(&a).unwrap();
    assert_eq!(a_graph.edges.len(), 1);
    assert_eq!(a_graph.edges[0].id, created.edge.id);
    assert!(api.get_starmap_graph(&b).unwrap().edges.is_empty());
    assert!(api.get_starmap_graph(&c).unwrap().edges.is_empty());

    // 各自进入画布都能反查到这条跨兄弟画布的边。
    let seen_via_b = api.list_starmap_connections(via_b.clone(), None).unwrap();
    assert!(
        seen_via_b.iter().any(|c| c.edge_id == created.edge.id),
        "经 A/[eB] 进入 B 应看到 from 端在本层的边"
    );
    let seen_via_c = api.list_starmap_connections(via_c.clone(), None).unwrap();
    assert!(
        seen_via_c.iter().any(|c| c.edge_id == created.edge.id),
        "经 A/[eC] 进入 C 应看到 to 端在本层的边"
    );

    // 根画布 A 是这条边的宿主，canvas 查询按约定包含本图全部 Edge。
    let root_canvas = api
        .list_starmap_connections(route(&a, vec![]), None)
        .unwrap();
    assert!(
        root_canvas
            .iter()
            .any(|c| c.edge_id == created.edge.id && c.owner_starmap_id == a),
        "宿主图 canvas 查询应包含本图 Edge"
    );

    // 但按深层目标反查时 route-aware 过滤必须生效：根画布不能把 b1/c1 当作本层对象。
    let root_looking_for_b1 = api
        .list_starmap_connections(route(&a, vec![]), Some(node_target("b1")))
        .unwrap();
    assert!(
        root_looking_for_b1
            .iter()
            .all(|c| c.edge_id != created.edge.id),
        "根画布按 b1 反查不应命中端点穿越 eB 落在更深画布的边"
    );
    let root_looking_for_c1 = api
        .list_starmap_connections(route(&a, vec![]), Some(node_target("c1")))
        .unwrap();
    assert!(
        root_looking_for_c1
            .iter()
            .all(|c| c.edge_id != created.edge.id),
        "根画布按 c1 反查不应命中端点穿越 eC 落在更深画布的边"
    );

    // 按对端对象反查：B 视角按 c1 查不到（c1 不在 B），C 视角按 c1 查得到。
    let b_looking_for_c1 = api
        .list_starmap_connections(via_b.clone(), Some(node_target("c1")))
        .unwrap();
    assert!(
        b_looking_for_c1
            .iter()
            .all(|c| c.edge_id != created.edge.id),
        "B 视角按 c1 反查不应命中 to 端在 C 的边"
    );
    let c_looking_for_c1 = api
        .list_starmap_connections(via_c.clone(), Some(node_target("c1")))
        .unwrap();
    assert!(
        c_looking_for_c1
            .iter()
            .any(|c| c.edge_id == created.edge.id),
        "C 视角按 c1 反查应命中 to 端在本层的边"
    );
}
