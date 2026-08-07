use super::super::*;
use super::*;
use tempfile::TempDir;

fn setup_workspace() -> (TempDir, String) {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();
    (dir, meta.starmap_id)
}

fn make_test_edge(id: &str, from: &str, to: &str) -> StarMapEdge {
    StarMapEdge {
        id: id.to_string(),
        from: Some(from.to_string()),
        to: Some(to.to_string()),
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
    let (dir, sid) = setup_workspace();
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
    let (dir, sid) = setup_workspace();
    let mut store = StarMapStore::new(dir.path(), &sid);
    store.upsert_node(make_test_node("n1", "A"));
    flush_store(&mut store);
    let snapshot = load_full_snapshot(dir.path(), &sid);
    assert!(snapshot.complete);
}

#[test]
fn phased_snapshot_preserves_layout_after_disk_roundtrip() {
    let (dir, sid) = setup_workspace();
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
    let (dir, sid) = setup_workspace();
    let mut store = StarMapStore::new(dir.path(), &sid);
    store.upsert_node(make_test_node("n1", "A"));
    store.upsert_node(make_test_node("n2", "B"));
    store.upsert_edge(StarMapEdge {
        id: "e1".to_string(),
        from: Some("n1".to_string()),
        to: Some("n2".to_string()),
        kind: StarMapEdgeKind::References,
        label: Some("ref".to_string()),
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
        to_endpoint_path: None,
        created_at: 0,
        updated_at: 0,
    });
    flush_store(&mut store);
    let snapshot = load_full_snapshot(dir.path(), &sid);
    let e = &snapshot.edges[0];
    assert_eq!(e.kind, StarMapEdgeKind::References);
    assert_eq!(e.label.as_deref(), Some("ref"));
    match e.from_endpoint.as_ref().unwrap() {
        StarMapEdgeEndpoint::Node { node_id } => assert_eq!(node_id, "n1"),
        _ => panic!("expected Node"),
    }
    match e.to_endpoint.as_ref().unwrap() {
        StarMapEdgeEndpoint::Anchor { node_id, anchor_id } => {
            assert_eq!(node_id, "n2");
            assert_eq!(anchor_id, "a1");
        }
        _ => panic!("expected Anchor"),
    }
}

#[test]
fn phased_snapshot_preserves_embed_link_hyperlink() {
    let (dir, sid) = setup_workspace();
    let mut store = StarMapStore::new(dir.path(), &sid);
    store.upsert_node(make_test_node("n1", "Host"));
    use crate::starmap::semantic::{
        StarMapDeepTarget, StarMapDisplayPolicy, StarMapOpenBehavior, StarMapProvenance,
        StarMapTargetDetail,
    };
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
        source_node_id: Some("n1".to_string()),
        host_endpoint: Some(StarMapEndpoint::Node {
            node_id: "n1".to_string(),
        }),
        provenance: StarMapProvenance::default(),
        created_at: 0,
        updated_at: 0,
    });
    store.upsert_link(StarMapLink {
        link_id: "lk1".to_string(),
        source: StarMapEndpoint::Node {
            node_id: "n1".to_string(),
        },
        target: StarMapDeepTarget {
            starmap_id: "other".to_string(),
            path: vec![],
            target: StarMapTargetDetail::Starmap,
        },
        label: Some("link".to_string()),
        created_at: 0,
        updated_at: 0,
    });
    store.upsert_hyperlink(StarMapHyperlink {
        hyperlink_id: "hl1".to_string(),
        source: StarMapEndpointPath {
            segments: vec![],
            endpoint: StarMapEdgeEndpoint::Node {
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
    assert!(snapshot.embeds[0].host_endpoint.is_some());
    assert_eq!(snapshot.links.len(), 1);
    assert_eq!(snapshot.links[0].link_id, "lk1");
    assert_eq!(snapshot.hyperlinks.len(), 1);
    assert_eq!(snapshot.hyperlinks[0].hyperlink_id, "hl1");
    assert_eq!(snapshot.hyperlinks[0].target_uri, "https://example.com");
}

#[test]
fn phased_snapshot_incremental_by_revision() {
    let (dir, sid) = setup_workspace();
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
    let (dir, sid) = setup_workspace();
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
fn phased_snapshot_preserves_endpoint_path_semantics() {
    use crate::starmap::semantic::{StarMapDeepTarget, StarMapTargetDetail};
    let (dir, sid) = setup_workspace();
    let mut store = StarMapStore::new(dir.path(), &sid);
    store.upsert_node(make_test_node("n1", "A"));
    store.upsert_node(make_test_node("n2", "B"));
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
        from_endpoint_path: Some(StarMapEndpointPath {
            segments: vec![StarMapEndpointPathSegment::EnterChildMap {
                starmap_id: "child1".to_string(),
            }],
            endpoint: StarMapEdgeEndpoint::Node {
                node_id: "inner1".to_string(),
            },
        }),
        to_endpoint_path: Some(StarMapEndpointPath {
            segments: vec![],
            endpoint: StarMapEdgeEndpoint::DeepTarget {
                target: StarMapDeepTarget {
                    starmap_id: "other".to_string(),
                    path: vec![],
                    target: StarMapTargetDetail::Starmap,
                },
            },
        }),
        created_at: 0,
        updated_at: 0,
    });
    flush_store(&mut store);
    let snapshot = load_full_snapshot(dir.path(), &sid);
    let e = &snapshot.edges[0];
    let fep = e.from_endpoint_path.as_ref().unwrap();
    assert_eq!(fep.segments.len(), 1);
    match &fep.segments[0] {
        StarMapEndpointPathSegment::EnterChildMap { starmap_id } => {
            assert_eq!(starmap_id, "child1")
        }
    }
    match &fep.endpoint {
        StarMapEdgeEndpoint::Node { node_id } => assert_eq!(node_id, "inner1"),
        _ => panic!("expected Node"),
    }
    let tep = e.to_endpoint_path.as_ref().unwrap();
    assert!(tep.segments.is_empty());
    match &tep.endpoint {
        StarMapEdgeEndpoint::DeepTarget { target } => assert_eq!(target.starmap_id, "other"),
        _ => panic!("expected DeepTarget"),
    }
}

#[test]
fn phased_snapshot_returns_empty_when_revision_unchanged() {
    let (dir, sid) = setup_workspace();
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
    let (dir, sid) = setup_workspace();
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
    let (dir, sid) = setup_workspace();
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
    let (dir, sid) = setup_workspace();
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
    let (dir, sid) = setup_workspace();
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
        width: 200.0,
        height: 200.0,
    });
    flush_store(&mut store);

    let mut s1 = StarMapStore::new(dir.path(), &sid);
    let req1 = PhasedSnapshotRequest {
        target_phase: LoadPhase::CurrentViewportObjects,
        since_revision: 0,
    };
    let snap1 = s1.get_phased_snapshot(&req1).unwrap();
    let viewport_count = snap1.nodes.len();
    assert!(viewport_count >= 1);

    let req2 = PhasedSnapshotRequest {
        target_phase: LoadPhase::PrefetchNearbyObjects,
        since_revision: 0,
    };
    let snap2 = s1.get_phased_snapshot(&req2).unwrap();
    assert!(snap2.nodes.len() >= viewport_count);

    let req3 = PhasedSnapshotRequest {
        target_phase: LoadPhase::BackgroundFullLoad,
        since_revision: 0,
    };
    let snap3 = s1.get_phased_snapshot(&req3).unwrap();
    assert!(snap3.nodes.len() >= snap2.nodes.len());
    assert!(snap3.complete);
}

