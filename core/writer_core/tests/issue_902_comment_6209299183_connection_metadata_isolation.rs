//! Issue #902 评论 6209299183 — 跨画布连线返回元数据与多入口隔离集成测试。
//!
//! #902 要求"关联反查"返回 `{ owner_starmap_id, edge_id, from, to, kind, label }`，
//! 并且"读取不同入口不会重复制造关系"。既有测试已经覆盖了跨画布边的宿主选择、
//! 删除保护与实例路径隔离，但缺少对以下两点的真实 `WriterCoreApi` 断言：
//!
//! 1. `create_cross_canvas_edge` 写入的 `kind` / `label` 必须原样持久化到宿主图，
//!    并由 `list_starmap_connections` 在 canvas / target 两种查询里原样返回；
//! 2. 同一宿主图里的多条跨画布边在按不同入口反查时——只看得到与当前实例路径
//!    匹配的边，且每条边最多出现一次，不会因为候选祖先集合是保守上界而重复。
//!
//! 拓扑：根图 R 有节点 `n_root`，经 Embed `e_b` 进入子图 B（节点 `b1`），
//! 经 Embed `e_c` 进入子图 C（节点 `c1`）。R 中建立两条跨画布边：
//! - `n_root → B/[e_b]/b1`，kind=`Foreshadows`，label=`伏笔`；
//! - `n_root → C/[e_c]/c1`，kind=`ConflictsWith`，无 label。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tempfile::TempDir;
use writer_core::api::types::{
    StarMapConnectionInfoDto, StarMapEdgeKindDto, StarMapEndpointSelectionDto,
    StarMapNavigationRouteDto, StarMapNodeDto, StarMapNodeKindDto, StarMapPathSegmentDto,
    StarMapPointDto, StarMapTargetDetailDto,
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

fn enter_embed(instance_id: &str) -> StarMapPathSegmentDto {
    StarMapPathSegmentDto {
        kind: "enterEmbed".to_string(),
        instance_id: Some(instance_id.to_string()),
        node_id: None,
    }
}

fn route(root: &str, segments: Vec<StarMapPathSegmentDto>) -> StarMapNavigationRouteDto {
    StarMapNavigationRouteDto {
        root_starmap_id: root.to_string(),
        segments,
    }
}

fn selection(
    route: StarMapNavigationRouteDto,
    target: StarMapTargetDetailDto,
) -> StarMapEndpointSelectionDto {
    StarMapEndpointSelectionDto { route, target }
}

struct Fixture {
    root: String,
    child_b: String,
    child_c: String,
    e_b: String,
    e_c: String,
    foreshadows_edge: String,
    conflicts_edge: String,
}

fn setup() -> (TempDir, WriterCoreApi, Fixture) {
    let (tmp, api) = make_api();

    let root = api.create_starmap("根图", "", None).unwrap().starmap_id;
    let b = api
        .create_starmap_child_embed(&root, "子图B", point(0.0, 0.0))
        .unwrap();
    let c = api
        .create_starmap_child_embed(&root, "子图C", point(0.0, 0.0))
        .unwrap();
    let child_b = b.starmap.starmap_id.clone();
    let child_c = c.starmap.starmap_id.clone();
    let e_b = b.embed.instance_id.clone();
    let e_c = c.embed.instance_id.clone();

    api.add_starmap_node(&root, node("n_root"), 0.0, 0.0)
        .unwrap();
    api.add_starmap_node(&child_b, node("b1"), 0.0, 0.0)
        .unwrap();
    api.add_starmap_node(&child_c, node("c1"), 0.0, 0.0)
        .unwrap();

    let foreshadows = api
        .create_cross_canvas_edge(
            selection(route(&root, vec![]), node_target("n_root")),
            selection(route(&root, vec![enter_embed(&e_b)]), node_target("b1")),
            StarMapEdgeKindDto::Foreshadows,
            Some("伏笔".to_string()),
            None,
        )
        .unwrap();

    let conflicts = api
        .create_cross_canvas_edge(
            selection(route(&root, vec![]), node_target("n_root")),
            selection(route(&root, vec![enter_embed(&e_c)]), node_target("c1")),
            StarMapEdgeKindDto::ConflictsWith,
            None,
            None,
        )
        .unwrap();

    let fixture = Fixture {
        root,
        child_b,
        child_c,
        e_b,
        e_c,
        foreshadows_edge: foreshadows.edge.id,
        conflicts_edge: conflicts.edge.id,
    };
    (tmp, api, fixture)
}

/// 在反查结果里按 `edge_id` 精确定位一条连接，找不到即失败。
fn find_connection<'a>(
    conns: &'a [StarMapConnectionInfoDto],
    edge_id: &str,
    msg: &str,
) -> &'a StarMapConnectionInfoDto {
    conns.iter().find(|c| c.edge_id == edge_id).expect(msg)
}

