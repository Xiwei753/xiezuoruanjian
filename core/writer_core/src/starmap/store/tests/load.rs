use super::super::meta::DeletedSinceLastSync;
use super::super::*;
use super::*;
use crate::starmap::semantic::StarMapTargetDetail;
use crate::starmap::types::reference::StarMapTargetPath;
use tempfile::TempDir;

#[test]
fn store_new_has_zero_counts() {
    let dir = TempDir::new().unwrap();
    let store = StarMapStore::new(dir.path(), "test-id");
    assert_eq!(store.node_count(), 0);
    assert_eq!(store.edge_count(), 0);
    assert_eq!(store.embed_count(), 0);
    assert_eq!(store.package_revision(), 0);
    assert!(!store.is_dirty());
}
#[test]
fn load_full_errors_for_missing_declared_node() {
    let dir = TempDir::new().unwrap();
    let starmap_dir = dir.path().join("starmaps").join("test-id");
    std::fs::create_dir_all(starmap_dir.join("nodes")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("edges")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("embeds")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("links")).unwrap();

    let meta = GraphMeta {
        schema_version: "4".to_string(),
        starmap_id: "test-id".to_string(),
        node_ids: vec!["missing-node".to_string()],
        edge_ids: vec![],
        embed_instance_ids: vec![],
        link_ids: vec![],
        edge_relation_index: vec![],
        embed_host_index: vec![],
        link_relation_index: vec![],
        node_kind_counts: HashMap::new(),
        package_revision: 1,
        updated_at: 0,
        deleted_since_last_sync: DeletedSinceLastSync::default(),
        ..Default::default()
    };
    let json = serde_json::to_string_pretty(&meta).unwrap();
    std::fs::write(starmap_dir.join("graph.json"), json).unwrap();

    let mut store = StarMapStore::new(dir.path(), "test-id");
    // fail-closed：GraphMeta 声明 node 但文件缺失 → Err，不能返回残缺图。
    assert!(
        store.load_full().is_err(),
        "load_full must Err when GraphMeta declares a node but its file is missing"
    );
}
#[test]
fn load_full_errors_for_missing_declared_link() {
    let dir = TempDir::new().unwrap();
    let starmap_dir = dir.path().join("starmaps").join("test-id");
    std::fs::create_dir_all(starmap_dir.join("nodes")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("edges")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("embeds")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("links")).unwrap();

    let meta = GraphMeta {
        schema_version: "4".to_string(),
        starmap_id: "test-id".to_string(),
        node_ids: vec![],
        edge_ids: vec![],
        embed_instance_ids: vec![],
        link_ids: vec!["missing-link".to_string()],
        edge_relation_index: vec![],
        embed_host_index: vec![],
        link_relation_index: vec![],
        node_kind_counts: HashMap::new(),
        package_revision: 1,
        updated_at: 0,
        deleted_since_last_sync: DeletedSinceLastSync::default(),
        ..Default::default()
    };
    let json = serde_json::to_string_pretty(&meta).unwrap();
    std::fs::write(starmap_dir.join("graph.json"), json).unwrap();

    let mut store = StarMapStore::new(dir.path(), "test-id");
    // 新契约（Issue #1060）：链接唯一持久化在 annotations 仓库；
    // `GraphMeta.link_ids` 只是派生缓存。声明的旧缓存条目没有对应仓库记录时
    // 不算损坏，按仓库重建为 []，不 fail-closed。
    let result = store
        .load_full()
        .expect("load_full must not Err on stale derived link_ids cache");
    assert_eq!(result.loaded_link_count, 0);
    assert!(
        store.graph_meta_link_ids().is_empty(),
        "stale link_ids cache must be rebuilt from annotations repo"
    );
}
#[test]
fn load_full_detects_dangling_edge_reference() {
    let dir = TempDir::new().unwrap();
    let starmap_dir = dir.path().join("starmaps").join("test-id");
    std::fs::create_dir_all(starmap_dir.join("nodes")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("edges")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("embeds")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("links")).unwrap();

    let node = make_test_node("n1", "Node1");
    let node_json = serde_json::to_string_pretty(&node).unwrap();
    write_to_bucket(&starmap_dir, "nodes", "n1", &node_json);

    let edge = StarMapEdge {
        id: "e1".to_string(),
        kind: StarMapEdgeKind::References,
        label: None,
        payload: None,
        from: StarMapTargetPath {
            starmap_id: "test-id".to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n1".to_string(),
            },
        },
        to: StarMapTargetPath {
            starmap_id: "test-id".to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "nonexistent".to_string(),
            },
        },
        created_at: 0,
        updated_at: 0,
    };
    let edge_json = serde_json::to_string_pretty(&edge).unwrap();
    write_to_bucket(&starmap_dir, "edges", "e1", &edge_json);

    let meta = GraphMeta {
        schema_version: "4".to_string(),
        starmap_id: "test-id".to_string(),
        node_ids: vec!["n1".to_string()],
        edge_ids: vec!["e1".to_string()],
        embed_instance_ids: vec![],
        link_ids: vec![],
        edge_relation_index: vec![],
        embed_host_index: vec![],
        link_relation_index: vec![],
        node_kind_counts: HashMap::new(),
        package_revision: 1,
        updated_at: 0,
        deleted_since_last_sync: DeletedSinceLastSync::default(),
        ..Default::default()
    };
    let json = serde_json::to_string_pretty(&meta).unwrap();
    std::fs::write(starmap_dir.join("graph.json"), json).unwrap();

    let mut store = StarMapStore::new(dir.path(), "test-id");
    let result = store.load_full().unwrap();
    let dangling: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.kind == LoadDiagnosticKind::DanglingReference)
        .collect();
    assert!(!dangling.is_empty());
    assert!(dangling[0].detail.contains("non-existent node"));
}
#[test]
fn load_full_detects_orphan_object_on_disk() {
    let dir = TempDir::new().unwrap();
    let starmap_dir = dir.path().join("starmaps").join("test-id");
    std::fs::create_dir_all(starmap_dir.join("nodes")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("edges")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("embeds")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("links")).unwrap();

    let orphan_node = make_test_node("orphan-node", "Orphan");
    let orphan_json = serde_json::to_string_pretty(&orphan_node).unwrap();
    write_to_bucket(&starmap_dir, "nodes", "orphan-node", &orphan_json);

    let meta = GraphMeta {
        schema_version: "4".to_string(),
        starmap_id: "test-id".to_string(),
        node_ids: vec![],
        edge_ids: vec![],
        embed_instance_ids: vec![],
        link_ids: vec![],
        edge_relation_index: vec![],
        embed_host_index: vec![],
        link_relation_index: vec![],
        node_kind_counts: HashMap::new(),
        package_revision: 1,
        updated_at: 0,
        deleted_since_last_sync: DeletedSinceLastSync::default(),
        ..Default::default()
    };
    let json = serde_json::to_string_pretty(&meta).unwrap();
    std::fs::write(starmap_dir.join("graph.json"), json).unwrap();

    let mut store = StarMapStore::new(dir.path(), "test-id");
    let result = store.load_full().unwrap();
    let orphan: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.kind == LoadDiagnosticKind::OrphanObject)
        .collect();
    assert!(!orphan.is_empty());
    assert_eq!(orphan[0].object_id, "orphan-node");
}
#[test]
fn load_full_detects_unsupported_version() {
    let dir = TempDir::new().unwrap();
    let starmap_dir = dir.path().join("starmaps").join("test-id");
    std::fs::create_dir_all(starmap_dir.join("nodes")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("edges")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("embeds")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("links")).unwrap();

    let meta = GraphMeta {
        schema_version: "99".to_string(),
        starmap_id: "test-id".to_string(),
        node_ids: vec![],
        edge_ids: vec![],
        embed_instance_ids: vec![],
        link_ids: vec![],
        edge_relation_index: vec![],
        embed_host_index: vec![],
        link_relation_index: vec![],
        node_kind_counts: HashMap::new(),
        package_revision: 1,
        updated_at: 0,
        deleted_since_last_sync: DeletedSinceLastSync::default(),
        ..Default::default()
    };
    let json = serde_json::to_string_pretty(&meta).unwrap();
    std::fs::write(starmap_dir.join("graph.json"), json).unwrap();

    let mut store = StarMapStore::new(dir.path(), "test-id");
    let result = store.load_full();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code(), "UNSUPPORTED_VERSION");
}
#[test]
fn load_phased_graph_meta_only() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Node1"));
    store.flush().unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    let result = store2.load_phased(LoadPhase::GraphMeta).unwrap();
    assert_eq!(store2.current_load_phase(), Some(LoadPhase::GraphMeta));
    assert_eq!(result.loaded_node_count, 0);
    assert!(store2.get_node("n1").is_none());
}
#[test]
fn load_phased_full_equivalent_to_load_full() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Node1"));
    store.upsert_node(make_test_node("n2", "Node2"));
    store.flush().unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    let result = store2.load_phased(LoadPhase::BackgroundFullLoad).unwrap();
    assert_eq!(
        store2.current_load_phase(),
        Some(LoadPhase::BackgroundFullLoad)
    );
    assert_eq!(result.loaded_node_count, 2);
}
#[test]
fn load_phase_sequence() {
    assert_eq!(LoadPhase::GraphMeta.next(), Some(LoadPhase::CurrentObjects));
    assert_eq!(
        LoadPhase::CurrentObjects.next(),
        Some(LoadPhase::PrefetchNearbyObjects)
    );
    assert_eq!(
        LoadPhase::PrefetchNearbyObjects.next(),
        Some(LoadPhase::BackgroundFullLoad)
    );
    assert_eq!(LoadPhase::BackgroundFullLoad.next(), None);
}
#[test]
fn load_full_preserves_pending_deletes() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Node1"));
    store.upsert_node(make_test_node("n2", "Node2"));
    store.flush().unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    store2.load_full().unwrap();
    store2.remove_node("n1");
    assert!(store2.has_pending_deletes());

    store2.load_full().unwrap();
    assert!(store2.has_pending_deletes());
}
#[test]
fn load_phased_preserves_pending_deletes() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Node1"));
    store.flush().unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    store2.load_phased(LoadPhase::CurrentObjects).unwrap();
    store2.remove_node("n1");
    assert!(store2.has_pending_deletes());

    store2.load_phased(LoadPhase::BackgroundFullLoad).unwrap();
    assert!(store2.has_pending_deletes());
}
#[test]
fn ensure_fully_loaded_reaches_background_phase() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Node1"));
    store.flush().unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    store2.ensure_fully_loaded().unwrap();
    assert_eq!(
        store2.current_load_phase(),
        Some(LoadPhase::BackgroundFullLoad)
    );
    assert!(store2.get_node("n1").is_some());
}
#[test]
fn save_starmap_graph_corrupt_existing_returns_error() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Node1"));
    store.flush().unwrap();

    let graph_json = dir
        .path()
        .join("starmaps")
        .join(&meta.starmap_id)
        .join("graph.json");
    std::fs::write(&graph_json, "not valid json at all {{{").unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    let result = store2.load_full();
    assert!(
        result.is_err(),
        "fail-closed: corrupt graph.json should cause load_full to return Err"
    );
}

