use super::super::*;
use super::*;
use crate::starmap::semantic::StarMapTargetDetail;
use crate::starmap::types::reference::StarMapTargetPath;
use tempfile::TempDir;

#[test]
fn upsert_node_marks_dirty() {
    let dir = TempDir::new().unwrap();
    let mut store = StarMapStore::new(dir.path(), "test-id");
    store.upsert_node(make_test_node("n1", "Test Node"));
    assert!(store.is_dirty());
    assert_eq!(store.node_count(), 1);
    assert!(store.get_node("n1").is_some());
}

#[test]
fn remove_node_marks_deleted() {
    let dir = TempDir::new().unwrap();
    let mut store = StarMapStore::new(dir.path(), "test-id");
    store.upsert_node(make_test_node("n1", "Test Node"));
    store.remove_node("n1");
    assert_eq!(store.node_count(), 0);
    assert!(store.get_node("n1").is_none());
}

#[test]
fn link_save_reload_update_delete_round_trip() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    let link = make_test_link("l1", "Test Link");
    store.upsert_link(link.clone());
    store.flush().unwrap();
    assert_eq!(store.link_count(), 1);
    assert!(store.get_link("l1").is_some());

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    let result = store2.load_full().unwrap();
    assert_eq!(result.loaded_link_count, 1);
    assert!(store2.get_link("l1").is_some());
    assert_eq!(
        store2.get_link("l1").unwrap().label.as_deref(),
        Some("Test Link")
    );

    let patch = StarMapLinkPatch {
        source: None,
        target: None,
        label: Some(Some("Updated Link".to_string())),
    };
    store2.update_link("l1", &patch).unwrap();
    store2.flush().unwrap();

    let mut store3 = StarMapStore::new(dir.path(), &meta.starmap_id);
    store3.load_full().unwrap();
    assert_eq!(store3.link_count(), 1);
    assert_eq!(
        store3.get_link("l1").unwrap().label.as_deref(),
        Some("Updated Link")
    );

    store3.delete_link("l1").unwrap();
    store3.flush().unwrap();

    let mut store4 = StarMapStore::new(dir.path(), &meta.starmap_id);
    let result4 = store4.load_full().unwrap();
    assert_eq!(result4.loaded_link_count, 0);
    assert!(store4.get_link("l1").is_none());
}

fn make_simple_edge(id: &str, from: &str, to: &str, sid: &str) -> StarMapEdge {
    StarMapEdge {
        id: id.to_string(),
        from: StarMapTargetPath {
            starmap_id: sid.to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: from.to_string(),
            },
        },
        to: StarMapTargetPath {
            starmap_id: sid.to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: to.to_string(),
            },
        },
        kind: StarMapEdgeKind::References,
        label: None,
        payload: None,
        created_at: 0,
        updated_at: 0,
    }
}

#[test]
fn upsert_edge_existing_marks_dirty_graph_meta() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    let node1 = make_test_node("n1", "A");
    let node2 = make_test_node("n2", "B");
    let node3 = make_test_node("n3", "C");
    store.upsert_node(node1);
    store.upsert_node(node2);
    store.upsert_node(node3);

    let edge = make_simple_edge("e1", "n1", "n2", &meta.starmap_id);
    store.upsert_edge(edge);
    store.flush().unwrap();

    let edge_updated = make_simple_edge("e1", "n1", "n3", &meta.starmap_id);
    store.upsert_edge(edge_updated);
    assert!(
        store.dirty_graph_meta,
        "upsert_edge on existing edge should mark dirty_graph_meta because relation index changed"
    );

    store.flush().unwrap();
    let meta_on_disk: GraphMeta = serde_json::from_str(
        &std::fs::read_to_string(store.starmap_dir().join("graph.json")).unwrap(),
    )
    .unwrap();
    let eri = meta_on_disk
        .edge_relation_index
        .iter()
        .find(|e| e.edge_id == "e1")
        .unwrap();
    match &eri.to.target {
        StarMapTargetDetail::Node { node_id } => {
            assert_eq!(node_id, "n3");
        }
        _ => panic!("expected Node"),
    }
}

#[test]
fn update_edge_endpoint_marks_dirty_graph_meta() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "A"));
    store.upsert_node(make_test_node("n2", "B"));
    store.upsert_node(make_test_node("n3", "C"));

    let edge = make_simple_edge("e1", "n1", "n2", &meta.starmap_id);
    store.upsert_edge(edge);
    store.flush().unwrap();

    let patch = StarMapEdgePatch {
        kind: None,
        label: None,
        payload: None,
        from: Some(StarMapTargetPath {
            starmap_id: meta.starmap_id.clone(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n3".to_string(),
            },
        }),
        to: None,
    };
    store.update_edge("e1", &patch).unwrap();
    assert!(
        store.dirty_graph_meta,
        "update_edge changing from should mark dirty_graph_meta"
    );

    store.flush().unwrap();
    let meta_on_disk: GraphMeta = serde_json::from_str(
        &std::fs::read_to_string(store.starmap_dir().join("graph.json")).unwrap(),
    )
    .unwrap();
    let eri = meta_on_disk
        .edge_relation_index
        .iter()
        .find(|e| e.edge_id == "e1")
        .unwrap();
    match &eri.from.target {
        StarMapTargetDetail::Node { node_id } => {
            assert_eq!(node_id, "n3");
        }
        _ => panic!("expected Node"),
    }
}

