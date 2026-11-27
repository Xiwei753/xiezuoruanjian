//! Issue #1062 评论 #7213634487 第 C 项：`references.json` 是派生视图。
//!
//! 画布实例 JSON 已经合法但反向引用索引缺项时，正常加载与批量入口都必须从
//! 实例事实幂等补齐；既有绑定与实例资产身份冲突时保留材料并明确报错，不能
//! 静默覆盖。

use serde_json::json;
use writer_core::starmap::library::{
    ensure_library_index, load_reference_index, save_reference_index,
};
use writer_core::starmap::package_storage::save_node_instance;
use writer_core::starmap::store::meta::GraphMeta;
use writer_core::starmap::store::StarMapStore;
use writer_core::starmap::types::{
    StarMapNodeInstance, StarMapNodeReference, StarMapPoint, StarMapReferenceIndex,
    CURRENT_LIBRARY_SCHEMA_VERSION,
};

fn instance(
    starmap_id: &str,
    instance_id: &str,
    asset_id: &str,
    project_id: Option<&str>,
) -> StarMapNodeInstance {
    StarMapNodeInstance {
        instance_id: instance_id.to_string(),
        asset_id: asset_id.to_string(),
        position: StarMapPoint::default(),
        starmap_id: starmap_id.to_string(),
        project_id: project_id.map(str::to_string),
        created_at: 5,
        updated_at: 6,
    }
}

fn reference(
    starmap_id: &str,
    instance_id: &str,
    asset_id: &str,
    project_id: Option<&str>,
) -> StarMapNodeReference {
    StarMapNodeReference {
        asset_id: asset_id.to_string(),
        starmap_id: starmap_id.to_string(),
        instance_id: instance_id.to_string(),
        project_id: project_id.map(str::to_string),
    }
}

fn write_instance(
    root: &std::path::Path,
    starmap_id: &str,
    instance_id: &str,
    asset_id: &str,
    project_id: Option<&str>,
) -> std::path::PathBuf {
    let rel = save_node_instance(
        root,
        starmap_id,
        &instance(starmap_id, instance_id, asset_id, project_id),
    )
    .unwrap();
    root.join(rel)
}

fn write_references(root: &std::path::Path, nodes: Vec<StarMapNodeReference>) {
    save_reference_index(
        root,
        &StarMapReferenceIndex {
            schema_version: CURRENT_LIBRARY_SCHEMA_VERSION,
            nodes,
            updated_at: 0,
        },
    )
    .unwrap();
}

fn reference_index_path(root: &std::path::Path) -> std::path::PathBuf {
    root.join("starmaps")
        .join("library")
        .join("references.json")
}

/// 让 `ensure_library_index` 认为库已初始化（走批量收敛而不是旧数据迁移）。
fn initialize_library(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("starmaps").join("library").join("entries")).unwrap();
}

fn write_json(path: &std::path::Path, value: &serde_json::Value) {
    std::fs::create_dir_all(path.parent().expect("json path has a parent")).unwrap();
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

#[test]
fn ensure_library_index_backfills_missing_references_in_one_pass() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    initialize_library(root);
    write_instance(root, "sm_1", "n1", "asset_a", Some("p1"));
    write_instance(root, "sm_1", "n2", "asset_b", None);
    write_references(root, vec![reference("sm_1", "n1", "asset_a", Some("p1"))]);

    ensure_library_index(root).expect("missing reference must be backfilled");
    let refs = load_reference_index(root).unwrap();
    assert_eq!(refs.nodes.len(), 2, "{refs:?}");
    let n2 = refs
        .nodes
        .iter()
        .find(|record| record.instance_id == "n2")
        .expect("missing reference must be added");
    assert_eq!(n2.asset_id, "asset_b");

    // 没有缺项时不重复重写全量索引。
    let before = std::fs::read(reference_index_path(root)).unwrap();
    ensure_library_index(root).unwrap();
    assert_eq!(
        std::fs::read(reference_index_path(root)).unwrap(),
        before,
        "no missing entry must not rewrite the full reference index"
    );
}

#[test]
fn ensure_library_index_preserves_conflicting_reference_and_reports_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    initialize_library(root);
    write_instance(root, "sm_1", "n1", "asset_new", Some("p1"));
    write_references(root, vec![reference("sm_1", "n1", "asset_old", Some("p1"))]);
    let before = std::fs::read(reference_index_path(root)).unwrap();

    let error = ensure_library_index(root)
        .expect_err("conflicting reference must not be silently overwritten");
    let message = error.to_string();
    assert!(
        message.contains("starmap reference index conflict"),
        "{message}"
    );
    assert!(message.contains("instance_id=n1"), "{message}");
    assert!(message.contains("instance_asset_id=asset_new"), "{message}");
    assert!(
        message.contains("reference_asset_id=asset_old"),
        "{message}"
    );
    assert_eq!(
        std::fs::read(reference_index_path(root)).unwrap(),
        before,
        "conflicting binding must be preserved byte-for-byte"
    );
}