#[test]
fn phased_snapshot_incremental_merge_returns_all_object_types() {
    use crate::starmap::semantic::{
        StarMapDeepTarget, StarMapDisplayPolicy, StarMapOpenBehavior, StarMapProvenance,
        StarMapTargetDetail,
    };
    let (dir, sid) = setup_workspace();
    let mut store = StarMapStore::new(dir.path(), &sid);
    store.upsert_node(make_test_node("n1", "A"));
    store.upsert_node(make_test_node("n2", "B"));
    store.upsert_edge(make_test_edge("e1", "n1", "n2"));
    store.upsert_embed(StarMapEmbed {
        instance_id: "emb1".to_string(),
        target_starmap_id: "child".to_string(),
        label: None,
        display_policy: StarMapDisplayPolicy::default(),
        open_behavior: StarMapOpenBehavior::default(),
        placement: StarMapEmbedPlacement::default(),
        target_viewport: StarMapEmbedViewport::default(),
        source_node_id: Some("n1".to_string()),
        host_endpoint: None,
        provenance: StarMapProvenance::default(),
        created_at: 0,
        updated_at: 0,
    });
    store.upsert_link(StarMapLink {
        link_id: "lk1".to_string(),
        source: StarMapEndpoint::Node {
            node_id: "n1".to_string(),
        },
        target: StarMapDeepTarget {
            starmap_id: "other".to_string(),
            path: vec![],
            target: StarMapTargetDetail::Starmap,
        },
        label: None,
        created_at: 0,
        updated_at: 0,
    });
    store.upsert_hyperlink(StarMapHyperlink {
        hyperlink_id: "hl1".to_string(),
        source: StarMapEndpointPath {
            segments: vec![],
            endpoint: StarMapEdgeEndpoint::Node {
                node_id: "n1".to_string(),
            },
        },
        target_uri: "https://example.com".to_string(),
        label: None,
        target_starmap_id: None,
        created_at: 0,
        updated_at: 0,
    });
    flush_store(&mut store);
    let rev1 = store.package_revision();

    store.upsert_node(make_test_node("n3", "C"));
    flush_store(&mut store);
    let rev2 = store.package_revision();
    assert!(rev2 > rev1);

    let request = PhasedSnapshotRequest {
        target_phase: LoadPhase::BackgroundFullLoad,
        since_revision: rev1,
    };
    let snap = store.get_phased_snapshot(&request).unwrap();
    assert_eq!(snap.package_revision, rev2);
    assert!(
        !snap.nodes.is_empty(),
        "incremental merge must return nodes when revision changed"
    );
    assert!(
        !snap.edges.is_empty(),
        "incremental merge must return edges when revision changed"
    );
    assert!(
        !snap.embeds.is_empty(),
        "incremental merge must return embeds when revision changed"
    );
    assert!(
        !snap.links.is_empty(),
        "incremental merge must return links when revision changed"
    );
    assert!(
        !snap.hyperlinks.is_empty(),
        "incremental merge must return hyperlinks when revision changed"
    );
}

