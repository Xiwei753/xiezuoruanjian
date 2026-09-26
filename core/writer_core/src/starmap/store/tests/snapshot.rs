use super::super::*;
use super::*;
use tempfile::TempDir;

fn setup_temp_dir() -> (TempDir, String) {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();
    (dir, meta.starmap_id)
}

fn make_test_edge(id: &str, from: &str, to: &str) -> StarMapEdge {
    use crate::starmap::semantic::StarMapTargetDetail;
    use crate::starmap::types::reference::StarMapTargetPath;
    StarMapEdge {
        id: id.to_string(),
        from: StarMapTargetPath {
            starmap_id: "self".to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: from.to_string(),
            },
        },
        to: StarMapTargetPath {
            starmap_id: "self".to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: to.to_string(),
            },
        },
        kind: StarMapEdgeKind::RelatedTo,
        label: None,
        payload: None,
        created_at: 0,
        updated_at: 0,
    }
}

fn flush_store(store: &mut StarMapStore) {
    store.enqueue_save(SaveQueueEntry::Node);
    store.enqueue_save(SaveQueueEntry::Edge);
    store.enqueue_save(SaveQueueEntry::Embed);
    store.enqueue_save(SaveQueueEntry::Link);
    store.enqueue_save(SaveQueueEntry::Hyperlink);
    store.enqueue_save(SaveQueueEntry::Layout);
    store.enqueue_save(SaveQueueEntry::GraphMeta);
    store.flush_save_queue().unwrap();
}

fn load_full_snapshot(dir: &std::path::Path, sid: &str) -> StarMapPhasedSnapshot {
    let mut store = StarMapStore::new(dir, sid);
    store.load_phased(LoadPhase::BackgroundFullLoad).unwrap();
    let request = PhasedSnapshotRequest {
        target_phase: LoadPhase::BackgroundFullLoad,
        since_revision: 0,
    };
    store.get_phased_snapshot(&request).unwrap()
}

#[test]
fn phased_snapshot_includes_load_phase_and_revision() {
    let (dir, sid) = setup_temp_dir();
    let mut store = StarMapStore::new(dir.path(), &sid);
    store.upsert_node(make_test_node("n1", "A"));
    flush_store(&mut store);
    let snapshot = load_full_snapshot(dir.path(), &sid);
    assert_eq!(snapshot.load_phase, LoadPhase::BackgroundFullLoad);
    assert!(snapshot.complete);
    assert!(snapshot.package_revision >= 1);
}

#[test]
fn phased_snapshot_complete_at_background_full_load() {
    let (dir, sid) = setup_temp_dir();
    let mut store = StarMapStore::new(dir.path(), &sid);
    store.upsert_node(make_test_node("n1", "A"));
    flush_store(&mut store);
    let snapshot = load_full_snapshot(dir.path(), &sid);
    assert!(snapshot.complete);
}

#[test]
fn phased_snapshot_preserves_layout_after_disk_roundtrip() {
    let (dir, sid) = setup_temp_dir();
    let mut store = StarMapStore::new(dir.path(), &sid);
    store.upsert_node(make_test_node("n1", "Node1"));
    store.set_layout(StarMapLayout {
        kind: StarMapLayoutKind::Freeform,
        nodes: vec![StarMapLayoutNode {
            node_id: "n1".to_string(),
            x: 100.0,
            y: 200.0,
            width: 150.0,
            height: 80.0,
            radius: 40.0,
            collapsed: false,
            z_index: 0,
            scale: 1.0,
            depth: 0.0,
            focus_weight: 1.0,
            orbit_group: None,
        }],
    });
    flush_store(&mut store);
    let snapshot = load_full_snapshot(dir.path(), &sid);
    let l = snapshot.layout.as_ref().expect("layout should be present");
    assert_eq!(l.nodes.len(), 1);
    assert!((l.nodes[0].x - 100.0).abs() < f32::EPSILON);
    assert!((l.nodes[0].y - 200.0).abs() < f32::EPSILON);
}