#[test]
fn save_starmap_graph_new_store_no_graph_json_succeeds() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let graph_json = dir
        .path()
        .join("starmaps")
        .join(&meta.starmap_id)
        .join("graph.json");
    assert!(!graph_json.exists());

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    assert!(store.load_full().is_ok());
}
#[test]
fn bucket_directory_structure_on_save() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Node1"));
    store.flush().unwrap();

    let bucket = package_storage::bucket_for_id("n1");
    let node_path = dir
        .path()
        .join("starmaps")
        .join(&meta.starmap_id)
        .join("nodes")
        .join(bucket)
        .join("n1.json");
    assert!(node_path.exists());
}
#[test]
fn ensure_loaded_preserves_dirty_after_crud() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Node1"));
    store.enqueue_save(SaveQueueEntry::Node);
    store.enqueue_save(SaveQueueEntry::GraphMeta);
    store.flush().unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    store2.ensure_loaded().unwrap();
    store2.upsert_node(make_test_node("n2", "Node2"));
    store2.enqueue_save(SaveQueueEntry::Node);
    store2.enqueue_save(SaveQueueEntry::GraphMeta);
    assert!(store2.is_dirty());

    store2.ensure_loaded().unwrap();
    assert!(store2.is_dirty());
    assert!(store2.dirty_nodes.contains("n2"));
}
#[test]
fn ensure_object_loaded_for_edge_embed_link() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    let node = make_test_node("n1", "Node1");
    store.upsert_node(node.clone());
    let edge = StarMapEdge {
        id: "e1".to_string(),
        from: StarMapTargetPath {
            starmap_id: meta.starmap_id.clone(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n1".to_string(),
            },
        },
        to: StarMapTargetPath {
            starmap_id: meta.starmap_id.clone(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n1".to_string(),
            },
        },
        kind: StarMapEdgeKind::RelatedTo,
        label: None,
        payload: None,
        created_at: 0,
        updated_at: 0,
    };
    store.upsert_edge(edge.clone());
    store.enqueue_save(SaveQueueEntry::Node);
    store.enqueue_save(SaveQueueEntry::Edge);
    store.enqueue_save(SaveQueueEntry::GraphMeta);
    store.flush().unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    store2.load_phased(LoadPhase::GraphMeta).unwrap();
    assert!(!store2.edges.contains_key("e1"));

    store2.ensure_edge_loaded("e1").unwrap();
    assert!(store2.edges.contains_key("e1"));
}
#[test]
fn list_links_with_diagnostics_returns_missing_diagnostic() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    // Write graph.json directly to disk with a link_ids entry that has no corresponding file.
    let graph_meta = GraphMeta {
        schema_version: "4".to_string(),
        starmap_id: meta.starmap_id.clone(),
        node_ids: vec![],
        edge_ids: vec![],
        embed_instance_ids: vec![],
        link_ids: vec!["missing-link".to_string()],
        edge_relation_index: vec![],
        embed_host_index: vec![],
        link_relation_index: vec![],
        node_kind_counts: HashMap::new(),
        package_revision: 0,
        updated_at: 0,
        deleted_since_last_sync: DeletedSinceLastSync::default(),
        ..Default::default()
    };
    let graph_json = serde_json::to_string_pretty(&graph_meta).unwrap();
    let graph_path = dir
        .path()
        .join("starmaps")
        .join(&meta.starmap_id)
        .join("graph.json");
    std::fs::create_dir_all(graph_path.parent().unwrap()).unwrap();
    std::fs::write(&graph_path, &graph_json).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    // 只加载 GraphMeta 阶段。
    store.load_phased(LoadPhase::GraphMeta).unwrap();

    // 新契约（Issue #1060）：链接不在星图包内，没有可缺失的 links/*.json，
    // `link_ids` 只是派生缓存。列表以 annotations 仓库为准，空仓库即空列表，
    // 不再为旧缓存条目产出 missing diagnostic；缓存被重建为空。
    let result = store.list_links_with_diagnostics().unwrap();
    assert!(
        result.diagnostics.is_empty(),
        "links are not package objects: no missing-link diagnostic may be reported"
    );
    assert!(result.items.is_empty());
    assert!(
        store.graph_meta_link_ids().is_empty(),
        "stale link_ids cache must be reconciled to the annotations repo"
    );
}
#[test]
fn prefetch_only_loads_adjacent_edges() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    for i in 1..=10 {
        store.upsert_node(make_test_node(&format!("n{}", i), &format!("Node{}", i)));
    }

    use crate::starmap::types::{StarMapEdge, StarMapEdgeKind};
    for i in 1..=9 {
        store.upsert_edge(StarMapEdge {
            id: format!("e{}", i),
            from: StarMapTargetPath {
                starmap_id: meta.starmap_id.clone(),
                segments: vec![],
                target: StarMapTargetDetail::Node {
                    node_id: format!("n{}", i),
                },
            },
            to: StarMapTargetPath {
                starmap_id: meta.starmap_id.clone(),
                segments: vec![],
                target: StarMapTargetDetail::Node {
                    node_id: format!("n{}", i + 1),
                },
            },
            kind: StarMapEdgeKind::References,
            label: None,
            payload: None,
            created_at: 0,
            updated_at: 0,
        });
    }
    store.flush().unwrap();

    let disk_meta: GraphMeta = serde_json::from_str(
        &std::fs::read_to_string(store.starmap_dir().join("graph.json")).unwrap(),
    )
    .unwrap();
    assert!(
        !disk_meta.edge_relation_index.is_empty(),
        "graph_meta should have edge_relation_index"
    );

    let mut fresh = StarMapStore::new(dir.path(), &meta.starmap_id);
    fresh.graph_meta = Some(disk_meta);
    fresh.upsert_node(make_test_node("n2", "Node2"));
    fresh.upsert_node(make_test_node("n3", "Node3"));
    assert_eq!(fresh.nodes.len(), 2, "only loaded 2 selected nodes");

    fresh.prefetch_nearby_objects(&mut vec![]).unwrap();

    assert!(
        fresh.nodes.contains_key("n1"),
        "n1 should be loaded via prefetch (adjacent to n2 via e1)"
    );
    assert!(
        fresh.nodes.contains_key("n4"),
        "n4 should be loaded via prefetch (adjacent to n3 via e3)"
    );
    assert!(
        fresh.edges.contains_key("e1"),
        "edge e1 (n1-n2) should be loaded"
    );
    assert!(
        fresh.edges.contains_key("e2"),
        "edge e2 (n2-n3) should be loaded"
    );
    assert!(
        fresh.edges.contains_key("e3"),
        "edge e3 (n3-n4) should be loaded"
    );
    assert!(
        !fresh.edges.contains_key("e5"),
        "edge e5 (n5-n6) should NOT be loaded (no loaded node adjacent)"
    );
    assert!(
        !fresh.edges.contains_key("e9"),
        "edge e9 (n9-n10) should NOT be loaded (no loaded node adjacent)"
    );
}
#[test]
fn edge_relation_index_preserves_endpoint_fields() {
    use crate::starmap::types::{StarMapEdge, StarMapEdgeKind};
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Node1"));
    store.upsert_node(make_test_node("n2", "Node2"));
    store.upsert_edge(StarMapEdge {
        id: "e1".to_string(),
        from: StarMapTargetPath {
            starmap_id: meta.starmap_id.clone(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n1".to_string(),
            },
        },
        to: StarMapTargetPath {
            starmap_id: meta.starmap_id.clone(),
            segments: vec![],
            target: StarMapTargetDetail::Anchor {
                node_id: "n2".to_string(),
                anchor_id: "a1".to_string(),
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
    store2.load_full().unwrap();
    let eri = store2
        .graph_meta
        .as_ref()
        .unwrap()
        .edge_relation_index
        .iter()
        .find(|e| e.edge_id == "e1")
        .unwrap();
    assert!(
        matches!(eri.from.target, StarMapTargetDetail::Node { .. }),
        "from target should be Node in index"
    );
    assert!(
        matches!(eri.to.target, StarMapTargetDetail::Anchor { .. }),
        "to target should be Anchor in index"
    );
}
#[test]
fn embed_host_index_preserves_host_endpoint() {
    use crate::starmap::semantic::StarMapProvenance;
    use crate::starmap::types::StarMapEmbed;
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Node1"));
    store.upsert_embed(StarMapEmbed {
        instance_id: "emb1".to_string(),
        target_starmap_id: "sm-child".to_string(),
        label: None,
        position: Default::default(),
        host_path: StarMapTargetPath {
            starmap_id: meta.starmap_id.clone(),
            segments: vec![],
            target: StarMapTargetDetail::Anchor {
                node_id: "n1".to_string(),
                anchor_id: "a1".to_string(),
            },
        },
        provenance: StarMapProvenance::default(),
        created_at: 0,
        updated_at: 0,
    });
    store.flush().unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    store2.load_full().unwrap();
    let ehi = store2
        .graph_meta
        .as_ref()
        .unwrap()
        .embed_host_index
        .iter()
        .find(|e| e.instance_id == "emb1")
        .unwrap();
    assert!(
        matches!(ehi.host_path.target, StarMapTargetDetail::Anchor { .. }),
        "host_path should preserve Anchor target in index"
    );
}
#[test]
fn list_links_with_diagnostics_reads_the_unified_repository() {
    use crate::starmap::types::StarMapLink;
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    // Issue #1060：链接唯一所有者在 annotations 仓库；add_link 写仓库，
    // 星图包不再生成 links/*.json。
    let link = StarMapLink {
        link_id: "l1".to_string(),
        source: StarMapTargetPath {
            starmap_id: meta.starmap_id.clone(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n1".to_string(),
            },
        },
        target: StarMapTargetPath {
            starmap_id: meta.starmap_id.clone(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "other".to_string(),
            },
        },
        label: None,
        created_at: 0,
        updated_at: 0,
    };
    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.add_link(link.clone()).unwrap();
    store.enqueue_save(SaveQueueEntry::GraphMeta);
    store.flush().unwrap();

    let starmap_dir = dir.path().join("starmaps").join(&meta.starmap_id);
    let bucket = package_storage::bucket_for_id("l1");
    assert!(
        !starmap_dir
            .join("links")
            .join(bucket)
            .join("l1.json")
            .exists(),
        "旧 links/*.json 不得再被写出"
    );

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    let result = store2.list_links_with_diagnostics().unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].link_id, "l1");
    assert!(result.diagnostics.is_empty(), "仓库读取不应产生链接诊断");
}

#[test]
fn prefetch_nearby_objects_no_infinite_recursion_when_no2_index() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "NoIndex", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Node1"));
    store.upsert_node(make_test_node("n2", "Node2"));

    let edge = crate::starmap::types::StarMapEdge {
        id: "e1".to_string(),
        from: StarMapTargetPath {
            starmap_id: meta.starmap_id.clone(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n1".to_string(),
            },
        },
        to: StarMapTargetPath {
            starmap_id: meta.starmap_id.clone(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n2".to_string(),
            },
        },
        kind: crate::starmap::types::StarMapEdgeKind::RelatedTo,
        label: None,
        payload: None,
        created_at: 0,
        updated_at: 0,
    };
    store.upsert_edge(edge);

    store.flush_save_queue().unwrap();
    let dirty = store.collect_flush_dirty_set();
    store.update_graph_meta_file(&dirty).unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    let result = store2.load_phased(LoadPhase::PrefetchNearbyObjects);
    assert!(
        result.is_ok(),
        "load_phased must not stack overflow: {:?}",
        result
    );
}

// ---------------------------------------------------------------------------
// 测试组 B：phased/full loader 服从 GraphMeta 成员列表（Issue #772 回归）
// ---------------------------------------------------------------------------

#[test]
fn full_load_evicts_non_dirty_orphan_from_memory() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    // 手动 insert orphan node（不在 GraphMeta.node_ids）
    store.upsert_node(make_test_node("n_old", "Orphan"));
    assert!(store.get_node("n_old").is_some());
    // 清掉 dirty，让它成为非 dirty orphan（partial cache 残留或旧 revision 残留）
    store.dirty_nodes.clear();

    store.load_full().unwrap();

    assert!(
        store.get_node("n_old").is_none(),
        "non-dirty orphan not in GraphMeta.node_ids should be evicted by load_full"
    );
}

