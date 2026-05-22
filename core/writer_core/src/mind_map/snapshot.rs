use serde::{Deserialize, Serialize};

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

    for g_node in &graph.nodes {
        let l_node = layout.nodes.iter().find(|ln| ln.node_id == g_node.id);

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

        let links_for_node: Vec<_> = graph.links.iter().filter(|l| l.node_id == g_node.id).collect();
        let mut broken_link = false;
        let mut anchor_count = 0;

        for link in links_for_node {
            anchor_count += 1;
            // Simplified check: If anchor doesn't exist in graph.anchors, it's broken
            // A more rigorous check would require full content parsing, but for snapshot generation, we use current state
            if !graph.anchors.iter().any(|a| a.id == link.anchor_id) {
                broken_link = true;
            }
        }

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
