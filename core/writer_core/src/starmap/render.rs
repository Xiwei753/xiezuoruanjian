//! # 星图边渲染计算（Core 层，跨端共享）
//!
//! 根据节点位置和边参数计算箭头、偏移、标签位置等渲染几何。
//! 双向边自动偏移以避免重叠。所有坐标为星图文档坐标。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::starmap::hittest::point_to_segment_distance;
use crate::starmap::types::reference::StarMapTargetPath;
use crate::starmap::types::{StarMapGraph, StarMapLayout};

const DEFAULT_BIDIRECTIONAL_OFFSET: f32 = 12.0;
const DEFAULT_ARROW_PADDING: f32 = 42.0;
const DEFAULT_ARROW_LENGTH: f32 = 10.0;
const DEFAULT_HIT_THRESHOLD: f32 = 10.0;

/// 边端点锚点解析失败的诊断信息。
///
/// `compute_edge_renders` 不再静默丢掉无法定位的端点，而是返回诊断，
/// 让平台端能向用户提示"这条边的 from/to 路径无法在当前画布上定位"。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EdgeAnchorDiagnostic {
    pub edge_id: String,
    pub endpoint: String, // "from" | "to"
    pub reason: EdgeAnchorDiagnosticReason,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum EdgeAnchorDiagnosticReason {
    /// 本地节点/锚点引用，但节点不存在于 graph 或 layout 中
    LocalNodeMissing,
    /// 跨层路径（起点星图不是当前画布的星图），当前画布无法直接定位
    CrossLayerPath,
    /// 路径终点不是 Node/Anchor（例如 Starmap/ChapterRange），没有几何锚点
    NonGeometricTarget,
    /// 第一段 EnterEmbed，但 embed 不存在于 graph 中
    EmbedMissing,
    /// 第一段 EnterPortal，但 portal 节点不存在于 graph 或 layout 中，或节点没有 portal
    PortalMissing,
}

/// 边渲染批结果：成功渲染的边 + 无法定位端点的诊断。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeRenderBatch {
    pub renders: Vec<EdgeRender>,
    pub diagnostics: Vec<EdgeAnchorDiagnostic>,
}

/// 边渲染几何数据 — 平台端据此绘制箭头和标签。
///
/// 所有坐标为星图文档坐标（像素）。
/// `from_cx/cy`、`to_cx/cy`：节点中心点。
/// `start_x/y`、`end_x/y`：边线段起止点（含双向偏移和箭头内缩）。
/// `arrow_tip/left/right`：箭头三角形的三个顶点。
/// `label_x/y`：标签定位点（边中点）。
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
    pub label: Option<String>,
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
    pub label: Option<String>,
}

