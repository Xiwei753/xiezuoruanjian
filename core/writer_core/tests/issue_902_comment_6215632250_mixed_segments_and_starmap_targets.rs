//! Issue #902 评论 6215632250 — 跨画布连线剩余的两条端点组合经 `WriterCoreApi` 验证。
//!
//! #902 实现路线第 2 节点名跨画布边端点目标是 `Node / Anchor / Starmap`，宿主由两端
//! 导航路线的最长公共实例路径前缀决定。此前各轮已覆盖的拓扑里：
//!
//! - 宿主为根的 Portal 路径（`A/[eB, EnterPortal(p)]`）用的是**根图宿主**；
//! - 宿主为中间画布的用例（`B/[] → B/[eBC]`）两条路径段**全是 Embed**；
//! - `Starmap`（整张画布）端点只在 `to` 端、以及在根图宿主下的 `from` 端验证过，
//!   尚未有两个端点**同时**为整张画布的用例。
//!
//! 本测试用真实 `WriterCoreApi` 补齐：
//!
//! 1. `R --Embed(eB)--> B`，B 内有本地节点 `b1` 与两个指向 C 的 Portal `p1`/`p2`：
//!    `from = R/[eB]/Node(b1)`、`to = R/[eB, EnterPortal(p1)]/Node(c1)`。公共前缀为
//!    `[eB]`，宿主是中间画布 `B`；`to.segments` 记录 `[EnterPortal(p1)]`，即宿主不是
//!    根时仍保留 Portal 穿越段。只有 `B` 写一条真实 Edge，`R`/`C` 不落假边；`B` 画布与
//!    经 `p1` 进入的 `C` 都能反查，经兄弟 Portal `p2` 进入的 `C` 看不到；根 `R` 也
//!    看不到（边存于 `B`）；删除后 `C` 的反向依赖索引同步失效。
//! 2. 根图 `A` 用两个 Embed `e1`/`e2` 分别进入子画布 `B`/`C`：两端都是整张画布的
//!    `Starmap` 目标，`from = A/[e1]/Starmap`、`to = A/[e2]/Starmap`，宿主是 `A`；
//!    两个下端画布各自按 canvas / `Starmap` target 命中，根 `A` 的 `Starmap` target
//!    查询不误命中（穿越路径非空）；删除后两侧都不再反查。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tempfile::TempDir;
use writer_core::api::types::{
    CreateCrossCanvasEdgeResultDto, StarMapConnectionInfoDto, StarMapEdgeKindDto, StarMapNodeDto,
    StarMapNodeKindDto, StarMapPathSegmentDto, StarMapPointDto, StarMapPortalDto,
    StarMapTargetDetailDto,
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

fn portal_node(id: &str, destination: &str) -> StarMapNodeDto {
    let mut n = node(id);
    n.portal = Some(StarMapPortalDto {
        destination_starmap_id: destination.to_string(),
        destination_target: None,
    });
    n
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

fn route(
    root: &str,
    segments: Vec<StarMapPathSegmentDto>,
) -> writer_core::api::types::StarMapNavigationRouteDto {
    writer_core::api::types::StarMapNavigationRouteDto {
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

fn enter_portal(node_id: &str) -> StarMapPathSegmentDto {
    StarMapPathSegmentDto {
        kind: "enterPortal".to_string(),
        instance_id: None,
        node_id: Some(node_id.to_string()),
    }
}

fn selection(
    route: writer_core::api::types::StarMapNavigationRouteDto,
    target: StarMapTargetDetailDto,
) -> writer_core::api::types::StarMapEndpointSelectionDto {
    writer_core::api::types::StarMapEndpointSelectionDto { route, target }
}

fn contains_edge(connections: &[StarMapConnectionInfoDto], id: &str) -> bool {
    connections.iter().any(|c| c.edge_id == id)
}

// ---------------------------------------------------------------------------
// 场景一：宿主为中间画布 B，to 端穿越 Portal
// ---------------------------------------------------------------------------

struct PortalHostFixture {
    _tmp: TempDir,
    api: WriterCoreApi,
    r: String,
    b: String,
    c: String,
    eb: String,
}

impl PortalHostFixture {
    fn route_b(&self) -> writer_core::api::types::StarMapNavigationRouteDto {
        route(&self.r, vec![enter_embed(&self.eb)])
    }

    fn route_c_via(&self, portal: &str) -> writer_core::api::types::StarMapNavigationRouteDto {
        route(&self.r, vec![enter_embed(&self.eb), enter_portal(portal)])
    }
}

/// `R --Embed(eB)--> B`，B 有本地 `b1` 与 Portal `p1`/`p2` 指向 `C`，C 有 `c1`。
fn make_portal_host_fixture() -> PortalHostFixture {
    let (tmp, api) = make_api();

    let r = api.create_starmap("R", "", None).unwrap().starmap_id;
    let child = api
        .create_starmap_child_embed(&r, "B", point(0.0, 0.0))
        .unwrap();
    let b = child.starmap.starmap_id.clone();
    let eb = child.embed.instance_id.clone();
    let c = api.create_starmap("C", "", None).unwrap().starmap_id;

    api.add_starmap_node(&b, node("b1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&b, portal_node("p1", &c), 0.0, 0.0)
        .unwrap();
    api.add_starmap_node(&b, portal_node("p2", &c), 0.0, 0.0)
        .unwrap();
    api.add_starmap_node(&c, node("c1"), 0.0, 0.0).unwrap();

    PortalHostFixture {
        _tmp: tmp,
        api,
        r,
        b,
        c,
        eb,
    }
}

/// 宿主是中间画布 `B`，`to.segments` 保留 Portal 穿越段，`R`/`C` 不落假边。
fn assert_portal_segment_stored_in_intermediate_host(
    f: &PortalHostFixture,
    created: &CreateCrossCanvasEdgeResultDto,
) {
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
        vec![enter_portal("p1")],
        "to 端穿越 Portal，EnterPortal 段必须写进宿主 B 的 to.segments"
    );
    assert_eq!(created.edge.to.target, node_target("c1"));

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

/// B 画布与经 `p1` 进入的 C 都能反查；经兄弟 Portal `p2` 进入的 C、以及根 R 看不到。
fn assert_portal_host_canvas_queries(f: &PortalHostFixture, edge_id: &str) {
    let seen_at_b = f.api.list_starmap_connections(f.route_b(), None).unwrap();
    assert!(
        seen_at_b
            .iter()
            .any(|c| c.edge_id == edge_id && c.owner_starmap_id == f.b),
        "经 R/[eB] 进入 B 应看到宿主 B 的边；实际: {seen_at_b:?}"
    );
    let seen_via_p1 = f
        .api
        .list_starmap_connections(f.route_c_via("p1"), None)
        .unwrap();
    assert!(
        seen_via_p1
            .iter()
            .any(|c| c.edge_id == edge_id && c.owner_starmap_id == f.b),
        "经 (eB,p1) 进入 C 应看到 to 端落在这里的宿主 B 边；实际: {seen_via_p1:?}"
    );
    let seen_via_p2 = f
        .api
        .list_starmap_connections(f.route_c_via("p2"), None)
        .unwrap();
    assert!(
        seen_via_p2.iter().all(|c| c.edge_id != edge_id),
        "经 (eB,p2) 进入 C 不应看到只属于 p1 的边；实际: {seen_via_p2:?}"
    );

    let seen_at_root = f
        .api
        .list_starmap_connections(route(&f.r, vec![]), None)
        .unwrap();
    assert!(
        seen_at_root.iter().all(|c| c.edge_id != edge_id),
        "根画布 R 不应看到存于中间画布 B 的边；实际: {seen_at_root:?}"
    );
}

/// 按对象精确反查：B 层区分 b1/c1，C 层经 `p1`/`p2` 区分实例路径。
fn assert_portal_host_target_queries(f: &PortalHostFixture, edge_id: &str) {
    let b_for_b1 = f
        .api
        .list_starmap_connections(f.route_b(), Some(node_target("b1")))
        .unwrap();
    assert!(
        contains_edge(&b_for_b1, edge_id),
        "B 层按 b1 应命中 from 端；实际: {b_for_b1:?}"
    );
    let b_for_c1 = f
        .api
        .list_starmap_connections(f.route_b(), Some(node_target("c1")))
        .unwrap();
    assert!(
        !contains_edge(&b_for_c1, edge_id),
        "B 层按 c1 不应命中跨越 Portal p1 落在 C 的端点；实际: {b_for_c1:?}"
    );
    let c_p1_for_c1 = f
        .api
        .list_starmap_connections(f.route_c_via("p1"), Some(node_target("c1")))
        .unwrap();
    assert!(
        contains_edge(&c_p1_for_c1, edge_id),
        "经 (eB,p1) 进入 C 按 c1 应命中；实际: {c_p1_for_c1:?}"
    );
    let c_p2_for_c1 = f
        .api
        .list_starmap_connections(f.route_c_via("p2"), Some(node_target("c1")))
        .unwrap();
    assert!(
        !contains_edge(&c_p2_for_c1, edge_id),
        "经 (eB,p2) 进入 C 按 c1 不应命中未经过 p2 的边；实际: {c_p2_for_c1:?}"
    );
}

/// 删除宿主 B 中的边后，C 经 `p1` 的反向依赖索引查询同步失效。
fn assert_portal_host_delete_clears(f: &PortalHostFixture, edge_id: &str) {
    assert!(f.api.delete_starmap_edge(&f.b, edge_id).unwrap());
    assert!(f.api.get_starmap_graph(&f.b).unwrap().edges.is_empty());
    let after_delete = f
        .api
        .list_starmap_connections(f.route_c_via("p1"), None)
        .unwrap();
    assert!(
        !contains_edge(&after_delete, edge_id),
        "删除后 C 不应再反查到已删除的边；实际: {after_delete:?}"
    );
}

#[test]
fn intermediate_canvas_hosts_portal_segment_and_isolates_sibling_portal() {
    let f = make_portal_host_fixture();

    // from：宿主 B 本层节点 b1；to：经 Portal p1 穿越进 C 的节点 c1。
    let created = f
        .api
        .create_cross_canvas_edge(
            selection(f.route_b(), node_target("b1")),
            selection(f.route_c_via("p1"), node_target("c1")),
            StarMapEdgeKindDto::References,
            Some("B 内跨 Portal".to_string()),
            None,
        )
        .unwrap();

    let edge_id = created.edge.id.as_str();
    assert_portal_segment_stored_in_intermediate_host(&f, &created);
    assert_portal_host_canvas_queries(&f, edge_id);
    assert_portal_host_target_queries(&f, edge_id);
    assert_portal_host_delete_clears(&f, edge_id);
}

// ---------------------------------------------------------------------------
// 场景二：两个端点都是整张画布（Starmap），宿主为根
// ---------------------------------------------------------------------------

struct SiblingCanvases {
    _tmp: TempDir,
    api: WriterCoreApi,
    a: String,
    b: String,
    c: String,
    e1: String,
    e2: String,
}

impl SiblingCanvases {
    fn route_b(&self) -> writer_core::api::types::StarMapNavigationRouteDto {
        route(&self.a, vec![enter_embed(&self.e1)])
    }

    fn route_c(&self) -> writer_core::api::types::StarMapNavigationRouteDto {
        route(&self.a, vec![enter_embed(&self.e2)])
    }
}

/// 根图 `A` 用 Embed `e1`/`e2` 分别进入子画布 `B`/`C`。
fn make_sibling_canvases() -> SiblingCanvases {
    let (tmp, api) = make_api();
    let a = api.create_starmap("A", "", None).unwrap().starmap_id;
    let b_child = api
        .create_starmap_child_embed(&a, "B", point(0.0, 0.0))
        .unwrap();
    let c_child = api
        .create_starmap_child_embed(&a, "C", point(0.0, 0.0))
        .unwrap();
    SiblingCanvases {
        _tmp: tmp,
        api,
        a,
        b: b_child.starmap.starmap_id,
        c: c_child.starmap.starmap_id,
        e1: b_child.embed.instance_id,
        e2: c_child.embed.instance_id,
    }
}

/// 两个整张画布端点同属根 `A`，宿主是 `A`，`B`/`C` 不落假边。
fn assert_whole_targets_stored_in_root(
    f: &SiblingCanvases,
    created: &CreateCrossCanvasEdgeResultDto,
) {
    assert_eq!(
        created.owner_starmap_id, f.a,
        "两个子画布同属根 A，宿主是公共前缀对应的 A"
    );
    assert_eq!(created.edge.from.starmap_id, f.a);
    assert_eq!(created.edge.from.segments, vec![enter_embed(&f.e1)]);
    assert_eq!(created.edge.from.target, starmap_target());
    assert_eq!(created.edge.to.starmap_id, f.a);
    assert_eq!(created.edge.to.segments, vec![enter_embed(&f.e2)]);
    assert_eq!(created.edge.to.target, starmap_target());

    let a_graph = f.api.get_starmap_graph(&f.a).unwrap();
    assert_eq!(a_graph.edges.len(), 1, "宿主 A 应恰有一条边");
    assert_eq!(a_graph.edges[0].id, created.edge.id);
    assert!(
        f.api.get_starmap_graph(&f.b).unwrap().edges.is_empty(),
        "子画布 B 不应写入伪造的本地 Edge"
    );
    assert!(
        f.api.get_starmap_graph(&f.c).unwrap().edges.is_empty(),
        "子画布 C 不应写入伪造的本地 Edge"
    );
}

/// B 是 from 端落点、C 是 to 端落点：两侧 canvas 查询都能看到。
fn assert_whole_target_canvas_queries(f: &SiblingCanvases, edge_id: &str) {
    let seen_at_b = f.api.list_starmap_connections(f.route_b(), None).unwrap();
    assert!(
        seen_at_b
            .iter()
            .any(|x| x.edge_id == edge_id && x.owner_starmap_id == f.a),
        "经 A/[e1] 进入 B 应看到 from 端指向整张 B 的宿主边；实际: {seen_at_b:?}"
    );
    let seen_at_c = f.api.list_starmap_connections(f.route_c(), None).unwrap();
    assert!(
        seen_at_c
            .iter()
            .any(|x| x.edge_id == edge_id && x.owner_starmap_id == f.a),
        "经 A/[e2] 进入 C 应看到 to 端指向整张 C 的宿主边；实际: {seen_at_c:?}"
    );
}

/// 两侧按 `Starmap` target 各命中自己一端；根 A 不该因目标类型相同而误命中。
fn assert_whole_target_target_queries(f: &SiblingCanvases, edge_id: &str) {
    let b_for_starmap = f
        .api
        .list_starmap_connections(f.route_b(), Some(starmap_target()))
        .unwrap();
    assert!(
        contains_edge(&b_for_starmap, edge_id),
        "B 层按 Starmap 应命中 from 端；实际: {b_for_starmap:?}"
    );
    let c_for_starmap = f
        .api
        .list_starmap_connections(f.route_c(), Some(starmap_target()))
        .unwrap();
    assert!(
        contains_edge(&c_for_starmap, edge_id),
        "C 层按 Starmap 应命中 to 端；实际: {c_for_starmap:?}"
    );
    let root_for_starmap = f
        .api
        .list_starmap_connections(route(&f.a, vec![]), Some(starmap_target()))
        .unwrap();
    assert!(
        !contains_edge(&root_for_starmap, edge_id),
        "根 A 按 Starmap 不应命中穿越 e1/e2 落在子画布的端点；实际: {root_for_starmap:?}"
    );
}

/// 删除根 `A` 中的边后，两侧下端都不再反查。
fn assert_whole_target_delete_clears(f: &SiblingCanvases, edge_id: &str) {
    assert!(f.api.delete_starmap_edge(&f.a, edge_id).unwrap());
    assert!(f.api.get_starmap_graph(&f.a).unwrap().edges.is_empty());
    let after_b = f.api.list_starmap_connections(f.route_b(), None).unwrap();
    assert!(!contains_edge(&after_b, edge_id));
    let after_c = f.api.list_starmap_connections(f.route_c(), None).unwrap();
    assert!(!contains_edge(&after_c, edge_id));
}

#[test]
fn whole_canvas_to_whole_canvas_edge_is_hosted_by_root_and_queryable_from_both_children() {
    let f = make_sibling_canvases();

    // 两端都是整张子画布：B 画布 → C 画布。
    let created = f
        .api
        .create_cross_canvas_edge(
            selection(f.route_b(), starmap_target()),
            selection(f.route_c(), starmap_target()),
            StarMapEdgeKindDto::References,
            None,
            None,
        )
        .unwrap();

    let edge_id = created.edge.id.as_str();
    assert_whole_targets_stored_in_root(&f, &created);
    assert_whole_target_canvas_queries(&f, edge_id);
    assert_whole_target_target_queries(&f, edge_id);
    assert_whole_target_delete_clears(&f, edge_id);
}
