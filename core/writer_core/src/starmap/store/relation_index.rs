use serde::{Deserialize, Serialize};

use crate::starmap::connections::{
    load_graph_edges, load_graph_embeds, load_graph_hyperlinks, load_graph_links,
    route_aware_endpoint_matches,
};
use crate::starmap::graph::resolve::{resolve_status_to_error, GraphResolverContext};
use crate::starmap::navigation::{resolve_navigation_route, StarMapNavigationRoute};
use crate::starmap::semantic::StarMapTargetDetail;
use crate::starmap::types::reference::StarMapTargetPath;
use crate::starmap::types::{
    StarMapEdge, StarMapEdgeKind, StarMapEmbed, StarMapHyperlink, StarMapLink,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeRelationIndex {
    pub edge_id: String,
    pub from: StarMapTargetPath,
    pub to: StarMapTargetPath,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbedHostIndex {
    pub instance_id: String,
    pub host_path: StarMapTargetPath,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkRelationIndex {
    pub link_id: String,
    pub source: StarMapTargetPath,
    pub target: StarMapTargetPath,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperlinkRelationIndex {
    pub hyperlink_id: String,
    pub source_node_id: String,
}

/// 提取路径终点引用的**本地图**节点 ID。
///
/// 只有当 `path.starmap_id == host_starmap_id` 且 `path.segments.is_empty()`
/// 时，路径终点的 Node/Anchor 才是"本地图节点引用"，返回其 `node_id`。
///
/// 跨层路径（`segments` 非空或 `starmap_id` 与宿主图不同）的终点 node_id
/// **绝不**返回——它属于另一张星图，把它当成本地 node_id 会破坏 relation_index
/// 的本地引用不变量，导致 `delete_node` 误删跨层引用、prefetch 错误加载等。
pub(super) fn target_path_node_id<'a>(
    path: &'a StarMapTargetPath,
    host_starmap_id: &str,
) -> Option<&'a str> {
    if path.starmap_id != host_starmap_id || !path.segments.is_empty() {
        return None;
    }
    match &path.target {
        crate::starmap::semantic::StarMapTargetDetail::Node { node_id } => Some(node_id),
        crate::starmap::semantic::StarMapTargetDetail::Anchor { node_id, .. } => Some(node_id),
        _ => None,
    }
}

/// 提取路径**第一段** `EnterPortal` 引用的**本地图**节点 ID。
///
/// 路径起点固定是宿主图。当 `path.starmap_id == host_starmap_id` 且
/// `path.segments` 非空且第一段是 `EnterPortal { node_id }` 时，该
/// `node_id` 属于当前图（即被穿越的 portal 节点），返回它。
///
/// 后续 segment 已经进入别的图，不能拿同名 node_id 当本地图对象。
/// 空 segments 或第一段不是 `EnterPortal` 返回 `None`。
pub(super) fn first_segment_portal_node_id<'a>(
    path: &'a StarMapTargetPath,
    host_starmap_id: &str,
) -> Option<&'a str> {
    if path.starmap_id != host_starmap_id {
        return None;
    }
    match path.segments.first()? {
        crate::starmap::types::reference::StarMapPathSegment::EnterPortal { node_id } => {
            Some(node_id)
        }
        _ => None,
    }
}

/// 提取路径**第一段** `EnterEmbed` 引用的**本地图**嵌入实例 ID。
///
/// 路径起点固定是宿主图。当 `path.starmap_id == host_starmap_id` 且
/// `path.segments` 非空且第一段是 `EnterEmbed { instance_id }` 时，该
/// `instance_id` 属于当前图（即被穿越的 embed 实例），返回它。
///
/// 后续 segment 已经进入别的图，不能拿同名 instance_id 当本地图对象。
/// 空 segments 或第一段不是 `EnterEmbed` 返回 `None`。
pub(super) fn first_segment_embed_instance_id<'a>(
    path: &'a StarMapTargetPath,
    host_starmap_id: &str,
) -> Option<&'a str> {
    if path.starmap_id != host_starmap_id {
        return None;
    }
    match path.segments.first()? {
        crate::starmap::types::reference::StarMapPathSegment::EnterEmbed { instance_id } => {
            Some(instance_id)
        }
        _ => None,
    }
}

/// 提取边关系索引中引用的**本地图**节点 ID。
///
/// `host_starmap_id` 是该边所属星图的 ID。只有 from/to 路径的终点落在
/// `host_starmap_id` 本图（`starmap_id` 匹配且无 segments）时才返回 node_id。
pub(super) fn extract_eri_node_refs<'a>(
    eri: &'a EdgeRelationIndex,
    host_starmap_id: &str,
) -> Vec<&'a str> {
    let mut refs = Vec::new();
    if let Some(id) = target_path_node_id(&eri.from, host_starmap_id) {
        refs.push(id);
    }
    if let Some(id) = target_path_node_id(&eri.to, host_starmap_id) {
        refs.push(id);
    }
    refs
}

