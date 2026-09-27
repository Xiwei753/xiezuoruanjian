use crate::error::{Error, Result};
use crate::starmap::graph::resolve::{resolve_target_path, GraphResolverContext};
use crate::starmap::types::reference::StarMapTargetPath;
use crate::starmap::types::*;

/// 校验 hyperlink target_uri 的 scheme。
///
/// URI 必须有合法 scheme：以 `xxx:` 开头，scheme 第一字符必须是 ASCII 字母，
/// 后续字符允许 ASCII 字母/数字/+/-/.。
/// 不用 `contains("://")` 因为 `mailto:`、`tel:` 等没有 `//`。
pub(crate) fn validate_hyperlink_uri(uri: &str) -> Result<()> {
    let colon = uri.find(':').ok_or_else(|| {
        Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "target_uri must have a scheme (e.g. 'https:', 'mailto:')",
        ))
    })?;
    let scheme = &uri[..colon];
    if scheme.is_empty() {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "target_uri scheme must not be empty",
        )));
    }
    let mut chars = scheme.chars();
    let first = chars.next().ok_or_else(|| {
        Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "target_uri scheme must not be empty",
        ))
    })?;
    if !first.is_ascii_alphabetic() {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "target_uri scheme first character must be a letter",
        )));
    }
    for c in chars {
        if !c.is_ascii_alphanumeric() && c != '+' && c != '-' && c != '.' {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("target_uri scheme contains invalid character: {c}"),
            )));
        }
    }
    Ok(())
}

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
/// - 超链接的 `hyperlink_id` 全局唯一，source 路径合法，URI 非空且有 scheme
/// - Portal 的 `destination_starmap_id` 必须存在，可选落点在目标图中存在
/// - 数值字段 finite（无 NaN/Inf）
pub(crate) fn validate_graph(context: &GraphResolverContext, graph: &StarMapGraph) -> Result<()> {
    let node_ids = validate_nodes(context, graph)?;
    validate_edges(context, graph, &node_ids)?;
    validate_embeds(context, graph, &node_ids)?;
    validate_links(context, graph, &node_ids)?;
    validate_hyperlinks(context, graph, &node_ids)?;
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
    context: &GraphResolverContext,
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
            // 所有 mode（EnterPortal/PreviewInline/ReferenceOnly）都必须保证
            // destination_starmap_id 存在且落点可达。统一走 resolve_target_path：
            // 无论 destination_target 有没有值，都构造 synthetic path 解析。
            // 无 destination_target 时用 StarMapTargetDetail::Starmap（只检查目标星图存在）。
            let path = StarMapTargetPath {
                starmap_id: portal.destination_starmap_id.clone(),
                segments: vec![],
                target: portal
                    .destination_target
                    .clone()
                    .unwrap_or(crate::starmap::semantic::StarMapTargetDetail::Starmap),
            };
            let status = resolve_target_path(context, &path);
            use crate::starmap::semantic::StarMapTargetResolveStatus::*;
            match status {
                Resolved => {}
                _ => {
                    return Err(Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Portal destination resolve failed: {:?}", status),
                    )));
                }
            }
        }
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
    context: &GraphResolverContext,
    graph: &StarMapGraph,
    node_ids: &std::collections::HashSet<String>,
) -> Result<()> {
    let mut edge_ids = std::collections::HashSet::new();
    for edge in &graph.edges {
        // Edge ID 全局唯一，与 node/embed/link/hyperlink 一致。
        // add_edge 也会显式拒绝重复 ID，这里在 validate_graph 层再守一道。
        if !edge_ids.insert(&edge.id) {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Duplicate edge ID",
            )));
        }
        validate_target_path(context, &edge.from, graph, node_ids, "from")?;
        validate_target_path(context, &edge.to, graph, node_ids, "to")?;
    }
    Ok(())
}

