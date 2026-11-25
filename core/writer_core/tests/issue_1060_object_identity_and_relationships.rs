//! Issue #1060 回归测试 — 对象身份、标签/链接唯一所有者与素材/实例 revision 拆分。
//!
//! 覆盖：
//! 1. 节点编辑按权威记录路由：只移动改实例 revision，不改素材本体 revision；
//!    改共享内容只改本体 revision，不动实例 revision，且画布文件不重复保存内容。
//! 2. 标签唯一关系：同一命名空间 + 同一素材 + 同一标签只有一条有效绑定，
//!    重复 attach 幂等且 binding_id 确定性。
//! 3. 旧数据迁移：节点定义里的旧 `tags` 与 `starmaps/<id>/links/*.json`
//!    迁移到 annotations 仓库后删除旧字段/旧文件，星图图视图从仓库读回链接。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use tempfile::TempDir;

use writer_core::annotations::AnnotationStore;
use writer_core::object_ref::{ObjectRef, ObjectScope};
use writer_core::starmap::semantic::StarMapNodeContent;
use writer_core::starmap::store::StarMapStore;
use writer_core::starmap::types::*;

fn setup() -> (TempDir, String) {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = writer_core::starmap::create_starmap(dir.path(), "星图", "", None).unwrap();
    (dir, meta.starmap_id)
}

fn setup_project() -> (TempDir, String, String) {
    let dir = TempDir::new().unwrap();
    let project_root = dir.path().join("projects").join("p1");
    std::fs::create_dir_all(&project_root).unwrap();
    std::fs::write(project_root.join("project.json"), "{}").unwrap();
    let meta = writer_core::starmap::create_starmap(dir.path(), "星图", "", None).unwrap();
    writer_core::starmap::bind_starmap_to_project(dir.path(), &meta.starmap_id, "p1").unwrap();
    (dir, meta.starmap_id, "p1".to_string())
}

/// 与 `package_storage::bucket_for_id` 一致：hex 高 4 bit，两位十六进制。
fn bucket(id: &str) -> String {
    let first = id.as_bytes().first().copied().unwrap_or(0);
    format!("{:02x}", (first >> 4) & 0x0F)
}

fn definition_path(root: &Path, asset_id: &str) -> std::path::PathBuf {
    root.join("starmaps")
        .join("library")
        .join("nodes")
        .join(bucket(asset_id))
        .join(format!("{asset_id}.json"))
}

fn legacy_link_path(root: &Path, starmap_id: &str, link_id: &str) -> std::path::PathBuf {
    root.join("starmaps")
        .join(starmap_id)
        .join("links")
        .join(bucket(link_id))
        .join(format!("{link_id}.json"))
}

fn node(id: &str, title: &str) -> StarMapNode {
    StarMapNode {
        id: id.to_string(),
        asset_id: id.to_string(),
        title: title.to_string(),
        kind: StarMapNodeKind::Concept,
        payload: None,
        content: StarMapNodeContent::Empty,
        anchors: vec![],
        portal: None,
        position: StarMapPoint::default(),
        style: StarMapNodeStyle::default(),
        provenance: Default::default(),
        created_at: 0,
        updated_at: 0,
        definition_revision: 0,
    }
}

fn seed_node(
    dir: &Path,
    starmap_id: &str,
    node: StarMapNode,
) -> (StarMapStore, StarMapNodeDefinition) {
    let definition = StarMapNodeDefinition::from_node(node.id.clone(), &node);
    writer_core::starmap::library::save_node_definition(dir, &definition).unwrap();
    let mut store = StarMapStore::new(dir, starmap_id);
    store.set_node_asset(&node.id, &definition.asset_id);
    store.add_node(node);
    store.flush().unwrap();
    (store, definition)
}

#[test]
fn move_and_content_edit_use_separate_revisions() {
    let (dir, starmap_id) = setup();
    let (mut store, definition) = seed_node(dir.path(), &starmap_id, node("n1", "旧标题"));
    store
        .update_node_definition(
            "n1",
            &NodeDefinitionPatch {
                title: Some("初始".to_string()),
                ..Default::default()
            },
            Some(definition.revision()),
        )
        .unwrap();
    let before = store.current_node_view("n1").unwrap();

    // 只移动：实例 revision 前进，素材 revision 不动。
    let moved = store
        .move_node_instance(
            "n1",
            StarMapPoint { x: 8.0, y: 9.0 },
            Some(before.updated_at),
        )
        .unwrap();
    assert!(
        moved.updated_at > before.updated_at,
        "移动必须推进实例 revision"
    );
    assert_eq!(
        moved.definition_revision, before.definition_revision,
        "移动不得改素材本体 revision"
    );

    // 改共享内容：素材 revision 前进，实例 revision 不动。
    let after_title = store
        .update_node_definition(
            "n1",
            &NodeDefinitionPatch {
                title: Some("新标题".to_string()),
                ..Default::default()
            },
            Some(moved.definition_revision),
        )
        .unwrap();
    assert!(after_title.definition_revision > moved.definition_revision);
    assert_eq!(
        after_title.updated_at, moved.updated_at,
        "内容修改不得推进实例 revision"
    );

    // 画布节点文件只保存实例事实（位置/assetId），不重复保存共享内容。
    let node_path = dir
        .path()
        .join("starmaps")
        .join(&starmap_id)
        .join("nodes")
        .join(bucket("n1"))
        .join("n1.json");
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(node_path).unwrap()).unwrap();
    assert_eq!(value["assetId"], "n1");
    assert!(
        value.get("content").is_none(),
        "画布实例文件不得保存共享内容，实际: {value}"
    );
}

