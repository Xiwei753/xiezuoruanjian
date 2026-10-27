//! # 星图导航模块
//!
//! 导航路由（`StarMapNavigationRoute`）仅承载**语义位置**——从根星图出发，
//! 经过哪些 Embed 实例或 Portal 节点到达当前画布。它不保存镜头/相机参数，
//! 是导航身份而非持久化到 graph 的相机状态。
//!
//! ## 核心设计
//!
//! - **复用现有 resolver**：`resolve_navigation_route` 构造 target 为
//!   `StarMapTargetDetail::Starmap` 的 `StarMapTargetPath`，调用
//!   `graph/resolve.rs::resolve_target`，不复制一套解析逻辑。
//! - **进入子画布先验证**：`enter_embed` / `enter_portal` 在当前图确认
//!   实例/portal 存在后才追加路径段，再解析整条路径有效性。
//! - **退出是纯路径操作**：`exit_canvas` 弹出最后一个段，回到上一层。
//!   对同一目标星图的多个 Embed，返回的是原进入实例（因为路径段记录了
//!   具体的 `instance_id`，不是靠目标标题推测）。
//! - **跳到祖先**：`jump_to_ancestor` 纯路径截断，不需要 context。
//! - **错误沿用现有类型**：缺失实例 → `MissingEmbed`，缺失 portal → `MissingPortal`。
//!   路径允许重复进入同一星图（A→B→A、A→A），循环是合法内容；每次进入只追加
//!   一个路径段，身份由完整路径和当次入口实例决定。
//!
//! 每一层自己的节点/Embed `position` 仍只属于这一张图的文档坐标。
//! 不存在"父画布无限放大后把子画布全部铺进去"的坐标链。

use crate::starmap::graph::resolve::{resolve_target, GraphResolverContext, ResolvedTarget};
use crate::starmap::semantic::{StarMapTargetDetail, StarMapTargetResolveStatus};
use crate::starmap::types::reference::{StarMapPathSegment, StarMapTargetPath};

/// 导航路由：从根星图到当前画布的语义路径。
///
/// 仅承载语义位置（经过哪些 Embed 实例 / Portal 节点），
/// 不保存镜头/相机参数。`segments` 复用现有 `StarMapPathSegment`。
///
/// - `root_starmap_id`：导航起点（根星图 ID）
/// - `segments`：路径段列表，每段是一次层级穿越
#[derive(Debug, Clone, PartialEq)]
pub struct StarMapNavigationRoute {
    pub root_starmap_id: String,
    pub segments: Vec<StarMapPathSegment>,
}

impl StarMapNavigationRoute {
    /// 构造空路径（在根图上，没有任何穿越段）。
    pub fn new(root_starmap_id: String) -> Self {
        Self {
            root_starmap_id,
            segments: Vec::new(),
        }
    }

    /// 转换为 `StarMapTargetPath`，target 设为 `StarMapTargetDetail::Starmap`。
    ///
    /// 用于复用 `resolve_target` 进行路径解析。
    pub fn to_target_path(&self) -> StarMapTargetPath {
        StarMapTargetPath {
            starmap_id: self.root_starmap_id.clone(),
            segments: self.segments.clone(),
            target: StarMapTargetDetail::Starmap,
        }
    }
}

/// 导航位置：解析路由后得到的当前画布信息。
///
/// - `current_starmap_id`：当前所在画布 ID
/// - `traversed_starmap_ids`：经过的所有画布 ID（含起点和终点，顺序为穿越顺序）
#[derive(Debug, Clone, PartialEq)]
pub struct StarMapNavigationLocation {
    pub current_starmap_id: String,
    pub traversed_starmap_ids: Vec<String>,
}