/// 解析边端点路径在当前画布上的可见锚点坐标。
///
/// 这是边渲染的**唯一锚点解析入口**。平台端不应自己从 DTO 猜 node_id，
/// 必须通过此函数把 `StarMapTargetPath` 解析为画布坐标。
///
/// ## 解析规则
///
/// - **本地 Node/Anchor**（`path.starmap_id == graph.starmap_id && segments.is_empty()`）
///   -> 对应 node layout 中心。Anchor 附在节点上，几何位置就是节点中心。
/// - **深路径**（`path.starmap_id == graph.starmap_id && !segments.is_empty()`）
///   -> 根据第一段定位当前画布上的可见锚点：
///   - **EnterEmbed** -> embed placement 中心。
///   - **EnterPortal** -> portal 所在 node 的 layout 中心（且确认 node 有 portal）。
///     后续更深层的穿越不在当前画布上可见，但第一段的锚点可见。
/// - **跨层路径**（`path.starmap_id != graph.starmap_id`）
///   -> `CrossLayerPath`，当前画布无法直接定位。
/// - **非几何 target**（Starmap/ChapterRange/Entity/External）-> `NonGeometricTarget`。
///
/// 返回 `Err(diagnostic)` 表示无法定位，调用方应收集诊断而非静默丢掉。
pub fn resolve_edge_endpoint_anchor(
    path: &StarMapTargetPath,
    graph: &StarMapGraph,
    layout: &StarMapLayout,
    endpoint: &str,
    edge_id: &str,
) -> Result<(f32, f32), EdgeAnchorDiagnostic> {
    use crate::starmap::semantic::StarMapTargetDetail;
    use crate::starmap::types::reference::StarMapPathSegment;

    let is_local = path.starmap_id == graph.starmap_id && path.segments.is_empty();

    if is_local {
        return match &path.target {
            StarMapTargetDetail::Node { node_id } | StarMapTargetDetail::Anchor { node_id, .. } => {
                node_layout_center(node_id, layout).ok_or(EdgeAnchorDiagnostic {
                    edge_id: edge_id.to_string(),
                    endpoint: endpoint.to_string(),
                    reason: EdgeAnchorDiagnosticReason::LocalNodeMissing,
                })
            }
            _ => Err(EdgeAnchorDiagnostic {
                edge_id: edge_id.to_string(),
                endpoint: endpoint.to_string(),
                reason: EdgeAnchorDiagnosticReason::NonGeometricTarget,
            }),
        };
    }

    // 起点星图不是当前画布的星图 —— 纯跨图引用，无法在当前画布定位。
    if path.starmap_id != graph.starmap_id {
        return Err(EdgeAnchorDiagnostic {
            edge_id: edge_id.to_string(),
            endpoint: endpoint.to_string(),
            reason: EdgeAnchorDiagnosticReason::CrossLayerPath,
        });
    }

    // 深路径：起点是当前 graph 且 segments 非空。
    // 根据第一段定位当前画布上的可见锚点。后续更深层穿越不在当前画布可见，
    // 但第一段的锚点可见，足以绘制边的端点。
    match &path.segments[0] {
        StarMapPathSegment::EnterEmbed { instance_id } => {
            // embed placement 的可见锚点 = placement 中心
            let embed = graph.embeds.iter().find(|e| e.instance_id == *instance_id);
            match embed {
                Some(em) => {
                    let p = &em.placement;
                    Ok((p.x + p.width / 2.0, p.y + p.height / 2.0))
                }
                None => Err(EdgeAnchorDiagnostic {
                    edge_id: edge_id.to_string(),
                    endpoint: endpoint.to_string(),
                    reason: EdgeAnchorDiagnosticReason::EmbedMissing,
                }),
            }
        }
        StarMapPathSegment::EnterPortal { node_id } => {
            // portal 所在 node 的 layout 锚点；确认 node 确实有 portal
            let node = graph.nodes.iter().find(|n| n.id == *node_id);
            if node.and_then(|n| n.portal.as_ref()).is_none() {
                return Err(EdgeAnchorDiagnostic {
                    edge_id: edge_id.to_string(),
                    endpoint: endpoint.to_string(),
                    reason: EdgeAnchorDiagnosticReason::PortalMissing,
                });
            }
            node_layout_center(node_id, layout).ok_or(EdgeAnchorDiagnostic {
                edge_id: edge_id.to_string(),
                endpoint: endpoint.to_string(),
                reason: EdgeAnchorDiagnosticReason::PortalMissing,
            })
        }
    }
}

/// 查节点 layout 中心坐标。节点不存在于 layout 时返回 None。
fn node_layout_center(node_id: &str, layout: &StarMapLayout) -> Option<(f32, f32)> {
    layout
        .nodes
        .iter()
        .find(|n| n.node_id == node_id)
        .map(|n| (n.x + n.width / 2.0, n.y + n.height / 2.0))
}

/// 已解析端点的边，用于渲染计算。
struct ResolvedEdge {
    id: String,
    from: (f32, f32),
    to: (f32, f32),
    label: Option<String>,
}

