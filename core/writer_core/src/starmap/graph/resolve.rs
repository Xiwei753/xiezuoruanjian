use crate::starmap::semantic::StarMapTargetDetail;
use crate::starmap::types::reference::{StarMapPathSegment, StarMapTargetPath};
use crate::starmap::types::StarMapGraph;

/// Resolver 上下文：统一管理 overlay graphs 和磁盘路径。
///
/// `overlays` 按 `starmap_id` 索引，包含所有需要优先于磁盘读取的内存图：
/// - 当前正在校验的 candidate graph
/// - 所有已加载/dirty 的 Store 的 `to_starmap_graph()` 快照
///
/// 引用扫描等已 flush 的场景传空 overlays（从磁盘读取即可）。
#[derive(Debug, Clone)]
pub struct GraphResolverContext {
    pub app_data_root: std::path::PathBuf,
    pub overlays: std::collections::HashMap<String, StarMapGraph>,
}

impl GraphResolverContext {
    /// 创建一个只有 app_data_root、没有 overlays 的 context。
    /// 用于引用扫描等已 flush 的场景。
    pub fn new_disk_only(app_data_root: &std::path::Path) -> Self {
        Self {
            app_data_root: app_data_root.to_path_buf(),
            overlays: std::collections::HashMap::new(),
        }
    }
}

/// 解析后的目标：resolver 的唯一真实解析结果。
///
/// 由 [`resolve_target`] 返回，包含解析过程中收集的全部事实：
/// - `final_starmap_id`：路径最终落在的星图 ID
/// - `target`：路径终点的具体引用（Node/Anchor/ChapterRange 等）
/// - `traversed_starmap_ids`：实际经过的 starmap_id 列表，**含起点和终点图**，
///   顺序为穿越顺序。删除保护据此判断一张星图是否被某条路径经过。
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTarget {
    pub final_starmap_id: String,
    pub target: StarMapTargetDetail,
    pub traversed_starmap_ids: Vec<String>,
}

/// 从 context.overlays 或磁盘 Store 读取一个 embed 实例。
///
/// 先查 context.overlays（按 starmap_id 匹配），找不到再从磁盘 Store 读取。
fn lookup_embed(
    context: &GraphResolverContext,
    current_starmap_id: &str,
    instance_id: &str,
) -> Result<
    Option<crate::starmap::types::StarMapEmbed>,
    crate::starmap::semantic::StarMapTargetResolveStatus,
> {
    use crate::starmap::semantic::StarMapTargetResolveStatus::*;
    if let Some(g) = context.overlays.get(current_starmap_id) {
        return Ok(g
            .embeds
            .iter()
            .find(|e| e.instance_id == instance_id)
            .cloned());
    }
    let mut store =
        crate::starmap::store::StarMapStore::new(&context.app_data_root, current_starmap_id);
    if store.load_full().is_err() {
        return Err(MissingEmbed);
    }
    Ok(store.get_embed(instance_id).cloned())
}

/// 从 context.overlays 或磁盘 Store 读取一个节点。
fn lookup_node(
    context: &GraphResolverContext,
    current_starmap_id: &str,
    node_id: &str,
) -> Result<
    Option<crate::starmap::types::StarMapNode>,
    crate::starmap::semantic::StarMapTargetResolveStatus,
> {
    use crate::starmap::semantic::StarMapTargetResolveStatus::*;
    if let Some(g) = context.overlays.get(current_starmap_id) {
        return Ok(g.nodes.iter().find(|n| n.id == node_id).cloned());
    }
    let mut store =
        crate::starmap::store::StarMapStore::new(&context.app_data_root, current_starmap_id);
    if store.load_full().is_err() {
        return Err(MissingNode);
    }
    Ok(store.get_node(node_id).cloned())
}

/// 检查星图元数据是否存在（先查 context.overlays，找不到再查磁盘）。
pub(crate) fn starmap_exists(context: &GraphResolverContext, starmap_id: &str) -> bool {
    if context.overlays.contains_key(starmap_id) {
        return true;
    }
    crate::starmap::load_starmap_meta(&context.app_data_root, starmap_id).is_ok()
}