#[test]
fn ensure_library_index_keeps_reference_for_unreadable_instance_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    initialize_library(root);
    write_instance(root, "sm_1", "n1", "asset_a", Some("p1"));
    let broken_rel =
        save_node_instance(root, "sm_1", &instance("sm_1", "n_broken", "asset_b", None)).unwrap();
    std::fs::write(root.join(&broken_rel), b"{not json").unwrap();
    write_references(
        root,
        vec![
            reference("sm_1", "n1", "asset_a", Some("p1")),
            reference("sm_1", "n_broken", "asset_b", None),
        ],
    );

    // 实例文件还在（只是暂时读不出来/尚未落盘的新实例）：保留既有绑定，
    // 不能因为实例事实覆盖不到就删除派生材料。
    ensure_library_index(root).unwrap();
    let refs = load_reference_index(root).unwrap();
    assert!(
        refs.nodes
            .iter()
            .any(|record| record.instance_id == "n_broken" && record.asset_id == "asset_b"),
        "{refs:?}"
    );
}

#[test]
fn load_full_repairs_missing_reference_without_rewriting_instance() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let starmap_id = "sm_partial_index";
    let graph = GraphMeta {
        schema_version: "4".to_string(),
        starmap_id: starmap_id.to_string(),
        node_ids: vec!["n1".to_string()],
        ..Default::default()
    };
    write_json(
        &root.join(format!("starmaps/{starmap_id}/graph.json")),
        &serde_json::to_value(&graph).unwrap(),
    );
    let node_path = write_instance(root, starmap_id, "n1", "asset_shared", Some("p1"));
    let original = std::fs::read(&node_path).unwrap();
    // 索引文件存在且可读，只缺 n1 这一条。
    write_references(root, Vec::new());

    let mut store = StarMapStore::new(root, starmap_id);
    store.load_full().expect("canonical instance must load");
    assert_eq!(
        std::fs::read(&node_path).unwrap(),
        original,
        "missing reference repair must not rewrite the instance file"
    );
    let refs = load_reference_index(root).unwrap();
    assert_eq!(refs.nodes.len(), 1, "{refs:?}");
    assert_eq!(refs.nodes[0].asset_id, "asset_shared");
    assert_eq!(refs.nodes[0].project_id.as_deref(), Some("p1"));
}

#[test]
fn load_full_restores_reference_after_crash_between_instance_and_index_write() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let starmap_id = "sm_crash";
    let graph = GraphMeta {
        schema_version: "4".to_string(),
        starmap_id: starmap_id.to_string(),
        node_ids: vec!["n1".to_string()],
        ..Default::default()
    };
    write_json(
        &root.join(format!("starmaps/{starmap_id}/graph.json")),
        &serde_json::to_value(&graph).unwrap(),
    );
    // bucket = id 首字节高 4 bit：'n' = 0x6e → "06"。
    let legacy_node = json!({
        "id": "n1",
        "title": "旧节点",
        "kind": "Note",
        "content": {"type": "empty"},
        "anchors": [],
        "portal": null,
        "position": {"x": 7.0, "y": 9.0},
        "style": {},
        "provenance": {},
        "createdAt": 11,
        "updatedAt": 22
    });
    let node_path = root.join(format!("starmaps/{starmap_id}/nodes/06/n1.json"));
    write_json(&node_path, &legacy_node);

    let mut store = StarMapStore::new(root, starmap_id);
    store.load_full().expect("legacy node must migrate");
    let migrated = std::fs::read(&node_path).unwrap();

    // 模拟崩溃：实例文件已落盘，`record_node_reference` 尚未写入。
    std::fs::remove_file(reference_index_path(root)).unwrap();

    let mut reentered = StarMapStore::new(root, starmap_id);
    reentered.load_full().expect("crash re-entry must recover");
    assert_eq!(
        std::fs::read(&node_path).unwrap(),
        migrated,
        "already-canonical instance must not be rewritten just to repair the index"
    );
    let refs = load_reference_index(root).unwrap();
    assert!(
        refs.nodes.iter().any(|record| {
            record.starmap_id == starmap_id && record.instance_id == "n1" && record.asset_id == "n1"
        }),
        "{refs:?}"
    );
}
