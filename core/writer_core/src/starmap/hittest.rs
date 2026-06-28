use crate::starmap::types::StarMapLayoutNode;

const EDGE_HIT_THRESHOLD: f32 = 10.0;

#[derive(Debug, Clone)]
pub struct HitResult {
    pub kind: HitKind,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HitKind {
    Node,
    Edge,
}

pub fn hit_test_nodes(x: f32, y: f32, nodes: &[StarMapLayoutNode]) -> Option<HitResult> {
    let mut best: Option<(i32, &StarMapLayoutNode)> = None;
    for node in nodes {
        if x >= node.x && x <= node.x + node.width && y >= node.y && y <= node.y + node.height {
            match &best {
                Some((best_z, _)) if *best_z >= node.z_index => {}
                _ => best = Some((node.z_index, node)),
            }
        }
    }
    best.map(|(_, node)| HitResult {
        kind: HitKind::Node,
        id: node.node_id.clone(),
    })
}

pub fn hit_test_edges(
    x: f32,
    y: f32,
    edges: &[(String, String)],
    node_positions: &std::collections::HashMap<String, (f32, f32)>,
) -> Option<HitResult> {
    let mut closest_dist = f32::MAX;
    let mut closest_id = None;

    for (from_id, to_id) in edges {
        let (fx, fy) = match node_positions.get(from_id.as_str()) {
            Some(p) => *p,
            None => continue,
        };
        let (tx, ty) = match node_positions.get(to_id.as_str()) {
            Some(p) => *p,
            None => continue,
        };

        let dist = point_to_segment_distance(x, y, fx, fy, tx, ty);
        if dist < EDGE_HIT_THRESHOLD && dist < closest_dist {
            closest_dist = dist;
            closest_id = Some(format!("{}->{}", from_id, to_id));
        }
    }

    closest_id.map(|id| HitResult {
        kind: HitKind::Edge,
        id,
    })
}

pub fn point_to_segment_distance(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let dx = bx - ax;
    let dy = by - ay;
    let len_sq = dx * dx + dy * dy;
    if len_sq < 1e-10 {
        return ((px - ax).powi(2) + (py - ay).powi(2)).sqrt();
    }
    let t = (((px - ax) * dx + (py - ay) * dy) / len_sq).clamp(0.0, 1.0);
    let proj_x = ax + t * dx;
    let proj_y = ay + t * dy;
    ((px - proj_x).powi(2) + (py - proj_y).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::starmap::types::StarMapLayoutNode;

    fn make_node(id: &str, x: f32, y: f32) -> StarMapLayoutNode {
        StarMapLayoutNode {
            node_id: id.into(),
            x,
            y,
            width: 150.0,
            height: 60.0,
            radius: 30.0,
            collapsed: false,
            z_index: 0,
            scale: 1.0,
            depth: 0.0,
            focus_weight: 0.0,
            orbit_group: None,
        }
    }

    #[test]
    fn test_hit_node_inside() {
        let nodes = vec![make_node("a", 100.0, 100.0)];
        let r = hit_test_nodes(150.0, 130.0, &nodes).unwrap();
        assert_eq!(r.id, "a");
    }

    #[test]
    fn test_hit_node_outside() {
        let nodes = vec![make_node("a", 100.0, 100.0)];
        assert!(hit_test_nodes(0.0, 0.0, &nodes).is_none());
    }

    #[test]
    fn test_hit_node_z_order() {
        let mut n1 = make_node("a", 100.0, 100.0);
        n1.z_index = 1;
        let mut n2 = make_node("b", 100.0, 100.0);
        n2.z_index = 5;
        let nodes = vec![n1, n2];
        let r = hit_test_nodes(150.0, 130.0, &nodes).unwrap();
        assert_eq!(r.id, "b");
    }

    #[test]
    fn test_hit_edge_near() {
        let edges = vec![("a".to_string(), "b".to_string())];
        let mut positions = std::collections::HashMap::new();
        positions.insert("a".to_string(), (0.0f32, 0.0f32));
        positions.insert("b".to_string(), (100.0f32, 0.0f32));

        // Debug: verify positions are accessible
        assert!(positions.get("a").is_some(), "positions.get('a') failed");
        assert!(positions.get("b").is_some(), "positions.get('b') failed");

        let d = point_to_segment_distance(50.0, 5.0, 0.0, 0.0, 100.0, 0.0);
        assert!(d < 10.0, "distance {} should be < 10", d);

        let r = hit_test_edges(50.0, 5.0, &edges, &positions);
        assert!(
            r.is_some(),
            "expected edge hit at (50,5) near segment (0,0)-(100,0), got None"
        );
        assert_eq!(r.unwrap().kind, HitKind::Edge);
    }

    #[test]
    fn test_hit_edge_far() {
        let edges = vec![("a".to_string(), "b".to_string())];
        let mut positions = std::collections::HashMap::new();
        positions.insert("a".to_string(), (0.0, 0.0));
        positions.insert("b".to_string(), (100.0, 0.0));

        assert!(hit_test_edges(50.0, 20.0, &edges, &positions).is_none());
    }

    #[test]
    fn test_point_to_segment_distance() {
        // Point (50,5) near segment (0,0)-(100,0): distance should be 5
        let d = point_to_segment_distance(50.0, 5.0, 0.0, 0.0, 100.0, 0.0);
        assert!((d - 5.0).abs() < 0.01, "expected ~5.0, got {}", d);

        // Point at endpoint
        let d = point_to_segment_distance(0.0, 0.0, 0.0, 0.0, 100.0, 0.0);
        assert!((d - 0.0).abs() < 0.01, "expected 0, got {}", d);

        // Point beyond endpoint
        let d = point_to_segment_distance(200.0, 0.0, 0.0, 0.0, 100.0, 0.0);
        assert!((d - 100.0).abs() < 0.01, "expected 100, got {}", d);
    }
}