#[test]
fn update_embed_host_marks_dirty_graph_meta() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "A"));
    store.upsert_node(make_test_node("n2", "B"));

    use crate::starmap::semantic::{
        StarMapDisplayPolicy, StarMapOpenBehavior, StarMapProvenance, StarMapTargetDetail,
    };
    let embed = StarMapEmbed {
        instance_id: "emb1".to_string(),
        target_starmap_id: "other".to_string(),
        label: None,
        display_policy: StarMapDisplayPolicy::default(),
        open_behavior: StarMapOpenBehavior::default(),
        placement: StarMapEmbedPlacement::default(),
        target_viewport: StarMapEmbedViewport::default(),
        host_path: StarMapTargetPath {
            starmap_id: meta.starmap_id.clone(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n1".to_string(),
            },
        },
        provenance: StarMapProvenance::default(),
        created_at: 0,
        updated_at: 0,
    };
    store.upsert_embed(embed);
    store.flush().unwrap();

    let patch = StarMapEmbedPatch {
        label: None,
        display_policy: None,
        open_behavior: None,
        placement: None,
        target_viewport: None,
        host_path: Some(StarMapTargetPath {
            starmap_id: meta.starmap_id.clone(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n2".to_string(),
            },
        }),
    };
    store.update_embed("emb1", &patch).unwrap();
    assert!(
        store.dirty_graph_meta,
        "update_embed changing host_path should mark dirty_graph_meta"
    );

    store.flush().unwrap();
    let meta_on_disk: GraphMeta = serde_json::from_str(
        &std::fs::read_to_string(store.starmap_dir().join("graph.json")).unwrap(),
    )
    .unwrap();
    let ehi = meta_on_disk
        .embed_host_index
        .iter()
        .find(|e| e.instance_id == "emb1")
        .unwrap();
    match &ehi.host_path.target {
        StarMapTargetDetail::Node { node_id } => {
            assert_eq!(node_id, "n2");
        }
        _ => panic!("expected Node"),
    }
}

#[test]
fn delete_also_removes_flat_path() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let starmap_dir = dir.path().join("starmaps").join(&meta.starmap_id);
    let nodes_dir = starmap_dir.join("nodes");
    std::fs::create_dir_all(&nodes_dir).unwrap();

    let flat_path = nodes_dir.join("n1.json");
    std::fs::write(&flat_path, "{}").unwrap();
    assert!(flat_path.exists(), "flat file should exist before delete");

    package_storage::delete_node_file(dir.path(), &meta.starmap_id, "n1").unwrap();
    assert!(
        !flat_path.exists(),
        "flat file should be removed by delete_node_file"
    );
}

#[test]
fn add_link_updates_graph_meta_link_ids() {
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

    // graph_meta indexes are rebuilt during flush, not immediately after CRUD
    store.flush().unwrap();
    let meta_ids = store.graph_meta.as_ref().unwrap().link_ids.clone();
    assert!(
        meta_ids.contains(&"l1".to_string()),
        "after flush, graph_meta.link_ids must contain the link_id"
    );

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

#[test]
fn delete_link_updates_graph_meta_link_ids() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    let link = make_test_link("l1", "Test");
    store.add_link(link).unwrap();
    store.flush().unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    store2.load_full().unwrap();
    store2.delete_link("l1").unwrap();
    assert!(
        store2.deleted_link_ids.contains("l1"),
        "delete_link must mark deleted_link_ids"
    );
    assert!(
        store2.dirty_graph_meta,
        "delete_link must mark dirty_graph_meta"
    );

    // graph_meta indexes are rebuilt during flush, not immediately after CRUD
    store2.flush().unwrap();
    assert!(
        !store2
            .graph_meta
            .as_ref()
            .unwrap()
            .link_ids
            .contains(&"l1".to_string()),
        "after flush, graph_meta.link_ids must not contain deleted link_id"
    );

    let mut store3 = StarMapStore::new(dir.path(), &meta.starmap_id);
    store3.load_full().unwrap();
    assert_eq!(store3.link_count(), 0);
    assert!(
        !store3
            .graph_meta
            .as_ref()
            .unwrap()
            .link_ids
            .contains(&"l1".to_string()),
        "link_id must be removed from graph.json after flush"
    );
}

#[test]
fn hyperlink_add_update_delete_round_trip() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.load_full().unwrap();
    store.upsert_hyperlink(StarMapHyperlink {
        hyperlink_id: "hl1".to_string(),
        source: StarMapTargetPath {
            starmap_id: meta.starmap_id.clone(),
            segments: vec![],
            target: StarMapTargetDetail::Starmap,
        },
        target_uri: "https://example.com".to_string(),
        label: Some("Example".to_string()),
        target_starmap_id: None,
        created_at: 0,
        updated_at: 0,
    });
    store.flush().unwrap();
    assert_eq!(store.hyperlink_count(), 1);

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    store2.load_full().unwrap();
    assert_eq!(store2.hyperlink_count(), 1);
    let hl = store2.get_hyperlink("hl1").unwrap();
    assert_eq!(hl.target_uri, "https://example.com");

    store2
        .update_hyperlink("hl1", Some("Updated"), None)
        .unwrap();
    store2.flush().unwrap();

    let mut store3 = StarMapStore::new(dir.path(), &meta.starmap_id);
    store3.load_full().unwrap();
    assert_eq!(
        store3.get_hyperlink("hl1").unwrap().label.as_deref(),
        Some("Updated")
    );

    store3.delete_hyperlink("hl1").unwrap();
    store3.flush().unwrap();

    let mut store4 = StarMapStore::new(dir.path(), &meta.starmap_id);
    store4.load_full().unwrap();
    assert_eq!(store4.hyperlink_count(), 0);
}
