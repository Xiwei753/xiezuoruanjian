use super::super::meta::DeletedSinceLastSync;
use super::super::*;
use super::*;
use tempfile::TempDir;

#[test]
fn store_new_has_zero_counts() {
    let dir = TempDir::new().unwrap();
    let store = StarMapStore::new(dir.path(), "test-id");
    assert_eq!(store.node_count(), 0);
    assert_eq!(store.edge_count(), 0);
    assert_eq!(store.embed_count(), 0);
    assert_eq!(store.hyperlink_count(), 0);
    assert_eq!(store.package_revision(), 0);
    assert!(!store.is_dirty());
}
#[test]
fn load_full_returns_diagnostics_for_missing_files() {
    let dir = TempDir::new().unwrap();
    let starmap_dir = dir.path().join("starmaps").join("test-id");
    std::fs::create_dir_all(starmap_dir.join("nodes")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("edges")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("child_starmaps")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("hyperlinks")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("links")).unwrap();

    let meta = GraphMeta {
        schema_version: "2".to_string(),
        starmap_id: "test-id".to_string(),
        title: "Test".to_string(),
        node_ids: vec!["missing-node".to_string()],
        edge_ids: vec![],
        embed_instance_ids: vec![],
        link_ids: vec![],
        hyperlink_ids: vec![],
        edge_relation_index: vec![],
        embed_host_index: vec![],
        link_relation_index: vec![],
        hyperlink_relation_index: vec![],
        node_kind_counts: HashMap::new(),
        package_revision: 1,
        updated_at: 0,
        deleted_since_last_sync: DeletedSinceLastSync::default(),
    };
    let json = serde_json::to_string_pretty(&meta).unwrap();
    std::fs::write(starmap_dir.join("graph.json"), json).unwrap();

    let mut store = StarMapStore::new(dir.path(), "test-id");
    let result = store.load_full().unwrap();
    assert_eq!(result.loaded_node_count, 0);
    assert!(!result.diagnostics.is_empty());
    assert_eq!(result.diagnostics[0].kind, LoadDiagnosticKind::Missing);
}
#[test]
fn load_full_returns_diagnostics_for_missing_link() {
    let dir = TempDir::new().unwrap();
    let starmap_dir = dir.path().join("starmaps").join("test-id");
    std::fs::create_dir_all(starmap_dir.join("nodes")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("edges")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("child_starmaps")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("hyperlinks")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("links")).unwrap();

    let meta = GraphMeta {
        schema_version: "2".to_string(),
        starmap_id: "test-id".to_string(),
        title: "Test".to_string(),
        node_ids: vec![],
        edge_ids: vec![],
        embed_instance_ids: vec![],
        link_ids: vec!["missing-link".to_string()],
        hyperlink_ids: vec![],
        edge_relation_index: vec![],
        embed_host_index: vec![],
        link_relation_index: vec![],
        hyperlink_relation_index: vec![],
        node_kind_counts: HashMap::new(),
        package_revision: 1,
        updated_at: 0,
        deleted_since_last_sync: DeletedSinceLastSync::default(),
    };
    let json = serde_json::to_string_pretty(&meta).unwrap();
    std::fs::write(starmap_dir.join("graph.json"), json).unwrap();

    let mut store = StarMapStore::new(dir.path(), "test-id");
    let result = store.load_full().unwrap();
    assert_eq!(result.loaded_link_count, 0);
    let link_diag: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.object_type == "link")
        .collect();
    assert!(!link_diag.is_empty());
    assert_eq!(link_diag[0].kind, LoadDiagnosticKind::Missing);
}
#[test]
fn load_full_detects_dangling_edge_reference() {
    let dir = TempDir::new().unwrap();
    let starmap_dir = dir.path().join("starmaps").join("test-id");
    std::fs::create_dir_all(starmap_dir.join("nodes")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("edges")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("child_starmaps")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("hyperlinks")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("links")).unwrap();

    let node = make_test_node("n1", "Node1");
    let node_json = serde_json::to_string_pretty(&node).unwrap();
    write_to_bucket(&starmap_dir, "nodes", "n1", &node_json);

    let edge = StarMapEdge {
        id: "e1".to_string(),
        kind: StarMapEdgeKind::References,
        label: None,
        payload: None,
        from: Some("n1".to_string()),
        to: Some("nonexistent".to_string()),
        from_target: None,
        to_target: None,
        from_endpoint: None,
        to_endpoint: None,
        from_endpoint_path: None,
        to_endpoint_path: None,
        created_at: 0,
        updated_at: 0,
    };
    let edge_json = serde_json::to_string_pretty(&edge).unwrap();
    write_to_bucket(&starmap_dir, "edges", "e1", &edge_json);

    let meta = GraphMeta {
        schema_version: "2".to_string(),
        starmap_id: "test-id".to_string(),
        title: "Test".to_string(),
        node_ids: vec!["n1".to_string()],
        edge_ids: vec!["e1".to_string()],
        embed_instance_ids: vec![],
        link_ids: vec![],
        hyperlink_ids: vec![],
        edge_relation_index: vec![],
        embed_host_index: vec![],
        link_relation_index: vec![],
        hyperlink_relation_index: vec![],
        node_kind_counts: HashMap::new(),
        package_revision: 1,
        updated_at: 0,
        deleted_since_last_sync: DeletedSinceLastSync::default(),
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
    assert!(dangling[0].detail.contains("nonexistent"));
}
#[test]
fn load_full_detects_orphan_object_on_disk() {
    let dir = TempDir::new().unwrap();
    let starmap_dir = dir.path().join("starmaps").join("test-id");
    std::fs::create_dir_all(starmap_dir.join("nodes")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("edges")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("child_starmaps")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("hyperlinks")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("links")).unwrap();

    let orphan_node = make_test_node("orphan-node", "Orphan");
    let orphan_json = serde_json::to_string_pretty(&orphan_node).unwrap();
    write_to_bucket(&starmap_dir, "nodes", "orphan-node", &orphan_json);

    let meta = GraphMeta {
        schema_version: "2".to_string(),
        starmap_id: "test-id".to_string(),
        title: "Test".to_string(),
        node_ids: vec![],
        edge_ids: vec![],
        embed_instance_ids: vec![],
        link_ids: vec![],
        hyperlink_ids: vec![],
        edge_relation_index: vec![],
        embed_host_index: vec![],
        link_relation_index: vec![],
        hyperlink_relation_index: vec![],
        node_kind_counts: HashMap::new(),
        package_revision: 1,
        updated_at: 0,
        deleted_since_last_sync: DeletedSinceLastSync::default(),
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
    std::fs::create_dir_all(starmap_dir.join("child_starmaps")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("hyperlinks")).unwrap();
    std::fs::create_dir_all(starmap_dir.join("links")).unwrap();

    let meta = GraphMeta {
        schema_version: "99".to_string(),
        starmap_id: "test-id".to_string(),
        title: "Test".to_string(),
        node_ids: vec![],
        edge_ids: vec![],
        embed_instance_ids: vec![],
        link_ids: vec![],
        hyperlink_ids: vec![],
        edge_relation_index: vec![],
        embed_host_index: vec![],
        link_relation_index: vec![],
        hyperlink_relation_index: vec![],
        node_kind_counts: HashMap::new(),
        package_revision: 1,
        updated_at: 0,
        deleted_since_last_sync: DeletedSinceLastSync::default(),
    };
    let json = serde_json::to_string_pretty(&meta).unwrap();
    std::fs::write(starmap_dir.join("graph.json"), json).unwrap();

    let mut store = StarMapStore::new(dir.path(), "test-id");
    let result = store.load_full().unwrap();
    let unsupported: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.kind == LoadDiagnosticKind::UnsupportedVersion)
        .collect();
    assert!(!unsupported.is_empty());
    assert!(unsupported[0].detail.contains("99"));
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
fn load_phased_to_current_viewport_objects() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Node1"));
    let mut layout = StarMapLayout::default();
    layout.nodes.push(StarMapLayoutNode {
        node_id: "n1".to_string(),
        x: 0.0,
        y: 0.0,
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
    store.set_viewport(StarMapViewport {
        scale: 1.0,
        offset_x: 0.0,
        offset_y: 0.0,
        width: 200.0,
        height: 200.0,
    });
    store.flush().unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    let result = store2
        .load_phased(LoadPhase::CurrentViewportObjects)
        .unwrap();
    assert_eq!(
        store2.current_load_phase(),
        Some(LoadPhase::CurrentViewportObjects)
    );
    assert_eq!(result.loaded_node_count, 1);
    assert!(store2.get_node("n1").is_some());
}
#[test]
fn load_phased_viewport_objects_with_layout() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Node1"));
    store.upsert_node(make_test_node("n2", "Node2"));
    let mut layout = StarMapLayout::default();
    layout.nodes.push(StarMapLayoutNode {
        node_id: "n1".to_string(),
        x: 0.0,
        y: 0.0,
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
    store.flush().unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    let result = store2
        .load_phased(LoadPhase::CurrentViewportObjects)
        .unwrap();
    assert_eq!(
        store2.current_load_phase(),
        Some(LoadPhase::CurrentViewportObjects)
    );
    assert_eq!(result.loaded_node_count, 1);
    assert!(store2.get_node("n1").is_some());
    assert!(store2.get_node("n2").is_none());
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
    assert_eq!(
        LoadPhase::GraphMeta.next(),
        Some(LoadPhase::ViewportAndLayoutIndex)
    );
    assert_eq!(
        LoadPhase::ViewportAndLayoutIndex.next(),
        Some(LoadPhase::CurrentViewportObjects)
    );
    assert_eq!(
        LoadPhase::CurrentViewportObjects.next(),
        Some(LoadPhase::PrefetchNearbyObjects)
    );
    assert_eq!(
        LoadPhase::PrefetchNearbyObjects.next(),
        Some(LoadPhase::BackgroundFullLoad)
    );
    assert_eq!(LoadPhase::BackgroundFullLoad.next(), None);
}
#[test]
fn ensure_loaded_skips_repeated_load() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Node1"));
    let mut layout = StarMapLayout::default();
    layout.nodes.push(StarMapLayoutNode {
        node_id: "n1".to_string(),
        x: 0.0,
        y: 0.0,
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
    store.set_viewport(StarMapViewport {
        scale: 1.0,
        offset_x: 0.0,
        offset_y: 0.0,
        width: 200.0,
        height: 200.0,
    });
    store.flush().unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    store2.ensure_loaded().unwrap();
    assert_eq!(store2.node_count(), 1);

    store2.ensure_loaded().unwrap();
    assert_eq!(store2.node_count(), 1);
}
#[test]
fn load_phased_viewport_only_loads_layout_nodes() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "InViewport"));
    store.upsert_node(make_test_node("n2", "OutOfViewport"));
    let mut layout = StarMapLayout::default();
    layout.nodes.push(StarMapLayoutNode {
        node_id: "n1".to_string(),
        x: 0.0,
        y: 0.0,
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
    store.flush().unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    let result = store2
        .load_phased(LoadPhase::CurrentViewportObjects)
        .unwrap();
    assert_eq!(
        store2.current_load_phase(),
        Some(LoadPhase::CurrentViewportObjects)
    );
    assert!(store2.get_node("n1").is_some());
    assert!(store2.get_node("n2").is_none());
    assert_eq!(result.loaded_node_count, 1);
}
#[test]
fn prefetch_nearby_loads_adjacent_nodes() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Visible"));
    store.upsert_node(make_test_node("n2", "Adjacent"));
    let edge = StarMapEdge {
        id: "e1".to_string(),
        kind: StarMapEdgeKind::References,
        label: None,
        payload: None,
        from: Some("n1".to_string()),
        to: Some("n2".to_string()),
        from_target: None,
        to_target: None,
        from_endpoint: None,
        to_endpoint: None,
        from_endpoint_path: None,
        to_endpoint_path: None,
        created_at: 0,
        updated_at: 0,
    };
    store.upsert_edge(edge);
    let mut layout = StarMapLayout::default();
    layout.nodes.push(StarMapLayoutNode {
        node_id: "n1".to_string(),
        x: 0.0,
        y: 0.0,
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
    store.flush().unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    store2
        .load_phased(LoadPhase::CurrentViewportObjects)
        .unwrap();
    assert!(store2.get_node("n1").is_some());

    store2
        .load_phased(LoadPhase::PrefetchNearbyObjects)
        .unwrap();
    assert!(store2.get_node("n2").is_some());
    assert!(store2.get_edge("e1").is_some());
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
    store2
        .load_phased(LoadPhase::CurrentViewportObjects)
        .unwrap();
    store2.remove_node("n1");
    assert!(store2.has_pending_deletes());

    store2.load_phased(LoadPhase::BackgroundFullLoad).unwrap();
    assert!(store2.has_pending_deletes());
}
#[test]
fn ensure_loaded_uses_phased_loading() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Node1"));
    let mut layout = StarMapLayout::default();
    layout.nodes.push(StarMapLayoutNode {
        node_id: "n1".to_string(),
        x: 0.0,
        y: 0.0,
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
    store.set_viewport(StarMapViewport {
        scale: 1.0,
        offset_x: 0.0,
        offset_y: 0.0,
        width: 200.0,
        height: 200.0,
    });
    store.flush().unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    store2.ensure_loaded().unwrap();
    assert_eq!(
        store2.current_load_phase(),
        Some(LoadPhase::PrefetchNearbyObjects)
    );
    assert!(store2.get_node("n1").is_some());
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
    assert!(result.is_ok());
    assert!(!store2.diagnostics().is_empty());
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
fn viewport_culling_excludes_offscreen_nodes() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Visible"));
    store.upsert_node(make_test_node("n2", "Offscreen"));
    let mut layout = StarMapLayout::default();
    layout.nodes.push(StarMapLayoutNode {
        node_id: "n1".to_string(),
        x: 10.0,
        y: 10.0,
        width: 80.0,
        height: 40.0,
        radius: 20.0,
        collapsed: false,
        z_index: 0,
        scale: 1.0,
        depth: 0.0,
        focus_weight: 0.0,
        orbit_group: None,
    });
    layout.nodes.push(StarMapLayoutNode {
        node_id: "n2".to_string(),
        x: 5000.0,
        y: 5000.0,
        width: 80.0,
        height: 40.0,
        radius: 20.0,
        collapsed: false,
        z_index: 0,
        scale: 1.0,
        depth: 0.0,
        focus_weight: 0.0,
        orbit_group: None,
    });
    store.set_layout(layout);
    store.set_viewport(StarMapViewport {
        scale: 1.0,
        offset_x: 0.0,
        offset_y: 0.0,
        width: 200.0,
        height: 200.0,
    });
    store.flush().unwrap();
    store.flush_viewport().unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    let result = store2
        .load_phased(LoadPhase::CurrentViewportObjects)
        .unwrap();
    assert!(store2.get_node("n1").is_some());
    assert!(store2.get_node("n2").is_none());
    assert_eq!(result.loaded_node_count, 1);
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
fn viewport_saved_to_session_path() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.set_viewport(StarMapViewport {
        scale: 2.0,
        offset_x: 100.0,
        offset_y: 50.0,
        width: 800.0,
        height: 600.0,
    });
    store.flush_viewport().unwrap();

    let session_path = dir
        .path()
        .join("session")
        .join("starmaps")
        .join(&meta.starmap_id)
        .join("viewport.json");
    assert!(session_path.exists());

    let pkg_viewport = dir
        .path()
        .join("starmaps")
        .join(&meta.starmap_id)
        .join("viewport.json");
    assert!(!pkg_viewport.exists());
}
#[test]
fn prefetch_nearby_does_not_load_all_objects() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Visible"));
    store.upsert_node(make_test_node("n2", "Disconnected"));
    let mut layout = StarMapLayout::default();
    layout.nodes.push(StarMapLayoutNode {
        node_id: "n1".to_string(),
        x: 0.0,
        y: 0.0,
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
    store.set_viewport(StarMapViewport {
        scale: 1.0,
        offset_x: 0.0,
        offset_y: 0.0,
        width: 200.0,
        height: 200.0,
    });
    store.flush().unwrap();
    store.flush_viewport().unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    store2
        .load_phased(LoadPhase::PrefetchNearbyObjects)
        .unwrap();
    assert!(store2.get_node("n1").is_some());
    assert!(store2.get_node("n2").is_none());
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
fn ensure_object_loaded_for_edge_embed_link_hyperlink() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    let node = make_test_node("n1", "Node1");
    store.upsert_node(node.clone());
    let edge = StarMapEdge {
        id: "e1".to_string(),
        from: Some("n1".to_string()),
        to: Some("n1".to_string()),
        kind: StarMapEdgeKind::RelatedTo,
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

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    let graph_meta = GraphMeta {
        schema_version: "2".to_string(),
        starmap_id: meta.starmap_id.clone(),
        title: "Test".to_string(),
        node_ids: vec![],
        edge_ids: vec![],
        embed_instance_ids: vec![],
        link_ids: vec!["missing-link".to_string()],
        hyperlink_ids: vec![],
        edge_relation_index: vec![],
        embed_host_index: vec![],
        link_relation_index: vec![],
        hyperlink_relation_index: vec![],
        node_kind_counts: HashMap::new(),
        package_revision: 0,
        updated_at: 0,
        deleted_since_last_sync: DeletedSinceLastSync::default(),
    };
    store.graph_meta = Some(graph_meta);
    store.flush().unwrap();

    let result = store.list_links_with_diagnostics();
    assert!(
        !result.diagnostics.is_empty(),
        "should report missing link as diagnostic"
    );
    assert_eq!(result.diagnostics[0].object_id, "missing-link");
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
            from: Some(format!("n{}", i)),
            to: Some(format!("n{}", i + 1)),
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

    fresh.prefetch_nearby_objects(&mut vec![]);

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
    use crate::starmap::types::{
        StarMapEdge, StarMapEdgeEndpoint, StarMapEdgeKind, StarMapEndpointPath,
    };
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Node1"));
    store.upsert_node(make_test_node("n2", "Node2"));
    store.upsert_edge(StarMapEdge {
        id: "e1".to_string(),
        from: Some("n1".to_string()),
        to: Some("n2".to_string()),
        kind: StarMapEdgeKind::References,
        label: None,
        payload: None,
        from_target: None,
        to_target: None,
        from_endpoint: Some(StarMapEdgeEndpoint::Node {
            node_id: "n1".to_string(),
        }),
        to_endpoint: Some(StarMapEdgeEndpoint::Anchor {
            node_id: "n2".to_string(),
            anchor_id: "a1".to_string(),
        }),
        from_endpoint_path: None,
        to_endpoint_path: Some(StarMapEndpointPath {
            segments: vec![],
            endpoint: StarMapEdgeEndpoint::Node {
                node_id: "n2".to_string(),
            },
        }),
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
        eri.from_endpoint.is_some(),
        "from_endpoint should be preserved in index"
    );
    assert!(
        eri.to_endpoint.is_some(),
        "to_endpoint should be preserved in index"
    );
    assert!(
        eri.to_endpoint_path.is_some(),
        "to_endpoint_path should be preserved in index"
    );
}
#[test]
fn embed_host_index_preserves_host_endpoint() {
    use crate::starmap::semantic::{StarMapDisplayPolicy, StarMapOpenBehavior, StarMapProvenance};
    use crate::starmap::types::{StarMapEmbed, StarMapEndpoint};
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Node1"));
    store.upsert_embed(StarMapEmbed {
        instance_id: "emb1".to_string(),
        target_starmap_id: "sm-child".to_string(),
        label: None,
        display_policy: StarMapDisplayPolicy::default(),
        open_behavior: StarMapOpenBehavior::default(),
        placement: Default::default(),
        target_viewport: Default::default(),
        source_node_id: Some("n1".to_string()),
        host_endpoint: Some(StarMapEndpoint::Anchor {
            node_id: "n1".to_string(),
            anchor_id: "a1".to_string(),
        }),
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
        ehi.host_endpoint.is_some(),
        "host_endpoint should be preserved in index"
    );
}
#[test]
fn list_links_with_diagnostics_returns_corrupt_for_bad_file() {
    use crate::starmap::semantic::{StarMapDeepTarget, StarMapTargetDetail};
    use crate::starmap::types::{StarMapEndpoint, StarMapLink};
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let link = StarMapLink {
        link_id: "l1".to_string(),
        source: StarMapEndpoint::Node {
            node_id: "n1".to_string(),
        },
        target: StarMapDeepTarget {
            starmap_id: meta.starmap_id.clone(),
            path: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "other".to_string(),
            },
        },
        label: None,
        created_at: 0,
        updated_at: 0,
    };
    package_storage::save_link(dir.path(), &meta.starmap_id, &link).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_link(link.clone());
    store.enqueue_save(SaveQueueEntry::GraphMeta);
    store.flush().unwrap();

    let starmap_dir = dir
        .path()
        .join("starmaps")
        .join(&meta.starmap_id);
    let bucket = package_storage::bucket_for_id("l1");
    let link_bucket = starmap_dir.join("links").join(bucket).join("l1.json");
    let link_flat = starmap_dir.join("links").join("l1.json");
    let link_path = if link_bucket.exists() {
        link_bucket
    } else {
        link_flat
    };
    assert!(
        link_path.exists(),
        "link file should exist at {:?}",
        link_path
    );
    std::fs::write(&link_path, "THIS_IS_NOT_JSON_AT_ALL").unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    let graph_json_path = starmap_dir.join("graph.json");
    let content = std::fs::read_to_string(&graph_json_path).unwrap();
    let gm: GraphMeta = serde_json::from_str(&content).unwrap();
    store2.graph_meta = Some(gm);
    store2.current_load_phase = Some(LoadPhase::GraphMeta);

    let result = store2.list_links_with_diagnostics();
    assert!(
        !result.diagnostics.is_empty(),
        "should have diagnostics for corrupt link, got {} diagnostics",
        result.diagnostics.len()
    );
    assert_eq!(
        result.diagnostics[0].kind,
        LoadDiagnosticKind::Corrupt,
        "should report Corrupt not Missing"
    );
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
        from: Some("n1".to_string()),
        to: Some("n2".to_string()),
        kind: crate::starmap::types::StarMapEdgeKind::RelatedTo,
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
    };
    store.upsert_edge(edge);

    store.flush_save_queue().unwrap();
    store.update_graph_meta_file().unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    let result = store2.load_phased(LoadPhase::PrefetchNearbyObjects);
    assert!(
        result.is_ok(),
        "load_phased must not stack overflow: {:?}",
        result
    );
}
