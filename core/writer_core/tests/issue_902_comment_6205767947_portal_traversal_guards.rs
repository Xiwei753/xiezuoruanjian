//! Issue #902 评论 6205767947 — Portal 穿行画布的跨层连线与跨图引用保护集成测试。
//!
//! 第 32/33 轮已用真实 `WriterCoreApi` 覆盖 **Embed** 进入的跨画布连线、关联反查
//! 与不同根拒绝；本测试补齐同一 #902 语义中 **Portal**（`EnterPortal`）这条穿行
//! 路径的对外 API 组合验证，以及它触发的跨图引用删除/修改保护：
//!
//! 1. 根图 A `Embed` 子图 B，B 中 Portal `p1` 指向 C。A 内节点 → C 内节点的边，
//!    宿主图是最长公共实例路径前缀对应的 A，`to.segments` 记录 `[EnterEmbed(eB),
//!    EnterPortal(p1)]`；A 只写一条真实 Edge，B/C 不写假边。
//! 2. C 画布能反查这条边；同一 B 经另一个 Portal `p2` 进入 C 的访问上下文看不到它
//!    （实例路径后缀区分，不能只按落点星图匹配）。
//! 3. 删除被 A 的边穿越的 Portal 节点 `p1` 必须被拒绝；未被引用的 `p2` 可正常删除。
//! 4. 重定目标或清空被外部路径穿越的 Portal 也必须被拒绝，不能先改再让 A 的边悬空。
//! 5. `enter_starmap_portal` / `resolve_starmap_navigation` / `exit_starmap_canvas`
//!    经真实 API 的 Portal 穿行与返回层级闭环。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tempfile::TempDir;
use writer_core::api::types::{
    StarMapEdgeKindDto, StarMapEmbedDto, StarMapEndpointSelectionDto, StarMapNavigationRouteDto,
    StarMapNodeDto, StarMapNodeKindDto, StarMapNodePatchDto, StarMapPathSegmentDto,
    StarMapPointDto, StarMapPortalDto, StarMapTargetDetailDto, StarMapTargetPathDto,
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

fn empty_patch() -> StarMapNodePatchDto {
    StarMapNodePatchDto {
        title: None,
        kind: None,
        payload: None,
        tags: None,
        content: None,
        anchors: None,
        portal: None,
        position: None,
        style: None,
        provenance: None,
    }
}

fn portal_to(starmap_id: &str) -> StarMapPortalDto {
    StarMapPortalDto {
        destination_starmap_id: starmap_id.to_string(),
        destination_target: None,
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
) -> StarMapEndpointSelectionDto {
    StarMapEndpointSelectionDto { route, target }
}

struct Fixture {
    _tmp: TempDir,
    api: WriterCoreApi,
    a: String,
    b: String,
    c: String,
    eb: String,
}

/// 构造根图 A → Embed eB → 子图 B → Portal p1/p2 → 子图 C 的三层结构。
fn make_portal_fixture() -> Fixture {
    let (tmp, api) = make_api();

    let a = api.create_starmap("A", "", None).unwrap().starmap_id;
    let child = api
        .create_starmap_child_embed(&a, "B", point(0.0, 0.0))
        .unwrap();
    let b = child.starmap.starmap_id.clone();
    let eb = child.embed.instance_id.clone();
    let c = api.create_starmap("C", "", None).unwrap().starmap_id;

    for (portal_id, dest) in [("p1", c.as_str()), ("p2", c.as_str())] {
        let mut p = node(portal_id);
        p.portal = Some(portal_to(dest));
        api.add_starmap_node(&b, p, 0.0, 0.0).unwrap();
    }
    api.add_starmap_node(&a, node("a1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&c, node("c1"), 0.0, 0.0).unwrap();

    Fixture {
        _tmp: tmp,
        api,
        a,
        b,
        c,
        eb,
    }
}

/// 经 Portal 穿行的跨层边写在公共宿主图 A，C 能反查，另一 Portal 入口看不到。
#[test]
fn portal_edge_is_written_to_host_and_queryable_from_child() {
    let f = make_portal_fixture();

    let from = selection(route(&f.a, vec![]), node_target("a1"));
    let to = selection(
        route(&f.a, vec![enter_embed(&f.eb), enter_portal("p1")]),
        node_target("c1"),
    );
    let created = f
        .api
        .create_cross_canvas_edge(from, to, StarMapEdgeKindDto::References, None, None)
        .unwrap();

    // 唯一一条真实 Edge 写在宿主图 A，穿越路径（Embed + Portal）完整写入 to.segments。
    assert_eq!(created.owner_starmap_id, f.a);
    assert_eq!(
        created.edge.to.segments,
        vec![enter_embed(&f.eb), enter_portal("p1")]
    );

    let a_graph = f.api.get_starmap_graph(&f.a).unwrap();
    assert_eq!(a_graph.edges.len(), 1, "宿主图 A 必须只写入一条 Edge");
    assert_eq!(a_graph.edges[0].id, created.edge.id);
    assert_eq!(
        a_graph.edges[0].to.segments,
        vec![enter_embed(&f.eb), enter_portal("p1")]
    );

    let b_graph = f.api.get_starmap_graph(&f.b).unwrap();
    assert!(b_graph.edges.is_empty(), "B 不应写入伪造的本地 Edge");
    let c_graph = f.api.get_starmap_graph(&f.c).unwrap();
    assert!(c_graph.edges.is_empty(), "C 不应写入伪造的本地 Edge");

    // C 画布经 (eB,p1) 反查到这条边。
    let via_p1 = f
        .api
        .list_starmap_connections(
            route(&f.a, vec![enter_embed(&f.eb), enter_portal("p1")]),
            None,
        )
        .unwrap();
    assert!(
        via_p1.iter().any(|c| c.edge_id == created.edge.id),
        "经 (eB,p1) 进入 C 应反查到这条边"
    );

    // 同一 C，经另一个 Portal p2 进入的实例路径不同，不应看到这条只属于 p1 的边。
    let via_p2 = f
        .api
        .list_starmap_connections(
            route(&f.a, vec![enter_embed(&f.eb), enter_portal("p2")]),
            None,
        )
        .unwrap();
    assert!(
        via_p2.iter().all(|c| c.edge_id != created.edge.id),
        "经 (eB,p2) 进入 C 不应看到经 p1 建立的边"
    );
}

/// 删除被外部路径穿越的 Portal 节点被拒绝；未被引用的兄弟 Portal 可正常删除。
#[test]
fn deleting_referenced_portal_node_is_rejected() {
    let f = make_portal_fixture();

    let from = selection(route(&f.a, vec![]), node_target("a1"));
    let to = selection(
        route(&f.a, vec![enter_embed(&f.eb), enter_portal("p1")]),
        node_target("c1"),
    );
    f.api
        .create_cross_canvas_edge(from, to, StarMapEdgeKindDto::References, None, None)
        .unwrap();

    // p1 被 A 持有的边穿越：拒绝删除并报告跨图引用。
    let error = f
        .api
        .delete_starmap_node(&f.b, "p1")
        .expect_err("删除被跨层边穿越的 Portal 必须被拒绝");
    assert!(
        format!("{error:?}").contains("cross-canvas references exist"),
        "必须报告跨图引用，而非其他错误；实际: {error:?}"
    );

    // p2 未被任何外部路径引用：可以正常删除。
    f.api
        .delete_starmap_node(&f.b, "p2")
        .expect("未被引用的 Portal 节点应可删除");
}

/// 重定目标或清空被外部路径穿越的 Portal 被拒绝；普通属性修改仍然允许。
#[test]
fn repointing_or_clearing_referenced_portal_is_rejected() {
    let f = make_portal_fixture();

    let from = selection(route(&f.a, vec![]), node_target("a1"));
    let to = selection(
        route(&f.a, vec![enter_embed(&f.eb), enter_portal("p1")]),
        node_target("c1"),
    );
    f.api
        .create_cross_canvas_edge(from, to, StarMapEdgeKindDto::References, None, None)
        .unwrap();

    let d = f.api.create_starmap("D", "", None).unwrap().starmap_id;

    // 把 p1 的目标改成 D：A 的边会立刻悬空，必须拒绝。
    let mut repoint = empty_patch();
    repoint.portal = Some(Some(portal_to(&d)));
    let error = f
        .api
        .update_starmap_node(&f.b, "p1", repoint)
        .expect_err("重定被穿越 Portal 的目标必须被拒绝");
    assert!(
        format!("{error:?}").contains("external dependencies would dangle"),
        "必须报告外部依赖会悬空；实际: {error:?}"
    );

    // 清空 p1 的 portal：同样让 A 的边失效，必须拒绝。
    let mut clear = empty_patch();
    clear.portal = Some(None);
    f.api
        .update_starmap_node(&f.b, "p1", clear)
        .expect_err("清空被穿越 Portal 必须被拒绝");

    // 只改标题不影响路径解析，必须允许。
    let mut rename = empty_patch();
    rename.title = Some("入口一".to_string());
    f.api
        .update_starmap_node(&f.b, "p1", rename)
        .expect("不影响路径解析的标题修改应当允许");
}

/// Portal 穿行、当前画布解析、返回层级经真实 API 闭环。
#[test]
fn portal_navigation_round_trip_through_public_api() {
    let f = make_portal_fixture();

    // A -eB-> B -p1-> C
    let entered_b = f
        .api
        .enter_starmap_embed(f.a.clone(), vec![], f.eb.clone())
        .unwrap();
    assert_eq!(entered_b.location.current_starmap_id, f.b);
    assert_eq!(entered_b.route.segments, vec![enter_embed(&f.eb)]);

    let entered_c = f
        .api
        .enter_starmap_portal(
            f.a.clone(),
            entered_b.route.segments.clone(),
            "p1".to_string(),
        )
        .unwrap();
    assert_eq!(entered_c.location.current_starmap_id, f.c);
    assert_eq!(
        entered_c.route.segments,
        vec![enter_embed(&f.eb), enter_portal("p1")]
    );

    // 当前画布解析。
    let resolved = f
        .api
        .resolve_starmap_navigation(f.a.clone(), entered_c.route.segments.clone())
        .unwrap();
    assert_eq!(resolved.current_starmap_id, f.c);
    assert_eq!(
        resolved.traversed_starmap_ids,
        vec![f.a.clone(), f.b.clone(), f.c.clone()]
    );

    // 退出一层回到 B。
    let exited = f.api.exit_starmap_canvas(entered_c.route.clone()).unwrap();
    assert_eq!(exited.segments, vec![enter_embed(&f.eb)]);
    let resolved_b = f
        .api
        .resolve_starmap_navigation(f.a.clone(), exited.segments.clone())
        .unwrap();
    assert_eq!(resolved_b.current_starmap_id, f.b);

    // 跳到根图。
    let jumped = f
        .api
        .jump_starmap_ancestor(entered_c.route.clone(), 0)
        .unwrap();
    assert!(jumped.segments.is_empty());
    assert_eq!(jumped.root_starmap_id, f.a);

    // 通过 Portal 直接进入 C 的路线也可从根图解析。
    let direct = f
        .api
        .enter_starmap_portal(f.a.clone(), vec![], "p1".to_string());
    // p1 位于 B，不在根图 A，因此从根图直接穿 p1 必须失败。
    assert!(direct.is_err(), "Portal 位于子图 B，不能从根图 A 直接穿行");
}

/// `add_starmap_embed` 的 host_path 使用说明性契约：确保测试夹具 helper 与主线 DTO 对齐。
#[test]
fn embed_host_path_dto_round_trips() {
    let (_tmp, api) = make_api();
    let root = api.create_starmap("根图", "", None).unwrap().starmap_id;
    let target = api.create_starmap("目标", "", None).unwrap().starmap_id;

    let embed = api
        .add_starmap_embed(
            &root,
            StarMapEmbedDto {
                instance_id: "emb_x".to_string(),
                target_starmap_id: target.clone(),
                label: None,
                position: point(1.0, 2.0),
                host_path: StarMapTargetPathDto {
                    starmap_id: root.clone(),
                    segments: vec![],
                    target: StarMapTargetDetailDto::default(),
                },
                provenance: Default::default(),
                created_at: 0,
                updated_at: 0,
            },
        )
        .unwrap();
    assert_eq!(embed.target_starmap_id, target);
    assert_eq!(embed.host_path.starmap_id, root);
}
