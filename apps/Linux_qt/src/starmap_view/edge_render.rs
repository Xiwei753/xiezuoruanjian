//! # 星图边渲染计算（Linux 平台端）
//!
//! 根据节点位置和边参数计算箭头、偏移、标签位置等渲染几何。
//! 双向边自动偏移以避免重叠。所有坐标为星图文档坐标。
//!
//! Core 已在「星图 Core 最终收口」中移出显示层职责，本文件随算法一同归到 Linux 平台端。

use serde::{Deserialize, Serialize};

use super::hittest::{point_in_triangle, point_to_segment_distance};
use super::layout_types::{StarMapEmbedSceneRect, StarMapLayout};
use writer_core::starmap::types::reference::StarMapTargetPath;
use writer_core::starmap::types::StarMapGraph;

const DEFAULT_BIDIRECTIONAL_OFFSET: f32 = 12.0;
const DEFAULT_ARROW_LENGTH: f32 = 10.0;
/// 边命中的兜底阈值（world 单位，仅供无缩放的 Rust 调用方/测试使用）。
/// UI 路径必须传"屏幕像素 ÷ effectiveScale"算出的阈值，不能吃这个固定值：
/// 相机范围放开到 1e-4~1e5 之后，固定 world 阈值在屏幕上会差好几个数量级。
const DEFAULT_HIT_THRESHOLD: f32 = 10.0;

/// 边端点锚点解析失败的诊断信息。
///
/// `compute_edge_renders_from_paths` 不再静默丢掉无法定位的端点，而是返回诊断，
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
/// `from_cx/cy`、`to_cx/cy`：端点形状中心点。
/// `start_x/y`、`end_x/y`：边线段起止点 = 各自形状（Node 矩形 / Embed 圆周）与
///   连线的真实边界交点，含双向偏移。
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
    #[serde(default = "default_arrow_length")]
    pub arrow_length: f32,
}

fn default_bidirectional_offset() -> f32 {
    DEFAULT_BIDIRECTIONAL_OFFSET
}
fn default_arrow_length() -> f32 {
    DEFAULT_ARROW_LENGTH
}

impl Default for EdgeRenderParams {
    fn default() -> Self {
        Self {
            bidirectional_offset: DEFAULT_BIDIRECTIONAL_OFFSET,
            arrow_length: DEFAULT_ARROW_LENGTH,
        }
    }
}

/// 端点在当前画布上的可见几何：普通 Node 是矩形，Embed（含旧 Portal 归一）
/// 永远是正圆（`width == height`，半径 = 宽 / 2）。
///
/// 边端点的起止点只能由"形状 + 连线方向"求交得到，不能再用固定内缩常量猜边界。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EndpointShape {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub is_embed: bool,
}

