use super::*;
use super::super::*;
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
        crate::workspace::create_workspace(dir.path()).unwrap();
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
        assert_eq!(store2.get_link("l1").unwrap().label.as_deref(), Some("Test Link"));

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
        assert_eq!(store3.get_link("l1").unwrap().label.as_deref(), Some("Updated Link"));

        store3.delete_link("l1").unwrap();
        store3.flush().unwrap();

        let mut store4 = StarMapStore::new(dir.path(), &meta.starmap_id);
        let result4 = store4.load_full().unwrap();
        assert_eq!(result4.loaded_link_count, 0);
        assert!(store4.get_link("l1").is_none());
    }
#[test]
    fn upsert_edge_existing_marks_dirty_graph_meta() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        let node1 = make_test_node("n1", "A");
        let node2 = make_test_node("n2", "B");
        let node3 = make_test_node("n3", "C");
        store.upsert_node(node1);
        store.upsert_node(node2);
        store.upsert_node(node3);

        use crate::starmap::types::{StarMapEdge, StarMapEdgeKind};
        let edge = StarMapEdge {
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
        };
        store.upsert_edge(edge);
        store.flush().unwrap();

        let edge_updated = StarMapEdge {
            id: "e1".to_string(),
            from: Some("n1".to_string()),
            to: Some("n3".to_string()),
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
        };
        store.upsert_edge(edge_updated);
        assert!(store.dirty_graph_meta, "upsert_edge on existing edge should mark dirty_graph_meta because relation index changed");

        store.flush().unwrap();
        let meta_on_disk: GraphMeta = serde_json::from_str(
            &std::fs::read_to_string(store.starmap_dir().join("graph.json")).unwrap()
        ).unwrap();
        let eri = meta_on_disk.edge_relation_index.iter().find(|e| e.edge_id == "e1").unwrap();
        assert_eq!(eri.to, "n3", "disk relation index should reflect updated endpoint");
    }
#[test]
    fn update_edge_endpoint_marks_dirty_graph_meta() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "A"));
        store.upsert_node(make_test_node("n2", "B"));
        store.upsert_node(make_test_node("n3", "C"));

        use crate::starmap::types::{StarMapEdge, StarMapEdgeKind, StarMapEdgePatch};
        let edge = StarMapEdge {
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
        };
        store.upsert_edge(edge);
        store.flush().unwrap();

        let patch = StarMapEdgePatch {
            kind: None,
            label: None,
            payload: None,
            from_target: Some(Some(crate::starmap::semantic::StarMapDeepTarget {
                starmap_id: meta.starmap_id.clone(),
                path: vec![],
                target: crate::starmap::semantic::StarMapTargetDetail::Node { node_id: "n3".to_string() },
            })),
            to_target: None,
            from_endpoint: None,
            to_endpoint: None,
            from_endpoint_path: None,
            to_endpoint_path: None,
        };
        store.update_edge("e1", &patch).unwrap();
        assert!(store.dirty_graph_meta, "update_edge changing from_target should mark dirty_graph_meta");

        store.flush().unwrap();
        let meta_on_disk: GraphMeta = serde_json::from_str(
            &std::fs::read_to_string(store.starmap_dir().join("graph.json")).unwrap()
        ).unwrap();
        let eri = meta_on_disk.edge_relation_index.iter().find(|e| e.edge_id == "e1").unwrap();
        assert_eq!(eri.from, "n3", "disk relation index should reflect updated from endpoint");
    }
#[test]
    fn update_embed_host_marks_dirty_graph_meta() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "A"));
        store.upsert_node(make_test_node("n2", "B"));

        use crate::starmap::types::{StarMapEmbed, StarMapEmbedPlacement, StarMapEmbedViewport, StarMapEmbedPatch};
        let embed = StarMapEmbed {
            instance_id: "emb1".to_string(),
            target_starmap_id: "other".to_string(),
            label: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            placement: StarMapEmbedPlacement::default(),
            target_viewport: StarMapEmbedViewport::default(),
            source_node_id: Some("n1".to_string()),
            host_endpoint: None,
            provenance: Default::default(),
            created_at: 0,
            updated_at: 0,
        };
        store.upsert_embed(embed);
        store.flush().unwrap();

        let patch = StarMapEmbedPatch {
            label: None,
            display_policy: None,
            open_behavior: None,
            viewport: None,
            placement: None,
            target_viewport: None,
            source_node_id: Some(Some("n2".to_string())),
            host_anchor: None,
            host_endpoint: None,
        };
        store.update_embed("emb1", &patch).unwrap();
        assert!(store.dirty_graph_meta, "update_embed changing source_node_id should mark dirty_graph_meta");

        store.flush().unwrap();
        let meta_on_disk: GraphMeta = serde_json::from_str(
            &std::fs::read_to_string(store.starmap_dir().join("graph.json")).unwrap()
        ).unwrap();
        let ehi = meta_on_disk.embed_host_index.iter().find(|e| e.instance_id == "emb1").unwrap();
        assert_eq!(ehi.host_node_id, "n2", "disk host index should reflect updated host");
    }