/// 提取嵌入宿主索引中引用的**本地图**节点 ID。
///
/// `host_starmap_id` 是该 embed 所属星图的 ID。只有 host_path 终点落在
/// `host_starmap_id` 本图时才返回 node_id。
pub(super) fn extract_ehi_node_refs<'a>(
    ehi: &'a EmbedHostIndex,
    host_starmap_id: &str,
) -> Vec<&'a str> {
    let mut refs = Vec::new();
    if let Some(id) = target_path_node_id(&ehi.host_path, host_starmap_id) {
        refs.push(id);
    }
    refs
}

/// 删除本地 node 或 embed 时需要级联删除的对象 ID 集合。
///
/// 由 `node_cascade_ids` / `embed_cascade_ids` 纯函数计算，
/// store 真实删除和 candidate 模拟删除都复用，避免两套逻辑漂移。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CascadeIds {
    pub edge_ids: Vec<String>,
    pub embed_ids: Vec<String>,
    pub link_ids: Vec<String>,
    pub hyperlink_ids: Vec<String>,
}

/// 计算删除本地 node 时需要级联删除的 edge/embed/link/hyperlink ID 集合。
///
/// 级联规则（对每条路径检查终点引用 OR 第一段 portal 引用）：
/// - edge: from 或 to 命中被删 node → 级联删 edge。
/// - embed: host_path 命中被删 node → 级联删 embed。
/// - link: source 或 target 命中被删 node → 级联删 link。
/// - hyperlink: source 命中被删 node → 级联删 hyperlink。
///
/// "命中"= `target_path_node_id(path, host) == Some(node_id)`（终点引用）
/// OR `first_segment_portal_node_id(path, host) == Some(node_id)`（第一段
/// portal 引用，即被穿越的 portal 节点就是被删 node）。
///
/// 纯函数：store 真实删除和 candidate 模拟删除都复用，保证两者产生相同
/// 的最终对象集合。
pub(super) fn node_cascade_ids<'a>(
    node_id: &str,
    host_starmap_id: &str,
    edges: impl Iterator<Item = &'a StarMapEdge>,
    embeds: impl Iterator<Item = &'a StarMapEmbed>,
    links: impl Iterator<Item = &'a StarMapLink>,
    hyperlinks: impl Iterator<Item = &'a StarMapHyperlink>,
) -> CascadeIds {
    let mut result = CascadeIds::default();
    for e in edges {
        let from_target = target_path_node_id(&e.from, host_starmap_id);
        let to_target = target_path_node_id(&e.to, host_starmap_id);
        let from_first = first_segment_portal_node_id(&e.from, host_starmap_id);
        let to_first = first_segment_portal_node_id(&e.to, host_starmap_id);
        if from_target == Some(node_id)
            || to_target == Some(node_id)
            || from_first == Some(node_id)
            || to_first == Some(node_id)
        {
            result.edge_ids.push(e.id.clone());
        }
    }
    for em in embeds {
        let host_target = target_path_node_id(&em.host_path, host_starmap_id);
        let host_first = first_segment_portal_node_id(&em.host_path, host_starmap_id);
        if host_target == Some(node_id) || host_first == Some(node_id) {
            result.embed_ids.push(em.instance_id.clone());
        }
    }
    for l in links {
        let s_target = target_path_node_id(&l.source, host_starmap_id);
        let t_target = target_path_node_id(&l.target, host_starmap_id);
        let s_first = first_segment_portal_node_id(&l.source, host_starmap_id);
        let t_first = first_segment_portal_node_id(&l.target, host_starmap_id);
        if s_target == Some(node_id)
            || t_target == Some(node_id)
            || s_first == Some(node_id)
            || t_first == Some(node_id)
        {
            result.link_ids.push(l.link_id.clone());
        }
    }
    for hl in hyperlinks {
        let s_target = target_path_node_id(&hl.source, host_starmap_id);
        let s_first = first_segment_portal_node_id(&hl.source, host_starmap_id);
        if s_target == Some(node_id) || s_first == Some(node_id) {
            result.hyperlink_ids.push(hl.hyperlink_id.clone());
        }
    }
    result
}