#[test]
fn full_load_keeps_dirty_object_not_in_graph_meta() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    // 刚 add 的新 node，在 dirty_nodes，还没 flush，不在 GraphMeta.node_ids
    store.upsert_node(make_test_node("n_new", "New"));
    assert!(store.dirty_nodes.contains("n_new"));

    store.load_full().unwrap();

    assert!(
        store.get_node("n_new").is_some(),
        "dirty object should be kept by load_full even if not in GraphMeta.node_ids"
    );
}

// ---------------------------------------------------------------------------
// Issue #781 评论 5863487463 复现测试
// ---------------------------------------------------------------------------
//
// 这组测试断言 issue 要求的 fail-closed / 数据完整性行为。
// 当前代码违反这些要求（load_full 吞错误返回残缺图），
// 因此这些测试在未修复的代码上会失败，从而证明 bug 存在。
// 修复后这些测试应当通过。

/// 复现问题 2：load_full 对 GraphMeta 声明的 node 文件缺失必须 Err，不能返回残缺图。
///
/// 场景：schema "4" graph.json 声明 node "missing-node"，但对应文件不存在。
/// 期望：load_full 返回 Err（GraphMeta 声明的对象不允许 None）。
/// 当前错误行为：load_full 跳过失败对象，返回 Ok 且 loaded_node_count=0（残缺图）。
#[test]
fn repro_781_load_full_missing_declared_node_must_error() {
    let dir = TempDir::new().unwrap();
    let starmap_dir = dir.path().join("starmaps").join("test-id");
    std::fs::create_dir_all(starmap_dir.join("nodes")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("edges")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("embeds")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("links")).unwrap();

    // graph.json 声明 node "missing-node"，但不写对应的 node 文件。
    let meta = GraphMeta {
        schema_version: "4".to_string(),
        starmap_id: "test-id".to_string(),
        node_ids: vec!["missing-node".to_string()],
        edge_ids: vec![],
        embed_instance_ids: vec![],
        link_ids: vec![],
        edge_relation_index: vec![],
        embed_host_index: vec![],
        link_relation_index: vec![],
        node_kind_counts: HashMap::new(),
        package_revision: 1,
        updated_at: 0,
        deleted_since_last_sync: DeletedSinceLastSync::default(),
        ..Default::default()
    };
    let json = serde_json::to_string_pretty(&meta).unwrap();
    std::fs::write(starmap_dir.join("graph.json"), json).unwrap();

    let mut store = StarMapStore::new(dir.path(), "test-id");
    let result = store.load_full();

    // 期望：fail-closed 返回 Err（GraphMeta 声明的 node 文件缺失）。
    // 当前 bug 行为：返回 Ok 且 loaded_node_count=0（残缺图，看起来成功但少对象）。
    assert!(
        result.is_err(),
        "repro_781: load_full must Err when GraphMeta declares a node but its file is missing, \
         but got Ok (current bug: returns incomplete graph as if successful). result={result:?}"
    );
}