#[test]
    fn delete_also_removes_flat_path() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let starmap_dir = dir.path().join("app-meta").join("starmaps").join(&meta.starmap_id);
        let nodes_dir = starmap_dir.join("nodes");
        std::fs::create_dir_all(&nodes_dir).unwrap();

        let flat_path = nodes_dir.join("n1.json");
        std::fs::write(&flat_path, "{}").unwrap();
        assert!(flat_path.exists(), "flat file should exist before delete");

        package_storage::delete_node_file(dir.path(), &meta.starmap_id, "n1").unwrap();
        assert!(!flat_path.exists(), "flat file should be removed by delete_node_file");
    }
#[test]
    fn update_edge_endpoint_marks_dirty_and_updates_index() {
        use crate::starmap::types::{StarMapEdge, StarMapEdgeKind, StarMapEdgeEndpoint, StarMapEdgePatch};
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
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
            from_endpoint: None,
            to_endpoint: None,
            from_endpoint_path: None,
            to_endpoint_path: None,
            created_at: 0,
            updated_at: 0,
        });
        store.flush().unwrap();
        assert!(!store.is_dirty());

        let patch = StarMapEdgePatch {
            kind: None,
            label: None,
            payload: None,
            from_target: None,
            to_target: None,
            from_endpoint: Some(Some(StarMapEdgeEndpoint::Node { node_id: "n1".to_string() })),
            to_endpoint: None,
            from_endpoint_path: None,
            to_endpoint_path: None,
        };
        store.update_edge("e1", &patch).unwrap();
        assert!(store.is_dirty(), "update_edge with endpoint change should mark dirty");
        let eri = store.graph_meta.as_ref().unwrap().edge_relation_index.iter().find(|e| e.edge_id == "e1").unwrap();
        assert!(eri.from_endpoint.is_some(), "index should reflect updated from_endpoint");
    }
#[test]
    fn update_embed_host_endpoint_marks_dirty_and_updates_index() {
        use crate::starmap::types::{StarMapEmbed, StarMapEmbedPatch, StarMapEndpoint};
        use crate::starmap::semantic::{StarMapDisplayPolicy, StarMapOpenBehavior, StarMapProvenance};
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
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
            host_endpoint: None,
            provenance: StarMapProvenance::default(),
            created_at: 0,
            updated_at: 0,
        });
        store.flush().unwrap();
        assert!(!store.is_dirty());

        let patch = StarMapEmbedPatch {
            label: None,
            display_policy: None,
            open_behavior: None,
            viewport: None,
            placement: None,
            target_viewport: None,
            source_node_id: None,
            host_anchor: None,
            host_endpoint: Some(Some(StarMapEndpoint::Anchor { node_id: "n1".to_string(), anchor_id: "a1".to_string() })),
        };
        store.update_embed("emb1", &patch).unwrap();
        assert!(store.is_dirty(), "update_embed with host_endpoint change should mark dirty");
        let ehi = store.graph_meta.as_ref().unwrap().embed_host_index.iter().find(|e| e.instance_id == "emb1").unwrap();
        assert!(ehi.host_endpoint.is_some(), "index should reflect updated host_endpoint");
    }
#[test]
    fn add_link_updates_graph_meta_link_ids() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        let link = make_test_link("l1", "Test");
        store.add_link(link).unwrap();
        assert!(store.dirty_links.contains("l1"));
        assert!(store.dirty_graph_meta, "add_link must mark dirty_graph_meta");
        assert!(store.graph_meta.is_some(), "add_link must initialize graph_meta");
        let meta_ids = store.graph_meta.as_ref().unwrap().link_ids.clone();
        assert!(meta_ids.contains(&"l1".to_string()), "add_link must add link_id to graph_meta.link_ids");

        store.flush().unwrap();
        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store2.load_full().unwrap();
        assert_eq!(store2.link_count(), 1);
        assert!(store2.graph_meta.as_ref().unwrap().link_ids.contains(&"l1".to_string()),
            "link_id must persist in graph.json after flush");
    }
#[test]
    fn delete_link_updates_graph_meta_link_ids() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        let link = make_test_link("l1", "Test");
        store.add_link(link).unwrap();
        store.flush().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store2.load_full().unwrap();
        store2.delete_link("l1").unwrap();
        assert!(store2.deleted_link_ids.contains("l1"),
            "delete_link must mark deleted_link_ids");
        assert!(store2.dirty_graph_meta, "delete_link must mark dirty_graph_meta");
        assert!(!store2.graph_meta.as_ref().unwrap().link_ids.contains(&"l1".to_string()),
            "delete_link must remove link_id from graph_meta.link_ids");

        store2.flush().unwrap();
        let mut store3 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store3.load_full().unwrap();
        assert_eq!(store3.link_count(), 0);
        assert!(!store3.graph_meta.as_ref().unwrap().link_ids.contains(&"l1".to_string()),
            "link_id must be removed from graph.json after flush");
    }
