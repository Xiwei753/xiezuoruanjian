//! Issue #902 评论 6218093097 —「Portal 出现在两端导航路线的**最长公共前缀**里，
//! 由 Portal 目的地画布充当中介宿主」经真实 `WriterCoreApi` 验证。
//!
//! #902 实现路线第 2 节规定跨画布边的宿主由两端导航路线的**最长公共实例路径
//! 前缀**决定。此前各轮经真实 `WriterCoreApi` 覆盖的组合里，Portal 段只出现在
//! 两种位置：
//!
//! - 根图宿主、`to` 端穿越 Portal（评论 6206822408 等）；
//! - 中间画布宿主、`to` 端或 `from` 端在**公共前缀之后**再穿越 Portal
//!   （评论 6215632250）。
//!
//! **尚未覆盖**「`EnterPortal` 段本身是公共前缀的一部分，即宿主画布由 Portal
//! 的目的地决定」这一组合。它与既有用例的代码路径不同：
//!
//! - 公共前缀含 `EnterPortal(p1)`，`resolve_navigation_route` 必须先把前缀解析
//!   到 Portal 目的地画布 `C`，`C` 才是宿主；
//! - 两个端点再从 `C` 出发经各自 Embed 落到兄弟画布 `D`/`E`，落库后
//!   `from.segments`/`to.segments` 只剩一截 Embed，`p1` 不再出现在边端点里；
//! - 反向依赖索引必须为经 Portal 到达的宿主生成候选，使 `D`/`E` 的画布与
//!   target 查询命中，而更上层的 `B`/根 `R` 不得误命中。
//!
//! 拓扑：`R --Embed(eB)--> B`，`B` 有 Portal 节点 `p1` 指向独立画布 `C`；
//! `C` 用两个 Embed `eD`/`eE` 分别进入 `D`/`E`。一条跨画布边：
//! `Node(d1) → Node(e1)`，两端输入路线分别为 `R/[eB, EnterPortal(p1), eD]` 与
//! `R/[eB, EnterPortal(p1), eE]`，宿主为 Portal 目的地 `C`。
//!
//! 1. 公共输入前缀是 `[eB, EnterPortal(p1)]`，宿主是 `C`；落库后
//!    `from.segments = [eD]`、`to.segments = [eE]`；只有 `C` 写一条真实 Edge，
//!    `R`/`B`/`D`/`E` 都不落假边。
//! 2. `C`、`D`、`E` 的画布查询都命中；`B`、`R` 不得命中。
//! 3. `D/Node(d1)`、`E/Node(e1)` 的 target 查询命中；`C/Node(d1)` 与
//!    `D/Node(e1)` 不得命中（实例路径后缀与终点身份都不符）。
//! 4. 被宿主 `C` 的边引用为端点的 `D/d1`、`E/e1` 删除必须被拒绝，来源身份分别
//!    指明 `EdgeFrom`、`EdgeTo`。
//! 5. 删除宿主 `C` 中的原始边后，反向依赖索引同步失效：允许删除 `d1`、`e1`。

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