/// 校验单条目标路径的引用完整性。
///
/// ## 不变量
///
/// `path.starmap_id` 必须等于 `graph.starmap_id`——路径起点必须是宿主图
/// 本身（见 `types/reference.rs` 的语义定义）。这防止在 A 图里保存
/// `starmap_id = B` 的路径。
///
/// ## context
///
/// 跨层路径走 resolver 时，使用 context（包含 overlays）使 resolver 优先
/// 从内存图读取对象（内存刚改完、磁盘还没 flush 的场景）。
#[allow(clippy::excessive_nesting)]
fn validate_target_path(
    context: &GraphResolverContext,
    path: &StarMapTargetPath,
    graph: &StarMapGraph,
    node_ids: &std::collections::HashSet<String>,
    endpoint_name: &str,
) -> Result<()> {
    // 不变量：路径起点必须等于宿主图。
    if path.starmap_id != graph.starmap_id {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "{} target path starmap_id '{}' does not match host graph '{}'",
                endpoint_name, path.starmap_id, graph.starmap_id
            ),
        )));
    }

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
            crate::starmap::semantic::StarMapTargetDetail::ChapterRange {
                range_start,
                range_end,
                ..
            } => {
                // 空 segments 的 ChapterRange 也要校验 range，
                // 不能只有跨层才经过 resolver。
                if let (Some(s), Some(e)) = (range_start, range_end) {
                    if s > e {
                        return Err(Error::Io(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!(
                                "Edge {} ChapterRange range_start > range_end",
                                endpoint_name
                            ),
                        )));
                    }
                }
            }
            _ => {}
        }
    } else {
        // 跨星图路径，调用 resolver 验证，使用 context（包含 overlays）
        let status = resolve_target_path(context, path);
        use crate::starmap::semantic::StarMapTargetResolveStatus::*;
        match status {
            CycleDetected | TooDeep | MissingStarmap | MissingNode | MissingAnchor
            | MissingEmbed | MissingPortal | InvalidRange | UnsupportedVersion | CorruptStarmap
            | ReadFailed => {
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
/// position 数值合法性、host_path 引用完整性。
#[allow(
    clippy::excessive_nesting,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]
fn validate_embeds(
    context: &GraphResolverContext,
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

        // 统一走 resolve_target_path 校验目标星图存在且可读。
        let embed_target_path = StarMapTargetPath {
            starmap_id: embed.target_starmap_id.clone(),
            segments: vec![],
            target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
        };
        let embed_status = resolve_target_path(context, &embed_target_path);
        use crate::starmap::semantic::StarMapTargetResolveStatus::*;
        match embed_status {
            Resolved => {}
            _ => {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Embed target starmap resolve failed: {:?}", embed_status),
                )));
            }
        }

        // position 数值必须 finite。
        if !embed.position.x.is_finite() || !embed.position.y.is_finite() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid embed position values",
            )));
        }

        // 验证 host_path 引用完整性：统一走 validate_target_path，
        // 无 segments 时做本地校验，有 segments 时走 resolve_target 跨星图解析。
        validate_target_path(
            context,
            &embed.host_path,
            graph,
            node_ids,
            "embed host_path",
        )?;
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
    context: &GraphResolverContext,
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
        validate_target_path(context, &link.source, graph, node_ids, "link source")?;
        // 验证 target 路径
        validate_target_path(context, &link.target, graph, node_ids, "link target")?;
    }
    Ok(())
}

/// 验证超链接：hyperlink_id 唯一、source 路径合法、target_uri 非空且有合法 scheme。
fn validate_hyperlinks(
    context: &GraphResolverContext,
    graph: &StarMapGraph,
    node_ids: &std::collections::HashSet<String>,
) -> Result<()> {
    let mut hyperlink_ids = std::collections::HashSet::new();
    for hl in &graph.hyperlinks {
        if !hyperlink_ids.insert(&hl.hyperlink_id) {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Duplicate hyperlink_id",
            )));
        }
        // source 路径必须合法
        validate_target_path(context, &hl.source, graph, node_ids, "hyperlink source")?;
        // target_uri 非空
        if hl.target_uri.is_empty() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Hyperlink target_uri cannot be empty",
            )));
        }
        // target_uri 必须有合法 scheme（统一调用 validate_hyperlink_uri）
        validate_hyperlink_uri(&hl.target_uri)?;
    }
    Ok(())
}
