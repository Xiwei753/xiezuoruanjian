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
/// - 边端点引用的节点/锚点必须存在（legacy ID、endpoint、deep_target 三级校验）
/// - 嵌入的 `instance_id` 全局唯一，且不能自嵌入
/// - 链接的 `link_id` 全局唯一
/// - Portal deep_target 可达（无循环、无缺失）
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
            if portal.mode == crate::starmap::semantic::StarMapPortalMode::EnterChild {
                let target_id = portal
                    .deep_target
                    .as_ref()
                    .map(|t| t.starmap_id.clone())
                    .unwrap_or_else(|| portal.target_starmap_id.clone());

                if crate::starmap::load_starmap_meta(app_data_root, &target_id).is_err() {
                    return Err(Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Portal target starmap does not exist",
                    )));
                }

                if let Some(dt) = &portal.deep_target {
                    let status = super::resolve::resolve_deep_target(app_data_root, dt);
                    use crate::starmap::semantic::StarMapTargetResolveStatus::*;
                    match status {
                        CycleDetected | TooDeep | MissingStarmap | MissingNode | MissingAnchor
                        | InvalidRange => {
                            return Err(Error::Io(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                format!("Deep target resolve failed: {:?}", status),
                            )));
                        }
                        _ => {}
                    }
                }
            }
        }

        crate::starmap::semantic::validate_display_policy(&node.display_policy)?;
    }
    Ok(node_ids)
}

/// 验证边：端点引用完整性。
///
/// 每条边的 from/to 端点按优先级校验：
/// 1. `endpoint`（结构化端点）→ 检查节点/锚点存在性
/// 2. `legacy_target`（旧格式 deep_target）→ 调用 resolve_deep_target
/// 3. `legacy_id`（最旧格式节点 ID）→ 检查节点存在性
///
/// `Starmap` 端点和 `DeepTarget` 端点中的 Starmap 变体无需本地节点引用。
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
        let validate_edge_endpoint =
            |ep: &Option<StarMapEdgeEndpoint>,
             legacy_id: &Option<String>,
             legacy_target: &Option<crate::starmap::semantic::StarMapDeepTarget>,
             endpoint_name: &str|
             -> Result<()> {
                if let Some(endpoint) = ep {
                    match endpoint {
                        StarMapEdgeEndpoint::Node { node_id } => {
                            if !node_ids.contains(node_id) {
                                return Err(Error::Io(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    format!(
                                        "Edge {} endpoint references non-existent node",
                                        endpoint_name
                                    ),
                                )));
                            }
                        }
                        StarMapEdgeEndpoint::Anchor { node_id, anchor_id } => {
                            let mut anchor_found = false;
                            if let Some(node) = graph.nodes.iter().find(|n| &n.id == node_id) {
                                if node.anchors.iter().any(|a| &a.anchor_id == anchor_id) {
                                    anchor_found = true;
                                }
                            }
                            if !anchor_found {
                                return Err(Error::Io(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    format!(
                                        "Edge {} endpoint references non-existent anchor",
                                        endpoint_name
                                    ),
                                )));
                            }
                        }
                        StarMapEdgeEndpoint::Starmap => {}
                        StarMapEdgeEndpoint::DeepTarget { target } => {
                            let status = super::resolve::resolve_deep_target(app_data_root, target);
                            use crate::starmap::semantic::StarMapTargetResolveStatus::*;
                            match status {
                                CycleDetected | TooDeep | MissingStarmap | MissingNode
                                | MissingAnchor | InvalidRange => {
                                    return Err(Error::Io(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        format!("Edge deep target resolve failed: {:?}", status),
                                    )));
                                }
                                _ => {}
                            }
                        }
                    }
                } else if let Some(target) = legacy_target {
                    let status = super::resolve::resolve_deep_target(app_data_root, target);
                    use crate::starmap::semantic::StarMapTargetResolveStatus::*;
                    match status {
                        CycleDetected | TooDeep | MissingStarmap | MissingNode | MissingAnchor
                        | InvalidRange => {
                            return Err(Error::Io(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                format!("Edge deep target resolve failed: {:?}", status),
                            )));
                        }
                        _ => {}
                    }
                } else if let Some(id) = legacy_id {
                    if !node_ids.contains(id) {
                        return Err(Error::Io(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("Edge {} references non-existent node", endpoint_name),
                        )));
                    }
                }
                Ok(())
            };

        validate_edge_endpoint(&edge.from_endpoint, &edge.from, &edge.from_target, "from")?;
        validate_edge_endpoint(&edge.to_endpoint, &edge.to, &edge.to_target, "to")?;
    }
    Ok(())
}

