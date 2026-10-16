//! Issue #902 评论 6220726618 —「宿主是中间画布」的跨画布边在
//! `update_starmap_edge` 中**同一次调用把起点 `from` 与终点 `to` 一起换到
//! 另一组节点**时，两套反向来源事实必须分别被替换。
//!
//! #902 实现路线第 3 节要求反向关系索引在“写入/修改/删除后增量更新”。此前各轮
//! 已覆盖：
//! - 评论 6216141983：「**根图**宿主 + 同一次 update 两端对调」；
//! - 评论 6219406177：「**中间画布**宿主 + 只改起点 `from`」；
//! - 评论 6220202220：「**中间画布**宿主 + 只改终点 `to`」。
//!
//! 但**「宿主是中间画布」+「同一次 update 同时改两端」**这一组合尚未被真实
//! `WriterCoreApi` 验证。它走的是 `update_starmap_edge` 中两端同时非 None 的一
//! 支：先在 candidate 中应用 `from`/`to` patch 并校验，再对更新后的两条路径分别
//! 提取依赖事实，随后分别以 `EdgeFrom`、`EdgeTo` 调用
//! `replace_source_facts_in_path_dependency_index`。若任一端漏替换，就会出现
//! “旧起点/旧终点仍被拦截、新端点不被保护”的悬空或漏保护引用。
//!
//! 拓扑：`R --Embed(eB)--> B`，`B --Embed(eC)--> C`、`B --Embed(eD)--> D`。
//! `C` 有节点 `c1`/`c2`，`D` 有节点 `d1`/`d2`。一条跨画布边两端输入路线为
//! `R/[eB, eC]` 与 `R/[eB, eD]`，公共实例前缀 `[eB]`，宿主为中间画布 `B`：
//!
//! 1. 初始 `from = B/[eC]/Node(c1)`、`to = B/[eD]/Node(d1)`，唯一真实边只落在
//!    `B`，`R`/`C`/`D` 均无伪边；删 `c1` 被 `EdgeFrom` 拦截、删 `d1` 被
//!    `EdgeTo` 拦截。
//! 2. 一次 `update_starmap_edge(B, edge, patch={from: Some(B/[eC]/Node(c2)),
//!    to: Some(B/[eD]/Node(d2))})` 同时换两端：宿主仍只有同一条边。
//! 3. 索引精确迁移：删 `c2` 由 `EdgeFrom` 保护、删 `d2` 由 `EdgeTo` 保护；
//!    旧的 `c1`、`d1` 不再被引用而可删除，且新端点不会残留相反方向的来源。
//! 4. route-aware 反查与 target 过滤按完整实例路径隔离：经 `[eB,eC]` / `[eB,eD]`
//!    进入 `C`/`D` 均能反查宿主 `B` 的边并返回新端点；宿主 `B` 直接持有该边，
//!    根 `R` 不误命中；旧端点 `c1`/`d1` 与跨层错误 target 的查询为空。
//! 5. 删除宿主边后解除剩余引用，`c2` 与 `d2` 均可删除。

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
    eb: String,
    ec: String,
    ed: String,
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

/// `R --eB--> B --eC--> C`、`B --eD--> D`，创建一条宿主为中间画布 `B` 的跨画布边。
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

    api.add_starmap_node(&c, node("c1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&c, node("c2"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&d, node("d1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&d, node("d2"), 0.0, 0.0).unwrap();

    let edge = create_initial_edge(&api, &root, &b, &eb, &ec, &ed);

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
        edge,
    }
}

/// 创建 `Node(c1) → Node(d1)`（两端都在 `B` 的深层子画布），断言宿主是 `B`。
fn create_initial_edge(
    api: &WriterCoreApi,
    root: &str,
    host_b: &str,
    eb: &str,
    ec: &str,
    ed: &str,
) -> String {
    let created: CreateCrossCanvasEdgeResultDto = api
        .create_cross_canvas_edge(
            selection(
                route(root, vec![enter_embed(eb), enter_embed(ec)]),
                node_target("c1"),
            ),
            selection(
                route(root, vec![enter_embed(eb), enter_embed(ed)]),
                node_target("d1"),
            ),
            StarMapEdgeKindDto::References,
            Some("初始".to_string()),
            None,
        )
        .unwrap();

    assert_eq!(
        created.owner_starmap_id, host_b,
        "公共前缀 [eB] 解析后宿主应是中间画布 B"
    );
    assert_eq!(created.edge.from.starmap_id, host_b);
    assert_eq!(created.edge.from.segments, vec![enter_embed(ec)]);
    assert_eq!(created.edge.from.target, node_target("c1"));
    assert_eq!(created.edge.to.starmap_id, host_b);
    assert_eq!(created.edge.to.segments, vec![enter_embed(ed)]);
    assert_eq!(created.edge.to.target, node_target("d1"));
    created.edge.id
}

