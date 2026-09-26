use crate::starmap::semantic::StarMapTargetDetail;
use crate::starmap::types::reference::{StarMapPathSegment, StarMapTargetPath};

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

/// 解析目标路径的可达性，返回详细解析结果。
///
/// 这是星图路径解析的**唯一真实入口**。所有需要"从某张宿主图开始，经过
/// Embed/Portal，到终点"的引用解析都应调用此函数。
///
/// ## 算法
///
/// 1. **深度限制**：`segments.len() > 32` 返回 `TooDeep`。此上限防止恶意或错误数据
///    导致无限递归，32 层远超实际使用深度（通常 0-3 层）。
/// 2. **起点检查**：`path.starmap_id` 对应的星图必须存在。
/// 3. **逐段穿越**：沿 `segments` 遍历，用 `HashSet` 记录已访问的 `starmap_id`，
///    重复进入同一星图即返回 `CycleDetected`。
///    - `EnterEmbed`：当前图必须存在该 `instance_id`，再得到下一张图
///      （`embed.target_starmap_id`）。
///    - `EnterPortal`：当前图必须存在 portal 节点，再得到下一张图
///      （`portal.destination_starmap_id`）。
/// 4. **终节点校验**：路径末端的 `StarMapTargetDetail`（Node/Anchor/ChapterRange）
///    在最终星图中验证存在性和范围合法性。
///
/// ## 返回
///
/// - `Ok(ResolvedTarget)`：路径完整可达，含最终星图 ID、终点和经过的星图链。
/// - `Err(StarMapTargetResolveStatus)`：解析失败的具体状态。
///
/// ## 性能注意
///
/// 此函数在 `validation::validate_graph` 中对每个目标路径调用，
/// 涉及磁盘 I/O（`load_starmap_meta`、`read_to_string`）。
/// 对于大量目标路径的图，验证可能较慢。
pub fn resolve_target(
    app_data_root: &std::path::Path,
    path: &StarMapTargetPath,
) -> Result<ResolvedTarget, crate::starmap::semantic::StarMapTargetResolveStatus> {
    use crate::starmap::semantic::StarMapTargetResolveStatus::*;

    if path.segments.len() > 32 {
        return Err(TooDeep);
    }

    if crate::starmap::load_starmap_meta(app_data_root, &path.starmap_id).is_err() {
        return Err(MissingStarmap);
    }

    let mut current_starmap_id = path.starmap_id.clone();
    let mut traversed_starmap_ids = vec![current_starmap_id.clone()];
    let mut visited = std::collections::HashSet::new();
    visited.insert(current_starmap_id.clone());

    for segment in &path.segments {
        match segment {
            StarMapPathSegment::EnterEmbed { instance_id } => {
                let mut store =
                    crate::starmap::store::StarMapStore::new(app_data_root, &current_starmap_id);
                if store.load_full().is_err() {
                    return Err(MissingEmbed);
                }
                let embed = match store.get_embed(instance_id) {
                    Some(e) => e,
                    None => return Err(MissingEmbed),
                };
                current_starmap_id = embed.target_starmap_id.clone();
                if !visited.insert(current_starmap_id.clone()) {
                    return Err(CycleDetected);
                }
                if crate::starmap::load_starmap_meta(app_data_root, &current_starmap_id).is_err() {
                    return Err(MissingStarmap);
                }
                traversed_starmap_ids.push(current_starmap_id.clone());
            }
            StarMapPathSegment::EnterPortal { node_id } => {
                let mut store =
                    crate::starmap::store::StarMapStore::new(app_data_root, &current_starmap_id);
                if store.load_full().is_err() {
                    return Err(MissingPortal);
                }
                let node = match store.get_node(node_id) {
                    Some(n) => n,
                    None => return Err(MissingPortal),
                };
                let portal = match &node.portal {
                    Some(p) => p,
                    None => return Err(MissingPortal),
                };
                // Portal 的 destination_starmap_id 是目标星图
                current_starmap_id = portal.destination_starmap_id.clone();
                if !visited.insert(current_starmap_id.clone()) {
                    return Err(CycleDetected);
                }
                if crate::starmap::load_starmap_meta(app_data_root, &current_starmap_id).is_err() {
                    return Err(MissingStarmap);
                }
                traversed_starmap_ids.push(current_starmap_id.clone());
            }
        }
    }

    match &path.target {
        StarMapTargetDetail::Node { node_id } => {
            let mut store =
                crate::starmap::store::StarMapStore::new(app_data_root, &current_starmap_id);
            if store.load_full().is_err() {
                return Err(MissingNode);
            }
            if store.get_node(node_id).is_none() {
                return Err(MissingNode);
            }
        }
        StarMapTargetDetail::Anchor { node_id, anchor_id } => {
            let mut store =
                crate::starmap::store::StarMapStore::new(app_data_root, &current_starmap_id);
            if store.load_full().is_err() {
                return Err(MissingNode);
            }
            if let Some(n) = store.get_node(node_id) {
                if !n.anchors.iter().any(|a| &a.anchor_id == anchor_id) {
                    return Err(MissingAnchor);
                }
            } else {
                return Err(MissingNode);
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
/// 保留此函数以兼容现有只关心"是否可达"的调用点；需要经过的星图链等
/// 详细信息的调用点应直接使用 [`resolve_target`]。
pub fn resolve_target_path(
    app_data_root: &std::path::Path,
    path: &StarMapTargetPath,
) -> crate::starmap::semantic::StarMapTargetResolveStatus {
    use crate::starmap::semantic::StarMapTargetResolveStatus::*;

    match resolve_target(app_data_root, path) {
        Ok(_) => Resolved,
        Err(status) => status,
    }
}
