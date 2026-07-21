/// 解析深目标（DeepTarget）的可达性。
///
/// ## 算法
///
/// 1. **深度限制**：`path.len() > 32` 返回 `TooDeep`。此上限防止恶意或错误数据
///    导致无限递归，32 层远超实际使用深度（通常 0-3 层）。
/// 2. **循环检测**：沿 `path` 逐段遍历，用 `HashSet` 记录已访问的 `starmap_id`，
///    重复进入同一星图即返回 `CycleDetected`。
/// 3. **存在性校验**：每层 `EnterChild` 的 `starmap_id` 必须在磁盘上存在。
/// 4. **终节点校验**：路径末端的 `StarMapTargetDetail`（Node/Anchor/ChapterRange）
///    在目标星图的 `graph.json` 中验证存在性和范围合法性。
///
/// ## 性能注意
///
/// 此函数在 `validation::validate_graph` 中对每个 deep_target 调用，
/// 涉及磁盘 I/O（`load_starmap_meta`、`read_to_string`）。
/// 对于大量 deep_target 的图，验证可能较慢。
pub fn resolve_deep_target(
    workspace: &std::path::Path,
    dt: &crate::starmap::semantic::StarMapDeepTarget,
) -> crate::starmap::semantic::StarMapTargetResolveStatus {
    use crate::starmap::semantic::StarMapTargetResolveStatus::*;

    if dt.path.len() > 32 {
        return TooDeep;
    }

    if crate::starmap::load_starmap_meta(workspace, &dt.starmap_id).is_err() {
        return MissingStarmap;
    }

    let mut current_starmap_id = dt.starmap_id.clone();
    let mut visited = std::collections::HashSet::new();
    visited.insert(current_starmap_id.clone());

    for segment in &dt.path {
        match segment {
            crate::starmap::semantic::StarMapPathSegment::EnterChild { starmap_id } => {
                current_starmap_id = starmap_id.clone();
                if !visited.insert(current_starmap_id.clone()) {
                    return CycleDetected;
                }
                if crate::starmap::load_starmap_meta(workspace, &current_starmap_id).is_err() {
                    return MissingStarmap;
                }
            }
        }
    }

    match &dt.target {
        crate::starmap::semantic::StarMapTargetDetail::Node { node_id } => {
            let target_graph_path =
                crate::starmap::starmap_graph_path(workspace, &current_starmap_id);
            if target_graph_path.exists() {
                if let Ok(json_str) = std::fs::read_to_string(&target_graph_path) {
                    if let Ok(target_graph) =
                        serde_json::from_str::<crate::starmap::types::StarMapGraph>(&json_str)
                    {
                        if !target_graph.nodes.iter().any(|n| &n.id == node_id) {
                            return MissingNode;
                        }
                    }
                }
            } else {
                return MissingNode;
            }
        }
        crate::starmap::semantic::StarMapTargetDetail::Anchor { node_id, anchor_id } => {
            let target_graph_path =
                crate::starmap::starmap_graph_path(workspace, &current_starmap_id);
            if target_graph_path.exists() {
                if let Ok(json_str) = std::fs::read_to_string(&target_graph_path) {
                    if let Ok(target_graph) =
                        serde_json::from_str::<crate::starmap::types::StarMapGraph>(&json_str)
                    {
                        if let Some(n) = target_graph.nodes.iter().find(|n| &n.id == node_id) {
                            if !n.anchors.iter().any(|a| &a.anchor_id == anchor_id) {
                                return MissingAnchor;
                            }
                        } else {
                            return MissingNode;
                        }
                    }
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
