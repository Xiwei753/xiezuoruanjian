mod embed;
mod graph;
mod layout;
mod link;

use serde::{Deserialize, Serialize};

pub use embed::*;
pub use graph::*;
pub use layout::*;
pub use link::*;

fn default_accent_color() -> String {
    "#7B8CDE".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapDocument {
    pub starmap_id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub parent_starmap_id: Option<String>,
    #[serde(default)]
    pub is_main_for_project: bool,
    #[serde(default = "default_accent_color")]
    pub accent_color: String,
    #[serde(default)]
    pub graph: StarMapGraph,
    #[serde(default)]
    pub layout: StarMapLayout,
    #[serde(default)]
    pub viewport: StarMapViewport,
    #[serde(default)]
    pub child_map_placements: Vec<StarMapChildMapPlacement>,
    #[serde(default)]
    pub hyperlinks: Vec<StarMapHyperlink>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl Default for StarMapDocument {
    fn default() -> Self {
        Self {
            starmap_id: String::new(),
            title: String::new(),
            description: String::new(),
            project_id: None,
            parent_starmap_id: None,
            is_main_for_project: false,
            accent_color: default_accent_color(),
            graph: StarMapGraph::default(),
            layout: StarMapLayout::default(),
            viewport: StarMapViewport::default(),
            child_map_placements: vec![],
            hyperlinks: vec![],
            created_at: 0,
            updated_at: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEndpointPath {
    #[serde(default)]
    pub segments: Vec<StarMapEndpointPathSegment>,
    pub endpoint: StarMapEdgeEndpoint,
}

impl StarMapEndpointPath {
    pub fn has_cycle(&self) -> bool {
        let mut visited = std::collections::HashSet::new();
        for seg in &self.segments {
            let StarMapEndpointPathSegment::EnterChildMap { starmap_id } = seg;
            if !visited.insert(starmap_id.clone()) {
                return true;
            }
        }
        false
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StarMapEndpointPathSegment {
    EnterChildMap { starmap_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapChildMapPlacement {
    pub instance_id: String,
    pub target_starmap_id: String,
    pub placement: StarMapEmbedPlacement,
    #[serde(default)]
    pub target_viewport: StarMapEmbedViewport,
    #[serde(default)]
    pub display_policy: crate::starmap::semantic::StarMapDisplayPolicy,
    #[serde(default)]
    pub open_behavior: crate::starmap::semantic::StarMapOpenBehavior,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapHyperlink {
    pub hyperlink_id: String,
    pub source: StarMapEndpointPath,
    pub target_uri: String,
    pub label: Option<String>,
    #[serde(default)]
    pub target_starmap_id: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_endpoint_path_multi_layer_roundtrip() {
        let path = StarMapEndpointPath {
            segments: vec![
                StarMapEndpointPathSegment::EnterChildMap {
                    starmap_id: "sm_child_a".to_string(),
                },
                StarMapEndpointPathSegment::EnterChildMap {
                    starmap_id: "sm_child_b".to_string(),
                },
            ],
            endpoint: StarMapEdgeEndpoint::Anchor {
                node_id: "n2".to_string(),
                anchor_id: "a1".to_string(),
            },
        };

        let json = serde_json::to_string(&path).unwrap();
        let deserialized: StarMapEndpointPath = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized, path);
        assert_eq!(deserialized.segments.len(), 2);
        assert_eq!(
            deserialized.segments[0],
            StarMapEndpointPathSegment::EnterChildMap {
                starmap_id: "sm_child_a".to_string(),
            }
        );
        assert_eq!(
            deserialized.segments[1],
            StarMapEndpointPathSegment::EnterChildMap {
                starmap_id: "sm_child_b".to_string(),
            }
        );
        assert_eq!(
            deserialized.endpoint,
            StarMapEdgeEndpoint::Anchor {
                node_id: "n2".to_string(),
                anchor_id: "a1".to_string(),
            }
        );
    }

    #[test]
    fn test_endpoint_path_cycle_detection() {
        let no_cycle = StarMapEndpointPath {
            segments: vec![
                StarMapEndpointPathSegment::EnterChildMap {
                    starmap_id: "sm_a".to_string(),
                },
                StarMapEndpointPathSegment::EnterChildMap {
                    starmap_id: "sm_b".to_string(),
                },
            ],
            endpoint: StarMapEdgeEndpoint::Node {
                node_id: "n1".to_string(),
            },
        };
        assert!(!no_cycle.has_cycle());

        let with_cycle = StarMapEndpointPath {
            segments: vec![
                StarMapEndpointPathSegment::EnterChildMap {
                    starmap_id: "sm_a".to_string(),
                },
                StarMapEndpointPathSegment::EnterChildMap {
                    starmap_id: "sm_b".to_string(),
                },
                StarMapEndpointPathSegment::EnterChildMap {
                    starmap_id: "sm_a".to_string(),
                },
            ],
            endpoint: StarMapEdgeEndpoint::Node {
                node_id: "n2".to_string(),
            },
        };
        assert!(with_cycle.has_cycle());

        let empty = StarMapEndpointPath {
            segments: vec![],
            endpoint: StarMapEdgeEndpoint::Starmap,
        };
        assert!(!empty.has_cycle());

        let single = StarMapEndpointPath {
            segments: vec![StarMapEndpointPathSegment::EnterChildMap {
                starmap_id: "sm_a".to_string(),
            }],
            endpoint: StarMapEdgeEndpoint::Node {
                node_id: "n1".to_string(),
            },
        };
        assert!(!single.has_cycle());
    }

    #[test]
    fn test_child_starmap_is_not_node() {
        let placement = StarMapChildMapPlacement {
            instance_id: "embed_1".to_string(),
            target_starmap_id: "sm_child".to_string(),
            placement: StarMapEmbedPlacement::default(),
            target_viewport: StarMapEmbedViewport::default(),
            display_policy: crate::starmap::semantic::StarMapDisplayPolicy::default(),
            open_behavior: crate::starmap::semantic::StarMapOpenBehavior::default(),
        };

        let doc = StarMapDocument {
            starmap_id: "sm_parent".to_string(),
            title: "Parent".to_string(),
            child_map_placements: vec![placement.clone()],
            ..Default::default()
        };

        assert!(doc.graph.nodes.is_empty());
        assert_eq!(doc.child_map_placements.len(), 1);
        assert_eq!(doc.child_map_placements[0].target_starmap_id, "sm_child");

        let json = serde_json::to_string(&placement).unwrap();
        let deserialized: StarMapChildMapPlacement = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.instance_id, "embed_1");
        assert_eq!(deserialized.target_starmap_id, "sm_child");
    }

    #[test]
    fn test_hyperlink_does_not_produce_edge() {
        let hyperlink = StarMapHyperlink {
            hyperlink_id: "hl_1".to_string(),
            source: StarMapEndpointPath {
                segments: vec![],
                endpoint: StarMapEdgeEndpoint::Node {
                    node_id: "n1".to_string(),
                },
            },
            target_uri: "https://example.com".to_string(),
            label: Some("Example".to_string()),
            target_starmap_id: Some("sm_other".to_string()),
            created_at: 0,
            updated_at: 0,
        };

        let doc = StarMapDocument {
            starmap_id: "sm_1".to_string(),
            title: "Test".to_string(),
            hyperlinks: vec![hyperlink.clone()],
            ..Default::default()
        };

        assert!(doc.graph.edges.is_empty());
        assert_eq!(doc.hyperlinks.len(), 1);
        assert_eq!(doc.hyperlinks[0].hyperlink_id, "hl_1");

        let json = serde_json::to_string(&hyperlink).unwrap();
        let deserialized: StarMapHyperlink = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.hyperlink_id, "hl_1");
        assert_eq!(deserialized.target_uri, "https://example.com");
        assert_eq!(deserialized.target_starmap_id, Some("sm_other".to_string()));
    }

    #[test]
    fn test_containment_does_not_produce_edge() {
        let doc = StarMapDocument {
            starmap_id: "sm_parent".to_string(),
            title: "Parent Map".to_string(),
            child_map_placements: vec![
                StarMapChildMapPlacement {
                    instance_id: "embed_1".to_string(),
                    target_starmap_id: "sm_child_1".to_string(),
                    placement: StarMapEmbedPlacement::default(),
                    target_viewport: StarMapEmbedViewport::default(),
                    display_policy: crate::starmap::semantic::StarMapDisplayPolicy::default(),
                    open_behavior: crate::starmap::semantic::StarMapOpenBehavior::default(),
                },
                StarMapChildMapPlacement {
                    instance_id: "embed_2".to_string(),
                    target_starmap_id: "sm_child_2".to_string(),
                    placement: StarMapEmbedPlacement::default(),
                    target_viewport: StarMapEmbedViewport::default(),
                    display_policy: crate::starmap::semantic::StarMapDisplayPolicy::default(),
                    open_behavior: crate::starmap::semantic::StarMapOpenBehavior::default(),
                },
            ],
            ..Default::default()
        };

        assert!(doc.graph.edges.is_empty());
        assert_eq!(doc.child_map_placements.len(), 2);

        let contains_edges: Vec<_> = doc
            .graph
            .edges
            .iter()
            .filter(|e| e.kind == StarMapEdgeKind::Contains)
            .collect();
        assert!(contains_edges.is_empty());
    }

    #[test]
    fn test_starmap_document_roundtrip() {
        let doc = StarMapDocument {
            starmap_id: "sm_1".to_string(),
            title: "Test Document".to_string(),
            description: "A test".to_string(),
            project_id: Some("proj_1".to_string()),
            parent_starmap_id: None,
            is_main_for_project: true,
            accent_color: "#FF0000".to_string(),
            graph: StarMapGraph::default(),
            layout: StarMapLayout::default(),
            viewport: StarMapViewport::default(),
            child_map_placements: vec![],
            hyperlinks: vec![],
            created_at: 1000,
            updated_at: 2000,
        };

        let json = serde_json::to_string(&doc).unwrap();
        let deserialized: StarMapDocument = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.starmap_id, "sm_1");
        assert_eq!(deserialized.title, "Test Document");
        assert_eq!(deserialized.project_id, Some("proj_1".to_string()));
        assert!(deserialized.is_main_for_project);
        assert_eq!(deserialized.accent_color, "#FF0000");
    }

    #[test]
    fn test_edge_endpoint_path_roundtrip() {
        let edge = StarMapEdge {
            id: "e1".to_string(),
            from: None,
            to: None,
            kind: StarMapEdgeKind::RelatedTo,
            label: None,
            payload: None,
            from_target: None,
            to_target: None,
            from_endpoint: None,
            to_endpoint: None,
            from_endpoint_path: Some(StarMapEndpointPath {
                segments: vec![StarMapEndpointPathSegment::EnterChildMap {
                    starmap_id: "sm_child".to_string(),
                }],
                endpoint: StarMapEdgeEndpoint::Node {
                    node_id: "n1".to_string(),
                },
            }),
            to_endpoint_path: Some(StarMapEndpointPath {
                segments: vec![],
                endpoint: StarMapEdgeEndpoint::Starmap,
            }),
            created_at: 0,
            updated_at: 0,
        };

        let json = serde_json::to_string(&edge).unwrap();
        let deserialized: StarMapEdge = serde_json::from_str(&json).unwrap();

        assert!(deserialized.from.is_none());
        assert!(deserialized.to.is_none());
        assert!(deserialized.from_endpoint_path.is_some());
        assert!(deserialized.to_endpoint_path.is_some());

        let from_path = deserialized.from_endpoint_path.unwrap();
        assert_eq!(from_path.segments.len(), 1);
        assert_eq!(
            from_path.segments[0],
            StarMapEndpointPathSegment::EnterChildMap {
                starmap_id: "sm_child".to_string(),
            }
        );
    }

    #[test]
    fn test_edge_legacy_fields_backward_compatible() {
        let old_json = r#"{
            "id": "e_old",
            "from": "n1",
            "to": "n2",
            "kind": "relatedTo",
            "label": null,
            "payload": null,
            "createdAt": 0,
            "updatedAt": 0
        }"#;

        let edge: StarMapEdge = serde_json::from_str(old_json).unwrap();
        assert_eq!(edge.from, Some("n1".to_string()));
        assert_eq!(edge.to, Some("n2".to_string()));
        assert!(edge.from_endpoint_path.is_none());
        assert!(edge.to_endpoint_path.is_none());
    }
}
