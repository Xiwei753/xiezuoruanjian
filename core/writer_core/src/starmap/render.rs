use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::starmap::hittest::point_to_segment_distance;

const DEFAULT_BIDIRECTIONAL_OFFSET: f32 = 12.0;
const DEFAULT_ARROW_PADDING: f32 = 42.0;
const DEFAULT_ARROW_LENGTH: f32 = 10.0;
const DEFAULT_HIT_THRESHOLD: f32 = 10.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeRender {
    pub edge_id: String,
    pub from_cx: f32,
    pub from_cy: f32,
    pub to_cx: f32,
    pub to_cy: f32,
    pub start_x: f32,
    pub start_y: f32,
    pub end_x: f32,
    pub end_y: f32,
    pub offset_x: f32,
    pub offset_y: f32,
    pub arrow_tip_x: f32,
    pub arrow_tip_y: f32,
    pub arrow_left_x: f32,
    pub arrow_left_y: f32,
    pub arrow_right_x: f32,
    pub arrow_right_y: f32,
    pub label_x: f32,
    pub label_y: f32,
    pub has_bidirectional: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeRenderParams {
    #[serde(default = "default_bidirectional_offset")]
    pub bidirectional_offset: f32,
    #[serde(default = "default_arrow_padding")]
    pub arrow_padding: f32,
    #[serde(default = "default_arrow_length")]
    pub arrow_length: f32,
}

fn default_bidirectional_offset() -> f32 {
    DEFAULT_BIDIRECTIONAL_OFFSET
}
fn default_arrow_padding() -> f32 {
    DEFAULT_ARROW_PADDING
}
fn default_arrow_length() -> f32 {
    DEFAULT_ARROW_LENGTH
}

impl Default for EdgeRenderParams {
    fn default() -> Self {
        Self {
            bidirectional_offset: DEFAULT_BIDIRECTIONAL_OFFSET,
            arrow_padding: DEFAULT_ARROW_PADDING,
            arrow_length: DEFAULT_ARROW_LENGTH,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeInput {
    pub id: String,
    pub from: String,
    pub to: String,
}

pub fn compute_edge_renders(
    edges: &[EdgeInput],
    node_centers: &HashMap<String, (f32, f32)>,
    params: &EdgeRenderParams,
) -> Vec<EdgeRender> {
    let bidirectional_set: std::collections::HashSet<(String, String)> = edges
        .iter()
        .filter(|e| edges.iter().any(|o| o.from == e.to && o.to == e.from))
        .map(|e| (e.from.clone(), e.to.clone()))
        .collect();

    edges
        .iter()
        .filter_map(|edge| {
            let (fx, fy) = node_centers.get(edge.from.as_str())?;
            let (tx, ty) = node_centers.get(edge.to.as_str())?;

            let dx = tx - fx;
            let dy = ty - fy;
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-6 {
                return None;
            }

            let has_bi = bidirectional_set.contains(&(edge.from.clone(), edge.to.clone()));
            let offset = if has_bi {
                params.bidirectional_offset
            } else {
                0.0
            };
            let ox = if has_bi { -dy / len * offset } else { 0.0 };
            let oy = if has_bi { dx / len * offset } else { 0.0 };

            let dir_x = dx / len;
            let dir_y = dy / len;

            let sx = fx + ox + dir_x * params.arrow_padding;
            let sy = fy + oy + dir_y * params.arrow_padding;
            let ex = tx + ox - dir_x * params.arrow_padding;
            let ey = ty + oy - dir_y * params.arrow_padding;

            let angle = dy.atan2(dx);
            let half_spread = std::f32::consts::PI / 6.0;
            let al = params.arrow_length;

            Some(EdgeRender {
                edge_id: edge.id.clone(),
                from_cx: *fx,
                from_cy: *fy,
                to_cx: *tx,
                to_cy: *ty,
                start_x: sx,
                start_y: sy,
                end_x: ex,
                end_y: ey,
                offset_x: ox,
                offset_y: oy,
                arrow_tip_x: ex,
                arrow_tip_y: ey,
                arrow_left_x: ex - al * (angle - half_spread).cos(),
                arrow_left_y: ey - al * (angle - half_spread).sin(),
                arrow_right_x: ex - al * (angle + half_spread).cos(),
                arrow_right_y: ey - al * (angle + half_spread).sin(),
                label_x: (sx + ex) / 2.0,
                label_y: (sy + ey) / 2.0,
                has_bidirectional: has_bi,
            })
        })
        .collect()
}

pub fn hit_test_edge_renders(x: f32, y: f32, renders: &[EdgeRender]) -> Option<String> {
    hit_test_edge_renders_with_threshold(x, y, renders, DEFAULT_HIT_THRESHOLD)
}

pub fn hit_test_edge_renders_with_threshold(
    x: f32,
    y: f32,
    renders: &[EdgeRender],
    threshold: f32,
) -> Option<String> {
    let mut closest_dist = f32::MAX;
    let mut closest_id = None;

    for r in renders {
        let dist = point_to_segment_distance(x, y, r.start_x, r.start_y, r.end_x, r.end_y);
        if dist < threshold && dist < closest_dist {
            closest_dist = dist;
            closest_id = Some(r.edge_id.clone());
        }
    }

    closest_id
}

#[cfg(test)]
mod tests {
    use super::*;

    fn centers(a: (f32, f32), b: (f32, f32)) -> HashMap<String, (f32, f32)> {
        let mut m = HashMap::new();
        m.insert("a".to_string(), a);
        m.insert("b".to_string(), b);
        m
    }

    #[test]
    fn test_single_edge_render() {
        let edges = vec![EdgeInput {
            id: "e1".into(),
            from: "a".into(),
            to: "b".into(),
        }];
        let cs = centers((0.0, 0.0), (200.0, 0.0));
        let renders = compute_edge_renders(&edges, &cs, &EdgeRenderParams::default());
        assert_eq!(renders.len(), 1);
        let r = &renders[0];
        assert!((r.start_x - 42.0).abs() < 0.1);
        assert!((r.end_x - 158.0).abs() < 0.1);
        assert!((r.arrow_tip_x - 158.0).abs() < 0.1);
        assert!(!r.has_bidirectional);
    }

    #[test]
    fn test_bidirectional_edge_render() {
        let edges = vec![
            EdgeInput {
                id: "e1".into(),
                from: "a".into(),
                to: "b".into(),
            },
            EdgeInput {
                id: "e2".into(),
                from: "b".into(),
                to: "a".into(),
            },
        ];
        let cs = centers((0.0, 0.0), (200.0, 0.0));
        let renders = compute_edge_renders(&edges, &cs, &EdgeRenderParams::default());
        assert_eq!(renders.len(), 2);
        let r1 = &renders[0];
        let r2 = &renders[1];
        assert!(r1.has_bidirectional);
        assert!(r2.has_bidirectional);
        assert!(r1.offset_y.abs() > 0.01);
        assert!(r2.offset_y.abs() > 0.01);
    }

    #[test]
    fn test_hit_test_edge_render_near() {
        let edges = vec![EdgeInput {
            id: "e1".into(),
            from: "a".into(),
            to: "b".into(),
        }];
        let cs = centers((0.0, 0.0), (200.0, 0.0));
        let renders = compute_edge_renders(&edges, &cs, &EdgeRenderParams::default());
        let hit = hit_test_edge_renders(100.0, 5.0, &renders);
        assert!(hit.is_some());
        assert_eq!(hit.unwrap(), "e1");
    }

    #[test]
    fn test_hit_test_edge_render_far() {
        let edges = vec![EdgeInput {
            id: "e1".into(),
            from: "a".into(),
            to: "b".into(),
        }];
        let cs = centers((0.0, 0.0), (200.0, 0.0));
        let renders = compute_edge_renders(&edges, &cs, &EdgeRenderParams::default());
        let hit = hit_test_edge_renders(100.0, 20.0, &renders);
        assert!(hit.is_none());
    }

    #[test]
    fn test_arrow_geometry_points_toward_target() {
        let edges = vec![EdgeInput {
            id: "e1".into(),
            from: "a".into(),
            to: "b".into(),
        }];
        let cs = centers((0.0, 0.0), (200.0, 0.0));
        let renders = compute_edge_renders(&edges, &cs, &EdgeRenderParams::default());
        let r = &renders[0];
        assert!(r.arrow_left_y > r.arrow_tip_y);
        assert!(r.arrow_right_y < r.arrow_tip_y);
        assert!((r.arrow_left_x - r.arrow_right_x).abs() < 0.1);
    }
}
