//! Issue #902 评论 6212499958 — 正式 `resolve_starmap_path` 与
//! `find_starmap_references` / `delete_starmap` 集成测试。
//!
//! #902 要求 Core 提供「当前画布解析 / 关联反查」的统一入口。此前几轮已用真实
//! `WriterCoreApi` 覆盖导航 API、跨画布连线与增量引用索引，但**独立的两条对外
//! 入口**尚未从公开 API 验证：
//!
//! - `resolve_starmap_path(root, segments)`：按 root + 路径段逐层解析当前画布；
//! - `find_starmap_references(target)`：反查所有引用某张星图的宿主对象；
//! - `delete_starmap`：整张星图删除必须被外部引用拦住，解除引用后放行。
//!
//! 本测试构造三层画布 `R --Embed(eB)--> B --Portal(pC)--> C`，并在 R 内建一条
//! `R/[]/Node(r1) → R/[eB]/Node(b1)` 的跨画布边，验证：
//!
//! 1. `resolve_starmap_path` 逐层返回 R / B / C；不存在的 Embed 段显式报错，
//!    平台端不得回退到裸目标 ID。
//! 2. `find_starmap_references(B)` 同时报告 R 持有的 Embed 与 Edge 两类引用；
//!    `find_starmap_references(C)` 报告 B 的 Portal 引用；根图 R 只报告自身的
//!    自引用（`host == target`，不计入删除保护）。
//! 3. 删除被引用的 C（Portal 目标）与 B（Embed 目标）都会被拒绝；清除 Portal /
//!    删除 Embed 解除引用后，`delete_starmap` 才真正删除。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tempfile::TempDir;
use writer_core::api::types::{
    StarMapEdgeKindDto, StarMapEndpointSelectionDto, StarMapNavigationRouteDto, StarMapNodeDto,
    StarMapNodeKindDto, StarMapNodePatchDto, StarMapPathSegmentDto, StarMapPointDto,
    StarMapPortalDto, StarMapTargetDetailDto,
};
use writer_core::api::WriterCoreApi;

