use crate::starmap::types::reference::{StarMapPathSegment, StarMapTargetPath};

/// 解析目标路径的可达性。
///
/// ## 算法
///
/// 1. **深度限制**：`segments.len() > 32` 返回 `TooDeep`。此上限防止恶意或错误数据
///    导致无限递归，32 层远超实际使用深度（通常 0-3 层）。
/// 2. **循环检测**：沿 `segments` 逐段遍历，用 `HashSet` 记录已访问的 `starmap_id`，
///    重复进入同一星图即返回 `CycleDetected`。
/// 3. **存在性校验**：每层路径段引用的嵌入实例或 portal 节点必须在当前星图中存在。
/// 4. **终节点校验**：路径末端的 `StarMapTargetDetail`（Node/Anchor/ChapterRange）
///    在目标星图的 `graph.json` 中验证存在性和范围合法性。
///
/// ## 性能注意
///
/// 此函数在 `validation::validate_graph` 中对每个目标路径调用，
/// 涉及磁盘 I/O（`load_starmap_meta`、`read_to_string`）。
/// 对于大量目标路径的图，验证可能较慢。
pub fn resolve_target_path(
    app_data_root: &std::path::Path,
    path: &StarMapTargetPath,
) -> crate::starmap::semantic::StarMapTargetResolveStatus {
    use crate::starmap::semantic::StarMapTargetResolveStatus::*;

    if path.segments.len() > 32 {
        return TooDeep;
    }

    if crate::starmap::load_starmap_meta(app_data_root, &path.starmap_id).is_err() {
        return MissingStarmap;
    }

    let mut current_starmap_id = path.starmap_id.clone();
    let mut visited = std::collections::HashSet::new();
    visited.insert(current_starmap_id.clone());

    for segment in &path.segments {
        match segment {
            StarMapPathSegment::EnterEmbed { instance_id } => {
                let mut store =
                    crate::starmap::store::StarMapStore::new(app_data_root, &current_starmap_id);
                if store.load_full().is_err() {
                    return MissingEmbed;
                }
                let embed = match store.get_embed(instance_id) {
                    Some(e) => e,
                    None => return MissingEmbed,
                };
                current_starmap_id = embed.target_starmap_id.clone();
                if !visited.insert(current_starmap_id.clone()) {
                    return CycleDetected;
                }
                if crate::starmap::load_starmap_meta(app_data_root, &current_starmap_id).is_err() {
                    return MissingStarmap;
                }
            }
            StarMapPathSegment::EnterPortal { node_id } => {
                let mut store =
                    crate::starmap::store::StarMapStore::new(app_data_root, &current_starmap_id);
                if store.load_full().is_err() {
                    return MissingPortal;
                }
                let node = match store.get_node(node_id) {
                    Some(n) => n,
                    None => return MissingPortal,
                };
                let portal = match &node.portal {
                    Some(p) => p,
                    None => return MissingPortal,
                };
                // Portal target 的 starmap_id 是目标星图
                current_starmap_id = portal.target.starmap_id.clone();
                if !visited.insert(current_starmap_id.clone()) {
                    return CycleDetected;
                }
                if crate::starmap::load_starmap_meta(app_data_root, &current_starmap_id).is_err() {
                    return MissingStarmap;
                }
            }
        }
    }

    match &path.target {
        crate::starmap::semantic::StarMapTargetDetail::Node { node_id } => {
            let mut store =
                crate::starmap::store::StarMapStore::new(app_data_root, &current_starmap_id);
            if store.load_full().is_err() {
                return MissingNode;
            }
            if store.get_node(node_id).is_none() {
                return MissingNode;
            }
        }
        crate::starmap::semantic::StarMapTargetDetail::Anchor { node_id, anchor_id } => {
            let mut store =
                crate::starmap::store::StarMapStore::new(app_data_root, &current_starmap_id);
            if store.load_full().is_err() {
                return MissingNode;
            }
            if let Some(n) = store.get_node(node_id) {
                if !n.anchors.iter().any(|a| &a.anchor_id == anchor_id) {
                    return MissingAnchor;
                }
            } else {
                return MissingNode;
            }
        }
        crate::starmap::semantic::StarMapTargetDetail::ChapterRange {
            range_start,
            range_end,
            ..
        } => {
            if let (Some(s), Some(e)) = (range_start, range_end) {
                if s > e {
                    return InvalidRange;
                }
            }
        }
        _ => {}
    }

    Resolved
}
