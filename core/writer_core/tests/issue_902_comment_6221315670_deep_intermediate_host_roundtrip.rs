//! Issue #902 评论 6221315670 —「宿主是**深度 2**中间画布」的跨画布边
//! 创建、两端重定向、route-aware 反查与删除的完整回归。
//!
//! #902 实现路线第 2/3 节把「两个端点 route 的最长公共实例路径前缀对应的星图」
//! 作为跨画布边的宿主图，并要求反向关系索引在写入/修改/删除后增量更新。此前各轮
//! 覆盖的中间宿主都只到根图的**直接子画布**（`R --eB--> B`）：
//! - 评论 6214278678 / 6219406177 / 6220202220 / 6220726618：宿主 `B`（深度 1）；
//! - 评论 6216141983 / 6209895056：宿主为根图 `R`（深度 0）。
//!
//! 但**宿主落在深度 2 的中间画布**（公共前缀本身含两段实例路径）尚未被真实
//! `WriterCoreApi` 验证。它比深度 1 多一层实例前缀：`plan_cross_canvas_edge`
//! 必须把 `[eB, eC]` 整体解析到 `C`，再把深层 `[eD]` / `[eE]` 作为端点穿越段写回
//! `C`；反查时 `route_aware_endpoint_matches` 也必须按从 `C` 起的完整后缀比较，
//! 不能误用根路线前缀。
//!
//! 拓扑：`R --Embed(eB)--> B --Embed(eC)--> C`，`C --Embed(eD)--> D`、
//! `C --Embed(eE)--> E`。`D` 有节点 `d1`/`d2`，`E` 有节点 `e1`/`e2`。一条跨画布
//! 边两端输入路线为 `R/[eB, eC, eD]` 与 `R/[eB, eC, eE]`，公共实例前缀 `[eB, eC]`，
//! 宿主为深层中间画布 `C`：
//!
//! 1. 初始 `from = C/[eD]/Node(d1)`、`to = C/[eE]/Node(e1)`，唯一真实边只落在
//!    `C`，`R`/`B`/`D`/`E` 均无伪边；删 `d1` 被 `EdgeFrom` 拦截、删 `e1` 被
//!    `EdgeTo` 拦截。
//! 2. 一次 `update_starmap_edge(C, edge, patch={from: Some(C/[eD]/Node(d2)),
//!    to: Some(C/[eE]/Node(e2))})` 同时换两端：宿主 `C` 仍只有同一条边。
//! 3. 索引精确迁移：删 `d2` 由 `EdgeFrom` 保护、删 `e2` 由 `EdgeTo` 保护；
//!    旧 `d1`、`e1` 不再被引用而可删除，新端点不残留相反方向来源。
//! 4. route-aware 反查与 target 过滤按完整实例路径隔离：经 `[eB,eC,eD]` /
//!    `[eB,eC,eE]` 进入 `D`/`E` 均能反查深层宿主 `C` 的边并返回新端点；宿主 `C`
//!    直接持有该边；根 `R` 与中间层 `B` 不误命中；旧端点与跨层错误 target 查询为空。
//! 5. 删除宿主边后解除剩余引用，`d2` 与 `e2` 均可删除。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tempfile::TempDir;
use writer_core::api::types::{
    CreateCrossCanvasEdgeResultDto, StarMapConnectionInfoDto, StarMapEdgeKindDto,
    StarMapEdgePatchDto, StarMapEndpointSelectionDto, StarMapNavigationRouteDto, StarMapNodeDto,
    StarMapNodeKindDto, StarMapPathSegmentDto, StarMapPointDto, StarMapTargetDetailDto,
    StarMapTargetPathDto,
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

fn path(
    starmap_id: &str,
    segments: Vec<StarMapPathSegmentDto>,
    target: StarMapTargetDetailDto,
) -> StarMapTargetPathDto {
    StarMapTargetPathDto {
        starmap_id: starmap_id.to_string(),
        segments,
        target,
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
    e: String,
    eb: String,
    ec: String,
    ed: String,
    ee: String,
    edge: String,
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
        route(
            &self.r,
            vec![
                enter_embed(&self.eb),
                enter_embed(&self.ec),
                enter_embed(&self.ed),
            ],
        )
    }

    fn route_e(&self) -> StarMapNavigationRouteDto {
        route(
            &self.r,
            vec![
                enter_embed(&self.eb),
                enter_embed(&self.ec),
                enter_embed(&self.ee),
            ],
        )
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

/// `R --eB--> B --eC--> C`、`C --eD--> D`、`C --eE--> E`，创建一条宿主为深度 2
/// 中间画布 `C` 的跨画布边。
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
        .create_starmap_child_embed(&c, "子图D", point(0.0, 0.0))
        .unwrap();
    let d = d_child.starmap.starmap_id.clone();
    let ed = d_child.embed.instance_id.clone();

    let e_child = api
        .create_starmap_child_embed(&c, "子图E", point(0.0, 0.0))
        .unwrap();
    let e = e_child.starmap.starmap_id.clone();
    let ee = e_child.embed.instance_id.clone();

    api.add_starmap_node(&d, node("d1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&d, node("d2"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&e, node("e1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&e, node("e2"), 0.0, 0.0).unwrap();

    let edge = create_initial_edge(&api, &root, &c, &eb, &ec, &ed, &ee);

    Fixture {
        _tmp: tmp,
        api,
        r: root,
        b,
        c,
        d,
        e,
        eb,
        ec,
        ed,
        ee,
        edge,
    }
}

/// 创建 `Node(d1) → Node(e1)`（两端都在深度 2 宿主的深层子画布），断言宿主是 `C`。
fn create_initial_edge(
    api: &WriterCoreApi,
    root: &str,
    host_c: &str,
    eb: &str,
    ec: &str,
    ed: &str,
    ee: &str,
) -> String {
    let created: CreateCrossCanvasEdgeResultDto = api
        .create_cross_canvas_edge(
            selection(
                route(
                    root,
                    vec![enter_embed(eb), enter_embed(ec), enter_embed(ed)],
                ),
                node_target("d1"),
            ),
            selection(
                route(
                    root,
                    vec![enter_embed(eb), enter_embed(ec), enter_embed(ee)],
                ),
                node_target("e1"),
            ),
            StarMapEdgeKindDto::References,
            Some("深层宿主".to_string()),
            None,
        )
        .unwrap();

    assert_eq!(
        created.owner_starmap_id, host_c,
        "公共前缀 [eB, eC] 解析后宿主应是深度 2 中间画布 C"
    );
    assert_eq!(created.edge.from.starmap_id, host_c);
    assert_eq!(created.edge.from.segments, vec![enter_embed(ed)]);
    assert_eq!(created.edge.from.target, node_target("d1"));
    assert_eq!(created.edge.to.starmap_id, host_c);
    assert_eq!(created.edge.to.segments, vec![enter_embed(ee)]);
    assert_eq!(created.edge.to.target, node_target("e1"));
    created.edge.id
}

/// 唯一真实 Edge 只落在深度 2 宿主 `C`，`R`/`B`/`D`/`E` 都不落假边。
fn assert_edges_only_in_deep_host(f: &Fixture) {
    let c_graph = f.api.get_starmap_graph(&f.c).unwrap();
    assert_eq!(c_graph.edges.len(), 1, "宿主 C 必须只写入一条真实 Edge");
    assert_eq!(c_graph.edges[0].id, f.edge);

    for (id, label) in [
        (&f.r, "根 R"),
        (&f.b, "中间层 B"),
        (&f.d, "深层起点画布 D"),
        (&f.e, "深层终点画布 E"),
    ] {
        assert!(
            f.api.get_starmap_graph(id).unwrap().edges.is_empty(),
            "{label} 不应写入伪造的本地 Edge"
        );
    }
}

/// 初始：`d1` 由 `EdgeFrom` 保护、`e1` 由 `EdgeTo` 保护。
fn assert_initial_guards(f: &Fixture) {
    let blocked_d1 = f
        .api
        .delete_starmap_node(&f.d, "d1")
        .expect_err("d1 是深层宿主 C 的跨画布边起点，删除必须被拒绝");
    let d1_message = format!("{blocked_d1:?}");
    assert!(
        d1_message.contains("EdgeFrom"),
        "d1 的引用来源应是 EdgeFrom：{d1_message}"
    );
    assert!(
        !d1_message.contains("EdgeTo"),
        "d1 不应被误标为 EdgeTo：{d1_message}"
    );

    let blocked_e1 = f
        .api
        .delete_starmap_node(&f.e, "e1")
        .expect_err("e1 是深层宿主 C 的跨画布边终点，删除必须被拒绝");
    let e1_message = format!("{blocked_e1:?}");
    assert!(
        e1_message.contains("EdgeTo"),
        "e1 的引用来源应是 EdgeTo：{e1_message}"
    );
    assert!(
        !e1_message.contains("EdgeFrom"),
        "e1 不应被误标为 EdgeFrom：{e1_message}"
    );
}

/// 一次 update 同时把起点换到 `d2`、终点换到 `e2`；宿主仍是 `C`。
fn repoint_both_ends(f: &Fixture) {
    let updated = f
        .api
        .update_starmap_edge(
            &f.c,
            &f.edge,
            StarMapEdgePatchDto {
                kind: Some(StarMapEdgeKindDto::Foreshadows),
                label: Some(Some("深层两端都改".to_string())),
                payload: None,
                from: Some(path(&f.c, vec![enter_embed(&f.ed)], node_target("d2"))),
                to: Some(path(&f.c, vec![enter_embed(&f.ee)], node_target("e2"))),
            },
        )
        .unwrap();

    assert_eq!(updated.kind, StarMapEdgeKindDto::Foreshadows);
    assert_eq!(updated.label.as_deref(), Some("深层两端都改"));
    assert_eq!(updated.from.starmap_id, f.c);
    assert_eq!(updated.from.segments, vec![enter_embed(&f.ed)]);
    assert_eq!(updated.from.target, node_target("d2"));
    assert_eq!(updated.to.starmap_id, f.c);
    assert_eq!(updated.to.segments, vec![enter_embed(&f.ee)]);
    assert_eq!(updated.to.target, node_target("e2"));

    let c_graph = f.api.get_starmap_graph(&f.c).unwrap();
    assert_eq!(c_graph.edges.len(), 1, "同时改两端不应新增或复制边");
    assert_eq!(c_graph.edges[0].id, f.edge);
}

/// 同时改两端后：新端点各自被正确方向的来源保护，旧端点均已解除引用。
fn assert_guards_after_both_repoint(f: &Fixture) {
    let blocked_d2 = f
        .api
        .delete_starmap_node(&f.d, "d2")
        .expect_err("d2 成为新起点，删除必须被拒绝");
    let d2_message = format!("{blocked_d2:?}");
    assert!(
        d2_message.contains("EdgeFrom"),
        "新起点 d2 应由 EdgeFrom 保护：{d2_message}"
    );
    assert!(
        !d2_message.contains("EdgeTo"),
        "d2 不应残留相反方向的 EdgeTo 来源：{d2_message}"
    );

    let blocked_e2 = f
        .api
        .delete_starmap_node(&f.e, "e2")
        .expect_err("e2 成为新终点，删除必须被拒绝");
    let e2_message = format!("{blocked_e2:?}");
    assert!(
        e2_message.contains("EdgeTo"),
        "新终点 e2 应由 EdgeTo 保护：{e2_message}"
    );
    assert!(
        !e2_message.contains("EdgeFrom"),
        "e2 不应残留相反方向的 EdgeFrom 来源：{e2_message}"
    );

    assert!(
        f.api.delete_starmap_node(&f.d, "d1").unwrap(),
        "旧起点 d1 不再被引用，应可删除"
    );
    assert!(
        f.api.delete_starmap_node(&f.e, "e1").unwrap(),
        "旧终点 e1 不再被引用，应可删除"
    );
}

/// route-aware 反查与 target 精确过滤：深层宿主、两端均已迁移、隔离正确。
fn assert_queries_after_both_repoint(f: &Fixture) {
    let via_d = f.canvas(f.route_d());
    let conn_d = via_d
        .iter()
        .find(|c| c.edge_id == f.edge)
        .expect("经 [eB, eC, eD] 进入 D 应能反查到深层宿主 C 的边");
    assert_eq!(conn_d.owner_starmap_id, f.c);
    assert_eq!(conn_d.from.segments, vec![enter_embed(&f.ed)]);
    assert_eq!(conn_d.from.target, node_target("d2"));
    assert_eq!(conn_d.to.segments, vec![enter_embed(&f.ee)]);
    assert_eq!(conn_d.to.target, node_target("e2"));

    let via_e = f.canvas(f.route_e());
    let conn_e = via_e
        .iter()
        .find(|c| c.edge_id == f.edge)
        .expect("经 [eB, eC, eE] 进入 E 应能反查到深层宿主 C 的边");
    assert_eq!(conn_e.owner_starmap_id, f.c);
    assert_eq!(conn_e.from.target, node_target("d2"));
    assert_eq!(conn_e.to.target, node_target("e2"));

    assert!(
        contains_edge(&f.canvas(f.route_c()), &f.edge),
        "深层宿主 C 本图的画布查询应包含自己持有的边"
    );
    assert!(
        !contains_edge(&f.canvas(f.route_b()), &f.edge),
        "中间层 B 不持有也不落在端点，不应误命中"
    );
    assert!(
        !contains_edge(&f.canvas(f.route_r()), &f.edge),
        "根 R 不持有也不落在端点，不应误命中"
    );

    assert_eq!(
        ids(&f.target(f.route_d(), node_target("d2"))),
        vec![f.edge.as_str()],
        "新起点 d2 的 target 查询应只命中该边"
    );
    assert_eq!(
        ids(&f.target(f.route_e(), node_target("e2"))),
        vec![f.edge.as_str()],
        "新终点 e2 的 target 查询应只命中该边"
    );
    assert!(
        f.target(f.route_d(), node_target("d1")).is_empty(),
        "旧起点 d1 的 target 查询应变空"
    );
    assert!(
        f.target(f.route_e(), node_target("e1")).is_empty(),
        "旧终点 e1 的 target 查询应变空"
    );
    assert!(
        f.target(f.route_c(), node_target("d2")).is_empty(),
        "d2 在 D，不在深层宿主 C 本层，不应命中"
    );
    assert!(
        f.target(f.route_c(), node_target("e2")).is_empty(),
        "e2 在 E，不在深层宿主 C 本层，不应命中"
    );
    assert!(
        f.target(f.route_b(), node_target("d2")).is_empty(),
        "中间层 B 不是该端点落点，不应命中"
    );
}

/// 删除深层宿主边后剩余引用解除：`d2` 与 `e2` 可删。
fn assert_delete_edge_frees_new_ends(f: &Fixture) {
    assert!(
        f.api.delete_starmap_edge(&f.c, &f.edge).unwrap(),
        "删除深层宿主 C 中的边应返回 true"
    );
    assert!(
        f.api.delete_starmap_node(&f.d, "d2").unwrap(),
        "删除原始边后新起点 d2 应可删除"
    );
    assert!(
        f.api.delete_starmap_node(&f.e, "e2").unwrap(),
        "删除原始边后新终点 e2 应可删除"
    );
    assert!(
        f.api.get_starmap_graph(&f.c).unwrap().edges.is_empty(),
        "宿主边已删除，C 不应残留 Edge"
    );
}

/// 深度 2 中间画布宿主的跨画布边：创建、两端重定向、反查与删除的完整回归。
#[test]
fn deep_intermediate_host_cross_canvas_edge_roundtrip() {
    let f = setup();

    assert_edges_only_in_deep_host(&f);
    assert_initial_guards(&f);

    repoint_both_ends(&f);
    assert_queries_after_both_repoint(&f);
    assert_guards_after_both_repoint(&f);

    assert_delete_edge_frees_new_ends(&f);
}
