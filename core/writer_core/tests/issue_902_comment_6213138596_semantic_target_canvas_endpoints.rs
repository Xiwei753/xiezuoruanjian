//! Issue #902 评论 6213138596 — 跨画布边终点为语义目标
//!（`ChapterRange` / `Entity` / `External`）时的真实 `WriterCoreApi` 集成测试。
//!
//! #902 要求「跨画布语义连线连接真实节点对象」，并把终点身份写成节点/目标 ID
//! 而非屏幕线段。`starmap/graph/resolve.rs` 的 `push_terminal_dependency_facts`
//! 明确承诺：路径**成功到达终点画布**时产出 `CanvasEndpoint` 事实，覆盖
//! `Starmap / ChapterRange / Entity / External` 等非 Node/Anchor 终点。
//!
//! 此前各轮已从公开 API 覆盖 Node、Anchor、Starmap 三类终点，但
//! `ChapterRange / Entity / External` 只在内层单元里间接验证过。本测试补齐：
//!
//! 1. 从根图 `R`、经 `Embed(eB)` 进入子图 `B`，把三条跨画布边的终点分别设为
//!    `ChapterRange` / `Entity` / `External`；断言它们都只写在宿主 `R`，
//!    `to.segments` 保留 `eB`，`B` 不落假边。
//! 2. `list_starmap_connections` 在 `B` 的 canvas 查询依赖 `CanvasEndpoint`
//!    命中全部三条；按具体语义 target 查询只命中对应那一条，不串到其他终点，
//!    按 `Node(b1)` 查询也不误命中。
//! 3. 删除其中一条边后，反向依赖索引同步失效：canvas 查询不再返回它，其余两条
//!    仍在；被删终点的 target 查询变空，其他终点不受影响。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tempfile::TempDir;
use writer_core::api::types::{
    StarMapEdgeKindDto, StarMapEndpointSelectionDto, StarMapNavigationRouteDto, StarMapNodeDto,
    StarMapNodeKindDto, StarMapPathSegmentDto, StarMapPointDto, StarMapTargetDetailDto,
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

fn entity_target() -> StarMapTargetDetailDto {
    StarMapTargetDetailDto {
        kind: "entity".to_string(),
        entity_type: Some("character".to_string()),
        entity_id: Some("chen".to_string()),
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

struct Fixture {
    _tmp: TempDir,
    api: WriterCoreApi,
    root: String,
    child: String,
    eb: String,
    chapter_edge: String,
    entity_edge: String,
    external_edge: String,
}

/// 根图 `R --Embed(eB)--> B`，三条跨画布边终点分别是语义目标。
fn make_semantic_target_fixture() -> Fixture {
    let (tmp, api) = make_api();

    let root = api.create_starmap("根图", "", None).unwrap().starmap_id;
    let child = api
        .create_starmap_child_embed(&root, "子图 B", point(0.0, 0.0))
        .unwrap();
    let child_id = child.starmap.starmap_id.clone();
    let eb = child.embed.instance_id.clone();

    api.add_starmap_node(&root, node("r1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&child_id, node("b1"), 0.0, 0.0)
        .unwrap();

    let from = || selection(route(&root, vec![]), node_target("r1"));
    let to =
        |target: StarMapTargetDetailDto| selection(route(&root, vec![enter_embed(&eb)]), target);

    let chapter = api
        .create_cross_canvas_edge(
            from(),
            to(chapter_range_target()),
            StarMapEdgeKindDto::References,
            Some("章节区间".to_string()),
            None,
        )
        .unwrap();
    let entity = api
        .create_cross_canvas_edge(
            from(),
            to(entity_target()),
            StarMapEdgeKindDto::References,
            None,
            None,
        )
        .unwrap();
    let external = api
        .create_cross_canvas_edge(
            from(),
            to(external_target()),
            StarMapEdgeKindDto::References,
            None,
            None,
        )
        .unwrap();

    Fixture {
        _tmp: tmp,
        api,
        root,
        child: child_id,
        eb,
        chapter_edge: chapter.edge.id,
        entity_edge: entity.edge.id,
        external_edge: external.edge.id,
    }
}

/// 语义目标端点只写在宿主图，`to.segments` 保留入口实例，子图无假边。
#[test]
fn semantic_targets_are_hosted_by_root_with_instance_path() {
    let f = make_semantic_target_fixture();

    let root_graph = f.api.get_starmap_graph(&f.root).unwrap();
    assert_eq!(
        root_graph.edges.len(),
        3,
        "三条跨画布边都应写入宿主图 R；实际: {}",
        root_graph.edges.len()
    );
    assert!(
        f.api.get_starmap_graph(&f.child).unwrap().edges.is_empty(),
        "子画布 B 不应出现伪造的本地 Edge"
    );

    let expected = [
        (&f.chapter_edge, "chapterRange"),
        (&f.entity_edge, "entity"),
        (&f.external_edge, "external"),
    ];
    for (edge_id, kind) in expected {
        let edge = root_graph
            .edges
            .iter()
            .find(|e| &e.id == edge_id)
            .unwrap_or_else(|| panic!("宿主图缺少边 {edge_id}"));
        assert_eq!(edge.from.starmap_id, f.root);
        assert!(edge.from.segments.is_empty());
        assert_eq!(edge.from.target, node_target("r1"));
        assert_eq!(edge.to.starmap_id, f.root);
        assert_eq!(
            edge.to.segments,
            vec![enter_embed(&f.eb)],
            "语义目标端点必须保留进入实例路径"
        );
        assert_eq!(
            edge.to.target.kind, kind,
            "终点身份应是语义目标 {kind}，实际: {}",
            edge.to.target.kind
        );
    }
}

/// canvas 反查命中全部三个语义终点；target 反查精确区分三个终点，不串线。
#[test]
fn canvas_and_target_queries_isolate_each_semantic_endpoint() {
    let f = make_semantic_target_fixture();
    let child_route = route(&f.root, vec![enter_embed(&f.eb)]);

    // canvas 查询：依赖 CanvasEndpoint 事实，三条语义终点边都不能漏。
    let canvas = f
        .api
        .list_starmap_connections(child_route.clone(), None)
        .unwrap();
    for edge_id in [&f.chapter_edge, &f.entity_edge, &f.external_edge] {
        assert!(
            canvas
                .iter()
                .any(|c| &c.edge_id == edge_id && c.owner_starmap_id == f.root),
            "子画布 canvas 查询应包含语义终点边 {edge_id}；实际: {canvas:?}"
        );
    }

    // target 查询：各自只命中本终点类型的那一条。
    let chapter = f
        .api
        .list_starmap_connections(child_route.clone(), Some(chapter_range_target()))
        .unwrap();
    assert_eq!(
        chapter
            .iter()
            .map(|c| c.edge_id.as_str())
            .collect::<Vec<_>>(),
        vec![f.chapter_edge.as_str()],
        "chapterRange 查询只应命中区间终点边；实际: {chapter:?}"
    );
    assert_eq!(chapter[0].label.as_deref(), Some("章节区间"));

    let entity = f
        .api
        .list_starmap_connections(child_route.clone(), Some(entity_target()))
        .unwrap();
    assert_eq!(
        entity
            .iter()
            .map(|c| c.edge_id.as_str())
            .collect::<Vec<_>>(),
        vec![f.entity_edge.as_str()],
        "entity 查询只应命中实体终点边；实际: {entity:?}"
    );

    let external = f
        .api
        .list_starmap_connections(child_route.clone(), Some(external_target()))
        .unwrap();
    assert_eq!(
        external
            .iter()
            .map(|c| c.edge_id.as_str())
            .collect::<Vec<_>>(),
        vec![f.external_edge.as_str()],
        "external 查询只应命中外部终点边；实际: {external:?}"
    );

    // 按本图真实节点查询不应误命中只落在语义终点的边。
    let node_query = f
        .api
        .list_starmap_connections(child_route, Some(node_target("b1")))
        .unwrap();
    for edge_id in [&f.chapter_edge, &f.entity_edge, &f.external_edge] {
        assert!(
            node_query.iter().all(|c| &c.edge_id != edge_id),
            "Node(b1) 查询不应命中语义终点边 {edge_id}"
        );
    }
}

/// 删除一条语义终点边后反向索引同步失效，其余语义终点边不受影响。
#[test]
fn deleting_one_semantic_edge_clears_only_its_reverse_index() {
    let f = make_semantic_target_fixture();
    let child_route = || route(&f.root, vec![enter_embed(&f.eb)]);

    assert!(
        f.api.delete_starmap_edge(&f.root, &f.chapter_edge).unwrap(),
        "删除宿主图中的区间终点边应返回 true"
    );

    let canvas = f.api.list_starmap_connections(child_route(), None).unwrap();
    assert!(
        canvas.iter().all(|c| c.edge_id != f.chapter_edge),
        "删除后 canvas 查询不应再返回已删除的边；实际: {canvas:?}"
    );
    for edge_id in [&f.entity_edge, &f.external_edge] {
        assert!(
            canvas.iter().any(|c| &c.edge_id == edge_id),
            "删除一条边不应影响其余语义终点边 {edge_id}"
        );
    }

    let chapter = f
        .api
        .list_starmap_connections(child_route(), Some(chapter_range_target()))
        .unwrap();
    assert!(
        chapter.is_empty(),
        "被删终点的 target 查询应变空；实际: {chapter:?}"
    );

    let entity = f
        .api
        .list_starmap_connections(child_route(), Some(entity_target()))
        .unwrap();
    assert_eq!(
        entity
            .iter()
            .map(|c| c.edge_id.as_str())
            .collect::<Vec<_>>(),
        vec![f.entity_edge.as_str()],
        "删除区间终点不应影响实体终点边；实际: {entity:?}"
    );

    assert_eq!(f.api.get_starmap_graph(&f.root).unwrap().edges.len(), 2);
}