/// 宿主图 R 持久化的边元数据，以及“不得向子图复制假边”的约束。
fn assert_host_graph_metadata(api: &WriterCoreApi, fx: &Fixture) {
    // 两条边都落在根图 R，且 kind/label 原样保存。
    let root_graph = api.get_starmap_graph(&fx.root).unwrap();
    assert_eq!(root_graph.edges.len(), 2, "根图应恰好持有两条跨画布边");

    let stored_foreshadows = root_graph
        .edges
        .iter()
        .find(|e| e.id == fx.foreshadows_edge)
        .expect("宿主图应存在 Foreshadows 边");
    assert_eq!(stored_foreshadows.kind, StarMapEdgeKindDto::Foreshadows);
    assert_eq!(stored_foreshadows.label.as_deref(), Some("伏笔"));

    let stored_conflicts = root_graph
        .edges
        .iter()
        .find(|e| e.id == fx.conflicts_edge)
        .expect("宿主图应存在 ConflictsWith 边");
    assert_eq!(stored_conflicts.kind, StarMapEdgeKindDto::ConflictsWith);
    assert_eq!(stored_conflicts.label, None);

    // 跨画布边只落在最长公共宿主图 R，不得向子图复制假边。
    assert!(
        api.get_starmap_graph(&fx.child_b).unwrap().edges.is_empty(),
        "子图 B 不应写入伪造的本地边"
    );
    assert!(
        api.get_starmap_graph(&fx.child_c).unwrap().edges.is_empty(),
        "子图 C 不应写入伪造的本地边"
    );
}

/// 经子图 B 的入口与根画布 canvas 查询返回的连接元数据。
fn assert_canvas_query_metadata(api: &WriterCoreApi, fx: &Fixture) {
    // 经子图 B 的入口反查：只应看到 Foreshadows 边，元数据与宿主图一致。
    let via_b = api
        .list_starmap_connections(route(&fx.root, vec![enter_embed(&fx.e_b)]), None)
        .unwrap();
    let conn_b = find_connection(
        &via_b,
        &fx.foreshadows_edge,
        "经 e_b 进入子图 B 应反查到 Foreshadows 边",
    );
    assert_eq!(conn_b.owner_starmap_id, fx.root);
    assert_eq!(conn_b.kind, StarMapEdgeKindDto::Foreshadows);
    assert_eq!(conn_b.label.as_deref(), Some("伏笔"));
    assert_eq!(
        conn_b.to.segments,
        vec![enter_embed(&fx.e_b)],
        "连接信息必须保留原进入实例路径"
    );
    assert_eq!(conn_b.to.target, node_target("b1"));

    let root_canvas = api
        .list_starmap_connections(route(&fx.root, vec![]), None)
        .unwrap();
    let conn_conflicts = find_connection(
        &root_canvas,
        &fx.conflicts_edge,
        "根画布反查应包含 ConflictsWith 边",
    );
    assert_eq!(conn_conflicts.kind, StarMapEdgeKindDto::ConflictsWith);
    assert_eq!(conn_conflicts.label, None);

    // 无 label 的边不应被误判成有 label；有 label 的边不应丢失。
    let root_foreshadows = find_connection(
        &root_canvas,
        &fx.foreshadows_edge,
        "根画布反查应包含 Foreshadows 边",
    );
    assert_eq!(root_foreshadows.label.as_deref(), Some("伏笔"));
}

