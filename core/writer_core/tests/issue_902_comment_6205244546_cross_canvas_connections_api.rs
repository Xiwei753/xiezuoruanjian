//! Issue #902 评论 6205244546 — 跨画布连线与关联反查的对外 API 集成测试。
//!
//! #902 要求统一 Core API 覆盖"入口实例路径 / 当前画布解析 / 返回层级 /
//! **跨画布连线** / **关联反查**"。其中 `create_cross_canvas_edge` 和
//! `list_starmap_connections` 此前只有 facade / 领域层测试，没有经
//! `WriterCoreApi`（UniFFI / JSON C-ABI 的实际出口）走一遍真实 Store 的组合验证。
//!
//! 本测试通过真实 `WriterCoreApi` 出口验证：
//! 1. 父图节点 → 子画布节点 的跨层边，只写一条真实 Edge 到最长公共宿主图（父图），
//!    `from`/`to` 的 `starmap_id` 都是宿主图，穿越路径写进 `segments`；
//! 2. 该边既能在子画布 `list_starmap_connections`（target=None）里反查到，
//!    也能按子节点 `target=Some` 精确反查；
//! 3. 不同根星图之间无共同入口链时，`create_cross_canvas_edge` 明确报错，
//!    不伪造宿主关系；
//! 4. 同一子星图在父图有两个 Embed 实例时，只对经原进入实例的访问上下文可见，
//!    不会因为落点星图相同就串到另一个入口。

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

/// 父图节点 → 子画布节点：边写在父图，子画布能反查到，且按子节点精确反查命中。
#[test]
fn cross_canvas_edge_is_written_to_host_and_queryable_from_child() {
    let (_tmp, api) = make_api();

    let root = api.create_starmap("根图", "", None).unwrap().starmap_id;
    let child = api
        .create_starmap_child_embed(&root, "子图", point(0.0, 0.0))
        .unwrap();
    let child_id = child.starmap.starmap_id.clone();
    let instance_id = child.embed.instance_id.clone();

    api.add_starmap_node(&root, node("n_root"), 0.0, 0.0)
        .unwrap();
    api.add_starmap_node(&child_id, node("n_child"), 0.0, 0.0)
        .unwrap();

    let from = selection(route(&root, vec![]), node_target("n_root"));
    let to = selection(
        route(&root, vec![enter_embed(&instance_id)]),
        node_target("n_child"),
    );

    let created = api
        .create_cross_canvas_edge(
            from,
            to,
            StarMapEdgeKindDto::References,
            Some("跨层".to_string()),
            None,
        )
        .unwrap();

    // 只写一条真实 Edge，宿主图是父图；from/to 的 starmap_id 都是宿主图。
    assert_eq!(created.owner_starmap_id, root);
    assert_eq!(created.edge.from.starmap_id, root);
    assert!(created.edge.from.segments.is_empty());
    assert_eq!(created.edge.from.target, node_target("n_root"));
    assert_eq!(created.edge.to.starmap_id, root);
    assert_eq!(
        created.edge.to.segments,
        vec![enter_embed(&instance_id)],
        "跨层穿越路径必须写进 to.segments"
    );
    assert_eq!(created.edge.to.target, node_target("n_child"));

    // 子画布反查（target=None）：存于祖先图但一端落在当前画布的边必须出现。
    let child_canvas = api
        .list_starmap_connections(route(&root, vec![enter_embed(&instance_id)]), None)
        .unwrap();
    assert!(
        child_canvas
            .iter()
            .any(|c| c.edge_id == created.edge.id && c.owner_starmap_id == root),
        "子画布应反查到存于父图的跨层边"
    );

    // 按目标节点精确反查：应只命中这条边。
    let child_target = api
        .list_starmap_connections(
            route(&root, vec![enter_embed(&instance_id)]),
            Some(node_target("n_child")),
        )
        .unwrap();
    assert!(
        child_target.iter().any(|c| c.edge_id == created.edge.id),
        "按子节点 target 应命中跨层边"
    );

    // 根画布反查同样能看到（from 端落在根画布）。
    let root_canvas = api
        .list_starmap_connections(route(&root, vec![]), None)
        .unwrap();
    assert!(
        root_canvas.iter().any(|c| c.edge_id == created.edge.id),
        "根画布应能看到 from 端在本层的跨层边"
    );
}

/// 不同根星图之间没有共同入口链：明确报错，不伪造宿主关系。
#[test]
fn cross_canvas_edge_between_independent_roots_is_rejected() {
    let (_tmp, api) = make_api();

    let root_a = api.create_starmap("甲", "", None).unwrap().starmap_id;
    let root_b = api.create_starmap("乙", "", None).unwrap().starmap_id;
    api.add_starmap_node(&root_a, node("na"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&root_b, node("nb"), 0.0, 0.0).unwrap();

    let from = selection(route(&root_a, vec![]), node_target("na"));
    let to = selection(route(&root_b, vec![]), node_target("nb"));

    let result = api.create_cross_canvas_edge(from, to, StarMapEdgeKindDto::RelatedTo, None, None);
    assert!(
        result.is_err(),
        "两个独立根星图无共同宿主，必须报错而不是伪造宿主关系"
    );
}

/// 同一子星图在父图有两个 Embed 实例时，边只对经原进入实例的访问上下文可见。
#[test]
fn list_connections_distinguishes_embed_instances_for_same_canvas() {
    let (_tmp, api) = make_api();

    let root = api.create_starmap("根图", "", None).unwrap().starmap_id;
    let shared = api.create_starmap("共享子图", "", None).unwrap().starmap_id;
    api.add_starmap_node(&root, node("n_root"), 0.0, 0.0)
        .unwrap();
    api.add_starmap_node(&shared, node("n_child"), 0.0, 0.0)
        .unwrap();

    // 同一个子星图，在根图里建立两个入口实例 emb_a / emb_b。
    for instance_id in ["emb_a", "emb_b"] {
        api.add_starmap_embed(
            &root,
            StarMapEmbedDto {
                instance_id: instance_id.to_string(),
                target_starmap_id: shared.clone(),
                label: None,
                position: point(0.0, 0.0),
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
    }

    // 边经 emb_a 进入共享子图落在 n_child。
    let from = selection(route(&root, vec![]), node_target("n_root"));
    let to = selection(
        route(&root, vec![enter_embed("emb_a")]),
        node_target("n_child"),
    );
    let created = api
        .create_cross_canvas_edge(from, to, StarMapEdgeKindDto::References, None, None)
        .unwrap();

    let via_a = api
        .list_starmap_connections(route(&root, vec![enter_embed("emb_a")]), None)
        .unwrap();
    assert!(
        via_a.iter().any(|c| c.edge_id == created.edge.id),
        "经 emb_a 进入共享子图应看到这条边"
    );

    let via_b = api
        .list_starmap_connections(route(&root, vec![enter_embed("emb_b")]), None)
        .unwrap();
    assert!(
        via_b.iter().all(|c| c.edge_id != created.edge.id),
        "经 emb_b 进入同一落点子图，不应看到经 emb_a 建立的边"
    );
}
