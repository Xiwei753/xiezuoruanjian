use super::*;
use crate::annotations::store::AnnotationStore;
use crate::object_ref::ObjectScope;
use std::fs;

fn project_root(app_data_root: &Path, project_id: &str) -> PathBuf {
    app_data_root.join("projects").join(project_id)
}

fn write_downloaded_project(app_data_root: &Path, project_id: &str) {
    let root = project_root(app_data_root, project_id);
    fs::create_dir_all(&root).expect("create project dir");
    fs::write(root.join("project.json"), "{}").expect("write project marker");
}

fn node_target(starmap_id: &str, node_id: &str) -> StarMapTargetPath {
    StarMapTargetPath {
        starmap_id: starmap_id.to_string(),
        segments: Vec::new(),
        target: StarMapTargetDetail::Node {
            node_id: node_id.to_string(),
        },
    }
}

fn chapter_target(chapter_id: &str) -> StarMapTargetPath {
    StarMapTargetPath {
        starmap_id: "sm_1".to_string(),
        segments: Vec::new(),
        target: StarMapTargetDetail::ChapterRange {
            project_id: None,
            volume_id: None,
            chapter_id: chapter_id.to_string(),
            range_start: None,
            range_end: None,
        },
    }
}

#[test]
fn project_tag_migration_is_idempotent() {
    let temp = tempfile::tempdir().expect("tempdir");
    let app_data_root = temp.path();
    write_downloaded_project(app_data_root, "p1");
    let legacy = vec![(
        "asset_1".to_string(),
        Some("p1".to_string()),
        vec!["角色".to_string(), "主角".to_string()],
    )];

    let first = migrate_starmap_node_tags(app_data_root, &legacy).expect("first migration");
    assert_eq!(first.migrated_tags, 2, "两个标签关系都应迁移");
    assert!(
        first.conflicts.is_empty(),
        "不应有冲突: {:?}",
        first.conflicts
    );
    assert_eq!(first.changed_paths.len(), 4, "2 个标签定义 + 2 条绑定");

    let store = AnnotationStore::new(&project_root(app_data_root, "p1"), "p1");
    assert_eq!(store.list_tags().expect("tags").len(), 2);
    let bindings = store.list_bindings().expect("bindings");
    assert_eq!(bindings.len(), 2);
    let first_ids = bindings
        .iter()
        .map(|binding| binding.binding_id.clone())
        .collect::<Vec<_>>();
    assert!(first_ids.iter().all(|id| id.starts_with('b')));

    let second = migrate_starmap_node_tags(app_data_root, &legacy).expect("second migration");
    assert_eq!(second.migrated_tags, 0, "重复迁移不新增关系");
    assert!(second.changed_paths.is_empty());
    let second_bindings = store.list_bindings().expect("bindings");
    assert_eq!(second_bindings.len(), 2, "不产生重复绑定");
    assert_eq!(
        second_bindings
            .iter()
            .map(|binding| binding.binding_id.clone())
            .collect::<Vec<_>>(),
        first_ids,
        "确定性 binding ID 保持不变"
    );
}

#[test]
fn global_asset_tags_use_the_global_store() {
    let temp = tempfile::tempdir().expect("tempdir");
    let app_data_root = temp.path();
    write_downloaded_project(app_data_root, "p1");
    let legacy = vec![("asset_global".to_string(), None, vec!["共享".to_string()])];

    let report = migrate_starmap_node_tags(app_data_root, &legacy).expect("migration");
    assert_eq!(report.migrated_tags, 1);
    assert!(report.conflicts.is_empty());

    let global = AnnotationStore::global(app_data_root);
    let tags = global.list_tags().expect("global tags");
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].project_id, "", "全局标签命名空间为空");
    let bindings = global.list_bindings().expect("global bindings");
    assert_eq!(bindings.len(), 1);
    assert_eq!(
        bindings[0].target,
        ObjectRef::NodeAsset {
            scope: ObjectScope::Global,
            object_id: "asset_global".to_string()
        }
    );
    let project_store = AnnotationStore::new(&project_root(app_data_root, "p1"), "p1");
    assert!(
        project_store.list_tags().expect("project tags").is_empty(),
        "全局素材标签不进入作品标签目录"
    );
}