#[test]
fn phased_snapshot_preserves_edge_endpoint_semantics() {
    let (dir, sid) = setup_temp_dir();
    let mut store = StarMapStore::new(dir.path(), &sid);
    store.upsert_node(make_test_node("n1", "A"));
    store.upsert_node(make_test_node("n2", "B"));
    use crate::starmap::semantic::StarMapTargetDetail;
    use crate::starmap::types::reference::StarMapTargetPath;
    store.upsert_edge(StarMapEdge {
        id: "e1".to_string(),
        from: StarMapTargetPath {
            starmap_id: sid.clone(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n1".to_string(),
            },
        },
        to: StarMapTargetPath {
            starmap_id: sid.clone(),
            segments: vec![],
            target: StarMapTargetDetail::Anchor {
                node_id: "n2".to_string(),
                anchor_id: "a1".to_string(),
            },
        },
        kind: StarMapEdgeKind::References,
        label: Some("ref".to_string()),
        payload: None,
        created_at: 0,
        updated_at: 0,
    });
    flush_store(&mut store);
    let snapshot = load_full_snapshot(dir.path(), &sid);
    let e = &snapshot.edges[0];
    assert_eq!(e.kind, StarMapEdgeKind::References);
    assert_eq!(e.label.as_deref(), Some("ref"));
    match &e.from.target {
        StarMapTargetDetail::Node { node_id } => assert_eq!(node_id, "n1"),
        _ => panic!("expected Node"),
    }
    match &e.to.target {
        StarMapTargetDetail::Anchor { node_id, anchor_id } => {
            assert_eq!(node_id, "n2");
            assert_eq!(anchor_id, "a1");
        }
        _ => panic!("expected Anchor"),
    }
}

#[test]
fn phased_snapshot_preserves_embed_link_hyperlink() {
    let (dir, sid) = setup_temp_dir();
    let mut store = StarMapStore::new(dir.path(), &sid);
    store.upsert_node(make_test_node("n1", "Host"));
    use crate::starmap::semantic::{
        StarMapDisplayPolicy, StarMapOpenBehavior, StarMapProvenance, StarMapTargetDetail,
    };
    use crate::starmap::types::reference::StarMapTargetPath;
    store.upsert_embed(StarMapEmbed {
        instance_id: "emb1".to_string(),
        target_starmap_id: "child".to_string(),
        label: Some("child".to_string()),
        display_policy: StarMapDisplayPolicy::default(),
        open_behavior: StarMapOpenBehavior::default(),
        placement: StarMapEmbedPlacement {
            x: 10.0,
            y: 20.0,
            width: 200.0,
            height: 150.0,
            scale: 1.0,
            z_index: 0,
            collapsed: false,
        },
        target_viewport: StarMapEmbedViewport {
            scale: 1.0,
            offset_x: 0.0,
            offset_y: 0.0,
        },
        host_path: StarMapTargetPath {
            starmap_id: sid.clone(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n1".to_string(),
            },
        },
        provenance: StarMapProvenance::default(),
        created_at: 0,
        updated_at: 0,
    });
    store.upsert_link(StarMapLink {
        link_id: "lk1".to_string(),
        source: StarMapTargetPath {
            starmap_id: sid.clone(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n1".to_string(),
            },
        },
        target: StarMapTargetPath {
            starmap_id: "other".to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Starmap,
        },
        label: Some("link".to_string()),
        created_at: 0,
        updated_at: 0,
    });
    store.upsert_hyperlink(StarMapHyperlink {
        hyperlink_id: "hl1".to_string(),
        source: StarMapTargetPath {
            starmap_id: sid.clone(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n1".to_string(),
            },
        },
        target_uri: "https://example.com".to_string(),
        label: Some("hl".to_string()),
        target_starmap_id: Some("tgt".to_string()),
        created_at: 0,
        updated_at: 0,
    });
    flush_store(&mut store);
    let snapshot = load_full_snapshot(dir.path(), &sid);
    assert_eq!(snapshot.embeds.len(), 1);
    assert_eq!(snapshot.embeds[0].instance_id, "emb1");
    assert!((snapshot.embeds[0].placement.x - 10.0).abs() < f32::EPSILON);
    assert_eq!(snapshot.links.len(), 1);
    assert_eq!(snapshot.links[0].link_id, "lk1");
    assert_eq!(snapshot.hyperlinks.len(), 1);
    assert_eq!(snapshot.hyperlinks[0].hyperlink_id, "hl1");
    assert_eq!(snapshot.hyperlinks[0].target_uri, "https://example.com");
}