fn make_api() -> (TempDir, WriterCoreApi) {
    writer_core::storage::git_runtime::ensure_initialized().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let app_data_root = tmp.path().to_path_buf();
    let projects_root = app_data_root.join("projects");
    std::fs::create_dir_all(&projects_root).unwrap();
    // `delete_starmap` 成功路径会记录 workspace Git history，需先确保仓库存在。
    let layout = writer_core::storage::git_repo_layout::GitRepoLayout::new(app_data_root.clone());
    writer_core::storage::workspace_git::ensure_workspace_repo(&layout).unwrap();
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

fn portal_node(id: &str, destination: &str) -> StarMapNodeDto {
    StarMapNodeDto {
        portal: Some(StarMapPortalDto {
            destination_starmap_id: destination.to_string(),
            destination_target: None,
        }),
        ..node(id)
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

fn enter_portal(node_id: &str) -> StarMapPathSegmentDto {
    StarMapPathSegmentDto {
        kind: "enterPortal".to_string(),
        instance_id: None,
        node_id: Some(node_id.to_string()),
    }
}

fn embed_at(
    host: &str,
    instance_id: &str,
    target: &str,
) -> writer_core::api::types::StarMapEmbedDto {
    writer_core::api::types::StarMapEmbedDto {
        instance_id: instance_id.to_string(),
        target_starmap_id: target.to_string(),
        label: None,
        position: point(0.0, 0.0),
        host_path: writer_core::api::types::StarMapTargetPathDto {
            starmap_id: host.to_string(),
            segments: vec![],
            target: StarMapTargetDetailDto::default(),
        },
        provenance: Default::default(),
        created_at: 0,
        updated_at: 0,
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
    pc: String,
}

/// `R --eB--> B --pC--> C`，R 内有节点 r1，B 内 b1 与 Portal 节点 pC，C 内 c1。
fn make_three_level_fixture() -> Fixture {
    let (tmp, api) = make_api();
    let root = api.create_starmap("根图", "", None).unwrap().starmap_id;
    let b = api.create_starmap("子图 B", "", None).unwrap().starmap_id;
    let c = api.create_starmap("子图 C", "", None).unwrap().starmap_id;

    api.add_starmap_node(&root, node("r1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&b, node("b1"), 0.0, 0.0).unwrap();
    api.add_starmap_node(&c, node("c1"), 0.0, 0.0).unwrap();

    api.add_starmap_embed(&root, embed_at(&root, "eB", &b))
        .unwrap();
    api.add_starmap_node(&b, portal_node("pC", &c), 0.0, 0.0)
        .unwrap();

    api.flush_all_starmap_stores().unwrap();

    Fixture {
        _tmp: tmp,
        api,
        root,
        b,
        c,
        eb: "eB".to_string(),
        pc: "pC".to_string(),
    }
}

fn patch_clear_portal() -> StarMapNodePatchDto {
    StarMapNodePatchDto {
        title: None,
        kind: None,
        payload: None,
        tags: None,
        content: None,
        anchors: None,
        portal: Some(None),
        position: None,
        style: None,
        provenance: None,
    }
}

/// `resolve_starmap_path` 逐层解析当前画布；缺失 Embed 段必须报错而不是回退。
#[test]
fn resolve_starmap_path_walks_each_canvas_and_rejects_missing_embed() {
    let f = make_three_level_fixture();

    // 空路径 = 根图 R。
    let at_root = f.api.resolve_starmap_path(&f.root, vec![]).unwrap();
    assert_eq!(at_root.final_starmap_id, f.root);

    // 经 Embed 进 B。
    let at_b = f
        .api
        .resolve_starmap_path(&f.root, vec![enter_embed(&f.eb)])
        .unwrap();
    assert_eq!(at_b.final_starmap_id, f.b);

    // 经 Embed 再经 Portal 进 C。
    let at_c = f
        .api
        .resolve_starmap_path(&f.root, vec![enter_embed(&f.eb), enter_portal(&f.pc)])
        .unwrap();
    assert_eq!(at_c.final_starmap_id, f.c);

    // 不存在的 Embed 段必须显式失败，不能返回起点或裸目标 ID。
    let error = f
        .api
        .resolve_starmap_path(&f.root, vec![enter_embed("missing")])
        .expect_err("不存在的 Embed 段必须解析失败");
    let message = format!("{error:?}");
    assert!(
        message.contains("MissingEmbed"),
        "必须暴露 MissingEmbed 诊断；实际: {message}"
    );
}

/// `find_starmap_references` 报告 Embed / Edge / Portal 三类真实引用，自引用单独区分。
#[test]
fn find_references_reports_embed_edge_and_portal_hosts() {
    let f = make_three_level_fixture();

    // R 内一条跨画布边落到 B 的 b1（经 eB），使 B 同时被 Embed 和 Edge 引用。
    let created = f
        .api
        .create_cross_canvas_edge(
            selection(route(&f.root, vec![]), node_target("r1")),
            selection(route(&f.root, vec![enter_embed(&f.eb)]), node_target("b1")),
            StarMapEdgeKindDto::Foreshadows,
            Some("伏笔".to_string()),
            None,
        )
        .unwrap();
    f.api.flush_all_starmap_stores().unwrap();

    // B 被 R 的 Embed 与 Edge 引用。
    let refs_b = f.api.find_starmap_references(&f.b).unwrap();
    assert!(
        refs_b
            .iter()
            .any(|r| r.ref_type == "embed" && r.ref_id == f.eb && r.host_starmap_id == f.root),
        "B 应被 R 的 Embed 引用；实际: {refs_b:?}"
    );
    assert!(
        refs_b.iter().any(|r| r.ref_type == "edge"
            && r.ref_id == created.edge.id
            && r.host_starmap_id == f.root),
        "B 应被 R 的跨画布 Edge 引用；实际: {refs_b:?}"
    );

    // C 只被 B 的 Portal 节点引用。
    let refs_c = f.api.find_starmap_references(&f.c).unwrap();
    assert_eq!(refs_c.len(), 1, "C 只有一条 Portal 引用；实际: {refs_c:?}");
    assert_eq!(refs_c[0].ref_type, "portal");
    assert_eq!(refs_c[0].ref_id, f.pc);
    assert_eq!(refs_c[0].host_starmap_id, f.b);

    // 根图 R：R 内 Embed 的 host_path 与跨画布边的起点路径都落在 R 自身，反查
    // 会报告「宿主=R、目标=R」的自引用；这些不计入删除保护
    // （`plan_delete_starmap` 会过滤 host == target 的条目）。
    let refs_root = f.api.find_starmap_references(&f.root).unwrap();
    assert!(
        !refs_root.is_empty(),
        "R 内的 Embed host_path / Edge 起点会形成自引用；实际: {refs_root:?}"
    );
    assert!(
        refs_root
            .iter()
            .all(|r| r.host_starmap_id == f.root && r.target_starmap_id == f.root),
        "R 的反查结果必须全部是自引用；实际: {refs_root:?}"
    );

    // B / C 的反查结果必须指向被查询星图，且宿主是被引用的外图（非自引用）。
    for r in &refs_b {
        assert_eq!(r.target_starmap_id, f.b);
        assert_ne!(r.host_starmap_id, r.target_starmap_id);
    }
    for r in &refs_c {
        assert_eq!(r.target_starmap_id, f.c);
        assert_ne!(r.host_starmap_id, r.target_starmap_id);
    }
}

/// `delete_starmap` 尊重外部引用：被引用时拒绝，解除引用后才真正删除。
#[test]
fn delete_starmap_is_guarded_by_external_references() {
    let f = make_three_level_fixture();

    // C 被 B 的 Portal 引用 → 整图删除被拒绝。
    let error = f
        .api
        .delete_starmap(&f.c)
        .expect_err("被 Portal 引用的 C 不得被删除");
    let message = format!("{error:?}");
    assert!(
        message.contains("referenced by"),
        "必须以外部引用为由拒绝删除；实际: {message}"
    );

    // 清除 Portal 后解除引用，允许删除 C。
    f.api
        .update_starmap_node(&f.b, &f.pc, patch_clear_portal())
        .unwrap();
    // `find_starmap_references` 从磁盘读取，更新需先 flush 才能被引用扫描看到。
    f.api.flush_all_starmap_stores().unwrap();
    assert!(f.api.find_starmap_references(&f.c).unwrap().is_empty());
    assert!(
        f.api.delete_starmap(&f.c).unwrap(),
        "解除引用后应允许删除 C"
    );

    // B 仍被 R 的 Embed 引用 → 删除被拒绝。
    let error = f
        .api
        .delete_starmap(&f.b)
        .expect_err("被 Embed 引用的 B 不得被删除");
    assert!(
        format!("{error:?}").contains("referenced by"),
        "必须以外部引用为由拒绝删除 B"
    );

    // 删除 R 中的 Embed 入口后，B 不再被引用，删除成功。
    assert!(f.api.delete_starmap_embed(&f.root, &f.eb).unwrap());
    f.api.flush_all_starmap_stores().unwrap();
    assert!(
        f.api.find_starmap_references(&f.b).unwrap().is_empty(),
        "删除 Embed 后 B 应无外部引用"
    );
    assert!(f.api.delete_starmap(&f.b).unwrap(), "应允许删除 B");
}
