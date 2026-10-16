//! Issue #902 评论 6218801760 —「语义目标（`ChapterRange` / `External`）的跨画布边
//! 宿主落在**中间画布**」经真实 `WriterCoreApi` 验证。
//!
//! #902 实现路线第 2 节规定跨画布边的宿主由两端导航路线的**最长公共实例路径
//! 前缀**决定，第 3 节要求连接反查从反向依赖索引收集候选祖先图。此前各轮经真实
//! `WriterCoreApi` 覆盖的组合里，语义目标只出现过两种宿主位置：
//!
//! - 根图宿主、`to` 端语义目标（评论 6213138596）；
//! - 根图宿主、`from` 端语义目标（评论 6213739902）。
//!
//! 中间/深层画布宿主此前只覆盖了 Node、Anchor、Starmap 终点（评论 6214278678、
//! 6216900870、6215632250）。**尚未覆盖**「语义目标端点落在更深的子画布、而宿主
//! 是中间画布」这一组合，它的代码路径不同：
//!
//! - 公共前缀裁剪后，`from.segments`/`to.segments` 保留的是**从中间宿主出发**的
//!   深层 `EnterEmbed`，语义目标（无 Node 对象）只产出 `CanvasEndpoint` 事实；
//! - `collect_candidate_edge_owners_for_target` 对语义目标**回退**到按当前画布的
//!   `CanvasEndpoint` 收集候选，必须能找到存于中间宿主的边，再由
//!   `route_aware_endpoint_matches` 用实例路径后缀精确过滤；
//! - 同一宿主同时挂语义端点与 Anchor 端点时，Anchor 走精确 `AnchorTarget` 候选，
//!   语义目标走 `CanvasEndpoint` 回退，两条候选路径不得互相串线。
//!
//! 拓扑：`R --Embed(eB)--> B`，`B --Embed(eC)--> C`、`B --Embed(eD)--> D`。
//! `B` 有节点 `b1`，`C` 有节点 `c1`（挂锚点 `a1`）。两条跨画布边的宿主都是 `B`：
//!
//! - `semantic_edge`：`ChapterRange → External`，两端输入路线分别为
//!   `R/[eB, eC]` 与 `R/[eB, eD]`；
//! - `anchor_edge`：`Anchor(c1, a1) → Node(b1)`，两端输入路线分别为
//!   `R/[eB, eC]` 与 `R/[eB]`。
//!
//! 1. 公共输入前缀都是 `[eB]`，宿主是 `B`；落库后 `semantic_edge.from.segments =
//!    [eC]`、`to.segments = [eD]`；`anchor_edge.from.segments = [eC]`、
//!    `to.segments = []`；只有 `B` 写真实 Edge，`R`/`C`/`D` 都不落假边。
//! 2. `B`/`C` 的画布查询命中两条；`D` 只命中 `semantic_edge`；`R` 不命中。
//! 3. 按语义 target 查询只命中对应那条，`Anchor(c1, a1)` 走精确候选命中
//!    `anchor_edge`，`Node(c1)`、`Node(b1)`、跨画布的错误 target 不得串线。
//! 4. 被 `anchor_edge` 作为起点引用的锚点 `a1` 与节点 `c1` 在解除引用前不得
//!    删除/移除，来源身份指明 `EdgeFrom`。
//! 5. 删除宿主 `B` 中的边后，反向依赖索引同步失效：删除 `semantic_edge` 只
//!    清掉语义端点反查，`anchor_edge` 不受影响；删除 `anchor_edge` 后
//!    `C/c1` 可删。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tempfile::TempDir;
use writer_core::api::types::{
    CreateCrossCanvasEdgeResultDto, StarMapAnchorDto, StarMapAnchorRoleDto, StarMapAnchorTargetDto,
    StarMapConnectionInfoDto, StarMapEdgeKindDto, StarMapEndpointSelectionDto,
    StarMapNavigationRouteDto, StarMapNodeDto, StarMapNodeKindDto, StarMapNodePatchDto,
    StarMapPathSegmentDto, StarMapPointDto, StarMapTargetDetailDto,
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

fn chapter_range_target() -> StarMapTargetDetailDto {
    StarMapTargetDetailDto {
        kind: "chapterRange".to_string(),
        project_id: Some("proj".to_string()),
        volume_id: Some("vol".to_string()),
        chapter_id: Some("ch1".to_string()),
        range_start: Some(3),
        range_end: Some(9),
        ..Default::default()
    }
}

fn external_target() -> StarMapTargetDetailDto {
    StarMapTargetDetailDto {
        kind: "external".to_string(),
        uri: Some("https://example.com/doc".to_string()),
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
        role: StarMapAnchorRoleDto::Source,
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

fn contains_edge(connections: &[StarMapConnectionInfoDto], id: &str) -> bool {
    connections.iter().any(|c| c.edge_id == id)
}

fn ids(connections: &[StarMapConnectionInfoDto]) -> Vec<&str> {
    connections.iter().map(|c| c.edge_id.as_str()).collect()
}

struct Fixture {
    _tmp: TempDir,
    api: WriterCoreApi,
    r: String,
    b: String,
    c: String,
    d: String,
    eb: String,
    ec: String,
    ed: String,
    semantic_edge: String,
    anchor_edge: String,
}

impl Fixture {
    fn route_r(&self) -> StarMapNavigationRouteDto {
        route(&self.r, vec![])
    }

    fn route_b(&self) -> StarMapNavigationRouteDto {
        route(&self.r, vec![enter_embed(&self.eb)])
    }

    fn route_c(&self) -> StarMapNavigationRouteDto {
        route(&self.r, vec![enter_embed(&self.eb), enter_embed(&self.ec)])
    }

    fn route_d(&self) -> StarMapNavigationRouteDto {
        route(&self.r, vec![enter_embed(&self.eb), enter_embed(&self.ed)])
    }

    fn canvas(&self, route: StarMapNavigationRouteDto) -> Vec<StarMapConnectionInfoDto> {
        self.api.list_starmap_connections(route, None).unwrap()
    }

    fn target(
        &self,
        route: StarMapNavigationRouteDto,
        target: StarMapTargetDetailDto,
    ) -> Vec<StarMapConnectionInfoDto> {
        self.api
            .list_starmap_connections(route, Some(target))
            .unwrap()
    }
}

/// `R --eB--> B --eC--> C`、`B --eD--> D`，`b1`/`c1` 落在不同画布，`c1` 挂锚点 `a1`。
fn setup() -> Fixture {
    let (tmp, api) = make_api();

    let root = api.create_starmap("根图", "", None).unwrap().starmap_id;
    let b_child = api
        .create_starmap_child_embed(&root, "子图B", point(0.0, 0.0))
        .unwrap();
    let b = b_child.starmap.starmap_id.clone();
    let eb = b_child.embed.instance_id.clone();

    let c_child = api
        .create_starmap_child_embed(&b, "子图C", point(0.0, 0.0))
        .unwrap();
    let c = c_child.starmap.starmap_id.clone();
    let ec = c_child.embed.instance_id.clone();

    let d_child = api
        .create_starmap_child_embed(&b, "子图D", point(0.0, 0.0))
        .unwrap();
    let d = d_child.starmap.starmap_id.clone();
    let ed = d_child.embed.instance_id.clone();

    api.add_starmap_node(&b, node("b1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&c, node("c1"), 0.0, 0.0).unwrap();

    let updated = api
        .update_starmap_node(&c, "c1", patch_anchors(vec![anchor("a1", &c)]))
        .unwrap();
    assert_eq!(updated.anchors.len(), 1, "锚点应写入 c1");
    assert_eq!(updated.anchors[0].anchor_id, "a1");

    let semantic_edge = create_semantic_edge(&api, &root, &b, &eb, &ec, &ed);
    let anchor_edge = create_anchor_edge(&api, &root, &b, &eb, &ec);

    Fixture {
        _tmp: tmp,
        api,
        r: root,
        b,
        c,
        d,
        eb,
        ec,
        ed,
        semantic_edge,
        anchor_edge,
    }
}

/// 创建 `ChapterRange → External`（两端都在 `B` 的深层子画布），断言宿主是 `B`。
fn create_semantic_edge(
    api: &WriterCoreApi,
    root: &str,
    b: &str,
    eb: &str,
    ec: &str,
    ed: &str,
) -> String {
    let created = api
        .create_cross_canvas_edge(
            selection(
                route(root, vec![enter_embed(eb), enter_embed(ec)]),
                chapter_range_target(),
            ),
            selection(
                route(root, vec![enter_embed(eb), enter_embed(ed)]),
                external_target(),
            ),
            StarMapEdgeKindDto::References,
            Some("深层语义".to_string()),
            None,
        )
        .unwrap();

    assert_host_storage(
        &created,
        b,
        vec![enter_embed(ec)],
        chapter_range_target(),
        vec![enter_embed(ed)],
        external_target(),
    );
    created.edge.id
}

/// 创建 `Anchor(c1, a1) → Node(b1)`（起点在深层 `C`，终点在宿主本层），断言宿主是 `B`。
fn create_anchor_edge(api: &WriterCoreApi, root: &str, b: &str, eb: &str, ec: &str) -> String {
    let created = api
        .create_cross_canvas_edge(
            selection(
                route(root, vec![enter_embed(eb), enter_embed(ec)]),
                anchor_target("c1", "a1"),
            ),
            selection(route(root, vec![enter_embed(eb)]), node_target("b1")),
            StarMapEdgeKindDto::References,
            Some("深层锚点".to_string()),
            None,
        )
        .unwrap();

    assert_host_storage(
        &created,
        b,
        vec![enter_embed(ec)],
        anchor_target("c1", "a1"),
        vec![],
        node_target("b1"),
    );
    created.edge.id
}

fn assert_host_storage(
    created: &CreateCrossCanvasEdgeResultDto,
    host: &str,
    from_segments: Vec<StarMapPathSegmentDto>,
    from_target: StarMapTargetDetailDto,
    to_segments: Vec<StarMapPathSegmentDto>,
    to_target: StarMapTargetDetailDto,
) {
    assert_eq!(
        created.owner_starmap_id, host,
        "公共前缀裁剪后宿主应是中间画布 B"
    );
    assert_eq!(created.edge.from.starmap_id, host);
    assert_eq!(created.edge.from.segments, from_segments);
    assert_eq!(created.edge.from.target, from_target);
    assert_eq!(created.edge.to.starmap_id, host);
    assert_eq!(created.edge.to.segments, to_segments);
    assert_eq!(created.edge.to.target, to_target);
}

/// 唯一真实 Edge 只落在中间宿主 `B`，`R`/`C`/`D` 都不落假边。
fn assert_edges_only_in_intermediate_host(f: &Fixture) {
    let b_graph = f.api.get_starmap_graph(&f.b).unwrap();
    assert_eq!(b_graph.edges.len(), 2, "宿主 B 必须只写入两条真实 Edge");
    let mut stored = ids_of_graph(&b_graph);
    stored.sort();
    let mut expected = vec![f.semantic_edge.clone(), f.anchor_edge.clone()];
    expected.sort();
    assert_eq!(stored, expected);

    for (id, label) in [
        (&f.r, "根 R"),
        (&f.c, "深层起点画布 C"),
        (&f.d, "深层终点画布 D"),
    ] {
        assert!(
            f.api.get_starmap_graph(id).unwrap().edges.is_empty(),
            "{label} 不应写入伪造的本地 Edge"
        );
    }
}

/// 画布查询：`B`/`C` 命中两条，`D` 只命中语义边，`R` 不命中。
fn assert_canvas_queries(f: &Fixture) {
    for edge_id in [&f.semantic_edge, &f.anchor_edge] {
        assert!(
            contains_edge(&f.canvas(f.route_b()), edge_id),
            "宿主 B 的画布查询应包含 {edge_id}"
        );
        assert!(
            contains_edge(&f.canvas(f.route_c()), edge_id),
            "深层起点画布 C 的画布查询应反查到存于 B 的 {edge_id}"
        );
    }

    let d_canvas = f.canvas(f.route_d());
    assert_eq!(
        ids(&d_canvas),
        vec![f.semantic_edge.as_str()],
        "D 只应看到终点落在 D 的语义边；实际: {d_canvas:?}"
    );

    let r_canvas = f.canvas(f.route_r());
    assert!(
        !contains_edge(&r_canvas, &f.semantic_edge) && !contains_edge(&r_canvas, &f.anchor_edge),
        "根 R 不持有也不落在端点，不应误命中; 实际: {r_canvas:?}"
    );
}

/// target 查询：语义目标走 `CanvasEndpoint` 回退，Anchor 走精确候选，各不串线。
fn assert_target_queries(f: &Fixture) {
    let chapter = f.target(f.route_c(), chapter_range_target());
    assert_eq!(
        ids(&chapter),
        vec![f.semantic_edge.as_str()],
        "C 的 ChapterRange 查询只应命中语义边；实际: {chapter:?}"
    );

    let external = f.target(f.route_d(), external_target());
    assert_eq!(
        ids(&external),
        vec![f.semantic_edge.as_str()],
        "D 的 External 查询只应命中语义边；实际: {external:?}"
    );

    let anchor = f.target(f.route_c(), anchor_target("c1", "a1"));
    assert_eq!(
        ids(&anchor),
        vec![f.anchor_edge.as_str()],
        "Anchor(c1, a1) 走精确候选，只应命中锚点边；实际: {anchor:?}"
    );

    let b_node = f.target(f.route_b(), node_target("b1"));
    assert_eq!(
        ids(&b_node),
        vec![f.anchor_edge.as_str()],
        "宿主 B 按 Node(b1) 查询只应命中锚点边的 to 端；实际: {b_node:?}"
    );

    // 跨画布/跨类型的错误 target 不得误命中。
    assert!(
        f.target(f.route_c(), external_target()).is_empty(),
        "C 查询 External 不应命中终点在 D 的语义边"
    );
    assert!(
        f.target(f.route_c(), node_target("c1")).is_empty(),
        "anchor 端点不是 Node(c1)，按 Node(c1) 查询不应命中"
    );
    assert!(
        f.target(f.route_b(), chapter_range_target()).is_empty(),
        "宿主 B 本层没有 chapterRange 端点，不应命中"
    );
}

/// 深层锚点 `a1` 与节点 `c1` 被外图起点引用，解除前不得删除/移除，来源是 `EdgeFrom`。
fn assert_deep_anchor_is_guarded(f: &Fixture) {
    let blocked = f
        .api
        .delete_starmap_node(&f.c, "c1")
        .expect_err("被宿主 B 的边作为起点指向的 c1 不得被删除");
    let message = format!("{blocked:?}");
    assert!(
        message.contains("EdgeFrom"),
        "起点侧引用的来源身份应指明 EdgeFrom；实际: {message}"
    );
    assert!(
        !message.contains("EdgeTo"),
        "c1 不得被误标为 EdgeTo 来源；实际: {message}"
    );

    let blocked_anchor = f
        .api
        .update_starmap_node(&f.c, "c1", patch_anchors(vec![]))
        .expect_err("被外图边作为起点引用的锚点 a1 不得被移除");
    let anchor_message = format!("{blocked_anchor:?}");
    assert!(
        anchor_message.contains("external dependencies would dangle")
            && anchor_message.contains("AnchorTarget(anchor=a1)"),
        "移除被引用锚点应报外部依赖悬空并指明 anchor=a1；实际: {anchor_message}"
    );

    let still = f.api.get_starmap_graph(&f.c).unwrap();
    let c1 = still.nodes.iter().find(|n| n.id == "c1").unwrap();
    assert_eq!(c1.anchors.len(), 1, "被拒绝后锚点 a1 必须保持存在");
}

/// 删除 `semantic_edge` 只清语义端点反查，`anchor_edge` 不受影响。
fn assert_delete_semantic_edge_clears_index(f: &Fixture) {
    assert!(
        f.api.delete_starmap_edge(&f.b, &f.semantic_edge).unwrap(),
        "删除中间宿主 B 中的语义边应返回 true"
    );

    assert!(
        f.target(f.route_c(), chapter_range_target()).is_empty(),
        "被删语义边的 ChapterRange 查询应变空"
    );
    assert!(
        f.target(f.route_d(), external_target()).is_empty(),
        "被删语义边的 External 查询应变空"
    );

    let c_canvas = f.canvas(f.route_c());
    assert!(
        !contains_edge(&c_canvas, &f.semantic_edge),
        "删除后 C 的画布查询不应再返回语义边；实际: {c_canvas:?}"
    );
    assert!(
        contains_edge(&c_canvas, &f.anchor_edge),
        "删除语义边不应影响锚点边；实际: {c_canvas:?}"
    );
}

/// 删除 `anchor_edge` 后解除 `C/c1` 的外部引用，允许删锚点与节点。
fn assert_delete_anchor_edge_frees_deep_node(f: &Fixture) {
    assert!(
        f.api.delete_starmap_edge(&f.b, &f.anchor_edge).unwrap(),
        "删除中间宿主 B 中的锚点边应返回 true"
    );

    let cleared = f
        .api
        .update_starmap_node(&f.c, "c1", patch_anchors(vec![]))
        .unwrap();
    assert!(cleared.anchors.is_empty(), "解除引用后应允许移除锚点");

    assert!(
        f.api.delete_starmap_node(&f.c, "c1").unwrap(),
        "解除引用后应允许删除 c1"
    );
    assert!(
        f.api.get_starmap_graph(&f.b).unwrap().edges.is_empty(),
        "两条宿主边都已删除，B 不应残留 Edge"
    );
}

fn ids_of_graph(graph: &writer_core::api::types::StarMapGraphDto) -> Vec<String> {
    graph.edges.iter().map(|e| e.id.clone()).collect()
}

/// 语义端点宿主落在中间画布：宿主判定、route-aware 反查与删除保护都正确。
#[test]
fn semantic_and_anchor_endpoints_are_hosted_by_intermediate_canvas() {
    let f = setup();

    assert_edges_only_in_intermediate_host(&f);
    assert_canvas_queries(&f);
    assert_target_queries(&f);
    assert_deep_anchor_is_guarded(&f);
    assert_delete_semantic_edge_clears_index(&f);
    assert_delete_anchor_edge_frees_deep_node(&f);
}
