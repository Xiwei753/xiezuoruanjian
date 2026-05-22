use crate::mind_map::graph::{MindMapGraph, MindMapGraphNode, MindMapGraphEdge, MindMapNodeKind, MindMapEdgeKind};
use crate::mind_map::anchor::{MindMapAnchor, MindMapLink};

// Represents V1 structures for parsing old files
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct V1MindMapNode {
    pub id: String,
    pub title: String,
    pub kind: String, // String in V1
    // Layout and style were mixed in
    #[allow(dead_code)]
    pub parent_id: Option<String>,
    #[allow(dead_code)]
    pub depth: i32,
    #[allow(dead_code)]
    pub x: f32,
    #[allow(dead_code)]
    pub y: f32,
    #[allow(dead_code)]
    pub radius: f32,
    #[allow(dead_code)]
    pub width: f32,
    #[allow(dead_code)]
    pub height: f32,
    #[allow(dead_code)]
    pub collapsed: bool,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct V1MindMapEdge {
    pub from: String,
    pub to: String,
    pub kind: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct V1MindMapGraph {
    pub id: String,
    pub nodes: Vec<V1MindMapNode>,
    pub edges: Vec<V1MindMapEdge>,
    #[serde(default)]
    pub anchors: Vec<MindMapAnchor>,
    #[serde(default)]
    pub links: Vec<MindMapLink>,
}

pub fn migrate_graph_schema(json_str: &str, project_id: &str) -> Result<MindMapGraph, crate::error::Error> {
    // Try to parse as V2 first
    if let Ok(v2_graph) = serde_json::from_str::<MindMapGraph>(json_str) {
        if v2_graph.schema_version == 2 {
            return Ok(v2_graph);
        } else {
             return Err(crate::error::Error::Json(serde::de::Error::custom("Unsupported schema version")));
        }
    }

    // Try parsing as V1
    let v1_graph: V1MindMapGraph = serde_json::from_str(json_str)?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let v2_nodes = v1_graph.nodes.into_iter().map(|n| {
        let kind = match n.kind.as_str() {
            "Project" => MindMapNodeKind::Project,
            "Volume" => MindMapNodeKind::Volume,
            "Chapter" => MindMapNodeKind::Chapter,
            "Character" => MindMapNodeKind::Character,
            "Event" => MindMapNodeKind::Event,
            "Location" => MindMapNodeKind::Location,
            _ => MindMapNodeKind::Custom,
        };

        MindMapGraphNode {
            id: n.id,
            title: n.title,
            kind,
            payload: None,
            tags: vec![],
            created_at: now,
            updated_at: now,
        }
    }).collect();

    let v2_edges = v1_graph.edges.into_iter().enumerate().map(|(i, e)| {
        let kind = match e.kind.as_str() {
            "hierarchy" => MindMapEdgeKind::Contains,
            "References" => MindMapEdgeKind::References,
            _ => MindMapEdgeKind::Custom,
        };
        MindMapGraphEdge {
            id: format!("migrated_edge_{}_{}_{}", i, e.from, e.to),
            from: e.from,
            to: e.to,
            kind,
            label: None,
            payload: None,
            created_at: now,
            updated_at: now,
        }
    }).collect();

    Ok(MindMapGraph {
        schema_version: 2,
        id: v1_graph.id,
        project_id: project_id.to_string(),
        title: "Migrated Graph".to_string(),
        nodes: v2_nodes,
        edges: v2_edges,
        anchors: v1_graph.anchors,
        links: v1_graph.links,
        created_at: now,
        updated_at: now,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migration_from_v1() {
        let v1_json = r#"{
            "id": "graph_1",
            "nodes": [
                {
                    "id": "node_1",
                    "title": "Character A",
                    "kind": "Character",
                    "parentId": null,
                    "depth": 0,
                    "x": 10.0,
                    "y": 20.0,
                    "radius": 10.0,
                    "width": 100.0,
                    "height": 50.0,
                    "collapsed": false
                }
            ],
            "edges": [
                {
                    "from": "node_1",
                    "to": "node_2",
                    "kind": "References"
                }
            ]
        }"#;

        let migrated = migrate_graph_schema(v1_json, "p1").unwrap();
        assert_eq!(migrated.schema_version, 2);
        assert_eq!(migrated.project_id, "p1");
        assert_eq!(migrated.nodes.len(), 1);
        assert_eq!(migrated.nodes[0].kind, crate::mind_map::graph::MindMapNodeKind::Character);
        assert_eq!(migrated.edges.len(), 1);
        assert_eq!(migrated.edges[0].kind, crate::mind_map::graph::MindMapEdgeKind::References);
    }

    #[test]
    fn test_unknown_schema_version_returns_error() {
        let future_json = r#"{
            "schemaVersion": 999,
            "id": "g1",
            "projectId": "p1",
            "title": "Future",
            "nodes": [],
            "edges": [],
            "anchors": [],
            "links": [],
            "createdAt": 0,
            "updatedAt": 0
        }"#;

        let result = migrate_graph_schema(future_json, "p1");
        assert!(result.is_err());
    }
}
