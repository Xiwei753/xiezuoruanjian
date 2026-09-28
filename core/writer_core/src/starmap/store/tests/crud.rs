use super::super::*;
use super::*;
use crate::facade::WriterCore;
use crate::starmap::semantic::{StarMapPortal, StarMapTargetDetail};
use crate::starmap::types::reference::{StarMapPathSegment, StarMapTargetPath};
use crate::starmap::types::StarMapHyperlinkPatch;
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

    use crate::starmap::semantic::{StarMapProvenance, StarMapTargetDetail};
    let embed = StarMapEmbed {
        instance_id: "emb1".to_string(),
        target_starmap_id: "other".to_string(),
        label: None,
        position: Default::default(),
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
        position: None,
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
        .update_hyperlink(
            "hl1",
            &StarMapHyperlinkPatch {
                label: Some(Some("Updated".to_string())),
                target_uri: None,
                source: None,
            },
        )
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

// ---------------------------------------------------------------------------
// 测试组 A：delete Node/Embed 走统一 validator + 级联 EnterPortal/EnterEmbed
// 第一段引用（Issue #772 回归）
// ---------------------------------------------------------------------------

/// 构造一个 portal 节点（destination 指向 `dest_starmap_id`）。
fn make_portal_node(id: &str, title: &str, dest_starmap_id: &str) -> StarMapNode {
    let mut node = make_test_node(id, title);
    node.portal = Some(StarMapPortal {
        destination_starmap_id: dest_starmap_id.to_string(),
        destination_target: None,
    });
    node
}

#[test]
fn delete_node_cascades_edge_with_first_segment_enter_portal() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Host", "", None).unwrap();
    let other = crate::starmap::create_starmap(dir.path(), "Other", "", None).unwrap();
    let host = &meta.starmap_id;

    let mut store = StarMapStore::new(dir.path(), host);
    store.upsert_node(make_portal_node("A", "Portal A", &other.starmap_id));
    store.upsert_node(make_test_node("B", "Node B"));

    // edge E：from 第一段 EnterPortal{A} 穿越 A 的 portal
    let edge_e = StarMapEdge {
        id: "E".to_string(),
        from: StarMapTargetPath {
            starmap_id: host.to_string(),
            segments: vec![StarMapPathSegment::EnterPortal {
                node_id: "A".to_string(),
            }],
            target: StarMapTargetDetail::Node {
                node_id: "B".to_string(),
            },
        },
        to: StarMapTargetPath {
            starmap_id: host.to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "B".to_string(),
            },
        },
        kind: StarMapEdgeKind::References,
        label: None,
        payload: None,
        created_at: 0,
        updated_at: 0,
    };
    store.upsert_edge(edge_e);
    assert!(store.get_edge("E").is_some());

    store.delete_node("A").unwrap();

    assert!(store.get_node("A").is_none(), "node A should be deleted");
    assert!(
        store.get_edge("E").is_none(),
        "edge E should be cascaded deleted (from first segment EnterPortal references A)"
    );
}

#[test]
fn delete_node_cascades_link_with_first_segment_enter_portal() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Host", "", None).unwrap();
    let other = crate::starmap::create_starmap(dir.path(), "Other", "", None).unwrap();
    let host = &meta.starmap_id;

    let mut store = StarMapStore::new(dir.path(), host);
    store.upsert_node(make_portal_node("A", "Portal A", &other.starmap_id));
    store.upsert_node(make_test_node("B", "Node B"));

    let link = StarMapLink {
        link_id: "L".to_string(),
        source: StarMapTargetPath {
            starmap_id: host.to_string(),
            segments: vec![StarMapPathSegment::EnterPortal {
                node_id: "A".to_string(),
            }],
            target: StarMapTargetDetail::Starmap,
        },
        target: StarMapTargetPath {
            starmap_id: host.to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Starmap,
        },
        label: Some("L".to_string()),
        created_at: 0,
        updated_at: 0,
    };
    store.upsert_link(link);
    assert!(store.get_link("L").is_some());

    store.delete_node("A").unwrap();

    assert!(store.get_node("A").is_none(), "node A should be deleted");
    assert!(
        store.get_link("L").is_none(),
        "link L should be cascaded deleted (source first segment EnterPortal references A)"
    );
}

