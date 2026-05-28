//! # 思维导图布局算法模块
//!
//! 本模块实现了思维导图的布局算法，负责计算节点在画布上的位置。
//! 支持多种布局方式，包括自动放射状布局和自由布局。
//!
//! ## 布局类型
//!
//! ### `LayoutKind`枚举
//! - `AutoRadial`：自动放射状布局，以项目节点为中心，向外辐射排列
//! - `HorizontalTree`：水平树布局（暂未实现，回退到自由布局）
//! - `Freeform`：自由布局，按顺序排列节点
//! - `Timeline`：时间线布局（暂未实现，回退到自由布局）
//! - `Relationship`：关系布局（暂未实现，回退到自由布局）
//! - `Custom`：自定义布局
//!
//! ## 数据结构
//!
//! ### `MindMapLayout`
//! 布局信息，包含布局类型和所有节点的布局数据：
//! - `kind`：布局类型
//! - `nodes`：节点布局列表
//!
//! ### `MindMapLayoutNode`
//! 单个节点的布局信息：
//! - `node_id`：节点ID
//! - `x`, `y`：节点位置坐标
//! - `width`, `height`：节点大小
//! - `radius`：节点圆角半径
//! - `collapsed`：是否折叠
//! - `z_index`：层叠顺序
//!
//! ## 布局算法
//!
//! ### 自动放射状布局 (`calculate_radial_layout`)
//! - 以项目节点为中心（0,0）
//! - 卷节点围绕项目节点均匀分布，半径200单位
//! - 章节节点围绕卷节点分布，半径400单位
//! - 未在层次结构中的节点按自由布局排列
//!
//! ### 自由布局 (`calculate_freeform_layout`)
//! - 按节点顺序从左到右排列
//! - 每行最多600单位宽度
//! - 超出宽度时自动换行
//!
//! ## 核心函数
//! - `calculate_layout`：根据布局类型计算布局
//! - `calculate_radial_layout`：计算放射状布局
//! - `calculate_freeform_layout`：计算自由布局
//!
//! ## 依赖关系
//! - `crate::mind_map::graph`：思维导图数据类型
//! - `serde`：JSON序列化/反序列化
//!
//! ## 使用场景
//! - 自动计算节点位置
//! - 为新创建的图形提供初始布局
//! - 支持多种布局算法切换
//! - 前端渲染时的位置参考

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LayoutKind {
    AutoRadial,
    HorizontalTree,
    Freeform,
    Timeline,
    Relationship,
    #[serde(other)]
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MindMapLayout {
    pub kind: LayoutKind,
    pub nodes: Vec<MindMapLayoutNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MindMapLayoutNode {
    pub node_id: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub radius: f32,
    pub collapsed: bool,
    pub z_index: i32,
}

pub fn calculate_layout(
    graph: &crate::mind_map::graph::MindMapGraph,
    kind: LayoutKind,
) -> MindMapLayout {
    match kind {
        LayoutKind::AutoRadial => calculate_radial_layout(graph),
        _ => calculate_freeform_layout(graph), // Fallback
    }
}

fn calculate_radial_layout(graph: &crate::mind_map::graph::MindMapGraph) -> MindMapLayout {
    let mut layout_nodes = Vec::new();

    // Reconstruct tree from graph
    // Find project node
    let project_node = graph
        .nodes
        .iter()
        .find(|n| n.kind == crate::mind_map::graph::MindMapNodeKind::Project);

    if let Some(project) = project_node {
        layout_nodes.push(MindMapLayoutNode {
            node_id: project.id.clone(),
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 60.0,
            radius: 40.0,
            collapsed: false,
            z_index: 0,
        });

        // Volumes
        let volumes: Vec<_> = graph
            .edges
            .iter()
            .filter(|e| {
                e.from == project.id
                    && graph.nodes.iter().any(|n| {
                        n.id == e.to && n.kind == crate::mind_map::graph::MindMapNodeKind::Volume
                    })
            })
            .map(|e| e.to.clone())
            .collect();

        let num_volumes = volumes.len();
        let volume_radius_offset = 200.0;
        let chapter_radius_offset = 400.0;

        for (i, vol_id) in volumes.iter().enumerate() {
            let vol_angle = if num_volumes > 1 {
                (i as f32) * std::f32::consts::TAU / (num_volumes as f32)
            } else {
                0.0
            };

            let vx = volume_radius_offset * vol_angle.cos();
            let vy = volume_radius_offset * vol_angle.sin();

            layout_nodes.push(MindMapLayoutNode {
                node_id: vol_id.clone(),
                x: vx,
                y: vy,
                width: 100.0,
                height: 50.0,
                radius: 30.0,
                collapsed: false,
                z_index: 0,
            });

            // Chapters
            let chapters: Vec<_> = graph
                .edges
                .iter()
                .filter(|e| {
                    e.from == *vol_id
                        && graph.nodes.iter().any(|n| {
                            n.id == e.to
                                && n.kind == crate::mind_map::graph::MindMapNodeKind::Chapter
                        })
                })
                .map(|e| e.to.clone())
                .collect();

            let num_chapters = chapters.len();
            let spread_angle = std::f32::consts::PI / 2.0;
            let start_angle = vol_angle - spread_angle / 2.0;
            let angle_step = if num_chapters > 1 {
                spread_angle / ((num_chapters - 1) as f32)
            } else {
                0.0
            };

            for (j, chap_id) in chapters.iter().enumerate() {
                let chap_angle = start_angle + (j as f32) * angle_step;
                let cx = chapter_radius_offset * chap_angle.cos();
                let cy = chapter_radius_offset * chap_angle.sin();

                layout_nodes.push(MindMapLayoutNode {
                    node_id: chap_id.clone(),
                    x: cx,
                    y: cy,
                    width: 80.0,
                    height: 40.0,
                    radius: 20.0,
                    collapsed: false,
                    z_index: 0,
                });
            }
        }
    }

    // Add remaining nodes as freeform
    let mut current_y = 200.0;
    for node in &graph.nodes {
        if !layout_nodes.iter().any(|ln| ln.node_id == node.id) {
            layout_nodes.push(MindMapLayoutNode {
                node_id: node.id.clone(),
                x: 200.0,
                y: current_y,
                width: 100.0,
                height: 50.0,
                radius: 25.0,
                collapsed: false,
                z_index: 0,
            });
            current_y += 100.0;
        }
    }

    MindMapLayout {
        kind: LayoutKind::AutoRadial,
        nodes: layout_nodes,
    }
}

fn calculate_freeform_layout(graph: &crate::mind_map::graph::MindMapGraph) -> MindMapLayout {
    let mut layout_nodes = Vec::new();
    let mut x = 0.0;
    let mut y = 0.0;

    for node in &graph.nodes {
        layout_nodes.push(MindMapLayoutNode {
            node_id: node.id.clone(),
            x,
            y,
            width: 100.0,
            height: 50.0,
            radius: 20.0,
            collapsed: false,
            z_index: 0,
        });
        x += 120.0;
        if x > 600.0 {
            x = 0.0;
            y += 80.0;
        }
    }

    MindMapLayout {
        kind: LayoutKind::Freeform,
        nodes: layout_nodes,
    }
}