#[test]
    fn hyperlink_add_update_delete_round_trip() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.load_full().unwrap();

        let hl = StarMapHyperlink {
            hyperlink_id: "hl1".to_string(),
            source: StarMapEndpointPath {
                segments: vec![],
                endpoint: StarMapEdgeEndpoint::Starmap,
            },
            target_uri: "https://example.com".to_string(),
            label: Some("TestHL".to_string()),
            target_starmap_id: None,
            created_at: 0,
            updated_at: 0,
        };
        let result = store.add_hyperlink(hl).unwrap();
        assert_eq!(result.hyperlink_id, "hl1");
        assert!(store.dirty_hyperlinks.contains("hl1"));
        assert!(store.dirty_graph_meta, "add_hyperlink must mark dirty_graph_meta");
        assert!(store.graph_meta.as_ref().unwrap().hyperlink_ids.contains(&"hl1".to_string()));

        store.enqueue_save(SaveQueueEntry::Hyperlink);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        store.flush_save_queue().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store2.load_full().unwrap();
        assert_eq!(store2.hyperlink_count(), 1);
        assert_eq!(store2.get_hyperlink("hl1").unwrap().label.as_deref(), Some("TestHL"));

        let updated = store2.update_hyperlink("hl1", Some("UpdatedHL"), None).unwrap();
        assert_eq!(updated.label.as_deref(), Some("UpdatedHL"));
        store2.enqueue_save(SaveQueueEntry::Hyperlink);
        store2.flush_save_queue().unwrap();

        let mut store3 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store3.load_full().unwrap();
        assert_eq!(store3.get_hyperlink("hl1").unwrap().label.as_deref(), Some("UpdatedHL"));

        store3.delete_hyperlink("hl1").unwrap();
        assert!(!store3.graph_meta.as_ref().unwrap().hyperlink_ids.contains(&"hl1".to_string()));
        assert!(store3.dirty_graph_meta, "delete_hyperlink must mark dirty_graph_meta");
        store3.enqueue_save(SaveQueueEntry::DeleteHyperlink);
        store3.enqueue_save(SaveQueueEntry::GraphMeta);
        store3.flush_save_queue().unwrap();

        let mut store4 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store4.load_full().unwrap();
        assert_eq!(store4.hyperlink_count(), 0);
    }
#[test]
    fn delete_node_cascades_to_links_and_hyperlinks() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.load_full().unwrap();

        let node = StarMapNode {
            id: "n1".to_string(),
            title: "Node1".to_string(),
            kind: StarMapNodeKind::Chapter,
            payload: None,
            tags: vec![],
            content: Default::default(),
            anchors: vec![],
            portal: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            provenance: Default::default(),
            created_at: 0,
            updated_at: 0,
        };
        store.upsert_node(node);
        store.enqueue_save(SaveQueueEntry::Node);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        store.flush_save_queue().unwrap();

        let link = StarMapLink {
            link_id: "l1".to_string(),
            source: StarMapEndpoint::Node { node_id: "n1".to_string() },
            target: crate::starmap::semantic::StarMapDeepTarget {
                starmap_id: "other".to_string(),
                path: vec![],
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            },
            label: Some("LinkToN1".to_string()),
            created_at: 0,
            updated_at: 0,
        };
        store.upsert_link(link);
        store.enqueue_save(SaveQueueEntry::Link);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        store.flush_save_queue().unwrap();

        let hl = StarMapHyperlink {
            hyperlink_id: "hl1".to_string(),
            source: StarMapEndpointPath {
                segments: vec![],
                endpoint: StarMapEdgeEndpoint::Node { node_id: "n1".to_string() },
            },
            target_uri: "https://example.com".to_string(),
            label: Some("HLonN1".to_string()),
            target_starmap_id: None,
            created_at: 0,
            updated_at: 0,
        };
        store.upsert_hyperlink(hl);
        store.enqueue_save(SaveQueueEntry::Hyperlink);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        store.flush_save_queue().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store2.load_full().unwrap();
        assert_eq!(store2.link_count(), 1);
        assert_eq!(store2.hyperlink_count(), 1);

        store2.delete_node("n1").unwrap();
        assert!(store2.get_link("l1").is_none(), "delete_node must cascade remove link");
        assert!(store2.get_hyperlink("hl1").is_none(), "delete_node must cascade remove hyperlink");
        assert!(store2.deleted_link_ids.contains("l1"));
        assert!(store2.deleted_hyperlink_ids.contains("hl1"));
        assert!(store2.dirty_graph_meta);

        store2.enqueue_save(SaveQueueEntry::DeleteNode);
        store2.enqueue_save(SaveQueueEntry::DeleteLink);
        store2.enqueue_save(SaveQueueEntry::DeleteHyperlink);
        store2.enqueue_save(SaveQueueEntry::GraphMeta);
        store2.flush_save_queue().unwrap();

        let mut store3 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store3.load_full().unwrap();
        assert_eq!(store3.node_count(), 0);
        assert_eq!(store3.link_count(), 0);
        assert_eq!(store3.hyperlink_count(), 0);
    }
