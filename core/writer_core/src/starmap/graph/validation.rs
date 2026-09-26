use crate::error::{Error, Result};
use crate::starmap::types::*;

/// 图数据完整性验证入口。
///
/// 在 `save_starmap_graph` 保存前调用，确保写入磁盘的数据满足引用完整性不变量。
/// 验证失败时阻止保存（返回 Err），避免持久化损坏的图数据。
///
/// ## 验证不变量
///
/// - 节点 ID 全局唯一
/// - 边端点引用的节点/锚点必须存在
/// - 嵌入的 `instance_id` 全局唯一，且不能自嵌入
/// - 链接的 `link_id` 全局唯一
/// - Portal target 可达（无循环、无缺失）
/// - DisplayPolicy scale 层级有序
/// - 数值字段无 NaN/非法值
pub(crate) fn validate_graph(app_data_root: &std::path::Path, graph: &StarMapGraph) -> Result<()> {
    let node_ids = validate_nodes(app_data_root, graph)?;
    validate_edges(app_data_root, graph, &node_ids)?;
    validate_embeds(app_data_root, graph, &node_ids)?;
    validate_links(app_data_root, graph, &node_ids)?;
    Ok(())
}

/// 验证节点：ID 唯一性、内容范围合法性、锚点 ID 唯一性、portal 可达性、display_policy。
/// 返回节点 ID 集合，供后续边/嵌入/链接验证使用。
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
fn validate_nodes(
    app_data_root: &std::path::Path,
    graph: &StarMapGraph,
) -> Result<std::collections::HashSet<String>> {
    let mut node_ids = std::collections::HashSet::new();
    for node in &graph.nodes {
        if !node_ids.insert(node.id.clone()) {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Duplicate node ID",
            )));
        }

        if let crate::starmap::semantic::StarMapNodeContent::ChapterRef {
            range_start,
            range_end,
            ..
        } = &node.content
        {
            if let (Some(s), Some(e)) = (range_start, range_end) {
                if s > e {
                    return Err(Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Content range_start cannot be greater than range_end",
                    )));
                }
            }
        }

        let mut anchor_ids = std::collections::HashSet::new();
        for anchor in &node.anchors {
            if !anchor_ids.insert(&anchor.anchor_id) {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Duplicate anchor ID in node",
                )));
            }
            if let crate::starmap::semantic::StarMapAnchorTarget::ChapterRange {
                range_start,
                range_end,
                ..
            } = &anchor.target
            {
                if let (Some(s), Some(e)) = (range_start, range_end) {
                    if s > e {
                        return Err(Error::Io(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "Anchor range_start cannot be greater than range_end",
                        )));
                    }
                }
            }
        }

        if let Some(portal) = &node.portal {
            if portal.mode == crate::starmap::semantic::StarMapPortalMode::EnterPortal {
                let status = super::resolve::resolve_target_path(app_data_root, &portal.target);
                use crate::starmap::semantic::StarMapTargetResolveStatus::*;
                match status {
                    CycleDetected | TooDeep | MissingStarmap | MissingNode | MissingAnchor
                    | MissingEmbed | MissingPortal | InvalidRange => {
                        return Err(Error::Io(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("Portal target resolve failed: {:?}", status),
                        )));
                    }
                    _ => {}
                }
            }
        }

        crate::starmap::semantic::validate_display_policy(&node.display_policy)?;
    }
    Ok(node_ids)
}

/// 验证边：端点引用完整性。
///
/// 每条边的 from/to 端点使用 `StarMapTargetPath`，
/// 验证路径中的节点/锚点在当前星图中存在，或跨星图路径可达。
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
fn validate_edges(
    app_data_root: &std::path::Path,
    graph: &StarMapGraph,
    node_ids: &std::collections::HashSet<String>,
) -> Result<()> {
    for edge in &graph.edges {
        validate_target_path(app_data_root, &edge.from, graph, node_ids, "from")?;
        validate_target_path(app_data_root, &edge.to, graph, node_ids, "to")?;
    }
    Ok(())
}