/// 唯一真实 Edge 只落在中间宿主 `B`，`R`/`C`/`D` 都不落假边。
fn assert_edges_only_in_intermediate_host(f: &Fixture) {
    let b_graph = f.api.get_starmap_graph(&f.b).unwrap();
    assert_eq!(b_graph.edges.len(), 1, "宿主 B 必须只写入一条真实 Edge");
    assert_eq!(b_graph.edges[0].id, f.edge);

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

/// 初始：`c1` 由 `EdgeFrom` 保护、`d1` 由 `EdgeTo` 保护。
fn assert_initial_guards(f: &Fixture) {
    let blocked_c1 = f
        .api
        .delete_starmap_node(&f.c, "c1")
        .expect_err("c1 是宿主 B 的跨画布边起点，删除必须被拒绝");
    let c1_message = format!("{blocked_c1:?}");
    assert!(
        c1_message.contains("EdgeFrom"),
        "c1 的引用来源应是 EdgeFrom：{c1_message}"
    );
    assert!(
        !c1_message.contains("EdgeTo"),
        "c1 不应被误标为 EdgeTo：{c1_message}"
    );

    let blocked_d1 = f
        .api
        .delete_starmap_node(&f.d, "d1")
        .expect_err("d1 是宿主 B 的跨画布边终点，删除必须被拒绝");
    let d1_message = format!("{blocked_d1:?}");
    assert!(
        d1_message.contains("EdgeTo"),
        "d1 的引用来源应是 EdgeTo：{d1_message}"
    );
    assert!(
        !d1_message.contains("EdgeFrom"),
        "d1 不应被误标为 EdgeFrom：{d1_message}"
    );
}

/// 一次 update 同时把起点换到 `c2`、终点换到 `d2`；宿主仍是 `B`。
fn repoint_both_ends(f: &Fixture) {
    let updated = f
        .api
        .update_starmap_edge(
            &f.b,
            &f.edge,
            StarMapEdgePatchDto {
                kind: Some(StarMapEdgeKindDto::Foreshadows),
                label: Some(Some("两端都改".to_string())),
                payload: None,
                from: Some(path(&f.b, vec![enter_embed(&f.ec)], node_target("c2"))),
                to: Some(path(&f.b, vec![enter_embed(&f.ed)], node_target("d2"))),
            },
        )
        .unwrap();

    assert_eq!(updated.kind, StarMapEdgeKindDto::Foreshadows);
    assert_eq!(updated.label.as_deref(), Some("两端都改"));
    assert_eq!(updated.from.starmap_id, f.b);
    assert_eq!(updated.from.segments, vec![enter_embed(&f.ec)]);
    assert_eq!(updated.from.target, node_target("c2"));
    assert_eq!(updated.to.starmap_id, f.b);
    assert_eq!(updated.to.segments, vec![enter_embed(&f.ed)]);
    assert_eq!(updated.to.target, node_target("d2"));

    let b_graph = f.api.get_starmap_graph(&f.b).unwrap();
    assert_eq!(b_graph.edges.len(), 1, "同时改两端不应新增或复制边");
    assert_eq!(b_graph.edges[0].id, f.edge);
}

/// 同时改两端后：新端点各自被正确方向的来源保护，旧端点均已解除引用。
fn assert_guards_after_both_repoint(f: &Fixture) {
    let blocked_c2 = f
        .api
        .delete_starmap_node(&f.c, "c2")
        .expect_err("c2 成为新起点，删除必须被拒绝");
    let c2_message = format!("{blocked_c2:?}");
    assert!(
        c2_message.contains("EdgeFrom"),
        "新起点 c2 应由 EdgeFrom 保护：{c2_message}"
    );
    assert!(
        !c2_message.contains("EdgeTo"),
        "c2 不应残留相反方向的 EdgeTo 来源：{c2_message}"
    );

    let blocked_d2 = f
        .api
        .delete_starmap_node(&f.d, "d2")
        .expect_err("d2 成为新终点，删除必须被拒绝");
    let d2_message = format!("{blocked_d2:?}");
    assert!(
        d2_message.contains("EdgeTo"),
        "新终点 d2 应由 EdgeTo 保护：{d2_message}"
    );
    assert!(
        !d2_message.contains("EdgeFrom"),
        "d2 不应残留相反方向的 EdgeFrom 来源：{d2_message}"
    );

    assert!(
        f.api.delete_starmap_node(&f.c, "c1").unwrap(),
        "旧起点 c1 不再被引用，应可删除"
    );
    assert!(
        f.api.delete_starmap_node(&f.d, "d1").unwrap(),
        "旧终点 d1 不再被引用，应可删除"
    );
}

/// route-aware 反查与 target 精确过滤：两端均已迁移、隔离正确。
fn assert_queries_after_both_repoint(f: &Fixture) {
    let via_c = f.canvas(f.route_c());
    let conn_c = via_c
        .iter()
        .find(|c| c.edge_id == f.edge)
        .expect("经 [eB, eC] 进入 C 应能反查到宿主 B 的边");
    assert_eq!(conn_c.owner_starmap_id, f.b);
    assert_eq!(conn_c.from.segments, vec![enter_embed(&f.ec)]);
    assert_eq!(conn_c.from.target, node_target("c2"));
    assert_eq!(conn_c.to.segments, vec![enter_embed(&f.ed)]);
    assert_eq!(conn_c.to.target, node_target("d2"));

    let via_d = f.canvas(f.route_d());
    let conn_d = via_d
        .iter()
        .find(|c| c.edge_id == f.edge)
        .expect("经 [eB, eD] 进入 D 应能反查到宿主 B 的边");
    assert_eq!(conn_d.owner_starmap_id, f.b);
    assert_eq!(conn_d.from.target, node_target("c2"));
    assert_eq!(conn_d.to.target, node_target("d2"));

    assert!(
        contains_edge(&f.canvas(f.route_b()), &f.edge),
        "宿主 B 本图的画布查询应包含自己持有的边"
    );
    assert!(
        !contains_edge(&f.canvas(f.route_r()), &f.edge),
        "根 R 不持有也不落在端点，不应误命中"
    );

    assert_eq!(
        ids(&f.target(f.route_c(), node_target("c2"))),
        vec![f.edge.as_str()],
        "新起点 c2 的 target 查询应只命中该边"
    );
    assert_eq!(
        ids(&f.target(f.route_d(), node_target("d2"))),
        vec![f.edge.as_str()],
        "新终点 d2 的 target 查询应只命中该边"
    );
    assert!(
        f.target(f.route_c(), node_target("c1")).is_empty(),
        "旧起点 c1 的 target 查询应变空"
    );
    assert!(
        f.target(f.route_d(), node_target("d1")).is_empty(),
        "旧终点 d1 的 target 查询应变空"
    );
    assert!(
        f.target(f.route_b(), node_target("c2")).is_empty(),
        "c2 在 C，不在宿主 B 本层，不应命中"
    );
    assert!(
        f.target(f.route_b(), node_target("d2")).is_empty(),
        "d2 在 D，不在宿主 B 本层，不应命中"
    );
}

/// 删除宿主边后剩余引用解除：`c2` 与 `d2` 可删。
fn assert_delete_edge_frees_new_ends(f: &Fixture) {
    assert!(
        f.api.delete_starmap_edge(&f.b, &f.edge).unwrap(),
        "删除中间宿主 B 中的边应返回 true"
    );
    assert!(
        f.api.delete_starmap_node(&f.c, "c2").unwrap(),
        "删除原始边后新起点 c2 应可删除"
    );
    assert!(
        f.api.delete_starmap_node(&f.d, "d2").unwrap(),
        "删除原始边后新终点 d2 应可删除"
    );
    assert!(
        f.api.get_starmap_graph(&f.b).unwrap().edges.is_empty(),
        "宿主边已删除，B 不应残留 Edge"
    );
}

/// 中间画布宿主的跨画布边同一次 update 同时改两端：两套来源事实必须分别迁移。
#[test]
fn intermediate_host_both_endpoints_repoint_replaces_both_index_sources() {
    let f = setup();

    assert_edges_only_in_intermediate_host(&f);
    assert_initial_guards(&f);

    repoint_both_ends(&f);
    assert_queries_after_both_repoint(&f);
    assert_guards_after_both_repoint(&f);

    assert_delete_edge_frees_new_ends(&f);
}
