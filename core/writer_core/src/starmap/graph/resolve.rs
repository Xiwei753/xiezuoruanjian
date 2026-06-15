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