//! Issue #902 评论 6221885605 —「宿主是**深度 2** 中间画布、且某一端端点残留路径还要
//! 再穿越一个 Portal」的跨画布边，经真实 `WriterCoreApi` 验证宿主选择、唯一落库、
//! route-aware 反查与删除保护。
//!
//! 此前各轮经真实 API 覆盖的组合里，「中间/深层宿主」与「Portal 段」各自出现，
//! 但没有一个用例把它们叠在一起：
//!
//! - 宿主深度 1、端点残留穿越 Portal（评论 6215632250）；
//! - Portal 位于公共前缀、宿主深度 1（评论 6218093097）；
//! - 宿主深度 2、两端残留都只有 Embed（评论 6221315670）。
//!
//! **尚未覆盖**「宿主由两段 Embed 的最长公共前缀决定（深度 2），而 `to` 端从宿主
//! 出发还要先穿越本层 Portal 再经 Embed 落到更深画布」。它同时触发三条不同的
//! 代码路径：
//!
//! - `plan_cross_canvas_edge` 的公共前缀只含两个 `EnterEmbed`，宿主是深度 2 的
//!   中间画布 `C`；`to` 端残留保存为 `[EnterPortal(p1), eF]`，Portal 段留在端点里；
//! - `extract_path_dependency_facts` 对 `to` 路径依次记录
//!   `PortalTraversal(C,p1)`、`EmbedTraversal(P,eF)`、`NodeTarget(F,f1)` 与终点画布
//!   的 `CanvasEndpoint(F)`；
//! - `route_aware_endpoint_matches` 必须用从 `C` 起的完整后缀
//!   `[EnterPortal(p1), eF]`（含 Portal 段）精确匹配，而不是只看最终画布。
//!
//! 拓扑：`R --Embed(eB)--> B --Embed(eC)--> C`；`C` 有 Portal 节点 `p1` 指向独立
//! 画布 `P`，并用 `Embed(eD)` 进入兄弟画布 `D`；`P --Embed(eF)--> F`。`D` 有节点
//! `d1`，`F` 有节点 `f1`。一条跨画布边：
//! `from = R/[eB, eC, eD]/Node(d1)`、
//! `to = R/[eB, eC, EnterPortal(p1), eF]/Node(f1)`，
//! 公共实例前缀 `[eB, eC]`，宿主为深度 2 中间画布 `C`：
//!
//! 1. 落库 `from = C/[eD]/Node(d1)`、`to = C/[EnterPortal(p1), eF]/Node(f1)`；唯一
//!    真实 Edge 只落在 `C`，`R`/`B`/`P`/`D`/`F` 都不落假边。
//! 2. 画布反查按完整后缀隔离：宿主 `C`、起点落点 `D` 与终点画布 `F`（经 `p1`+`eF`）
//!    命中；仅被穿越、并非端点落点的 Portal 目的地 `P` 以及 `B`/`R` 不得命中。
//! 3. target 查询：`D/Node(d1)`、`F/Node(f1)` 各自命中本边；跨层或错误端点为空。
//! 4. 被删端点删除必须被拒绝：`D/d1` 报 `EdgeFrom`、`F/f1` 报 `EdgeTo`，且不误报
//!    相反方向。
//! 5. 删除宿主 `C` 中的原始边后，`d1`、`f1` 均可删除，`C` 不残留 Edge。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tempfile::TempDir;
use writer_core::api::types::{
    CreateCrossCanvasEdgeResultDto, StarMapConnectionInfoDto, StarMapEdgeKindDto,
    StarMapNavigationRouteDto, StarMapNodeDto, StarMapNodeKindDto, StarMapPathSegmentDto,
    StarMapPointDto, StarMapPortalDto, StarMapTargetDetailDto,
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

fn route(root: &str, segments: Vec<StarMapPathSegmentDto>) -> StarMapNavigationRouteDto {
    StarMapNavigationRouteDto {
        root_starmap_id: root.to_string(),
        segments,
    }
}

fn selection(
    route: StarMapNavigationRouteDto,
    target: StarMapTargetDetailDto,
) -> writer_core::api::types::StarMapEndpointSelectionDto {
    writer_core::api::types::StarMapEndpointSelectionDto { route, target }
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
    p: String,
    d: String,
    f: String,
    eb: String,
    ec: String,
    ed: String,
    ef: String,
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

    fn route_p(&self) -> StarMapNavigationRouteDto {
        route(
            &self.r,
            vec![
                enter_embed(&self.eb),
                enter_embed(&self.ec),
                enter_portal("p1"),
            ],
        )
    }

    fn route_f(&self) -> StarMapNavigationRouteDto {
        route(
            &self.r,
            vec![
                enter_embed(&self.eb),
                enter_embed(&self.ec),
                enter_portal("p1"),
                enter_embed(&self.ef),
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

/// `R --eB--> B --eC--> C`；`C` 有 Portal `p1 --> P` 与 Embed `eD --> D`；
/// `P --eF--> F`。创建一条宿主为深度 2 中间画布 `C`、`to` 端残留穿越 Portal 的边。
fn setup() -> Fixture {
    let (tmp, api) = make_api();

    let root = api.create_starmap("根图R", "", None).unwrap().starmap_id;
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

    let p_canvas = api.create_starmap("子图P", "", None).unwrap().starmap_id;
    api.add_starmap_node(&c, portal_node("p1", &p_canvas), 0.0, 0.0)
        .unwrap();

    let d_child = api
        .create_starmap_child_embed(&c, "子图D", point(0.0, 0.0))
        .unwrap();
    let d = d_child.starmap.starmap_id.clone();
    let ed = d_child.embed.instance_id.clone();
    api.add_starmap_node(&d, node("d1"), 0.0, 0.0).unwrap();

    let f_child = api
        .create_starmap_child_embed(&p_canvas, "子图F", point(0.0, 0.0))
        .unwrap();
    let f = f_child.starmap.starmap_id.clone();
    let ef = f_child.embed.instance_id.clone();
    api.add_starmap_node(&f, node("f1"), 0.0, 0.0).unwrap();

    let edge = create_initial_edge(&api, &root, &c, &eb, &ec, &ed, &ef);

    Fixture {
        _tmp: tmp,
        api,
        r: root,
        b,
        c,
        p: p_canvas,
        d,
        f,
        eb,
        ec,
        ed,
        ef,
        edge,
    }
}

/// 创建 `Node(d1) →（经 p1、eF）Node(f1)`，断言宿主是深度 2 中间画布 `C`。
fn create_initial_edge(
    api: &WriterCoreApi,
    root: &str,
    host_c: &str,
    eb: &str,
    ec: &str,
    ed: &str,
    ef: &str,
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
                    vec![
                        enter_embed(eb),
                        enter_embed(ec),
                        enter_portal("p1"),
                        enter_embed(ef),
                    ],
                ),
                node_target("f1"),
            ),
            StarMapEdgeKindDto::References,
            Some("深层宿主穿越 Portal".to_string()),
            None,
        )
        .unwrap();

    assert_eq!(
        created.owner_starmap_id, host_c,
        "公共前缀 [eB, eC] 解析后宿主应是深度 2 中间画布 C"
    );
    assert_eq!(created.edge.from.starmap_id, host_c);
    assert_eq!(
        created.edge.from.segments,
        vec![enter_embed(ed)],
        "from 从宿主 C 还要再穿 eD 才能到 D 的 d1"
    );
    assert_eq!(created.edge.from.target, node_target("d1"));
    assert_eq!(created.edge.to.starmap_id, host_c);
    assert_eq!(
        created.edge.to.segments,
        vec![enter_portal("p1"), enter_embed(ef)],
        "to 从宿主 C 出发先穿 Portal p1 再穿 eF，残留段应原样保留"
    );
    assert_eq!(created.edge.to.target, node_target("f1"));
    created.edge.id
}

/// 唯一真实 Edge 只落在深度 2 宿主 `C`，其余画布都不落假边。
fn assert_edges_only_in_deep_host(f: &Fixture) {
    let c_graph = f.api.get_starmap_graph(&f.c).unwrap();
    assert_eq!(c_graph.edges.len(), 1, "宿主 C 必须只写入一条真实 Edge");
    assert_eq!(c_graph.edges[0].id, f.edge);
    assert_eq!(
        c_graph.edges[0].to.segments,
        vec![enter_portal("p1"), enter_embed(&f.ef)],
        "宿主 C 持久化的 to 端点应保留 Portal+Embed 穿越段"
    );

    for (id, label) in [
        (&f.r, "根 R"),
        (&f.b, "中间层 B"),
        (&f.p, "Portal 目的地 P"),
        (&f.d, "起点画布 D"),
        (&f.f, "终点画布 F"),
    ] {
        assert!(
            f.api.get_starmap_graph(id).unwrap().edges.is_empty(),
            "{label} 不应写入伪造的本地 Edge"
        );
    }
}

/// 画布反查：宿主 C、起点落点 D、终点画布 F 命中；中间 Portal 目的地 P 与 B/R 不得命中。
fn assert_canvas_queries_are_route_aware(f: &Fixture) {
    assert!(
        contains_edge(&f.canvas(f.route_c()), &f.edge),
        "深层宿主 C 本图的画布查询应包含自己持有的边"
    );

    let via_d = f.canvas(f.route_d());
    let conn_d = via_d
        .iter()
        .find(|c| c.edge_id == f.edge)
        .expect("经 [eB,eC,eD] 进入 D 应反查到宿主 C 的边");
    assert_eq!(conn_d.owner_starmap_id, f.c);
    assert_eq!(conn_d.from.segments, vec![enter_embed(&f.ed)]);
    assert_eq!(conn_d.from.target, node_target("d1"));

    let via_f = f.canvas(f.route_f());
    let conn_f = via_f
        .iter()
        .find(|c| c.edge_id == f.edge)
        .expect("经 [eB,eC,EnterPortal(p1),eF] 进入 F 应反查到宿主 C 的边");
    assert_eq!(conn_f.owner_starmap_id, f.c);
    assert_eq!(
        conn_f.to.segments,
        vec![enter_portal("p1"), enter_embed(&f.ef)]
    );

    assert!(
        !contains_edge(&f.canvas(f.route_p()), &f.edge),
        "P 只是被穿越的 Portal 目的地，不是端点落点，不应误命中"
    );
    assert!(
        !contains_edge(&f.canvas(f.route_b()), &f.edge),
        "中间层 B 不持有也不落在端点，不应误命中"
    );
    assert!(
        !contains_edge(&f.canvas(f.route_r()), &f.edge),
        "根 R 不持有也不落在端点，不应误命中"
    );
}

/// target 查询按完整实例路径后缀（含 Portal 段）与终点身份精确过滤。
fn assert_target_queries_are_route_aware(f: &Fixture) {
    assert_eq!(
        ids(&f.target(f.route_d(), node_target("d1"))),
        vec![f.edge.as_str()],
        "起点落点 D/Node(d1) 的 target 查询应只命中该边"
    );
    assert_eq!(
        ids(&f.target(f.route_f(), node_target("f1"))),
        vec![f.edge.as_str()],
        "经 p1+eF 到达的 F/Node(f1) target 查询应只命中该边"
    );

    assert!(
        f.target(f.route_f(), node_target("d1")).is_empty(),
        "d1 在 D，不在 F，不应命中"
    );
    assert!(
        f.target(f.route_d(), node_target("f1")).is_empty(),
        "f1 在 F，不在 D，不应命中"
    );
    assert!(
        f.target(f.route_c(), node_target("d1")).is_empty(),
        "d1 在 D，不在宿主 C 本层，不应命中"
    );
    assert!(
        f.target(f.route_c(), node_target("f1")).is_empty(),
        "f1 在 F，不在宿主 C 本层，不应命中"
    );
    assert!(
        f.target(f.route_p(), node_target("f1")).is_empty(),
        "P 不是终点画布，Node(f1) 查询不应命中"
    );
}

/// 被宿主 `C` 的边引用为端点的 `D/d1`、`F/f1` 删除必须被拒绝，来源方向不混淆。
fn assert_delete_guards(f: &Fixture) {
    let blocked_d1 = f
        .api
        .delete_starmap_node(&f.d, "d1")
        .expect_err("d1 是跨画布边起点，删除必须被拒绝");
    let d1_message = format!("{blocked_d1:?}");
    assert!(
        d1_message.contains("EdgeFrom"),
        "d1 的引用来源应是 EdgeFrom：{d1_message}"
    );
    assert!(
        !d1_message.contains("EdgeTo"),
        "d1 不应被误标为 EdgeTo：{d1_message}"
    );

    let blocked_f1 = f
        .api
        .delete_starmap_node(&f.f, "f1")
        .expect_err("f1 是跨画布边终点（经 Portal 穿越），删除必须被拒绝");
    let f1_message = format!("{blocked_f1:?}");
    assert!(
        f1_message.contains("EdgeTo"),
        "f1 的引用来源应是 EdgeTo：{f1_message}"
    );
    assert!(
        !f1_message.contains("EdgeFrom"),
        "f1 不应被误标为 EdgeFrom：{f1_message}"
    );
}

/// 删除宿主边后解除引用：`d1` 与 `f1` 均可删除，`C` 不残留 Edge。
fn assert_delete_edge_frees_ends(f: &Fixture) {
    assert!(
        f.api.delete_starmap_edge(&f.c, &f.edge).unwrap(),
        "删除宿主 C 中的边应返回 true"
    );
    assert!(
        f.api.delete_starmap_node(&f.d, "d1").unwrap(),
        "删除原始边后起点 d1 应可删除"
    );
    assert!(
        f.api.delete_starmap_node(&f.f, "f1").unwrap(),
        "删除原始边后终点 f1 应可删除"
    );
    assert!(
        f.api.get_starmap_graph(&f.c).unwrap().edges.is_empty(),
        "宿主边已删除，C 不应残留 Edge"
    );
}

/// 深度 2 中间宿主 + 端点残留穿越 Portal：宿主选择、唯一落库、反查与删除保护。
#[test]
fn deep_host_with_portal_residual_cross_canvas_edge() {
    let f = setup();

    assert_edges_only_in_deep_host(&f);
    assert_canvas_queries_are_route_aware(&f);
    assert_target_queries_are_route_aware(&f);
    assert_delete_guards(&f);

    assert_delete_edge_frees_ends(&f);
}
