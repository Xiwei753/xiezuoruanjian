//! Issue #1060 复核评论（7181967097）回归测试。
//!
//! 覆盖：
//! 1. 删除节点时级联链接墓碑写入 annotations 仓库，删除事实回执与投影一致，
//!    重新加载不会复活链接；
//! 2. 外部（MCP/同步/另一个实例）写入的链接在下一次读取时被版本刷新观察到；
//! 3. 实例 revision 以磁盘持久记录为准：外部改坐标后旧 expected 必须冲突；
//! 4. 混合编辑先校验两个 expected revision，任一过期都不产生半提交。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use tempfile::TempDir;

use writer_core::annotations::{AnnotationStore, LinkOrigin, LinkTarget, ObjectLink};
use writer_core::facade::WriterCore;
use writer_core::object_ref::ObjectRef;
use writer_core::starmap::semantic::StarMapNodeContent;
use writer_core::starmap::store::StarMapStore;
use writer_core::starmap::types::*;

fn bucket(id: &str) -> String {
    let first = id.as_bytes().first().copied().unwrap_or(0);
    format!("{:02x}", (first >> 4) & 0x0F)
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

fn node_patch(title: Option<&str>, position: Option<StarMapPoint>) -> StarMapNodePatch {
    StarMapNodePatch {
        title: title.map(str::to_string),
        kind: None,
        payload: None,
        content: None,
        anchors: None,
        portal: None,
        position,
        style: None,
        provenance: None,
    }
}

fn link_patch(label: &str) -> StarMapLinkPatch {
    StarMapLinkPatch {
        source: None,
        target: None,
        label: Some(Some(label.to_string())),
    }
}

fn canvas_path(starmap_id: &str, node_id: &str) -> StarMapTargetPath {
    StarMapTargetPath {
        starmap_id: starmap_id.to_string(),
        segments: Vec::new(),
        target: StarMapTargetDetail::Node {
            node_id: node_id.to_string(),
        },
    }
}

fn link(starmap_id: &str, link_id: &str, from: &str, to: &str) -> StarMapLink {
    StarMapLink {
        link_id: link_id.to_string(),
        source: canvas_path(starmap_id, from),
        target: canvas_path(starmap_id, to),
        label: Some("导航".to_string()),
        created_at: 1,
        updated_at: 1,
    }
}

fn setup_project() -> (TempDir, WriterCore, String, String) {
    let dir = TempDir::new().unwrap();
    let project_root = dir.path().join("projects").join("p1");
    std::fs::create_dir_all(&project_root).unwrap();
    std::fs::write(project_root.join("project.json"), "{}").unwrap();
    let core = WriterCore::new(dir.path(), dir.path().join("projects"));
    let meta = core.create_starmap("星图", "", None).unwrap();
    writer_core::starmap::bind_starmap_to_project(dir.path(), &meta.starmap_id, "p1").unwrap();
    (dir, core, meta.starmap_id, "p1".to_string())
}

fn instance_path(root: &Path, starmap_id: &str, node_id: &str) -> PathBuf {
    root.join("starmaps")
        .join(starmap_id)
        .join("nodes")
        .join(bucket(node_id))
        .join(format!("{node_id}.json"))
}

#[test]
fn deleting_node_tombstones_cascaded_links_in_annotations() {
    let (dir, core, starmap_id, project_id) = setup_project();
    core.add_starmap_node(&starmap_id, node("n1", "起点"), 0.0, 0.0)
        .unwrap();
    core.add_starmap_node(&starmap_id, node("n2", "终点"), 10.0, 0.0)
        .unwrap();
    core.add_starmap_link(&starmap_id, link(&starmap_id, "l1", "n1", "n2"))
        .unwrap();

    let deleted = core.delete_starmap_node(&starmap_id, "n1").unwrap();
    assert_eq!(deleted.nodes, vec!["n1".to_string()]);
    assert_eq!(
        deleted.links,
        vec!["l1".to_string()],
        "级联删除必须把链接 ID 作为删除事实返回"
    );

    // 仓库记录保留为 tombstone（删除事实不丢），不是物理删除。
    let link_path = dir
        .path()
        .join("projects")
        .join(&project_id)
        .join("annotations")
        .join("links")
        .join("l1.json");
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&link_path).unwrap()).unwrap();
    assert!(
        value.get("deletedAtMs").is_some_and(|v| !v.is_null()),
        "级联链接必须写 annotations tombstone，实际: {value}"
    );

    // 投影与仓库一致，重新加载不会复活链接。
    let graph = core.get_starmap_graph(&starmap_id).unwrap();
    assert!(graph.links.is_empty(), "删除后图视图不得包含链接");
    let mut reloaded = StarMapStore::new(dir.path(), &starmap_id);
    reloaded.ensure_fully_loaded().unwrap();
    assert_eq!(reloaded.link_count(), 0, "重新加载不得复活已删链接");
}