/// 解析导航路由，返回当前画布位置。
///
/// 构造 target 为 `StarMapTargetDetail::Starmap` 的 `StarMapTargetPath`，
/// 复用 `graph/resolve.rs::resolve_target` 进行解析。
///
/// 成功时返回 `StarMapNavigationLocation`，失败时返回对应的
/// `StarMapTargetResolveStatus` 错误（如 `MissingEmbed`、`MissingPortal` 等）。
pub fn resolve_navigation_route(
    context: &GraphResolverContext,
    route: &StarMapNavigationRoute,
) -> Result<StarMapNavigationLocation, StarMapTargetResolveStatus> {
    let path = route.to_target_path();
    let resolved: ResolvedTarget = resolve_target(context, &path)?;
    Ok(StarMapNavigationLocation {
        current_starmap_id: resolved.final_starmap_id,
        traversed_starmap_ids: resolved.traversed_starmap_ids,
    })
}

/// 通过 Embed 实例进入子画布。
///
/// 先解析当前 route 确认当前位置有效，再在当前图中查找 embed 实例是否存在，
/// 追加 `StarMapPathSegment::EnterEmbed` 到 segments，最后重新解析新 route
/// 验证整体有效性。
///
/// 成功时返回更新后的 `StarMapNavigationRoute` 和对应的 `StarMapNavigationLocation`。
/// 不靠"目标标题相同"或父字段推测——通过具体的 `instance_id` 确认实例存在。
pub fn enter_embed(
    context: &GraphResolverContext,
    route: &StarMapNavigationRoute,
    instance_id: &str,
) -> Result<(StarMapNavigationRoute, StarMapNavigationLocation), StarMapTargetResolveStatus> {
    // 先解析当前 route 确认当前位置有效
    resolve_navigation_route(context, route)?;

    // 追加 EnterEmbed 段
    let mut new_segments = route.segments.clone();
    new_segments.push(StarMapPathSegment::EnterEmbed {
        instance_id: instance_id.to_string(),
    });
    let new_route = StarMapNavigationRoute {
        root_starmap_id: route.root_starmap_id.clone(),
        segments: new_segments,
    };

    // 重新解析新 route 验证有效性。允许进入同一张星图（自引用 Embed），
    // 每进入一次就追加一个路径段，身份由完整路径与当次实例共同确定。
    let new_location = resolve_navigation_route(context, &new_route)?;

    Ok((new_route, new_location))
}

/// 通过 Portal 节点进入子画布。
///
/// 先解析当前 route 确认当前位置有效，再追加 `StarMapPathSegment::EnterPortal`
/// 到 segments，最后重新解析新 route 验证整体有效性。
///
/// 成功时返回更新后的 `StarMapNavigationRoute` 和对应的 `StarMapNavigationLocation`。
/// 不靠"目标标题相同"或父字段推测——通过具体的 `node_id` 确认 portal 存在。
pub fn enter_portal(
    context: &GraphResolverContext,
    route: &StarMapNavigationRoute,
    node_id: &str,
) -> Result<(StarMapNavigationRoute, StarMapNavigationLocation), StarMapTargetResolveStatus> {
    // 先解析当前 route 确认当前位置有效
    resolve_navigation_route(context, route)?;

    // 追加 EnterPortal 段
    let mut new_segments = route.segments.clone();
    new_segments.push(StarMapPathSegment::EnterPortal {
        node_id: node_id.to_string(),
    });
    let new_route = StarMapNavigationRoute {
        root_starmap_id: route.root_starmap_id.clone(),
        segments: new_segments,
    };

    // 重新解析新 route 验证有效性。允许 portal 回到当前或祖先星图，
    // 每进入一次就追加一个路径段。
    let new_location = resolve_navigation_route(context, &new_route)?;

    Ok((new_route, new_location))
}

/// 退出当前画布，返回上一层位置。
///
/// 弹出 `route.segments` 最后一个段。如果 segments 为空（已在根图），返回原 route。
///
/// 对同一目标星图的多个 Embed，返回的是**原进入实例**——因为路径段记录了
/// 具体的 `instance_id`，弹出最后一个段后，前面的段仍然保留了进入时的实例信息。
pub fn exit_canvas(route: &StarMapNavigationRoute) -> StarMapNavigationRoute {
    if route.segments.is_empty() {
        // 已在根图，无法再退出
        return route.clone();
    }
    let mut new_segments = route.segments.clone();
    new_segments.pop();
    StarMapNavigationRoute {
        root_starmap_id: route.root_starmap_id.clone(),
        segments: new_segments,
    }
}

