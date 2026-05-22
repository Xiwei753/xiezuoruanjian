use crate::mind_map::graph::MindMapGraph;
use std::collections::HashSet;

#[derive(Debug, PartialEq)]
pub enum ValidationError {
    EmptyGraphId,
    EmptyProjectId,
    UnsupportedSchemaVersion(u32),
    EmptyNodeId,
    DuplicateNodeId(String),
    EmptyEdgeId,
    DuplicateEdgeId(String),
    EdgeReferencesMissingNode(String, String), // edge_id, node_id
    LinkReferencesMissingNode(String, String), // link_id, node_id
    LinkReferencesMissingAnchor(String, String), // link_id, anchor_id
    AnchorChapterNotInProject(String, String), // anchor_id, chapter_id
    ProjectNotFound(String),
}

pub fn validate_graph(graph: &MindMapGraph, core: &crate::facade::WriterCore) -> Result<(), ValidationError> {
    if graph.id.is_empty() {
        return Err(ValidationError::EmptyGraphId);
    }
    if graph.project_id.is_empty() {
        return Err(ValidationError::EmptyProjectId);
    }
    if graph.schema_version != 2 {
        return Err(ValidationError::UnsupportedSchemaVersion(graph.schema_version));
    }

    // Validate project existence
    match core.list_projects() {
        Ok(projects) => {
            if !projects.into_iter().any(|p| p.id == graph.project_id) {
                return Err(ValidationError::ProjectNotFound(graph.project_id.clone()));
            }
        }
        Err(_) => {
            // Other errors treated as missing or invalid project for validation
            return Err(ValidationError::ProjectNotFound(graph.project_id.clone()));
        }
    }

    // Gather all valid node ids
    let mut node_ids = HashSet::new();
    for node in &graph.nodes {
        if node.id.is_empty() {
            return Err(ValidationError::EmptyNodeId);
        }
        if !node_ids.insert(node.id.clone()) {
            return Err(ValidationError::DuplicateNodeId(node.id.clone()));
        }
    }

    // Validate edges
    let mut edge_ids = HashSet::new();
    for edge in &graph.edges {
        if edge.id.is_empty() {
            return Err(ValidationError::EmptyEdgeId);
        }
        if !edge_ids.insert(edge.id.clone()) {
            return Err(ValidationError::DuplicateEdgeId(edge.id.clone()));
        }
        if !node_ids.contains(&edge.from) {
            return Err(ValidationError::EdgeReferencesMissingNode(edge.id.clone(), edge.from.clone()));
        }
        if !node_ids.contains(&edge.to) {
            return Err(ValidationError::EdgeReferencesMissingNode(edge.id.clone(), edge.to.clone()));
        }
    }

    // Gather valid anchors
    let mut anchor_ids = HashSet::new();

    // Cache for chapter existence check
    let mut valid_chapters = HashSet::new();

    for anchor in &graph.anchors {
        anchor_ids.insert(anchor.id.clone());

        // Validate anchor chapter belongs to project
        if !valid_chapters.contains(&anchor.chapter_id) {
            let vols = core.list_volumes(&graph.project_id).unwrap_or_default();
            let mut found = false;
            for vol in vols {
                let chapters = core.list_chapters(&graph.project_id, &vol.id).unwrap_or_default();
                if chapters.iter().any(|c| c.id == anchor.chapter_id) {
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(ValidationError::AnchorChapterNotInProject(anchor.id.clone(), anchor.chapter_id.clone()));
            } else {
                valid_chapters.insert(anchor.chapter_id.clone());
            }
        }
    }

    // Validate links
    for link in &graph.links {
        if !node_ids.contains(&link.node_id) {
            return Err(ValidationError::LinkReferencesMissingNode(link.id.clone(), link.node_id.clone()));
        }
        if !anchor_ids.contains(&link.anchor_id) {
            return Err(ValidationError::LinkReferencesMissingAnchor(link.id.clone(), link.anchor_id.clone()));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use crate::facade::WriterCore;
    use crate::mind_map::graph::{MindMapGraphNode, MindMapNodeKind, MindMapGraphEdge, MindMapEdgeKind};

    #[test]
    fn test_validate_graph_missing_nodes() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let core = WriterCore::new(temp_dir.path());
        let proj = core.create_project("Test Project").unwrap();

        let graph = MindMapGraph {
            schema_version: 2,
            id: "g1".into(),
            project_id: proj.id.clone(),
            title: "Test".into(),
            nodes: vec![
                MindMapGraphNode {
                    id: "n1".into(),
                    title: "Node 1".into(),
                    kind: MindMapNodeKind::Note,
                    payload: None,
                    tags: vec![],
                    created_at: 0,
                    updated_at: 0,
                }
            ],
            edges: vec![
                MindMapGraphEdge {
                    id: "e1".into(),
                    from: "n1".into(),
                    to: "n2".into(), // n2 is missing
                    kind: MindMapEdgeKind::Custom,
                    label: None,
                    payload: None,
                    created_at: 0,
                    updated_at: 0,
                }
            ],
            anchors: vec![],
            links: vec![],
            created_at: 0,
            updated_at: 0,
        };

        let result = validate_graph(&graph, &core);
        assert_eq!(result, Err(ValidationError::EdgeReferencesMissingNode("e1".into(), "n2".into())));
    }

    #[test]
    fn test_validate_graph_duplicate_nodes() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let core = WriterCore::new(temp_dir.path());
        let proj = core.create_project("Test Project").unwrap();

        let graph = MindMapGraph {
            schema_version: 2,
            id: "g1".into(),
            project_id: proj.id.clone(),
            title: "Test".into(),
            nodes: vec![
                MindMapGraphNode {
                    id: "n1".into(),
                    title: "Node 1".into(),
                    kind: MindMapNodeKind::Note,
                    payload: None,
                    tags: vec![],
                    created_at: 0,
                    updated_at: 0,
                },
                MindMapGraphNode {
                    id: "n1".into(), // Duplicate
                    title: "Node 1".into(),
                    kind: MindMapNodeKind::Note,
                    payload: None,
                    tags: vec![],
                    created_at: 0,
                    updated_at: 0,
                }
            ],
            edges: vec![],
            anchors: vec![],
            links: vec![],
            created_at: 0,
            updated_at: 0,
        };

        let result = validate_graph(&graph, &core);
        assert_eq!(result, Err(ValidationError::DuplicateNodeId("n1".into())));
    }
}