fn validate_target_path(
    app_data_root: &std::path::Path,
    path: &crate::starmap::types::reference::StarMapTargetPath,
    graph: &StarMapGraph,
    node_ids: &std::collections::HashSet<String>,
    endpoint_name: &str,
) -> Result<()> {
    // 如果路径没有 segments，则 target 在当前星图中
    if path.segments.is_empty() {
        match &path.target {
            crate::starmap::semantic::StarMapTargetDetail::Node { node_id } => {
                if !node_ids.contains(node_id) {
                    return Err(Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Edge {} references non-existent node", endpoint_name),
                    )));
                }
            }
            crate::starmap::semantic::StarMapTargetDetail::Anchor { node_id, anchor_id } => {
                let anchor_found = graph
                    .nodes
                    .iter()
                    .find(|n| &n.id == node_id)
                    .is_some_and(|node| node.anchors.iter().any(|a| &a.anchor_id == anchor_id));
                if !anchor_found {
                    return Err(Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Edge {} references non-existent anchor", endpoint_name),
                    )));
                }
            }
            _ => {}
        }
    } else {
        // 跨星图路径，调用 resolver 验证
        let status = super::resolve::resolve_target_path(app_data_root, path);
        use crate::starmap::semantic::StarMapTargetResolveStatus::*;
        match status {
            CycleDetected | TooDeep | MissingStarmap | MissingNode | MissingAnchor
            | MissingEmbed | MissingPortal | InvalidRange => {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "Edge {} target path resolve failed: {:?}",
                        endpoint_name, status
                    ),
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

/// 验证嵌入：instance_id 唯一、禁止自嵌入、目标星图存在、
/// placement/viewport 数值合法性、host_path 引用完整性。
#[allow(
    clippy::excessive_nesting,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]
fn validate_embeds(
    app_data_root: &std::path::Path,
    graph: &StarMapGraph,
    node_ids: &std::collections::HashSet<String>,
) -> Result<()> {
    let mut instance_ids = std::collections::HashSet::new();
    for embed in &graph.embeds {
        if !instance_ids.insert(&embed.instance_id) {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Duplicate embed instance_id",
            )));
        }
        if embed.target_starmap_id == graph.starmap_id {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Self-embed is prohibited",
            )));
        }

        if crate::starmap::load_starmap_meta(app_data_root, &embed.target_starmap_id).is_err() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Embed target starmap does not exist",
            )));
        }

        let p = &embed.placement;
        if p.width < 0.0
            || p.height < 0.0
            || p.scale <= 0.0
            || p.width.is_nan()
            || p.height.is_nan()
            || p.scale.is_nan()
            || p.x.is_nan()
            || p.y.is_nan()
        {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid embed placement values",
            )));
        }

        let tvp = &embed.target_viewport;
        if tvp.scale <= 0.0 || tvp.scale.is_nan() || tvp.offset_x.is_nan() || tvp.offset_y.is_nan()
        {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid embed target_viewport values",
            )));
        }

        // 验证 host_path 引用完整性
        if embed.host_path.segments.is_empty() {
            match &embed.host_path.target {
                crate::starmap::semantic::StarMapTargetDetail::Node { node_id } => {
                    if !node_ids.contains(node_id) {
                        return Err(Error::Io(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "Embed host_path references non-existent node",
                        )));
                    }
                }
                crate::starmap::semantic::StarMapTargetDetail::Anchor { node_id, anchor_id } => {
                    let mut anchor_found = false;
                    if let Some(node) = graph.nodes.iter().find(|n| &n.id == node_id) {
                        if node.anchors.iter().any(|a| &a.anchor_id == anchor_id) {
                            anchor_found = true;
                        }
                    }
                    if !anchor_found {
                        return Err(Error::Io(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "Embed host_path references non-existent anchor",
                        )));
                    }
                }
                _ => {}
            }
        }

        crate::starmap::semantic::validate_display_policy(&embed.display_policy)?;
    }
    Ok(())
}

/// 验证链接：link_id 唯一、source 端点引用完整、target 路径可达。
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
fn validate_links(
    app_data_root: &std::path::Path,
    graph: &StarMapGraph,
    node_ids: &std::collections::HashSet<String>,
) -> Result<()> {
    let mut link_ids = std::collections::HashSet::new();
    for link in &graph.links {
        if !link_ids.insert(&link.link_id) {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Duplicate link_id",
            )));
        }
        // 验证 source 路径
        validate_target_path(app_data_root, &link.source, graph, node_ids, "link source")?;
        // 验证 target 路径
        validate_target_path(app_data_root, &link.target, graph, node_ids, "link target")?;
    }
    Ok(())
}

/// 布局验证：scale > 0 且非 NaN，depth/focus_weight 非 NaN。
/// 坐标值（x/y/width/height）允许为负或零，因为平台端可能使用不同坐标系原点。
pub(crate) fn validate_layout(layout: &StarMapLayout) -> Result<()> {
    for node in &layout.nodes {
        if node.scale <= 0.0
            || node.scale.is_nan()
            || node.x.is_nan()
            || node.y.is_nan()
            || node.width.is_nan()
            || node.height.is_nan()
            || node.radius.is_nan()
            || node.depth.is_nan()
            || node.focus_weight.is_nan()
        {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid layout node values",
            )));
        }
    }
    Ok(())
}
