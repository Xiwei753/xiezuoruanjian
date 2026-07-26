//! # 星图图数据 CRUD 操作
//!
//! 节点、边、嵌入、链接的增删改查，以及深目标解析和图验证。
//! 所有写操作通过 `atomic_write_string` 持久化到 `graph.json`。

mod edge_ops;
mod embed_ops;
mod link_ops;
mod node_ops;
mod ops;
pub mod resolve;
pub mod validation;

pub use edge_ops::*;
pub use embed_ops::*;
pub use link_ops::*;
pub use node_ops::*;
pub use ops::*;
pub use resolve::resolve_deep_target;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::starmap::create_starmap;
    use crate::starmap::types::*;
    use crate::starmap::{load_starmap_meta, now_epoch};
    use crate::workspace::create_workspace;
    use tempfile::tempdir;

    fn setup_workspace() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        create_workspace(dir.path()).unwrap();
        dir
    }

    #[test]
    fn test_starmap_graph_crud() {
        let dir = setup_workspace();
        let meta = create_starmap(dir.path(), "Test Map", "desc", None).unwrap();
        crate::starmap::bind_starmap_to_project(dir.path(), &meta.starmap_id, "test_proj").unwrap();

        let mut graph = get_starmap_graph(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(graph.nodes.len(), 0);
        assert_eq!(graph.edges.len(), 0);

        let node1 = StarMapNode {
            id: "n1".to_string(),
            title: "Node 1".to_string(),
            kind: StarMapNodeKind::Note,
            payload: None,
            tags: vec![],
            content: Default::default(),
            anchors: vec![],
            portal: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            provenance: Default::default(),
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        add_starmap_node(dir.path(), &meta.starmap_id, node1.clone(), 0.0, 0.0).unwrap();

        let node2 = StarMapNode {
            id: "n2".to_string(),
            title: "Node 2".to_string(),
            kind: StarMapNodeKind::Concept,
            payload: None,
            tags: vec![],
            content: Default::default(),
            anchors: vec![],
            portal: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            provenance: Default::default(),
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        add_starmap_node(dir.path(), &meta.starmap_id, node2.clone(), 0.0, 0.0).unwrap();

        graph = get_starmap_graph(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(graph.nodes.len(), 2);

        let refreshed_meta = load_starmap_meta(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(refreshed_meta.node_count, 2);

        update_starmap_node(
            dir.path(),
            &meta.starmap_id,
            "n1",
            StarMapNodePatch {
                title: Some("Updated N1".to_string()),
                kind: Some(StarMapNodeKind::Chapter),
                payload: None,
                tags: None,
                content: None,
                anchors: None,
                portal: None,
                display_policy: None,
                open_behavior: None,
                provenance: None,
            },
        )
        .unwrap();
        graph = get_starmap_graph(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(
            graph.nodes.iter().find(|n| n.id == "n1").unwrap().title,
            "Updated N1"
        );

        let refreshed_meta2 = load_starmap_meta(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(refreshed_meta2.linked_chapter_count, 1);

        let edge = StarMapEdge {
            id: "e1".to_string(),
            from: Some("n1".to_string()),
            to: Some("n2".to_string()),
            kind: StarMapEdgeKind::RelatedTo,
            label: Some("relates".to_string()),
            payload: None,
            from_target: None,
            to_target: None,
            from_endpoint: None,
            to_endpoint: None,
            from_endpoint_path: None,
            to_endpoint_path: None,
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        add_starmap_edge(dir.path(), &meta.starmap_id, edge.clone()).unwrap();

        graph = get_starmap_graph(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(graph.edges.len(), 1);

        update_starmap_edge(
            dir.path(),
            &meta.starmap_id,
            "e1",
            StarMapEdgePatch {
                kind: Some(StarMapEdgeKind::Causes),
                label: Some(Some("causes".to_string())),
                payload: None,
                from_target: None,
                to_target: None,
                from_endpoint: None,
                to_endpoint: None,
                from_endpoint_path: None,
                to_endpoint_path: None,
            },
        )
        .unwrap();
        graph = get_starmap_graph(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(graph.edges[0].label.as_deref(), Some("causes"));

        delete_starmap_node(dir.path(), &meta.starmap_id, "n1").unwrap();
        graph = get_starmap_graph(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.edges.len(), 0);

        let refreshed_meta3 = load_starmap_meta(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(refreshed_meta3.node_count, 1);
        assert_eq!(refreshed_meta3.edge_count, 0);
        assert_eq!(refreshed_meta3.linked_chapter_count, 0);
    }

    #[test]
    fn test_starmap_layout_crud() {
        let dir = setup_workspace();
        let meta = create_starmap(dir.path(), "Layout Map", "desc", None).unwrap();
        crate::starmap::bind_starmap_to_project(dir.path(), &meta.starmap_id, "test_proj").unwrap();

        let mut layout = get_starmap_layout(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(layout.nodes.len(), 0);

        layout.nodes.push(StarMapLayoutNode {
            node_id: "n1".to_string(),
            x: 100.0,
            y: 200.0,
            width: 50.0,
            height: 50.0,
            radius: 25.0,
            collapsed: false,
            z_index: 0,
            scale: 1.0,
            depth: 0.0,
            focus_weight: 0.0,
            orbit_group: None,
        });

        save_starmap_layout(dir.path(), &meta.starmap_id, &layout).unwrap();

        let loaded = get_starmap_layout(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(loaded.nodes.len(), 1);
        assert_eq!(loaded.nodes[0].x, 100.0);
    }

    #[test]
    fn test_starmap_viewport_crud() {
        let dir = setup_workspace();
        let meta = create_starmap(dir.path(), "Viewport Map", "desc", None).unwrap();

        let default_viewport = get_starmap_viewport(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(default_viewport.scale, 1.0);
        assert_eq!(default_viewport.offset_x, 0.0);

        let viewport = StarMapViewport {
            scale: 1.5,
            offset_x: 120.0,
            offset_y: -40.0,
            width: 1080.0,
            height: 1920.0,
        };
        save_starmap_viewport(dir.path(), &meta.starmap_id, &viewport).unwrap();

        let loaded = get_starmap_viewport(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(loaded.scale, 1.5);
        assert_eq!(loaded.offset_x, 120.0);
        assert_eq!(loaded.offset_y, -40.0);
        assert_eq!(loaded.width, 1080.0);
        assert_eq!(loaded.height, 1920.0);
    }

    #[test]
    fn test_embed_and_link_crud_and_validation() {
        let dir = setup_workspace();
        let meta_a = create_starmap(dir.path(), "Map A", "", None).unwrap();
        let meta_b = create_starmap(dir.path(), "Map B", "", None).unwrap();

        let embed = crate::starmap::types::StarMapEmbed {
            instance_id: "inst1".to_string(),
            target_starmap_id: meta_b.starmap_id.clone(),
            label: Some("embed".to_string()),
            display_policy: Default::default(),
            open_behavior: Default::default(),
            placement: Default::default(),
            target_viewport: Default::default(),
            source_node_id: None,
            host_endpoint: None,
            provenance: Default::default(),
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        assert!(add_starmap_embed(dir.path(), &meta_a.starmap_id, embed.clone()).is_ok());
        assert!(add_starmap_embed(dir.path(), &meta_a.starmap_id, embed.clone()).is_err());

        assert!(update_starmap_embed(
            dir.path(),
            &meta_a.starmap_id,
            "inst1",
            crate::starmap::types::StarMapEmbedPatch {
                label: Some(Some("updated".to_string())),
                display_policy: None,
                open_behavior: None,
                placement: Default::default(),
                target_viewport: Default::default(),
                source_node_id: None,
                host_endpoint: None,
                viewport: None,
                host_anchor: None,
            }
        )
        .is_ok());

        assert!(delete_starmap_embed(dir.path(), &meta_a.starmap_id, "inst1").is_ok());
        assert!(crate::starmap::load_starmap_meta(dir.path(), &meta_b.starmap_id).is_ok());

        let embed_missing = crate::starmap::types::StarMapEmbed {
            instance_id: "inst_missing".to_string(),
            target_starmap_id: "non-existent".to_string(),
            label: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            placement: Default::default(),
            target_viewport: Default::default(),
            source_node_id: None,
            host_endpoint: None,
            provenance: Default::default(),
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        let mut g = get_starmap_graph(dir.path(), &meta_a.starmap_id).unwrap();
        g.embeds.push(embed_missing);
        assert!(save_starmap_graph(dir.path(), &meta_a.starmap_id, &g).is_err());

        let embed_missing_node = crate::starmap::types::StarMapEmbed {
            instance_id: "inst2".to_string(),
            target_starmap_id: meta_b.starmap_id.clone(),
            label: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            placement: Default::default(),
            target_viewport: Default::default(),
            source_node_id: Some("missing_node".to_string()),
            host_endpoint: None,
            provenance: Default::default(),
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        let mut g2 = get_starmap_graph(dir.path(), &meta_a.starmap_id).unwrap();
        g2.embeds.push(embed_missing_node);
        assert!(save_starmap_graph(dir.path(), &meta_a.starmap_id, &g2).is_err());

        let link = crate::starmap::types::StarMapLink {
            link_id: "link1".to_string(),
            source: crate::starmap::types::StarMapEndpoint::Starmap,
            target: crate::starmap::semantic::StarMapDeepTarget {
                starmap_id: meta_b.starmap_id.clone(),
                path: vec![],
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            },
            label: None,
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        assert!(add_starmap_link(dir.path(), &meta_a.starmap_id, link.clone()).is_ok());
        assert!(add_starmap_link(dir.path(), &meta_a.starmap_id, link.clone()).is_err());

        assert!(update_starmap_link(
            dir.path(),
            &meta_a.starmap_id,
            "link1",
            crate::starmap::types::StarMapLinkPatch {
                source: None,
                target: None,
                label: Some(Some("updated_link".to_string())),
            }
        )
        .is_ok());

        assert!(delete_starmap_link(dir.path(), &meta_a.starmap_id, "link1").is_ok());
        assert!(crate::starmap::load_starmap_meta(dir.path(), &meta_b.starmap_id).is_ok());

        let link_missing_src = crate::starmap::types::StarMapLink {
            link_id: "link_bad_src".to_string(),
            source: crate::starmap::types::StarMapEndpoint::Node {
                node_id: "missing_node".to_string(),
            },
            target: crate::starmap::semantic::StarMapDeepTarget {
                starmap_id: meta_b.starmap_id.clone(),
                path: vec![],
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            },
            label: None,
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        let mut g3 = get_starmap_graph(dir.path(), &meta_a.starmap_id).unwrap();
        g3.links.push(link_missing_src);
        assert!(save_starmap_graph(dir.path(), &meta_a.starmap_id, &g3).is_err());

        let link_missing_tgt = crate::starmap::types::StarMapLink {
            link_id: "link_bad_tgt".to_string(),
            source: crate::starmap::types::StarMapEndpoint::Starmap,
            target: crate::starmap::semantic::StarMapDeepTarget {
                starmap_id: "missing_starmap".to_string(),
                path: vec![],
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            },
            label: None,
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        let mut g4 = get_starmap_graph(dir.path(), &meta_a.starmap_id).unwrap();
        g4.links.push(link_missing_tgt);
        assert!(save_starmap_graph(dir.path(), &meta_a.starmap_id, &g4).is_err());

        let n1 = StarMapNode {
            id: "n1".to_string(),
            title: "n1".to_string(),
            kind: StarMapNodeKind::Note,
            payload: None,
            tags: vec![],
            content: Default::default(),
            anchors: vec![],
            portal: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            provenance: Default::default(),
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        let mut g5 = get_starmap_graph(dir.path(), &meta_a.starmap_id).unwrap();
        g5.nodes.push(n1);
        save_starmap_graph(dir.path(), &meta_a.starmap_id, &g5).unwrap();

        let edge = StarMapEdge {
            id: "e_semantic".to_string(),
            from: Some("n1".to_string()),
            to: Some("dummy_missing".to_string()),
            kind: StarMapEdgeKind::RelatedTo,
            label: None,
            payload: None,
            from_target: None,
            to_target: Some(crate::starmap::semantic::StarMapDeepTarget {
                starmap_id: meta_b.starmap_id.clone(),
                path: vec![],
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            }),
            from_endpoint: None,
            to_endpoint: None,
            from_endpoint_path: None,
            to_endpoint_path: None,
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        assert!(add_starmap_edge(dir.path(), &meta_a.starmap_id, edge).is_ok());

        let meta_c = create_starmap(dir.path(), "Map C", "", None).unwrap();
        let link_deep = crate::starmap::types::StarMapLink {
            link_id: "link_deep".to_string(),
            source: crate::starmap::types::StarMapEndpoint::Starmap,
            target: crate::starmap::semantic::StarMapDeepTarget {
                starmap_id: meta_b.starmap_id.clone(),
                path: vec![crate::starmap::semantic::StarMapPathSegment::EnterChild {
                    starmap_id: meta_c.starmap_id.clone(),
                }],
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            },
            label: None,
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        add_starmap_link(dir.path(), &meta_a.starmap_id, link_deep).unwrap();

        let refs_to_b =
            crate::starmap::find_starmap_references(dir.path(), &meta_b.starmap_id).unwrap();
        assert_eq!(refs_to_b.len(), 2);

        let refs_to_c =
            crate::starmap::find_starmap_references(dir.path(), &meta_c.starmap_id).unwrap();
        assert_eq!(refs_to_c.len(), 1);
        assert_eq!(refs_to_c[0].ref_type, "link");
    }

    #[test]
    fn test_starmap_deep_target_validation() {
        let dir = setup_workspace();
        let meta_a = create_starmap(dir.path(), "Map A", "", None).unwrap();
        let meta_b = create_starmap(dir.path(), "Map B", "", None).unwrap();

        let mut graph_a = get_starmap_graph(dir.path(), &meta_a.starmap_id).unwrap();

        let node_a1 = StarMapNode {
            id: "a1".to_string(),
            title: "Node A1".to_string(),
            kind: StarMapNodeKind::Note,
            payload: None,
            tags: vec![],
            content: Default::default(),
            anchors: vec![],
            portal: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            provenance: Default::default(),
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        graph_a.nodes.push(node_a1.clone());
        save_starmap_graph(dir.path(), &meta_a.starmap_id, &graph_a).unwrap();

        let mut node_with_portal = node_a1.clone();
        node_with_portal.portal = Some(crate::starmap::semantic::StarMapPortal {
            target_starmap_id: "non-existent".to_string(),
            deep_target: None,
            mode: crate::starmap::semantic::StarMapPortalMode::EnterChild,
            preview_policy: Default::default(),
        });
        let mut g = graph_a.clone();
        g.nodes[0] = node_with_portal;
        assert!(save_starmap_graph(dir.path(), &meta_a.starmap_id, &g).is_err());

        let edge_to_b = StarMapEdge {
            id: "e_to_b".to_string(),
            from: Some("a1".to_string()),
            to: Some("dummy".to_string()),
            kind: StarMapEdgeKind::RelatedTo,
            label: None,
            payload: None,
            from_target: None,
            to_target: Some(crate::starmap::semantic::StarMapDeepTarget {
                starmap_id: meta_b.starmap_id.clone(),
                path: vec![],
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            }),
            from_endpoint: None,
            to_endpoint: None,
            from_endpoint_path: None,
            to_endpoint_path: None,
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        let mut g2 = graph_a.clone();
        g2.edges.push(edge_to_b);
        assert!(save_starmap_graph(dir.path(), &meta_a.starmap_id, &g2).is_ok());

        let dt_cycle = crate::starmap::semantic::StarMapDeepTarget {
            starmap_id: meta_b.starmap_id.clone(),
            path: vec![crate::starmap::semantic::StarMapPathSegment::EnterChild {
                starmap_id: meta_b.starmap_id.clone(),
            }],
            target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
        };
        let edge_cycle = StarMapEdge {
            id: "e_cycle".to_string(),
            from: Some("a1".to_string()),
            to: Some("dummy".to_string()),
            kind: StarMapEdgeKind::RelatedTo,
            label: None,
            payload: None,
            from_target: None,
            to_target: Some(dt_cycle),
            from_endpoint: None,
            to_endpoint: None,
            from_endpoint_path: None,
            to_endpoint_path: None,
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        let mut g3 = graph_a.clone();
        g3.edges.push(edge_cycle);
        assert!(save_starmap_graph(dir.path(), &meta_a.starmap_id, &g3).is_err());

        let mut node_dp = node_a1.clone();
        node_dp.display_policy = crate::starmap::semantic::StarMapDisplayPolicy {
            importance: -1.0,
            ..Default::default()
        };
        let mut g4 = graph_a.clone();
        g4.nodes[0] = node_dp;
        assert!(save_starmap_graph(dir.path(), &meta_a.starmap_id, &g4).is_err());

        let mut node_anchor = node_a1.clone();
        node_anchor
            .anchors
            .push(crate::starmap::semantic::StarMapAnchor {
                anchor_id: "anc1".to_string(),
                target: crate::starmap::semantic::StarMapAnchorTarget::ChapterRange {
                    project_id: None,
                    volume_id: None,
                    chapter_id: "chap1".to_string(),
                    range_start: Some(100),
                    range_end: Some(50),
                },
                label: None,
                role: Default::default(),
            });
        let mut g5 = graph_a.clone();
        g5.nodes[0] = node_anchor;
        assert!(save_starmap_graph(dir.path(), &meta_a.starmap_id, &g5).is_err());
    }

    #[test]
    fn test_starmap_embed_and_link_semantics() {
        let dir = setup_workspace();
        let meta_a = create_starmap(dir.path(), "Map A", "", None).unwrap();
        let meta_b = create_starmap(dir.path(), "Map B", "", None).unwrap();
        let meta_c = create_starmap(dir.path(), "Map C", "", None).unwrap();

        let mut graph_a = get_starmap_graph(dir.path(), &meta_a.starmap_id).unwrap();
        let _graph_b = get_starmap_graph(dir.path(), &meta_b.starmap_id).unwrap();
        let mut graph_c = get_starmap_graph(dir.path(), &meta_c.starmap_id).unwrap();

        graph_a.embeds.push(crate::starmap::types::StarMapEmbed {
            instance_id: "inst1".to_string(),
            target_starmap_id: meta_b.starmap_id.clone(),
            label: Some("B in A".to_string()),
            display_policy: Default::default(),
            open_behavior: Default::default(),
            placement: Default::default(),
            target_viewport: Default::default(),
            source_node_id: None,
            host_endpoint: None,
            provenance: Default::default(),
            created_at: now_epoch(),
            updated_at: now_epoch(),
        });
        assert!(save_starmap_graph(dir.path(), &meta_a.starmap_id, &graph_a).is_ok());

        graph_c.embeds.push(crate::starmap::types::StarMapEmbed {
            instance_id: "inst2".to_string(),
            target_starmap_id: meta_b.starmap_id.clone(),
            label: Some("B in C".to_string()),
            display_policy: Default::default(),
            open_behavior: Default::default(),
            placement: Default::default(),
            target_viewport: Default::default(),
            source_node_id: None,
            host_endpoint: None,
            provenance: Default::default(),
            created_at: now_epoch(),
            updated_at: now_epoch(),
        });
        assert!(save_starmap_graph(dir.path(), &meta_c.starmap_id, &graph_c).is_ok());

        graph_a.links.push(crate::starmap::types::StarMapLink {
            link_id: "link1".to_string(),
            source: crate::starmap::types::StarMapEndpoint::Starmap,
            target: crate::starmap::semantic::StarMapDeepTarget {
                starmap_id: meta_b.starmap_id.clone(),
                path: vec![],
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            },
            label: None,
            created_at: now_epoch(),
            updated_at: now_epoch(),
        });
        assert!(save_starmap_graph(dir.path(), &meta_a.starmap_id, &graph_a).is_ok());

        assert!(crate::starmap::load_starmap_meta(dir.path(), &meta_b.starmap_id).is_ok());

        let refs = crate::starmap::find_starmap_references(dir.path(), &meta_b.starmap_id).unwrap();
        assert_eq!(refs.len(), 3);
        let has_a = refs
            .iter()
            .any(|r| r.host_starmap_id == meta_a.starmap_id && r.ref_type == "embed");
        let has_c = refs
            .iter()
            .any(|r| r.host_starmap_id == meta_c.starmap_id && r.ref_type == "embed");
        assert!(has_a);
        assert!(has_c);

        graph_a.embeds.clear();
        assert!(save_starmap_graph(dir.path(), &meta_a.starmap_id, &graph_a).is_ok());
        assert!(crate::starmap::load_starmap_meta(dir.path(), &meta_b.starmap_id).is_ok());
    }

    #[test]
    fn test_starmap_edge_patching() {
        let dir = setup_workspace();
        let meta_a = create_starmap(dir.path(), "Map A", "", None).unwrap();
        let mut graph_a = get_starmap_graph(dir.path(), &meta_a.starmap_id).unwrap();

        let n1 = StarMapNode {
            id: "n1".to_string(),
            title: "n1".to_string(),
            kind: StarMapNodeKind::Note,
            payload: None,
            tags: vec![],
            content: Default::default(),
            anchors: vec![],
            portal: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            provenance: Default::default(),
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        let n2 = StarMapNode {
            id: "n2".to_string(),
            title: "n2".to_string(),
            kind: StarMapNodeKind::Note,
            payload: None,
            tags: vec![],
            content: Default::default(),
            anchors: vec![],
            portal: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            provenance: Default::default(),
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };

        graph_a.nodes.push(n1);
        graph_a.nodes.push(n2);
        save_starmap_graph(dir.path(), &meta_a.starmap_id, &graph_a).unwrap();

        let _edge = add_starmap_edge(
            dir.path(),
            &meta_a.starmap_id,
            StarMapEdge {
                id: "e1".to_string(),
                from: Some("n1".to_string()),
                to: Some("n2".to_string()),
                kind: StarMapEdgeKind::RelatedTo,
                label: None,
                payload: None,
                from_target: None,
                to_target: None,
                from_endpoint: None,
                to_endpoint: None,
                from_endpoint_path: None,
                to_endpoint_path: None,
                created_at: now_epoch(),
                updated_at: now_epoch(),
            },
        )
        .unwrap();

        let dt = crate::starmap::semantic::StarMapDeepTarget {
            starmap_id: meta_a.starmap_id.clone(),
            path: vec![],
            target: crate::starmap::semantic::StarMapTargetDetail::Node {
                node_id: "n2".to_string(),
            },
        };
        update_starmap_edge(
            dir.path(),
            &meta_a.starmap_id,
            "e1",
            StarMapEdgePatch {
                kind: None,
                label: None,
                payload: None,
                from_target: None,
                to_target: None,
                from_endpoint: None,
                to_endpoint: Some(Some(
                    crate::starmap::types::StarMapEdgeEndpoint::DeepTarget { target: dt.clone() },
                )),
                from_endpoint_path: None,
                to_endpoint_path: None,
            },
        )
        .unwrap();
        let g = get_starmap_graph(dir.path(), &meta_a.starmap_id).unwrap();
        assert!(matches!(
            g.edges[0].to_endpoint,
            Some(crate::starmap::types::StarMapEdgeEndpoint::DeepTarget { .. })
        ));

        update_starmap_edge(
            dir.path(),
            &meta_a.starmap_id,
            "e1",
            StarMapEdgePatch {
                kind: None,
                label: None,
                payload: None,
                from_target: None,
                to_target: None,
                from_endpoint: None,
                to_endpoint: Some(None),
                from_endpoint_path: None,
                to_endpoint_path: None,
            },
        )
        .unwrap();
        let g = get_starmap_graph(dir.path(), &meta_a.starmap_id).unwrap();
        assert!(g.edges[0].to_endpoint.is_none());

        update_starmap_edge(
            dir.path(),
            &meta_a.starmap_id,
            "e1",
            StarMapEdgePatch {
                kind: None,
                label: None,
                payload: None,
                from_target: None,
                to_target: None,
                from_endpoint: Some(Some(crate::starmap::types::StarMapEdgeEndpoint::Node {
                    node_id: "n1".to_string(),
                })),
                to_endpoint: None,
                from_endpoint_path: None,
                to_endpoint_path: None,
            },
        )
        .unwrap();
        let g = get_starmap_graph(dir.path(), &meta_a.starmap_id).unwrap();
        assert!(g.edges[0].to_endpoint.is_none());
        assert!(matches!(
            g.edges[0].from_endpoint,
            Some(crate::starmap::types::StarMapEdgeEndpoint::Node { .. })
        ));
    }

    #[test]
    fn test_deep_target_resolution() {
        use crate::starmap::semantic::StarMapTargetResolveStatus::*;
        let dir = setup_workspace();
        let meta_a = create_starmap(dir.path(), "Map A", "", None).unwrap();

        let dt_missing_sm = crate::starmap::semantic::StarMapDeepTarget {
            starmap_id: "missing".to_string(),
            path: vec![],
            target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
        };
        assert_eq!(
            resolve_deep_target(dir.path(), &dt_missing_sm),
            MissingStarmap
        );

        let dt_missing_node = crate::starmap::semantic::StarMapDeepTarget {
            starmap_id: meta_a.starmap_id.clone(),
            path: vec![],
            target: crate::starmap::semantic::StarMapTargetDetail::Node {
                node_id: "non-existent".to_string(),
            },
        };
        assert_eq!(
            resolve_deep_target(dir.path(), &dt_missing_node),
            MissingNode
        );

        let mut g = get_starmap_graph(dir.path(), &meta_a.starmap_id).unwrap();
        g.nodes.push(StarMapNode {
            id: "n1".to_string(),
            title: "n1".to_string(),
            kind: StarMapNodeKind::Note,
            payload: None,
            tags: vec![],
            content: Default::default(),
            anchors: vec![crate::starmap::semantic::StarMapAnchor {
                anchor_id: "a1".to_string(),
                target: crate::starmap::semantic::StarMapAnchorTarget::Project {
                    project_id: "p".to_string(),
                },
                label: None,
                role: Default::default(),
            }],
            portal: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            provenance: Default::default(),
            created_at: now_epoch(),
            updated_at: now_epoch(),
        });
        save_starmap_graph(dir.path(), &meta_a.starmap_id, &g).unwrap();

        let dt_node_exists = crate::starmap::semantic::StarMapDeepTarget {
            starmap_id: meta_a.starmap_id.clone(),
            path: vec![],
            target: crate::starmap::semantic::StarMapTargetDetail::Node {
                node_id: "n1".to_string(),
            },
        };
        assert_eq!(resolve_deep_target(dir.path(), &dt_node_exists), Resolved);

        let dt_missing_anchor = crate::starmap::semantic::StarMapDeepTarget {
            starmap_id: meta_a.starmap_id.clone(),
            path: vec![],
            target: crate::starmap::semantic::StarMapTargetDetail::Anchor {
                node_id: "n1".to_string(),
                anchor_id: "missing".to_string(),
            },
        };
        assert_eq!(
            resolve_deep_target(dir.path(), &dt_missing_anchor),
            MissingAnchor
        );

        let dt_anchor_exists = crate::starmap::semantic::StarMapDeepTarget {
            starmap_id: meta_a.starmap_id.clone(),
            path: vec![],
            target: crate::starmap::semantic::StarMapTargetDetail::Anchor {
                node_id: "n1".to_string(),
                anchor_id: "a1".to_string(),
            },
        };
        assert_eq!(resolve_deep_target(dir.path(), &dt_anchor_exists), Resolved);

        let dt_too_deep = crate::starmap::semantic::StarMapDeepTarget {
            starmap_id: meta_a.starmap_id.clone(),
            path: vec![
                crate::starmap::semantic::StarMapPathSegment::EnterChild {
                    starmap_id: "dummy".to_string()
                };
                33
            ],
            target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
        };
        assert_eq!(resolve_deep_target(dir.path(), &dt_too_deep), TooDeep);
    }
}
