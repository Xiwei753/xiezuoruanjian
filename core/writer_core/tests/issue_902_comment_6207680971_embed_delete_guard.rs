//! Issue #902 评论 6207680971 — Embed 入口的跨画布引用删除保护经 `WriterCoreApi` 验证。
//!
//! #902 实现路线第 3 节要求：删除「目标节点、Embed 入口或 Portal」之前，必须先反查
//! 其他 graph 持有的跨层引用，有跨图关联时拒绝破坏性删除。此前几轮已覆盖：
//! - 删除被跨图 Edge 指向的 Node、被途经的 Portal 会被拒绝（Portal 穿行保护测试）；
//! - 删除宿主图中的跨画布 Edge 后，反向依赖索引同步失效。
//!
//! 尚未覆盖的是 **Embed 入口本身被外部边途经** 的情形。本测试构造
//! `R → Embed(rA) → A → Embed(eB) → B` 的嵌套入口，并创建一条存于 R、
//! 经 `[rA, eB]` 落到 B 节点的跨画布边：
//!
//! 1. 该边途经 A 中的 `eB`，来源宿主却是外图 R，所以删除 `eB` 必须被拒绝，
//!    并以 `cross-canvas references exist` 说明原因；拒绝后 A 的 graph 仍保留 `eB`。
//! 2. 删除 R 中的原始边后，`eB` 不再被任何外部路径引用，删除应成功；
//!    删除入口不等于删除目标画布，子画布 B 自身仍可读取。

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

/// 被外图边途经的 Embed 入口：有引用时拒绝删除，解除引用后可删除。
#[test]
fn embed_entry_traversed_by_external_edge_blocks_delete_until_edge_removed() {
    let (_tmp, api) = make_api();

    let r = api.create_starmap("R", "", None).unwrap().starmap_id;
    let a_child = api
        .create_starmap_child_embed(&r, "A", point(0.0, 0.0))
        .unwrap();
    let a = a_child.starmap.starmap_id.clone();
    let ra = a_child.embed.instance_id.clone();
    let b_child = api
        .create_starmap_child_embed(&a, "B", point(0.0, 0.0))
        .unwrap();
    let b = b_child.starmap.starmap_id.clone();
    let eb = b_child.embed.instance_id.clone();

    api.add_starmap_node(&r, node("r1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&b, node("b1"), 0.0, 0.0).unwrap();

    // 存于 R 的跨画布边：from = R/[]/Node(r1)，to = R/[rA, eB]/Node(b1)。
    let created = api
        .create_cross_canvas_edge(
            selection(route(&r, vec![]), node_target("r1")),
            selection(
                route(&r, vec![enter_embed(&ra), enter_embed(&eb)]),
                node_target("b1"),
            ),
            StarMapEdgeKindDto::References,
            None,
            None,
        )
        .unwrap();

    assert_eq!(
        created.owner_starmap_id, r,
        "两端公共前缀为空，宿主是根图 R"
    );
    assert_eq!(
        created.edge.to.segments,
        vec![enter_embed(&ra), enter_embed(&eb)],
        "穿过 A 中 eB 的进入路径必须写入 to.segments"
    );

    // 真实边只落在 R，A / B 不写假边。
    let r_graph = api.get_starmap_graph(&r).unwrap();
    assert_eq!(r_graph.edges.len(), 1);
    assert_eq!(r_graph.edges[0].id, created.edge.id);
    assert!(api.get_starmap_graph(&a).unwrap().edges.is_empty());
    assert!(api.get_starmap_graph(&b).unwrap().edges.is_empty());

    // 题干：边途经 A 中的 eB，但来源宿主是外图 R → 删除 eB 必须被拒绝。
    let error = api
        .delete_starmap_embed(&a, &eb)
        .expect_err("被外图边途经的 Embed 入口不得被删除");
    assert!(
        format!("{error:?}").contains("cross-canvas references exist"),
        "必须以跨图引用为由拒绝，而非其他错误；实际: {error:?}"
    );

    // 拒绝的删除不得实际移除入口。
    let a_graph = api.get_starmap_graph(&a).unwrap();
    assert!(
        a_graph.embeds.iter().any(|e| e.instance_id == eb),
        "被拒绝后 A 的 graph 必须仍保留 eB"
    );

    // 删除 R 中的原始跨画布边，解除对 eB 的外部引用。
    assert!(
        api.delete_starmap_edge(&r, &created.edge.id).unwrap(),
        "删除宿主图中的跨画布边应返回 true"
    );
    assert!(api.get_starmap_graph(&r).unwrap().edges.is_empty());

    // 解除外部引用后，eB 可被删除；入口消失但目标画布 B 仍存在。
    assert!(
        api.delete_starmap_embed(&a, &eb).unwrap(),
        "解除外部引用后应允许删除 Embed 入口"
    );
    let a_graph_after = api.get_starmap_graph(&a).unwrap();
    assert!(
        a_graph_after.embeds.iter().all(|e| e.instance_id != eb),
        "删除成功后 A 的 graph 不应再包含 eB"
    );
    assert!(
        api.get_starmap_graph(&b).is_ok(),
        "删除 Embed 入口不应删除目标画布 B 自身"
    );
}
