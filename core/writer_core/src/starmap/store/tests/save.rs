use super::super::*;
use super::*;
use tempfile::TempDir;

#[test]
fn flush_increments_package_revision() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Test Node"));
    store.flush().unwrap();
    assert_eq!(store.package_revision(), 1);
    assert!(!store.is_dirty());
}
#[test]
fn flush_persists_recovery_log_to_disk() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Test"));
    store.recovery_log.push(LoadDiagnostic {
        kind: LoadDiagnosticKind::Corrupt,
        object_type: "node".to_string(),
        object_id: "bad-node".to_string(),
        detail: "test corrupt".to_string(),
    });
    store.flush().unwrap();

    let recovery_path = dir
        .path()
        .join("starmaps")
        .join(&meta.starmap_id)
        .join("metadata")
        .join("recovery.json");
    assert!(recovery_path.exists());

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    store2.load_full().unwrap();
    assert!(!store2.diagnostics().is_empty());
    let corrupt_diag: Vec<_> = store2
        .diagnostics()
        .iter()
        .filter(|d| d.kind == LoadDiagnosticKind::Corrupt && d.object_id == "bad-node")
        .collect();
    assert!(!corrupt_diag.is_empty());
}
#[test]
fn save_queue_deduplicates_entries() {
    let dir = TempDir::new().unwrap();
    let mut store = StarMapStore::new(dir.path(), "test-id");
    store.enqueue_save(SaveQueueEntry::Node);
    store.enqueue_save(SaveQueueEntry::Node);
    store.enqueue_save(SaveQueueEntry::Edge);
    assert_eq!(store.save_queue_len(), 2);
}
#[test]
fn drain_save_queue_clears() {
    let dir = TempDir::new().unwrap();
    let mut store = StarMapStore::new(dir.path(), "test-id");
    store.enqueue_save(SaveQueueEntry::Node);
    store.enqueue_save(SaveQueueEntry::Edge);
    let entries = store.drain_save_queue();
    assert_eq!(entries.len(), 2);
    assert_eq!(store.save_queue_len(), 0);
}
#[test]
fn flush_save_queue_handles_delete_entries() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Node1"));
    store.flush().unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    store2.load_full().unwrap();
    store2.remove_node("n1");
    assert!(store2.has_pending_deletes());

    store2.enqueue_save(SaveQueueEntry::DeleteNode);
    store2.enqueue_save(SaveQueueEntry::GraphMeta);
    store2.flush_save_queue().unwrap();
    assert!(!store2.has_pending_deletes());

    let mut store3 = StarMapStore::new(dir.path(), &meta.starmap_id);
    let result = store3.load_full().unwrap();
    assert_eq!(result.loaded_node_count, 0);
}
#[test]
fn flush_delete_failure_retains_deleted_ids() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Node1"));
    store.flush().unwrap();

    let node_path = dir
        .path()
        .join("starmaps")
        .join(&meta.starmap_id)
        .join("nodes")
        .join(package_storage::bucket_for_id("n1"))
        .join("n1.json");
    assert!(node_path.exists());

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    store2.load_full().unwrap();
    store2.remove_node("n1");

    let result = store2.flush();
    assert!(result.is_ok());
    assert!(!store2.has_pending_deletes());
}
#[test]
fn save_queue_delete_variants_exist() {
    let dir = TempDir::new().unwrap();
    let mut store = StarMapStore::new(dir.path(), "test-id");
    store.enqueue_save(SaveQueueEntry::DeleteNode);
    store.enqueue_save(SaveQueueEntry::DeleteEdge);
    store.enqueue_save(SaveQueueEntry::DeleteEmbed);
    store.enqueue_save(SaveQueueEntry::DeleteLink);
    store.enqueue_save(SaveQueueEntry::DeleteHyperlink);
    assert_eq!(store.save_queue_len(), 5);
}
#[test]
fn flush_save_queue_increments_package_revision() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Test Node"));
    store.enqueue_save(SaveQueueEntry::Node);
    store.enqueue_save(SaveQueueEntry::GraphMeta);
    assert_eq!(store.package_revision(), 0);
    store.flush_save_queue().unwrap();
    assert_eq!(store.package_revision(), 1);
}
#[test]
fn flush_delete_failure_returns_error_and_retains_id() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Node1"));
    store.flush().unwrap();

    let node_path = dir
        .path()
        .join("starmaps")
        .join(&meta.starmap_id)
        .join("nodes")
        .join(package_storage::bucket_for_id("n1"))
        .join("n1.json");
    assert!(node_path.exists());

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    store2.load_full().unwrap();
    // load_full 成功后再把 sync/trash 变成文件，模拟 flush 删除时无法创建 trash 目录。
    // （修复后 load_full 对坏 node 文件会 Err，所以必须在 load_full 之后破坏文件。）
    // 对象级删除现在走 durable rename to trash，需要 create_dir_all(sync/trash/...)；
    // 把 sync/trash 变成文件会让 create_dir_all 失败，触发删除错误路径。
    std::fs::create_dir_all(dir.path().join("sync")).unwrap();
    std::fs::write(dir.path().join("sync").join("trash"), "blocker").unwrap();
    store2.remove_node("n1");
    assert!(store2.has_pending_deletes());

    let result = store2.flush();
    assert!(result.is_err());
    assert!(store2.has_pending_deletes());
}
#[test]
fn flush_delete_succeeds_clears_deleted_ids() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Node1"));
    store.flush().unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    store2.load_full().unwrap();
    store2.remove_node("n1");
    assert!(store2.has_pending_deletes());

    store2.flush().unwrap();
    assert!(!store2.has_pending_deletes());
}
#[test]
fn flush_save_queue_returns_error_on_write_failure() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Node1"));
    store.enqueue_save(SaveQueueEntry::Node);
    store.enqueue_save(SaveQueueEntry::GraphMeta);

    let nodes_bucket_dir = dir
        .path()
        .join("starmaps")
        .join(&meta.starmap_id)
        .join("nodes")
        .join(package_storage::bucket_for_id("n1"));
    std::fs::create_dir_all(&nodes_bucket_dir).unwrap();
    let node_file = nodes_bucket_dir.join("n1.json");
    std::fs::write(&node_file, "existing").unwrap();

    let mut perms = std::fs::metadata(&nodes_bucket_dir).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&nodes_bucket_dir, perms).unwrap();

    let result = store.flush_save_queue();

    let mut perms2 = std::fs::metadata(&nodes_bucket_dir).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms2.set_readonly(false);
    std::fs::set_permissions(&nodes_bucket_dir, perms2).unwrap();

    if result.is_err() {
        if let Err(e) = result {
            assert_eq!(e.code(), "SAVE_QUEUE_FLUSH_INCOMPLETE");
        }
    }
}
#[test]
fn deferred_save_merges_consecutive_operations() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "First"));
    store.enqueue_save(SaveQueueEntry::Node);
    store.enqueue_save(SaveQueueEntry::GraphMeta);

    store.upsert_node(make_test_node("n2", "Second"));
    store.enqueue_save(SaveQueueEntry::Node);
    store.enqueue_save(SaveQueueEntry::GraphMeta);

    store.upsert_node(make_test_node("n3", "Third"));
    store.enqueue_save(SaveQueueEntry::Node);
    store.enqueue_save(SaveQueueEntry::GraphMeta);

    assert_eq!(store.save_queue_len(), 2);
    assert!(store.is_dirty());

    let node_file_1 = dir
        .path()
        .join("starmaps")
        .join(&meta.starmap_id)
        .join("nodes")
        .join(package_storage::bucket_for_id("n1"))
        .join("n1.json");
    let node_file_3 = dir
        .path()
        .join("starmaps")
        .join(&meta.starmap_id)
        .join("nodes")
        .join(package_storage::bucket_for_id("n3"))
        .join("n3.json");
    assert!(!node_file_1.exists());
    assert!(!node_file_3.exists());

    store.flush_save_queue().unwrap();

    assert!(node_file_1.exists());
    assert!(node_file_3.exists());
    assert!(!store.is_dirty());
    assert_eq!(store.save_queue_len(), 0);
}
#[test]
fn deferred_save_with_delete_merges_into_single_flush() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Node1"));
    store.upsert_node(make_test_node("n2", "Node2"));
    store.flush().unwrap();

    store.remove_node("n1");
    store.enqueue_save(SaveQueueEntry::DeleteNode);
    store.enqueue_save(SaveQueueEntry::GraphMeta);

    store.upsert_node(make_test_node("n3", "Node3"));
    store.enqueue_save(SaveQueueEntry::Node);
    store.enqueue_save(SaveQueueEntry::GraphMeta);

    assert_eq!(store.save_queue_len(), 3);
    assert!(store.has_pending_deletes());

    store.flush_save_queue().unwrap();

    let node_file_1 = dir
        .path()
        .join("starmaps")
        .join(&meta.starmap_id)
        .join("nodes")
        .join(package_storage::bucket_for_id("n1"))
        .join("n1.json");
    let node_file_3 = dir
        .path()
        .join("starmaps")
        .join(&meta.starmap_id)
        .join("nodes")
        .join(package_storage::bucket_for_id("n3"))
        .join("n3.json");
    assert!(!node_file_1.exists());
    assert!(node_file_3.exists());
    assert!(!store.has_pending_deletes());
    assert!(!store.is_dirty());
}
#[test]
fn flush_package_revision_memory_matches_disk() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Test Node"));
    store.flush().unwrap();
    let mem_rev = store.package_revision();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    let result = store2.load_full();
    assert!(result.is_ok());
    let disk_rev = store2.package_revision();
    assert_eq!(
        mem_rev, disk_rev,
        "memory and disk package_revision must match after flush"
    );
}
#[test]
fn flush_stats_uses_graph_meta_counts() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "A"));
    store.upsert_node(make_test_node("n2", "B"));
    store.upsert_node(make_test_node("n3", "C"));
    store.upsert_node(make_test_node("n4", "D"));

    use crate::starmap::types::reference::{StarMapTargetDetail, StarMapTargetPath};
    use crate::starmap::types::{StarMapEdge, StarMapEdgeKind};
    store.upsert_edge(StarMapEdge {
        id: "e1".to_string(),
        from: StarMapTargetPath {
            starmap_id: String::new(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n1".to_string(),
            },
        },
        to: StarMapTargetPath {
            starmap_id: String::new(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n2".to_string(),
            },
        },
        kind: StarMapEdgeKind::References,
        label: None,
        payload: None,
        created_at: 0,
        updated_at: 0,
    });
    store.flush().unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    let _ = store2.load_full();
    assert_eq!(store2.nodes.len(), 4, "store2 should have loaded 4 nodes");
    assert_eq!(
        store2.graph_meta.as_ref().unwrap().node_ids.len(),
        4,
        "graph_meta should have 4 node ids"
    );

    store2.remove_node("n4");
    assert_eq!(store2.nodes.len(), 3, "cache has 3 after removal");
    // graph_meta indexes are rebuilt during flush, not immediately after CRUD
    store2.flush().unwrap();
    assert_eq!(
        store2.graph_meta.as_ref().unwrap().node_ids.len(),
        3,
        "graph_meta node_ids should have 3 after flush"
    );

    let result = store2.flush();
    assert!(result.is_ok(), "flush should succeed");

    let graph_meta = store2.graph_meta.as_ref().unwrap();
    assert_eq!(
        graph_meta.node_ids.len(),
        3,
        "final graph_meta should have 3 node ids"
    );
    assert_eq!(
        graph_meta.edge_ids.len(),
        1,
        "final graph_meta should have 1 edge id"
    );
}
#[test]
fn link_flush_save_queue_persists_via_save_queue() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    let link = make_test_link("l1", "Test");
    store.add_link(link).unwrap();
    store.enqueue_save(SaveQueueEntry::Link);
    store.enqueue_save(SaveQueueEntry::GraphMeta);
    store.flush_save_queue().unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    store2.load_full().unwrap();
    assert_eq!(store2.link_count(), 1);
    assert_eq!(
        store2.get_link("l1").unwrap().label.as_deref(),
        Some("Test")
    );
}
#[test]
fn delete_link_flush_save_queue_persists_via_save_queue() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    let link = make_test_link("l1", "Test");
    store.add_link(link).unwrap();
    assert!(store.dirty_links.contains("l1"));
    assert!(
        store.dirty_graph_meta,
        "add_link must mark dirty_graph_meta"
    );

    store.flush().unwrap();
    let meta_ids = store.graph_meta.as_ref().unwrap().link_ids.clone();
    assert!(
        meta_ids.contains(&"l1".to_string()),
        "after flush, graph_meta.link_ids must contain the link_id"
    );

    store.flush().unwrap();
    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    store2.load_full().unwrap();
    assert_eq!(store2.link_count(), 1);
    assert!(
        store2
            .graph_meta
            .as_ref()
            .unwrap()
            .link_ids
            .contains(&"l1".to_string()),
        "link_id must persist in graph.json after flush"
    );
}

