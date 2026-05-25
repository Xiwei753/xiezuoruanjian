use crate::error::{Error, Result};
use crate::mind_map::graph::{MindMapEdgeKind, MindMapGraph, MindMapGraphEdge, MindMapGraphNode, MindMapNodeKind};
use crate::mind_map::layout::{LayoutKind, MindMapLayout, MindMapLayoutNode};
use crate::starmap::{load_starmap_meta, starmaps_dir, update_starmap_stats, now_epoch};
use std::fs;
use std::path::Path;

pub fn get_starmap_graph(workspace: &Path, starmap_id: &str) -> Result<MindMapGraph> {
    let meta = load_starmap_meta(workspace, starmap_id)?;
    if meta.project_id.is_none() {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "StarMap is not bound to a project",
        )));
    }

    let graph_path = starmaps_dir(workspace).join(starmap_id).join("graph.json");
    if !graph_path.exists() {
        return Ok(MindMapGraph {
            schema_version: 2,
            id: starmap_id.to_string(),
            project_id: meta.project_id.unwrap(),
            title: meta.title.clone(),
            nodes: vec![],
            edges: vec![],
            anchors: vec![],
            links: vec![],
            created_at: now_epoch(),
            updated_at: now_epoch(),
        });
    }

    let json_str = fs::read_to_string(&graph_path)?;
    let graph: MindMapGraph = serde_json::from_str(&json_str)?;
    Ok(graph)
}

pub fn save_starmap_graph(workspace: &Path, starmap_id: &str, graph: &MindMapGraph) -> Result<()> {
    let starmap_dir = starmaps_dir(workspace).join(starmap_id);
    fs::create_dir_all(&starmap_dir)?;

    let graph_path = starmap_dir.join("graph.json");
    let json_str = serde_json::to_string_pretty(graph)?;
    fs::write(graph_path, json_str)?;

    let node_count = graph.nodes.len() as u32;
    let edge_count = graph.edges.len() as u32;

    // linked_chapter_count calculation
    let mut linked_chapters = 0;
    for node in &graph.nodes {
        if node.kind == MindMapNodeKind::Chapter {
            linked_chapters += 1;
        }
    }

    update_starmap_stats(workspace, starmap_id, node_count, edge_count, linked_chapters)?;

    Ok(())
}

pub fn get_starmap_layout(workspace: &Path, starmap_id: &str) -> Result<MindMapLayout> {
    let layout_path = starmaps_dir(workspace).join(starmap_id).join("layout.json");
    if !layout_path.exists() {
        return Ok(MindMapLayout {
            kind: LayoutKind::Freeform,
            nodes: vec![],
        });
    }

    let json_str = fs::read_to_string(&layout_path)?;
    let layout: MindMapLayout = serde_json::from_str(&json_str)?;
    Ok(layout)
}

pub fn save_starmap_layout(workspace: &Path, starmap_id: &str, layout: &MindMapLayout) -> Result<()> {
    let starmap_dir = starmaps_dir(workspace).join(starmap_id);
    fs::create_dir_all(&starmap_dir)?;

    let layout_path = starmap_dir.join("layout.json");
    let json_str = serde_json::to_string_pretty(layout)?;
    fs::write(layout_path, json_str)?;

    Ok(())
}

pub fn add_starmap_node(workspace: &Path, starmap_id: &str, node: MindMapGraphNode) -> Result<MindMapGraphNode> {
    let mut graph = get_starmap_graph(workspace, starmap_id)?;
    let new_node = node.clone();
    graph.nodes.push(new_node.clone());
    graph.updated_at = now_epoch();
    save_starmap_graph(workspace, starmap_id, &graph)?;
    Ok(new_node)
}

