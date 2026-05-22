use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

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
    pub nodes: Vec<MindMapSnapshotNode>,
    pub edges: Vec<MindMapSnapshotEdge>,
    pub bounds: MindMapBounds,
    pub generated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MindMapSnapshotNode {
    pub id: String,
    pub title: String,
    pub kind: crate::mind_map::graph::MindMapNodeKind,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub radius: f32,
    pub collapsed: bool,
    pub anchor_count: usize,
    pub broken_link: bool,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MindMapSnapshotEdge {
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: String, // Stringified for frontend simplicity, or keep typed if preferred
    pub label: Option<String>,
}

pub fn generate_snapshot(
    graph: &crate::mind_map::graph::MindMapGraph,
    layout: &crate::mind_map::layout::MindMapLayout,
) -> MindMapSnapshot {
    let mut snapshot_nodes = Vec::new();
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;

    // 1. Build layout_by_node_id HashMap
    let layout_by_node_id: HashMap<&str, &crate::mind_map::layout::MindMapLayoutNode> = layout
        .nodes
        .iter()
        .map(|ln| (ln.node_id.as_str(), ln))
        .collect();

    // 2. Build anchor_ids set
    let anchor_ids: HashSet<&str> = graph
        .anchors
        .iter()
        .map(|a| a.id.as_str())
        .collect();

    // 3. Build link_count_by_node_id and broken_link_by_node_id
    let mut link_count_by_node_id = HashMap::new();
    let mut broken_link_by_node_id = HashMap::new();

    for link in &graph.links {
        *link_count_by_node_id.entry(link.node_id.as_str()).or_insert(0) += 1;
        if !anchor_ids.contains(link.anchor_id.as_str()) {
            broken_link_by_node_id.insert(link.node_id.as_str(), true);
        }
    }

    for g_node in &graph.nodes {
        let l_node = layout_by_node_id.get(g_node.id.as_str());

        // Defaults if layout is missing for some reason
        let (x, y, w, h, r, collapsed) = if let Some(ln) = l_node {
            (ln.x, ln.y, ln.width, ln.height, ln.radius, ln.collapsed)
        } else {
            (0.0, 0.0, 100.0, 50.0, 25.0, false)
        };

        if x - w / 2.0 < min_x { min_x = x - w / 2.0; }
        if y - h / 2.0 < min_y { min_y = y - h / 2.0; }
        if x + w / 2.0 > max_x { max_x = x + w / 2.0; }
        if y + h / 2.0 > max_y { max_y = y + h / 2.0; }

        let anchor_count = link_count_by_node_id.get(g_node.id.as_str()).copied().unwrap_or(0);
        let broken_link = broken_link_by_node_id.get(g_node.id.as_str()).copied().unwrap_or(false);

        snapshot_nodes.push(MindMapSnapshotNode {
            id: g_node.id.clone(),
            title: g_node.title.clone(),
            kind: g_node.kind.clone(),
            x,
            y,
            width: w,
            height: h,
            radius: r,
            collapsed,
            anchor_count,
            broken_link,
            tags: g_node.tags.clone(),
        });
    }

    if snapshot_nodes.is_empty() {
        min_x = -200.0;
        min_y = -200.0;
        max_x = 200.0;
        max_y = 200.0;
    }

    let snapshot_edges = graph.edges.iter().map(|e| MindMapSnapshotEdge {
        id: e.id.clone(),
        from: e.from.clone(),
        to: e.to.clone(),
        kind: format!("{:?}", e.kind),
        label: e.label.clone(),
    }).collect();

    MindMapSnapshot {
        project_id: graph.project_id.clone(),
        layout_kind: format!("{:?}", layout.kind),
        nodes: snapshot_nodes,
        edges: snapshot_edges,
        bounds: MindMapBounds { min_x, min_y, max_x, max_y },
        generated_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    }
}
