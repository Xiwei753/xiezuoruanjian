use crate::starmap::types::{StarMapLayout, StarMapLayoutKind, StarMapLayoutNode};

const DEFAULT_NODE_WIDTH: f32 = 150.0;
const DEFAULT_NODE_HEIGHT: f32 = 60.0;
const DEFAULT_RADIUS: f32 = 30.0;
const GRID_COL_SPACING: f32 = 200.0;
const GRID_ROW_SPACING: f32 = 100.0;
const GRID_MAX_WIDTH: f32 = 800.0;
const RADIAL_CENTER_X: f32 = 400.0;
const RADIAL_CENTER_Y: f32 = 300.0;
const RADIAL_RING_SPACING: f32 = 200.0;

pub fn calculate_grid_layout(
    node_ids: &[String],
    existing: &StarMapLayout,
) -> StarMapLayout {
    let existing_map: std::collections::HashMap<&str, &StarMapLayoutNode> =
        existing.nodes.iter().map(|n| (n.node_id.as_str(), n)).collect();

    let mut nodes = Vec::new();
    let mut x: f32 = 100.0;
    let mut y: f32 = 100.0;

    for id in node_ids {
        if let Some(en) = existing_map.get(id.as_str()) {
            nodes.push((*en).clone());
        } else {
            nodes.push(StarMapLayoutNode {
                node_id: id.clone(),
                x,
                y,
                width: DEFAULT_NODE_WIDTH,
                height: DEFAULT_NODE_HEIGHT,
                radius: DEFAULT_RADIUS,
                collapsed: false,
                z_index: 0,
                scale: 1.0,
                depth: 0.0,
                focus_weight: 0.0,
                orbit_group: None,
            });
            x += GRID_COL_SPACING;
            if x > GRID_MAX_WIDTH {
                x = 100.0;
                y += GRID_ROW_SPACING;
            }
        }
    }

    StarMapLayout {
        kind: StarMapLayoutKind::Freeform,
        nodes,
    }
}

