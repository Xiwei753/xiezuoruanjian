use crate::starmap::semantic::StarMapTargetDetail;
use crate::starmap::types::reference::{StarMapPathSegment, StarMapTargetPath};
use crate::starmap::types::*;

#[test]
fn test_target_path_multi_layer_roundtrip() {
    let path = StarMapTargetPath {
        starmap_id: "sm_root".to_string(),
        segments: vec![
            StarMapPathSegment::EnterEmbed {
                instance_id: "embed_a".to_string(),
            },
            StarMapPathSegment::EnterPortal {
                node_id: "portal_b".to_string(),
            },
        ],
        target: StarMapTargetDetail::Anchor {
            node_id: "n2".to_string(),
            anchor_id: "a1".to_string(),
        },
    };

    let json = serde_json::to_string(&path).unwrap();
    let deserialized: StarMapTargetPath = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized, path);
    assert_eq!(deserialized.segments.len(), 2);
    assert_eq!(
        deserialized.segments[0],
        StarMapPathSegment::EnterEmbed {
            instance_id: "embed_a".to_string(),
        }
    );
    assert_eq!(
        deserialized.segments[1],
        StarMapPathSegment::EnterPortal {
            node_id: "portal_b".to_string(),
        }
    );
}

#[test]
fn test_hyperlink_roundtrip() {
    let hyperlink = StarMapHyperlink {
        hyperlink_id: "hl_1".to_string(),
        source: StarMapTargetPath {
            starmap_id: "sm_1".to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n1".to_string(),
            },
        },
        target_uri: "https://example.com".to_string(),
        label: Some("Example".to_string()),
        created_at: 0,
        updated_at: 0,
    };

    let json = serde_json::to_string(&hyperlink).unwrap();
    let deserialized: StarMapHyperlink = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.hyperlink_id, "hl_1");
    assert_eq!(deserialized.target_uri, "https://example.com");
}

#[test]
fn test_edge_target_path_roundtrip() {
    let edge = StarMapEdge {
        id: "e1".to_string(),
        from: StarMapTargetPath {
            starmap_id: "sm_1".to_string(),
            segments: vec![StarMapPathSegment::EnterEmbed {
                instance_id: "embed_child".to_string(),
            }],
            target: StarMapTargetDetail::Node {
                node_id: "n1".to_string(),
            },
        },
        to: StarMapTargetPath {
            starmap_id: "sm_1".to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Starmap,
        },
        kind: StarMapEdgeKind::RelatedTo,
        label: None,
        payload: None,
        created_at: 0,
        updated_at: 0,
    };

    let json = serde_json::to_string(&edge).unwrap();
    let deserialized: StarMapEdge = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.from.segments.len(), 1);
    assert_eq!(
        deserialized.from.segments[0],
        StarMapPathSegment::EnterEmbed {
            instance_id: "embed_child".to_string(),
        }
    );
    assert!(deserialized.to.segments.is_empty());
}

#[test]
fn test_starmap_graph_default() {
    let graph = StarMapGraph::default();
    assert_eq!(graph.schema_version, 1);
    assert!(graph.nodes.is_empty());
    assert!(graph.edges.is_empty());
    assert!(graph.embeds.is_empty());
    assert!(graph.links.is_empty());
    assert!(graph.hyperlinks.is_empty());
}