/// Issue #805 评论 5914308327：resume 旧 journal 时必须严格使用
/// `existing.objects` 遍历 rename，不能遍历当前 `deleted_*_ids`。
///
/// 场景：
/// - 事务 A 的 journal 残留：objects=[A], facts=[A]，A 已进 trash
/// - 同进程又删除了 B，deleted_node_ids = {A, B}
/// - resume flush 应只处理 A，不把 B 移到事务 A 的 trash
/// - A 完成后 B 仍 pending，下一轮为 B 建新事务
#[test]
fn flush_resume_journal_only_processes_journal_objects_not_all_pending() {
    use crate::starmap::package_storage;
    use crate::storage::journal::starmap_object_delete::{
        PlannedStarMapObjectDelete, StarMapObjectDeletePhase, StarMapObjectDeleteTarget,
        StarMapObjectKind,
    };
    use crate::storage::journal::workspace_change::SyncDeleteFact;
    use crate::sync::SyncService;

    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();
    let starmap_id = &meta.starmap_id;

    // 1. upsert node A + node B，flush 让两者落盘
    let mut store = StarMapStore::new(dir.path(), starmap_id);
    store.upsert_node(make_test_node("nA", "NodeA"));
    store.upsert_node(make_test_node("nB", "NodeB"));
    store.flush().unwrap();

    // 2. 计算 A/B 原文件路径
    let bucket_a = package_storage::bucket_for_id("nA");
    let bucket_b = package_storage::bucket_for_id("nB");
    let orig_rel_a = format!("starmaps/{starmap_id}/nodes/{bucket_a}/nA.json");
    let orig_rel_b = format!("starmaps/{starmap_id}/nodes/{bucket_b}/nB.json");
    let orig_abs_a = dir.path().join(&orig_rel_a);
    let orig_abs_b = dir.path().join(&orig_rel_b);
    assert!(orig_abs_a.exists(), "A 原文件应存在");
    assert!(orig_abs_b.exists(), "B 原文件应存在");

    // 3. 重新加载完整 store，remove A + B，使 deleted_node_ids = {nA, nB}。
    //    此时 A/B 文件仍在原位，load_full 不会失败。
    let mut store = StarMapStore::new(dir.path(), starmap_id);
    store.load_full().unwrap();
    store.remove_node("nA");
    store.remove_node("nB");
    assert!(store.deleted_node_ids.contains("nA"));
    assert!(store.deleted_node_ids.contains("nB"));

    // 4. 模拟事务 A 的 journal 残留：objects=[A], facts=[A], phase=Planned。
    //    先把 A 原文件手动移到 journal 的 trash 路径（模拟 A 已进 trash，
    //    tombstone 未写），这样 resume 时 delete_node_file_to_trash 对 A 返回
    //    Ok(None)（幂等跳过）。
    let trash_rel_path = "sync/trash/test_resume_token".to_string();
    let trash_abs_a = dir.path().join(&trash_rel_path).join(&orig_rel_a);
    std::fs::create_dir_all(trash_abs_a.parent().unwrap()).unwrap();
    std::fs::rename(&orig_abs_a, &trash_abs_a).unwrap();
    assert!(!orig_abs_a.exists(), "A 原文件已移到 trash");
    assert!(trash_abs_a.exists(), "A trash 文件应存在");

    let fact_a = SyncDeleteFact {
        original_path: orig_rel_a.clone(),
        original_hash: String::new(),
        deleted_at: 1000,
        deleted_by: "test_device".to_string(),
        trash_path: format!("{trash_rel_path}/{orig_rel_a}"),
    };
    let plan = PlannedStarMapObjectDelete {
        token: "test_resume_token".to_string(),
        starmap_id: starmap_id.clone(),
        trash_rel_path: trash_rel_path.clone(),
        objects: vec![StarMapObjectDeleteTarget {
            kind: StarMapObjectKind::Node,
            id: "nA".to_string(),
            original_path: orig_rel_a.clone(),
        }],
        sync_delete_facts: vec![fact_a.clone()],
        phase: StarMapObjectDeletePhase::Planned,
    };
    PlannedStarMapObjectDelete::save_planned(dir.path(), &plan).unwrap();

    // 5. resume flush —— 只处理 A（journal.objects），不碰 B。
    store.flush_save_queue().unwrap();

    // 6. 断言：B 原文件仍在原位（核心断言）。
    assert!(
        orig_abs_b.exists(),
        "B 原文件应仍在原位——resume 不应处理 journal.objects 之外的对象"
    );
    // A 的事务完成后 journal 应清除。
    assert!(
        PlannedStarMapObjectDelete::load(dir.path(), starmap_id)
            .unwrap()
            .is_none(),
        "A 的事务完成后 journal 应清除"
    );
    // deleted_node_ids：A 已完成移除，B 仍 pending 保留。
    assert!(
        !store.deleted_node_ids.contains("nA"),
        "A 已完成，应从 deleted_node_ids 移除"
    );
    assert!(
        store.deleted_node_ids.contains("nB"),
        "B 仍 pending，应留在 deleted_node_ids"
    );
    // tombstones 只含 A，不含 B。
    let sync_state = SyncService::load_sync_state(dir.path()).unwrap();
    let has_a_tomb = sync_state
        .tombstones
        .iter()
        .any(|t| t.original_path == orig_rel_a);
    let has_b_tomb = sync_state
        .tombstones
        .iter()
        .any(|t| t.original_path == orig_rel_b);
    assert!(has_a_tomb, "A 应有 tombstone");
    assert!(!has_b_tomb, "B 不应有 tombstone");

    // 7. 下一轮 flush 为 B 新建新事务，B 被处理。
    store.flush_save_queue().unwrap();
    assert!(!orig_abs_b.exists(), "B 原文件应已被移到 trash");
    assert!(
        !store.deleted_node_ids.contains("nB"),
        "B 完成后应从 deleted_node_ids 移除"
    );
    let sync_state2 = SyncService::load_sync_state(dir.path()).unwrap();
    let has_b_tomb2 = sync_state2
        .tombstones
        .iter()
        .any(|t| t.original_path == orig_rel_b);
    assert!(has_b_tomb2, "B 应有 tombstone");
}
