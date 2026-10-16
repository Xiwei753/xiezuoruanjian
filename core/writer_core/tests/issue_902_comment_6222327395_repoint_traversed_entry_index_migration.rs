//! Issue #902 评论 6222327395 —「重定向一条端点残留路径**穿越外图 Portal 与外图
//! Embed** 的跨画布边」时，路径依赖索引必须把旧穿越来源换掉。
//!
//! #902 实现路线第 3 节要求反向依赖索引在写入/修改/删除后增量更新：先按来源
//! `(owner, source_kind, source_id)` 移除旧事实，再插入新事实。此前各轮的重定向
//! 用例（评论 6216141983 / 6220202220 / 6220726618 / 6221315670 等）端点残留都只有
//! **Embed 段**，因此只迁移了终点 `NodeTarget`/`AnchorTarget`/`CanvasEndpoint` 事实。
//!
//! **尚未覆盖**的是「端点路径在到达终点前先穿越**另一个画布的 Portal 节点**，
//! 再穿越**又一个画布的 Embed**」——此时 `extract_path_dependency_facts` 会为同一
//! 端点产生 `PortalTraversal` 与 `EmbedTraversal` 事实。把这条边重定向到另一组
//! Portal/Embed 之后，旧 Portal/Embed 的穿越依赖必须解除、新的一组必须建立。
//!
//! 拓扑：根图 `R` 有节点 `r1` 与 `Embed(eB) --> B`；`B` 有 `b1`、Portal `p1`/`p2`
//! 均指向独立画布 `P`；`P` 有 `Embed(eD) --> D`、`Embed(eE) --> E`；`D` 有 `d1`，
//! `E` 有 `e1`。初始一条存于 `R` 的跨画布边：
//! `from = R/[]/Node(r1)`、
//! `to = R/[eB, EnterPortal(p1), eD]/Node(d1)`，公共前缀为空，宿主为 `R`：
//!
//! 1. 初始 `to` 的穿越事实为 `PortalTraversal(B,p1)`、`EmbedTraversal(P,eD)` 与终点
//!    `NodeTarget(D,d1)`；删除 `p1`（B）、`eD`（P）、`d1`（D）都因外图来源被拒绝，
//!    且来源方向为 `EdgeTo`。
//! 2. 一次 `update_starmap_edge(R, edge, patch={to: R/[eB,EnterPortal(p2),eE]/Node(e1)})`
//!    只换终点：宿主 `R` 仍是同一条边，`to.segments` 更新为 `[eB, EnterPortal(p2), eE]`。
//! 3. 索引精确迁移：删 `p2`、`eE`、`e1` 分别因 `EdgeTo` 被拒绝；旧 `p1`、`eD`、`d1`
//!    已解除引用，可以直接删除。
//! 4. route-aware 反查按完整后缀隔离：经 `[eB,EnterPortal(p2),eE]` 进入 `E` 能反查到
//!    宿主 `R` 的更新后边；更新前的 `[eB,EnterPortal(p1),eD]` 进入 `D` 不再命中。
//! 5. 删除宿主边后，剩余的新终点 `p2`、`eE`、`e1` 均可删除。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tempfile::TempDir;
use writer_core::api::types::{
    CreateCrossCanvasEdgeResultDto, StarMapConnectionInfoDto, StarMapEdgeKindDto,
    StarMapEdgePatchDto, StarMapEndpointSelectionDto, StarMapNavigationRouteDto, StarMapNodeDto,
    StarMapNodeKindDto, StarMapPathSegmentDto, StarMapPointDto, StarMapPortalDto,
    StarMapTargetDetailDto, StarMapTargetPathDto,
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

fn selection(
    route: StarMapNavigationRouteDto,
    target: StarMapTargetDetailDto,
) -> StarMapEndpointSelectionDto {
    StarMapEndpointSelectionDto { route, target }
}

struct Fixture {
    _tmp: TempDir,
    api: WriterCoreApi,
    r: String,
    b: String,
    p: String,
    d: String,
    e: String,
    eb: String,
    ed: String,
    ee: String,
    edge: String,
}

impl Fixture {
    fn route_r(&self) -> StarMapNavigationRouteDto {
        route(&self.r, vec![])
    }

    fn route_via_p1(&self) -> StarMapNavigationRouteDto {
        route(
            &self.r,
            vec![
                enter_embed(&self.eb),
                enter_portal("p1"),
                enter_embed(&self.ed),
            ],
        )
    }

    fn route_via_p2(&self) -> StarMapNavigationRouteDto {
        route(
            &self.r,
            vec![
                enter_embed(&self.eb),
                enter_portal("p2"),
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

/// `R --eB--> B`；`B` 有 Portal `p1`/`p2` 指向 `P`；`P --eD--> D`、`P --eE--> E`。
fn setup() -> Fixture {
    let (tmp, api) = make_api();

    let root = api.create_starmap("根图R", "", None).unwrap().starmap_id;
    let b_child = api
        .create_starmap_child_embed(&root, "子图B", point(0.0, 0.0))
        .unwrap();
    let b = b_child.starmap.starmap_id.clone();
    let eb = b_child.embed.instance_id.clone();

    let p = api.create_starmap("子图P", "", None).unwrap().starmap_id;

    api.add_starmap_node(&root, node("r1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&b, node("b1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&b, portal_node("p1", &p), 0.0, 0.0)
        .unwrap();
    api.add_starmap_node(&b, portal_node("p2", &p), 0.0, 0.0)
        .unwrap();

    let d_child = api
        .create_starmap_child_embed(&p, "子图D", point(0.0, 0.0))
        .unwrap();
    let d = d_child.starmap.starmap_id.clone();
    let ed = d_child.embed.instance_id.clone();

    let e_child = api
        .create_starmap_child_embed(&p, "子图E", point(0.0, 0.0))
        .unwrap();
    let e = e_child.starmap.starmap_id.clone();
    let ee = e_child.embed.instance_id.clone();

    api.add_starmap_node(&d, node("d1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&e, node("e1"), 0.0, 0.0).unwrap();

    let edge = create_initial_edge(&api, &root, &eb, &ed);

    Fixture {
        _tmp: tmp,
        api,
        r: root,
        b,
        p,
        d,
        e,
        eb,
        ed,
        ee,
        edge,
    }
}

/// 创建 `Node(r1) →（经 eB、Portal p1、eD）Node(d1)`，断言宿主是根图 `R`。
fn create_initial_edge(api: &WriterCoreApi, root: &str, eb: &str, ed: &str) -> String {
    let created: CreateCrossCanvasEdgeResultDto = api
        .create_cross_canvas_edge(
            selection(route(root, vec![]), node_target("r1")),
            selection(
                route(
                    root,
                    vec![enter_embed(eb), enter_portal("p1"), enter_embed(ed)],
                ),
                node_target("d1"),
            ),
            StarMapEdgeKindDto::References,
            Some("穿越外图 Portal 与 Embed".to_string()),
            None,
        )
        .unwrap();

    assert_eq!(
        created.owner_starmap_id, root,
        "两端公共前缀为空，宿主应是根图 R"
    );
    assert_eq!(created.edge.from.starmap_id, root);
    assert!(created.edge.from.segments.is_empty());
    assert_eq!(created.edge.from.target, node_target("r1"));
    assert_eq!(created.edge.to.starmap_id, root);
    assert_eq!(
        created.edge.to.segments,
        vec![enter_embed(eb), enter_portal("p1"), enter_embed(ed)],
        "到达 d1 前的 Portal 段与两段 Embed 都必须写入 to.segments"
    );
    assert_eq!(created.edge.to.target, node_target("d1"));
    created.edge.id
}

/// 唯一真实 Edge 只落在根图 `R`，其余画布都不落假边。
fn assert_edge_only_in_root(f: &Fixture) {
    let r_graph = f.api.get_starmap_graph(&f.r).unwrap();
    assert_eq!(r_graph.edges.len(), 1, "宿主 R 必须只写入一条真实 Edge");
    assert_eq!(r_graph.edges[0].id, f.edge);

    for (id, label) in [
        (&f.b, "中间画布 B"),
        (&f.p, "Portal 目的地 P"),
        (&f.d, "起点画布 D"),
        (&f.e, "终点画布 E"),
    ] {
        assert!(
            f.api.get_starmap_graph(id).unwrap().edges.is_empty(),
            "{label} 不应写入伪造的本地 Edge"
        );
    }
}

/// 初始：外图 Portal `p1`、外图 Embed `eD` 与终点 `d1` 都受 `EdgeTo` 保护。
fn assert_initial_traversal_guards(f: &Fixture) {
    let blocked_p1 = f
        .api
        .delete_starmap_node(&f.b, "p1")
        .expect_err("被外图边途经的 Portal 节点 p1 不得被删除");
    let p1_message = format!("{blocked_p1:?}");
    assert!(
        p1_message.contains("EdgeTo"),
        "途经 p1 的来源方向应是 EdgeTo：{p1_message}"
    );
    assert!(
        p1_message.contains("EnterPortal") && p1_message.contains("p1"),
        "p1 的删除拒绝依据应是被边途经的 EnterPortal 路径：{p1_message}"
    );
    assert!(
        !p1_message.contains("EdgeFrom"),
        "p1 不应被误标为 EdgeFrom：{p1_message}"
    );

    let blocked_ed = f
        .api
        .delete_starmap_embed(&f.p, &f.ed)
        .expect_err("被外图边途经的 Embed eD 不得被删除");
    let ed_message = format!("{blocked_ed:?}");
    assert!(
        ed_message.contains("EdgeTo"),
        "途经 eD 的来源方向应是 EdgeTo：{ed_message}"
    );
    assert!(
        ed_message.contains(&f.ed),
        "eD 的删除拒绝依据应是被边途经的 Embed 实例 {ed}：{ed_message}",
        ed = f.ed
    );

    let blocked_d1 = f
        .api
        .delete_starmap_node(&f.d, "d1")
        .expect_err("d1 是跨画布边终点，删除必须被拒绝");
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

/// 一次 update 把终点换成经 `p2`/`eE` 到 `e1` 的路径；宿主仍是 `R`，边不复制。
fn repoint_to_via_p2(f: &Fixture) {
    let updated = f
        .api
        .update_starmap_edge(
            &f.r,
            &f.edge,
            StarMapEdgePatchDto {
                kind: Some(StarMapEdgeKindDto::Foreshadows),
                label: Some(Some("终点改走 p2/eE".to_string())),
                payload: None,
                from: None,
                to: Some(path(
                    &f.r,
                    vec![enter_embed(&f.eb), enter_portal("p2"), enter_embed(&f.ee)],
                    node_target("e1"),
                )),
            },
        )
        .unwrap();

    assert_eq!(updated.kind, StarMapEdgeKindDto::Foreshadows);
    assert_eq!(updated.label.as_deref(), Some("终点改走 p2/eE"));
    assert_eq!(updated.from.starmap_id, f.r);
    assert!(updated.from.segments.is_empty());
    assert_eq!(updated.from.target, node_target("r1"));
    assert_eq!(updated.to.starmap_id, f.r);
    assert_eq!(
        updated.to.segments,
        vec![enter_embed(&f.eb), enter_portal("p2"), enter_embed(&f.ee)],
        "重定向后 to 端穿越路径应换成新的 Portal+Embed 段"
    );
    assert_eq!(updated.to.target, node_target("e1"));

    let r_graph = f.api.get_starmap_graph(&f.r).unwrap();
    assert_eq!(r_graph.edges.len(), 1, "只换终点不应新增或复制边");
    assert_eq!(r_graph.edges[0].id, f.edge);
}

/// 重定向后：新 Portal/Embed/终点被保护，旧 Portal/Embed/终点已解除引用。
fn assert_traversal_guards_after_repoint(f: &Fixture) {
    let blocked_p2 = f
        .api
        .delete_starmap_node(&f.b, "p2")
        .expect_err("新穿越的 Portal 节点 p2 删除必须被拒绝");
    let p2_message = format!("{blocked_p2:?}");
    assert!(
        p2_message.contains("EdgeTo"),
        "途经 p2 的来源方向应是 EdgeTo：{p2_message}"
    );
    assert!(
        p2_message.contains("EnterPortal") && p2_message.contains("p2"),
        "p2 的删除拒绝依据应是被边途经的 EnterPortal 路径：{p2_message}"
    );
    assert!(
        !p2_message.contains("EdgeFrom"),
        "p2 不应被误标为 EdgeFrom：{p2_message}"
    );

    let blocked_ee = f
        .api
        .delete_starmap_embed(&f.p, &f.ee)
        .expect_err("新穿越的 Embed eE 删除必须被拒绝");
    let ee_message = format!("{blocked_ee:?}");
    assert!(
        ee_message.contains("EdgeTo"),
        "途经 eE 的来源方向应是 EdgeTo：{ee_message}"
    );
    assert!(
        ee_message.contains(&f.ee),
        "eE 的删除拒绝依据应是被边途经的 Embed 实例 {ee}：{ee_message}",
        ee = f.ee
    );

    let blocked_e1 = f
        .api
        .delete_starmap_node(&f.e, "e1")
        .expect_err("e1 成为新终点，删除必须被拒绝");
    let e1_message = format!("{blocked_e1:?}");
    assert!(
        e1_message.contains("EdgeTo"),
        "e1 的引用来源应是 EdgeTo：{e1_message}"
    );
    assert!(
        !e1_message.contains("EdgeFrom"),
        "e1 不应残留相反方向的 EdgeFrom 来源：{e1_message}"
    );

    // 旧穿越来源必须已从索引移除，可以直接删除。
    assert!(
        f.api.delete_starmap_node(&f.b, "p1").unwrap(),
        "旧 Portal p1 不再被引用，应可删除"
    );
    assert!(
        f.api.delete_starmap_embed(&f.p, &f.ed).unwrap(),
        "旧 Embed eD 不再被引用，应可删除"
    );
    assert!(
        f.api.delete_starmap_node(&f.d, "d1").unwrap(),
        "旧终点 d1 不再被引用，应可删除"
    );
}

/// route-aware 反查与 target 精确过滤：新路径命中，旧路径不再命中。
fn assert_queries_after_repoint(f: &Fixture) {
    let via_e = f.canvas(f.route_via_p2());
    let conn_e = via_e
        .iter()
        .find(|c| c.edge_id == f.edge)
        .expect("经 [eB,EnterPortal(p2),eE] 进入 E 应反查到宿主 R 的更新后边");
    assert_eq!(conn_e.owner_starmap_id, f.r);
    assert_eq!(
        conn_e.to.segments,
        vec![enter_embed(&f.eb), enter_portal("p2"), enter_embed(&f.ee)]
    );
    assert_eq!(conn_e.to.target, node_target("e1"));

    assert!(
        !contains(&f.canvas(f.route_via_p1()), &f.edge),
        "旧路径经 p1+eD 不应再命中已重定向的边"
    );

    assert!(
        contains(&f.canvas(f.route_r()), &f.edge),
        "宿主 R 本图的画布查询应包含自己持有的边"
    );

    let e1_targets = f.target(f.route_via_p2(), node_target("e1"));
    assert_eq!(
        ids(&e1_targets),
        vec![f.edge.as_str()],
        "新终点 e1 的 target 查询应只命中该边"
    );
    assert!(
        f.target(f.route_via_p1(), node_target("d1")).is_empty(),
        "旧终点 d1 的 target 查询应变空"
    );
}

/// 删除宿主边后，剩余的新 Portal/Embed/终点均可删除，`R` 不残留 Edge。
fn assert_delete_edge_frees_new_entries(f: &Fixture) {
    assert!(
        f.api.delete_starmap_edge(&f.r, &f.edge).unwrap(),
        "删除宿主 R 中的边应返回 true"
    );
    assert!(
        f.api.delete_starmap_node(&f.b, "p2").unwrap(),
        "删除原始边后新 Portal p2 应可删除"
    );
    assert!(
        f.api.delete_starmap_embed(&f.p, &f.ee).unwrap(),
        "删除原始边后新 Embed eE 应可删除"
    );
    assert!(
        f.api.delete_starmap_node(&f.e, "e1").unwrap(),
        "删除原始边后新终点 e1 应可删除"
    );
    assert!(
        f.api.get_starmap_graph(&f.r).unwrap().edges.is_empty(),
        "宿主边已删除，R 不应残留 Edge"
    );
}

fn contains(connections: &[StarMapConnectionInfoDto], id: &str) -> bool {
    connections.iter().any(|c| c.edge_id == id)
}

fn ids(connections: &[StarMapConnectionInfoDto]) -> Vec<&str> {
    connections.iter().map(|c| c.edge_id.as_str()).collect()
}

/// 重定向穿越外图 Portal/Embed 的跨画布边：遍历依赖的旧来源清理与新来源登记。
#[test]
fn repoint_traversed_external_entries_migrates_traversal_index() {
    let f = setup();

    assert_edge_only_in_root(&f);
    assert_initial_traversal_guards(&f);

    repoint_to_via_p2(&f);
    assert_queries_after_repoint(&f);
    assert_traversal_guards_after_repoint(&f);

    assert_delete_edge_frees_new_entries(&f);
}