/// 复现问题 2（补充）：load_full 对 GraphMeta 声明的 link 文件缺失不得误报。
///
/// 场景：schema "4" graph.json 遗留派生缓存 `link_ids=["missing-link"]`。
/// 新契约（Issue #1060）：链接唯一事实在 annotations 仓库，包内不存在
/// `links/*.json`；该声明只是过期派生缓存，必须按仓库重建而不是 fail-closed。
#[test]
fn repro_781_load_full_missing_declared_link_must_error() {
    let dir = TempDir::new().unwrap();
    let starmap_dir = dir.path().join("starmaps").join("test-id");
    std::fs::create_dir_all(starmap_dir.join("nodes")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("edges")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("embeds")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("links")).unwrap();

    let meta = GraphMeta {
        schema_version: "4".to_string(),
        starmap_id: "test-id".to_string(),
        node_ids: vec![],
        edge_ids: vec![],
        embed_instance_ids: vec![],
        link_ids: vec!["missing-link".to_string()],
        edge_relation_index: vec![],
        embed_host_index: vec![],
        link_relation_index: vec![],
        node_kind_counts: HashMap::new(),
        package_revision: 1,
        updated_at: 0,
        deleted_since_last_sync: DeletedSinceLastSync::default(),
        ..Default::default()
    };
    let json = serde_json::to_string_pretty(&meta).unwrap();
    std::fs::write(starmap_dir.join("graph.json"), json).unwrap();

    let mut store = StarMapStore::new(dir.path(), "test-id");
    // 新契约（Issue #1060）：`link_ids` 是 annotations 仓库的派生缓存，
    // 过期的 "missing-link" 必须被重建掉，而不是当成缺失包内对象报错。
    let result = store
        .load_full()
        .expect("stale derived link_ids cache must not fail package load");
    assert_eq!(result.loaded_link_count, 0);
    assert!(
        store.graph_meta_link_ids().is_empty(),
        "stale link_ids cache must be rebuilt from annotations repo"
    );
}

