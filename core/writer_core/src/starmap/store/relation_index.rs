use serde::{Deserialize, Serialize};

use crate::starmap::types::reference::StarMapTargetPath;
use crate::starmap::types::{StarMapEdge, StarMapEmbed, StarMapHyperlink, StarMapLink};

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