fn enter_portal(node_id: &str) -> StarMapPathSegmentDto {
    StarMapPathSegmentDto {
        kind: "enterPortal".to_string(),
        instance_id: None,
        node_id: Some(node_id.to_string()),
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

struct Fixture {
    _tmp: TempDir,
    api: WriterCoreApi,
    r: String,
    b: String,
    c: String,
    d: String,
    e: String,
    eb: String,
    ed: String,
    ee: String,
}

impl Fixture {
    fn route_b(&self) -> StarMapNavigationRouteDto {
        route(&self.r, vec![enter_embed(&self.eb)])
    }

    fn route_c(&self) -> StarMapNavigationRouteDto {
        route(&self.r, vec![enter_embed(&self.eb), enter_portal("p1")])
    }

    fn route_d(&self) -> StarMapNavigationRouteDto {
        route(
            &self.r,
            vec![
                enter_embed(&self.eb),
                enter_portal("p1"),
                enter_embed(&self.ed),
            ],
        )
    }

    fn route_e(&self) -> StarMapNavigationRouteDto {
        route(
            &self.r,
            vec![
                enter_embed(&self.eb),
                enter_portal("p1"),
                enter_embed(&self.ee),
            ],
        )
    }

    fn route_r(&self) -> StarMapNavigationRouteDto {
        route(&self.r, vec![])
    }
}

/// `R --eB--> B --Portal(p1)--> C`，`C --eD--> D`、`C --eE--> E`。
fn setup() -> Fixture {
    let (tmp, api) = make_api();

    let root = api.create_starmap("R", "", None).unwrap().starmap_id;
    let b_child = api
        .create_starmap_child_embed(&root, "B", point(0.0, 0.0))
        .unwrap();
    let b = b_child.starmap.starmap_id.clone();
    let eb = b_child.embed.instance_id.clone();

    let c = api.create_starmap("C", "", None).unwrap().starmap_id;
    api.add_starmap_node(&b, portal_node("p1", &c), 0.0, 0.0)
        .unwrap();

    let d_child = api
        .create_starmap_child_embed(&c, "D", point(0.0, 0.0))
        .unwrap();
    let d = d_child.starmap.starmap_id.clone();
    let ed = d_child.embed.instance_id.clone();

    let e_child = api
        .create_starmap_child_embed(&c, "E", point(0.0, 0.0))
        .unwrap();
    let e = e_child.starmap.starmap_id.clone();
    let ee = e_child.embed.instance_id.clone();

    api.add_starmap_node(&root, node("r1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&d, node("d1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&e, node("e1"), 0.0, 0.0).unwrap();

    Fixture {
        _tmp: tmp,
        api,
        r: root,
        b,
        c,
        d,
        e,
        eb,
        ed,
        ee,
    }
}

/// 创建 `Node(d1) → Node(e1)`（公共前缀穿过 Portal `p1`），返回宿主 `C` 中的 Edge ID。
fn create_portal_host_edge(f: &Fixture) -> String {
    let created = f
        .api
        .create_cross_canvas_edge(
            selection(f.route_d(), node_target("d1")),
            selection(f.route_e(), node_target("e1")),
            StarMapEdgeKindDto::References,
            Some("穿越 Portal 的宿主".to_string()),
            None,
        )
        .unwrap();

    assert_host_is_portal_destination(f, &created);
    created.edge.id
}

/// 公共输入前缀是 `[eB, EnterPortal(p1)]`，宿主应是 Portal 目的地 `C`。
fn assert_host_is_portal_destination(f: &Fixture, created: &CreateCrossCanvasEdgeResultDto) {
    assert_eq!(
        created.owner_starmap_id, f.c,
        "公共前缀 [eB, EnterPortal(p1)] 解析到 Portal 目的地 C，宿主应是 C"
    );
    assert_eq!(created.edge.from.starmap_id, f.c);
    assert_eq!(
        created.edge.from.segments,
        vec![enter_embed(&f.ed)],
        "from 从宿主 C 还要再穿 eD 才能到 D 的 d1"
    );
    assert_eq!(created.edge.from.target, node_target("d1"));
    assert_eq!(created.edge.to.starmap_id, f.c);
    assert_eq!(
        created.edge.to.segments,
        vec![enter_embed(&f.ee)],
        "to 从宿主 C 还要再穿 eE 才能到 E 的 e1"
    );
    assert_eq!(created.edge.to.target, node_target("e1"));
}

/// 唯一真实 Edge 只落在 Portal 目的地 `C`，其余画布都不落假边。
fn assert_edge_only_in_portal_host(f: &Fixture, edge_id: &str) {
    let c_graph = f.api.get_starmap_graph(&f.c).unwrap();
    assert_eq!(c_graph.edges.len(), 1, "宿主 C 必须只写入一条 Edge");
    assert_eq!(c_graph.edges[0].id, edge_id);

    for (id, label) in [
        (&f.r, "根 R"),
        (&f.b, "Portal 所在 B"),
        (&f.d, "起点画布 D"),
        (&f.e, "终点画布 E"),
    ] {
        assert!(
            f.api.get_starmap_graph(id).unwrap().edges.is_empty(),
            "{label} 不应写入伪造的本地 Edge"
        );
    }
}

/// `C`/`D`/`E` 的画布查询命中；更上层的 `B`/`R` 不得命中。
fn assert_canvas_queries_see_portal_host(f: &Fixture, edge_id: &str) {
    for (route, label) in [
        (f.route_c(), "Portal 目的地 C"),
        (f.route_d(), "经 p1+eD 的 D"),
        (f.route_e(), "经 p1+eE 的 E"),
    ] {
        let conns = f.api.list_starmap_connections(route, None).unwrap();
        assert!(
            conns
                .iter()
                .any(|c| c.edge_id == edge_id && c.owner_starmap_id == f.c),
            "{label} 的画布查询应反查到存于宿主 C 的边"
        );
    }

    for (route, label) in [(f.route_b(), "Portal 所在 B"), (f.route_r(), "根 R")] {
        let conns = f.api.list_starmap_connections(route, None).unwrap();
        assert!(
            !contains_edge(&conns, edge_id),
            "{label} 不在端点的实例路径上，不得误命中存于 C 的边"
        );
    }
}

/// `D/Node(d1)`、`E/Node(e1)` target 命中；`C/Node(d1)`、`D/Node(e1)` 不命中。
fn assert_target_queries_are_route_aware(f: &Fixture, edge_id: &str) {
    let d_node = f
        .api
        .list_starmap_connections(f.route_d(), Some(node_target("d1")))
        .unwrap();
    assert!(
        contains_edge(&d_node, edge_id),
        "按 Node(d1) 在起点画布 D 查询应命中该边"
    );

    let e_node = f
        .api
        .list_starmap_connections(f.route_e(), Some(node_target("e1")))
        .unwrap();
    assert!(
        contains_edge(&e_node, edge_id),
        "按 Node(e1) 在终点画布 E 查询应命中该边"
    );

    let c_wrong_node = f
        .api
        .list_starmap_connections(f.route_c(), Some(node_target("d1")))
        .unwrap();
    assert!(
        !contains_edge(&c_wrong_node, edge_id),
        "宿主 C 本层没有 d1，按 Node(d1) 查询不得命中"
    );

    let d_wrong_node = f
        .api
        .list_starmap_connections(f.route_d(), Some(node_target("e1")))
        .unwrap();
    assert!(
        !contains_edge(&d_wrong_node, edge_id),
        "起点画布 D 看不到终点 e1，按 Node(e1) 查询不得命中"
    );
}

/// 被宿主 `C` 的边引用为端点的 `d1`/`e1` 在解除引用前不得被删除。
fn assert_endpoints_are_guarded(f: &Fixture) {
    let blocked_from = f
        .api
        .delete_starmap_node(&f.d, "d1")
        .expect_err("被宿主 C 的边作为起点指向的 d1 不得被删除");
    let from_message = format!("{blocked_from:?}");
    assert!(
        from_message.contains("EdgeFrom"),
        "起点侧引用的来源身份应指明 EdgeFrom；实际: {from_message}"
    );
    assert!(
        !from_message.contains("EdgeTo"),
        "起点 d1 不得被误标为 EdgeTo 来源；实际: {from_message}"
    );

    let blocked_to = f
        .api
        .delete_starmap_node(&f.e, "e1")
        .expect_err("被宿主 C 的边作为终点指向的 e1 不得被删除");
    let to_message = format!("{blocked_to:?}");
    assert!(
        to_message.contains("EdgeTo"),
        "终点侧引用的来源身份应指明 EdgeTo；实际: {to_message}"
    );
    assert!(
        !to_message.contains("EdgeFrom"),
        "终点 e1 不得被误标为 EdgeFrom 来源；实际: {to_message}"
    );
}

/// 删除宿主原始边后解除外部引用：允许删除 `d1`、`e1`。
fn assert_delete_edge_frees_endpoints(f: &Fixture, edge_id: &str) {
    assert!(
        f.api.delete_starmap_edge(&f.c, edge_id).unwrap(),
        "删除 Portal 目的地 C 中的跨画布边应返回 true"
    );
    assert!(f.api.get_starmap_graph(&f.c).unwrap().edges.is_empty());

    assert!(
        f.api.delete_starmap_node(&f.d, "d1").unwrap(),
        "解除外部引用后应允许删除 d1"
    );
    assert!(
        f.api.delete_starmap_node(&f.e, "e1").unwrap(),
        "解除外部引用后应允许删除 e1"
    );
}

/// Portal 落在公共前缀：宿主判定、route-aware 反查与端点删除保护都正确。
#[test]
fn portal_in_common_prefix_makes_destination_canvas_the_host() {
    let f = setup();

    let edge_id = create_portal_host_edge(&f);
    assert_edge_only_in_portal_host(&f, &edge_id);
    assert_canvas_queries_see_portal_host(&f, &edge_id);
    assert_target_queries_are_route_aware(&f, &edge_id);
    assert_endpoints_are_guarded(&f);
    assert_delete_edge_frees_endpoints(&f, &edge_id);
}