#[test]
fn phased_snapshot_incremental_by_revision() {
    let (dir, sid) = setup_temp_dir();
    let mut store = StarMapStore::new(dir.path(), &sid);
    store.upsert_node(make_test_node("n1", "A"));
    flush_store(&mut store);
    let rev1 = store.package_revision();
    assert!(rev1 >= 1);

    store.upsert_node(make_test_node("n2", "B"));
    flush_store(&mut store);
    let rev2 = store.package_revision();
    assert!(rev2 > rev1);

    let request = PhasedSnapshotRequest {
        target_phase: LoadPhase::BackgroundFullLoad,
        since_revision: rev1,
    };
    let snap = store.get_phased_snapshot(&request).unwrap();
    assert_eq!(snap.package_revision, rev2);
    assert_eq!(snap.nodes.len(), 2);
}

#[test]
fn phased_snapshot_preserves_node_anchor_semantics() {
    use crate::starmap::semantic::{
        StarMapAnchor, StarMapAnchorRole, StarMapAnchorTarget, StarMapDisplayPolicy,
        StarMapNodeContent, StarMapOpenBehavior, StarMapProvenance,
    };
    let (dir, sid) = setup_temp_dir();
    let mut store = StarMapStore::new(dir.path(), &sid);
    store.upsert_node(StarMapNode {
        id: "n1".to_string(),
        title: "Char".to_string(),
        kind: StarMapNodeKind::Character,
        payload: None,
        tags: vec!["protagonist".to_string()],
        content: StarMapNodeContent::Empty,
        anchors: vec![StarMapAnchor {
            anchor_id: "a1".to_string(),
            target: StarMapAnchorTarget::Chapter {
                project_id: None,
                volume_id: None,
                chapter_id: "ch1".to_string(),
            },
            label: Some("ch_ref".to_string()),
            role: StarMapAnchorRole::Source,
        }],
        portal: None,
        display_policy: StarMapDisplayPolicy::default(),
        open_behavior: StarMapOpenBehavior::default(),
        provenance: StarMapProvenance::default(),
        created_at: 0,
        updated_at: 0,
    });
    flush_store(&mut store);
    let snapshot = load_full_snapshot(dir.path(), &sid);
    let n = &snapshot.nodes[0];
    assert_eq!(n.kind, StarMapNodeKind::Character);
    assert_eq!(n.tags, vec!["protagonist".to_string()]);
    assert_eq!(n.anchors.len(), 1);
    assert_eq!(n.anchors[0].anchor_id, "a1");
    assert_eq!(n.anchors[0].role, StarMapAnchorRole::Source);
}

#[test]
fn phased_snapshot_returns_empty_when_revision_unchanged() {
    let (dir, sid) = setup_temp_dir();
    let mut store = StarMapStore::new(dir.path(), &sid);
    store.upsert_node(make_test_node("n1", "A"));
    flush_store(&mut store);
    let rev = store.package_revision();

    let request = PhasedSnapshotRequest {
        target_phase: LoadPhase::BackgroundFullLoad,
        since_revision: rev,
    };
    let snap = store.get_phased_snapshot(&request).unwrap();
    assert_eq!(snap.since_revision, rev);
    assert_eq!(snap.package_revision, rev);
    assert!(snap.nodes.is_empty());
    assert!(snap.edges.is_empty());
    assert!(snap.embeds.is_empty());
    assert!(snap.links.is_empty());
    assert!(snap.hyperlinks.is_empty());
}

