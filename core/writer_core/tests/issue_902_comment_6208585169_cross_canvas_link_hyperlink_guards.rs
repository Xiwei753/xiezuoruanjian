//! Issue #902 评论 6208585169 — 跨画布 **Link / Hyperlink** 引用的删除保护。
//!
//! #902 实现路线第 3 节要求统一的反向依赖索引收录 Edge 两端、**Link 两端**、
//! **Hyperlink source**、Embed host_path、Portal 入口。此前几轮已覆盖 Edge 端点、
//! Embed 入口、Portal 穿行与 Anchor 终点，但 `Link` / `Hyperlink` 作为跨画布
//! 引用来源尚未经 `WriterCoreApi` 验证。
//!
//! 本测试构造 `R → Embed(eB) → B`，在 R 中写入：
//!
//! 1. 一条 Link，`source = R/[]/Node(r1)`、`target = R/[eB]/Node(b1)`；
//! 2. 一条 Hyperlink，`source = R/[eB]/Node(b1)`、`target_uri` 指向外部地址。
//!
//! 二者都存于 R，却把路径终点落在 B 的节点 b1 上。断言：
//!
//! 1. 两条对象都能通过正式 API 读回，端点/进入路径保持原样；
//! 2. 删除 B 的 b1 必须被拒绝，报错须分别指明 `kind=LinkTarget` 与
//!    `kind=HyperlinkSource` 的外部来源（而不是笼统的“有引用”）；
//! 3. 被拒绝后 b1 仍在；删除 R 中的 Link / Hyperlink 解除引用后，才允许删除 b1。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tempfile::TempDir;
use writer_core::api::types::{
    StarMapHyperlinkDto, StarMapLinkDto, StarMapNodeDto, StarMapNodeKindDto, StarMapPathSegmentDto,
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

/// R 存有 Link / Hyperlink，路径终点落在 B 的 b1；删除 b1 必须分别被两类来源拦下。
#[test]
fn cross_canvas_link_and_hyperlink_block_target_node_removal() {
    let (_tmp, api) = make_api();

    let r = api.create_starmap("根图", "", None).unwrap().starmap_id;
    let b_child = api
        .create_starmap_child_embed(&r, "子图", point(0.0, 0.0))
        .unwrap();
    let b = b_child.starmap.starmap_id.clone();
    let eb = b_child.embed.instance_id.clone();

    api.add_starmap_node(&r, node("r1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&b, node("b1"), 0.0, 0.0).unwrap();

    // R 内的 Link：本地源节点 r1 → 经 eB 进入 B 的 b1。
    let link = api
        .add_starmap_link(
            &r,
            StarMapLinkDto {
                link_id: "l1".to_string(),
                source: path(&r, vec![], node_target("r1")),
                target: path(&r, vec![enter_embed(&eb)], node_target("b1")),
                label: Some("跨画布跳转".to_string()),
                created_at: 0,
                updated_at: 0,
            },
        )
        .unwrap();
    assert_eq!(link.target.starmap_id, r, "路径起点必须是宿主图 R");
    assert_eq!(
        link.target.segments,
        vec![enter_embed(&eb)],
        "Link 的跨画布进入路径必须保留"
    );
    assert_eq!(link.target.target, node_target("b1"));

    // R 内的 Hyperlink：source 经 eB 进入 B 的 b1，指向外部 URI。
    let hyperlink = api
        .add_starmap_hyperlink(
            &r,
            StarMapHyperlinkDto {
                hyperlink_id: "h1".to_string(),
                source: path(&r, vec![enter_embed(&eb)], node_target("b1")),
                target_uri: "https://example.com/doc".to_string(),
                label: None,
                created_at: 0,
                updated_at: 0,
            },
        )
        .unwrap();
    assert_eq!(
        hyperlink.source.segments,
        vec![enter_embed(&eb)],
        "Hyperlink 的跨画布 source 路径必须保留"
    );

    // 两条对象都能从真实图数据读回（不是只检查刚返回的 DTO）。
    let r_graph = api.get_starmap_graph(&r).unwrap();
    assert!(
        r_graph.links.iter().any(|l| l.link_id == "l1"),
        "R 的真实图数据必须包含 Link l1"
    );
    assert!(
        r_graph.hyperlinks.iter().any(|h| h.hyperlink_id == "h1"),
        "R 的真实图数据必须包含 Hyperlink h1"
    );

    // 删除 B 的 b1：Link target 与 Hyperlink source 都是外部（owner=R）依赖。
    let error = api
        .delete_starmap_node(&b, "b1")
        .expect_err("被外图 Link / Hyperlink 引用的节点不得被删除");
    let message = format!("{error:?}");
    assert!(
        message.contains("cross-canvas references exist"),
        "必须以跨画布引用存在为由拒绝；实际: {message}"
    );
    assert!(
        message.contains("kind=LinkTarget") && message.contains("source=l1"),
        "必须指明 R 中 Link l1 的 target 端依赖；实际: {message}"
    );
    assert!(
        message.contains("kind=HyperlinkSource") && message.contains("source=h1"),
        "必须指明 R 中 Hyperlink h1 的 source 端依赖；实际: {message}"
    );

    // 被拒绝的删除不得真正生效。
    let b_graph = api.get_starmap_graph(&b).unwrap();
    assert!(
        b_graph.nodes.iter().any(|n| n.id == "b1"),
        "被拒绝后 b1 必须仍在 B"
    );

    // 解除两类外部引用后，b1 才可以删除。
    assert!(
        api.delete_starmap_link(&r, "l1").unwrap(),
        "删除宿主图 R 中的 Link 应返回 true"
    );
    assert!(
        api.delete_starmap_hyperlink(&r, "h1").unwrap(),
        "删除宿主图 R 中的 Hyperlink 应返回 true"
    );
    assert!(
        api.delete_starmap_node(&b, "b1").unwrap(),
        "解除外部引用后应允许删除 b1"
    );
    assert!(
        api.get_starmap_graph(&b)
            .unwrap()
            .nodes
            .iter()
            .all(|n| n.id != "b1"),
        "b1 必须已被删除"
    );
}
