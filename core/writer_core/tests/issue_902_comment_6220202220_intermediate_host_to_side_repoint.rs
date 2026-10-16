//! Issue #902 评论 6220202220 —「宿主是中间画布」的跨画布边在
//! `update_starmap_edge` 中**只改终点 `to`**、起点 `from` 原样保留时，
//! 反向索引与删除保护必须精确迁移。
//!
//! #902 实现路线第 3 节要求反向关系索引在“写入/修改/删除后增量更新，读取不同
//! 入口不会重复制造关系”。评论 6219406177 的测试已经覆盖了「中间画布宿主 +
//! 只改起点 `from`」；评论 6209895056 覆盖了「**根图**宿主 + 只改终点 `to`」。
//! 但**「宿主是中间画布」+「只改终点 `to`」**这一组合尚未被真实
//! `WriterCoreApi` 验证。
//!
//! 它走的是 `update_starmap_edge` 中与起点侧对称、但方向相反的一支：
//! 以 `starmap_id` 为宿主对 patch 后的边重新提取 `from` 与 `to` 两套事实，
//! 再分别 `replace_source_facts(EdgeFrom)`、`replace_source_facts(EdgeTo)`。
//! 若只改终点时把未改的 `from` 事实一起漏删，就会出现“起点仍在被引用、
//! 终点却不受保护”的不对称错误；若旧的 `to` 事实未清理，则“删掉的旧终点仍
//! 拦截、新终点反查不到”。
//!
//! 拓扑：`R --Embed(eB)--> B`，`B --Embed(eC)--> C`、`B --Embed(eD)--> D`。
//! `C` 有节点 `c1`，`D` 有节点 `d1`/`d2`。一条跨画布边两端输入路线为
//! `R/[eB, eC]` 与 `R/[eB, eD]`，公共实例前缀 `[eB]`，宿主为中间画布 `B`：
//!
//! 1. 初始 `from = B/[eC]/Node(c1)`、`to = B/[eD]/Node(d1)`，唯一真实边只落在
//!    `B`，`R`/`C`/`D` 均无伪边；删 `c1` 被 `EdgeFrom` 拦截、删 `d1` 被
//!    `EdgeTo` 拦截。
//! 2. 一次 `update_starmap_edge(B, edge, patch={from: None, to: B/[eD]/Node(d2)})`
//!    只移动终点：宿主仍只有同一条边；`to` 变为 `d2` 且 `from` 原样保留。
//! 3. 索引精确迁移：删 `d2` 被 `EdgeTo` 拦截、删 `c1` 仍被 `EdgeFrom` 拦截，
//!    旧的 `d1` 不再被引用而可删除。
//! 4. route-aware 反查与 target 过滤按完整实例路径隔离：经 `[eB,eC]` 进入 `C`
//!    命中并返回未改起点，经 `[eB,eD]` 进入 `D` 命中新终点，宿主 `B` 直接持有
//!    该边，根 `R` 不误命中；旧终点 `d1` / 跨层错误 target 的查询为空。
//! 5. 删除宿主边后解除剩余引用，`c1` 与 `d2` 均可删除。

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

/// 只移动终点到 `d2`，`from` 端原样保留；宿主仍是 `B`。
fn repoint_to_d2(f: &Fixture) {
    let updated = f
        .api
        .update_starmap_edge(
            &f.b,
            &f.edge,
            StarMapEdgePatchDto {
                kind: Some(StarMapEdgeKindDto::Foreshadows),
                label: Some(Some("改终点".to_string())),
                payload: None,
                from: None,
                to: Some(path(&f.b, vec![enter_embed(&f.ed)], node_target("d2"))),
            },
        )
        .unwrap();

    assert_eq!(updated.kind, StarMapEdgeKindDto::Foreshadows);
    assert_eq!(updated.label.as_deref(), Some("改终点"));
    assert_eq!(
        updated.from.starmap_id, f.b,
        "未改的 from 端宿主必须保持不变"
    );
    assert_eq!(updated.from.segments, vec![enter_embed(&f.ec)]);
    assert_eq!(updated.from.target, node_target("c1"));
    assert_eq!(updated.to.starmap_id, f.b);
    assert_eq!(updated.to.segments, vec![enter_embed(&f.ed)]);
    assert_eq!(updated.to.target, node_target("d2"));

    let b_graph = f.api.get_starmap_graph(&f.b).unwrap();
    assert_eq!(b_graph.edges.len(), 1, "只改一端不应新增或复制边");
    assert_eq!(b_graph.edges[0].id, f.edge);
}