pub fn calculate_radial_layout(
    node_ids: &[String],
    parent_map: &std::collections::HashMap<String, Option<String>>,
    existing: &StarMapLayout,
) -> StarMapLayout {
    let existing_map: std::collections::HashMap<&str, &StarMapLayoutNode> =
        existing.nodes.iter().map(|n| (n.node_id.as_str(), n)).collect();

    if node_ids.is_empty() {
        return StarMapLayout {
            kind: StarMapLayoutKind::AutoRadial,
            nodes: Vec::new(),
        };
    }

    let children_map: std::collections::HashMap<String, Vec<String>> = {
        let mut m: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
        for id in node_ids {
            let parent = parent_map.get(id).and_then(|p| p.as_deref()).unwrap_or("");
            m.entry(parent.to_string()).or_default().push(id.clone());
        }
        m
    };

    let roots: Vec<String> = node_ids
        .iter()
        .filter(|id| {
            parent_map
                .get(*id)
                .and_then(|p| p.as_deref())
                .map(|p| p.is_empty() || !node_ids.iter().any(|nid| nid == p))
                .unwrap_or(true)
        })
        .cloned()
        .collect();

    let mut positions: std::collections::HashMap<String, (f32, f32)> = std::collections::HashMap::new();
    let mut depths: std::collections::HashMap<String, f32> = std::collections::HashMap::new();

    for (i, root_id) in roots.iter().enumerate() {
        let angle = (i as f32) * 2.0 * std::f32::consts::PI / (roots.len().max(1) as f32);
        let x = RADIAL_CENTER_X + RADIAL_RING_SPACING * angle.cos();
        let y = RADIAL_CENTER_Y + RADIAL_RING_SPACING * angle.sin();
        positions.insert(root_id.clone(), (x, y));
        depths.insert(root_id.clone(), 1.0);
    }

    let mut queue: std::collections::VecDeque<(String, f32, f32, f32)> = std::collections::VecDeque::new();
    for root_id in &roots {
        if let Some((x, y)) = positions.get(root_id) {
            queue.push_back((root_id.clone(), *x, *y, 1.0));
        }
    }

    while let Some((parent_id, px, py, depth)) = queue.pop_front() {
        let children = children_map.get(&parent_id).cloned().unwrap_or_default();
        if children.is_empty() {
            continue;
        }
        let ring_radius = RADIAL_RING_SPACING / (depth + 1.0).max(1.5);
        for (i, child_id) in children.iter().enumerate() {
            if positions.contains_key(child_id) {
                continue;
            }
            let base_angle = (i as f32) * 2.0 * std::f32::consts::PI / (children.len().max(1) as f32);
            let angle = base_angle + depth * 0.3;
            let x = px + ring_radius * angle.cos();
            let y = py + ring_radius * angle.sin();
            positions.insert(child_id.clone(), (x, y));
            depths.insert(child_id.clone(), depth + 1.0);
            queue.push_back((child_id.clone(), x, y, depth + 1.0));
        }
    }

    let mut nodes = Vec::new();
    for id in node_ids {
        let (x, y) = positions.get(id).copied().unwrap_or((RADIAL_CENTER_X, RADIAL_CENTER_Y));
        let depth = depths.get(id).copied().unwrap_or(0.0);
        if let Some(en) = existing_map.get(id.as_str()) {
            let mut node = (*en).clone();
            node.depth = depth;
            nodes.push(node);
        } else {
            nodes.push(StarMapLayoutNode {
                node_id: id.clone(),
                x,
                y,
                width: DEFAULT_NODE_WIDTH,
                height: DEFAULT_NODE_HEIGHT,
                radius: DEFAULT_RADIUS,
                collapsed: false,
                z_index: 0,
                scale: 1.0,
                depth,
                focus_weight: 0.0,
                orbit_group: None,
            });
        }
    }

    StarMapLayout {
        kind: StarMapLayoutKind::AutoRadial,
        nodes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grid_layout_basic() {
        let ids = vec!["a".into(), "b".into(), "c".into()];
        let layout = calculate_grid_layout(&ids, &StarMapLayout { kind: StarMapLayoutKind::Freeform, nodes: vec![] });
        assert_eq!(layout.nodes.len(), 3);
        assert_eq!(layout.nodes[0].x, 100.0);
        assert_eq!(layout.nodes[1].x, 300.0);
        assert_eq!(layout.nodes[2].x, 500.0);
    }

    #[test]
    fn test_grid_layout_preserves_existing() {
        let existing = StarMapLayout {
            kind: StarMapLayoutKind::Freeform,
            nodes: vec![StarMapLayoutNode {
                node_id: "a".into(),
                x: 50.0,
                y: 50.0,
                width: 200.0,
                height: 80.0,
                radius: 40.0,
                collapsed: false,
                z_index: 5,
                scale: 1.0,
                depth: 0.0,
                focus_weight: 0.0,
                orbit_group: None,
            }],
        };
        let ids = vec!["a".into(), "b".into()];
        let layout = calculate_grid_layout(&ids, &existing);
        assert_eq!(layout.nodes[0].x, 50.0);
        assert_eq!(layout.nodes[0].width, 200.0);
        assert_eq!(layout.nodes[1].x, 100.0);
    }

    #[test]
    fn test_radial_layout_basic() {
        let ids = vec!["root".into(), "child1".into(), "child2".into()];
        let mut parent_map = std::collections::HashMap::new();
        parent_map.insert("root".to_string(), None);
        parent_map.insert("child1".to_string(), Some("root".to_string()));
        parent_map.insert("child2".to_string(), Some("root".to_string()));
        let layout = calculate_radial_layout(&ids, &parent_map, &StarMapLayout { kind: StarMapLayoutKind::AutoRadial, nodes: vec![] });
        assert_eq!(layout.nodes.len(), 3);
    }

    #[test]
    fn test_radial_layout_empty_nodes() {
        let ids: Vec<String> = vec![];
        let parent_map = std::collections::HashMap::new();
        let layout = calculate_radial_layout(&ids, &parent_map, &StarMapLayout { kind: StarMapLayoutKind::AutoRadial, nodes: vec![] });

        assert_eq!(layout.kind, StarMapLayoutKind::AutoRadial);
        assert!(layout.nodes.is_empty());
    }
}
