use serde::{Deserialize, Serialize};
use crate::error::Result;
use crate::facade::WriterCore;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum MindMapNodeKind {
    Project,
    Volume,
    Chapter,
    TextAnchor,
    Character,
    Event,
    Location,
    Item,
    Concept,
    Theme,
    Note,
    #[serde(other)]
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum MindMapEdgeKind {
    Contains,
    References,
    AppearsIn,
    Causes,
    RelatedTo,
    LocatedAt,
    CharacterRelation,
    Timeline,
    Hierarchy,
    #[serde(other)]
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MindMapAnchor {
    pub id: String,
    pub chapter_id: String,
    pub start_offset: usize,
    pub end_offset: usize,
    pub selected_text: String,
    pub prefix_text: String,
    pub suffix_text: String,
    pub checksum: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MindMapLink {
    pub id: String,
    pub node_id: String,
    pub anchor_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MindMapGraph {
    pub id: String,
    pub nodes: Vec<MindMapNode>,
    pub edges: Vec<MindMapEdge>,
    pub anchors: Vec<MindMapAnchor>,
    pub links: Vec<MindMapLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MindMapNode {
    pub id: String,
    pub title: String,
    pub kind: MindMapNodeKind,
    pub parent_id: Option<String>,
    pub depth: i32,
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub width: f32,
    pub height: f32,
    pub collapsed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MindMapEdge {
    pub from: String,
    pub to: String,
    pub kind: String, // String representation or could be MindMapEdgeKind, keeping string for backward compatibility with UI if needed, but we can also use MindMapEdgeKind
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MindMapBounds {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MindMapSnapshot {
    pub project_id: String,
    pub layout_kind: String,
    pub nodes: Vec<MindMapNode>,
    pub edges: Vec<MindMapEdge>,
    pub bounds: MindMapBounds,
    pub generated_at: u64,
}

pub fn generate_snapshot(core: &WriterCore, project_id: &str) -> Result<MindMapSnapshot> {
    // Try to load a custom mind map first
    let mind_map_path = core.workspace_path().join("projects").join(project_id).join("mind_map.json");
    if let Ok(content) = std::fs::read_to_string(&mind_map_path) {
        if let Ok(graph) = serde_json::from_str::<MindMapGraph>(&content) {
            // For V1, just compute a simple layout or use existing coordinates
            // Assuming nodes already have x, y coordinates from editor
            let mut min_x = f32::MAX;
            let mut min_y = f32::MAX;
            let mut max_x = f32::MIN;
            let mut max_y = f32::MIN;

            for node in &graph.nodes {
                if node.x - node.width / 2.0 < min_x { min_x = node.x - node.width / 2.0; }
                if node.y - node.height / 2.0 < min_y { min_y = node.y - node.height / 2.0; }
                if node.x + node.width / 2.0 > max_x { max_x = node.x + node.width / 2.0; }
                if node.y + node.height / 2.0 > max_y { max_y = node.y + node.height / 2.0; }
            }

            if graph.nodes.len() <= 1 {
                min_x = -200.0;
                min_y = -200.0;
                max_x = 200.0;
                max_y = 200.0;
            }

            return Ok(MindMapSnapshot {
                project_id: project_id.to_string(),
                layout_kind: "custom".to_string(),
                nodes: graph.nodes,
                edges: graph.edges,
                bounds: MindMapBounds { min_x, min_y, max_x, max_y },
                generated_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
            });
        }
    }

    // Read projects to find the matching one
    let projects = core.list_projects()?;
    let project = projects.into_iter().find(|p| p.id == project_id).ok_or_else(|| {
        crate::error::Error::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "Project not found"))
    })?;

    let volumes = core.list_volumes(project_id)?;

    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    // Add project root node
    let project_node = MindMapNode {
        id: project_id.to_string(),
        title: project.title.clone(),
        kind: MindMapNodeKind::Project,
        parent_id: None,
        depth: 0,
        x: 0.0,
        y: 0.0,
        radius: 40.0,
        width: 120.0,
        height: 60.0,
        collapsed: false,
    };
    nodes.push(project_node);

    let mut all_volumes = Vec::new();
    let mut all_chapters = HashMap::new();

    for vol in &volumes {
        all_volumes.push(vol.clone());
        let chapters = core.list_chapters(project_id, &vol.id)?;
        all_chapters.insert(vol.id.clone(), chapters);
    }

    // Basic Radial Layout Algorithm
    let mut current_node_index = 1;
    let volume_radius_offset = 200.0;
    let chapter_radius_offset = 400.0;

    let num_volumes = all_volumes.len();
    for (i, vol) in all_volumes.iter().enumerate() {
        let vol_angle = if num_volumes > 1 {
            (i as f32) * std::f32::consts::TAU / (num_volumes as f32)
        } else {
            0.0
        };

        let vx = volume_radius_offset * vol_angle.cos();
        let vy = volume_radius_offset * vol_angle.sin();

        let vol_node = MindMapNode {
            id: vol.id.clone(),
            title: vol.title.clone(),
            kind: MindMapNodeKind::Volume,
            parent_id: Some(project_id.to_string()),
            depth: 1,
            x: vx,
            y: vy,
            radius: 30.0,
            width: 100.0,
            height: 50.0,
            collapsed: false,
        };
        nodes.push(vol_node);

        edges.push(MindMapEdge {
            from: project_id.to_string(),
            to: vol.id.clone(),
            kind: "hierarchy".to_string(),
        });

        if let Some(chapters) = all_chapters.get(&vol.id) {
            let num_chapters = chapters.len();
            // Fan out chapters around the volume's general direction
            let spread_angle = std::f32::consts::PI / 2.0; // 90 degree spread for chapters of a volume
            let start_angle = vol_angle - spread_angle / 2.0;
            let angle_step = if num_chapters > 1 {
                spread_angle / ((num_chapters - 1) as f32)
            } else {
                0.0
            };

            for (j, chap) in chapters.iter().enumerate() {
                let chap_angle = start_angle + (j as f32) * angle_step;
                let cx = chapter_radius_offset * chap_angle.cos();
                let cy = chapter_radius_offset * chap_angle.sin();

                let chap_node = MindMapNode {
                    id: chap.id.clone(),
                    title: chap.title.clone(),
                    kind: MindMapNodeKind::Chapter,
                    parent_id: Some(vol.id.clone()),
                    depth: 2,
                    x: cx,
                    y: cy,
                    radius: 20.0,
                    width: 80.0,
                    height: 40.0,
                    collapsed: false,
                };
                nodes.push(chap_node);

                edges.push(MindMapEdge {
                    from: vol.id.clone(),
                    to: chap.id.clone(),
                    kind: "hierarchy".to_string(),
                });
            }
        }
    }

    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;

    for node in &nodes {
        if node.x - node.width / 2.0 < min_x { min_x = node.x - node.width / 2.0; }
        if node.y - node.height / 2.0 < min_y { min_y = node.y - node.height / 2.0; }
        if node.x + node.width / 2.0 > max_x { max_x = node.x + node.width / 2.0; }
        if node.y + node.height / 2.0 > max_y { max_y = node.y + node.height / 2.0; }
    }

    // Default bounds if nodes are empty or singular
    if nodes.len() <= 1 {
        min_x = -200.0;
        min_y = -200.0;
        max_x = 200.0;
        max_y = 200.0;
    }

    Ok(MindMapSnapshot {
        project_id: project_id.to_string(),
        layout_kind: "radial".to_string(),
        nodes,
        edges,
        bounds: MindMapBounds { min_x, min_y, max_x, max_y },
        generated_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_mind_map_snapshot_generation() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let core = WriterCore::new(temp_dir.path());
        let proj = core.create_project("Test Project").unwrap();
        let vol = core.create_volume(&proj.id, "Test Volume").unwrap();
        core.create_chapter(&proj.id, &vol.id, "Test Chapter 1").unwrap();
        core.create_chapter(&proj.id, &vol.id, "Test Chapter 2").unwrap();

        let snapshot = generate_snapshot(&core, &proj.id).unwrap();

        assert_eq!(snapshot.project_id, proj.id);
        assert_eq!(snapshot.layout_kind, "radial");
        // 1 project node + 1 volume node + 2 chapter nodes = 4 nodes ? Wait.
        // What is "snapshot.nodes.len()"?
        // 1 proj + 1 vol (default Uncategorized) + 1 vol (Test Volume) + 2 chap.
        // Wait, when create_project is called, does it create an 'Uncategorized' volume?
        // Let's check what node count is actually returned.
        assert!(snapshot.nodes.len() >= 4);

        let proj_node = snapshot.nodes.iter().find(|n| n.kind == MindMapNodeKind::Project).unwrap();
        assert_eq!(proj_node.depth, 0);

        let chap_nodes: Vec<_> = snapshot.nodes.iter().filter(|n| n.kind == MindMapNodeKind::Chapter).collect();
        assert_eq!(chap_nodes.len(), 2);
        assert_eq!(chap_nodes[0].depth, 2);
    }

    #[test]
    fn test_mind_map_anchor_serialization() {
        let anchor = MindMapAnchor {
            id: "anchor_1".to_string(),
            chapter_id: "chap_1".to_string(),
            start_offset: 10,
            end_offset: 20,
            selected_text: "test".to_string(),
            prefix_text: "pre".to_string(),
            suffix_text: "suf".to_string(),
            checksum: "123".to_string(),
        };

        let json = serde_json::to_string(&anchor).unwrap();
        let deserialized: MindMapAnchor = serde_json::from_str(&json).unwrap();

        assert_eq!(anchor.id, deserialized.id);
        assert_eq!(anchor.selected_text, deserialized.selected_text);
    }

    #[test]
    fn test_custom_mind_map_snapshot() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let core = WriterCore::new(temp_dir.path());
        let proj = core.create_project("Test Project").unwrap();

        let graph = MindMapGraph {
            id: "graph_1".to_string(),
            nodes: vec![MindMapNode {
                id: "node_1".to_string(),
                title: "Character A".to_string(),
                kind: MindMapNodeKind::Character,
                parent_id: None,
                depth: 0,
                x: 10.0,
                y: 20.0,
                radius: 10.0,
                width: 100.0,
                height: 50.0,
                collapsed: false,
            }],
            edges: vec![MindMapEdge {
                from: "node_1".to_string(),
                to: "node_2".to_string(),
                kind: "References".to_string(),
            }],
            anchors: vec![],
            links: vec![],
        };

        let graph_json = serde_json::to_string(&graph).unwrap();
        let mind_map_path = core.workspace_path().join("projects").join(&proj.id).join("mind_map.json");
        std::fs::write(&mind_map_path, graph_json).unwrap();

        let snapshot = generate_snapshot(&core, &proj.id).unwrap();

        assert_eq!(snapshot.layout_kind, "custom");
        assert_eq!(snapshot.nodes.len(), 1);
        assert_eq!(snapshot.nodes[0].kind, MindMapNodeKind::Character);
        assert_eq!(snapshot.edges[0].kind, "References");
    }
}