/// 计算删除本地 embed instance 时需要级联删除的 edge/embed/link/hyperlink ID 集合。
///
/// 级联规则（对每条路径检查第一段 embed 引用）：
/// - edge: from 或 to 第一段 `EnterEmbed` 命中被删 instance → 级联删 edge。
/// - embed: host_path 第一段 `EnterEmbed` 命中被删 instance → 级联删 embed
///   （排除被删 instance 自己，避免自删）。
/// - link: source 或 target 第一段 `EnterEmbed` 命中 → 级联删 link。
/// - hyperlink: source 第一段 `EnterEmbed` 命中 → 级联删 hyperlink。
///
/// "命中"= `first_segment_embed_instance_id(path, host) == Some(instance_id)`。
/// 只看第一段：路径起点固定是宿主图，第一段对象才属于当前图；后续 segment
/// 已经进入别的图。
///
/// 纯函数：store 真实删除和 candidate 模拟删除都复用。
pub(super) fn embed_cascade_ids<'a>(
    instance_id: &str,
    host_starmap_id: &str,
    edges: impl Iterator<Item = &'a StarMapEdge>,
    embeds: impl Iterator<Item = &'a StarMapEmbed>,
    links: impl Iterator<Item = &'a StarMapLink>,
    hyperlinks: impl Iterator<Item = &'a StarMapHyperlink>,
) -> CascadeIds {
    let mut result = CascadeIds::default();
    for e in edges {
        let from_first = first_segment_embed_instance_id(&e.from, host_starmap_id);
        let to_first = first_segment_embed_instance_id(&e.to, host_starmap_id);
        if from_first == Some(instance_id) || to_first == Some(instance_id) {
            result.edge_ids.push(e.id.clone());
        }
    }
    for em in embeds {
        // 排除被删 instance 自己：自引用的 embed 由 validate_graph 拒绝，
        // 但此处仍防御性跳过，避免级联误删自己。
        if em.instance_id == instance_id {
            continue;
        }
        let host_first = first_segment_embed_instance_id(&em.host_path, host_starmap_id);
        if host_first == Some(instance_id) {
            result.embed_ids.push(em.instance_id.clone());
        }
    }
    for l in links {
        let s_first = first_segment_embed_instance_id(&l.source, host_starmap_id);
        let t_first = first_segment_embed_instance_id(&l.target, host_starmap_id);
        if s_first == Some(instance_id) || t_first == Some(instance_id) {
            result.link_ids.push(l.link_id.clone());
        }
    }
    for hl in hyperlinks {
        let s_first = first_segment_embed_instance_id(&hl.source, host_starmap_id);
        if s_first == Some(instance_id) {
            result.hyperlink_ids.push(hl.hyperlink_id.clone());
        }
    }
    result
}

/// 统一级联删除起点：待删除的本地 Node 或 Embed 实例。
///
/// 由 [`cascade_closure_plan`] 接收，计算传递级联闭包——删除该对象后，
/// 因路径失效而连带删除的所有 Edge/Embed/Link/Hyperlink。
#[derive(Debug, Clone)]
pub enum CascadeOrigin {
    /// 删除本地节点 `node_id`。
    Node(String),
    /// 删除本地嵌入实例 `instance_id`。
    Embed(String),
}