/// 只改终点后：`d2` 由 `EdgeTo` 保护、`c1` 仍由 `EdgeFrom` 保护、旧 `d1` 可删。
fn assert_guards_after_to_repoint(f: &Fixture) {
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
        "d2 不应被误标为 EdgeFrom：{d2_message}"
    );

    assert!(
        f.api.delete_starmap_node(&f.d, "d1").unwrap(),
        "旧终点 d1 不再被引用，应可删除"
    );

    let blocked_c1 = f
        .api
        .delete_starmap_node(&f.c, "c1")
        .expect_err("from 端未改，c1 仍应被跨画布引用拦截");
    let c1_message = format!("{blocked_c1:?}");
    assert!(
        c1_message.contains("EdgeFrom"),
        "未改的 c1 仍应由 EdgeFrom 保护：{c1_message}"
    );
    assert!(
        !c1_message.contains("EdgeTo"),
        "c1 不应被误标为 EdgeTo：{c1_message}"
    );
}

/// route-aware 反查与 target 精确过滤：终点已迁移、起点不变、隔离正确。
fn assert_queries_after_to_repoint(f: &Fixture) {
    let via_d = f.canvas(f.route_d());
    let conn_d = via_d
        .iter()
        .find(|c| c.edge_id == f.edge)
        .expect("经 [eB, eD] 进入 D 应能反查到宿主 B 的边");
    assert_eq!(conn_d.owner_starmap_id, f.b);
    assert_eq!(conn_d.from.segments, vec![enter_embed(&f.ec)]);
    assert_eq!(conn_d.from.target, node_target("c1"));
    assert_eq!(conn_d.to.segments, vec![enter_embed(&f.ed)]);
    assert_eq!(conn_d.to.target, node_target("d2"));

    assert!(
        contains_edge(&f.canvas(f.route_c()), &f.edge),
        "经 [eB, eC] 进入 C 应能反查到起点落在此的宿主边"
    );
    assert!(
        contains_edge(&f.canvas(f.route_b()), &f.edge),
        "宿主 B 本图的画布查询应包含自己持有的边"
    );
    assert!(
        !contains_edge(&f.canvas(f.route_r()), &f.edge),
        "根 R 不持有也不落在端点，不应误命中"
    );

    assert_eq!(
        ids(&f.target(f.route_d(), node_target("d2"))),
        vec![f.edge.as_str()],
        "新终点 d2 的 target 查询应只命中该边"
    );
    assert!(
        f.target(f.route_d(), node_target("d1")).is_empty(),
        "旧终点 d1 的 target 查询应变空"
    );
    assert_eq!(
        ids(&f.target(f.route_c(), node_target("c1"))),
        vec![f.edge.as_str()],
        "未改起点 c1 的 target 查询仍应命中"
    );
    assert!(
        f.target(f.route_b(), node_target("d2")).is_empty(),
        "d2 在 D，不在宿主 B 本层，不应命中"
    );
    assert!(
        f.target(f.route_b(), node_target("c1")).is_empty(),
        "c1 在 C，不在宿主 B 本层，不应命中"
    );
}

/// 删除宿主边后剩余引用解除：`c1` 与 `d2` 可删。
fn assert_delete_edge_frees_new_to(f: &Fixture) {
    assert!(
        f.api.delete_starmap_edge(&f.b, &f.edge).unwrap(),
        "删除中间宿主 B 中的边应返回 true"
    );
    assert!(
        f.api.delete_starmap_node(&f.c, "c1").unwrap(),
        "删除原始边后起点 c1 应可删除"
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

/// 中间画布宿主的跨画布边只改终点：两套来源事实必须分别正确迁移。
#[test]
fn intermediate_host_single_endpoint_repoint_migrates_to_index_only() {
    let f = setup();

    assert_edges_only_in_intermediate_host(&f);
    assert_initial_guards(&f);

    repoint_to_d2(&f);
    assert_queries_after_to_repoint(&f);
    assert_guards_after_to_repoint(&f);

    assert_delete_edge_frees_new_to(&f);
}
