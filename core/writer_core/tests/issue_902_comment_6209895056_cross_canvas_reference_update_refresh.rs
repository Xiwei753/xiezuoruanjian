//! Issue #902 评论 6209895056 — 跨画布反向依赖索引在 **修改** 后增量刷新。
//!
//! #902 实现路线第 3 节要求："关系写入/修改/删除后增量更新，读取不同入口不会
//! 重复制造关系"。此前各轮已用真实 `WriterCoreApi` 覆盖了**写入**（创建 Edge /
//! Link / Hyperlink / Embed）与**删除**（删目标节点/入口/Portal、解除引用后可删）
//! 的反向索引行为，但**修改**路径——`update_starmap_edge` / `update_starmap_link`
//! 改变端点后旧的跨图引用事实是否被移除、新的引用事实是否建立——尚未被真实 API
//! 验证。若只 replace 新事实而漏删旧事实，就会出现"指向已不再连接的画布的悬空
//! 引用"或"改完仍拦截旧目标"。
//!
//! 拓扑：根图 R 有节点 `r1`，经 Embed `e_b` 进入子图 B（节点 `b1`），经 Embed
//! `e_c` 进入子图 C（节点 `c1`）。
//!
//! 1. `update_starmap_edge` 把一条跨画布边的 `to` 从 `B/[e_b]/b1` 改到
//!    `C/[e_c]/c1`：宿主图 R 仍只有同一条边、`to` 已更新；B 反查不再看到它、C
//!    反查看得到；删除保护随之从 b1 移到 c1。
//! 2. 只改 Edge 的 kind/label：反查原样返回新元数据，且删除保护不变。
//! 3. `update_starmap_link` 把 `target` 从 `B/[e_b]/b1` 改到 `C/[e_c]/c1`：删除
//!    保护随之从 b1 移到 c1。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tempfile::TempDir;
use writer_core::api::types::{
    StarMapEdgeKindDto, StarMapEdgePatchDto, StarMapEndpointSelectionDto, StarMapLinkDto,
    StarMapLinkPatchDto, StarMapNavigationRouteDto, StarMapNodeDto, StarMapNodeKindDto,
    StarMapPathSegmentDto, StarMapPointDto, StarMapTargetDetailDto, StarMapTargetPathDto,
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

struct Fixture {
    root: String,
    child_b: String,
    child_c: String,
    e_b: String,
    e_c: String,
}

fn setup() -> (TempDir, WriterCoreApi, Fixture) {
    let (tmp, api) = make_api();

    let root = api.create_starmap("根图", "", None).unwrap().starmap_id;
    let b = api
        .create_starmap_child_embed(&root, "子图B", point(0.0, 0.0))
        .unwrap();
    let c = api
        .create_starmap_child_embed(&root, "子图C", point(0.0, 0.0))
        .unwrap();
    let child_b = b.starmap.starmap_id.clone();
    let child_c = c.starmap.starmap_id.clone();
    let e_b = b.embed.instance_id.clone();
    let e_c = c.embed.instance_id.clone();

    api.add_starmap_node(&root, node("r1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&child_b, node("b1"), 0.0, 0.0)
        .unwrap();
    api.add_starmap_node(&child_c, node("c1"), 0.0, 0.0)
        .unwrap();

    let fixture = Fixture {
        root,
        child_b,
        child_c,
        e_b,
        e_c,
    };
    (tmp, api, fixture)
}

/// 移动 Edge 的 `to` 目标后，反向索引与删除保护都必须跟着迁移。
#[test]
fn repointing_edge_target_moves_reverse_index_and_delete_guard() {
    let (_tmp, api, fx) = setup();

    let created = api
        .create_cross_canvas_edge(
            selection(route(&fx.root, vec![]), node_target("r1")),
            selection(
                route(&fx.root, vec![enter_embed(&fx.e_b)]),
                node_target("b1"),
            ),
            StarMapEdgeKindDto::RelatedTo,
            Some("原".to_string()),
            None,
        )
        .unwrap();
    let edge_id = created.edge.id.clone();

    // 初始：删除 b1 被跨图引用拦截（来源是宿主 R 的 EdgeTo）。
    let blocked = api
        .delete_starmap_node(&fx.child_b, "b1")
        .expect_err("b1 被宿主 R 的跨画布边引用，删除必须被拒绝");
    let blocked_text = format!("{blocked:?}");
    assert!(
        blocked_text.contains("cross-canvas references exist"),
        "应报告跨画布引用，实际: {blocked_text}"
    );
    assert!(
        blocked_text.contains("EdgeTo"),
        "引用来源应指明 EdgeTo，实际: {blocked_text}"
    );

    // 修改：把 to 从 B/[e_b]/b1 改到 C/[e_c]/c1，同时改 kind/label。
    api.update_starmap_edge(
        &fx.root,
        &edge_id,
        StarMapEdgePatchDto {
            kind: Some(StarMapEdgeKindDto::Foreshadows),
            label: Some(Some("改".to_string())),
            payload: None,
            from: None,
            to: Some(path(
                &fx.root,
                vec![enter_embed(&fx.e_c)],
                node_target("c1"),
            )),
        },
    )
    .unwrap();

    // 宿主图仍只有同一条真实边，且 to 已指向 C。
    let root_graph = api.get_starmap_graph(&fx.root).unwrap();
    assert_eq!(root_graph.edges.len(), 1, "修改端点不应新增或复制边");
    let updated = root_graph
        .edges
        .iter()
        .find(|e| e.id == edge_id)
        .expect("宿主图应仍持有原边");
    assert_eq!(updated.kind, StarMapEdgeKindDto::Foreshadows);
    assert_eq!(updated.label.as_deref(), Some("改"));
    assert_eq!(updated.to.starmap_id, fx.root);
    assert_eq!(updated.to.segments, vec![enter_embed(&fx.e_c)]);
    assert_eq!(updated.to.target, node_target("c1"));

    // 反查：B 不再看到这条边，C 看得到且元数据已更新。
    let via_b = api
        .list_starmap_connections(route(&fx.root, vec![enter_embed(&fx.e_b)]), None)
        .unwrap();
    assert!(
        via_b.iter().all(|c| c.edge_id != edge_id),
        "改到 C 之后，经 e_b 进入 B 不应再反查到这条边"
    );

    let via_c = api
        .list_starmap_connections(route(&fx.root, vec![enter_embed(&fx.e_c)]), None)
        .unwrap();
    let conn_c = via_c
        .iter()
        .find(|c| c.edge_id == edge_id)
        .expect("改到 C 之后，经 e_c 进入 C 应反查到这条边");
    assert_eq!(conn_c.owner_starmap_id, fx.root);
    assert_eq!(conn_c.kind, StarMapEdgeKindDto::Foreshadows);
    assert_eq!(conn_c.label.as_deref(), Some("改"));
    assert_eq!(conn_c.to.segments, vec![enter_embed(&fx.e_c)]);
    assert_eq!(conn_c.to.target, node_target("c1"));

    // 删除保护迁移：旧目标 b1 现在可删，新目标 c1 现在被拦截。
    assert!(
        api.delete_starmap_node(&fx.child_b, "b1").unwrap(),
        "旧目标 b1 不再被引用，应可删除"
    );
    let blocked_c = api
        .delete_starmap_node(&fx.child_c, "c1")
        .expect_err("新目标 c1 被宿主 R 的跨画布边引用，删除必须被拒绝");
    assert!(
        format!("{blocked_c:?}").contains("EdgeTo"),
        "c1 的引用来源应指明 EdgeTo，实际: {blocked_c:?}"
    );

    // 删除原始边后，c1 解除引用可删；证明索引没有留下指向旧目标的悬空事实。
    api.delete_starmap_edge(&fx.root, &edge_id).unwrap();
    assert!(
        api.delete_starmap_node(&fx.child_c, "c1").unwrap(),
        "删除原始边后 c1 应可删除"
    );
}

/// 只改 Edge 元数据（不改端点）时反查原样返回新值，删除保护不变。
#[test]
fn updating_edge_metadata_is_returned_by_connection_queries() {
    let (_tmp, api, fx) = setup();

    let created = api
        .create_cross_canvas_edge(
            selection(route(&fx.root, vec![]), node_target("r1")),
            selection(
                route(&fx.root, vec![enter_embed(&fx.e_b)]),
                node_target("b1"),
            ),
            StarMapEdgeKindDto::RelatedTo,
            Some("旧".to_string()),
            None,
        )
        .unwrap();
    let edge_id = created.edge.id.clone();

    api.update_starmap_edge(
        &fx.root,
        &edge_id,
        StarMapEdgePatchDto {
            kind: Some(StarMapEdgeKindDto::ConflictsWith),
            label: Some(None),
            payload: None,
            from: None,
            to: None,
        },
    )
    .unwrap();

    // 端点未变，删除保护仍在 b1 上。
    let blocked = api
        .delete_starmap_node(&fx.child_b, "b1")
        .expect_err("只改元数据不应解除跨画布引用");
    assert!(format!("{blocked:?}").contains("cross-canvas references exist"));

    // 反查返回新元数据，且每条边只出现一次。
    let via_b = api
        .list_starmap_connections(route(&fx.root, vec![enter_embed(&fx.e_b)]), None)
        .unwrap();
    let matches: Vec<_> = via_b.iter().filter(|c| c.edge_id == edge_id).collect();
    assert_eq!(matches.len(), 1, "同一次反查中一条边只应出现一次");
    assert_eq!(matches[0].kind, StarMapEdgeKindDto::ConflictsWith);
    assert_eq!(matches[0].label, None);
    assert_eq!(matches[0].owner_starmap_id, fx.root);
}

/// 移动 Link 的 `target` 后，反向索引与删除保护同样必须迁移。
#[test]
fn repointing_link_target_moves_reverse_index_and_delete_guard() {
    let (_tmp, api, fx) = setup();

    let link = api
        .add_starmap_link(
            &fx.root,
            StarMapLinkDto {
                link_id: "link_move".to_string(),
                source: path(&fx.root, vec![], node_target("r1")),
                target: path(&fx.root, vec![enter_embed(&fx.e_b)], node_target("b1")),
                label: Some("链接".to_string()),
                created_at: 0,
                updated_at: 0,
            },
        )
        .unwrap();
    let link_id = link.link_id.clone();

    let blocked = api
        .delete_starmap_node(&fx.child_b, "b1")
        .expect_err("b1 被宿主 R 的跨画布 Link target 引用，删除必须被拒绝");
    assert!(
        format!("{blocked:?}").contains("LinkTarget"),
        "引用来源应指明 LinkTarget，实际: {blocked:?}"
    );

    api.update_starmap_link(
        &fx.root,
        &link_id,
        StarMapLinkPatchDto {
            source: None,
            target: Some(path(
                &fx.root,
                vec![enter_embed(&fx.e_c)],
                node_target("c1"),
            )),
            label: None,
        },
    )
    .unwrap();

    // 旧目标可删，新目标被拦截。
    assert!(
        api.delete_starmap_node(&fx.child_b, "b1").unwrap(),
        "旧目标 b1 不再被 Link 引用，应可删除"
    );
    let blocked_c = api
        .delete_starmap_node(&fx.child_c, "c1")
        .expect_err("新目标 c1 被宿主 R 的 Link target 引用，删除必须被拒绝");
    assert!(
        format!("{blocked_c:?}").contains("LinkTarget"),
        "c1 的引用来源应指明 LinkTarget，实际: {blocked_c:?}"
    );

    // 删除 Link 后 c1 可删。
    api.delete_starmap_link(&fx.root, &link_id).unwrap();
    assert!(
        api.delete_starmap_node(&fx.child_c, "c1").unwrap(),
        "删除 Link 后 c1 应可删除"
    );
}