#[test]
fn phased_snapshot_returns_objects_when_revision_advanced() {
    let (dir, sid) = setup_temp_dir();
    let mut store = StarMapStore::new(dir.path(), &sid);
    store.upsert_node(make_test_node("n1", "A"));
    flush_store(&mut store);
    let rev1 = store.package_revision();

    store.upsert_node(make_test_node("n2", "B"));
    flush_store(&mut store);
    let rev2 = store.package_revision();
    assert!(rev2 > rev1);

    let request = PhasedSnapshotRequest {
        target_phase: LoadPhase::BackgroundFullLoad,
        since_revision: rev1,
    };
    let snap = store.get_phased_snapshot(&request).unwrap();
    assert_eq!(snap.since_revision, rev1);
    assert_eq!(snap.package_revision, rev2);
    assert_eq!(snap.nodes.len(), 2);
}

#[test]
fn phased_snapshot_since_revision_zero_returns_all() {
    let (dir, sid) = setup_temp_dir();
    let mut store = StarMapStore::new(dir.path(), &sid);
    store.upsert_node(make_test_node("n1", "A"));
    flush_store(&mut store);

    let request = PhasedSnapshotRequest {
        target_phase: LoadPhase::BackgroundFullLoad,
        since_revision: 0,
    };
    let snap = store.get_phased_snapshot(&request).unwrap();
    assert_eq!(snap.since_revision, 0);
    assert_eq!(snap.nodes.len(), 1);
}

#[test]
fn phased_snapshot_incremental_preserves_layout_and_viewport() {
    let (dir, sid) = setup_temp_dir();
    let mut store = StarMapStore::new(dir.path(), &sid);
    store.upsert_node(make_test_node("n1", "A"));
    store.set_layout(StarMapLayout {
        kind: StarMapLayoutKind::Freeform,
        nodes: vec![StarMapLayoutNode {
            node_id: "n1".to_string(),
            x: 50.0,
            y: 60.0,
            width: 100.0,
            height: 80.0,
            radius: 30.0,
            collapsed: false,
            z_index: 0,
            scale: 1.0,
            depth: 0.0,
            focus_weight: 1.0,
            orbit_group: None,
        }],
    });
    store.set_viewport(StarMapViewport {
        scale: 1.0,
        offset_x: 0.0,
        offset_y: 0.0,
        width: 800.0,
        height: 600.0,
    });
    flush_store(&mut store);
    store.flush_viewport().unwrap();
    let rev = store.package_revision();

    let request = PhasedSnapshotRequest {
        target_phase: LoadPhase::BackgroundFullLoad,
        since_revision: rev,
    };
    let snap = store.get_phased_snapshot(&request).unwrap();
    assert!(snap.nodes.is_empty());
    assert!(snap.layout.is_some());
    assert!(snap.viewport.is_some());
}

#[test]
fn phased_snapshot_phases_have_increasing_object_counts() {
    let (dir, sid) = setup_temp_dir();
    let mut store = StarMapStore::new(dir.path(), &sid);
    store.upsert_node(make_test_node("n1", "InView"));
    store.upsert_node(make_test_node("n2", "Nearby"));
    store.upsert_edge(make_test_edge("e1", "n1", "n2"));
    store.set_layout(StarMapLayout {
        kind: StarMapLayoutKind::Freeform,
        nodes: vec![StarMapLayoutNode {
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
            focus_weight: 1.0,
            orbit_group: None,
        }],
    });
    store.set_viewport(StarMapViewport {
        scale: 1.0,
        offset_x: 0.0,
        offset_y: 0.0,
        width: 800.0,
        height: 600.0,
    });
    flush_store(&mut store);

    // Use a fresh store instance so GraphMeta phase doesn't have in-memory nodes
    let mut fresh_store = StarMapStore::new(dir.path(), &sid);
    let request = PhasedSnapshotRequest {
        target_phase: LoadPhase::GraphMeta,
        since_revision: 0,
    };
    let snap = fresh_store.get_phased_snapshot(&request).unwrap();
    assert!(snap.nodes.is_empty());

    let request = PhasedSnapshotRequest {
        target_phase: LoadPhase::BackgroundFullLoad,
        since_revision: 0,
    };
    let snap = fresh_store.get_phased_snapshot(&request).unwrap();
    assert!(!snap.nodes.is_empty());
    assert!(!snap.edges.is_empty());
}