#[test]
fn external_link_write_is_observed_by_projection_refresh() {
    let (dir, core, starmap_id, project_id) = setup_project();
    core.add_starmap_node(&starmap_id, node("n1", "起点"), 0.0, 0.0)
        .unwrap();
    core.add_starmap_node(&starmap_id, node("n2", "终点"), 10.0, 0.0)
        .unwrap();
    // 先加载一次，让画布持有链接投影与仓库版本。
    core.get_starmap_graph(&starmap_id).unwrap();

    // 模拟另一个进程（MCP/同步）直接写 annotations 仓库。
    let project_root = dir.path().join("projects").join(&project_id);
    let annotations = AnnotationStore::new(&project_root, &project_id);
    annotations
        .attach_link(ObjectLink {
            link_id: "external".to_string(),
            source: ObjectRef::NodeInstance {
                starmap_id: starmap_id.clone(),
                object_id: "n1".to_string(),
            },
            target: LinkTarget::CanvasNavigation(canvas_path(&starmap_id, "n2")),
            origin: LinkOrigin::Manual,
            label: Some("外部写入".to_string()),
            created_at_ms: 1_000,
            updated_at_ms: 1_000,
            deleted_at_ms: None,
        })
        .unwrap();

    let graph = core.get_starmap_graph(&starmap_id).unwrap();
    let observed = graph
        .links
        .iter()
        .find(|link| link.link_id == "external")
        .expect("外部写入的链接必须被下一次读取观察到");
    assert_eq!(observed.label.as_deref(), Some("外部写入"));

    // 旧投影被替换后，编辑/删除也基于同一事实。
    core.update_starmap_link(&starmap_id, "external", link_patch("已编辑"))
        .unwrap();
    let graph = core.get_starmap_graph(&starmap_id).unwrap();
    assert_eq!(
        graph
            .links
            .iter()
            .find(|link| link.link_id == "external")
            .and_then(|link| link.label.as_deref()),
        Some("已编辑")
    );
}

#[test]
fn instance_revision_conflicts_with_newer_disk_record() {
    let (dir, core, starmap_id, _project_id) = setup_project();
    core.add_starmap_node(&starmap_id, node("n1", "节点"), 1.0, 2.0)
        .unwrap();
    core.flush_starmap_store(&starmap_id).unwrap();
    let before = core
        .get_starmap_graph(&starmap_id)
        .unwrap()
        .nodes
        .into_iter()
        .find(|node| node.id == "n1")
        .unwrap();

    // 另一个进程把坐标写进实例文件并推进 revision。
    let path = instance_path(dir.path(), &starmap_id, "n1");
    let mut value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    value["position"] = serde_json::json!({ "x": 40.0, "y": 50.0 });
    value["updatedAt"] = serde_json::json!(before.updated_at + 5);
    std::fs::write(&path, serde_json::to_string_pretty(&value).unwrap()).unwrap();

    let err = core
        .update_starmap_node_with_revisions(
            &starmap_id,
            "n1",
            node_patch(None, Some(StarMapPoint { x: 7.0, y: 8.0 })),
            None,
            Some(before.updated_at),
        )
        .unwrap_err();
    assert!(
        err.to_string().contains("instance revision conflict"),
        "旧 expected 必须与磁盘持久 revision 冲突，实际: {err}"
    );

    // 用磁盘 revision 重试成功，且以磁盘记录为基准确认。
    let moved = core
        .update_starmap_node_with_revisions(
            &starmap_id,
            "n1",
            node_patch(None, Some(StarMapPoint { x: 7.0, y: 8.0 })),
            None,
            Some(before.updated_at + 5),
        )
        .unwrap();
    assert_eq!(moved.position, StarMapPoint { x: 7.0, y: 8.0 });
}