pub fn update_starmap_node(
    workspace: &Path,
    starmap_id: &str,
    node_id: &str,
    title: Option<String>,
    kind: Option<MindMapNodeKind>,
    payload: Option<serde_json::Value>,
    tags: Option<Vec<String>>
) -> Result<MindMapGraphNode> {
    let mut graph = get_starmap_graph(workspace, starmap_id)?;
    if let Some(node) = graph.nodes.iter_mut().find(|n| n.id == node_id) {
        if let Some(t) = title { node.title = t; }
        if let Some(k) = kind { node.kind = k; }
        if let Some(p) = payload { node.payload = Some(p); }
        if let Some(t) = tags { node.tags = t; }
        node.updated_at = now_epoch();
        let updated_node = node.clone();
        graph.updated_at = now_epoch();
        save_starmap_graph(workspace, starmap_id, &graph)?;
        Ok(updated_node)
    } else {
        Err(Error::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "Node not found")))
    }
}

pub fn delete_starmap_node(workspace: &Path, starmap_id: &str, node_id: &str) -> Result<()> {
    let mut graph = get_starmap_graph(workspace, starmap_id)?;
    let initial_node_count = graph.nodes.len();
    graph.nodes.retain(|n| n.id != node_id);
    if graph.nodes.len() == initial_node_count {
        return Err(Error::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "Node not found")));
    }

    // Cascade delete edges
    graph.edges.retain(|e| e.from != node_id && e.to != node_id);

    // Also remove from layout
    if let Ok(mut layout) = get_starmap_layout(workspace, starmap_id) {
        layout.nodes.retain(|n| n.node_id != node_id);
        let _ = save_starmap_layout(workspace, starmap_id, &layout);
    }

    graph.updated_at = now_epoch();
    save_starmap_graph(workspace, starmap_id, &graph)?;
    Ok(())
}

pub fn add_starmap_edge(workspace: &Path, starmap_id: &str, edge: MindMapGraphEdge) -> Result<MindMapGraphEdge> {
    let mut graph = get_starmap_graph(workspace, starmap_id)?;

    // Validate from and to nodes exist
    if !graph.nodes.iter().any(|n| n.id == edge.from) || !graph.nodes.iter().any(|n| n.id == edge.to) {
        return Err(Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Edge nodes do not exist")));
    }

    let new_edge = edge.clone();
    graph.edges.push(new_edge.clone());
    graph.updated_at = now_epoch();
    save_starmap_graph(workspace, starmap_id, &graph)?;
    Ok(new_edge)
}

pub fn update_starmap_edge(
    workspace: &Path,
    starmap_id: &str,
    edge_id: &str,
    kind: Option<MindMapEdgeKind>,
    label: Option<String>,
    payload: Option<serde_json::Value>
) -> Result<MindMapGraphEdge> {
    let mut graph = get_starmap_graph(workspace, starmap_id)?;
    if let Some(edge) = graph.edges.iter_mut().find(|e| e.id == edge_id) {
        if let Some(k) = kind { edge.kind = k; }
        // For label, we might want to allow clearing it, so we can't just use `Option<String>` easily,
        // but for now, we'll assume if Some, update it. If we need to clear, we can pass Some("".to_string()).
        if let Some(l) = label {
            if l.is_empty() {
                edge.label = None;
            } else {
                edge.label = Some(l);
            }
        }
        if let Some(p) = payload { edge.payload = Some(p); }
        edge.updated_at = now_epoch();
        let updated_edge = edge.clone();
        graph.updated_at = now_epoch();
        save_starmap_graph(workspace, starmap_id, &graph)?;
        Ok(updated_edge)
    } else {
        Err(Error::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "Edge not found")))
    }
}

