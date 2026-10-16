//! Issue #902 评论 6206357941 — 同一 StarMap 在**多个地方** Embed 的跨画布连线集成测试。
//!
//! #902 明确要求 Core API "支持同一 StarMap 在多个地方 Embed 的情形"。第 32/33 轮
//! 已覆盖同一父图用两个 Embed 进入同一子图（`e1→D`、`e2→D`）的实例隔离；第 34 轮
//! 覆盖 Portal 穿行。本测试补齐剩余的拓扑：**同一子图 D 被两个不同父图 B、C 分别
//! Embed**（B、C 又是同一根图 A 的子图，即菱形/钻石拓扑），经真实 `WriterCoreApi`
//! 验证：
//!
//! 1. 两个端点分别经 `A/[eB,eDB]`、`A/[eC,eDC]` 进入同一物理画布 D 时，跨画布边仍
//!    以**最长公共实例路径前缀**定宿主图 A，而不是被 D、B 或 C 的落点星图身份误导；
//! 2. `from`/`to` 的完整实例路径都写进 `segments`，且只在 A 写一条真实 Edge，B/C/D
//!    都不写假边；
//! 3. 关联反查按完整实例路径后缀精确过滤：经 `A/[eB,eDB]`、`A/[eC,eDC]` 都能看到这条
//!    边；中间画布 A/[eB]、A/[eC] 看不到；按终点对象查询也不会把 from/to 端串用；
//! 4. 反向（子画布 → 祖先画布）连线同样以祖先为宿主图，方向不影响最长公共前缀。
//!
//! 这些断言真正走 `WriterCoreApi` → `create_cross_canvas_edge` /
//! `list_starmap_connections` → `PathDependencyIndex` 的真实数据链路。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tempfile::TempDir;
use writer_core::api::types::{
    StarMapEdgeKindDto, StarMapEmbedDto, StarMapEndpointSelectionDto, StarMapNavigationRouteDto,
    StarMapNodeDto, StarMapNodeKindDto, StarMapPathSegmentDto, StarMapPointDto,
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

struct Diamond {
    _tmp: TempDir,
    api: WriterCoreApi,
    a: String,
    b: String,
    c: String,
    d: String,
    eb: String,
    ec: String,
}

/// 构造根图 A → (B, C)；B、C 又各自 Embed 同一张 D（菱形拓扑）。
fn make_diamond() -> Diamond {
    let (tmp, api) = make_api();

    let a = api.create_starmap("A", "", None).unwrap().starmap_id;
    let b_child = api
        .create_starmap_child_embed(&a, "B", point(0.0, 0.0))
        .unwrap();
    let b = b_child.starmap.starmap_id.clone();
    let eb = b_child.embed.instance_id.clone();
    let c_child = api
        .create_starmap_child_embed(&a, "C", point(0.0, 0.0))
        .unwrap();
    let c = c_child.starmap.starmap_id.clone();
    let ec = c_child.embed.instance_id.clone();

    let d = api.create_starmap("D", "", None).unwrap().starmap_id;

    api.add_starmap_embed(&b, embed_to(&b, "eDB", &d)).unwrap();
    api.add_starmap_embed(&c, embed_to(&c, "eDC", &d)).unwrap();

    api.add_starmap_node(&a, node("a1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&b, node("b1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&c, node("c1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&d, node("d1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&d, node("d2"), 0.0, 0.0).unwrap();

    Diamond {
        _tmp: tmp,
        api,
        a,
        b,
        c,
        d,
        eb,
        ec,
    }
}

/// 同一张 D 经两个不同父图 B、C Embed：宿主图是最长公共前缀对应的 A；
/// 只有 A 写一条真实 Edge；反查按完整实例路径隔离。
#[test]
fn diamond_embedding_uses_longest_common_prefix_host() {
    let f = make_diamond();

    let via_b = route(&f.a, vec![enter_embed(&f.eb), enter_embed("eDB")]);
    let via_c = route(&f.a, vec![enter_embed(&f.ec), enter_embed("eDC")]);

    let created = f
        .api
        .create_cross_canvas_edge(
            selection(via_b.clone(), node_target("d1")),
            selection(via_c.clone(), node_target("d2")),
            StarMapEdgeKindDto::References,
            None,
            None,
        )
        .unwrap();

    // 宿主图是公共前缀对应的 A，而不是两个端点落点的 D。
    assert_eq!(created.owner_starmap_id, f.a);
    assert_eq!(created.edge.from.starmap_id, f.a);
    assert_eq!(
        created.edge.from.segments,
        vec![enter_embed(&f.eb), enter_embed("eDB")]
    );
    assert_eq!(created.edge.to.starmap_id, f.a);
    assert_eq!(
        created.edge.to.segments,
        vec![enter_embed(&f.ec), enter_embed("eDC")]
    );

    // 只在 A 写一条真实 Edge；B/C/D 都不写假边。
    let a_graph = f.api.get_starmap_graph(&f.a).unwrap();
    assert_eq!(a_graph.edges.len(), 1, "宿主图 A 必须只写入一条 Edge");
    assert_eq!(a_graph.edges[0].id, created.edge.id);
    assert!(f.api.get_starmap_graph(&f.b).unwrap().edges.is_empty());
    assert!(f.api.get_starmap_graph(&f.c).unwrap().edges.is_empty());
    assert!(
        f.api.get_starmap_graph(&f.d).unwrap().edges.is_empty(),
        "落点画布 D 不应写入伪造的本地 Edge"
    );

    // 经 (eB,eDB) 与 (eC,eDC) 进入 D 都能反查到这条边。
    let seen_via_b = f.api.list_starmap_connections(via_b.clone(), None).unwrap();
    assert!(
        seen_via_b.iter().any(|c| c.edge_id == created.edge.id),
        "经 A/[eB,eDB] 进入 D 应看到 from 端在本访问上下文的边"
    );
    let seen_via_c = f.api.list_starmap_connections(via_c.clone(), None).unwrap();
    assert!(
        seen_via_c.iter().any(|c| c.edge_id == created.edge.id),
        "经 A/[eC,eDC] 进入 D 应看到 to 端在本访问上下文的边"
    );

    // 中间画布 B（A/[eB]）、C（A/[eC]）不是这条边的端点，不应看到它。
    let b_canvas = f
        .api
        .list_starmap_connections(route(&f.a, vec![enter_embed(&f.eb)]), None)
        .unwrap();
    assert!(
        b_canvas.iter().all(|c| c.edge_id != created.edge.id),
        "中间画布 B 不应看到两端都在更深画布 D 的边"
    );
    let c_canvas = f
        .api
        .list_starmap_connections(route(&f.a, vec![enter_embed(&f.ec)]), None)
        .unwrap();
    assert!(
        c_canvas.iter().all(|c| c.edge_id != created.edge.id),
        "中间画布 C 不应看到两端都在更深画布 D 的边"
    );

    // 按终点对象精确反查：只有 to 端落入的访问上下文能看到终点 d2。
    let target_d1_via_b = f
        .api
        .list_starmap_connections(via_b.clone(), Some(node_target("d1")))
        .unwrap();
    assert!(
        target_d1_via_b.iter().any(|c| c.edge_id == created.edge.id),
        "from 端 d1 应能按目标反查到这条边"
    );
    let target_d2_via_b = f
        .api
        .list_starmap_connections(via_b.clone(), Some(node_target("d2")))
        .unwrap();
    assert!(
        target_d2_via_b.iter().all(|c| c.edge_id != created.edge.id),
        "from 端查询不应误把 to 端 d2 当作本端目标"
    );
    let target_d2_via_c = f
        .api
        .list_starmap_connections(via_c.clone(), Some(node_target("d2")))
        .unwrap();
    assert!(
        target_d2_via_c.iter().any(|c| c.edge_id == created.edge.id),
        "to 端 d2 应能按目标反查到这条边"
    );
}

/// 反向（子画布 → 祖先画布）连线：方向不影响最长公共前缀，宿主仍是祖先 A。
#[test]
fn child_to_ancestor_edge_is_hosted_by_ancestor() {
    let (_tmp, api) = make_api();

    let a = api.create_starmap("A", "", None).unwrap().starmap_id;
    let child = api
        .create_starmap_child_embed(&a, "B", point(0.0, 0.0))
        .unwrap();
    let b = child.starmap.starmap_id.clone();
    let eb = child.embed.instance_id.clone();

    api.add_starmap_node(&a, node("a1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&b, node("b1"), 0.0, 0.0).unwrap();

    let from = selection(route(&a, vec![enter_embed(&eb)]), node_target("b1"));
    let to = selection(route(&a, vec![]), node_target("a1"));

    let created = api
        .create_cross_canvas_edge(from, to, StarMapEdgeKindDto::RelatedTo, None, None)
        .unwrap();

    assert_eq!(created.owner_starmap_id, a);
    assert_eq!(
        created.edge.from.segments,
        vec![enter_embed(&eb)],
        "from 端在子画布，穿越路径写入 segments"
    );
    assert!(
        created.edge.to.segments.is_empty(),
        "to 端在祖先画布，无穿越路径"
    );

    let a_graph = api.get_starmap_graph(&a).unwrap();
    assert_eq!(a_graph.edges.len(), 1, "宿主图 A 必须只写入一条 Edge");
    assert!(api.get_starmap_graph(&b).unwrap().edges.is_empty());

    // 子画布与祖先画布都能反查这条边。
    let child_canvas = api
        .list_starmap_connections(route(&a, vec![enter_embed(&eb)]), None)
        .unwrap();
    assert!(child_canvas.iter().any(|c| c.edge_id == created.edge.id));
    let root_canvas = api
        .list_starmap_connections(route(&a, vec![]), None)
        .unwrap();
    assert!(root_canvas.iter().any(|c| c.edge_id == created.edge.id));
}
