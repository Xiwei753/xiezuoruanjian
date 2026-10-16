//! Issue #902 评论 6210626655 — 跨画布反向依赖索引在修改 **Hyperlink source** 与
//! **Embed host_path** 后的增量刷新。
//!
//! #902 实现路线第 3 节要求统一的反向依赖索引收录 Edge 两端、Link 两端、
//! Hyperlink source、Embed host_path、Portal 入口，并在“关系写入/修改/删除后
//! 增量更新，读取不同入口不会重复制造关系”。
//!
//! 上一轮（评论 6209895056）的集成测试已经用真实 `WriterCoreApi` 覆盖了
//! `update_starmap_edge` / `update_starmap_link` 改变端点后的索引刷新。但索引收录的
//! 来源类型里，**Hyperlink 的 source** 与 **Embed 自己的 host_path** 的**修改**
//! 路径仍未验证：若 `update_starmap_hyperlink` / `update_starmap_embed` 只写入新
//! 事实而漏删旧事实，就会留下"指向已不再引用的画布的悬空引用"或"改完仍拦截旧
//! 目标"。
//!
//! 拓扑：根图 R 有节点 `r1`，经 Embed `e_b` 进入子图 B（节点 `b1`）、经 Embed
//! `e_c` 进入子图 C（节点 `c1`），另有子图 D 作为被嵌入目标。
//!
//! 1. `update_starmap_hyperlink` 把 `source` 从 `R/[e_b]/b1` 改到 `R/[e_c]/c1`：
//!    真实图数据里 source 已更新，B 的 b1 解除外部引用可删，C 的 c1 转为被
//!    `HyperlinkSource` 拦截；删除该 Hyperlink 后 c1 才可删。
//! 2. `update_starmap_embed` 把 `host_path` 从 `R/[e_b]/b1` 改到 `R/[e_c]/c1`：
//!    删除保护随之由 b1 迁移到 c1，报错来源始终是 `EmbedHostPath` / `source=ex`。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tempfile::TempDir;
use writer_core::api::types::{
    StarMapEmbedDto, StarMapEmbedPatchDto, StarMapHyperlinkDto, StarMapHyperlinkPatchDto,
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

struct Fixture {
    _tmp: TempDir,
    api: WriterCoreApi,
    r: String,
    b: String,
    c: String,
    d: String,
    e_b: String,
    e_c: String,
}

fn setup() -> Fixture {
    let (tmp, api) = make_api();
    let r = api.create_starmap("根图", "", None).unwrap().starmap_id;

    let b_child = api
        .create_starmap_child_embed(&r, "子图B", point(0.0, 0.0))
        .unwrap();
    let b = b_child.starmap.starmap_id.clone();
    let e_b = b_child.embed.instance_id.clone();

    let c_child = api
        .create_starmap_child_embed(&r, "子图C", point(100.0, 0.0))
        .unwrap();
    let c = c_child.starmap.starmap_id.clone();
    let e_c = c_child.embed.instance_id.clone();

    let d_child = api
        .create_starmap_child_embed(&r, "子图D", point(200.0, 0.0))
        .unwrap();
    let d = d_child.starmap.starmap_id.clone();

    api.add_starmap_node(&r, node("r1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&b, node("b1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&c, node("c1"), 0.0, 0.0).unwrap();

    Fixture {
        _tmp: tmp,
        api,
        r,
        b,
        c,
        d,
        e_b,
        e_c,
    }
}

/// 移动 Hyperlink 的 `source` 后，反向索引与删除保护都必须跟着迁移。
#[test]
fn repointing_hyperlink_source_moves_reverse_index_and_delete_guard() {
    let f = setup();

    // 初始 source 经 e_b 落在 B 的 b1。
    let created = f
        .api
        .add_starmap_hyperlink(
            &f.r,
            StarMapHyperlinkDto {
                hyperlink_id: "h_move".to_string(),
                source: path(&f.r, vec![enter_embed(&f.e_b)], node_target("b1")),
                target_uri: "https://example.com/a".to_string(),
                label: Some("外链".to_string()),
                created_at: 0,
                updated_at: 0,
            },
        )
        .unwrap();
    assert_eq!(created.source.segments, vec![enter_embed(&f.e_b)]);
    assert_eq!(created.source.target, node_target("b1"));

    // 初始：删除 b1 被外图 Hyperlink source 拦截。
    let blocked = f
        .api
        .delete_starmap_node(&f.b, "b1")
        .expect_err("b1 被宿主 R 的 Hyperlink source 引用，删除必须被拒绝");
    let blocked_text = format!("{blocked:?}");
    assert!(
        blocked_text.contains("cross-canvas references exist"),
        "应以跨画布引用存在为由拒绝；实际: {blocked_text}"
    );
    assert!(
        blocked_text.contains("kind=HyperlinkSource") && blocked_text.contains("source=h_move"),
        "必须指明 R 中 Hyperlink h_move 的 source 依赖；实际: {blocked_text}"
    );

    // 修改：source 从 B/[e_b]/b1 改到 C/[e_c]/c1，同时改 target_uri 与 label。
    let updated = f
        .api
        .update_starmap_hyperlink(
            &f.r,
            "h_move",
            StarMapHyperlinkPatchDto {
                label: Some(Some("改".to_string())),
                target_uri: Some("https://example.com/b".to_string()),
                source: Some(path(&f.r, vec![enter_embed(&f.e_c)], node_target("c1"))),
            },
        )
        .unwrap();
    assert_eq!(updated.source.segments, vec![enter_embed(&f.e_c)]);
    assert_eq!(updated.source.target, node_target("c1"));
    assert_eq!(updated.target_uri, "https://example.com/b");

    // 真实图数据必须已更新，而不是只检查返回值。
    let r_graph = f.api.get_starmap_graph(&f.r).unwrap();
    let stored = r_graph
        .hyperlinks
        .iter()
        .find(|h| h.hyperlink_id == "h_move")
        .expect("宿主图 R 应仍持有该 Hyperlink");
    assert_eq!(stored.source.starmap_id, f.r);
    assert_eq!(stored.source.segments, vec![enter_embed(&f.e_c)]);
    assert_eq!(stored.source.target, node_target("c1"));
    assert_eq!(stored.target_uri, "https://example.com/b");
    assert_eq!(stored.label.as_deref(), Some("改"));

    // 删除保护迁移：旧目标 b1 现在可删，新目标 c1 现在被拦截。
    assert!(
        f.api.delete_starmap_node(&f.b, "b1").unwrap(),
        "旧目标 b1 不再被 Hyperlink 引用，应可删除"
    );
    let blocked_c = f
        .api
        .delete_starmap_node(&f.c, "c1")
        .expect_err("新目标 c1 被宿主 R 的 Hyperlink source 引用，删除必须被拒绝");
    let blocked_c_text = format!("{blocked_c:?}");
    assert!(
        blocked_c_text.contains("kind=HyperlinkSource") && blocked_c_text.contains("source=h_move"),
        "c1 的引用来源应仍指明 HyperlinkSource / h_move；实际: {blocked_c_text}"
    );

    // 删除原始 Hyperlink 后，c1 解除引用可删；证明索引没有留下指向旧目标的悬空事实。
    assert!(
        f.api.delete_starmap_hyperlink(&f.r, "h_move").unwrap(),
        "删除宿主图 R 中的 Hyperlink 应返回 true"
    );
    assert!(
        f.api.delete_starmap_node(&f.c, "c1").unwrap(),
        "删除原始 Hyperlink 后 c1 应可删除"
    );
}

/// 移动 Embed 自己的 `host_path` 后，反向索引与删除保护都必须跟着迁移。
#[test]
fn repointing_embed_host_path_moves_reverse_index_and_delete_guard() {
    let f = setup();

    // ex 存于 R、目标子图 D；host_path 经 e_b 落在 B 的 b1 上。
    let created = f
        .api
        .add_starmap_embed(
            &f.r,
            StarMapEmbedDto {
                instance_id: "ex".to_string(),
                target_starmap_id: f.d.clone(),
                label: None,
                position: point(0.0, 0.0),
                host_path: path(&f.r, vec![enter_embed(&f.e_b)], node_target("b1")),
                provenance: Default::default(),
                created_at: 0,
                updated_at: 0,
            },
        )
        .unwrap();
    assert_eq!(created.host_path.segments, vec![enter_embed(&f.e_b)]);
    assert_eq!(created.host_path.target, node_target("b1"));

    // 初始：删除 b1 被外图 Embed host_path 拦截。
    let blocked = f
        .api
        .delete_starmap_node(&f.b, "b1")
        .expect_err("b1 被宿主 R 的 Embed host_path 引用，删除必须被拒绝");
    let blocked_text = format!("{blocked:?}");
    assert!(
        blocked_text.contains("cross-canvas references exist"),
        "应以跨画布引用存在为由拒绝；实际: {blocked_text}"
    );
    assert!(
        blocked_text.contains("kind=EmbedHostPath") && blocked_text.contains("source=ex"),
        "必须指明 R 中 Embed ex 的 host_path 依赖；实际: {blocked_text}"
    );

    // 修改：host_path 从 B/[e_b]/b1 改到 C/[e_c]/c1。
    let updated = f
        .api
        .update_starmap_embed(
            &f.r,
            "ex",
            StarMapEmbedPatchDto {
                label: None,
                position: None,
                host_path: Some(path(&f.r, vec![enter_embed(&f.e_c)], node_target("c1"))),
            },
        )
        .unwrap();
    assert_eq!(updated.host_path.segments, vec![enter_embed(&f.e_c)]);
    assert_eq!(updated.host_path.target, node_target("c1"));

    // 真实图数据必须已更新。
    let r_graph = f.api.get_starmap_graph(&f.r).unwrap();
    let stored = r_graph
        .embeds
        .iter()
        .find(|e| e.instance_id == "ex")
        .expect("宿主图 R 应仍持有 Embed ex");
    assert_eq!(stored.host_path.starmap_id, f.r);
    assert_eq!(stored.host_path.segments, vec![enter_embed(&f.e_c)]);
    assert_eq!(stored.host_path.target, node_target("c1"));
    assert_eq!(stored.target_starmap_id, f.d);

    // 删除保护迁移：旧宿主目标 b1 现在可删，新宿主目标 c1 现在被拦截。
    assert!(
        f.api.delete_starmap_node(&f.b, "b1").unwrap(),
        "旧宿主节点 b1 不再被 Embed host_path 引用，应可删除"
    );
    let blocked_c = f
        .api
        .delete_starmap_node(&f.c, "c1")
        .expect_err("新宿主节点 c1 被宿主 R 的 Embed host_path 引用，删除必须被拒绝");
    let blocked_c_text = format!("{blocked_c:?}");
    assert!(
        blocked_c_text.contains("kind=EmbedHostPath") && blocked_c_text.contains("source=ex"),
        "c1 的引用来源应仍指明 EmbedHostPath / ex；实际: {blocked_c_text}"
    );

    // 删除 Embed ex 后，c1 解除引用可删。
    assert!(
        f.api.delete_starmap_embed(&f.r, "ex").unwrap(),
        "删除宿主图 R 中的 Embed ex 应返回 true"
    );
    assert!(
        f.api.delete_starmap_node(&f.c, "c1").unwrap(),
        "删除 Embed ex 后 c1 应可删除"
    );
}
