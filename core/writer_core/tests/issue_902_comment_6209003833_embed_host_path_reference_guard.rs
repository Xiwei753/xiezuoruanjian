//! Issue #902 评论 6209003833 — 跨画布 **Embed host_path** 引用的删除保护。
//!
//! #902 实现路线第 3 节要求统一的反向依赖索引收录 Edge 两端、Link 两端、
//! **Embed host_path**、Portal 入口。此前各轮已通过 `WriterCoreApi` 覆盖 Edge
//! 端点、Link target、Embed 入口、Portal 穿行与 Anchor 终点，但 **Embed 自己的
//! host_path 作为跨画布引用来源**尚未被真实验证。
//!
//! 本测试构造根图 R，R 通过 Embed `eB` 进入 B、通过 Embed `eC` 进入 C，并在 B 中
//! 放置节点 b1。随后在 R 中新增 Embed `ex`（目标 C），其 `host_path` 写成
//! `R/[eB]/Node(b1)`——实例 `ex` 挂在 B 的节点 b1 上。于是 b1 成为 **R 持有** 的
//! 跨画布引用终点，而 `ex` 对 `eB` 的穿越引用则是 **R 本图** 依赖。
//!
//! 1. `ex.host_path` 能通过正式 API 写入并原样读回；
//! 2. 删除 B 的 b1 必须被拒绝，报错须指明 `kind=EmbedHostPath`、`source=ex`
//!    的外部来源；被拒绝后 b1 仍在；删除 R 中的 `ex` 解除引用后才允许删除 b1；
//! 3. 对照：删除同宿主图 R 中的 `eB` 属于 **本图** 依赖，不走跨画布拦截，
//!    而由本地级联一并删除 `ex`，证明删除保护只拦外部来源。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tempfile::TempDir;
use writer_core::api::types::{
    StarMapEmbedDto, StarMapNodeDto, StarMapNodeKindDto, StarMapPathSegmentDto, StarMapPointDto,
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

fn enter_embed(instance_id: &str) -> StarMapPathSegmentDto {
    StarMapPathSegmentDto {
        kind: "enterEmbed".to_string(),
        instance_id: Some(instance_id.to_string()),
        node_id: None,
    }
}

fn embed_host_path(host: &str, segments: Vec<StarMapPathSegmentDto>) -> StarMapTargetPathDto {
    StarMapTargetPathDto {
        starmap_id: host.to_string(),
        segments,
        target: node_target("b1"),
    }
}

struct Fixture {
    _tmp: TempDir,
    api: WriterCoreApi,
    r: String,
    b: String,
    eb: String,
}

/// 构造 R → (B, C)，B 内有 b1；再在 R 中写入 Embed `ex`（目标 C），
/// `host_path` 经 `eB` 指向 B 的 b1。
fn setup_cross_host_path() -> Fixture {
    let (tmp, api) = make_api();
    let r = api.create_starmap("根图", "", None).unwrap().starmap_id;
    let b_child = api
        .create_starmap_child_embed(&r, "子图 B", point(0.0, 0.0))
        .unwrap();
    let b = b_child.starmap.starmap_id.clone();
    let eb = b_child.embed.instance_id.clone();

    let c_child = api
        .create_starmap_child_embed(&r, "子图 C", point(100.0, 0.0))
        .unwrap();
    let c = c_child.starmap.starmap_id.clone();

    api.add_starmap_node(&b, node("b1"), 0.0, 0.0).unwrap();

    let ex = api
        .add_starmap_embed(
            &r,
            StarMapEmbedDto {
                instance_id: "ex".to_string(),
                target_starmap_id: c.clone(),
                label: None,
                position: point(0.0, 0.0),
                host_path: embed_host_path(&r, vec![enter_embed(&eb)]),
                provenance: Default::default(),
                created_at: 0,
                updated_at: 0,
            },
        )
        .unwrap();
    assert_eq!(
        ex.host_path.segments,
        vec![enter_embed(&eb)],
        "host_path 的跨画布进入路径必须保留"
    );
    assert_eq!(ex.host_path.target, node_target("b1"));

    // 从真实图数据读回，而不是只检查刚返回的 DTO。
    let r_graph = api.get_starmap_graph(&r).unwrap();
    assert!(
        r_graph.embeds.iter().any(|e| e.instance_id == "ex"),
        "R 的真实图数据必须包含 Embed ex"
    );

    Fixture {
        _tmp: tmp,
        api,
        r,
        b,
        eb,
    }
}

/// R 中 Embed `ex` 的 host_path 落在 B 的 b1；删除 b1 必须被 EmbedHostPath 来源拦下。
#[test]
fn cross_canvas_embed_host_path_blocks_target_node_removal() {
    let f = setup_cross_host_path();

    // 删除 B 的 b1：ex.host_path 是外部（owner=R）依赖。
    let error = f
        .api
        .delete_starmap_node(&f.b, "b1")
        .expect_err("被外图 Embed host_path 引用的节点不得被删除");
    let message = format!("{error:?}");
    assert!(
        message.contains("cross-canvas references exist"),
        "必须以跨画布引用存在为由拒绝；实际: {message}"
    );
    assert!(
        message.contains("kind=EmbedHostPath") && message.contains("source=ex"),
        "必须指明 R 中 Embed ex 的 host_path 依赖；实际: {message}"
    );

    // 被拒绝的删除不得真正生效。
    let b_graph = f.api.get_starmap_graph(&f.b).unwrap();
    assert!(
        b_graph.nodes.iter().any(|n| n.id == "b1"),
        "被拒绝后 b1 必须仍在 B"
    );

    // 解除外部引用后，b1 才可以删除。
    assert!(
        f.api.delete_starmap_embed(&f.r, "ex").unwrap(),
        "删除宿主图 R 中的 Embed ex 应返回 true"
    );
    assert!(
        f.api.delete_starmap_node(&f.b, "b1").unwrap(),
        "解除 Embed host_path 引用后应允许删除 b1"
    );
    assert!(
        f.api
            .get_starmap_graph(&f.b)
            .unwrap()
            .nodes
            .iter()
            .all(|n| n.id != "b1"),
        "b1 必须已被删除"
    );
}

/// 对照：删除同宿主图 R 中被 host_path 穿越的 `eB` 属于本图依赖，
/// 不走跨画布拦截，而由本地级联一起删除 `ex`。
#[test]
fn local_embed_host_path_traversal_is_cascaded_not_blocked() {
    let f = setup_cross_host_path();

    assert!(
        f.api.delete_starmap_embed(&f.r, &f.eb).unwrap(),
        "同图依赖应走本地级联，而不是被跨画布删除保护拦下"
    );
    let r_graph = f.api.get_starmap_graph(&f.r).unwrap();
    assert!(
        r_graph.embeds.iter().all(|e| e.instance_id != "ex"),
        "穿越引用 ex 应随本地宿主 eB 一起被级联删除"
    );
    assert!(
        r_graph.embeds.iter().all(|e| e.instance_id != f.eb),
        "eB 必须已被删除"
    );
}
