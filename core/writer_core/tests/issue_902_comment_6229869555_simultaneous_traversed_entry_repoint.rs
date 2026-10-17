//! Issue #902 评论 6229869555 — 一次 `update_starmap_edge` **同时重定向两端**，
//! 且两端在到达终点前都**穿越外图 Portal 与外图 Embed** 时，路径依赖索引必须把
//! `EdgeFrom` 与 `EdgeTo` 两套穿越来源同时迁移。
//!
//! #902 实现路线第 3 节要求反向依赖索引在写入/修改/删除后增量更新：先按来源
//! `(owner, source_kind, source_id)` 移除旧事实，再插入新事实。此前各轮经真实
//! `WriterCoreApi` 覆盖的重定向组合是分开的：
//!
//! - 评论 6216141983：同一次 update 两端对调，但端点残留路径只有 **Embed 段**；
//! - 评论 6222327395：端点穿越外图 Portal + Embed，但只重定向 **`to` 端**。
//!
//! **尚未覆盖**「一次 update 同时重定向两端，且每端都穿越外图 Portal + Embed」：
//! 此时 `facade/starmap_ops/graph.rs::update_starmap_edge` 会分别调用
//! `replace_source_facts_in_path_dependency_index(EdgeFrom, ...)` 与
//! `(EdgeTo, ...)`，而两端的旧 `PortalTraversal`/`EmbedTraversal`/`NodeTarget` 都要
//! 被移除、新的一组都要被登记。任一端漏替换，就会出现「旧穿越入口仍被拦截 /
//! 新穿越入口不被保护」的悬空或漏保护引用。
//!
//! 拓扑：根图 `R` 有节点 `r1`，并经 `Embed(eB)` 进入 `B`、经 `Embed(eC)` 进入 `C`；
//! `B` 的 Portal `p1`/`p2` 均指向独立画布 `P`，`P` 有 `Embed(eD) --> D`、
//! `Embed(eE) --> E`；`C` 的 Portal `q1`/`q2` 均指向独立画布 `Q`，`Q` 有
//! `Embed(eF) --> F`、`Embed(eG) --> G`。初始一条存于 `R` 的跨画布边：
//! `from = R/[eB, EnterPortal(p1), eD]/Node(d1)`、
//! `to   = R/[eC, EnterPortal(q1), eF]/Node(f1)`，公共前缀为空，宿主为 `R`：
//!
//! 1. 初始 `from` 的穿越事实为 `EmbedTraversal(R,eB)`、`PortalTraversal(B,p1)`、
//!    `EmbedTraversal(P,eD)` 加终点 `NodeTarget(D,d1)`；删除 `p1`（B）、`eD`（P）、
//!    `d1`（D）都因外图 `EdgeFrom` 引用被拒绝。
//! 2. 初始 `to` 的穿越事实为 `EmbedTraversal(R,eC)`、`PortalTraversal(C,q1)`、
//!    `EmbedTraversal(Q,eF)` 加终点 `NodeTarget(F,f1)`；删除 `q1`（C）、`eF`（Q）、
//!    `f1`（F）都因外图 `EdgeTo` 引用被拒绝。
//! 3. 一次 `update_starmap_edge(R, edge, patch={from: R/[eB,EnterPortal(p2),eE]/Node(e1),
//!    to: R/[eC,EnterPortal(q2),eG]/Node(g1)})` 同时换两端：宿主 `R` 仍是同一条边。
//! 4. 索引精确迁移：新 `p2`/`eE`/`e1` 因 `EdgeFrom` 被拒绝、新 `q2`/`eG`/`g1` 因
//!    `EdgeTo` 被拒绝；旧 `p1`/`eD`/`d1` 与 `q1`/`eF`/`f1` 已解除引用，可直接删除。
//! 5. route-aware 反查按完整后缀隔离：经 `[eB,EnterPortal(p2),eE]` 进入 `E` 与经
//!    `[eC,EnterPortal(q2),eG]` 进入 `G` 都能反查到宿主 `R` 的更新后边；更新前的
//!    `[eB,EnterPortal(p1),eD]`（D）与 `[eC,EnterPortal(q1),eF]`（F）不再命中。
//! 6. 删除宿主边后，剩余的新穿越入口都可删除，`R` 不残留 Edge。

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
    c: String,
    q: String,
    f: String,
    g: String,
    eb: String,
    ec: String,
    ed: String,
    ee: String,
    ef: String,
    eg: String,
    edge: String,
}