/// 基于路径锚点解析的边渲染批计算。
///
/// 对每条边，用 [`resolve_edge_endpoint_anchor`] 解析 from/to 端点。
/// 任一端点解析失败则记录诊断并跳过该边（不静默丢掉）。
/// 成功解析的边按原有几何算法渲染。
pub fn compute_edge_renders_from_paths(
    edges: &[crate::starmap::types::StarMapEdge],
    graph: &StarMapGraph,
    layout: &StarMapLayout,
    params: &EdgeRenderParams,
) -> EdgeRenderBatch {
    let mut renders = Vec::new();
    let mut diagnostics = Vec::new();

    let mut resolved_edges: Vec<ResolvedEdge> = Vec::new();

    for edge in edges {
        let from = match resolve_edge_endpoint_anchor(&edge.from, graph, layout, "from", &edge.id) {
            Ok(p) => p,
            Err(d) => {
                diagnostics.push(d);
                continue;
            }
        };
        let to = match resolve_edge_endpoint_anchor(&edge.to, graph, layout, "to", &edge.id) {
            Ok(p) => p,
            Err(d) => {
                diagnostics.push(d);
                continue;
            }
        };
        resolved_edges.push(ResolvedEdge {
            id: edge.id.clone(),
            from,
            to,
            label: edge.label.clone(),
        });
    }

    compute_renders_from_resolved(&resolved_edges, params, &mut renders, &mut diagnostics);

    EdgeRenderBatch {
        renders,
        diagnostics,
    }
}

/// 从已解析端点的边列表计算渲染几何。
fn compute_renders_from_resolved(
    resolved_edges: &[ResolvedEdge],
    params: &EdgeRenderParams,
    renders: &mut Vec<EdgeRender>,
    diagnostics: &mut Vec<EdgeAnchorDiagnostic>,
) {
    // 双向边检测：基于坐标对匹配（而非 node_id），因为路径锚点解析后只有坐标。
    let bidirectional_set: std::collections::HashSet<(usize, usize)> = resolved_edges
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            resolved_edges.iter().enumerate().find_map(|(j, o)| {
                if i != j && coords_eq(e.from, o.to) && coords_eq(e.to, o.from) {
                    Some((i, j))
                } else {
                    None
                }
            })
        })
        .collect();

    for (i, edge) in resolved_edges.iter().enumerate() {
        let (fx, fy) = edge.from;
        let (tx, ty) = edge.to;
        let dx = tx - fx;
        let dy = ty - fy;
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-6 {
            diagnostics.push(EdgeAnchorDiagnostic {
                edge_id: edge.id.clone(),
                endpoint: "from".to_string(),
                reason: EdgeAnchorDiagnosticReason::LocalNodeMissing,
            });
            continue;
        }

        let has_bi = bidirectional_set.iter().any(|(a, b)| *a == i || *b == i);
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

        renders.push(EdgeRender {
            edge_id: edge.id.clone(),
            from_cx: fx,
            from_cy: fy,
            to_cx: tx,
            to_cy: ty,
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
            label: edge.label.clone(),
            has_bidirectional: has_bi,
        });
    }
}

fn coords_eq(a: (f32, f32), b: (f32, f32)) -> bool {
    (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3
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
                label: edge.label.clone(),
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
            label: None,
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
                label: None,
            },
            EdgeInput {
                id: "e2".into(),
                from: "b".into(),
                to: "a".into(),
                label: None,
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
            label: None,
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
            label: None,
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
            label: None,
        }];
        let cs = centers((0.0, 0.0), (200.0, 0.0));
        let renders = compute_edge_renders(&edges, &cs, &EdgeRenderParams::default());
        let r = &renders[0];
        assert!(r.arrow_left_y > r.arrow_tip_y);
        assert!(r.arrow_right_y < r.arrow_tip_y);
        assert!((r.arrow_left_x - r.arrow_right_x).abs() < 0.1);
    }
}