/// 统一级联闭包规划：计算删除 Node 或 Embed 后，传递级联会删除的全部对象。
///
/// 现有 [`node_cascade_ids`] / [`embed_cascade_ids`] 只做一次级联计算，
/// 不处理传递级联：删除 node n 会级联删 embed e，但如果 e 的删除又应该
/// 级联删其他 embed/edge（以其为第一段 `EnterEmbed` 的路径），旧实现不会继续扩展。
///
/// 本函数循环扩展直到不动点：对每个被删 Embed，继续计算它的
/// [`embed_cascade_ids`]，把新增的被删 Embed 加入待处理队列，直到没有新增。
/// 用 visited 集合防止无限循环。
///
/// 只有被删 Embed 会引起下一层级联——删除 Edge/Link/Hyperlink 不会让其他
/// 对象的路径失效（它们只是路径的载体，不是路径入口）；删除 Embed 会让
/// 穿越它（第一段 `EnterEmbed`）的路径失效，因此需要继续扩展。
///
/// 输出 [`CascadeIds`] 包含本轮会删除的全部 Edge/Embed/Link/Hyperlink ID
/// （不含 origin 本身）。纯函数：store 真实删除和 candidate 模拟删除都复用，
/// 保证两者产生相同的最终对象集合。
pub(super) fn cascade_closure_plan<'a>(
    origin: &CascadeOrigin,
    host_starmap_id: &str,
    edges: impl IntoIterator<Item = &'a StarMapEdge>,
    embeds: impl IntoIterator<Item = &'a StarMapEmbed>,
    links: impl IntoIterator<Item = &'a StarMapLink>,
    hyperlinks: impl IntoIterator<Item = &'a StarMapHyperlink>,
) -> CascadeIds {
    use std::collections::HashSet;

    // 收集成 Vec 以支持多次迭代（循环扩展需要反复扫描）。
    let edges: Vec<&StarMapEdge> = edges.into_iter().collect();
    let embeds: Vec<&StarMapEmbed> = embeds.into_iter().collect();
    let links: Vec<&StarMapLink> = links.into_iter().collect();
    let hyperlinks: Vec<&StarMapHyperlink> = hyperlinks.into_iter().collect();

    let mut edge_ids: HashSet<String> = HashSet::new();
    let mut embed_ids: HashSet<String> = HashSet::new();
    let mut link_ids: HashSet<String> = HashSet::new();
    let mut hyperlink_ids: HashSet<String> = HashSet::new();

    // 已计算过 embed_cascade_ids 的被删 Embed，防止无限循环。
    let mut visited_embeds: HashSet<String> = HashSet::new();
    // 待处理的被删 Embed 队列。
    let mut pending_embeds: Vec<String> = Vec::new();

    // 合并一次级联结果中的 edge/link/hyperlink，返回该级联产生的 embed_ids
    // （调用方负责 embed_ids 去重和 pending_embeds 入队，避免闭包同时
    // 可变借用 pending_embeds 导致后续 pop 冲突）。
    let mut merge_non_embed = |cascade: CascadeIds| {
        for id in cascade.edge_ids {
            edge_ids.insert(id);
        }
        for id in cascade.link_ids {
            link_ids.insert(id);
        }
        for id in cascade.hyperlink_ids {
            hyperlink_ids.insert(id);
        }
        cascade.embed_ids
    };

    // 第一步：根据 origin 计算直接级联。
    let direct = match origin {
        CascadeOrigin::Node(node_id) => node_cascade_ids(
            node_id,
            host_starmap_id,
            edges.iter().copied(),
            embeds.iter().copied(),
            links.iter().copied(),
            hyperlinks.iter().copied(),
        ),
        CascadeOrigin::Embed(instance_id) => embed_cascade_ids(
            instance_id,
            host_starmap_id,
            edges.iter().copied(),
            embeds.iter().copied(),
            links.iter().copied(),
            hyperlinks.iter().copied(),
        ),
    };
    for id in merge_non_embed(direct) {
        if embed_ids.insert(id.clone()) {
            pending_embeds.push(id);
        }
    }

    // 循环扩展：对每个被删 Embed 继续计算它的级联，直到不动点。
    while let Some(embed_id) = pending_embeds.pop() {
        if !visited_embeds.insert(embed_id.clone()) {
            continue;
        }
        let cascade = embed_cascade_ids(
            &embed_id,
            host_starmap_id,
            edges.iter().copied(),
            embeds.iter().copied(),
            links.iter().copied(),
            hyperlinks.iter().copied(),
        );
        for id in merge_non_embed(cascade) {
            if embed_ids.insert(id.clone()) {
                pending_embeds.push(id);
            }
        }
    }

    CascadeIds {
        edge_ids: edge_ids.into_iter().collect(),
        embed_ids: embed_ids.into_iter().collect(),
        link_ids: link_ids.into_iter().collect(),
        hyperlink_ids: hyperlink_ids.into_iter().collect(),
    }
}

// ---------------------------------------------------------------------------
// 跨画布引用反查
// ---------------------------------------------------------------------------

/// 跨画布引用信息：其他图中持有的、指向当前图目标的 Edge 引用。
///
/// 由 `find_cross_canvas_references` 和 `find_cross_canvas_references_for_canvas`
/// 返回，用于删除保护——在删除节点/Embed/Portal 之前，检查是否有其他图
/// 持有的跨层引用指向当前图的目标对象。
#[derive(Debug, Clone)]
pub struct CrossCanvasReferenceInfo {
    /// Edge 所在的宿主星图 ID。
    pub owner_starmap_id: String,
    pub edge_id: String,
    pub from_path: StarMapTargetPath,
    pub to_path: StarMapTargetPath,
    pub kind: StarMapEdgeKind,
    pub label: Option<String>,
}

/// 查找指向特定目标的跨画布引用。
///
/// 遍历当前 route 的所有祖先图（不含当前图本身）的 Edge，使用 route-aware 匹配
/// 检查每条 Edge 的 from/to 路径是否指向 `(current_starmap_id, target)`。
///
/// **route-aware 匹配**：Edge 端点的 `segments` 必须与从该祖先进入当前画布的
/// 完整实例路径后缀完全一致，确保同一星图的不同 Embed 实例具有可区分的访问上下文。
///
/// **不变量**：`relation_index.rs` 的本地引用不变量保持不变——此函数
/// 不把深层 Node ID 错当作宿主图本地 Node ID。
///
/// 读取失败（磁盘 IO、JSON 解析、schema 版本不兼容）时返回 `Err`，
/// 不把读取失败伪装成"没有引用"。
///
/// 返回的每条 `CrossCanvasReferenceInfo` 包含 Edge 所在的宿主星图 ID、
/// Edge ID、from/to 路径、类型和标签。
pub fn find_cross_canvas_references(
    context: &GraphResolverContext,
    route: &StarMapNavigationRoute,
    target: &StarMapTargetDetail,
) -> crate::error::Result<Vec<CrossCanvasReferenceInfo>> {
    let location = resolve_navigation_route(context, route).map_err(resolve_status_to_error)?;

    let current_starmap_id = &location.current_starmap_id;
    let mut results = Vec::new();

    // 只检查祖先图（不含当前图），当前图的边由本地级联删除处理。
    for (i, ancestor_starmap_id) in location.traversed_starmap_ids.iter().enumerate() {
        if ancestor_starmap_id == current_starmap_id {
            continue;
        }
        let route_from_ancestor = &route.segments[i..];
        let edges = load_graph_edges(context, ancestor_starmap_id)?;

        for edge in &edges {
            let from_matches = route_aware_endpoint_matches(
                &edge.from,
                ancestor_starmap_id,
                route_from_ancestor,
                Some(target),
            );
            let to_matches = route_aware_endpoint_matches(
                &edge.to,
                ancestor_starmap_id,
                route_from_ancestor,
                Some(target),
            );

            if from_matches || to_matches {
                results.push(CrossCanvasReferenceInfo {
                    owner_starmap_id: ancestor_starmap_id.clone(),
                    edge_id: edge.id.clone(),
                    from_path: edge.from.clone(),
                    to_path: edge.to.clone(),
                    kind: edge.kind.clone(),
                    label: edge.label.clone(),
                });
            }
        }
    }

    Ok(results)
}

