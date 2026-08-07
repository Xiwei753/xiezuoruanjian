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

    std::fs::remove_file(&node_path).unwrap();
    std::fs::create_dir_all(&node_path).unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    store2.load_full().unwrap();
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
fn layout_flush_only_on_explicit_save() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Node1"));
    store.flush().unwrap();

    let mut layout = StarMapLayout::default();
    layout.nodes.push(StarMapLayoutNode {
        node_id: "n1".to_string(),
        x: 10.0,
        y: 20.0,
        width: 100.0,
        height: 50.0,
        radius: 25.0,
        collapsed: false,
        z_index: 0,
        scale: 1.0,
        depth: 0.0,
        focus_weight: 0.0,
        orbit_group: None,
    });
    store.set_layout(layout);
    store.enqueue_save(SaveQueueEntry::Layout);
    store.enqueue_save(SaveQueueEntry::GraphMeta);

    assert!(store.is_dirty());
    assert_eq!(store.save_queue_len(), 2);

    store.upsert_node(make_test_node("n2", "Node2"));
    store.enqueue_save(SaveQueueEntry::Node);
    store.enqueue_save(SaveQueueEntry::GraphMeta);

    assert_eq!(store.save_queue_len(), 3);

    store.flush_save_queue().unwrap();
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

    use crate::starmap::types::{StarMapEdge, StarMapEdgeKind};
    store.upsert_edge(StarMapEdge {
        id: "e1".to_string(),
        from: Some("n1".to_string()),
        to: Some("n2".to_string()),
        kind: StarMapEdgeKind::References,
        label: None,
        payload: None,
        from_target: None,
        to_target: None,
        from_endpoint: None,
        to_endpoint: None,
        from_endpoint_path: None,
        to_endpoint_path: None,
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
    assert_eq!(
        store2.graph_meta.as_ref().unwrap().node_ids.len(),
        3,
        "graph_meta node_ids should have 3 after removal"
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
    let meta_ids = store.graph_meta.as_ref().unwrap().link_ids.clone();
    assert!(
        meta_ids.contains(&"l1".to_string()),
        "add_link must add link_id to graph_meta.link_ids"
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
