//! # 思维导图快照生成模块
//!
//! 本模块负责生成思维导图的只读快照，用于前端渲染和展示。快照包含了
//! 图形数据和布局信息的合并结果，以及计算出的边界信息。
//!
//! ## 主要功能
//! - **快照生成**：将图形数据和布局信息合并为快照
//! - **边界计算**：自动计算所有节点的边界框
//! - **锚点统计**：统计每个节点关联的锚点数量
//! - **断链检测**：检测节点是否存在断开的链接
//!
//! ## 数据结构
//!
//! ### `MindMapSnapshot`
//! 思维导图快照，包含完整的可视化信息：
//! - `project_id`：项目ID
//! - `layout_kind`：布局类型
//! - `nodes`：快照节点列表
//! - `edges`：快照边列表
//! - `bounds`：边界信息
//! - `generated_at`：生成时间戳
//!
//! ### `MindMapSnapshotNode`
//! 快照节点，包含节点的完整可视化信息：
//! - `id`：节点ID
//! - `title`：节点标题
//! - `kind`：节点类型
//! - `x`, `y`：节点位置
//! - `width`, `height`：节点大小
//! - `radius`：节点圆角半径
//! - `collapsed`：是否折叠
//! - `anchor_count`：关联的锚点数量
//! - `broken_link`：是否存在断开的链接
//! - `tags`：标签列表
//!
//! ### `MindMapSnapshotEdge`
//! 快照边，包含边的可视化信息：
//! - `id`：边ID
//! - `from`：起始节点ID
//! - `to`：结束节点ID
//! - `kind`：边类型
//! - `label`：边标签
//!
//! ### `MindMapBounds`
//! 边界信息，用于确定视图范围：
//! - `min_x`, `min_y`：最小坐标
//! - `max_x`, `max_y`：最大坐标
//!
//! ## 核心函数
//! - `generate_snapshot`：生成思维导图快照
//!
//! ## 依赖关系
//! - `crate::mind_map::graph`：思维导图数据类型
//! - `crate::mind_map::layout`：布局类型
//! - `serde`：JSON序列化/反序列化
//! - `std::collections`：HashMap和HashSet
//!
//! ## 使用场景
//! - 前端渲染思维导图
//! - 计算视图边界
//! - 统计节点的锚点信息
//! - 检测断开的链接

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
    let anchor_ids: HashSet<&str> = graph.anchors.iter().map(|a| a.id.as_str()).collect();

    // 3. Build link_count_by_node_id and broken_link_by_node_id
    let mut link_count_by_node_id = HashMap::new();
    let mut broken_link_by_node_id = HashMap::new();

    for link in &graph.links {
        *link_count_by_node_id
            .entry(link.node_id.as_str())
            .or_insert(0) += 1;
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

        if x - w / 2.0 < min_x {
            min_x = x - w / 2.0;
        }
        if y - h / 2.0 < min_y {
            min_y = y - h / 2.0;
        }
        if x + w / 2.0 > max_x {
            max_x = x + w / 2.0;
        }
        if y + h / 2.0 > max_y {
            max_y = y + h / 2.0;
        }

        let anchor_count = link_count_by_node_id
            .get(g_node.id.as_str())
            .copied()
            .unwrap_or(0);
        let broken_link = broken_link_by_node_id
            .get(g_node.id.as_str())
            .copied()
            .unwrap_or(false);

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

    let snapshot_edges = graph
        .edges
        .iter()
        .map(|e| MindMapSnapshotEdge {
            id: e.id.clone(),
            from: e.from.clone(),
            to: e.to.clone(),
            kind: format!("{:?}", e.kind),
            label: e.label.clone(),
        })
        .collect();

    MindMapSnapshot {
        project_id: graph.project_id.clone(),
        layout_kind: format!("{:?}", layout.kind),
        nodes: snapshot_nodes,
        edges: snapshot_edges,
        bounds: MindMapBounds {
            min_x,
            min_y,
            max_x,
            max_y,
        },
        generated_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    }
}