#[test]
fn legacy_link_migration_preserves_identity_and_timestamps() {
    let temp = tempfile::tempdir().expect("tempdir");
    let app_data_root = temp.path();
    let legacy_path = app_data_root.join("starmaps/sm_1/links/link_1.json");
    fs::create_dir_all(legacy_path.parent().expect("parent")).expect("links dir");
    fs::write(&legacy_path, "{}").expect("legacy link file");
    let target = chapter_target("c1");
    let legacy = vec![LegacyStarMapLink {
        link_id: "link_1".to_string(),
        source: node_target("sm_1", "n1"),
        target: target.clone(),
        label: Some("去第一章".to_string()),
        created_at: 100,
        updated_at: 200,
    }];

    let report =
        migrate_legacy_links(app_data_root, "sm_1", None, &legacy).expect("link migration");
    assert_eq!(report.migrated_links, 1);
    assert!(report.conflicts.is_empty());

    let store = AnnotationStore::global(app_data_root);
    let link = store
        .get_link("link_1")
        .expect("read link")
        .expect("link exists");
    assert_eq!(
        link.source,
        ObjectRef::NodeInstance {
            starmap_id: "sm_1".to_string(),
            object_id: "n1".to_string()
        }
    );
    assert_eq!(link.target, LinkTarget::CanvasNavigation(target));
    assert_eq!(link.label.as_deref(), Some("去第一章"));
    assert_eq!(link.created_at_ms, 100_000);
    assert_eq!(link.updated_at_ms, 200_000);

    let second =
        migrate_legacy_links(app_data_root, "sm_1", None, &legacy).expect("repeat migration");
    assert_eq!(second.migrated_links, 0);
    assert_eq!(store.list_links().expect("links").len(), 1);

    let removable = finish_link_migration(
        app_data_root,
        &store,
        &["link_1".to_string()],
        &[PathBuf::from("starmaps/sm_1/links/link_1.json")],
    )
    .expect("finish migration");
    assert_eq!(removable, vec!["starmaps/sm_1/links/link_1.json"]);

    store
        .apply_mutation(AnnotationMutation::Delete {
            kind: crate::annotations::AnnotationKind::Link,
            scope: ObjectScope::Global,
            object_id: "link_1".to_string(),
            deleted_at_ms: 300_000,
        })
        .expect("tombstone link");
    assert!(
        finish_link_migration(
            app_data_root,
            &store,
            &["link_1".to_string()],
            &[PathBuf::from("starmaps/sm_1/links/link_1.json")],
        )
        .is_err(),
        "已删除的链接不能给出删除旧文件的许可"
    );
}

#[test]
fn multiple_scope_candidates_are_conflicts() {
    let temp = tempfile::tempdir().expect("tempdir");
    let app_data_root = temp.path();
    write_downloaded_project(app_data_root, "p1");
    let library_index = serde_json::json!({
        "schemaVersion": crate::starmap::types::CURRENT_LIBRARY_SCHEMA_VERSION,
        "entries": [{
            "entryId": "le_1",
            "scope": {"kind": "global"},
            "target": {"kind": "node", "assetId": "asset_ambiguous"},
            "createdAt": 1,
            "updatedAt": 1
        }],
        "folders": [],
        "updatedAt": 1
    });
    let index_path = app_data_root.join("starmaps/library/index.json");
    fs::create_dir_all(index_path.parent().expect("parent")).expect("library dir");
    fs::write(
        &index_path,
        serde_json::to_string_pretty(&library_index).expect("encode index"),
    )
    .expect("write index");

    let legacy = vec![(
        "asset_ambiguous".to_string(),
        Some("p1".to_string()),
        vec!["线索".to_string()],
    )];
    let report = migrate_starmap_node_tags(app_data_root, &legacy).expect("migration");
    assert_eq!(report.migrated_tags, 0);
    assert_eq!(report.conflicts.len(), 1);
    assert_eq!(report.conflicts[0].kind, "scopeAmbiguous");
    assert!(
        AnnotationStore::global(app_data_root)
            .list_bindings()
            .expect("bindings")
            .is_empty(),
        "有冲突时不写任何作用域"
    );
    assert!(
        AnnotationStore::new(&project_root(app_data_root, "p1"), "p1")
            .list_bindings()
            .expect("bindings")
            .is_empty()
    );
}
