use crate::error::{Error, Result};
use crate::starmap::now_epoch;
use crate::starmap::types::*;

/// 添加节点到星图，同时在 layout 中创建默认定位条目。
///
/// ## 双写语义
///
/// 节点同时写入 `graph.json`（语义数据）和 `layout.json`（视觉定位）。
/// layout 写入失败时静默忽略（`let _ =`），因为 layout 缺失不影响语义完整性——
/// 下次加载 layout 会回退到默认值，平台端可重新触发布局计算。
pub(crate) fn add_starmap_node(
    workspace: &std::path::Path,
    starmap_id: &str,
    node: StarMapNode,
    default_x: f32,
    default_y: f32,
) -> Result<StarMapNode> {
    let mut graph = super::ops::get_starmap_graph(workspace, starmap_id)?;
    let new_node = node.clone();
    graph.nodes.push(new_node.clone());
    graph.updated_at = now_epoch();
    super::ops::save_starmap_graph(workspace, starmap_id, &graph)?;

    if let Ok(mut layout) = super::ops::get_starmap_layout(workspace, starmap_id) {
        layout.nodes.push(StarMapLayoutNode {
            node_id: new_node.id.clone(),
            x: default_x,
            y: default_y,
            width: 150.0,
            height: 60.0,
            // 默认圆角半径 30px，与默认宽高 150×60 构成胶囊形状。
            // 这些值是 UI 层的初始占位，平台端可按需覆盖。
            radius: 30.0,
            collapsed: false,
            z_index: 0,
            scale: 1.0,
            depth: 0.0,
            focus_weight: 0.0,
            orbit_group: None,
        });
        let _ = super::ops::save_starmap_layout(workspace, starmap_id, &layout);
    }

    Ok(new_node)
}

/// 局部更新节点（patch 语义）。
///
/// `StarMapNodePatch` 中 `None` 表示"不修改"，`Some(None)` 表示"清空可选字段"。
/// 保存时 `validation::validate_graph` 会校验 portal deep_target 可达性、
/// anchor 唯一性、display_policy scale 层级等不变量。
pub(crate) fn update_starmap_node(
    workspace: &std::path::Path,
    starmap_id: &str,
    node_id: &str,
    patch: StarMapNodePatch,
) -> Result<StarMapNode> {
    let mut graph = super::ops::get_starmap_graph(workspace, starmap_id)?;
    if let Some(node) = graph.nodes.iter_mut().find(|n| n.id == node_id) {
        if let Some(t) = patch.title {
            node.title = t;
        }
        if let Some(k) = patch.kind {
            node.kind = k;
        }
        if let Some(p) = patch.payload {
            node.payload = p;
        }
        if let Some(t) = patch.tags {
            node.tags = t;
        }
        if let Some(c) = patch.content {
            node.content = c;
        }
        if let Some(a) = patch.anchors {
            node.anchors = a;
        }
        if let Some(p) = patch.portal {
            node.portal = p;
        }
        if let Some(dp) = patch.display_policy {
            node.display_policy = dp;
        }
        if let Some(ob) = patch.open_behavior {
            node.open_behavior = ob;
        }
        if let Some(p) = patch.provenance {
            node.provenance = p;
        }
        node.updated_at = now_epoch();
        let updated_node = node.clone();
        graph.updated_at = now_epoch();
        super::ops::save_starmap_graph(workspace, starmap_id, &graph)?;
        Ok(updated_node)
    } else {
        Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Node not found",
        )))
    }
}

/// 删除节点并级联清理所有引用该节点的边、嵌入、链接和布局条目。
///
/// ## 级联清理范围
///
/// 1. **edges**：`from`/`to` legacy ID 或 `from_endpoint`/`to_endpoint` 中
///    的 `Node`/`Anchor` 变体引用了此 node_id 的边全部移除。
///    `Starmap`/`DeepTarget` 端点不受影响（它们不直接引用节点 ID）。
/// 2. **embeds**：`source_node_id` 或 `host_endpoint` 引用了此 node_id 的嵌入全部移除。
/// 3. **links**：`source` 端点引用了此 node_id 的链接全部移除。
///    link 的 `target` 是 `StarMapDeepTarget`，不直接引用节点 ID，故不做级联。
/// 4. **layout**：移除对应的 `StarMapLayoutNode` 条目（静默，失败不影响主流程）。
///
/// ## 设计意图
///
/// 级联删除确保图数据引用完整性——不允许存在悬空引用。
/// 这是一次性全量扫描，适用于当前数据规模；若未来图规模增长，
/// 可能需要索引加速。
pub(crate) fn delete_starmap_node(
    workspace: &std::path::Path,
    starmap_id: &str,
    node_id: &str,
) -> Result<()> {
    let mut graph = super::ops::get_starmap_graph(workspace, starmap_id)?;
    let initial_count = graph.nodes.len();
    graph.nodes.retain(|n| n.id != node_id);
    if graph.nodes.len() == initial_count {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Node not found",
        )));
    }

    graph.edges.retain(|e| {
        let mut keep = true;
        if let Some(id) = &e.from {
            if id == node_id {
                keep = false;
            }
        }
        if let Some(id) = &e.to {
            if id == node_id {
                keep = false;
            }
        }
        if let Some(ep) = &e.from_endpoint {
            match ep {
                StarMapEdgeEndpoint::Node { node_id: id } => {
                    if id == node_id {
                        keep = false;
                    }
                }
                // Anchor 的 node_id 标识此 anchor 属于哪个节点；
                // 节点被删除时，其下所有 anchor 引用的边也必须级联删除
                StarMapEdgeEndpoint::Anchor { node_id: id, .. }
                    if id == node_id => {
                        keep = false;
                    }
                _ => {}
            }
        }
        if let Some(ep) = &e.to_endpoint {
            match ep {
                StarMapEdgeEndpoint::Node { node_id: id } => {
                    if id == node_id {
                        keep = false;
                    }
                }
                StarMapEdgeEndpoint::Anchor { node_id: id, .. }
                    if id == node_id => {
                        keep = false;
                    }
                _ => {}
            }
        }
        keep
    });

    graph.embeds.retain(|e| {
        let mut keep = true;
        if let Some(id) = &e.source_node_id {
            if id == node_id {
                keep = false;
            }
        }
        if let Some(ep) = &e.host_endpoint {
            match ep {
                StarMapEndpoint::Node { node_id: id } => {
                    if id == node_id {
                        keep = false;
                    }
                }
                StarMapEndpoint::Anchor { node_id: id, .. }
                    if id == node_id => {
                        keep = false;
                    }
                _ => {}
            }
        }
        keep
    });

    graph.links.retain(|l| {
        let mut keep = true;
        match &l.source {
            StarMapEndpoint::Node { node_id: id } => {
                if id == node_id {
                    keep = false;
                }
            }
            StarMapEndpoint::Anchor { node_id: id, .. }
                if id == node_id => {
                    keep = false;
                }
            _ => {}
        }
        keep
    });

    if let Ok(mut layout) = super::ops::get_starmap_layout(workspace, starmap_id) {
        layout.nodes.retain(|n| n.node_id != node_id);
        let _ = super::ops::save_starmap_layout(workspace, starmap_id, &layout);
    }

    graph.updated_at = now_epoch();
    super::ops::save_starmap_graph(workspace, starmap_id, &graph)?;
    Ok(())
}