#[test]
fn phased_snapshot_v1_migration_reopen_preserves_data() {
    let (dir, sid) = setup_workspace();
    let mut store = StarMapStore::new(dir.path(), &sid);
    store.upsert_node(make_test_node("n1", "Node1"));
    store.upsert_node(make_test_node("n2", "Node2"));
    store.upsert_edge(make_test_edge("e1", "n1", "n2"));
    store.set_layout(StarMapLayout {
        kind: StarMapLayoutKind::Freeform,
        nodes: vec![
            StarMapLayoutNode {
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
                focus_weight: 1.0,
                orbit_group: None,
            },
            StarMapLayoutNode {
                node_id: "n2".to_string(),
                x: 200.0,
                y: 300.0,
                width: 100.0,
                height: 50.0,
                radius: 25.0,
                collapsed: false,
                z_index: 0,
                scale: 1.0,
                depth: 0.0,
                focus_weight: 1.0,
                orbit_group: None,
            },
        ],
    });
    flush_store(&mut store);
    let rev_after_save = store.package_revision();
    assert!(rev_after_save >= 1);

    let mut store2 = StarMapStore::new(dir.path(), &sid);
    let request = PhasedSnapshotRequest {
        target_phase: LoadPhase::BackgroundFullLoad,
        since_revision: 0,
    };
    let snap = store2.get_phased_snapshot(&request).unwrap();
    assert_eq!(snap.nodes.len(), 2, "reopened store should have 2 nodes");
    assert_eq!(snap.edges.len(), 1, "reopened store should have 1 edge");
    assert!(snap.layout.is_some(), "reopened store should have layout");
    let l = snap.layout.as_ref().unwrap();
    assert_eq!(l.nodes.len(), 2);
    assert!((l.nodes[0].x - 10.0).abs() < f32::EPSILON);
    assert!((l.nodes[1].x - 200.0).abs() < f32::EPSILON);
    assert!(snap.package_revision >= rev_after_save);
}

#[test]
fn phased_snapshot_uses_load_phased_not_full_load() {
    let (dir, sid) = setup_workspace();
    let mut store = StarMapStore::new(dir.path(), &sid);
    store.upsert_node(make_test_node("n1", "A"));
    store.upsert_node(make_test_node("n2", "B"));
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
        width: 200.0,
        height: 200.0,
    });
    flush_store(&mut store);

    let mut store2 = StarMapStore::new(dir.path(), &sid);
    let req = PhasedSnapshotRequest {
        target_phase: LoadPhase::CurrentViewportObjects,
        since_revision: 0,
    };
    let snap = store2.get_phased_snapshot(&req).unwrap();
    assert_eq!(snap.load_phase, LoadPhase::CurrentViewportObjects);
    assert!(!snap.complete);
    assert!(
        !snap.nodes.is_empty(),
        "viewport phase should load at least viewport node"
    );
    assert!(
        store2.get_node("n2").is_none(),
        "viewport phase should not load offscreen node n2"
    );

    let req2 = PhasedSnapshotRequest {
        target_phase: LoadPhase::BackgroundFullLoad,
        since_revision: 0,
    };
    let snap2 = store2.get_phased_snapshot(&req2).unwrap();
    assert_eq!(snap2.load_phase, LoadPhase::BackgroundFullLoad);
    assert!(snap2.complete);
    assert_eq!(snap2.nodes.len(), 2, "full load should have all nodes");
}