/// `target=Some(...)` 精确查询同样必须原样保留 kind / label / owner_starmap_id。
fn assert_target_query_metadata(api: &WriterCoreApi, fx: &Fixture) {
    let b_target = api
        .list_starmap_connections(
            route(&fx.root, vec![enter_embed(&fx.e_b)]),
            Some(node_target("b1")),
        )
        .unwrap();
    let b_conn = find_connection(&b_target, &fx.foreshadows_edge, "按 b1 查询应返回伏笔边");
    assert_eq!(b_conn.kind, StarMapEdgeKindDto::Foreshadows);
    assert_eq!(b_conn.label.as_deref(), Some("伏笔"));
    assert_eq!(b_conn.owner_starmap_id, fx.root);

    let c_target = api
        .list_starmap_connections(
            route(&fx.root, vec![enter_embed(&fx.e_c)]),
            Some(node_target("c1")),
        )
        .unwrap();
    let c_conn = find_connection(&c_target, &fx.conflicts_edge, "按 c1 查询应返回冲突边");
    assert_eq!(c_conn.kind, StarMapEdgeKindDto::ConflictsWith);
    assert_eq!(c_conn.label, None);
    assert_eq!(c_conn.owner_starmap_id, fx.root);
}

/// 宿主图持久化的边元数据（kind/label）与反查返回的连接信息完全一致。
#[test]
fn connection_info_preserves_kind_and_label_from_host_graph() {
    let (_tmp, api, fx) = setup();
    assert_host_graph_metadata(&api, &fx);
    assert_canvas_query_metadata(&api, &fx);
    assert_target_query_metadata(&api, &fx);
}

/// 不同入口的反查只返回与自身实例路径匹配的边，且任一查询中每条边最多出现一次。
#[test]
fn per_entry_queries_isolate_edges_and_never_duplicate() {
    let (_tmp, api, fx) = setup();

    let via_b = api
        .list_starmap_connections(route(&fx.root, vec![enter_embed(&fx.e_b)]), None)
        .unwrap();
    assert!(
        via_b.iter().all(|c| c.edge_id != fx.conflicts_edge),
        "经 e_b 进入 B 不应看到通往 C 的边"
    );
    assert_eq!(
        via_b
            .iter()
            .filter(|c| c.edge_id == fx.foreshadows_edge)
            .count(),
        1,
        "同一次反查中一条边只应出现一次"
    );

    let via_c = api
        .list_starmap_connections(route(&fx.root, vec![enter_embed(&fx.e_c)]), None)
        .unwrap();
    assert!(
        via_c.iter().all(|c| c.edge_id != fx.foreshadows_edge),
        "经 e_c 进入 C 不应看到通往 B 的边"
    );
    assert_eq!(
        via_c
            .iter()
            .filter(|c| c.edge_id == fx.conflicts_edge)
            .count(),
        1,
        "同一次反查中一条边只应出现一次"
    );

    // 按端点对象精确反查同样不串入口、不重复。
    let target_b = api
        .list_starmap_connections(
            route(&fx.root, vec![enter_embed(&fx.e_b)]),
            Some(node_target("b1")),
        )
        .unwrap();
    assert_eq!(
        target_b
            .iter()
            .filter(|c| c.edge_id == fx.foreshadows_edge)
            .count(),
        1,
        "按 b1 精确反查应恰好命中一次"
    );
    assert!(
        target_b.iter().all(|c| c.edge_id != fx.conflicts_edge),
        "按 b1 精确反查不应命中通往 C 的边"
    );

    // 根画布 canvas 查询按约定返回本图全部边，但仍不得重复。
    let root_canvas = api
        .list_starmap_connections(route(&fx.root, vec![]), None)
        .unwrap();
    let mut ids: Vec<&str> = root_canvas.iter().map(|c| c.edge_id.as_str()).collect();
    ids.sort_unstable();
    let unique: std::collections::BTreeSet<&str> = ids.iter().copied().collect();
    assert_eq!(
        ids.len(),
        unique.len(),
        "根画布反查结果不得包含重复 edge_id"
    );
    assert_eq!(root_canvas.len(), 2, "根画布应返回本图全部两条跨画布边");

    // 子图 C 的 canvas 查询仍能看到本层目标，验证隔离不是靠"隐藏所有"实现。
    let c_canvas = api
        .list_starmap_connections(route(&fx.root, vec![enter_embed(&fx.e_c)]), None)
        .unwrap();
    assert!(
        c_canvas.iter().any(|c| c.edge_id == fx.conflicts_edge),
        "经 e_c 进入 C 必须能看到通往 c1 的边"
    );
    assert!(
        c_canvas.iter().any(|c| c.owner_starmap_id == fx.root),
        "该边登记在根图 R"
    );
}
