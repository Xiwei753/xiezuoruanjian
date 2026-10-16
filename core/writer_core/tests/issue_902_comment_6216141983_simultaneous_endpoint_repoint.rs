//! Issue #902 评论 6216141983 — 一次 `update_starmap_edge` 同时移动跨画布边的
//! `from` 与 `to` 两端。
//!
//! #902 实现路线第 3 节要求反向关系索引在“写入/修改/删除后增量更新”。此前各轮
//! 已覆盖只移动单端（评论 6209895056 移动 `to`）、只改元数据（`from/to` 都为
//! None）、以及 Hyperlink/Embed 的路径迁移，但**同一条边在同一次 update 里两端
//! 都换到另一个画布对象**尚未被真实 `WriterCoreApi` 验证。这条路径在
//! `facade/starmap_ops/graph.rs::update_starmap_edge` 里分别调用
//! `replace_source_facts_in_path_dependency_index(EdgeFrom, ...)` 与
//! `(EdgeTo, ...)`；若任何一端漏替换，就会出现“旧端点仍被拦截 / 新端点不被
//! 保护”的悬空或漏保护引用。
//!
//! 拓扑：根图 R 有节点 `r1`，经 Embed `e_b` 进入子图 B（节点 `b1`），经 Embed
//! `e_c` 进入子图 C（节点 `c1`）。一条跨画布边同层宿主为 R：
//!
//! 1. 初始 `B/[e_b]/b1 → C/[e_c]/c1`：删 b1 被 `EdgeFrom` 拦截、删 c1 被
//!    `EdgeTo` 拦截。
//! 2. 一次 update 把两端**对调**为 `C/[e_c]/c1 → B/[e_b]/b1`：宿主仍只有同一条
//!    边；删 b1 改由 `EdgeTo` 拦截、删 c1 改由 `EdgeFrom` 拦截，证明两种来源都
//!    被同一次替换更新。
//! 3. 反查仍按完整实例路径隔离：经 `e_b` 进入 B 与经 `e_c` 进入 C 都能看到这条
//!    宿主边，`from/to` 已按新方向返回。
//! 4. 删除原始边后两端都解除引用。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tempfile::TempDir;
use writer_core::api::types::{
    StarMapEdgeKindDto, StarMapEdgePatchDto, StarMapEndpointSelectionDto,
    StarMapNavigationRouteDto, StarMapNodeDto, StarMapNodeKindDto, StarMapPathSegmentDto,
    StarMapPointDto, StarMapTargetDetailDto, StarMapTargetPathDto,
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

struct Fx {
    api: WriterCoreApi,
    root: String,
    child_b: String,
    child_c: String,
    e_b: String,
    e_c: String,
}

impl Fx {
    fn route_b(&self) -> StarMapNavigationRouteDto {
        route(&self.root, vec![enter_embed(&self.e_b)])
    }

    fn route_c(&self) -> StarMapNavigationRouteDto {
        route(&self.root, vec![enter_embed(&self.e_c)])
    }
}

fn setup() -> (TempDir, Fx) {
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

    let fx = Fx {
        api,
        root,
        child_b,
        child_c,
        e_b,
        e_c,
    };
    (tmp, fx)
}

/// 创建初始方向 `B/[e_b]/b1 → C/[e_c]/c1` 的跨画布边，返回其 ID。
fn create_initial_edge(f: &Fx) -> String {
    let created = f
        .api
        .create_cross_canvas_edge(
            selection(f.route_b(), node_target("b1")),
            selection(f.route_c(), node_target("c1")),
            StarMapEdgeKindDto::RelatedTo,
            Some("原".to_string()),
            None,
        )
        .unwrap();
    assert_eq!(created.owner_starmap_id, f.root);
    created.edge.id.clone()
}

/// 初始状态下 `b1` 由 `EdgeFrom` 保护、`c1` 由 `EdgeTo` 保护。
fn assert_initial_delete_guards(f: &Fx) {
    let blocked_b = f
        .api
        .delete_starmap_node(&f.child_b, "b1")
        .expect_err("b1 被宿主 R 的跨画布边 from 引用，删除必须被拒绝");
    assert!(
        format!("{blocked_b:?}").contains("EdgeFrom"),
        "b1 的引用来源应指明 EdgeFrom，实际: {blocked_b:?}"
    );
    let blocked_c = f
        .api
        .delete_starmap_node(&f.child_c, "c1")
        .expect_err("c1 被宿主 R 的跨画布边 to 引用，删除必须被拒绝");
    assert!(
        format!("{blocked_c:?}").contains("EdgeTo"),
        "c1 的引用来源应指明 EdgeTo，实际: {blocked_c:?}"
    );
}

/// 一次 update 把两端对调：`C/[e_c]/c1 → B/[e_b]/b1`。
fn swap_edge_endpoints(f: &Fx, edge_id: &str) {
    f.api
        .update_starmap_edge(
            &f.root,
            edge_id,
            StarMapEdgePatchDto {
                kind: Some(StarMapEdgeKindDto::Foreshadows),
                label: Some(Some("换向".to_string())),
                payload: None,
                from: Some(path(&f.root, vec![enter_embed(&f.e_c)], node_target("c1"))),
                to: Some(path(&f.root, vec![enter_embed(&f.e_b)], node_target("b1"))),
            },
        )
        .unwrap();
}

/// 宿主图仍只有同一条真实边，方向与元数据已按对调后的值落盘。
fn assert_swapped_storage(f: &Fx, edge_id: &str) {
    let root_graph = f.api.get_starmap_graph(&f.root).unwrap();
    assert_eq!(root_graph.edges.len(), 1, "同时改两端不应新增或复制边");
    let updated = root_graph
        .edges
        .iter()
        .find(|e| e.id == edge_id)
        .expect("宿主图应仍持有原边");
    assert_eq!(updated.kind, StarMapEdgeKindDto::Foreshadows);
    assert_eq!(updated.label.as_deref(), Some("换向"));
    assert_eq!(updated.from.starmap_id, f.root);
    assert_eq!(updated.from.segments, vec![enter_embed(&f.e_c)]);
    assert_eq!(updated.from.target, node_target("c1"));
    assert_eq!(updated.to.starmap_id, f.root);
    assert_eq!(updated.to.segments, vec![enter_embed(&f.e_b)]);
    assert_eq!(updated.to.target, node_target("b1"));
}

/// 对调后 `b1` 改由 `EdgeTo` 保护、`c1` 改由 `EdgeFrom` 保护。
fn assert_swapped_delete_guards(f: &Fx) {
    let blocked_b = f
        .api
        .delete_starmap_node(&f.child_b, "b1")
        .expect_err("对调后 b1 成为 to 端，仍应被跨画布引用拦截");
    assert!(
        format!("{blocked_b:?}").contains("EdgeTo"),
        "对调后 b1 的引用来源应改为 EdgeTo，实际: {blocked_b:?}"
    );
    let blocked_c = f
        .api
        .delete_starmap_node(&f.child_c, "c1")
        .expect_err("对调后 c1 成为 from 端，仍应被跨画布引用拦截");
    assert!(
        format!("{blocked_c:?}").contains("EdgeFrom"),
        "对调后 c1 的引用来源应改为 EdgeFrom，实际: {blocked_c:?}"
    );
}

/// 反查按完整实例路径隔离，且返回新方向的端点。
fn assert_reverse_lookup_is_route_aware(f: &Fx, edge_id: &str) {
    let via_b = f.api.list_starmap_connections(f.route_b(), None).unwrap();
    let conn_b = via_b
        .iter()
        .find(|c| c.edge_id == edge_id)
        .expect("经 e_b 进入 B 应能反查到宿主 R 的边");
    assert_eq!(conn_b.owner_starmap_id, f.root);
    assert_eq!(conn_b.from.segments, vec![enter_embed(&f.e_c)]);
    assert_eq!(conn_b.to.segments, vec![enter_embed(&f.e_b)]);

    let via_c = f.api.list_starmap_connections(f.route_c(), None).unwrap();
    let conn_c = via_c
        .iter()
        .find(|c| c.edge_id == edge_id)
        .expect("经 e_c 进入 C 应能反查到宿主 R 的边");
    assert_eq!(conn_c.owner_starmap_id, f.root);
    assert_eq!(conn_c.from.segments, vec![enter_embed(&f.e_c)]);
    assert_eq!(conn_c.to.segments, vec![enter_embed(&f.e_b)]);
}

/// 删除原始边后两端都解除引用，可正常删除节点。
fn assert_delete_edge_clears_both_guards(f: &Fx, edge_id: &str) {
    f.api.delete_starmap_edge(&f.root, edge_id).unwrap();
    assert!(
        f.api.delete_starmap_node(&f.child_b, "b1").unwrap(),
        "删除原始边后 b1 应可删除"
    );
    assert!(
        f.api.delete_starmap_node(&f.child_c, "c1").unwrap(),
        "删除原始边后 c1 应可删除"
    );
}

/// 一次 update 同时把 from 与 to 对调，两个来源事实都必须被替换。
#[test]
fn simultaneous_from_and_to_repoint_replaces_both_index_sources() {
    let (_tmp, f) = setup();

    let edge_id = create_initial_edge(&f);
    assert_initial_delete_guards(&f);

    swap_edge_endpoints(&f, &edge_id);
    assert_swapped_storage(&f, &edge_id);
    assert_swapped_delete_guards(&f);
    assert_reverse_lookup_is_route_aware(&f, &edge_id);

    assert_delete_edge_clears_both_guards(&f, &edge_id);
}
