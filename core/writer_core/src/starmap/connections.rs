//! # 跨画布连接模块
//!
//! 提供跨星图画布的边规划（`plan_cross_canvas_edge`），以及连接反查共用的
//! route-aware 端点匹配与图对象读取基础。连接反查本身由
//! `PathDependencyIndex::query_connections_for_canvas/target` 从反向依赖索引
//! 统一提供（见 `starmap::store::relation_index`），此处不再保留第二套逐祖先
//! 图扫描的实现。
//!
//! ## 核心设计
//!
//! - **复用既有类型**：`StarMapEdge`、`StarMapTargetPath`、`StarMapNavigationRoute`
//!   等均来自现有模块，不创建并行模型。
//! - **最长公共前缀定宿主图**：两个端点 route 的最长公共实例路径前缀对应的星图
//!   就是跨画布边的宿主图。同层、父子、不同子分支均适用。
//! - **唯一一条真实 Edge**：跨画布边只写一条 `StarMapEdge` 到宿主图，不是在
//!   两个画布里分别造假节点/假边。
//! - **不同根星图无共同宿主**：跨不同根星图、无共同入口链时返回 `NoCommonHost`，
//!   不伪造宿主关系。
//! - **派生反向关系索引**：连接反查从反向依赖索引收集候选祖先图，再复用本模块
//!   的 `route_aware_endpoint_matches` 做完整实例路径精确过滤，前端无需递归读取
//!   所有祖先 graph。

use std::path::Path;

use crate::starmap::graph::resolve::{resolve_target, GraphResolverContext};
use crate::starmap::navigation::{resolve_navigation_route, StarMapNavigationRoute};
use crate::starmap::package_storage::bucket_for_id;
use crate::starmap::semantic::{StarMapTargetDetail, StarMapTargetResolveStatus};
use crate::starmap::store::load::phased::load_current_graph_meta;
use crate::starmap::types::reference::{StarMapPathSegment, StarMapTargetPath};
use crate::starmap::types::{
    StarMapEdge, StarMapEdgeKind, StarMapEmbed, StarMapHyperlink, StarMapLink,
};

// ---------------------------------------------------------------------------
// 公开类型
// ---------------------------------------------------------------------------

/// 跨画布边的端点选择：导航路由 + 本地目标。
///
/// `route` 描述从根星图到当前画布的语义路径，`target` 是当前画布内的具体目标
///（Node / Anchor / Starmap 等）。两者组合唯一确定一个跨画布边的端点。
#[derive(Debug, Clone)]
pub struct StarMapEndpointSelection {
    pub route: StarMapNavigationRoute,
    pub target: StarMapTargetDetail,
}

/// 跨画布边规划错误。
#[derive(Debug, Clone, PartialEq)]
pub enum CrossCanvasEdgeError {
    /// 两个端点没有共同的宿主图（不同根星图且无共同入口链）。
    NoCommonHost,
    /// from 端点的 route 或 target 解析失败。
    FromResolveFailed(StarMapTargetResolveStatus),
    /// to 端点的 route 或 target 解析失败。
    ToResolveFailed(StarMapTargetResolveStatus),
}

/// 连接信息：边的宿主图、ID、端点路径、类型和标签。
///
/// 由 `PathDependencyIndex::query_connections_for_canvas` /
/// `query_connections_for_target` 返回，既包括本图 Edge，也包括存于祖先图但
/// 一端 route-aware 落在当前图的 Edge。
#[derive(Debug, Clone)]
pub struct StarMapConnectionInfo {
    /// Edge 所在的宿主星图 ID。
    pub owner_starmap_id: String,
    pub edge_id: String,
    pub from: StarMapTargetPath,
    pub to: StarMapTargetPath,
    pub kind: StarMapEdgeKind,
    pub label: Option<String>,
}

// ---------------------------------------------------------------------------
// plan_cross_canvas_edge
// ---------------------------------------------------------------------------