impl EndpointShape {
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

/// 解析边端点路径在当前画布上的可见锚点几何。
///
/// 这是边渲染的**唯一锚点解析入口**。平台端不应自己从 DTO 猜 node_id，
/// 必须通过此函数把 `StarMapTargetPath` 解析为画布上的形状（矩形或正圆）。
///
/// ## 解析规则
///
/// - **本地 Node/Anchor**（`path.starmap_id == graph.starmap_id && segments.is_empty()`）
///   -> 对应 node layout 矩形（宽高来自 layout；连线交点取矩形边界）。
/// - **深路径**（`path.starmap_id == graph.starmap_id && !segments.is_empty()`）
///   -> 根据第一段定位当前画布上的可见锚点：
///   - **EnterEmbed** -> 平台 scene geometry 的**圆盒**（`is_embed = true`）。
///     Core 只存 `StarMapEmbedDto.position`（左上角）不含宽高，显示锚点必须用
///     平台传入的 `embed_rects`，连线交点取圆周。
///   - **EnterPortal** -> 旧 portal 在 Linux UI 归一到正圆 Embed
///     （UI instanceId `legacy-portal:<nodeId>`）：有归一显示几何时按圆求交；
///     拿不到才退回 portal 所在 node 的 layout 矩形（且确认 node 有 portal）。
/// - **跨层路径**（`path.starmap_id != graph.starmap_id`）
///   -> `CrossLayerPath`，当前画布无法直接定位。
/// - **非几何 target**（Starmap/ChapterRange/Entity/External）-> `NonGeometricTarget`。
///
/// 返回 `Err(diagnostic)` 表示无法定位，调用方应收集诊断而非静默丢掉。
pub fn resolve_edge_endpoint_anchor(
    path: &StarMapTargetPath,
    graph: &StarMapGraph,
    layout: &StarMapLayout,
    embed_rects: &[StarMapEmbedSceneRect],
    endpoint: &str,
    edge_id: &str,
) -> Result<EndpointShape, EdgeAnchorDiagnostic> {
    use writer_core::starmap::semantic::StarMapTargetDetail;
    use writer_core::starmap::types::reference::StarMapPathSegment;

    let is_local = path.starmap_id == graph.starmap_id && path.segments.is_empty();

    if is_local {
        return match &path.target {
            StarMapTargetDetail::Node { node_id } | StarMapTargetDetail::Anchor { node_id, .. } => {
                // 旧 Portal 节点的可见身份是正圆 Embed（UI 归一到
                // "legacy-portal:<nodeId>"）：有归一显示几何时按圆求交，
                // 不能继续当普通矩形 Node 猜边界。
                let is_portal = graph
                    .nodes
                    .iter()
                    .find(|n| n.id == *node_id)
                    .map(|n| n.portal.is_some())
                    .unwrap_or(false);
                if is_portal {
                    let ui_instance_id = format!("legacy-portal:{}", node_id);
                    if let Some(r) = embed_rects
                        .iter()
                        .find(|r| r.instance_id == ui_instance_id)
                    {
                        return Ok(EndpointShape {
                            x: r.x,
                            y: r.y,
                            width: r.width,
                            height: r.height,
                            is_embed: true,
                        });
                    }
                    return node_layout_rect(node_id, layout, true).ok_or(EdgeAnchorDiagnostic {
                        edge_id: edge_id.to_string(),
                        endpoint: endpoint.to_string(),
                        reason: EdgeAnchorDiagnosticReason::LocalNodeMissing,
                    });
                }
                node_layout_rect(node_id, layout, false).ok_or(EdgeAnchorDiagnostic {
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
            // embed 锚点 = 平台 scene geometry 的圆盒。
            // Core 只存 `StarMapEmbedDto.position`（左上角）不含宽高，不能用来
            // 猜显示锚点。这里从平台传入的 `embed_rects` 按 instance_id 查显示
            // 包围盒，取圆心 + 半径，与 QML 拉线预览一致。
            embed_rects
                .iter()
                .find(|r| r.instance_id == *instance_id)
                .map(|r| EndpointShape {
                    x: r.x,
                    y: r.y,
                    width: r.width,
                    height: r.height,
                    is_embed: true,
                })
                .ok_or(EdgeAnchorDiagnostic {
                    edge_id: edge_id.to_string(),
                    endpoint: endpoint.to_string(),
                    reason: EdgeAnchorDiagnosticReason::EmbedMissing,
                })
        }
        StarMapPathSegment::EnterPortal { node_id } => {
            // portal 所在 node 的 layout 锚点；确认 node 确实有 portal。
            let node = graph.nodes.iter().find(|n| n.id == *node_id);
            if node.and_then(|n| n.portal.as_ref()).is_none() {
                return Err(EdgeAnchorDiagnostic {
                    edge_id: edge_id.to_string(),
                    endpoint: endpoint.to_string(),
                    reason: EdgeAnchorDiagnosticReason::PortalMissing,
                });
            }
            // 旧 Portal 在 Linux UI 已经按正圆 Embed 显示：优先用归一后的圆盒几何，
            // 不能继续当普通矩形 Node 猜边界。
            let ui_instance_id = format!("legacy-portal:{}", node_id);
            if let Some(r) = embed_rects
                .iter()
                .find(|r| r.instance_id == ui_instance_id)
            {
                return Ok(EndpointShape {
                    x: r.x,
                    y: r.y,
                    width: r.width,
                    height: r.height,
                    is_embed: true,
                });
            }
            node_layout_rect(node_id, layout, false).ok_or(EdgeAnchorDiagnostic {
                edge_id: edge_id.to_string(),
                endpoint: endpoint.to_string(),
                reason: EdgeAnchorDiagnosticReason::PortalMissing,
            })
        }
    }
}

/// 查节点 layout 矩形。节点不存在于 layout 时返回 None。
fn node_layout_rect(
    node_id: &str,
    layout: &StarMapLayout,
    is_embed: bool,
) -> Option<EndpointShape> {
    layout
        .nodes
        .iter()
        .find(|n| n.node_id == node_id)
        .map(|n| EndpointShape {
            x: n.x,
            y: n.y,
            width: n.width,
            height: n.height,
            is_embed,
        })
}

// ── 端点边界求交（与 Harmony StarMapGeometry 的 edgeEndpointBoundaryPoint 同一语义）──

/// 从 `p` 沿单位方向 `dir` 前进，与端点形状边界的第一个交点。
///
/// Embed 是正圆（线 × 圆周求交），普通 Node 是矩形（线 × 矩形求交）；
/// 正式边与 QML 拉线预览必须使用同一语义，否则松手瞬间端点会跳。
/// 退化输入（无交点）时退回 `p`。
pub fn endpoint_boundary_point(
    shape: &EndpointShape,
    p: (f32, f32),
    dir: (f32, f32),
) -> (f32, f32) {
    if shape.is_embed {
        line_circle_entry(shape, p, dir)
    } else {
        line_rect_entry(shape, p, dir)
    }
}

/// 线 × 圆周：返回沿 `dir` 前进方向与圆的第一个交点（射线进入点）。
/// 半径取短边一半：Embed 是正方形圆盒（radius = 宽 / 2），非正方形只可能来自
/// 兜底几何，取内切圆避免把椭圆当成圆。
fn line_circle_entry(shape: &EndpointShape, p: (f32, f32), dir: (f32, f32)) -> (f32, f32) {
    let (cx, cy) = shape.center();
    let radius = shape.width.min(shape.height) / 2.0;
    let fx = p.0 - cx;
    let fy = p.1 - cy;
    let b = fx * dir.0 + fy * dir.1;
    let c = fx * fx + fy * fy - radius * radius;
    let disc = b * b - c;
    if disc < 0.0 {
        return p;
    }
    let sq = disc.sqrt();
    let t1 = -b - sq;
    let t2 = -b + sq;
    let t = if t1 >= 0.0 { t1 } else { t2 };
    if t < 0.0 {
        return p;
    }
    (p.0 + t * dir.0, p.1 + t * dir.1)
}

/// 线 × 矩形：slab 裁剪取进入点（`p` 已在矩形内时取离开点）。
fn line_rect_entry(shape: &EndpointShape, p: (f32, f32), dir: (f32, f32)) -> (f32, f32) {
    let x1 = shape.x;
    let x2 = shape.x + shape.width;
    let y1 = shape.y;
    let y2 = shape.y + shape.height;

    let (tx_min, tx_max) = if dir.0.abs() > f32::EPSILON {
        let a = (x1 - p.0) / dir.0;
        let b = (x2 - p.0) / dir.0;
        if a < b {
            (a, b)
        } else {
            (b, a)
        }
    } else if p.0 < x1 || p.0 > x2 {
        return p;
    } else {
        (f32::NEG_INFINITY, f32::INFINITY)
    };
    let (ty_min, ty_max) = if dir.1.abs() > f32::EPSILON {
        let a = (y1 - p.1) / dir.1;
        let b = (y2 - p.1) / dir.1;
        if a < b {
            (a, b)
        } else {
            (b, a)
        }
    } else if p.1 < y1 || p.1 > y2 {
        return p;
    } else {
        (f32::NEG_INFINITY, f32::INFINITY)
    };

    let t_min = tx_min.max(ty_min);
    let t_max = tx_max.min(ty_max);
    if t_max < t_min.max(0.0) {
        return p;
    }
    let t = if t_min >= 0.0 { t_min } else { t_max };
    (p.0 + t * dir.0, p.1 + t * dir.1)
}

/// 已解析端点的边，用于渲染计算。
struct ResolvedEdge {
    id: String,
    from: EndpointShape,
    to: EndpointShape,
    label: Option<String>,
}

/// 基于路径锚点解析的边渲染批计算。
///
/// 对每条边，用 [`resolve_edge_endpoint_anchor`] 解析 from/to 端点。
/// 任一端点解析失败则记录诊断并跳过该边（不静默丢掉）。
/// 成功解析的边按原有几何算法渲染。
pub fn compute_edge_renders_from_paths(
    edges: &[writer_core::starmap::types::StarMapEdge],
    graph: &StarMapGraph,
    layout: &StarMapLayout,
    embed_rects: &[StarMapEmbedSceneRect],
    params: &EdgeRenderParams,
) -> EdgeRenderBatch {
    let mut renders = Vec::new();
    let mut diagnostics = Vec::new();

    let mut resolved_edges: Vec<ResolvedEdge> = Vec::new();

    for edge in edges {
        let from = match resolve_edge_endpoint_anchor(
            &edge.from,
            graph,
            layout,
            embed_rects,
            "from",
            &edge.id,
        ) {
            Ok(p) => p,
            Err(d) => {
                diagnostics.push(d);
                continue;
            }
        };
        let to = match resolve_edge_endpoint_anchor(
            &edge.to,
            graph,
            layout,
            embed_rects,
            "to",
            &edge.id,
        ) {
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

/// 拉线候选边的固定渲染 id：只存在于预览计算里，不落库、不与其他边冲突。
pub const PROSPECTIVE_EDGE_ID: &str = "__preview__";

/// 候选边预览：把候选边临时追加到宿主图现有边表，复用正式边的全套几何
/// （Node 矩形 / Embed 圆周、旧 portal 归一、深路径投影、双向边偏移），
/// 只返回候选边自己的 render。
///
/// QML 拉线预览只需要把 prospective LCA 规划出的 from/to 路径交给这里：
/// 预览与松手后的正式边天然同源。已有反向边时，候选边会和正式边一样
/// 进入双向模式并带上 `bidirectional_offset`，预览不会在松手瞬间平移。
pub fn compute_prospective_edge_render(
    from_path: &StarMapTargetPath,
    to_path: &StarMapTargetPath,
    graph: &StarMapGraph,
    layout: &StarMapLayout,
    embed_rects: &[StarMapEmbedSceneRect],
    params: &EdgeRenderParams,
) -> Result<EdgeRender, EdgeAnchorDiagnostic> {
    let candidate = writer_core::starmap::types::StarMapEdge {
        id: PROSPECTIVE_EDGE_ID.to_string(),
        from: from_path.clone(),
        to: to_path.clone(),
        kind: writer_core::starmap::types::StarMapEdgeKind::RelatedTo,
        label: None,
        payload: None,
        created_at: 0,
        updated_at: 0,
    };
    let mut edges = graph.edges.clone();
    edges.push(candidate);

    let mut batch = compute_edge_renders_from_paths(&edges, graph, layout, embed_rects, params);
    let index = batch
        .renders
        .iter()
        .position(|r| r.edge_id == PROSPECTIVE_EDGE_ID);
    match index {
        Some(i) => Ok(batch.renders.remove(i)),
        None => Err(batch
            .diagnostics
            .into_iter()
            .find(|d| d.edge_id == PROSPECTIVE_EDGE_ID)
            .unwrap_or(EdgeAnchorDiagnostic {
                edge_id: PROSPECTIVE_EDGE_ID.to_string(),
                endpoint: "from".to_string(),
                reason: EdgeAnchorDiagnosticReason::LocalNodeMissing,
            })),
    }
}

/// 从已解析端点的边列表计算渲染几何。
fn compute_renders_from_resolved(
    resolved_edges: &[ResolvedEdge],
    params: &EdgeRenderParams,
    renders: &mut Vec<EdgeRender>,
    diagnostics: &mut Vec<EdgeAnchorDiagnostic>,
) {
    // 双向边检测：基于端点中心坐标对匹配（而非 node_id），因为锚点解析后只有几何。
    let bidirectional_set: std::collections::HashSet<(usize, usize)> = resolved_edges
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            resolved_edges.iter().enumerate().find_map(|(j, o)| {
                if i != j && coords_eq(e.from.center(), o.to.center())
                    && coords_eq(e.to.center(), o.from.center())
                {
                    Some((i, j))
                } else {
                    None
                }
            })
        })
        .collect();

    for (i, edge) in resolved_edges.iter().enumerate() {
        let (fx, fy) = edge.from.center();
        let (tx, ty) = edge.to.center();
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

        // 双向边的平行偏移：整条连线垂直平移，两端交点仍然落在各自形状边界上。
        let has_bi = bidirectional_set.iter().any(|(a, b)| *a == i || *b == i);
        let offset = if has_bi {
            params.bidirectional_offset
        } else {
            0.0
        };
        let dir_x = dx / len;
        let dir_y = dy / len;
        let ox = if has_bi { -dir_y * offset } else { 0.0 };
        let oy = if has_bi { dir_x * offset } else { 0.0 };

        // 正式边端点 = 各自形状（Node 矩形 / Embed 圆周）与连线的真实边界交点。
        // 不再用固定内缩常量猜边界：200 正圆 Embed 上固定内缩会扎进圆里。
        let (sx, sy) =
            endpoint_boundary_point(&edge.from, (fx + ox, fy + oy), (dir_x, dir_y));
        let (ex, ey) =
            endpoint_boundary_point(&edge.to, (tx + ox, ty + oy), (-dir_x, -dir_y));

        // 箭头沿实际画出的线段方向（start → end）。
        let adx = ex - sx;
        let ady = ey - sy;
        let angle = if adx * adx + ady * ady > 1e-12 {
            ady.atan2(adx)
        } else {
            dy.atan2(dx)
        };
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
        let dist = distance_to_edge_render(x, y, r);
        if dist < threshold && dist < closest_dist {
            closest_dist = dist;
            closest_id = Some(r.edge_id.clone());
        }
    }

    closest_id
}

/// 点到"画出来的边"的最短距离：箭杆 + 箭头三角形。
///
/// 箭头是边的一部分（QML 会 fill 出三角形），只测箭杆会在高倍缩放下出现
/// "点在箭头上却点不中"：箭头长 10 world、半角 30°，翼尖离主线可达
/// `10 * sin(30°) = 5 world`，远大于放大后的屏幕折算阈值。
fn distance_to_edge_render(x: f32, y: f32, r: &EdgeRender) -> f32 {
    let shaft = point_to_segment_distance(x, y, r.start_x, r.start_y, r.end_x, r.end_y);
    shaft.min(arrow_triangle_distance(x, y, r))
}

/// 点到箭头三角形的距离：在三角形内部为 0，否则取到三条边的最短距离。
fn arrow_triangle_distance(x: f32, y: f32, r: &EdgeRender) -> f32 {
    let (tx, ty) = (r.arrow_tip_x, r.arrow_tip_y);
    let (lx, ly) = (r.arrow_left_x, r.arrow_left_y);
    let (rx, ry) = (r.arrow_right_x, r.arrow_right_y);

    if point_in_triangle(x, y, tx, ty, lx, ly, rx, ry) {
        return 0.0;
    }
    point_to_segment_distance(x, y, tx, ty, lx, ly)
        .min(point_to_segment_distance(x, y, lx, ly, rx, ry))
        .min(point_to_segment_distance(x, y, rx, ry, tx, ty))
}

#[cfg(test)]
mod tests;
