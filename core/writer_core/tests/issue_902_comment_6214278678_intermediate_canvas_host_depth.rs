//! Issue #902 评论 6214278678 — 嵌套三层树（`R → Embed(eB) → B → Embed(eBC) → C`）中
//! **宿主图可以是非根的中间画布**，及其与根图宿主边的反查隔离。
//!
//! #902 要求跨画布连线「连接真实节点对象」，并以最长公共实例路径前缀定宿主图。
//! 前几轮走真实 `WriterCoreApi` 覆盖的拓扑里，宿主图要么是根图 `A`，要么是端点落点
//! 更深的公共祖先（菱形 D）；**尚未覆盖宿主图本身就是一个被 Embed 的中间画布**，
//! 即子树内部的跨画布连线（宿主 `B`），且 `B`、`C` 又整体位于根图 `R` 之下。
//!
//! 本测试用真实 Store 验证：
//!
//! 1. `from = B/[]/Node(b1)`、`to = B/[eBC]/Node(c1)`：最长公共前缀为空，宿主是中间
//!    画布 `B` 而非根 `R`（也不是落点 `C`）；只有 `B` 写一条真实 Edge，`R`、`C`
//!    都不落假边。
//! 2. 反查：经 `R/[eB]` 进入 `B`、经 `R/[eB,eBC]` 进入 `C` 都能看到这条宿主 `B` 的边；
//!    根画布 `R`（空路径）看不到它；按 `Node(b1)` 在 `B` 命中、按 `Node(c1)` 在 `B`
//!    不命中、在 `C` 命中。
//! 3. 对比方向：`R/[]/Node(r1) → R/[eB,eBC]/Node(c1)` 跨越两级、宿主是根 `R`，此时
//!    中间画布 `B`（`R/[eB]`）看不到它，只有 `C` 与 `R` 能看到；按 `Node(c1)` 在 `B`
//!    不命中、在 `C` 命中。
//! 4. 删除宿主 `B` 中的边后反向依赖索引同步失效：`C` 的 canvas 查询不再返回它。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tempfile::TempDir;
use writer_core::api::types::{
    CreateCrossCanvasEdgeResultDto, StarMapEdgeKindDto, StarMapEndpointSelectionDto,
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

struct Nested {
    _tmp: TempDir,
    api: WriterCoreApi,
    r: String,
    b: String,
    c: String,
    eb: String,
    ebc: String,
}

/// 根图 `R --Embed(eB)--> B --Embed(eBC)--> C`，每层各放一个本地节点。
fn make_nested() -> Nested {
    let (tmp, api) = make_api();

    let r = api.create_starmap("R", "", None).unwrap().starmap_id;
    let b_child = api
        .create_starmap_child_embed(&r, "B", point(0.0, 0.0))
        .unwrap();
    let b = b_child.starmap.starmap_id.clone();
    let eb = b_child.embed.instance_id.clone();

    let c_child = api
        .create_starmap_child_embed(&b, "C", point(0.0, 0.0))
        .unwrap();
    let c = c_child.starmap.starmap_id.clone();
    let ebc = c_child.embed.instance_id.clone();

    api.add_starmap_node(&r, node("r1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&b, node("b1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&c, node("c1"), 0.0, 0.0).unwrap();

    Nested {
        _tmp: tmp,
        api,
        r,
        b,
        c,
        eb,
        ebc,
    }
}

impl Nested {
    fn route_b(&self) -> StarMapNavigationRouteDto {
        route(&self.r, vec![enter_embed(&self.eb)])
    }

    fn route_c(&self) -> StarMapNavigationRouteDto {
        route(&self.r, vec![enter_embed(&self.eb), enter_embed(&self.ebc)])
    }
}

/// 宿主是中间画布 `B`：边写在 `B`，`R`/`C` 无假边。
fn assert_edge_hosted_by_intermediate_canvas(f: &Nested, created: &CreateCrossCanvasEdgeResultDto) {
    assert_eq!(
        created.owner_starmap_id, f.b,
        "最长公共前缀为 [eB]，宿主应是中间画布 B，而不是根 R"
    );
    assert_eq!(created.edge.from.starmap_id, f.b);
    assert!(
        created.edge.from.segments.is_empty(),
        "from 端在宿主本层，不应带穿越路径"
    );
    assert_eq!(created.edge.from.target, node_target("b1"));
    assert_eq!(created.edge.to.starmap_id, f.b);
    assert_eq!(
        created.edge.to.segments,
        vec![enter_embed(&f.ebc)],
        "to 端穿越 eBC 进入 C，路径应写入宿主 B"
    );
    assert_eq!(created.edge.to.target, node_target("c1"));

    // 只有中间画布 B 写一条真实 Edge；根 R 与落点 C 都不写假边。
    let b_graph = f.api.get_starmap_graph(&f.b).unwrap();
    assert_eq!(b_graph.edges.len(), 1, "宿主 B 应恰有一条边");
    assert_eq!(b_graph.edges[0].id, created.edge.id);
    assert!(
        f.api.get_starmap_graph(&f.r).unwrap().edges.is_empty(),
        "根图 R 不应写入伪造的本地 Edge"
    );
    assert!(
        f.api.get_starmap_graph(&f.c).unwrap().edges.is_empty(),
        "落点画布 C 不应写入伪造的本地 Edge"
    );
}

/// `B` 与 `C` 都能反查这条宿主 `B` 的边，根 `R` 看不到。
fn assert_canvas_queries_see_intermediate_host_edge(f: &Nested, edge_id: &str) {
    // 经 R/[eB] 进入 B、经 R/[eB,eBC] 进入 C 都能反查这条宿主 B 的边。
    let seen_at_b = f.api.list_starmap_connections(f.route_b(), None).unwrap();
    assert!(
        seen_at_b
            .iter()
            .any(|c| c.edge_id == edge_id && c.owner_starmap_id == f.b),
        "经 R/[eB] 进入 B 应看到宿主 B 的边；实际: {seen_at_b:?}"
    );
    let seen_at_c = f.api.list_starmap_connections(f.route_c(), None).unwrap();
    assert!(
        seen_at_c
            .iter()
            .any(|c| c.edge_id == edge_id && c.owner_starmap_id == f.b),
        "经 R/[eB,eBC] 进入 C 应看到 to 端落在本层的宿主 B 边；实际: {seen_at_c:?}"
    );

    // 根画布 R 只含自己的边，不应看到存于 B 的边。
    let seen_at_root = f
        .api
        .list_starmap_connections(route(&f.r, vec![]), None)
        .unwrap();
    assert!(
        seen_at_root.iter().all(|c| c.edge_id != edge_id),
        "根画布 R 不应看到存于中间画布 B 的边；实际: {seen_at_root:?}"
    );
}

/// 按目标精确反查：B 层按 b1 命中，按 c1 不命中；C 层按 c1 命中。
fn assert_target_queries_are_route_aware_for_intermediate_host(f: &Nested, edge_id: &str) {
    let b_looking_for_b1 = f
        .api
        .list_starmap_connections(f.route_b(), Some(node_target("b1")))
        .unwrap();
    assert!(
        b_looking_for_b1.iter().any(|c| c.edge_id == edge_id),
        "B 层按 b1 反查应命中；实际: {b_looking_for_b1:?}"
    );
    let b_looking_for_c1 = f
        .api
        .list_starmap_connections(f.route_b(), Some(node_target("c1")))
        .unwrap();
    assert!(
        b_looking_for_c1.iter().all(|c| c.edge_id != edge_id),
        "B 层按 c1 反查不应命中（c1 在更深画布 C）；实际: {b_looking_for_c1:?}"
    );
    let c_looking_for_c1 = f
        .api
        .list_starmap_connections(f.route_c(), Some(node_target("c1")))
        .unwrap();
    assert!(
        c_looking_for_c1.iter().any(|c| c.edge_id == edge_id),
        "C 层按 c1 反查应命中；实际: {c_looking_for_c1:?}"
    );
}

/// 删除宿主 `B` 中的边后，反向依赖索引同步失效：`C` 的 canvas 查询不再返回它。
fn assert_delete_clears_intermediate_host_query(f: &Nested, edge_id: &str) {
    assert!(
        f.api.delete_starmap_edge(&f.b, edge_id).unwrap(),
        "删除宿主 B 中的边应返回 true"
    );
    assert!(f.api.get_starmap_graph(&f.b).unwrap().edges.is_empty());
    let after_delete = f.api.list_starmap_connections(f.route_c(), None).unwrap();
    assert!(
        after_delete.iter().all(|c| c.edge_id != edge_id),
        "删除后 C 不应再反查到已删除的边；实际: {after_delete:?}"
    );
}

/// 宿主是中间画布 `B`：边写在 `B`，`R`/`C` 无假边；`B` 与 `C` 都能反查。
#[test]
fn intermediate_canvas_hosts_edge_to_deeper_descendant() {
    let f = make_nested();

    let created = f
        .api
        .create_cross_canvas_edge(
            selection(f.route_b(), node_target("b1")),
            selection(f.route_c(), node_target("c1")),
            StarMapEdgeKindDto::RelatedTo,
            Some("子树内部".to_string()),
            None,
        )
        .unwrap();

    assert_edge_hosted_by_intermediate_canvas(&f, &created);
    assert_canvas_queries_see_intermediate_host_edge(&f, &created.edge.id);
    assert_target_queries_are_route_aware_for_intermediate_host(&f, &created.edge.id);
    assert_delete_clears_intermediate_host_query(&f, &created.edge.id);
}

/// 对比：跨两级的根宿主边 `R/[]/Node(r1) → R/[eB,eBC]/Node(c1)`，
/// 中间画布 `B` 看不到它，只有 `C`/`R` 能看到。
#[test]
fn root_hosted_multi_level_edge_is_invisible_to_intermediate_canvas() {
    let f = make_nested();

    let created = f
        .api
        .create_cross_canvas_edge(
            selection(route(&f.r, vec![]), node_target("r1")),
            selection(f.route_c(), node_target("c1")),
            StarMapEdgeKindDto::References,
            None,
            None,
        )
        .unwrap();

    assert_eq!(
        created.owner_starmap_id, f.r,
        "公共前缀为空且两端同根，宿主应是根 R"
    );
    assert!(created.edge.from.segments.is_empty());
    assert_eq!(
        created.edge.to.segments,
        vec![enter_embed(&f.eb), enter_embed(&f.ebc)],
        "to 端跨两级穿越路径都应保留"
    );

    let r_graph = f.api.get_starmap_graph(&f.r).unwrap();
    assert_eq!(r_graph.edges.len(), 1, "宿主 R 应恰有一条边");
    assert_eq!(r_graph.edges[0].id, created.edge.id);
    assert!(f.api.get_starmap_graph(&f.b).unwrap().edges.is_empty());
    assert!(f.api.get_starmap_graph(&f.c).unwrap().edges.is_empty());

    // 根 R 与最深 C 都能看到；中间画布 B 看不到（端点不落在 B）。
    let seen_at_root = f
        .api
        .list_starmap_connections(route(&f.r, vec![]), None)
        .unwrap();
    assert!(
        seen_at_root.iter().any(|c| c.edge_id == created.edge.id),
        "宿主根 R 的 canvas 查询应包含本图边"
    );
    let seen_at_c = f.api.list_starmap_connections(f.route_c(), None).unwrap();
    assert!(
        seen_at_c.iter().any(|c| c.edge_id == created.edge.id),
        "to 端落在 C，C 应能看到这条边"
    );
    let seen_at_b = f.api.list_starmap_connections(f.route_b(), None).unwrap();
    assert!(
        seen_at_b.iter().all(|c| c.edge_id != created.edge.id),
        "端点都不落在中间画布 B，B 不应看到这条边；实际: {seen_at_b:?}"
    );

    // 按目标精确反查：C 层按 c1 命中，B 层按 c1 不命中（实例路径后缀不同）。
    let c_looking_for_c1 = f
        .api
        .list_starmap_connections(f.route_c(), Some(node_target("c1")))
        .unwrap();
    assert!(
        c_looking_for_c1
            .iter()
            .any(|c| c.edge_id == created.edge.id),
        "C 层按 c1 应命中；实际: {c_looking_for_c1:?}"
    );
    let b_looking_for_c1 = f
        .api
        .list_starmap_connections(f.route_b(), Some(node_target("c1")))
        .unwrap();
    assert!(
        b_looking_for_c1
            .iter()
            .all(|c| c.edge_id != created.edge.id),
        "B 层按 c1 不应命中跨越 eBC 的端点；实际: {b_looking_for_c1:?}"
    );

    // 根 R 层按 r1 命中 from 端。
    let r_looking_for_r1 = f
        .api
        .list_starmap_connections(route(&f.r, vec![]), Some(node_target("r1")))
        .unwrap();
    assert!(
        r_looking_for_r1
            .iter()
            .any(|c| c.edge_id == created.edge.id),
        "R 层按 r1 应命中 from 端；实际: {r_looking_for_r1:?}"
    );
}