/// 查找与特定画布相关的跨画布引用。
///
/// 遍历当前图和所有祖先图的 Edge：
/// - 当前图：包含所有边（本图 Edge）
/// - 祖先图：只包含至少一端通过 route-aware 匹配落在当前画布的 Edge
///
/// **route-aware 匹配**：Edge 端点的 `segments` 必须与从该祖先进入当前画布的
/// 完整实例路径后缀完全一致。
///
/// 读取失败时返回 `Err`，不把读取失败伪装成"没有引用"。
#[allow(clippy::excessive_nesting)]
pub fn find_cross_canvas_references_for_canvas(
    context: &GraphResolverContext,
    route: &StarMapNavigationRoute,
) -> crate::error::Result<Vec<CrossCanvasReferenceInfo>> {
    let location = resolve_navigation_route(context, route).map_err(resolve_status_to_error)?;

    let mut results = Vec::new();

    let last_idx = location.traversed_starmap_ids.len().saturating_sub(1);

    for (i, ancestor_starmap_id) in location.traversed_starmap_ids.iter().enumerate() {
        let edges = load_graph_edges(context, ancestor_starmap_id)?;
        let is_current_canvas = i == last_idx;

        for edge in &edges {
            if is_current_canvas {
                // 当前图：包含所有边（本图 Edge）
                results.push(CrossCanvasReferenceInfo {
                    owner_starmap_id: ancestor_starmap_id.clone(),
                    edge_id: edge.id.clone(),
                    from_path: edge.from.clone(),
                    to_path: edge.to.clone(),
                    kind: edge.kind.clone(),
                    label: edge.label.clone(),
                });
            } else {
                // 祖先图：只包含至少一端 route-aware 落在当前画布的边
                let route_from_ancestor = &route.segments[i..];
                let from_in_canvas = route_aware_endpoint_matches(
                    &edge.from,
                    ancestor_starmap_id,
                    route_from_ancestor,
                    None,
                );
                let to_in_canvas = route_aware_endpoint_matches(
                    &edge.to,
                    ancestor_starmap_id,
                    route_from_ancestor,
                    None,
                );

                if from_in_canvas || to_in_canvas {
                    results.push(CrossCanvasReferenceInfo {
                        owner_starmap_id: ancestor_starmap_id.clone(),
                        edge_id: edge.id.clone(),
                        from_path: edge.from.clone(),
                        to_path: edge.to.clone(),
                        kind: edge.kind.clone(),
                        label: edge.label.clone(),
                    });
                }
            }
        }
    }

    Ok(results)
}

// ---------------------------------------------------------------------------
// 统一全图引用扫描 / 反向依赖索引
// ---------------------------------------------------------------------------

/// 跨画布引用的来源对象类型。
///
/// 标识引用来自哪种对象（Edge/Link/Hyperlink/Embed）的哪个端点，
/// 用于删除保护时定位具体的外部依赖。
#[derive(Debug, Clone, PartialEq)]
pub enum CrossCanvasRefSourceKind {
    /// Edge 的 from 端。
    EdgeFrom,
    /// Edge 的 to 端。
    EdgeTo,
    /// Link 的 source 端。
    LinkSource,
    /// Link 的 target 端。
    LinkTarget,
    /// Hyperlink 的 source 端。
    HyperlinkSource,
    /// Embed 的 host_path。
    EmbedHostPath,
}