#[test]
fn tag_binding_is_one_idempotent_relation() {
    let (dir, starmap_id, project_id) = setup_project();
    let (_store, definition) = seed_node(dir.path(), &starmap_id, node("n1", "节点"));
    let project_root = dir.path().join("projects").join(&project_id);
    let store = AnnotationStore::new(&project_root, &project_id);

    let tag = writer_core::annotations::Tag {
        tag_id: "t1".to_string(),
        project_id: project_id.clone(),
        name: "角色".to_string(),
        color: None,
        updated_at_ms: 1,
        deleted_at_ms: None,
    };
    store
        .apply_mutation(writer_core::annotations::AnnotationMutation::UpsertTag(
            tag.clone(),
        ))
        .unwrap();
    let target = ObjectRef::NodeAsset {
        scope: ObjectScope::Project {
            project_id: project_id.clone(),
        },
        object_id: definition.asset_id.clone(),
    };

    let (first, changed) = store.attach_tag(&tag, &target, 10).unwrap();
    assert!(!changed.is_empty(), "首次 attach 必须落盘");
    let (second, changed_again) = store.attach_tag(&tag, &target, 11).unwrap();
    assert!(changed_again.is_empty(), "重复 attach 必须幂等");
    assert_eq!(first.binding_id, second.binding_id, "绑定 ID 必须确定性");
    let bindings = store.list_bindings_for_scope(&target).unwrap();
    assert_eq!(bindings.len(), 1, "同一关系只允许一条有效绑定");

    // detach 保留 tombstone 与同一 ID；再 attach 复活原 ID。
    store.detach_tag(&tag, &target, 12).unwrap();
    assert!(store.list_bindings_for_scope(&target).unwrap().is_empty());
    let (revived, _) = store.attach_tag(&tag, &target, 13).unwrap();
    assert_eq!(revived.binding_id, first.binding_id);
}

#[test]
fn legacy_tags_and_link_files_migrate_to_annotations() {
    let (dir, starmap_id, project_id) = setup_project();
    let node = node("n1", "节点");
    let definition = StarMapNodeDefinition::from_node("asset_1", &node);
    writer_core::starmap::library::save_node_definition(dir.path(), &definition).unwrap();
    // 旧格式：定义文件里带 tags。
    let definition_path = definition_path(dir.path(), "asset_1");
    let mut definition_value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&definition_path).unwrap()).unwrap();
    definition_value["tags"] = serde_json::json!(["角色"]);
    std::fs::write(
        &definition_path,
        serde_json::to_string_pretty(&definition_value).unwrap(),
    )
    .unwrap();

    let mut seeded = node.clone();
    seeded.asset_id = "asset_1".to_string();
    let mut store = StarMapStore::new(dir.path(), &starmap_id);
    store.set_node_asset("n1", "asset_1");
    store.add_node(seeded);
    store.enqueue_save(writer_core::starmap::store::SaveQueueEntry::GraphMeta);
    store.flush().unwrap();

    // 旧链接文件（迁移前唯一存在形式）。
    let link = StarMapLink {
        link_id: "l1".to_string(),
        source: StarMapTargetPath {
            starmap_id: starmap_id.clone(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n1".to_string(),
            },
        },
        target: StarMapTargetPath {
            starmap_id: starmap_id.clone(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n2".to_string(),
            },
        },
        label: Some("跳转".to_string()),
        created_at: 5,
        updated_at: 6,
    };
    let legacy_link_path = legacy_link_path(dir.path(), &starmap_id, "l1");
    std::fs::create_dir_all(legacy_link_path.parent().unwrap()).unwrap();
    std::fs::write(
        &legacy_link_path,
        serde_json::to_string_pretty(&link).unwrap(),
    )
    .unwrap();

    writer_core::starmap::migration::migrate_one_starmap_graph(dir.path(), &starmap_id).unwrap();

    // 旧 tags 字段删除，关系落到项目命名空间。
    let migrated: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&definition_path).unwrap()).unwrap();
    assert!(
        migrated.get("tags").is_none(),
        "迁移成功后必须删除旧 tags 字段"
    );
    let annotation_store =
        AnnotationStore::new(&dir.path().join("projects").join(&project_id), &project_id);
    let tags = annotation_store.list_tags().unwrap();
    let tag = tags
        .iter()
        .find(|tag| tag.name == "角色")
        .expect("旧标签名必须迁移为 Tag 定义");
    let target = ObjectRef::NodeAsset {
        scope: ObjectScope::Project {
            project_id: project_id.clone(),
        },
        object_id: "asset_1".to_string(),
    };
    let bindings = annotation_store.list_bindings_for_target(&target).unwrap();
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].tag_id, tag.tag_id);

    // 旧链接文件删除，图视图从统一仓库读回同一 link_id/label/时间戳。
    assert!(!legacy_link_path.exists(), "迁移成功后旧链接文件必须删除");
    let mut reloaded = StarMapStore::new(dir.path(), &starmap_id);
    let links = reloaded.list_links_with_diagnostics().unwrap();
    assert_eq!(links.items.len(), 1);
    assert_eq!(links.items[0].link_id, "l1");
    assert_eq!(links.items[0].label.as_deref(), Some("跳转"));
    assert_eq!(links.items[0].created_at, 5);
    assert_eq!(links.items[0].updated_at, 6);
}
