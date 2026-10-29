//! Issue #902 评论 6215191254 — 跨画布连线剩余的两条真实边界经 `WriterCoreApi` 验证。
//!
//! #902 实现路线第 2 节明确跨画布边端点目标是 `Node / Anchor / Starmap`。此前各轮
//! 已从公开 API 覆盖 `Node`（父/子、兄弟、菱形多父图、多级嵌套）、`Anchor`、
//! `Starmap`（整张子画布作为 `to` 端）以及语义终点 `ChapterRange / Entity`
//! 的 from/to 对称用例。**`Starmap`（整张画布）作为 `from` 端**此前只在
//! `to` 端验证过，缺少对称覆盖；同一张画布被同一父图的两个 Embed 实例分别进入、
//! 两端指向**同一个物理节点对象**的自反连线也未走真实 Store。
//!
//! 本测试用真实 `WriterCoreApi` 补齐：
//!
//! 1. `from.target = Starmap`、route 经 `eB` 进入子画布 B；宿主是最长公共前缀对应的
//!    根图 A，`from.segments` 记录 `eB`，`to` 端留在根图无穿越路径。只有 A 写一条
//!    真实 Edge，B 不写假边；B 画布的 canvas / `Starmap` target 反查命中，按
//!    `Node(b1)` 反查不命中；根图按 `Node(a1)` 命中、按 `Starmap` 不命中；删除后
//!    反向依赖索引同步失效。
//! 2. 同一张共享画布被根图 A 用两个 Embed `e1`、`e2` 同时进入，两端都指向共享画布
//!    里的**同一个** `Node(s1)`：宿主是 A，`from.segments = [e1]`、`to.segments =
//!    [e2]`，两个实例路径各只命中自己一端；根图按对象反查不命中（端点穿越路径非空）；
//!    删除后两个入口都不再反查。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tempfile::TempDir;
use writer_core::api::types::{
    CreateCrossCanvasEdgeResultDto, StarMapConnectionInfoDto, StarMapEdgeKindDto, StarMapEmbedDto,
    StarMapEndpointSelectionDto, StarMapNavigationRouteDto, StarMapNodeDto, StarMapNodeKindDto,
    StarMapPathSegmentDto, StarMapPointDto, StarMapTargetDetailDto, StarMapTargetPathDto,
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

fn embed_to(host: &str, instance_id: &str, target: &str) -> StarMapEmbedDto {
    StarMapEmbedDto {
        instance_id: instance_id.to_string(),
        target_starmap_id: target.to_string(),
        label: None,
        position: point(0.0, 0.0),
        host_path: StarMapTargetPathDto {
            starmap_id: host.to_string(),
            segments: vec![],
            target: StarMapTargetDetailDto::default(),
        },
        provenance: Default::default(),
        created_at: 0,
        updated_at: 0,
    }
}

fn contains_edge(connections: &[StarMapConnectionInfoDto], id: &str) -> bool {
    connections.iter().any(|c| c.edge_id == id)
}

struct WholeCanvasFixture {
    _tmp: TempDir,
    api: WriterCoreApi,
    a: String,
    b: String,
    eb: String,
}

impl WholeCanvasFixture {
    fn child_route(&self) -> StarMapNavigationRouteDto {
        route(&self.a, vec![enter_embed(&self.eb)])
    }
}

/// 根图 A → Embed eB → 子画布 B，A 有节点 a1，B 有节点 b1。
fn make_whole_canvas_fixture() -> WholeCanvasFixture {
    let (tmp, api) = make_api();

    let a = api.create_starmap("A", "", None).unwrap().starmap_id;
    let child = api
        .create_starmap_child_embed(&a, "B", point(0.0, 0.0))
        .unwrap();
    let b = child.starmap.starmap_id.clone();
    let eb = child.embed.instance_id.clone();

    api.add_starmap_node(&a, node("a1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&b, node("b1"), 0.0, 0.0).unwrap();

    WholeCanvasFixture {
        _tmp: tmp,
        api,
        a,
        b,
        eb,
    }
}

/// 整张画布作为 `from` 端：宿主、穿越路径与"只有宿主写真实 Edge"。
fn assert_whole_canvas_from_written_to_root(
    f: &WholeCanvasFixture,
    created: &CreateCrossCanvasEdgeResultDto,
) {
    assert_eq!(
        created.owner_starmap_id, f.a,
        "整张画布端点在更深层，公共前缀为空，宿主应是根图 A"
    );
    assert_eq!(created.edge.from.starmap_id, f.a);
    assert_eq!(
        created.edge.from.segments,
        vec![enter_embed(&f.eb)],
        "from 端穿越 eB 指向子画布，穿越路径必须写进宿主 A 的 from.segments"
    );
    assert_eq!(
        created.edge.from.target,
        starmap_target(),
        "from 端终点身份是整张子画布，不是某个节点"
    );
    assert_eq!(created.edge.to.starmap_id, f.a);
    assert!(
        created.edge.to.segments.is_empty(),
        "to 端在宿主本层，不应带穿越路径"
    );
    assert_eq!(created.edge.to.target, node_target("a1"));

    let a_graph = f.api.get_starmap_graph(&f.a).unwrap();
    assert_eq!(a_graph.edges.len(), 1, "宿主 A 应恰有一条边");
    assert_eq!(a_graph.edges[0].id, created.edge.id);
    assert_eq!(
        a_graph.edges[0].from.segments,
        vec![enter_embed(&f.eb)],
        "Store 中的真实 Edge 必须保留 from 端穿越路径"
    );
    assert!(
        f.api.get_starmap_graph(&f.b).unwrap().edges.is_empty(),
        "子画布 B 不应写入伪造的本地 Edge"
    );
}

/// 子画布反查：canvas 与 `Starmap` target 命中，`Node(b1)` target 不命中。
fn assert_child_queries_whole_canvas(f: &WholeCanvasFixture, edge_id: &str) {
    let child_canvas = f
        .api
        .list_starmap_connections(f.child_route(), None)
        .unwrap();
    assert!(
        contains_edge(&child_canvas, edge_id),
        "子画布应反查到 from 端指向整张画布的跨层边；实际: {child_canvas:?}"
    );

    let child_starmap = f
        .api
        .list_starmap_connections(f.child_route(), Some(starmap_target()))
        .unwrap();
    assert!(
        contains_edge(&child_starmap, edge_id),
        "子画布按 Starmap target 应命中整张画布终点的边；实际: {child_starmap:?}"
    );

    let child_node = f
        .api
        .list_starmap_connections(f.child_route(), Some(node_target("b1")))
        .unwrap();
    assert!(
        !contains_edge(&child_node, edge_id),
        "终点是整张画布的边不应在按 Node(b1) 反查时出现；实际: {child_node:?}"
    );
}

/// 根图反查：canvas 与 `Node(a1)` 命中，`Starmap` target 不命中。
fn assert_root_queries_whole_canvas(f: &WholeCanvasFixture, edge_id: &str) {
    let root_route = route(&f.a, vec![]);

    let root_canvas = f
        .api
        .list_starmap_connections(root_route.clone(), None)
        .unwrap();
    assert!(
        contains_edge(&root_canvas, edge_id),
        "宿主根图 canvas 查询应包含本图 Edge"
    );

    let root_a1 = f
        .api
        .list_starmap_connections(root_route.clone(), Some(node_target("a1")))
        .unwrap();
    assert!(
        contains_edge(&root_a1, edge_id),
        "根图按 a1 应命中 to 端；实际: {root_a1:?}"
    );

    let root_starmap = f
        .api
        .list_starmap_connections(root_route, Some(starmap_target()))
        .unwrap();
    assert!(
        !contains_edge(&root_starmap, edge_id),
        "根图按 Starmap 不应命中穿越 eB 落在子画布的端点；实际: {root_starmap:?}"
    );
}

/// 删除宿主边后，子画布的反向依赖索引查询必须同步失效。
fn assert_delete_clears_child_query(f: &WholeCanvasFixture, edge_id: &str) {
    assert!(
        f.api.delete_starmap_edge(&f.a, edge_id).unwrap(),
        "删除宿主图中的跨画布边应返回 true"
    );
    assert!(f.api.get_starmap_graph(&f.a).unwrap().edges.is_empty());
    let after_delete = f
        .api
        .list_starmap_connections(f.child_route(), None)
        .unwrap();
    assert!(
        !contains_edge(&after_delete, edge_id),
        "删除后子画布不应再反查到已删除的跨画布边；实际: {after_delete:?}"
    );
}

/// 整张画布作为 `from` 端：宿主、穿越路径、反查隔离与删除后索引刷新。
#[test]
fn whole_canvas_endpoint_on_from_side_is_hosted_by_root_and_queryable_from_child() {
    let f = make_whole_canvas_fixture();

    // from：整张子画布 B（Starmap 端点）；to：根图 A 内的节点 a1。
    let created = f
        .api
        .create_cross_canvas_edge(
            selection(f.child_route(), starmap_target()),
            selection(route(&f.a, vec![]), node_target("a1")),
            StarMapEdgeKindDto::References,
            Some("从整张画布出发".to_string()),
            None,
        )
        .unwrap();

    assert_whole_canvas_from_written_to_root(&f, &created);
    assert_child_queries_whole_canvas(&f, &created.edge.id);
    assert_root_queries_whole_canvas(&f, &created.edge.id);
    assert_delete_clears_child_query(&f, &created.edge.id);
}

/// 同一张共享画布被同一父图的两个 Embed 同时进入，两端指向同一个物理节点对象：
/// 实例路径区分两端，两个入口各只命中自己一端。
#[test]
fn same_object_across_two_instances_self_link_is_isolated_per_instance() {
    let (_tmp, api) = make_api();

    let a = api.create_starmap("A", "", None).unwrap().starmap_id;
    let shared = api.create_starmap("共享画布", "", None).unwrap().starmap_id;
    api.add_starmap_node(&shared, node("s1"), 0.0, 0.0).unwrap();

    // 同一个共享画布，在根图 A 里建立两个入口实例 e1 / e2。
    for instance_id in ["e1", "e2"] {
        api.add_starmap_embed(&a, embed_to(&a, instance_id, &shared))
            .unwrap();
    }

    let via_e1 = route(&a, vec![enter_embed("e1")]);
    let via_e2 = route(&a, vec![enter_embed("e2")]);

    // 两端是同一个物理对象 Node(s1)，只靠实例路径 e1 / e2 区分。
    let created = api
        .create_cross_canvas_edge(
            selection(via_e1.clone(), node_target("s1")),
            selection(via_e2.clone(), node_target("s1")),
            StarMapEdgeKindDto::RelatedTo,
            None,
            None,
        )
        .unwrap();

    assert_eq!(
        created.owner_starmap_id, a,
        "两个实例同属根图 A，宿主是公共前缀对应的 A"
    );
    assert_eq!(created.edge.from.segments, vec![enter_embed("e1")]);
    assert_eq!(created.edge.to.segments, vec![enter_embed("e2")]);
    assert_eq!(created.edge.from.target, node_target("s1"));
    assert_eq!(created.edge.to.target, node_target("s1"));

    // 只在根图 A 写一条真实 Edge；共享画布本体不写假边。
    let a_graph = api.get_starmap_graph(&a).unwrap();
    assert_eq!(a_graph.edges.len(), 1, "宿主 A 应恰有一条边");
    assert_eq!(a_graph.edges[0].id, created.edge.id);
    assert!(
        api.get_starmap_graph(&shared).unwrap().edges.is_empty(),
        "共享画布本体不应写入伪造的本地 Edge"
    );

    // 经 e1 命中 from 端、经 e2 命中 to 端；两个入口都能反查这条自反连线。
    let seen_via_e1 = api.list_starmap_connections(via_e1.clone(), None).unwrap();
    assert!(
        contains_edge(&seen_via_e1, &created.edge.id),
        "经 e1 进入共享画布应看到 from 端在本入口的边；实际: {seen_via_e1:?}"
    );
    let seen_via_e2 = api.list_starmap_connections(via_e2.clone(), None).unwrap();
    assert!(
        contains_edge(&seen_via_e2, &created.edge.id),
        "经 e2 进入共享画布应看到 to 端在本入口的边；实际: {seen_via_e2:?}"
    );

    // 按对象反查：两个入口都按 s1 命中（各自一端）。
    let e1_looking_for_s1 = api
        .list_starmap_connections(via_e1.clone(), Some(node_target("s1")))
        .unwrap();
    assert!(
        contains_edge(&e1_looking_for_s1, &created.edge.id),
        "e1 视角按 s1 应命中 from 端；实际: {e1_looking_for_s1:?}"
    );
    let e2_looking_for_s1 = api
        .list_starmap_connections(via_e2.clone(), Some(node_target("s1")))
        .unwrap();
    assert!(
        contains_edge(&e2_looking_for_s1, &created.edge.id),
        "e2 视角按 s1 应命中 to 端；实际: {e2_looking_for_s1:?}"
    );

    // 根图 A 虽然持有这条边（canvas 查询可见），但按 s1 反查不应命中：
    // 两端穿越路径都非空，s1 不在根图层。
    let root_looking_for_s1 = api
        .list_starmap_connections(route(&a, vec![]), Some(node_target("s1")))
        .unwrap();
    assert!(
        !contains_edge(&root_looking_for_s1, &created.edge.id),
        "根图按 s1 不应命中端点穿越 e1/e2 落在子画布的边；实际: {root_looking_for_s1:?}"
    );

    // 删除后两个入口都不再反查。
    assert!(api.delete_starmap_edge(&a, &created.edge.id).unwrap());
    let after_delete_e1 = api.list_starmap_connections(via_e1, None).unwrap();
    assert!(!contains_edge(&after_delete_e1, &created.edge.id));
    let after_delete_e2 = api.list_starmap_connections(via_e2, None).unwrap();
    assert!(!contains_edge(&after_delete_e2, &created.edge.id));
}