/// 全类型跨画布引用信息：其他图中持有的、指向当前图目标的引用。
///
/// 由 [`find_all_cross_canvas_references`] 返回，收录 Edge 两端、Link 两端、
/// Hyperlink source、Embed host_path 的路径引用，用于删除 Node（含 Anchor/Portal）、
/// 删除 Embed 以及修改 Portal/Embed 路由前的外部依赖检查。
///
/// 与 [`CrossCanvasReferenceInfo`] 的区别：后者只收录 Edge 引用，
/// 本类型覆盖所有路径引用来源，确保跨图语义完整。
#[derive(Debug, Clone)]
pub struct AllCrossCanvasReferenceInfo {
    /// 引用所在的宿主星图 ID。
    pub owner_starmap_id: String,
    /// 引用来源类型（Edge/Link/Hyperlink/Embed 的哪个端点）。
    pub source_kind: CrossCanvasRefSourceKind,
    /// 引用来源对象 ID（edge_id / link_id / hyperlink_id / instance_id）。
    pub source_id: String,
    /// 被引用的路径。
    pub referenced_path: StarMapTargetPath,
    pub label: Option<String>,
}

/// 查找指向特定目标的**全类型**跨画布引用。
///
/// 遍历当前 route 的所有祖先图（不含当前图本身），扫描每种路径引用来源：
/// - Edge 两端（from/to 路径）
/// - Link 两端（source/target 路径）
/// - Hyperlink source（source 路径）
/// - Embed host_path
///
/// 对每个引用路径，使用 route-aware 匹配检查是否指向
/// `(current_starmap_id, target)`。
///
/// **route-aware 匹配**：引用路径的 `segments` 必须与从该祖先进入当前画布的
/// 完整实例路径后缀完全一致，确保同一星图的不同 Embed 实例具有可区分的访问上下文。
///
/// 读取失败时返回 `Err`，不把读取失败伪装成"没有引用"。
///
/// 返回的每条 `AllCrossCanvasReferenceInfo` 包含宿主星图 ID、来源类型、
/// 来源对象 ID 和被引用的路径，用于删除保护时定位具体的外部依赖。
#[allow(clippy::too_many_lines)]
pub fn find_all_cross_canvas_references(
    context: &GraphResolverContext,
    route: &StarMapNavigationRoute,
    target: &StarMapTargetDetail,
) -> crate::error::Result<Vec<AllCrossCanvasReferenceInfo>> {
    let location = resolve_navigation_route(context, route).map_err(resolve_status_to_error)?;

    let current_starmap_id = &location.current_starmap_id;
    let mut results = Vec::new();

    for (i, ancestor_starmap_id) in location.traversed_starmap_ids.iter().enumerate() {
        if ancestor_starmap_id == current_starmap_id {
            continue;
        }
        let route_from_ancestor = &route.segments[i..];

        // Edge 两端
        let edges = load_graph_edges(context, ancestor_starmap_id)?;
        for edge in &edges {
            if route_aware_endpoint_matches(
                &edge.from,
                ancestor_starmap_id,
                route_from_ancestor,
                Some(target),
            ) {
                results.push(AllCrossCanvasReferenceInfo {
                    owner_starmap_id: ancestor_starmap_id.clone(),
                    source_kind: CrossCanvasRefSourceKind::EdgeFrom,
                    source_id: edge.id.clone(),
                    referenced_path: edge.from.clone(),
                    label: edge.label.clone(),
                });
            }
            if route_aware_endpoint_matches(
                &edge.to,
                ancestor_starmap_id,
                route_from_ancestor,
                Some(target),
            ) {
                results.push(AllCrossCanvasReferenceInfo {
                    owner_starmap_id: ancestor_starmap_id.clone(),
                    source_kind: CrossCanvasRefSourceKind::EdgeTo,
                    source_id: edge.id.clone(),
                    referenced_path: edge.to.clone(),
                    label: edge.label.clone(),
                });
            }
        }

        // Link 两端
        let links = load_graph_links(context, ancestor_starmap_id)?;
        for link in &links {
            if route_aware_endpoint_matches(
                &link.source,
                ancestor_starmap_id,
                route_from_ancestor,
                Some(target),
            ) {
                results.push(AllCrossCanvasReferenceInfo {
                    owner_starmap_id: ancestor_starmap_id.clone(),
                    source_kind: CrossCanvasRefSourceKind::LinkSource,
                    source_id: link.link_id.clone(),
                    referenced_path: link.source.clone(),
                    label: link.label.clone(),
                });
            }
            if route_aware_endpoint_matches(
                &link.target,
                ancestor_starmap_id,
                route_from_ancestor,
                Some(target),
            ) {
                results.push(AllCrossCanvasReferenceInfo {
                    owner_starmap_id: ancestor_starmap_id.clone(),
                    source_kind: CrossCanvasRefSourceKind::LinkTarget,
                    source_id: link.link_id.clone(),
                    referenced_path: link.target.clone(),
                    label: link.label.clone(),
                });
            }
        }

        // Hyperlink source
        let hyperlinks = load_graph_hyperlinks(context, ancestor_starmap_id)?;
        for hl in &hyperlinks {
            if route_aware_endpoint_matches(
                &hl.source,
                ancestor_starmap_id,
                route_from_ancestor,
                Some(target),
            ) {
                results.push(AllCrossCanvasReferenceInfo {
                    owner_starmap_id: ancestor_starmap_id.clone(),
                    source_kind: CrossCanvasRefSourceKind::HyperlinkSource,
                    source_id: hl.hyperlink_id.clone(),
                    referenced_path: hl.source.clone(),
                    label: hl.label.clone(),
                });
            }
        }

        // Embed host_path
        let embeds = load_graph_embeds(context, ancestor_starmap_id)?;
        for embed in &embeds {
            if route_aware_endpoint_matches(
                &embed.host_path,
                ancestor_starmap_id,
                route_from_ancestor,
                Some(target),
            ) {
                results.push(AllCrossCanvasReferenceInfo {
                    owner_starmap_id: ancestor_starmap_id.clone(),
                    source_kind: CrossCanvasRefSourceKind::EmbedHostPath,
                    source_id: embed.instance_id.clone(),
                    referenced_path: embed.host_path.clone(),
                    label: embed.label.clone(),
                });
            }
        }
    }

    Ok(results)
}