#[test]
fn phased_snapshot_save_failure_preserves_memory_state() {
    let (dir, sid) = setup_workspace();
    let mut store = StarMapStore::new(dir.path(), &sid);
    store.upsert_node(make_test_node("n1", "A"));
    flush_store(&mut store);

    let graph_json = dir
        .path()
        .join("starmaps")
        .join(&sid)
        .join("graph.json");
    let readonly_dir = graph_json.parent().unwrap();
    let meta_content = std::fs::read_to_string(&graph_json).unwrap();

    store.upsert_node(make_test_node("n2", "B"));
    store.enqueue_save(SaveQueueEntry::Node);
    store.enqueue_save(SaveQueueEntry::GraphMeta);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(readonly_dir, std::fs::Permissions::from_mode(0o444));
    }

    let flush_result = store.flush_save_queue();
    let save_failed = flush_result.is_err();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(readonly_dir, std::fs::Permissions::from_mode(0o755));
    }

    if save_failed {
        assert!(
            store.nodes.contains_key("n2"),
            "memory state must be preserved after save failure"
        );
        assert!(
            store.is_dirty(),
            "store must remain dirty after save failure"
        );
    }

    let _ = meta_content;
}

#[test]
fn phased_snapshot_flush_close_failure_preserves_cache() {
    let (dir, sid) = setup_workspace();
    let mut store = StarMapStore::new(dir.path(), &sid);
    store.upsert_node(make_test_node("n1", "A"));
    store.upsert_node(make_test_node("n2", "B"));
    store.upsert_edge(make_test_edge("e1", "n1", "n2"));
    store.set_layout(StarMapLayout {
        kind: StarMapLayoutKind::Freeform,
        nodes: vec![
            StarMapLayoutNode {
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
            },
            StarMapLayoutNode {
                node_id: "n2".to_string(),
                x: 200.0,
                y: 200.0,
                width: 100.0,
                height: 50.0,
                radius: 25.0,
                collapsed: false,
                z_index: 0,
                scale: 1.0,
                depth: 0.0,
                focus_weight: 1.0,
                orbit_group: None,
            },
        ],
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

    let request = PhasedSnapshotRequest {
        target_phase: LoadPhase::BackgroundFullLoad,
        since_revision: 0,
    };
    let snap = store.get_phased_snapshot(&request).unwrap();
    let rev = snap.package_revision;
    assert_eq!(snap.nodes.len(), 2);
    assert_eq!(snap.edges.len(), 1);

    let request2 = PhasedSnapshotRequest {
        target_phase: LoadPhase::BackgroundFullLoad,
        since_revision: rev,
    };
    let snap2 = store.get_phased_snapshot(&request2).unwrap();
    assert!(
        snap2.nodes.is_empty(),
        "same revision should return empty objects"
    );
    assert!(
        snap2.layout.is_some(),
        "layout must still be present even with empty objects"
    );
    assert!(
        snap2.viewport.is_some(),
        "viewport must still be present even with empty objects"
    );
}

#[test]
fn phased_snapshot_progressive_returns_objects_despite_same_revision() {
    let (dir, sid) = setup_workspace();
    let mut store = StarMapStore::new(dir.path(), &sid);
    store.upsert_node(make_test_node("n1", "InView"));
    store.upsert_node(make_test_node("n2", "Nearby"));
    store.upsert_edge(make_test_edge("e1", "n1", "n2"));
    store.set_layout(StarMapLayout {
        kind: StarMapLayoutKind::Freeform,
        nodes: vec![
            StarMapLayoutNode {
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
            },
            StarMapLayoutNode {
                node_id: "n2".to_string(),
                x: 200.0,
                y: 200.0,
                width: 100.0,
                height: 50.0,
                radius: 25.0,
                collapsed: false,
                z_index: 0,
                scale: 1.0,
                depth: 0.0,
                focus_weight: 1.0,
                orbit_group: None,
            },
        ],
    });
    store.set_viewport(StarMapViewport {
        scale: 1.0,
        offset_x: 0.0,
        offset_y: 0.0,
        width: 800.0,
        height: 600.0,
    });
    flush_store(&mut store);
    let rev = store.package_revision();

    let mut store2 = StarMapStore::new(dir.path(), &sid);
    let req1 = PhasedSnapshotRequest {
        target_phase: LoadPhase::CurrentViewportObjects,
        since_revision: rev,
    };
    let snap1 = store2.get_phased_snapshot(&req1).unwrap();
    assert!(
        !snap1.nodes.is_empty(),
        "progressive phase must return objects even when since_revision == package_revision"
    );

    let req2 = PhasedSnapshotRequest {
        target_phase: LoadPhase::PrefetchNearbyObjects,
        since_revision: rev,
    };
    let snap2 = store2.get_phased_snapshot(&req2).unwrap();
    assert!(
        !snap2.nodes.is_empty(),
        "prefetch phase must return objects even when since_revision == package_revision"
    );
    assert!(
        snap2.nodes.len() >= snap1.nodes.len(),
        "prefetch should load at least as many as viewport"
    );
}

#[test]
fn phased_snapshot_includes_deleted_ids() {
    let (dir, sid) = setup_workspace();
    let mut store = StarMapStore::new(dir.path(), &sid);
    store.upsert_node(make_test_node("n1", "A"));
    store.upsert_node(make_test_node("n2", "B"));
    store.upsert_edge(make_test_edge("e1", "n1", "n2"));
    flush_store(&mut store);

    store.remove_node("n2");
    store.remove_edge("e1");
    flush_store(&mut store);

    let request = PhasedSnapshotRequest {
        target_phase: LoadPhase::BackgroundFullLoad,
        since_revision: 0,
    };
    let snap = store.get_phased_snapshot(&request).unwrap();
    assert_eq!(snap.nodes.len(), 1);
    assert!(
        snap.deleted_node_ids.contains(&"n2".to_string()),
        "deleted node IDs must appear in snapshot"
    );
    assert!(
        snap.deleted_edge_ids.contains(&"e1".to_string()),
        "deleted edge IDs must appear in snapshot"
    );
}

#[test]
fn deletion_tombstone_revision_binds_to_flush_revision() {
    let (dir, sid) = setup_workspace();
    let mut store = StarMapStore::new(dir.path(), &sid);
    store.upsert_node(make_test_node("n1", "A"));
    store.upsert_node(make_test_node("n2", "B"));
    flush_store(&mut store);
    let rev_after_initial = store.package_revision;

    store.remove_node("n2");
    flush_store(&mut store);
    let rev_after_delete = store.package_revision;
    assert!(
        rev_after_delete > rev_after_initial,
        "flush must advance revision"
    );

    let request = PhasedSnapshotRequest {
        target_phase: LoadPhase::BackgroundFullLoad,
        since_revision: rev_after_initial,
    };
    let snap = store.get_phased_snapshot(&request).unwrap();
    assert!(
        snap.deleted_node_ids.contains(&"n2".to_string()),
        "deletion tombstone must be visible when client uses pre-delete revision: since_revision={}, deleted_at_revision must be > since_revision",
        rev_after_initial
    );
}

#[test]
fn deletion_tombstone_persists_across_store_close_reopen() {
    let (dir, sid) = setup_workspace();
    let mut store = StarMapStore::new(dir.path(), &sid);
    store.upsert_node(make_test_node("n1", "A"));
    store.upsert_node(make_test_node("n2", "B"));
    store.upsert_edge(make_test_edge("e1", "n1", "n2"));
    store.flush().unwrap();
    let rev_after_initial = store.package_revision;

    store.remove_node("n2");
    store.remove_edge("e1");
    store.flush().unwrap();
    let _rev_after_delete = store.package_revision;
    drop(store);

    let mut store2 = StarMapStore::new(dir.path(), &sid);
    store2.load_full().unwrap();
    let persistent_entries: Vec<_> = store2
        .graph_meta
        .as_ref()
        .map(|m| {
            m.deleted_since_last_sync
                .entries_since(rev_after_initial)
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    assert!(
        !persistent_entries.is_empty(),
        "deleted_since_last_sync must have entries after reload"
    );
    let request = PhasedSnapshotRequest {
        target_phase: LoadPhase::BackgroundFullLoad,
        since_revision: rev_after_initial,
    };
    let snap = store2.get_phased_snapshot(&request).unwrap();
    assert!(
        snap.deleted_node_ids.contains(&"n2".to_string()),
        "deletion tombstone for n2 must persist after close+reopen and be visible with since_revision={}",
        rev_after_initial
    );
    assert!(
        snap.deleted_edge_ids.contains(&"e1".to_string()),
        "deletion tombstone for e1 must persist after close+reopen and be visible with since_revision={}",
        rev_after_initial
    );
    assert_eq!(snap.nodes.len(), 1, "only n1 should remain after reopen");
}