/// 解析目标路径的可达性，返回详细解析结果。
///
/// 这是星图路径解析的**唯一真实入口**。所有需要"从某张宿主图开始，经过
/// Embed/Portal，到终点"的引用解析都应调用此函数。
///
/// ## context 参数
///
/// `context` 包含 `app_data_root` 和 `overlays`。`overlays` 按 `starmap_id`
/// 索引，当路径穿越到某个 overlay 对应的星图时，优先从 overlay 读取对象，
/// 而不是从磁盘 Store 读取。这解决了"内存刚改完，resolver 去读旧磁盘"
/// 的一致性问题。多层路径中如果再次走回某个 overlay 对应的星图，也拿 overlay。
/// 引用扫描等已 flush 的场景传空 overlays。
///
/// ## 算法
///
/// 1. **深度限制**：`segments.len() > 32` 返回 `TooDeep`。
/// 2. **起点检查**：`path.starmap_id` 对应的星图必须存在。
/// 3. **逐段穿越**：沿 `segments` 遍历，用 `HashSet` 记录已访问的 `starmap_id`，
///    重复进入同一星图即返回 `CycleDetected`。
/// 4. **终节点校验**：路径末端的 `StarMapTargetDetail` 在最终星图中验证。
pub fn resolve_target(
    context: &GraphResolverContext,
    path: &StarMapTargetPath,
) -> Result<ResolvedTarget, crate::starmap::semantic::StarMapTargetResolveStatus> {
    use crate::starmap::semantic::StarMapTargetResolveStatus::*;

    if path.segments.len() > 32 {
        return Err(TooDeep);
    }

    if !starmap_exists(context, &path.starmap_id) {
        return Err(MissingStarmap);
    }

    let mut current_starmap_id = path.starmap_id.clone();
    let mut traversed_starmap_ids = vec![current_starmap_id.clone()];
    let mut visited = std::collections::HashSet::new();
    visited.insert(current_starmap_id.clone());

    for segment in &path.segments {
        match segment {
            StarMapPathSegment::EnterEmbed { instance_id } => {
                let embed = match lookup_embed(context, &current_starmap_id, instance_id) {
                    Ok(Some(e)) => e,
                    Ok(None) => return Err(MissingEmbed),
                    Err(status) => return Err(status),
                };
                current_starmap_id = embed.target_starmap_id.clone();
                if !visited.insert(current_starmap_id.clone()) {
                    return Err(CycleDetected);
                }
                if !starmap_exists(context, &current_starmap_id) {
                    return Err(MissingStarmap);
                }
                traversed_starmap_ids.push(current_starmap_id.clone());
            }
            StarMapPathSegment::EnterPortal { node_id } => {
                let node = match lookup_node(context, &current_starmap_id, node_id) {
                    Ok(Some(n)) => n,
                    Ok(None) => return Err(MissingPortal),
                    Err(status) => return Err(status),
                };
                let portal = match &node.portal {
                    Some(p) => p,
                    None => return Err(MissingPortal),
                };
                current_starmap_id = portal.destination_starmap_id.clone();
                if !visited.insert(current_starmap_id.clone()) {
                    return Err(CycleDetected);
                }
                if !starmap_exists(context, &current_starmap_id) {
                    return Err(MissingStarmap);
                }
                traversed_starmap_ids.push(current_starmap_id.clone());
            }
        }
    }

    match &path.target {
        StarMapTargetDetail::Node { node_id } => {
            match lookup_node(context, &current_starmap_id, node_id) {
                Ok(Some(_)) => {}
                Ok(None) => return Err(MissingNode),
                Err(status) => return Err(status),
            }
        }
        StarMapTargetDetail::Anchor { node_id, anchor_id } => {
            match lookup_node(context, &current_starmap_id, node_id) {
                Ok(Some(n)) => {
                    if !n.anchors.iter().any(|a| &a.anchor_id == anchor_id) {
                        return Err(MissingAnchor);
                    }
                }
                Ok(None) => return Err(MissingNode),
                Err(status) => return Err(status),
            }
        }
        StarMapTargetDetail::ChapterRange {
            range_start,
            range_end,
            ..
        } => {
            if let (Some(s), Some(e)) = (range_start, range_end) {
                if s > e {
                    return Err(InvalidRange);
                }
            }
        }
        _ => {}
    }

    Ok(ResolvedTarget {
        final_starmap_id: current_starmap_id,
        target: path.target.clone(),
        traversed_starmap_ids,
    })
}

/// 解析目标路径的可达性（兼容包装）。
///
/// 内部调用 [`resolve_target`]，仅返回状态而不返回详细解析结果。
pub fn resolve_target_path(
    context: &GraphResolverContext,
    path: &StarMapTargetPath,
) -> crate::starmap::semantic::StarMapTargetResolveStatus {
    match resolve_target(context, path) {
        Ok(_) => crate::starmap::semantic::StarMapTargetResolveStatus::Resolved,
        Err(status) => status,
    }
}