// ---------------------------------------------------------------------------
// 评论 5863582949：对象加载严格传播（缺失 / 损坏 / ID 不一致都必须 Err）
// ---------------------------------------------------------------------------

/// 写一份只声明指定成员列表的 graph.json（schema "4"）。
fn write_graph_meta_declaring(
    starmap_dir: &std::path::Path,
    starmap_id: &str,
    field: &str,
    ids: Vec<String>,
) {
    let mut meta = GraphMeta {
        schema_version: "4".to_string(),
        starmap_id: starmap_id.to_string(),
        package_revision: 1,
        ..Default::default()
    };
    match field {
        "nodeIds" => meta.node_ids = ids,
        "edgeIds" => meta.edge_ids = ids,
        "embedInstanceIds" => meta.embed_instance_ids = ids,
        "linkIds" => meta.link_ids = ids,
        other => panic!("unknown member list field '{other}'"),
    }
    std::fs::write(
        starmap_dir.join("graph.json"),
        serde_json::to_string_pretty(&meta).unwrap(),
    )
    .unwrap();
}

#[test]
fn load_full_errors_for_corrupt_declared_node_file() {
    let dir = TempDir::new().unwrap();
    let starmap_id = "test-id";
    let starmap_dir = dir.path().join("starmaps").join(starmap_id);
    std::fs::create_dir_all(starmap_dir.join("nodes")).unwrap();

    write_to_bucket(&starmap_dir, "nodes", "n1", "THIS IS NOT JSON");
    write_graph_meta_declaring(&starmap_dir, starmap_id, "nodeIds", vec!["n1".to_string()]);

    let mut store = StarMapStore::new(dir.path(), starmap_id);
    // fail-closed：声明的 node 文件存在但解析失败 → Err，不能当 missing 跳过。
    let result = store.load_full();
    assert!(
        result.is_err(),
        "load_full must Err when a declared node file cannot be parsed, got: {result:?}"
    );
}