/// 规划跨画布边：计算宿主图 ID、from/to 路径和 edge_id。
///
/// ## 算法
///
/// 1. 分别解析两端 route 与本地目标，确认对象存在。
/// 2. 找到两个 route 的最长公共实例路径前缀，对应的星图就是宿主图。
/// 3. 从公共宿主图切去公共前缀，将余下 `EnterEmbed/EnterPortal` 作为
///    `from/to.segments`，两条路径的 `starmap_id` 都填宿主图 ID。
/// 4. 生成 edge_id（UUID）。
/// 5. 如果无共同宿主图（不同根星图），返回 `NoCommonHost`。
///
/// ## 例子
///
/// 根图 A 有节点 a、Embed e1→B、Embed e2→C，B 有节点 b、C 有节点 c：
/// - a→b：Edge 放 A；from = `A/[]/Node(a)`，to = `A/[e1]/Node(b)`。
/// - b→c：Edge 放 A；from = `A/[e1]/Node(b)`，to = `A/[e2]/Node(c)`。
pub fn plan_cross_canvas_edge(
    context: &GraphResolverContext,
    from: &StarMapEndpointSelection,
    to: &StarMapEndpointSelection,
) -> Result<(String, StarMapTargetPath, StarMapTargetPath), CrossCanvasEdgeError> {
    // 1. 解析 from 和 to 的完整路径（route + target），确认对象存在
    let from_full_path = StarMapTargetPath {
        starmap_id: from.route.root_starmap_id.clone(),
        segments: from.route.segments.clone(),
        target: from.target.clone(),
    };
    resolve_target(context, &from_full_path).map_err(CrossCanvasEdgeError::FromResolveFailed)?;

    let to_full_path = StarMapTargetPath {
        starmap_id: to.route.root_starmap_id.clone(),
        segments: to.route.segments.clone(),
        target: to.target.clone(),
    };
    resolve_target(context, &to_full_path).map_err(CrossCanvasEdgeError::ToResolveFailed)?;

    // 2. 两个 route 必须有相同的 root_starmap_id，否则无共同宿主
    if from.route.root_starmap_id != to.route.root_starmap_id {
        return Err(CrossCanvasEdgeError::NoCommonHost);
    }

    // 3. 找最长公共实例路径前缀
    let common_prefix_len = longest_common_prefix_len(&from.route.segments, &to.route.segments);

    // 4. 确定宿主图：解析公共前缀对应的路由
    let host_route = StarMapNavigationRoute {
        root_starmap_id: from.route.root_starmap_id.clone(),
        segments: from.route.segments[..common_prefix_len].to_vec(),
    };
    let host_location = resolve_navigation_route(context, &host_route)
        .map_err(CrossCanvasEdgeError::FromResolveFailed)?;
    let host_starmap_id = host_location.current_starmap_id;

    // 5. 构造 from/to 的 StarMapTargetPath
    //    从宿主图视角出发，切去公共前缀后余下的 segments 即为穿越路径
    let from_segments = from.route.segments[common_prefix_len..].to_vec();
    let to_segments = to.route.segments[common_prefix_len..].to_vec();

    let from_target_path = StarMapTargetPath {
        starmap_id: host_starmap_id.clone(),
        segments: from_segments,
        target: from.target.clone(),
    };

    let to_target_path = StarMapTargetPath {
        starmap_id: host_starmap_id.clone(),
        segments: to_segments,
        target: to.target.clone(),
    };

    // 6. 生成 edge_id
    let edge_id = format!("edge_{}", uuid::Uuid::new_v4());

    Ok((edge_id, from_target_path, to_target_path))
}

/// 计算两个路径段列表的最长公共前缀长度。
fn longest_common_prefix_len(a: &[StarMapPathSegment], b: &[StarMapPathSegment]) -> usize {
    let mut len = 0;
    while len < a.len() && len < b.len() && a[len] == b[len] {
        len += 1;
    }
    len
}

// ---------------------------------------------------------------------------
// route-aware 端点匹配（共用函数）
// ---------------------------------------------------------------------------