/// 验证嵌入：instance_id 唯一、禁止自嵌入、目标星图存在、
/// placement/viewport 数值合法性、source_node_id/host_endpoint 引用完整性。
// TODO(#597): 既有代码可读性技术债，待后续重构拆分
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

        if let Some(sni) = &embed.source_node_id {
            if !node_ids.contains(sni) {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Embed source_node_id does not exist",
                )));
            }
        }

        if let Some(ep) = &embed.host_endpoint {
            match ep {
                StarMapEndpoint::Node { node_id } => {
                    if !node_ids.contains(node_id) {
                        return Err(Error::Io(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "Embed host_endpoint references non-existent node",
                        )));
                    }
                }
                StarMapEndpoint::Anchor { node_id, anchor_id } => {
                    let mut anchor_found = false;
                    if let Some(node) = graph.nodes.iter().find(|n| &n.id == node_id) {
                        if node.anchors.iter().any(|a| &a.anchor_id == anchor_id) {
                            anchor_found = true;
                        }
                    }
                    if !anchor_found {
                        return Err(Error::Io(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "Embed host_endpoint references non-existent anchor",
                        )));
                    }
                }
                StarMapEndpoint::Starmap => {}
            }
        }

        crate::starmap::semantic::validate_display_policy(&embed.display_policy)?;
    }
    Ok(())
}

/// 验证链接：link_id 唯一、source 端点引用完整、target deep_target 可达。
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
        match &link.source {
            StarMapEndpoint::Node { node_id } => {
                if !node_ids.contains(node_id) {
                    return Err(Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Link source node does not exist",
                    )));
                }
            }
            StarMapEndpoint::Anchor { node_id, anchor_id } => {
                if let Some(n) = graph.nodes.iter().find(|n| &n.id == node_id) {
                    if !n.anchors.iter().any(|a| &a.anchor_id == anchor_id) {
                        return Err(Error::Io(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "Link source anchor does not exist",
                        )));
                    }
                } else {
                    return Err(Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Link source node for anchor does not exist",
                    )));
                }
            }
            StarMapEndpoint::Starmap => {}
        }

        let status = super::resolve::resolve_deep_target(app_data_root, &link.target);
        use crate::starmap::semantic::StarMapTargetResolveStatus::*;
        match status {
            CycleDetected | TooDeep | MissingStarmap | MissingNode | MissingAnchor
            | InvalidRange => {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Link deep target resolve failed: {:?}", status),
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

/// 布局验证：scale > 0 且非 NaN，depth/focus_weight 非 NaN。
/// 坐标值（x/y/width/height）允许为负或零，因为平台端可能使用不同坐标系原点。
pub(crate) fn validate_layout(layout: &StarMapLayout) -> Result<()> {
    for node in &layout.nodes {
        if node.scale <= 0.0 || node.scale.is_nan() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Layout scale must be > 0",
            )));
        }
        if node.depth.is_nan() || node.focus_weight.is_nan() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Layout depth or focus_weight cannot be NaN",
            )));
        }
    }
    Ok(())
}

/// 视口验证：scale 为有限正值，所有偏移/尺寸为有限数。
/// offset 允许为负（视口可向左/上平移），但 NaN/Inf 会导致渲染异常。
pub(crate) fn validate_viewport(viewport: &StarMapViewport) -> Result<()> {
    if viewport.scale <= 0.0 || !viewport.scale.is_finite() {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Viewport scale must be a finite value > 0",
        )));
    }
    if !viewport.offset_x.is_finite()
        || !viewport.offset_y.is_finite()
        || !viewport.width.is_finite()
        || !viewport.height.is_finite()
    {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Viewport values must be finite",
        )));
    }
    Ok(())
}