#[test]
fn load_full_errors_when_object_internal_id_mismatches_filename() {
    use crate::starmap::semantic::StarMapProvenance;
    use crate::starmap::types::*;

    let dir = TempDir::new().unwrap();
    let starmap_id = "test-id";
    let starmap_dir = dir.path().join("starmaps").join(starmap_id);
    for subdir in ["nodes", "edges", "embeds", "links"] {
        std::fs::create_dir_all(starmap_dir.join(subdir)).unwrap();
    }

    let target = |starmap: &str| StarMapTargetPath {
        starmap_id: starmap.to_string(),
        segments: vec![],
        target: StarMapTargetDetail::Starmap,
    };
    let mismatched_node = make_test_node("id-in-file", "N");
    let mismatched_edge = StarMapEdge {
        id: "id-in-file".to_string(),
        from: target(starmap_id),
        to: target(starmap_id),
        kind: StarMapEdgeKind::RelatedTo,
        label: None,
        payload: None,
        created_at: 0,
        updated_at: 0,
    };
    let mismatched_embed = StarMapEmbed {
        instance_id: "id-in-file".to_string(),
        target_starmap_id: starmap_id.to_string(),
        label: None,
        position: Default::default(),
        host_path: target(starmap_id),
        provenance: StarMapProvenance::default(),
        created_at: 0,
        updated_at: 0,
    };
    let mismatched_link = make_test_link("id-in-file", "L", starmap_id, "id-in-file");

    // 新契约（Issue #1060）：links 不是星图包内对象，内部 id 校验只适用于
    // nodes/edges/embeds；旧 links/ 目录由迁移迁出，不再参与对象加载。
    let cases = [
        (
            "nodes",
            "nodeIds",
            "node-a",
            serde_json::to_string(&mismatched_node).unwrap(),
        ),
        (
            "edges",
            "edgeIds",
            "edge-a",
            serde_json::to_string(&mismatched_edge).unwrap(),
        ),
        (
            "embeds",
            "embedInstanceIds",
            "embed-a",
            serde_json::to_string(&mismatched_embed).unwrap(),
        ),
    ];
    for (subdir, field, declared_id, content) in cases {
        write_to_bucket(&starmap_dir, subdir, declared_id, &content);
        write_graph_meta_declaring(
            &starmap_dir,
            starmap_id,
            field,
            vec![declared_id.to_string()],
        );

        let mut store = StarMapStore::new(dir.path(), starmap_id);
        let result = store.load_full();
        assert!(
            result.is_err(),
            "load_full must Err when {subdir} internal id does not match filename \
             '{declared_id}', got: {result:?}"
        );
    }

    // 新契约（Issue #1060）：旧 links/ 文件不是星图包对象，加载时会一次性
    // 迁移进 annotations 仓库；链接身份取自记录内部的 `linkId`，文件名不再参与
    // id 一致性校验，因此文件名不匹配不会让 load_full 失败。
    write_to_bucket(
        &starmap_dir,
        "links",
        "link-a",
        &serde_json::to_string(&mismatched_link).unwrap(),
    );
    write_graph_meta_declaring(
        &starmap_dir,
        starmap_id,
        "linkIds",
        vec!["link-a".to_string()],
    );
    let mut store = StarMapStore::new(dir.path(), starmap_id);
    let result = store
        .load_full()
        .expect("legacy links/ files must be migrated, not treated as package objects");
    assert_eq!(result.loaded_link_count, 1);
    assert!(
        store.get_link("id-in-file").is_some(),
        "legacy link must be migrated by its internal linkId, not the file name"
    );
}