pub fn delete_starmap_edge(workspace: &Path, starmap_id: &str, edge_id: &str) -> Result<()> {
    let mut graph = get_starmap_graph(workspace, starmap_id)?;
    let initial_edge_count = graph.edges.len();
    graph.edges.retain(|e| e.id != edge_id);
    if graph.edges.len() == initial_edge_count {
        return Err(Error::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "Edge not found")));
    }
    graph.updated_at = now_epoch();
    save_starmap_graph(workspace, starmap_id, &graph)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use crate::workspace::create_workspace;
    use crate::starmap::create_starmap;

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

        // 1. Get empty graph
        let mut graph = get_starmap_graph(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(graph.nodes.len(), 0);
        assert_eq!(graph.edges.len(), 0);

        // 2. Add node
        let node1 = MindMapGraphNode {
            id: "n1".to_string(),
            title: "Node 1".to_string(),
            kind: MindMapNodeKind::Note,
            payload: None,
            tags: vec![],
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        add_starmap_node(dir.path(), &meta.starmap_id, node1.clone()).unwrap();

        let node2 = MindMapGraphNode {
            id: "n2".to_string(),
            title: "Node 2".to_string(),
            kind: MindMapNodeKind::Concept,
            payload: None,
            tags: vec![],
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        add_starmap_node(dir.path(), &meta.starmap_id, node2.clone()).unwrap();

        graph = get_starmap_graph(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(graph.nodes.len(), 2);

        // Verify meta stats updated
        let refreshed_meta = load_starmap_meta(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(refreshed_meta.node_count, 2);

        // 3. Update node
        update_starmap_node(dir.path(), &meta.starmap_id, "n1", Some("Updated N1".to_string()), Some(MindMapNodeKind::Chapter), None, None).unwrap();
        graph = get_starmap_graph(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(graph.nodes.iter().find(|n| n.id == "n1").unwrap().title, "Updated N1");

        // Verify meta stats linked chapter count
        let refreshed_meta2 = load_starmap_meta(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(refreshed_meta2.linked_chapter_count, 1);

        // 4. Add edge
        let edge = MindMapGraphEdge {
            id: "e1".to_string(),
            from: "n1".to_string(),
            to: "n2".to_string(),
            kind: MindMapEdgeKind::RelatedTo,
            label: Some("relates".to_string()),
            payload: None,
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        add_starmap_edge(dir.path(), &meta.starmap_id, edge.clone()).unwrap();

        graph = get_starmap_graph(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(graph.edges.len(), 1);

        // 5. Update edge
        update_starmap_edge(dir.path(), &meta.starmap_id, "e1", Some(MindMapEdgeKind::Causes), Some("causes".to_string()), None).unwrap();
        graph = get_starmap_graph(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(graph.edges[0].label.as_deref(), Some("causes"));

        // 6. Delete node (cascades edge)
        delete_starmap_node(dir.path(), &meta.starmap_id, "n1").unwrap();
        graph = get_starmap_graph(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(graph.nodes.len(), 1); // n2 remains
        assert_eq!(graph.edges.len(), 0); // e1 deleted

        // Verify stats again
        let refreshed_meta3 = load_starmap_meta(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(refreshed_meta3.node_count, 1);
        assert_eq!(refreshed_meta3.edge_count, 0);
        assert_eq!(refreshed_meta3.linked_chapter_count, 0); // n1 was the chapter
    }

    #[test]
    fn test_starmap_layout_crud() {
        let dir = setup_workspace();
        let meta = create_starmap(dir.path(), "Layout Map", "desc", None).unwrap();
        crate::starmap::bind_starmap_to_project(dir.path(), &meta.starmap_id, "test_proj").unwrap();

        let mut layout = get_starmap_layout(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(layout.nodes.len(), 0);

        layout.nodes.push(MindMapLayoutNode {
            node_id: "n1".to_string(),
            x: 100.0,
            y: 200.0,
            width: 50.0,
            height: 50.0,
            radius: 25.0,
            collapsed: false,
            z_index: 0,
        });

        save_starmap_layout(dir.path(), &meta.starmap_id, &layout).unwrap();

        let loaded = get_starmap_layout(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(loaded.nodes.len(), 1);
        assert_eq!(loaded.nodes[0].x, 100.0);
    }
}
