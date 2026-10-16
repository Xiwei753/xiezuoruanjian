//! Issue #902 评论 6217580337 —「Anchor 作为**起点**(`from`)的跨画布连线」经真实
//! `WriterCoreApi` 验证。
//!
//! #902 实现路线第 2 节要求跨画布边端点目标可以是 `Node / Anchor / Starmap`，
//! 宿主由两端导航路线的**最长公共实例路径前缀**决定。此前各轮经真实
//! `WriterCoreApi` 覆盖的组合里，`Anchor` 只出现在 `to` 端：
//!
//! - 根图宿主、`to` 端 Anchor（评论 6208047237）；
//! - 中间画布宿主、`to` 端深层 Anchor（评论 6216900870）。
//!
//! **尚未覆盖**「Anchor 作为 `from` 起点」这一组合。两端身份与方向并不对称：
//! 起点 Anchor 既要求宿主裁剪后在 `from.segments` 保留深层穿越段，又要求删除
//! 保护以 `EdgeFrom` 身份指出外部引用，且 `to` 端落在宿主本层。若方向处理或
//! 来源身份算错，就会出现「起点侧画布反查不到 / 删锚点不被拦截 / 删节点报错
//! 指向错误的来源端」。
//!
//! 拓扑：`R --Embed(eB)--> B --Embed(eC)--> C`，`R` 有节点 `r1`，`B` 有节点
//! `b1`，`C` 有节点 `c1`（挂锚点 `a1`）。一条跨画布边：
//! `Anchor(c1, a1) → Node(b1)`，两端输入路线分别为 `R/[eB, eC]` 与 `R/[eB]`，
//! 宿主为中间画布 `B`。
//!
//! 1. 公共输入前缀是 `[eB]`，宿主是 `B`；落库后 `from.segments = [eC]`、
//!    `to.segments = []`；只有 `B` 写一条真实 Edge，`R`、`C` 都不落假边。
//! 2. `C` 的画布查询与 `Some(Anchor(c1, a1))` 查询都命中；`Some(Node(c1))`
//!    不得命中（起点身份是 Anchor 而非 Node）。
//! 3. `B` 的画布查询与 `Some(Node(b1))` 命中（`to` 端在宿主本层）；
//!    在 `B` 按 `Some(Anchor(c1, a1))` 查询不得命中（`from` 端到该 Anchor 的
//!    实例路径后缀是 `[eC]`，不是空）。
//! 4. 移除被外图边引用为**起点**的锚点 `a1` 必须被拒绝（`external
//!    dependencies would dangle` + `AnchorTarget(anchor=a1)`）；直接删除 `C`
//!    的 `c1` 也必须被拒绝，且来源身份必须指明 `EdgeFrom`。
//! 5. 删除宿主 `B` 中的原始边后，反向依赖索引同步失效：允许移除锚点、删除 `c1`。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tempfile::TempDir;
use writer_core::api::types::{
    StarMapAnchorDto, StarMapAnchorRoleDto, StarMapAnchorTargetDto, StarMapEdgeKindDto,
    StarMapEndpointSelectionDto, StarMapNavigationRouteDto, StarMapNodeDto, StarMapNodeKindDto,
    StarMapNodePatchDto, StarMapPathSegmentDto, StarMapPointDto, StarMapTargetDetailDto,
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

fn anchor_target(node_id: &str, anchor_id: &str) -> StarMapTargetDetailDto {
    StarMapTargetDetailDto {
        kind: "anchor".to_string(),
        node_id: Some(node_id.to_string()),
        anchor_id: Some(anchor_id.to_string()),
        ..Default::default()
    }
}

fn anchor(anchor_id: &str, starmap_id: &str) -> StarMapAnchorDto {
    StarMapAnchorDto {
        anchor_id: anchor_id.to_string(),
        target: StarMapAnchorTargetDto {
            kind: "starmap".to_string(),
            starmap_id: Some(starmap_id.to_string()),
            ..Default::default()
        },
        label: None,
        role: StarMapAnchorRoleDto::Source,
    }
}

fn patch_anchors(anchors: Vec<StarMapAnchorDto>) -> StarMapNodePatchDto {
    StarMapNodePatchDto {
        title: None,
        kind: None,
        payload: None,
        tags: None,
        content: None,
        anchors: Some(anchors),
        portal: None,
        position: None,
        style: None,
        provenance: None,
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
    b: String,
    c: String,
    eb: String,
    ec: String,
}

/// `R --eB--> B --eC--> C`，`r1`/`b1`/`c1` 分别落在三张画布，`c1` 挂锚点 `a1`。
fn setup() -> Fixture {
    let (tmp, api) = make_api();

    let root = api.create_starmap("根图", "", None).unwrap().starmap_id;
    let b_child = api
        .create_starmap_child_embed(&root, "子图B", point(0.0, 0.0))
        .unwrap();
    let b = b_child.starmap.starmap_id.clone();
    let eb = b_child.embed.instance_id.clone();
    let c_child = api
        .create_starmap_child_embed(&b, "子图C", point(0.0, 0.0))
        .unwrap();
    let c = c_child.starmap.starmap_id.clone();
    let ec = c_child.embed.instance_id.clone();

    api.add_starmap_node(&root, node("r1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&b, node("b1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&c, node("c1"), 0.0, 0.0).unwrap();

    let updated = api
        .update_starmap_node(&c, "c1", patch_anchors(vec![anchor("a1", &c)]))
        .unwrap();
    assert_eq!(updated.anchors.len(), 1, "锚点应写入 c1");
    assert_eq!(updated.anchors[0].anchor_id, "a1");

    Fixture {
        _tmp: tmp,
        api,
        root,
        b,
        c,
        eb,
        ec,
    }
}

/// 创建 `Anchor(c1, a1) → Node(b1)`（起点在深层 `C`），返回宿主 `B` 中的 Edge ID。
fn create_anchor_from_edge(f: &Fixture) -> String {
    let created = f
        .api
        .create_cross_canvas_edge(
            selection(
                route(&f.root, vec![enter_embed(&f.eb), enter_embed(&f.ec)]),
                anchor_target("c1", "a1"),
            ),
            selection(route(&f.root, vec![enter_embed(&f.eb)]), node_target("b1")),
            StarMapEdgeKindDto::References,
            Some("起点锚点".to_string()),
            None,
        )
        .unwrap();

    assert_eq!(
        created.owner_starmap_id, f.b,
        "公共前缀是 [eB]，宿主应是中间画布 B，而不是根 R"
    );
    assert_eq!(created.edge.from.starmap_id, f.b);
    assert_eq!(
        created.edge.from.segments,
        vec![enter_embed(&f.ec)],
        "from 起点从宿主 B 出发还需再穿 eC 才能到 C 的锚点"
    );
    assert_eq!(created.edge.from.target, anchor_target("c1", "a1"));
    assert_eq!(created.edge.to.starmap_id, f.b);
    assert!(
        created.edge.to.segments.is_empty(),
        "to 在宿主 B 本层，穿越路径应为空"
    );
    assert_eq!(created.edge.to.target, node_target("b1"));
    created.edge.id
}

/// 唯一真实 Edge 只落在中间画布 `B`，`R`、`C` 都不落假边。
fn assert_edge_only_in_intermediate_host(f: &Fixture, edge_id: &str) {
    let b_graph = f.api.get_starmap_graph(&f.b).unwrap();
    assert_eq!(b_graph.edges.len(), 1, "宿主 B 必须只写入一条 Edge");
    assert_eq!(b_graph.edges[0].id, edge_id);

    assert!(
        f.api.get_starmap_graph(&f.root).unwrap().edges.is_empty(),
        "根画布 R 不应写入伪造的本地 Edge"
    );
    assert!(
        f.api.get_starmap_graph(&f.c).unwrap().edges.is_empty(),
        "起点画布 C 不应写入伪造的本地 Edge"
    );
}

/// `C` 的反查：画布与 Anchor target 命中；把 Anchor 当成 Node 查不得命中。
fn assert_source_canvas_query_is_anchor_aware(f: &Fixture, edge_id: &str) {
    let c_route = route(&f.root, vec![enter_embed(&f.eb), enter_embed(&f.ec)]);

    let c_canvas = f
        .api
        .list_starmap_connections(c_route.clone(), None)
        .unwrap();
    assert!(
        c_canvas
            .iter()
            .any(|c| c.edge_id == edge_id && c.owner_starmap_id == f.b),
        "C 画布应反查到以 C 的 Anchor 为起点、存于中间画布 B 的边"
    );

    let c_anchor = f
        .api
        .list_starmap_connections(c_route.clone(), Some(anchor_target("c1", "a1")))
        .unwrap();
    assert!(
        c_anchor.iter().any(|c| c.edge_id == edge_id),
        "按 Anchor(c1, a1) 在 C 查询应命中该边"
    );

    let c_node = f
        .api
        .list_starmap_connections(c_route, Some(node_target("c1")))
        .unwrap();
    assert!(
        c_node.iter().all(|c| c.edge_id != edge_id),
        "Anchor 起点不应被 Node(c1) 查询命中"
    );
}

/// `B` 的反查：本层 to 端命中；深层 Anchor 起点的实例路径后缀不是空，故不得误命中。
fn assert_host_canvas_query_is_route_aware(f: &Fixture, edge_id: &str) {
    let b_route = route(&f.root, vec![enter_embed(&f.eb)]);

    let b_canvas = f
        .api
        .list_starmap_connections(b_route.clone(), None)
        .unwrap();
    assert!(
        b_canvas.iter().any(|c| c.edge_id == edge_id),
        "B 画布应能看到 to 端落在本层的边"
    );

    let b_node = f
        .api
        .list_starmap_connections(b_route.clone(), Some(node_target("b1")))
        .unwrap();
    assert!(
        b_node.iter().any(|c| c.edge_id == edge_id),
        "按 Node(b1) 在宿主 B 查询应命中该边"
    );

    let b_anchor = f
        .api
        .list_starmap_connections(b_route, Some(anchor_target("c1", "a1")))
        .unwrap();
    assert!(
        b_anchor.iter().all(|c| c.edge_id != edge_id),
        "宿主 B 到深层 Anchor 起点的实例路径后缀是 [eC] 而非空，不得误命中"
    );
}

/// 被外图边引用为起点的锚点与节点在解除引用前都不得被破坏性删除/修改。
fn assert_anchor_from_is_guarded(f: &Fixture) {
    let blocked_anchor = f
        .api
        .update_starmap_node(&f.c, "c1", patch_anchors(vec![]))
        .expect_err("被中间画布 B 的边引用为起点的锚点不得被移除");
    let anchor_message = format!("{blocked_anchor:?}");
    assert!(
        anchor_message.contains("external dependencies would dangle"),
        "必须以外部依赖悬空为由拒绝移除锚点；实际: {anchor_message}"
    );
    assert!(
        anchor_message.contains("AnchorTarget(anchor=a1)"),
        "必须指出是锚点 a1 的外部依赖；实际: {anchor_message}"
    );

    // 被拒绝的更新不得实际移除锚点。
    let c_graph = f.api.get_starmap_graph(&f.c).unwrap();
    let c1 = c_graph
        .nodes
        .iter()
        .find(|n| n.id == "c1")
        .expect("c1 必须仍在 C");
    assert!(
        c1.anchors.iter().any(|a| a.anchor_id == "a1"),
        "被拒绝后 c1 必须仍保留锚点 a1"
    );

    let blocked_node = f
        .api
        .delete_starmap_node(&f.c, "c1")
        .expect_err("被中间画布 B 的跨画布边作为起点指向的 c1 不得被删除");
    let node_message = format!("{blocked_node:?}");
    assert!(
        node_message.contains("EdgeFrom"),
        "起点侧引用的来源身份应指明 EdgeFrom；实际: {node_message}"
    );
    assert!(
        !node_message.contains("EdgeTo"),
        "起点锚点不得被误标为 EdgeTo 来源；实际: {node_message}"
    );
}

/// 删除宿主原始边后解除外部引用：允许移除锚点、删除 c1。
fn assert_delete_edge_frees_anchor_from(f: &Fixture, edge_id: &str) {
    assert!(
        f.api.delete_starmap_edge(&f.b, edge_id).unwrap(),
        "删除中间画布 B 中的跨画布边应返回 true"
    );
    assert!(f.api.get_starmap_graph(&f.b).unwrap().edges.is_empty());

    let after = f
        .api
        .update_starmap_node(&f.c, "c1", patch_anchors(vec![]))
        .unwrap();
    assert!(after.anchors.is_empty(), "解除外部引用后应允许移除锚点");

    assert!(
        f.api.delete_starmap_node(&f.c, "c1").unwrap(),
        "解除外部引用后应允许删除 c1"
    );
}

/// Anchor 起点 + 中间画布宿主：宿主选择、route-aware 反查与删除保护都正确。
#[test]
fn anchor_from_endpoint_is_hosted_by_intermediate_canvas_and_guarded() {
    let f = setup();

    let edge_id = create_anchor_from_edge(&f);
    assert_edge_only_in_intermediate_host(&f, &edge_id);
    assert_source_canvas_query_is_anchor_aware(&f, &edge_id);
    assert_host_canvas_query_is_route_aware(&f, &edge_id);
    assert_anchor_from_is_guarded(&f);
    assert_delete_edge_frees_anchor_from(&f, &edge_id);
}