/// 跳到任意祖先层级。
///
/// `depth` 表示截断后保留的路径段数量：
/// - `0` = 根图（清空所有 segments）
/// - `1` = 第一层子图（保留第一个 segment）
/// - `n` = 保留前 n 个 segments
///
/// 纯路径操作，不需要 context。如果 `depth >= route.segments.len()`，
/// 返回原 route（不做任何截断）。
pub fn jump_to_ancestor(route: &StarMapNavigationRoute, depth: usize) -> StarMapNavigationRoute {
    if depth >= route.segments.len() {
        return route.clone();
    }
    let new_segments = route.segments[..depth].to_vec();
    StarMapNavigationRoute {
        root_starmap_id: route.root_starmap_id.clone(),
        segments: new_segments,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::starmap::semantic::{StarMapNodeContent, StarMapPortal, StarMapProvenance};
    use crate::starmap::types::reference::StarMapTargetPath;
    use crate::starmap::types::{
        StarMapEmbed, StarMapGraph, StarMapNode, StarMapNodeKind, StarMapPoint,
    };
    use std::collections::HashMap;
    use std::path::PathBuf;

    // -----------------------------------------------------------------------
    // StarMapNavigationRoute 构造与转换
    // -----------------------------------------------------------------------
    #[test]
    fn test_route_new_empty_segments() {
        let route = StarMapNavigationRoute::new("sm_root".to_string());
        assert_eq!(route.root_starmap_id, "sm_root");
        assert!(route.segments.is_empty());
    }

    #[test]
    fn test_route_to_target_path() {
        let route = StarMapNavigationRoute {
            root_starmap_id: "sm_root".to_string(),
            segments: vec![
                StarMapPathSegment::EnterEmbed {
                    instance_id: "emb_1".to_string(),
                },
                StarMapPathSegment::EnterPortal {
                    node_id: "portal_1".to_string(),
                },
            ],
        };

        let path = route.to_target_path();
        assert_eq!(path.starmap_id, "sm_root");
        assert_eq!(path.segments.len(), 2);
        assert_eq!(path.target, StarMapTargetDetail::Starmap);
    }

    #[test]
    fn test_route_to_target_path_empty_segments() {
        let route = StarMapNavigationRoute::new("sm_root".to_string());
        let path = route.to_target_path();
        assert_eq!(path.starmap_id, "sm_root");
        assert!(path.segments.is_empty());
        assert_eq!(path.target, StarMapTargetDetail::Starmap);
    }

    // -----------------------------------------------------------------------
    // exit_canvas
    // -----------------------------------------------------------------------
    #[test]
    fn test_exit_canvas_pop_last_segment() {
        let route = StarMapNavigationRoute {
            root_starmap_id: "sm_root".to_string(),
            segments: vec![
                StarMapPathSegment::EnterEmbed {
                    instance_id: "emb_1".to_string(),
                },
                StarMapPathSegment::EnterPortal {
                    node_id: "portal_1".to_string(),
                },
            ],
        };

        let exited = exit_canvas(&route);
        assert_eq!(exited.segments.len(), 1);
        assert_eq!(
            exited.segments[0],
            StarMapPathSegment::EnterEmbed {
                instance_id: "emb_1".to_string(),
            }
        );
    }

    #[test]
    fn test_exit_canvas_at_root_returns_clone() {
        let route = StarMapNavigationRoute::new("sm_root".to_string());
        let exited = exit_canvas(&route);
        assert_eq!(exited, route);
        assert!(exited.segments.is_empty());
    }

    #[test]
    fn test_exit_canvas_single_segment_back_to_root() {
        let route = StarMapNavigationRoute {
            root_starmap_id: "sm_root".to_string(),
            segments: vec![StarMapPathSegment::EnterEmbed {
                instance_id: "emb_1".to_string(),
            }],
        };

        let exited = exit_canvas(&route);
        assert!(exited.segments.is_empty());
        assert_eq!(exited.root_starmap_id, "sm_root");
    }

    // -----------------------------------------------------------------------
    // jump_to_ancestor
    // -----------------------------------------------------------------------
    #[test]
    fn test_jump_to_ancestor_root() {
        let route = StarMapNavigationRoute {
            root_starmap_id: "sm_root".to_string(),
            segments: vec![
                StarMapPathSegment::EnterEmbed {
                    instance_id: "emb_1".to_string(),
                },
                StarMapPathSegment::EnterPortal {
                    node_id: "portal_1".to_string(),
                },
                StarMapPathSegment::EnterEmbed {
                    instance_id: "emb_2".to_string(),
                },
            ],
        };

        let jumped = jump_to_ancestor(&route, 0);
        assert!(jumped.segments.is_empty());
        assert_eq!(jumped.root_starmap_id, "sm_root");
    }

    #[test]
    fn test_jump_to_ancestor_first_level() {
        let route = StarMapNavigationRoute {
            root_starmap_id: "sm_root".to_string(),
            segments: vec![
                StarMapPathSegment::EnterEmbed {
                    instance_id: "emb_1".to_string(),
                },
                StarMapPathSegment::EnterPortal {
                    node_id: "portal_1".to_string(),
                },
                StarMapPathSegment::EnterEmbed {
                    instance_id: "emb_2".to_string(),
                },
            ],
        };

        let jumped = jump_to_ancestor(&route, 1);
        assert_eq!(jumped.segments.len(), 1);
        assert_eq!(
            jumped.segments[0],
            StarMapPathSegment::EnterEmbed {
                instance_id: "emb_1".to_string(),
            }
        );
    }

    #[test]
    fn test_jump_to_ancestor_depth_exceeds_len() {
        let route = StarMapNavigationRoute {
            root_starmap_id: "sm_root".to_string(),
            segments: vec![StarMapPathSegment::EnterEmbed {
                instance_id: "emb_1".to_string(),
            }],
        };

        let jumped = jump_to_ancestor(&route, 10);
        assert_eq!(jumped, route);
    }

    #[test]
    fn test_jump_to_ancestor_exact_len() {
        let route = StarMapNavigationRoute {
            root_starmap_id: "sm_root".to_string(),
            segments: vec![
                StarMapPathSegment::EnterEmbed {
                    instance_id: "emb_1".to_string(),
                },
                StarMapPathSegment::EnterPortal {
                    node_id: "portal_1".to_string(),
                },
            ],
        };

        let jumped = jump_to_ancestor(&route, 2);
        assert_eq!(jumped, route);
    }

    // -----------------------------------------------------------------------
    // 辅助函数：构造测试用 overlay context
    // -----------------------------------------------------------------------
    fn make_overlay_context(overlays: HashMap<String, StarMapGraph>) -> GraphResolverContext {
        GraphResolverContext {
            app_data_root: PathBuf::from("/tmp/test_starmap_nav"),
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

    fn make_portal_node(node_id: &str, destination_starmap_id: &str) -> StarMapNode {
        StarMapNode {
            id: node_id.to_string(),
            title: node_id.to_string(),
            kind: StarMapNodeKind::Note,
            payload: None,
            tags: vec![],
            content: StarMapNodeContent::Empty,
            anchors: vec![],
            portal: Some(StarMapPortal {
                destination_starmap_id: destination_starmap_id.to_string(),
                destination_target: None,
            }),
            position: StarMapPoint::default(),
            style: Default::default(),
            provenance: StarMapProvenance::default(),
            created_at: 0,
            updated_at: 0,
        }
    }

    // -----------------------------------------------------------------------
    // resolve_navigation_route（使用 overlay context）
    // -----------------------------------------------------------------------
    #[test]
    fn test_resolve_navigation_route_at_root() {
        let mut overlays = HashMap::new();
        overlays.insert("sm_root".to_string(), make_empty_graph("sm_root"));
        let context = make_overlay_context(overlays);

        let route = StarMapNavigationRoute::new("sm_root".to_string());
        let location = resolve_navigation_route(&context, &route).unwrap();
        assert_eq!(location.current_starmap_id, "sm_root");
        assert_eq!(location.traversed_starmap_ids, vec!["sm_root"]);
    }

    #[test]
    fn test_resolve_navigation_route_missing_starmap() {
        let context = make_overlay_context(HashMap::new());
        let route = StarMapNavigationRoute::new("sm_nonexistent".to_string());
        let result = resolve_navigation_route(&context, &route);
        assert_eq!(result, Err(StarMapTargetResolveStatus::MissingStarmap));
    }

    #[test]
    fn test_resolve_navigation_route_through_embed() {
        let mut root_graph = make_empty_graph("sm_root");
        root_graph.embeds.push(make_embed("emb_1", "sm_child"));

        let mut overlays = HashMap::new();
        overlays.insert("sm_root".to_string(), root_graph);
        overlays.insert("sm_child".to_string(), make_empty_graph("sm_child"));
        let context = make_overlay_context(overlays);

        let route = StarMapNavigationRoute {
            root_starmap_id: "sm_root".to_string(),
            segments: vec![StarMapPathSegment::EnterEmbed {
                instance_id: "emb_1".to_string(),
            }],
        };

        let location = resolve_navigation_route(&context, &route).unwrap();
        assert_eq!(location.current_starmap_id, "sm_child");
        assert_eq!(location.traversed_starmap_ids, vec!["sm_root", "sm_child"]);
    }

    #[test]
    fn test_resolve_navigation_route_missing_embed() {
        let mut overlays = HashMap::new();
        overlays.insert("sm_root".to_string(), make_empty_graph("sm_root"));
        let context = make_overlay_context(overlays);

        let route = StarMapNavigationRoute {
            root_starmap_id: "sm_root".to_string(),
            segments: vec![StarMapPathSegment::EnterEmbed {
                instance_id: "emb_nonexistent".to_string(),
            }],
        };

        let result = resolve_navigation_route(&context, &route);
        assert_eq!(result, Err(StarMapTargetResolveStatus::MissingEmbed));
    }

    #[test]
    fn test_resolve_navigation_route_allows_mutual_embed_cycle() {
        // 两个图互相 embed 对方：A→B→A 是合法路径，重复的 starmap_id 保留在穿越记录里。
        let mut graph_a = make_empty_graph("sm_a");
        graph_a.embeds.push(make_embed("emb_a", "sm_b"));
        let mut graph_b = make_empty_graph("sm_b");
        graph_b.embeds.push(make_embed("emb_b", "sm_a"));

        let mut overlays = HashMap::new();
        overlays.insert("sm_a".to_string(), graph_a);
        overlays.insert("sm_b".to_string(), graph_b);
        let context = make_overlay_context(overlays);

        let route = StarMapNavigationRoute {
            root_starmap_id: "sm_a".to_string(),
            segments: vec![
                StarMapPathSegment::EnterEmbed {
                    instance_id: "emb_a".to_string(),
                },
                StarMapPathSegment::EnterEmbed {
                    instance_id: "emb_b".to_string(),
                },
            ],
        };

        let location = resolve_navigation_route(&context, &route).unwrap();
        assert_eq!(location.current_starmap_id, "sm_a");
        assert_eq!(
            location.traversed_starmap_ids,
            vec!["sm_a", "sm_b", "sm_a"]
        );
    }

    #[test]
    fn test_resolve_navigation_route_allows_self_embed() {
        let mut graph_a = make_empty_graph("sm_a");
        graph_a.embeds.push(make_embed("self", "sm_a"));

        let mut overlays = HashMap::new();
        overlays.insert("sm_a".to_string(), graph_a);
        let context = make_overlay_context(overlays);

        let route = StarMapNavigationRoute {
            root_starmap_id: "sm_a".to_string(),
            segments: vec![StarMapPathSegment::EnterEmbed {
                instance_id: "self".to_string(),
            }],
        };

        let location = resolve_navigation_route(&context, &route).unwrap();
        assert_eq!(location.current_starmap_id, "sm_a");
        assert_eq!(location.traversed_starmap_ids, vec!["sm_a", "sm_a"]);
    }

    #[test]
    fn test_resolve_navigation_route_allows_more_than_32_layers() {
        // 100 层真实链式 Embed：不再存在固定深度上限。
        let depth = 100;
        let mut overlays = HashMap::new();
        for i in 0..depth {
            let mut graph = make_empty_graph(&format!("sm_{}", i));
            graph
                .embeds
                .push(make_embed(&format!("emb_{}", i), &format!("sm_{}", i + 1)));
            overlays.insert(format!("sm_{}", i), graph);
        }
        overlays.insert(format!("sm_{}", depth), make_empty_graph(&format!("sm_{}", depth)));

        let mut segments = Vec::new();
        for i in 0..depth {
            segments.push(StarMapPathSegment::EnterEmbed {
                instance_id: format!("emb_{}", i),
            });
        }

        let route = StarMapNavigationRoute {
            root_starmap_id: "sm_0".to_string(),
            segments,
        };

        let context = make_overlay_context(overlays);
        let location = resolve_navigation_route(&context, &route).unwrap();
        assert_eq!(location.current_starmap_id, format!("sm_{}", depth));
        assert_eq!(location.traversed_starmap_ids.len(), depth as usize + 1);
    }

    // -----------------------------------------------------------------------
    // enter_embed / enter_portal
    // -----------------------------------------------------------------------
    #[test]
    fn test_enter_embed_success() {
        let mut root_graph = make_empty_graph("sm_root");
        root_graph.embeds.push(make_embed("emb_1", "sm_child"));

        let mut overlays = HashMap::new();
        overlays.insert("sm_root".to_string(), root_graph);
        overlays.insert("sm_child".to_string(), make_empty_graph("sm_child"));
        let context = make_overlay_context(overlays);

        let route = StarMapNavigationRoute::new("sm_root".to_string());
        let (new_route, location) = enter_embed(&context, &route, "emb_1").unwrap();

        assert_eq!(new_route.segments.len(), 1);
        assert_eq!(
            new_route.segments[0],
            StarMapPathSegment::EnterEmbed {
                instance_id: "emb_1".to_string(),
            }
        );
        assert_eq!(location.current_starmap_id, "sm_child");
        assert_eq!(location.traversed_starmap_ids, vec!["sm_root", "sm_child"]);
    }

    #[test]
    fn test_enter_embed_missing_instance() {
        let mut overlays = HashMap::new();
        overlays.insert("sm_root".to_string(), make_empty_graph("sm_root"));
        let context = make_overlay_context(overlays);

        let route = StarMapNavigationRoute::new("sm_root".to_string());
        let result = enter_embed(&context, &route, "emb_nonexistent");
        assert_eq!(result, Err(StarMapTargetResolveStatus::MissingEmbed));
    }

    #[test]
    fn test_enter_portal_success() {
        let mut root_graph = make_empty_graph("sm_root");
        root_graph
            .nodes
            .push(make_portal_node("portal_node", "sm_child"));

        let mut overlays = HashMap::new();
        overlays.insert("sm_root".to_string(), root_graph);
        overlays.insert("sm_child".to_string(), make_empty_graph("sm_child"));
        let context = make_overlay_context(overlays);

        let route = StarMapNavigationRoute::new("sm_root".to_string());
        let (new_route, location) = enter_portal(&context, &route, "portal_node").unwrap();

        assert_eq!(new_route.segments.len(), 1);
        assert_eq!(
            new_route.segments[0],
            StarMapPathSegment::EnterPortal {
                node_id: "portal_node".to_string(),
            }
        );
        assert_eq!(location.current_starmap_id, "sm_child");
        assert_eq!(location.traversed_starmap_ids, vec!["sm_root", "sm_child"]);
    }

    #[test]
    fn test_enter_portal_missing_node() {
        let mut overlays = HashMap::new();
        overlays.insert("sm_root".to_string(), make_empty_graph("sm_root"));
        let context = make_overlay_context(overlays);

        let route = StarMapNavigationRoute::new("sm_root".to_string());
        let result = enter_portal(&context, &route, "nonexistent_node");
        assert_eq!(result, Err(StarMapTargetResolveStatus::MissingPortal));
    }

    // -----------------------------------------------------------------------
    // exit_canvas + enter_embed 组合：对同一目标星图的多个 Embed，
    // 返回的是原进入实例
    // -----------------------------------------------------------------------
    #[test]
    fn test_exit_after_multiple_embeds_returns_original_instance() {
        // 根图有两个 embed，都指向同一个子图 sm_child
        let mut root_graph = make_empty_graph("sm_root");
        root_graph.embeds.push(make_embed("emb_a", "sm_child"));
        root_graph.embeds.push(make_embed("emb_b", "sm_child"));

        let mut overlays = HashMap::new();
        overlays.insert("sm_root".to_string(), root_graph);
        overlays.insert("sm_child".to_string(), make_empty_graph("sm_child"));
        let context = make_overlay_context(overlays);

        // 通过 emb_a 进入子图
        let route = StarMapNavigationRoute::new("sm_root".to_string());
        let (route_via_a, _loc_a) = enter_embed(&context, &route, "emb_a").unwrap();
        assert_eq!(
            route_via_a.segments[0],
            StarMapPathSegment::EnterEmbed {
                instance_id: "emb_a".to_string(),
            }
        );

        // 退出后回到根图
        let exited = exit_canvas(&route_via_a);
        assert!(exited.segments.is_empty());
        assert_eq!(exited.root_starmap_id, "sm_root");

        // 再通过 emb_b 进入同一个子图
        let (route_via_b, _loc_b) = enter_embed(&context, &exited, "emb_b").unwrap();
        assert_eq!(
            route_via_b.segments[0],
            StarMapPathSegment::EnterEmbed {
                instance_id: "emb_b".to_string(),
            }
        );

        // 退出后仍然回到根图，且路径段记录的是 emb_b（原进入实例），不是 emb_a
        let exited_b = exit_canvas(&route_via_b);
        assert!(exited_b.segments.is_empty());
    }

    // -----------------------------------------------------------------------
    // jump_to_ancestor + resolve 组合
    // -----------------------------------------------------------------------
    #[test]
    fn test_jump_to_ancestor_then_resolve() {
        // 三层路径：root -> emb_1 -> sm_child1 -> portal_1 -> sm_child2
        let mut root_graph = make_empty_graph("sm_root");
        root_graph.embeds.push(make_embed("emb_1", "sm_child1"));

        let mut child1_graph = make_empty_graph("sm_child1");
        child1_graph
            .nodes
            .push(make_portal_node("portal_1", "sm_child2"));

        let child2_graph = make_empty_graph("sm_child2");

        let mut overlays = HashMap::new();
        overlays.insert("sm_root".to_string(), root_graph);
        overlays.insert("sm_child1".to_string(), child1_graph);
        overlays.insert("sm_child2".to_string(), child2_graph);
        let context = make_overlay_context(overlays);

        let route = StarMapNavigationRoute {
            root_starmap_id: "sm_root".to_string(),
            segments: vec![
                StarMapPathSegment::EnterEmbed {
                    instance_id: "emb_1".to_string(),
                },
                StarMapPathSegment::EnterPortal {
                    node_id: "portal_1".to_string(),
                },
            ],
        };

        // 跳到第一层（只保留 emb_1）
        let jumped = jump_to_ancestor(&route, 1);
        let location = resolve_navigation_route(&context, &jumped).unwrap();
        assert_eq!(location.current_starmap_id, "sm_child1");
        assert_eq!(location.traversed_starmap_ids, vec!["sm_root", "sm_child1"]);

        // 跳到根图
        let jumped_root = jump_to_ancestor(&route, 0);
        let location_root = resolve_navigation_route(&context, &jumped_root).unwrap();
        assert_eq!(location_root.current_starmap_id, "sm_root");
        assert_eq!(location_root.traversed_starmap_ids, vec!["sm_root"]);
    }
}