#[test]
fn delete_embed_cascades_edge_with_first_segment_enter_embed() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Host", "", None).unwrap();
    let other = crate::starmap::create_starmap(dir.path(), "Other", "", None).unwrap();
    let host = &meta.starmap_id;

    let mut store = StarMapStore::new(dir.path(), host);
    store.upsert_node(make_test_node("B", "Node B"));
    let embed = StarMapEmbed {
        instance_id: "I".to_string(),
        target_starmap_id: other.starmap_id.clone(),
        label: None,
        position: Default::default(),
        host_path: StarMapTargetPath {
            starmap_id: host.to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "B".to_string(),
            },
        },
        provenance: crate::starmap::semantic::StarMapProvenance::default(),
        created_at: 0,
        updated_at: 0,
    };
    store.upsert_embed(embed);

    // edge E：from 第一段 EnterEmbed{I}
    let edge_e = StarMapEdge {
        id: "E".to_string(),
        from: StarMapTargetPath {
            starmap_id: host.to_string(),
            segments: vec![StarMapPathSegment::EnterEmbed {
                instance_id: "I".to_string(),
            }],
            target: StarMapTargetDetail::Node {
                node_id: "B".to_string(),
            },
        },
        to: StarMapTargetPath {
            starmap_id: host.to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "B".to_string(),
            },
        },
        kind: StarMapEdgeKind::References,
        label: None,
        payload: None,
        created_at: 0,
        updated_at: 0,
    };
    store.upsert_edge(edge_e);
    assert!(store.get_edge("E").is_some());

    store.delete_embed("I").unwrap();

    assert!(store.get_embed("I").is_none(), "embed I should be deleted");
    assert!(
        store.get_edge("E").is_none(),
        "edge E should be cascaded deleted (from first segment EnterEmbed references I)"
    );
}

#[test]
fn delete_embed_cascades_link_and_hyperlink_with_first_segment_enter_embed() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Host", "", None).unwrap();
    let other = crate::starmap::create_starmap(dir.path(), "Other", "", None).unwrap();
    let host = &meta.starmap_id;

    let mut store = StarMapStore::new(dir.path(), host);
    store.upsert_node(make_test_node("B", "Node B"));
    let embed = StarMapEmbed {
        instance_id: "I".to_string(),
        target_starmap_id: other.starmap_id.clone(),
        label: None,
        position: Default::default(),
        host_path: StarMapTargetPath {
            starmap_id: host.to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "B".to_string(),
            },
        },
        provenance: crate::starmap::semantic::StarMapProvenance::default(),
        created_at: 0,
        updated_at: 0,
    };
    store.upsert_embed(embed);

    let enter_embed = StarMapPathSegment::EnterEmbed {
        instance_id: "I".to_string(),
    };
    let link = StarMapLink {
        link_id: "L".to_string(),
        source: StarMapTargetPath {
            starmap_id: host.to_string(),
            segments: vec![enter_embed.clone()],
            target: StarMapTargetDetail::Starmap,
        },
        target: StarMapTargetPath {
            starmap_id: host.to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Starmap,
        },
        label: Some("L".to_string()),
        created_at: 0,
        updated_at: 0,
    };
    store.upsert_link(link);

    let hyperlink = StarMapHyperlink {
        hyperlink_id: "H".to_string(),
        source: StarMapTargetPath {
            starmap_id: host.to_string(),
            segments: vec![enter_embed],
            target: StarMapTargetDetail::Starmap,
        },
        target_uri: "https://example.com".to_string(),
        label: Some("H".to_string()),
        created_at: 0,
        updated_at: 0,
    };
    store.upsert_hyperlink(hyperlink);
    assert!(store.get_link("L").is_some());
    assert!(store.get_hyperlink("H").is_some());

    store.delete_embed("I").unwrap();

    assert!(store.get_embed("I").is_none(), "embed I should be deleted");
    assert!(
        store.get_link("L").is_none(),
        "link L should be cascaded deleted (source first segment EnterEmbed references I)"
    );
    assert!(
        store.get_hyperlink("H").is_none(),
        "hyperlink H should be cascaded deleted (source first segment EnterEmbed references I)"
    );
}

#[test]
fn delete_starmap_node_validates_candidate_before_mutating() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = crate::starmap::create_starmap(dir.path(), "Host", "", None).unwrap();
    let host = &meta.starmap_id;

    // node A：普通节点（将被删）。
    // node B：portal 落点指向 A（destination_starmap_id = host, destination_target = Node{A}）。
    // 删除 A 后 B.portal 落点 dangling → validate_graph 拒绝 candidate → Err，store 不变。
    // 这验证 delete 走了 candidate validate，不是先删后校验。
    let mut store = StarMapStore::new(dir.path(), host);
    store.upsert_node(make_test_node("A", "Node A"));
    let mut node_b = make_test_node("B", "Portal B");
    node_b.portal = Some(StarMapPortal {
        destination_starmap_id: host.to_string(),
        destination_target: Some(StarMapTargetDetail::Node {
            node_id: "A".to_string(),
        }),
    });
    store.upsert_node(node_b);
    store.flush().unwrap();

    let core = WriterCore::new(dir.path(), dir.path().join("projects"));
    let result = core.delete_starmap_node(host, "A");
    assert!(
        result.is_err(),
        "delete_starmap_node should return Err: deleting A leaves B.portal destination dangling"
    );

    // store 状态未变：A 还在，B 还在（candidate validate 失败，未真正改 Store）
    let mut store2 = StarMapStore::new(dir.path(), host);
    store2.load_full().unwrap();
    assert!(
        store2.get_node("A").is_some(),
        "node A should still exist (store unchanged after validate rejection)"
    );
    assert!(
        store2.get_node("B").is_some(),
        "node B should still exist (store unchanged after validate rejection)"
    );
}