impl Fixture {
    fn route_r(&self) -> StarMapNavigationRouteDto {
        route(&self.r, vec![])
    }

    /// 初始 `from` 端：经 `[eB, EnterPortal(p1), eD]` 到达 `D`。
    fn route_from_old(&self) -> StarMapNavigationRouteDto {
        route(
            &self.r,
            vec![
                enter_embed(&self.eb),
                enter_portal("p1"),
                enter_embed(&self.ed),
            ],
        )
    }

    /// 更新后 `from` 端：经 `[eB, EnterPortal(p2), eE]` 到达 `E`。
    fn route_from_new(&self) -> StarMapNavigationRouteDto {
        route(
            &self.r,
            vec![
                enter_embed(&self.eb),
                enter_portal("p2"),
                enter_embed(&self.ee),
            ],
        )
    }

    /// 初始 `to` 端：经 `[eC, EnterPortal(q1), eF]` 到达 `F`。
    fn route_to_old(&self) -> StarMapNavigationRouteDto {
        route(
            &self.r,
            vec![
                enter_embed(&self.ec),
                enter_portal("q1"),
                enter_embed(&self.ef),
            ],
        )
    }

    /// 更新后 `to` 端：经 `[eC, EnterPortal(q2), eG]` 到达 `G`。
    fn route_to_new(&self) -> StarMapNavigationRouteDto {
        route(
            &self.r,
            vec![
                enter_embed(&self.ec),
                enter_portal("q2"),
                enter_embed(&self.eg),
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

/// `R --eB--> B --p*--> P --eD/eE--> D/E` 与 `R --eC--> C --q*--> Q --eF/eG--> F/G`。
fn setup() -> Fixture {
    let (tmp, api) = make_api();

    let root = api.create_starmap("根图R", "", None).unwrap().starmap_id;

    let b_child = api
        .create_starmap_child_embed(&root, "子图B", point(0.0, 0.0))
        .unwrap();
    let canvas_b = b_child.starmap.starmap_id.clone();
    let eb = b_child.embed.instance_id.clone();

    let c_child = api
        .create_starmap_child_embed(&root, "子图C", point(0.0, 0.0))
        .unwrap();
    let canvas_c = c_child.starmap.starmap_id.clone();
    let ec = c_child.embed.instance_id.clone();

    let canvas_p = api.create_starmap("画布P", "", None).unwrap().starmap_id;
    let canvas_q = api.create_starmap("画布Q", "", None).unwrap().starmap_id;

    api.add_starmap_node(&root, node("r1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&canvas_b, portal_node("p1", &canvas_p), 0.0, 0.0)
        .unwrap();
    api.add_starmap_node(&canvas_b, portal_node("p2", &canvas_p), 0.0, 0.0)
        .unwrap();
    api.add_starmap_node(&canvas_c, portal_node("q1", &canvas_q), 0.0, 0.0)
        .unwrap();
    api.add_starmap_node(&canvas_c, portal_node("q2", &canvas_q), 0.0, 0.0)
        .unwrap();

    let d_child = api
        .create_starmap_child_embed(&canvas_p, "子图D", point(0.0, 0.0))
        .unwrap();
    let canvas_d = d_child.starmap.starmap_id.clone();
    let ed = d_child.embed.instance_id.clone();

    let e_child = api
        .create_starmap_child_embed(&canvas_p, "子图E", point(0.0, 0.0))
        .unwrap();
    let canvas_e = e_child.starmap.starmap_id.clone();
    let ee = e_child.embed.instance_id.clone();

    let f_child = api
        .create_starmap_child_embed(&canvas_q, "子图F", point(0.0, 0.0))
        .unwrap();
    let canvas_f = f_child.starmap.starmap_id.clone();
    let ef = f_child.embed.instance_id.clone();

    let g_child = api
        .create_starmap_child_embed(&canvas_q, "子图G", point(0.0, 0.0))
        .unwrap();
    let canvas_g = g_child.starmap.starmap_id.clone();
    let eg = g_child.embed.instance_id.clone();

    api.add_starmap_node(&canvas_d, node("d1"), 0.0, 0.0)
        .unwrap();
    api.add_starmap_node(&canvas_e, node("e1"), 0.0, 0.0)
        .unwrap();
    api.add_starmap_node(&canvas_f, node("f1"), 0.0, 0.0)
        .unwrap();
    api.add_starmap_node(&canvas_g, node("g1"), 0.0, 0.0)
        .unwrap();

    let edge = create_initial_edge(&api, &root, &eb, &ec, &ed, &ef);

    Fixture {
        _tmp: tmp,
        api,
        r: root,
        b: canvas_b,
        p: canvas_p,
        d: canvas_d,
        e: canvas_e,
        c: canvas_c,
        q: canvas_q,
        f: canvas_f,
        g: canvas_g,
        eb,
        ec,
        ed,
        ee,
        ef,
        eg,
        edge,
    }
}

/// 创建 `Node(d1) ←(经 eB/P·p1/eD)— R —(经 eC/Q·q1/eF)→ Node(f1)`，断言宿主是根图。
fn create_initial_edge(
    api: &WriterCoreApi,
    root: &str,
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
                    vec![enter_embed(eb), enter_portal("p1"), enter_embed(ed)],
                ),
                node_target("d1"),
            ),
            selection(
                route(
                    root,
                    vec![enter_embed(ec), enter_portal("q1"), enter_embed(ef)],
                ),
                node_target("f1"),
            ),
            StarMapEdgeKindDto::References,
            Some("两端都穿越外图 Portal/Embed".to_string()),
            None,
        )
        .unwrap();

    assert_eq!(
        created.owner_starmap_id, root,
        "两端公共前缀为空，宿主应是根图 R"
    );
    assert_eq!(created.edge.from.starmap_id, root);
    assert_eq!(
        created.edge.from.segments,
        vec![enter_embed(eb), enter_portal("p1"), enter_embed(ed)],
        "from 端 Portal 段与两段 Embed 都必须写入 from.segments"
    );
    assert_eq!(created.edge.from.target, node_target("d1"));
    assert_eq!(created.edge.to.starmap_id, root);
    assert_eq!(
        created.edge.to.segments,
        vec![enter_embed(ec), enter_portal("q1"), enter_embed(ef)],
        "to 端 Portal 段与两段 Embed 都必须写入 to.segments"
    );
    assert_eq!(created.edge.to.target, node_target("f1"));
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
        (&f.e, "起点画布 E"),
        (&f.c, "中间画布 C"),
        (&f.q, "Portal 目的地 Q"),
        (&f.f, "终点画布 F"),
        (&f.g, "终点画布 G"),
    ] {
        assert!(
            f.api.get_starmap_graph(id).unwrap().edges.is_empty(),
            "{label} 不应写入伪造的本地 Edge"
        );
    }
}

/// 初始：`from` 端穿越来源（p1/eD/d1）由 `EdgeFrom` 保护，
/// `to` 端穿越来源（q1/eF/f1）由 `EdgeTo` 保护。
fn assert_initial_guards(f: &Fixture) {
    // from 端：外图 Portal p1。
    let p1 = f
        .api
        .delete_starmap_node(&f.b, "p1")
        .expect_err("被外图边 from 途经的 Portal 节点 p1 不得被删除");
    let p1_message = format!("{p1:?}");
    assert!(
        p1_message.contains("EdgeFrom"),
        "途经 p1 的来源方向应是 EdgeFrom：{p1_message}"
    );
    assert!(
        p1_message.contains("EnterPortal") && p1_message.contains("p1"),
        "p1 的删除拒绝依据应是被边途经的 EnterPortal 路径：{p1_message}"
    );
    assert!(
        !p1_message.contains("EdgeTo"),
        "p1 不应被误标为 EdgeTo：{p1_message}"
    );

    // from 端：外图 Embed eD。
    let ed = f
        .api
        .delete_starmap_embed(&f.p, &f.ed)
        .expect_err("被外图边 from 途经的 Embed eD 不得被删除");
    let ed_message = format!("{ed:?}");
    assert!(
        ed_message.contains("EdgeFrom"),
        "途经 eD 的来源方向应是 EdgeFrom：{ed_message}"
    );
    assert!(
        ed_message.contains(&f.ed),
        "eD 的删除拒绝依据应是被边途经的 Embed 实例：{ed_message}"
    );

    // from 端：终点 d1。
    let d1 = f
        .api
        .delete_starmap_node(&f.d, "d1")
        .expect_err("d1 是跨画布边 from 终点，删除必须被拒绝");
    let d1_message = format!("{d1:?}");
    assert!(
        d1_message.contains("EdgeFrom"),
        "d1 的引用来源应是 EdgeFrom：{d1_message}"
    );
    assert!(
        !d1_message.contains("EdgeTo"),
        "d1 不应被误标为 EdgeTo：{d1_message}"
    );

    // to 端：外图 Portal q1。
    let q1 = f
        .api
        .delete_starmap_node(&f.c, "q1")
        .expect_err("被外图边 to 途经的 Portal 节点 q1 不得被删除");
    let q1_message = format!("{q1:?}");
    assert!(
        q1_message.contains("EdgeTo"),
        "途经 q1 的来源方向应是 EdgeTo：{q1_message}"
    );
    assert!(
        q1_message.contains("EnterPortal") && q1_message.contains("q1"),
        "q1 的删除拒绝依据应是被边途经的 EnterPortal 路径：{q1_message}"
    );
    assert!(
        !q1_message.contains("EdgeFrom"),
        "q1 不应被误标为 EdgeFrom：{q1_message}"
    );

    // to 端：外图 Embed eF。
    let ef = f
        .api
        .delete_starmap_embed(&f.q, &f.ef)
        .expect_err("被外图边 to 途经的 Embed eF 不得被删除");
    let ef_message = format!("{ef:?}");
    assert!(
        ef_message.contains("EdgeTo"),
        "途经 eF 的来源方向应是 EdgeTo：{ef_message}"
    );
    assert!(
        ef_message.contains(&f.ef),
        "eF 的删除拒绝依据应是被边途经的 Embed 实例：{ef_message}"
    );

    // to 端：终点 f1。
    let f1 = f
        .api
        .delete_starmap_node(&f.f, "f1")
        .expect_err("f1 是跨画布边 to 终点，删除必须被拒绝");
    let f1_message = format!("{f1:?}");
    assert!(
        f1_message.contains("EdgeTo"),
        "f1 的引用来源应是 EdgeTo：{f1_message}"
    );
    assert!(
        !f1_message.contains("EdgeFrom"),
        "f1 不应被误标为 EdgeFrom：{f1_message}"
    );
}

/// 一次 update 同时把两端换成经新 Portal/Embed 的路径；宿主仍是 `R`，边不复制。
fn repoint_both_ends(f: &Fixture) {
    let updated = f
        .api
        .update_starmap_edge(
            &f.r,
            &f.edge,
            StarMapEdgePatchDto {
                kind: Some(StarMapEdgeKindDto::Foreshadows),
                label: Some(Some("两端同时换穿越入口".to_string())),
                payload: None,
                from: Some(path(
                    &f.r,
                    vec![enter_embed(&f.eb), enter_portal("p2"), enter_embed(&f.ee)],
                    node_target("e1"),
                )),
                to: Some(path(
                    &f.r,
                    vec![enter_embed(&f.ec), enter_portal("q2"), enter_embed(&f.eg)],
                    node_target("g1"),
                )),
            },
        )
        .unwrap();

    assert_eq!(updated.kind, StarMapEdgeKindDto::Foreshadows);
    assert_eq!(updated.label.as_deref(), Some("两端同时换穿越入口"));
    assert_eq!(updated.from.starmap_id, f.r);
    assert_eq!(
        updated.from.segments,
        vec![enter_embed(&f.eb), enter_portal("p2"), enter_embed(&f.ee)],
        "重定向后 from 端穿越路径应换成新的 Portal+Embed 段"
    );
    assert_eq!(updated.from.target, node_target("e1"));
    assert_eq!(updated.to.starmap_id, f.r);
    assert_eq!(
        updated.to.segments,
        vec![enter_embed(&f.ec), enter_portal("q2"), enter_embed(&f.eg)],
        "重定向后 to 端穿越路径应换成新的 Portal+Embed 段"
    );
    assert_eq!(updated.to.target, node_target("g1"));

    let r_graph = f.api.get_starmap_graph(&f.r).unwrap();
    assert_eq!(r_graph.edges.len(), 1, "同时改两端不应新增或复制边");
    assert_eq!(r_graph.edges[0].id, f.edge);
}

/// 重定向后：新 from 端穿越来源由 `EdgeFrom` 保护、新 to 端由 `EdgeTo` 保护。
fn assert_guards_after_repoint(f: &Fixture) {
    // 新 from 端：Portal p2。
    let p2 = f
        .api
        .delete_starmap_node(&f.b, "p2")
        .expect_err("新穿越的 Portal 节点 p2 删除必须被拒绝");
    let p2_message = format!("{p2:?}");
    assert!(
        p2_message.contains("EdgeFrom"),
        "新 p2 的来源方向应是 EdgeFrom：{p2_message}"
    );
    assert!(
        p2_message.contains("EnterPortal") && p2_message.contains("p2"),
        "p2 的删除拒绝依据应是被边途经的 EnterPortal 路径：{p2_message}"
    );
    assert!(
        !p2_message.contains("EdgeTo"),
        "p2 不应被误标为 EdgeTo：{p2_message}"
    );

    // 新 from 端：Embed eE。
    let ee = f
        .api
        .delete_starmap_embed(&f.p, &f.ee)
        .expect_err("新穿越的 Embed eE 删除必须被拒绝");
    let ee_message = format!("{ee:?}");
    assert!(
        ee_message.contains("EdgeFrom"),
        "新 eE 的来源方向应是 EdgeFrom：{ee_message}"
    );
    assert!(
        ee_message.contains(&f.ee),
        "eE 的删除拒绝依据应是被边途经的 Embed 实例：{ee_message}"
    );

    // 新 from 端：终点 e1。
    let e1 = f
        .api
        .delete_starmap_node(&f.e, "e1")
        .expect_err("e1 成为新 from 终点，删除必须被拒绝");
    let e1_message = format!("{e1:?}");
    assert!(
        e1_message.contains("EdgeFrom"),
        "e1 的引用来源应是 EdgeFrom：{e1_message}"
    );
    assert!(
        !e1_message.contains("EdgeTo"),
        "e1 不应被误标为 EdgeTo：{e1_message}"
    );

    // 新 to 端：Portal q2。
    let q2 = f
        .api
        .delete_starmap_node(&f.c, "q2")
        .expect_err("新穿越的 Portal 节点 q2 删除必须被拒绝");
    let q2_message = format!("{q2:?}");
    assert!(
        q2_message.contains("EdgeTo"),
        "新 q2 的来源方向应是 EdgeTo：{q2_message}"
    );
    assert!(
        q2_message.contains("EnterPortal") && q2_message.contains("q2"),
        "q2 的删除拒绝依据应是被边途经的 EnterPortal 路径：{q2_message}"
    );
    assert!(
        !q2_message.contains("EdgeFrom"),
        "q2 不应被误标为 EdgeFrom：{q2_message}"
    );

    // 新 to 端：Embed eG。
    let eg = f
        .api
        .delete_starmap_embed(&f.q, &f.eg)
        .expect_err("新穿越的 Embed eG 删除必须被拒绝");
    let eg_message = format!("{eg:?}");
    assert!(
        eg_message.contains("EdgeTo"),
        "新 eG 的来源方向应是 EdgeTo：{eg_message}"
    );
    assert!(
        eg_message.contains(&f.eg),
        "eG 的删除拒绝依据应是被边途经的 Embed 实例：{eg_message}"
    );

    // 新 to 端：终点 g1。
    let g1 = f
        .api
        .delete_starmap_node(&f.g, "g1")
        .expect_err("g1 成为新 to 终点，删除必须被拒绝");
    let g1_message = format!("{g1:?}");
    assert!(
        g1_message.contains("EdgeTo"),
        "g1 的引用来源应是 EdgeTo：{g1_message}"
    );
    assert!(
        !g1_message.contains("EdgeFrom"),
        "g1 不应被误标为 EdgeFrom：{g1_message}"
    );
}

/// route-aware 反查与 target 过滤：新路径命中，旧路径不再命中（在释放旧入口前）。
fn assert_queries_after_repoint(f: &Fixture) {
    // 新 from 路径进入 E。
    let e_canvas = f.canvas(f.route_from_new());
    let e_conn = e_canvas
        .iter()
        .find(|c| c.edge_id == f.edge)
        .expect("经 [eB,EnterPortal(p2),eE] 进入 E 应反查到宿主 R 的更新后边");
    assert_eq!(e_conn.owner_starmap_id, f.r);
    assert_eq!(
        e_conn.from.segments,
        vec![enter_embed(&f.eb), enter_portal("p2"), enter_embed(&f.ee)]
    );
    assert_eq!(e_conn.from.target, node_target("e1"));

    // 新 to 路径进入 G。
    let g_canvas = f.canvas(f.route_to_new());
    let g_conn = g_canvas
        .iter()
        .find(|c| c.edge_id == f.edge)
        .expect("经 [eC,EnterPortal(q2),eG] 进入 G 应反查到宿主 R 的更新后边");
    assert_eq!(g_conn.owner_starmap_id, f.r);
    assert_eq!(
        g_conn.to.segments,
        vec![enter_embed(&f.ec), enter_portal("q2"), enter_embed(&f.eg)]
    );
    assert_eq!(g_conn.to.target, node_target("g1"));

    // 旧的 from/to 路径都不再命中。
    assert!(
        !contains(&f.canvas(f.route_from_old()), &f.edge),
        "旧 from 路径经 p1+eD 不应再命中已重定向的边"
    );
    assert!(
        !contains(&f.canvas(f.route_to_old()), &f.edge),
        "旧 to 路径经 q1+eF 不应再命中已重定向的边"
    );

    // 宿主 R 本图画布查询应包含自己持有的边。
    assert!(
        contains(&f.canvas(f.route_r()), &f.edge),
        "宿主 R 的画布查询应包含自己持有的边"
    );

    // target 精确过滤。
    assert_eq!(
        ids(&f.target(f.route_from_new(), node_target("e1"))),
        vec![f.edge.as_str()],
        "新 from 终点 e1 的 target 查询应只命中该边"
    );
    assert_eq!(
        ids(&f.target(f.route_to_new(), node_target("g1"))),
        vec![f.edge.as_str()],
        "新 to 终点 g1 的 target 查询应只命中该边"
    );
    assert!(
        f.target(f.route_from_old(), node_target("d1")).is_empty(),
        "旧 from 终点 d1 的 target 查询应变空"
    );
    assert!(
        f.target(f.route_to_old(), node_target("f1")).is_empty(),
        "旧 to 终点 f1 的 target 查询应变空"
    );
}

/// 旧的两组穿越来源必须已从索引移除，可以直接删除。
fn assert_old_entries_freed(f: &Fixture) {
    assert!(
        f.api.delete_starmap_node(&f.b, "p1").unwrap(),
        "旧 from Portal p1 不再被引用，应可删除"
    );
    assert!(
        f.api.delete_starmap_embed(&f.p, &f.ed).unwrap(),
        "旧 from Embed eD 不再被引用，应可删除"
    );
    assert!(
        f.api.delete_starmap_node(&f.d, "d1").unwrap(),
        "旧 from 终点 d1 不再被引用，应可删除"
    );
    assert!(
        f.api.delete_starmap_node(&f.c, "q1").unwrap(),
        "旧 to Portal q1 不再被引用，应可删除"
    );
    assert!(
        f.api.delete_starmap_embed(&f.q, &f.ef).unwrap(),
        "旧 to Embed eF 不再被引用，应可删除"
    );
    assert!(
        f.api.delete_starmap_node(&f.f, "f1").unwrap(),
        "旧 to 终点 f1 不再被引用，应可删除"
    );
}

/// 删除宿主边后，剩余的新穿越入口均可删除，`R` 不残留 Edge。
fn assert_delete_edge_frees_new_entries(f: &Fixture) {
    assert!(
        f.api.delete_starmap_edge(&f.r, &f.edge).unwrap(),
        "删除宿主 R 中的边应返回 true"
    );
    assert!(
        f.api.delete_starmap_node(&f.b, "p2").unwrap(),
        "删除原始边后新 from Portal p2 应可删除"
    );
    assert!(
        f.api.delete_starmap_embed(&f.p, &f.ee).unwrap(),
        "删除原始边后新 from Embed eE 应可删除"
    );
    assert!(
        f.api.delete_starmap_node(&f.e, "e1").unwrap(),
        "删除原始边后新 from 终点 e1 应可删除"
    );
    assert!(
        f.api.delete_starmap_node(&f.c, "q2").unwrap(),
        "删除原始边后新 to Portal q2 应可删除"
    );
    assert!(
        f.api.delete_starmap_embed(&f.q, &f.eg).unwrap(),
        "删除原始边后新 to Embed eG 应可删除"
    );
    assert!(
        f.api.delete_starmap_node(&f.g, "g1").unwrap(),
        "删除原始边后新 to 终点 g1 应可删除"
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

/// 同时重定向两端、且两端都穿越外图 Portal/Embed：两套穿越依赖同时迁移。
#[test]
fn simultaneous_repoint_of_both_traversed_ends_migrates_index() {
    let f = setup();

    assert_edge_only_in_root(&f);
    assert_initial_guards(&f);

    repoint_both_ends(&f);
    assert_guards_after_repoint(&f);
    assert_queries_after_repoint(&f);
    assert_old_entries_freed(&f);

    assert_delete_edge_frees_new_entries(&f);
}
