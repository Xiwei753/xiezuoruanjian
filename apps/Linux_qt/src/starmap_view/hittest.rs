//! # 星图命中测试（Linux 平台端）
//!
//! 提供节点 AABB 和边线段距离的命中测试算法。
//! 坐标空间为星图文档坐标（不含视口滚动偏移），由平台渲染层负责变换。
//!
//! Core 已在「星图 Core 最终收口」中移出显示层职责，本文件随算法一同归到 Linux 平台端。

use super::layout_types::StarMapLayoutNode;

/// 边命中距离阈值（文档坐标像素）。点击位置距边线段小于此值视为命中。
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

/// 节点 AABB 命中测试。
///
/// 遍历所有节点，返回包含点击位置且 z_index 最高的节点。
/// 坐标为星图文档坐标（不含视口滚动偏移）。
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

/// 边线段距离命中测试。
///
/// `node_positions` 为节点中心坐标（文档坐标）。
/// 返回距点击位置最近且距离 < EDGE_HIT_THRESHOLD 的边。
/// 多条边同时命中时取最近的。
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

/// 点到线段的最短距离。
///
/// 将点投影到线段方向，`t` 值 clamp 到 [0, 1] 保证投影不超出线段端点。
/// 退化为零长线段时返回点到端点的距离。
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

fn cross(ux: f32, uy: f32, vx: f32, vy: f32) -> f32 {
    ux * vy - uy * vx
}

/// 点是否在三角形内部（含边界）。
///
/// 用三条边的叉积符号一致性判断。退化三角形（三点共线，面积为 0）不覆盖任何点：
/// 否则一个"三个顶点重合"的退化箭头会把整个平面都算成命中。
pub fn point_in_triangle(
    px: f32,
    py: f32,
    ax: f32,
    ay: f32,
    bx: f32,
    by: f32,
    cx: f32,
    cy: f32,
) -> bool {
    if cross(bx - ax, by - ay, cx - ax, cy - ay).abs() < 1e-6 {
        return false;
    }
    let d1 = cross(px - ax, py - ay, bx - ax, by - ay);
    let d2 = cross(px - bx, py - by, cx - bx, cy - by);
    let d3 = cross(px - cx, py - cy, ax - cx, ay - cy);
    let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_neg && has_pos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::starmap_view::layout_types::StarMapLayoutNode;

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
        assert!(positions.contains_key("a"), "positions.get('a') failed");
        assert!(positions.contains_key("b"), "positions.get('b') failed");

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
