use serde::{Deserialize, Serialize};

use crate::starmap::types::reference::StarMapTargetPath;

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
    pub source_node_id: String,
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