// ---------------------------------------------------------------------------
// 反向依赖索引（修改4：派生倒排表）
// ---------------------------------------------------------------------------

/// 路径依赖来源：产生某条依赖事实的具体引用对象。
///
/// 记入 [`PathDependencyIndex`] 的每条 entry 都携带完整来源信息，
/// 删除保护据此返回"具体宿主、引用类型、引用 ID"，而不只是布尔拒绝。
#[derive(Debug, Clone)]
pub struct PathDependencySource {
    /// 引用所在的宿主星图 ID（Edge/Link/Hyperlink/Embed 所属图）。
    pub owner_starmap_id: String,
    /// 引用来源类型（Edge/Link/Hyperlink/Embed 的哪个端点）。
    pub source_kind: CrossCanvasRefSourceKind,
    /// 引用来源对象 ID（edge_id / link_id / hyperlink_id / instance_id）。
    pub source_id: String,
    /// 被引用的路径。
    pub referenced_path: StarMapTargetPath,
    pub label: Option<String>,
    /// 仅当该来源路径终点为 `Anchor` 时有值，用于精确删除单个 anchor 的依赖检查。
    pub target_anchor_id: Option<String>,
}

/// 反向依赖索引：按 `(host_starmap_id, kind, object_id)` 查询依赖该对象的路径来源。
///
/// 这是从星图真实数据派生的可重建查询结构，图对象仍是唯一事实来源。
/// 删除保护（节点/Embed/Portal/Anchor）与关联查询都走这**同一个**查询入口，
/// 不保留 facade/connections/relation_index 三套各自遍历全图的业务扫描。
///
/// 建索引用 [`PathDependencyIndex::build`]，查询用 [`PathDependencyIndex::query_dependents`]。
#[derive(Debug, Clone, Default)]
pub struct PathDependencyIndex {
    entries: std::collections::HashMap<
        (
            String,
            crate::starmap::graph::resolve::PathDependencyKind,
            String,
        ),
        Vec<PathDependencySource>,
    >,
}

impl PathDependencyIndex {
    /// 从所有星图数据重建反向依赖索引。
    ///
    /// 遍历每张星图的 Edge 两端、Link 两端、Hyperlink source、Embed host_path，
    /// 对每条路径调用 [`extract_path_dependency_facts`] 提取全部依赖事实
    /// （途中穿越 + 终点），按 `(host_starmap_id, kind, object_id)` 入倒排表。
    ///
    /// 读取错误（磁盘 IO、JSON 解析、schema 版本不兼容）逐层用 `?` 传播，
    /// 不把读取失败伪装成"无依赖"。
    pub fn build(
        context: &GraphResolverContext,
        starmap_ids: &[String],
    ) -> crate::error::Result<PathDependencyIndex> {
        let mut index = PathDependencyIndex::default();

        for starmap_id in starmap_ids {
            // Edge 两端
            for edge in load_graph_edges(context, starmap_id)? {
                index.add_path(
                    starmap_id,
                    CrossCanvasRefSourceKind::EdgeFrom,
                    &edge.id,
                    &edge.from,
                    &edge.label,
                    context,
                )?;
                index.add_path(
                    starmap_id,
                    CrossCanvasRefSourceKind::EdgeTo,
                    &edge.id,
                    &edge.to,
                    &edge.label,
                    context,
                )?;
            }

            // Link 两端
            for link in load_graph_links(context, starmap_id)? {
                index.add_path(
                    starmap_id,
                    CrossCanvasRefSourceKind::LinkSource,
                    &link.link_id,
                    &link.source,
                    &link.label,
                    context,
                )?;
                index.add_path(
                    starmap_id,
                    CrossCanvasRefSourceKind::LinkTarget,
                    &link.link_id,
                    &link.target,
                    &link.label,
                    context,
                )?;
            }

            // Hyperlink source
            for hl in load_graph_hyperlinks(context, starmap_id)? {
                index.add_path(
                    starmap_id,
                    CrossCanvasRefSourceKind::HyperlinkSource,
                    &hl.hyperlink_id,
                    &hl.source,
                    &hl.label,
                    context,
                )?;
            }

            // Embed host_path
            for embed in load_graph_embeds(context, starmap_id)? {
                index.add_path(
                    starmap_id,
                    CrossCanvasRefSourceKind::EmbedHostPath,
                    &embed.instance_id,
                    &embed.host_path,
                    &embed.label,
                    context,
                )?;
            }
        }

        Ok(index)
    }