#[test]
fn load_phased_current_objects_errors_for_missing_declared_node() {
    let dir = TempDir::new().unwrap();
    let starmap_id = "test-id";
    let starmap_dir = dir.path().join("starmaps").join(starmap_id);
    std::fs::create_dir_all(starmap_dir.join("nodes")).unwrap();

    write_graph_meta_declaring(
        &starmap_dir,
        starmap_id,
        "nodeIds",
        vec!["missing-node".to_string()],
    );

    let mut store = StarMapStore::new(dir.path(), starmap_id);
    let result = store.load_phased(LoadPhase::CurrentObjects);
    assert!(
        result.is_err(),
        "load_phased(CurrentObjects) must Err for a missing declared node, got: {result:?}"
    );
}

#[test]
fn load_remaining_objects_errors_for_missing_declared_node() {
    let dir = TempDir::new().unwrap();
    let starmap_id = "test-id";
    let starmap_dir = dir.path().join("starmaps").join(starmap_id);
    std::fs::create_dir_all(starmap_dir.join("nodes")).unwrap();

    write_graph_meta_declaring(
        &starmap_dir,
        starmap_id,
        "nodeIds",
        vec!["missing-node".to_string()],
    );

    let mut store = StarMapStore::new(dir.path(), starmap_id);
    store.load_graph_meta_phase(&mut vec![]).unwrap();
    let result = store.load_remaining_objects(&mut vec![]);
    assert!(
        result.is_err(),
        "load_remaining_objects must Err for a missing declared node, got: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// Issue #1062：本地加载必须带上 references.json 反向绑定
// ---------------------------------------------------------------------------

/// 旧端实例只写 `instanceId`/`position`，共享本体来自另一个作品；本地
/// `try_load_node` 必须按 references.json 反向绑定解析 assetId 并迁移落盘。
#[test]
fn try_load_node_resolves_cross_project_reference_binding() {
    let dir = TempDir::new().unwrap();
    let starmap_id = "sm_local";
    let instance_id = "n1";
    let asset_id = "asset_shared_from_other_project";
    let starmap_dir = dir.path().join("starmaps").join(starmap_id);
    std::fs::create_dir_all(starmap_dir.join("nodes")).unwrap();

    // 跨作品复用：真实 assetId 与 instanceId 不同，绑定记录属于另一作品。
    let source = make_test_node(instance_id, "共享标题");
    let definition = crate::starmap::types::StarMapNodeDefinition::from_node(asset_id, &source);
    crate::starmap::library::save_node_definition(dir.path(), &definition).unwrap();
    crate::starmap::library::record_node_reference(
        dir.path(),
        starmap_id,
        Some("p_other"),
        instance_id,
        asset_id,
    )
    .unwrap();

    write_to_bucket(
        &starmap_dir,
        "nodes",
        instance_id,
        &serde_json::json!({
            "instanceId": instance_id,
            "position": {"x": 3.0, "y": 4.0}
        })
        .to_string(),
    );

    let mut store = StarMapStore::new(dir.path(), starmap_id);
    let node = store
        .try_load_node(instance_id)
        .expect("local load must resolve the reference binding");
    assert_eq!(node.id, instance_id);
    assert_eq!(node.asset_id, asset_id);
    assert_eq!(node.title, "共享标题");
    assert_eq!(node.position.x, 3.0);
    assert_eq!(node.position.y, 4.0);

    let node_path = starmap_dir
        .join("nodes")
        .join(package_storage::bucket_for_id(instance_id))
        .join(format!("{instance_id}.json"));
    let rewritten: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&node_path).unwrap()).unwrap();
    assert_eq!(rewritten["assetId"], asset_id);
    assert_eq!(rewritten["projectId"], "p_other");
}