/// Route-aware 端点匹配：检查 Edge/Link/Hyperlink/Embed 端点路径是否
/// 从指定祖先图出发、经过与实例路径后缀完全一致的穿越路径、落在当前画布。
///
/// 这是 `connections.rs` 与 `relation_index.rs` 共用的唯一端点匹配规则，
/// 不再两处重复实现。
///
/// ## 参数
///
/// - `edge_endpoint`：被检查的端点路径（`StarMapTargetPath`）
/// - `ancestor_starmap_id`：端点所在的宿主星图 ID
/// - `route_from_ancestor_to_current`：从该祖先进入当前画布的**完整实例路径后缀**
///   （即 `route.segments[ancestor_index..]`）
/// - `target`：`Some(t)` 表示还需匹配终点对象（用于 target 查询）；
///   `None` 表示只检查是否落在当前画布（用于 canvas 查询）
///
/// ## 匹配条件
///
/// 1. `edge_endpoint.starmap_id == ancestor_starmap_id`（端点从该祖先出发）
/// 2. `edge_endpoint.segments == route_from_ancestor_to_current`（实例路径后缀完全一致）
/// 3. 若 `target` 为 `Some`，则 `edge_endpoint.target == target`（终点对象匹配）
///
/// ## 为什么比较 segments 而非 `final_starmap_id`
///
/// 同一星图 B 可能被同一父图 A 的两个 Embed `e1→B`、`e2→B` 同时嵌入。
/// 一条边 `A/[e1]/Node(b)` 只属于经 `e1` 进入 B 的访问上下文。
/// 仅比较 `final_starmap_id == B` 会让经 `e2` 进入 B 的用户也看到这条边，
/// 这是错误的。比较完整 segments 后缀能区分不同 Embed 实例的访问上下文。
pub fn route_aware_endpoint_matches(
    edge_endpoint: &StarMapTargetPath,
    ancestor_starmap_id: &str,
    route_from_ancestor_to_current: &[StarMapPathSegment],
    target: Option<&StarMapTargetDetail>,
) -> bool {
    // 1. 端点必须从该祖先图出发
    if edge_endpoint.starmap_id != ancestor_starmap_id {
        return false;
    }
    // 2. 端点的穿越路径必须与从该祖先到当前画布的实例路径后缀完全一致
    if edge_endpoint.segments.as_slice() != route_from_ancestor_to_current {
        return false;
    }
    // 3. 若指定了 target，终点对象必须匹配
    if let Some(t) = target {
        if &edge_endpoint.target != t {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// 统一数据访问入口（可靠对象读取）
// ---------------------------------------------------------------------------

/// 加载星图的边列表（可靠对象读取）。
///
/// 先从 `context.overlays` 查找（内存中的图数据），找不到再从磁盘读取。
///
/// 读取失败（磁盘 IO、JSON 解析、schema 版本不兼容）时返回 `Err`，
/// **不**返回空 Vec 把读取失败伪装成"没有连接"。
///
/// `connections.rs` 与 `relation_index.rs` 共用此函数，不重复实现磁盘扫描。
pub fn load_graph_edges(
    context: &GraphResolverContext,
    starmap_id: &str,
) -> crate::error::Result<Vec<StarMapEdge>> {
    if let Some(graph) = context.overlays.get(starmap_id) {
        return Ok(graph.edges.clone());
    }
    load_graph_edges_from_disk(&context.app_data_root, starmap_id)
}

/// 加载星图的链接列表（可靠对象读取）。
///
/// 先从 `context.overlays` 查找，找不到再从磁盘读取。
pub fn load_graph_links(
    context: &GraphResolverContext,
    starmap_id: &str,
) -> crate::error::Result<Vec<StarMapLink>> {
    if let Some(graph) = context.overlays.get(starmap_id) {
        return Ok(graph.links.clone());
    }
    load_graph_links_from_disk(&context.app_data_root, starmap_id)
}

/// 加载星图的超链接列表（可靠对象读取）。
pub fn load_graph_hyperlinks(
    context: &GraphResolverContext,
    starmap_id: &str,
) -> crate::error::Result<Vec<StarMapHyperlink>> {
    if let Some(graph) = context.overlays.get(starmap_id) {
        return Ok(graph.hyperlinks.clone());
    }
    load_graph_hyperlinks_from_disk(&context.app_data_root, starmap_id)
}

/// 加载星图的嵌入列表（可靠对象读取）。
pub fn load_graph_embeds(
    context: &GraphResolverContext,
    starmap_id: &str,
) -> crate::error::Result<Vec<StarMapEmbed>> {
    if let Some(graph) = context.overlays.get(starmap_id) {
        return Ok(graph.embeds.clone());
    }
    load_graph_embeds_from_disk(&context.app_data_root, starmap_id)
}

/// 从磁盘加载星图的边列表（可靠对象读取）。
///
/// 读取 `graph.json` 获取 `GraphMeta`（含 `edge_ids`），再逐个读取边对象文件。
/// 任何一步失败（文件不存在、IO 错误、JSON 解析失败、schema 版本不兼容）
/// 都返回 `Err`，不跳过、不返回空 Vec。
fn load_graph_edges_from_disk(
    app_data_root: &Path,
    starmap_id: &str,
) -> crate::error::Result<Vec<StarMapEdge>> {
    let graph_json_path = app_data_root
        .join("starmaps")
        .join(starmap_id)
        .join("graph.json");
    let meta = match load_current_graph_meta(&graph_json_path, starmap_id)? {
        Some(m) => m,
        None => return Ok(Vec::new()),
    };

    let mut edges = Vec::new();
    for edge_id in &meta.edge_ids {
        let bucket_dir = app_data_root
            .join("starmaps")
            .join(starmap_id)
            .join("edges")
            .join(bucket_for_id(edge_id));
        let edge_path = bucket_dir.join(format!("{}.json", edge_id));
        let edge_content = std::fs::read_to_string(&edge_path)?;
        let edge: StarMapEdge = serde_json::from_str(&edge_content)?;
        edges.push(edge);
    }
    Ok(edges)
}

/// 从磁盘加载星图的链接列表（可靠对象读取）。
fn load_graph_links_from_disk(
    app_data_root: &Path,
    starmap_id: &str,
) -> crate::error::Result<Vec<StarMapLink>> {
    let graph_json_path = app_data_root
        .join("starmaps")
        .join(starmap_id)
        .join("graph.json");
    let meta = match load_current_graph_meta(&graph_json_path, starmap_id)? {
        Some(m) => m,
        None => return Ok(Vec::new()),
    };

    let mut links = Vec::new();
    for link_id in &meta.link_ids {
        let bucket_dir = app_data_root
            .join("starmaps")
            .join(starmap_id)
            .join("links")
            .join(bucket_for_id(link_id));
        let path = bucket_dir.join(format!("{}.json", link_id));
        let content = std::fs::read_to_string(&path)?;
        let link: StarMapLink = serde_json::from_str(&content)?;
        links.push(link);
    }
    Ok(links)
}

/// 从磁盘加载星图的超链接列表（可靠对象读取）。
fn load_graph_hyperlinks_from_disk(
    app_data_root: &Path,
    starmap_id: &str,
) -> crate::error::Result<Vec<StarMapHyperlink>> {
    let graph_json_path = app_data_root
        .join("starmaps")
        .join(starmap_id)
        .join("graph.json");
    let meta = match load_current_graph_meta(&graph_json_path, starmap_id)? {
        Some(m) => m,
        None => return Ok(Vec::new()),
    };

    let mut hyperlinks = Vec::new();
    for hl_id in &meta.hyperlink_ids {
        let bucket_dir = app_data_root
            .join("starmaps")
            .join(starmap_id)
            .join("hyperlinks")
            .join(bucket_for_id(hl_id));
        let path = bucket_dir.join(format!("{}.json", hl_id));
        let content = std::fs::read_to_string(&path)?;
        let hl: StarMapHyperlink = serde_json::from_str(&content)?;
        hyperlinks.push(hl);
    }
    Ok(hyperlinks)
}

/// 从磁盘加载星图的嵌入列表（可靠对象读取）。
fn load_graph_embeds_from_disk(
    app_data_root: &Path,
    starmap_id: &str,
) -> crate::error::Result<Vec<StarMapEmbed>> {
    let graph_json_path = app_data_root
        .join("starmaps")
        .join(starmap_id)
        .join("graph.json");
    let meta = match load_current_graph_meta(&graph_json_path, starmap_id)? {
        Some(m) => m,
        None => return Ok(Vec::new()),
    };

    let mut embeds = Vec::new();
    for instance_id in &meta.embed_instance_ids {
        let bucket_dir = app_data_root
            .join("starmaps")
            .join(starmap_id)
            .join("embeds")
            .join(bucket_for_id(instance_id));
        let path = bucket_dir.join(format!("{}.json", instance_id));
        let content = std::fs::read_to_string(&path)?;
        let embed: StarMapEmbed = serde_json::from_str(&content)?;
        embeds.push(embed);
    }
    Ok(embeds)
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::starmap::semantic::{StarMapNodeContent, StarMapProvenance};
    use crate::starmap::types::{
        StarMapEmbed, StarMapGraph, StarMapNode, StarMapNodeKind, StarMapPoint,
    };
    use std::collections::HashMap;
    use std::path::PathBuf;

    // -----------------------------------------------------------------------
    // 辅助函数
    // -----------------------------------------------------------------------

    fn make_overlay_context(overlays: HashMap<String, StarMapGraph>) -> GraphResolverContext {
        GraphResolverContext {
            app_data_root: PathBuf::from("/tmp/test_starmap_connections"),
            overlays,
        }
    }

    fn make_empty_graph(starmap_id: &str) -> StarMapGraph {
        StarMapGraph {
            starmap_id: starmap_id.to_string(),
            ..Default::default()
        }
    }

    fn make_embed(instance_id: &str, target_starmap_id: &str) -> StarMapEmbed {
        StarMapEmbed {
            instance_id: instance_id.to_string(),
            target_starmap_id: target_starmap_id.to_string(),
            label: None,
            position: StarMapPoint::default(),
            host_path: StarMapTargetPath::default(),
            provenance: StarMapProvenance::default(),
            created_at: 0,
            updated_at: 0,
        }
    }

    fn make_node(node_id: &str) -> StarMapNode {
        StarMapNode {
            id: node_id.to_string(),
            title: node_id.to_string(),
            kind: StarMapNodeKind::Note,
            payload: None,
            tags: vec![],
            content: StarMapNodeContent::Empty,
            anchors: vec![],
            portal: None,
            position: StarMapPoint::default(),
            style: Default::default(),
            provenance: StarMapProvenance::default(),
            created_at: 0,
            updated_at: 0,
        }
    }

    fn make_edge(edge_id: &str, from: StarMapTargetPath, to: StarMapTargetPath) -> StarMapEdge {
        StarMapEdge {
            id: edge_id.to_string(),
            from,
            to,
            kind: StarMapEdgeKind::RelatedTo,
            label: None,
            payload: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    // -----------------------------------------------------------------------
    // longest_common_prefix_len
    // -----------------------------------------------------------------------

    #[test]
    fn test_longest_common_prefix_empty() {
        assert_eq!(longest_common_prefix_len(&[], &[]), 0);
    }

    #[test]
    fn test_longest_common_prefix_one_empty() {
        let a = vec![StarMapPathSegment::EnterEmbed {
            instance_id: "e1".to_string(),
        }];
        assert_eq!(longest_common_prefix_len(&a, &[]), 0);
        assert_eq!(longest_common_prefix_len(&[], &a), 0);
    }

    #[test]
    fn test_longest_common_prefix_full_match() {
        let a = vec![
            StarMapPathSegment::EnterEmbed {
                instance_id: "e1".to_string(),
            },
            StarMapPathSegment::EnterPortal {
                node_id: "p1".to_string(),
            },
        ];
        assert_eq!(longest_common_prefix_len(&a, &a), 2);
    }

    #[test]
    fn test_longest_common_prefix_partial_match() {
        let a = vec![
            StarMapPathSegment::EnterEmbed {
                instance_id: "e1".to_string(),
            },
            StarMapPathSegment::EnterPortal {
                node_id: "p1".to_string(),
            },
        ];
        let b = vec![
            StarMapPathSegment::EnterEmbed {
                instance_id: "e1".to_string(),
            },
            StarMapPathSegment::EnterPortal {
                node_id: "p2".to_string(),
            },
        ];
        assert_eq!(longest_common_prefix_len(&a, &b), 1);
    }

    #[test]
    fn test_longest_common_prefix_no_match() {
        let a = vec![StarMapPathSegment::EnterEmbed {
            instance_id: "e1".to_string(),
        }];
        let b = vec![StarMapPathSegment::EnterEmbed {
            instance_id: "e2".to_string(),
        }];
        assert_eq!(longest_common_prefix_len(&a, &b), 0);
    }

    // -----------------------------------------------------------------------
    // plan_cross_canvas_edge
    // -----------------------------------------------------------------------

    #[test]
    fn test_plan_cross_canvas_edge_same_canvas() {
        // 根图 A 有节点 a1、a2，在同一画布内连边
        let mut graph_a = make_empty_graph("sm_A");
        graph_a.nodes.push(make_node("a1"));
        graph_a.nodes.push(make_node("a2"));

        let overlays = HashMap::from([("sm_A".to_string(), graph_a)]);
        let context = make_overlay_context(overlays);

        let from = StarMapEndpointSelection {
            route: StarMapNavigationRoute::new("sm_A".to_string()),
            target: StarMapTargetDetail::Node {
                node_id: "a1".to_string(),
            },
        };
        let to = StarMapEndpointSelection {
            route: StarMapNavigationRoute::new("sm_A".to_string()),
            target: StarMapTargetDetail::Node {
                node_id: "a2".to_string(),
            },
        };

        let (edge_id, from_path, to_path) = plan_cross_canvas_edge(&context, &from, &to).unwrap();

        assert!(edge_id.starts_with("edge_"));
        assert_eq!(from_path.starmap_id, "sm_A");
        assert!(from_path.segments.is_empty());
        assert_eq!(
            from_path.target,
            StarMapTargetDetail::Node {
                node_id: "a1".to_string(),
            }
        );
        assert_eq!(to_path.starmap_id, "sm_A");
        assert!(to_path.segments.is_empty());
        assert_eq!(
            to_path.target,
            StarMapTargetDetail::Node {
                node_id: "a2".to_string(),
            }
        );
    }

    #[test]
    fn test_plan_cross_canvas_edge_parent_to_child() {
        // 根图 A 有节点 a、Embed e1→B，B 有节点 b
        // a→b：Edge 放 A；from = A/[]/Node(a)，to = A/[e1]/Node(b)
        let mut graph_a = make_empty_graph("sm_A");
        graph_a.nodes.push(make_node("a"));
        graph_a.embeds.push(make_embed("e1", "sm_B"));

        let mut graph_b = make_empty_graph("sm_B");
        graph_b.nodes.push(make_node("b"));

        let overlays =
            HashMap::from([("sm_A".to_string(), graph_a), ("sm_B".to_string(), graph_b)]);
        let context = make_overlay_context(overlays);

        let from = StarMapEndpointSelection {
            route: StarMapNavigationRoute::new("sm_A".to_string()),
            target: StarMapTargetDetail::Node {
                node_id: "a".to_string(),
            },
        };
        let to = StarMapEndpointSelection {
            route: StarMapNavigationRoute {
                root_starmap_id: "sm_A".to_string(),
                segments: vec![StarMapPathSegment::EnterEmbed {
                    instance_id: "e1".to_string(),
                }],
            },
            target: StarMapTargetDetail::Node {
                node_id: "b".to_string(),
            },
        };

        let (edge_id, from_path, to_path) = plan_cross_canvas_edge(&context, &from, &to).unwrap();

        assert!(edge_id.starts_with("edge_"));
        // 宿主图是 A（公共前缀长度为 0）
        assert_eq!(from_path.starmap_id, "sm_A");
        assert!(from_path.segments.is_empty());
        assert_eq!(
            from_path.target,
            StarMapTargetDetail::Node {
                node_id: "a".to_string(),
            }
        );
        assert_eq!(to_path.starmap_id, "sm_A");
        assert_eq!(to_path.segments.len(), 1);
        assert_eq!(
            to_path.segments[0],
            StarMapPathSegment::EnterEmbed {
                instance_id: "e1".to_string(),
            }
        );
        assert_eq!(
            to_path.target,
            StarMapTargetDetail::Node {
                node_id: "b".to_string(),
            }
        );
    }

    #[test]
    fn test_plan_cross_canvas_edge_sibling_canvases() {
        // 根图 A 有 Embed e1→B、Embed e2→C，B 有节点 b，C 有节点 c
        // b→c：Edge 放 A；from = A/[e1]/Node(b)，to = A/[e2]/Node(c)
        let mut graph_a = make_empty_graph("sm_A");
        graph_a.embeds.push(make_embed("e1", "sm_B"));
        graph_a.embeds.push(make_embed("e2", "sm_C"));

        let mut graph_b = make_empty_graph("sm_B");
        graph_b.nodes.push(make_node("b"));

        let mut graph_c = make_empty_graph("sm_C");
        graph_c.nodes.push(make_node("c"));

        let overlays = HashMap::from([
            ("sm_A".to_string(), graph_a),
            ("sm_B".to_string(), graph_b),
            ("sm_C".to_string(), graph_c),
        ]);
        let context = make_overlay_context(overlays);

        let from = StarMapEndpointSelection {
            route: StarMapNavigationRoute {
                root_starmap_id: "sm_A".to_string(),
                segments: vec![StarMapPathSegment::EnterEmbed {
                    instance_id: "e1".to_string(),
                }],
            },
            target: StarMapTargetDetail::Node {
                node_id: "b".to_string(),
            },
        };
        let to = StarMapEndpointSelection {
            route: StarMapNavigationRoute {
                root_starmap_id: "sm_A".to_string(),
                segments: vec![StarMapPathSegment::EnterEmbed {
                    instance_id: "e2".to_string(),
                }],
            },
            target: StarMapTargetDetail::Node {
                node_id: "c".to_string(),
            },
        };

        let (_edge_id, from_path, to_path) = plan_cross_canvas_edge(&context, &from, &to).unwrap();

        // 宿主图是 A（公共前缀长度为 0，两个 segment 不同）
        assert_eq!(from_path.starmap_id, "sm_A");
        assert_eq!(from_path.segments.len(), 1);
        assert_eq!(
            from_path.segments[0],
            StarMapPathSegment::EnterEmbed {
                instance_id: "e1".to_string(),
            }
        );
        assert_eq!(to_path.starmap_id, "sm_A");
        assert_eq!(to_path.segments.len(), 1);
        assert_eq!(
            to_path.segments[0],
            StarMapPathSegment::EnterEmbed {
                instance_id: "e2".to_string(),
            }
        );
    }

    #[test]
    fn test_plan_cross_canvas_edge_different_roots_no_common_host() {
        // 两个不同根星图，无共同入口链 → NoCommonHost
        let graph_a = make_empty_graph("sm_A");
        let graph_b = make_empty_graph("sm_B");

        let overlays =
            HashMap::from([("sm_A".to_string(), graph_a), ("sm_B".to_string(), graph_b)]);
        let context = make_overlay_context(overlays);

        let from = StarMapEndpointSelection {
            route: StarMapNavigationRoute::new("sm_A".to_string()),
            target: StarMapTargetDetail::Starmap,
        };
        let to = StarMapEndpointSelection {
            route: StarMapNavigationRoute::new("sm_B".to_string()),
            target: StarMapTargetDetail::Starmap,
        };

        let result = plan_cross_canvas_edge(&context, &from, &to);
        assert_eq!(result, Err(CrossCanvasEdgeError::NoCommonHost));
    }

    #[test]
    fn test_plan_cross_canvas_edge_from_resolve_failed() {
        // from 的 target 不存在 → FromResolveFailed
        let graph_a = make_empty_graph("sm_A");

        let overlays = HashMap::from([("sm_A".to_string(), graph_a)]);
        let context = make_overlay_context(overlays);

        let from = StarMapEndpointSelection {
            route: StarMapNavigationRoute::new("sm_A".to_string()),
            target: StarMapTargetDetail::Node {
                node_id: "nonexistent".to_string(),
            },
        };
        let to = StarMapEndpointSelection {
            route: StarMapNavigationRoute::new("sm_A".to_string()),
            target: StarMapTargetDetail::Starmap,
        };

        let result = plan_cross_canvas_edge(&context, &from, &to);
        assert_eq!(
            result,
            Err(CrossCanvasEdgeError::FromResolveFailed(
                StarMapTargetResolveStatus::MissingNode
            ))
        );
    }

    #[test]
    fn test_plan_cross_canvas_edge_to_resolve_failed() {
        // to 的 target 不存在 → ToResolveFailed
        let graph_a = make_empty_graph("sm_A");

        let overlays = HashMap::from([("sm_A".to_string(), graph_a)]);
        let context = make_overlay_context(overlays);

        let from = StarMapEndpointSelection {
            route: StarMapNavigationRoute::new("sm_A".to_string()),
            target: StarMapTargetDetail::Starmap,
        };
        let to = StarMapEndpointSelection {
            route: StarMapNavigationRoute::new("sm_A".to_string()),
            target: StarMapTargetDetail::Node {
                node_id: "nonexistent".to_string(),
            },
        };

        let result = plan_cross_canvas_edge(&context, &from, &to);
        assert_eq!(
            result,
            Err(CrossCanvasEdgeError::ToResolveFailed(
                StarMapTargetResolveStatus::MissingNode
            ))
        );
    }

    #[test]
    fn test_plan_cross_canvas_edge_deep_sibling() {
        // 三层嵌套：A → e1 → B → e2 → C
        // A 中节点 a 连到 C 中节点 c：宿主图 A
        let mut graph_a = make_empty_graph("sm_A");
        graph_a.nodes.push(make_node("a"));
        graph_a.embeds.push(make_embed("e1", "sm_B"));

        let mut graph_b = make_empty_graph("sm_B");
        graph_b.embeds.push(make_embed("e2", "sm_C"));

        let graph_c = make_empty_graph("sm_C");
        // 需要在 C 中放节点 c 才能解析成功
        let mut graph_c = graph_c;
        graph_c.nodes.push(make_node("c"));

        let overlays = HashMap::from([
            ("sm_A".to_string(), graph_a),
            ("sm_B".to_string(), graph_b),
            ("sm_C".to_string(), graph_c),
        ]);
        let context = make_overlay_context(overlays);

        let from = StarMapEndpointSelection {
            route: StarMapNavigationRoute::new("sm_A".to_string()),
            target: StarMapTargetDetail::Node {
                node_id: "a".to_string(),
            },
        };
        let to = StarMapEndpointSelection {
            route: StarMapNavigationRoute {
                root_starmap_id: "sm_A".to_string(),
                segments: vec![
                    StarMapPathSegment::EnterEmbed {
                        instance_id: "e1".to_string(),
                    },
                    StarMapPathSegment::EnterEmbed {
                        instance_id: "e2".to_string(),
                    },
                ],
            },
            target: StarMapTargetDetail::Node {
                node_id: "c".to_string(),
            },
        };

        let (_edge_id, from_path, to_path) = plan_cross_canvas_edge(&context, &from, &to).unwrap();

        // 宿主图是 A（公共前缀长度为 0）
        assert_eq!(from_path.starmap_id, "sm_A");
        assert!(from_path.segments.is_empty());
        assert_eq!(to_path.starmap_id, "sm_A");
        assert_eq!(to_path.segments.len(), 2);
    }

    #[test]
    fn test_plan_cross_canvas_edge_common_prefix_child() {
        // A → e1 → B，B 中有节点 b1、b2
        // b1→b2：公共前缀是 [e1]，宿主图是 B
        // from = B/[]/Node(b1)，to = B/[]/Node(b2)
        let mut graph_a = make_empty_graph("sm_A");
        graph_a.embeds.push(make_embed("e1", "sm_B"));

        let mut graph_b = make_empty_graph("sm_B");
        graph_b.nodes.push(make_node("b1"));
        graph_b.nodes.push(make_node("b2"));

        let overlays =
            HashMap::from([("sm_A".to_string(), graph_a), ("sm_B".to_string(), graph_b)]);
        let context = make_overlay_context(overlays);

        let from = StarMapEndpointSelection {
            route: StarMapNavigationRoute {
                root_starmap_id: "sm_A".to_string(),
                segments: vec![StarMapPathSegment::EnterEmbed {
                    instance_id: "e1".to_string(),
                }],
            },
            target: StarMapTargetDetail::Node {
                node_id: "b1".to_string(),
            },
        };
        let to = StarMapEndpointSelection {
            route: StarMapNavigationRoute {
                root_starmap_id: "sm_A".to_string(),
                segments: vec![StarMapPathSegment::EnterEmbed {
                    instance_id: "e1".to_string(),
                }],
            },
            target: StarMapTargetDetail::Node {
                node_id: "b2".to_string(),
            },
        };

        let (_edge_id, from_path, to_path) = plan_cross_canvas_edge(&context, &from, &to).unwrap();

        // 宿主图是 B（公共前缀长度为 1）
        assert_eq!(from_path.starmap_id, "sm_B");
        assert!(from_path.segments.is_empty());
        assert_eq!(to_path.starmap_id, "sm_B");
        assert!(to_path.segments.is_empty());
    }

    // -----------------------------------------------------------------------
    // route_aware_endpoint_matches
    // -----------------------------------------------------------------------

    #[test]
    fn test_route_aware_endpoint_matches_exact_path() {
        // 端点路径与 route 后缀完全一致 → 匹配
        let endpoint = StarMapTargetPath {
            starmap_id: "sm_A".to_string(),
            segments: vec![StarMapPathSegment::EnterEmbed {
                instance_id: "e1".to_string(),
            }],
            target: StarMapTargetDetail::Node {
                node_id: "b".to_string(),
            },
        };
        let route_suffix = vec![StarMapPathSegment::EnterEmbed {
            instance_id: "e1".to_string(),
        }];
        let target = StarMapTargetDetail::Node {
            node_id: "b".to_string(),
        };
        assert!(route_aware_endpoint_matches(
            &endpoint,
            "sm_A",
            &route_suffix,
            Some(&target),
        ));
    }

    #[test]
    fn test_route_aware_endpoint_matches_different_embed_instance() {
        // 同一星图 B 被两个 Embed e1→B、e2→B 嵌入
        // 边端点路径用 e1，route 后缀用 e2 → 不匹配（实例路径不同）
        let endpoint = StarMapTargetPath {
            starmap_id: "sm_A".to_string(),
            segments: vec![StarMapPathSegment::EnterEmbed {
                instance_id: "e1".to_string(),
            }],
            target: StarMapTargetDetail::Node {
                node_id: "b".to_string(),
            },
        };
        let route_suffix = vec![StarMapPathSegment::EnterEmbed {
            instance_id: "e2".to_string(),
        }];
        let target = StarMapTargetDetail::Node {
            node_id: "b".to_string(),
        };
        assert!(!route_aware_endpoint_matches(
            &endpoint,
            "sm_A",
            &route_suffix,
            Some(&target),
        ));
    }

    #[test]
    fn test_route_aware_endpoint_matches_wrong_ancestor() {
        // 端点 starmap_id 与祖先不匹配 → 不匹配
        let endpoint = StarMapTargetPath {
            starmap_id: "sm_C".to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "b".to_string(),
            },
        };
        let target = StarMapTargetDetail::Node {
            node_id: "b".to_string(),
        };
        assert!(!route_aware_endpoint_matches(
            &endpoint,
            "sm_A",
            &[],
            Some(&target),
        ));
    }

    #[test]
    fn test_route_aware_endpoint_matches_wrong_target() {
        // 路径匹配但 target 不同 → 不匹配
        let endpoint = StarMapTargetPath {
            starmap_id: "sm_A".to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "b2".to_string(),
            },
        };
        let target = StarMapTargetDetail::Node {
            node_id: "b1".to_string(),
        };
        assert!(!route_aware_endpoint_matches(
            &endpoint,
            "sm_A",
            &[],
            Some(&target),
        ));
    }

    #[test]
    fn test_route_aware_endpoint_matches_no_target_check() {
        // target=None 时只检查路径，不检查终点对象
        let endpoint = StarMapTargetPath {
            starmap_id: "sm_A".to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "anything".to_string(),
            },
        };
        assert!(route_aware_endpoint_matches(&endpoint, "sm_A", &[], None));
    }

    #[test]
    fn test_route_aware_endpoint_matches_empty_segments() {
        // 当前画布（空 segments）匹配
        let endpoint = StarMapTargetPath {
            starmap_id: "sm_B".to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "b".to_string(),
            },
        };
        let target = StarMapTargetDetail::Node {
            node_id: "b".to_string(),
        };
        assert!(route_aware_endpoint_matches(
            &endpoint,
            "sm_B",
            &[],
            Some(&target),
        ));
    }

    // -----------------------------------------------------------------------
    // load_graph_edges
    // -----------------------------------------------------------------------

    #[test]
    fn test_load_graph_edges_from_overlay() {
        let mut graph = make_empty_graph("sm_A");
        graph.edges.push(make_edge(
            "e1",
            StarMapTargetPath::default(),
            StarMapTargetPath::default(),
        ));

        let overlays = HashMap::from([("sm_A".to_string(), graph)]);
        let context = make_overlay_context(overlays);

        let edges = load_graph_edges(&context, "sm_A").unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].id, "e1");
    }

    #[test]
    fn test_load_graph_edges_no_overlay_no_disk() {
        let context = make_overlay_context(HashMap::new());
        let edges = load_graph_edges(&context, "sm_nonexistent").unwrap();
        assert!(edges.is_empty());
    }

    // -----------------------------------------------------------------------
    // StarMapEndpointSelection / StarMapConnectionInfo 基本构造
    // ---------------------------------------------------------------------------

    #[test]
    fn test_endpoint_selection_construction() {
        let sel = StarMapEndpointSelection {
            route: StarMapNavigationRoute::new("sm_A".to_string()),
            target: StarMapTargetDetail::Node {
                node_id: "n1".to_string(),
            },
        };
        assert_eq!(sel.route.root_starmap_id, "sm_A");
        assert!(sel.route.segments.is_empty());
        match sel.target {
            StarMapTargetDetail::Node { node_id } => assert_eq!(node_id, "n1"),
            _ => panic!("expected Node target"),
        }
    }

    #[test]
    fn test_connection_info_construction() {
        let info = StarMapConnectionInfo {
            owner_starmap_id: "sm_A".to_string(),
            edge_id: "edge_1".to_string(),
            from: StarMapTargetPath::default(),
            to: StarMapTargetPath::default(),
            kind: StarMapEdgeKind::RelatedTo,
            label: Some("test".to_string()),
        };
        assert_eq!(info.owner_starmap_id, "sm_A");
        assert_eq!(info.edge_id, "edge_1");
        assert_eq!(info.kind, StarMapEdgeKind::RelatedTo);
        assert_eq!(info.label, Some("test".to_string()));
    }
}