    /// 把单条路径的依赖事实加入倒排表。
    fn add_path(
        &mut self,
        owner_starmap_id: &str,
        source_kind: CrossCanvasRefSourceKind,
        source_id: &str,
        path: &StarMapTargetPath,
        label: &Option<String>,
        context: &GraphResolverContext,
    ) -> crate::error::Result<()> {
        let facts = crate::starmap::graph::resolve::extract_path_dependency_facts(context, path)?;
        for fact in facts {
            let key = (
                fact.host_starmap_id.clone(),
                fact.kind.clone(),
                fact.object_id.clone(),
            );
            self.entries
                .entry(key)
                .or_default()
                .push(PathDependencySource {
                    owner_starmap_id: owner_starmap_id.to_string(),
                    source_kind: source_kind.clone(),
                    source_id: source_id.to_string(),
                    referenced_path: path.clone(),
                    label: label.clone(),
                    target_anchor_id: fact.anchor_id.clone(),
                });
        }
        Ok(())
    }

    /// 查询依赖指定对象的所有路径来源。
    ///
    /// 返回的切片借用 `self`。删除节点时同时查 `NodeTarget`、`AnchorTarget`、
    /// `PortalTraversal`；删除 Embed 时查 `EmbedTraversal`；修改 Portal 时查
    /// `PortalTraversal`；删除单个 Anchor 时查 `AnchorTarget` 并按 `target_anchor_id` 过滤。
    pub fn query_dependents(
        &self,
        host_starmap_id: &str,
        kind: &crate::starmap::graph::resolve::PathDependencyKind,
        object_id: &str,
    ) -> &[PathDependencySource] {
        match self.entries.get(&(
            host_starmap_id.to_string(),
            kind.clone(),
            object_id.to_string(),
        )) {
            Some(v) => v.as_slice(),
            None => &[],
        }
    }

    /// 查询依赖指定对象的所有路径来源（[`query_dependents`] 的语义别名）。
    ///
    /// 暴露给 facade 层的持久化索引接口，供删除保护和关联查询使用。
    pub fn dependents_of(
        &self,
        host_starmap_id: &str,
        kind: &crate::starmap::graph::resolve::PathDependencyKind,
        object_id: &str,
    ) -> &[PathDependencySource] {
        self.query_dependents(host_starmap_id, kind, object_id)
    }

    /// 按来源对象 `(owner_starmap_id, source_kind, source_id)` 移除该来源的所有依赖事实。
    ///
    /// 增删改操作后增量更新索引：先移除旧来源产生的全部 entry，再插入新来源
    /// 的 entry（见 [`add_or_replace_source`]）。避免每次从零扫描全库重建索引。
    pub fn remove_source(
        &mut self,
        owner_starmap_id: &str,
        source_kind: &CrossCanvasRefSourceKind,
        source_id: &str,
    ) {
        for sources in self.entries.values_mut() {
            sources.retain(|s| {
                !(s.owner_starmap_id == owner_starmap_id
                    && &s.source_kind == source_kind
                    && s.source_id == source_id)
            });
        }
        // 清理空的 Vec，保持索引紧凑。
        self.entries.retain(|_, v| !v.is_empty());
    }

    /// 先按 `(owner, kind, source_id)` 移除该来源的所有旧依赖事实，再插入新依赖事实。
    ///
    /// 增删改操作后增量更新索引的统一入口：Edge/Link/Hyperlink/Embed 的 from/to/
    /// source/host_path 变更后，先用本方法移除旧路径产生的全部 entry，再插入
    /// 新路径的 entry。避免每次从零扫描全库重建索引。
    ///
    /// 读取错误（`extract_path_dependency_facts` 内部磁盘 IO/schema 不兼容）
    /// 通过 `?` 向上传播，不吞成"无依赖"。
    pub fn add_or_replace_source(
        &mut self,
        owner_starmap_id: &str,
        source_kind: CrossCanvasRefSourceKind,
        source_id: &str,
        path: &StarMapTargetPath,
        label: &Option<String>,
        context: &GraphResolverContext,
    ) -> crate::error::Result<()> {
        self.remove_source(owner_starmap_id, &source_kind, source_id);
        self.add_path(owner_starmap_id, source_kind, source_id, path, label, context)?;
        Ok(())
    }
}
