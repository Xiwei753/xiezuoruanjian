use super::super::*;
use super::*;
use crate::starmap::types::reference::{StarMapTargetDetail, StarMapTargetPath};
use tempfile::TempDir;

#[test]
fn merge_memory_ids_updates_edge_endpoint_in_index() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Node1"));
    store.upsert_node(make_test_node("n2", "Node2"));
    store.upsert_node(make_test_node("n3", "Node3"));

    let edge = crate::starmap::types::StarMapEdge {
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
        kind: crate::starmap::types::StarMapEdgeKind::RelatedTo,
        label: None,
        payload: None,
        created_at: 0,
        updated_at: 0,
    };
    store.upsert_edge(edge.clone());
    store.flush().unwrap();

    let updated_edge = crate::starmap::types::StarMapEdge {
        from: StarMapTargetPath {
            starmap_id: String::new(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n3".to_string(),
            },
        },
        ..edge
    };
    store.upsert_edge(updated_edge);
    store.flush().unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    let _ = store2.load_full();
    let meta2 = store2.graph_meta.as_ref().unwrap();
    let eri = meta2
        .edge_relation_index
        .iter()
        .find(|e| e.edge_id == "e1")
        .unwrap();
    assert_eq!(
        eri.from,
        StarMapTargetPath {
            starmap_id: String::new(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n3".to_string()
            },
        },
        "edge_relation_index should reflect updated endpoint"
    );
    assert_eq!(
        eri.to,
        StarMapTargetPath {
            starmap_id: String::new(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n2".to_string()
            },
        }
    );
}
#[test]
fn merge_memory_ids_updates_embed_host_in_index() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "Node1"));
    store.upsert_node(make_test_node("n2", "Node2"));

    let embed = crate::starmap::types::StarMapEmbed {
        instance_id: "em1".to_string(),
        target_starmap_id: String::new(),
        label: None,
        display_policy: crate::starmap::semantic::StarMapDisplayPolicy::default(),
        open_behavior: crate::starmap::semantic::StarMapOpenBehavior::default(),
        placement: crate::starmap::types::StarMapEmbedPlacement::default(),
        target_viewport: crate::starmap::types::StarMapEmbedViewport::default(),
        host_path: StarMapTargetPath {
            starmap_id: String::new(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n1".to_string(),
            },
        },
        provenance: crate::starmap::semantic::StarMapProvenance::default(),
        created_at: 0,
        updated_at: 0,
    };
    store.upsert_embed(embed.clone());
    store.flush().unwrap();

    let updated_embed = crate::starmap::types::StarMapEmbed {
        host_path: StarMapTargetPath {
            starmap_id: String::new(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n2".to_string(),
            },
        },
        ..embed
    };
    store.upsert_embed(updated_embed);
    store.flush().unwrap();

    let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
    let _ = store2.load_full();
    let meta2 = store2.graph_meta.as_ref().unwrap();
    let ehi = meta2
        .embed_host_index
        .iter()
        .find(|e| e.instance_id == "em1")
        .unwrap();
    assert_eq!(
        ehi.host_path,
        StarMapTargetPath {
            starmap_id: String::new(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n2".to_string()
            },
        },
        "embed_host_index should reflect updated host"
    );
}
#[test]
fn merge_memory_ids_removes_deleted_ids() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "A"));
    store.upsert_node(make_test_node("n2", "B"));
    store.upsert_node(make_test_node("n3", "C"));
    store.flush().unwrap();

    store.remove_node("n2");
    store.remove_node("n3");
    assert!(store.deleted_node_ids.contains("n2"));
    assert!(store.deleted_node_ids.contains("n3"));

    store.merge_memory_ids_into_graph_meta();
    let meta_ids = store.graph_meta.as_ref().unwrap().node_ids.clone();
    assert!(meta_ids.contains(&"n1".to_string()), "n1 should remain");
    assert!(
        !meta_ids.contains(&"n2".to_string()),
        "n2 should be removed from graph_meta"
    );
    assert!(
        !meta_ids.contains(&"n3".to_string()),
        "n3 should be removed from graph_meta"
    );
}
#[test]
fn merge_memory_ids_skips_deleted_edge_in_index() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
    store.upsert_node(make_test_node("n1", "A"));
    store.upsert_node(make_test_node("n2", "B"));
    store.upsert_node(make_test_node("n3", "C"));

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

    store.remove_edge("e1");
    assert!(store.deleted_edge_ids.contains("e1"));

    store.merge_memory_ids_into_graph_meta();
    let meta = store.graph_meta.as_ref().unwrap();
    assert!(
        !meta.edge_ids.contains(&"e1".to_string()),
        "deleted edge should be removed from edge_ids"
    );
    assert!(
        !meta.edge_relation_index.iter().any(|e| e.edge_id == "e1"),
        "deleted edge should be removed from relation index"
    );
}