#[test]
fn mixed_edit_validates_both_revisions_before_writing() {
    let (_dir, core, starmap_id, _project_id) = setup_project();
    core.add_starmap_node(&starmap_id, node("n1", "旧标题"), 0.0, 0.0)
        .unwrap();
    let initial = core
        .get_starmap_graph(&starmap_id)
        .unwrap()
        .nodes
        .into_iter()
        .find(|node| node.id == "n1")
        .unwrap();

    // 先把定义推进到新 revision，initial.definition_revision 变成过期值。
    let updated = core
        .update_starmap_node_with_revisions(
            &starmap_id,
            "n1",
            node_patch(Some("中间标题"), None),
            Some(initial.definition_revision),
            None,
        )
        .unwrap();
    assert!(updated.definition_revision > initial.definition_revision);

    // 混合编辑：定义 expected 过期 + 实例 expected 有效 → 整体失败，
    // 不得先写标题再报实例/定义冲突。
    let err = core
        .update_starmap_node_with_revisions(
            &starmap_id,
            "n1",
            node_patch(Some("不应落盘"), Some(StarMapPoint { x: 3.0, y: 4.0 })),
            Some(initial.definition_revision),
            Some(updated.updated_at),
        )
        .unwrap_err();
    assert!(
        err.to_string().contains("definition revision conflict"),
        "定义 revision 过期必须报错，实际: {err}"
    );

    let after = core
        .get_starmap_graph(&starmap_id)
        .unwrap()
        .nodes
        .into_iter()
        .find(|node| node.id == "n1")
        .unwrap();
    assert_eq!(after.title, "中间标题", "失败编辑不得留下半提交");
    assert_eq!(after.position, StarMapPoint { x: 0.0, y: 0.0 });
    assert_eq!(after.updated_at, updated.updated_at);

    // 用当前 revision 重试成功。
    let final_node = core
        .update_starmap_node_with_revisions(
            &starmap_id,
            "n1",
            node_patch(Some("最终标题"), Some(StarMapPoint { x: 3.0, y: 4.0 })),
            Some(after.definition_revision),
            Some(after.updated_at),
        )
        .unwrap();
    assert_eq!(final_node.title, "最终标题");
    assert_eq!(final_node.position, StarMapPoint { x: 3.0, y: 4.0 });
}

#[test]
fn library_entries_are_independent_files_and_tombstones_hide_them() {
    let (dir, core, starmap_id, project_id) = setup_project();
    let created = core
        .add_starmap_node(&starmap_id, node("n1", "素材"), 0.0, 0.0)
        .unwrap();
    let root = dir.path();
    let entries_dir = root.join("starmaps").join("library").join("entries");
    let files: Vec<_> = std::fs::read_dir(&entries_dir)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .collect();
    // 画布本身也会登记一条 StarMap 归属；节点素材必须另有独立 entry 文件。
    let node_entries: Vec<_> = files
        .iter()
        .filter_map(|file| {
            let value: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(file.path()).unwrap()).ok()?;
            (value["target"]["assetId"] == created.asset_id).then(|| file.path())
        })
        .collect();
    assert_eq!(node_entries.len(), 1, "新素材必须有独立 entry 对象文件");
    let entry_path = node_entries[0].clone();
    let entry_value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&entry_path).unwrap()).unwrap();
    assert_eq!(entry_value["target"]["assetId"], created.asset_id);

    // index.json 只是聚合缓存，包含同一条目。
    let index_value: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("starmaps/library/index.json")).unwrap(),
    )
    .unwrap();
    assert!(index_value["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["entryId"] == entry_value["entryId"]));

    // 移除归属写 tombstone，列表与聚合缓存都不再返回它。
    writer_core::starmap::library::remove_entries_for_asset(
        root,
        &StarMapAssetRef::node(&created.asset_id),
    )
    .unwrap();
    let tombstone: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&entry_path).unwrap()).unwrap();
    assert!(
        tombstone["deletedAt"].is_number(),
        "移除归属必须保留 tombstone，实际: {tombstone}"
    );
    let (entries, _) = writer_core::starmap::library::list_library(
        root,
        &StarMapAssetScope::Project {
            project_id: project_id.clone(),
        },
    )
    .unwrap();
    assert!(entries.is_empty(), "tombstone 条目不得出现在库列表");
}

#[test]
fn legacy_library_index_migrates_to_independent_entry_files() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let legacy = serde_json::json!({
        "schemaVersion": 2,
        "entries": [{
            "entryId": "le_old",
            "scope": { "kind": "global" },
            "target": { "kind": "node", "assetId": "n_old" },
            "createdAt": 1,
            "updatedAt": 2
        }],
        "folders": [{
            "folderId": "lf_old",
            "scope": { "kind": "global" },
            "name": "旧目录",
            "createdAt": 1,
            "updatedAt": 2
        }],
        "updatedAt": 2
    });
    std::fs::create_dir_all(root.join("starmaps/library")).unwrap();
    std::fs::write(
        root.join("starmaps/library/index.json"),
        serde_json::to_string_pretty(&legacy).unwrap(),
    )
    .unwrap();

    let index = writer_core::starmap::library::ensure_library_index(root).unwrap();
    assert_eq!(index.entries.len(), 1);
    assert_eq!(index.folders.len(), 1);
    assert!(root.join("starmaps/library/entries/le_old.json").is_file());
    assert!(root.join("starmaps/library/folders/lf_old.json").is_file());

    // 幂等：再次读取不产生重复对象，保留原 ID。
    let again = writer_core::starmap::library::ensure_library_index(root).unwrap();
    assert_eq!(again.entries.len(), 1);
    assert_eq!(again.entries[0].entry_id, "le_old");
}
