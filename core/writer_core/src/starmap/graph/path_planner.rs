//! # 跨层连线宿主规划（Core 唯一事实来源）
//!
//! 平台端（Qt QML / Harmony ArkTS）在"把一条 Edge / Link 落在哪张图上"时，
//! 过去各自复制了一套"最近公共祖先 + 端点相对宿主路径"的纯路径算法。此模块
//! 把这条业务规则收回 Core，平台端只提交两条**相对同一根星图**的绝对端点路径，
//! 由这里统一算出宿主星图和两条相对宿主的端点路径。
//!
//! ## 语义
//!
//! - 输入是两条 `StarMapTargetPath`，`starmap_id` 都必须是**同一条根星图**，
//!   `segments` 是从根星图逐层穿越到端点所在 Scene 的绝对路径。
//! - 端点有两种形态（与平台端构造规则一致）：
//!   - 普通对象（Node / Anchor 等）：`target` 就是对象，其所在 Scene 由
//!     整条 `segments` 决定。
//!   - 容器端点（Embed / 旧 Portal 实例）：`target` 为 `Starmap`，最后一段
//!     `EnterEmbed`/`EnterPortal` 就是被引用的容器自身，它属于**前一段**到达的
//!     Scene。
//! - 宿主 = 两个端点所在 Scene 的最近公共祖先；两端各自的 `EnterEmbed` 段
//!   按此公共前缀求最长公共前缀（LCP）。
//!
//! 本模块是**纯函数**：不读屏幕坐标，不依赖任何一端是否已经渲染出子 Scene，
//! 只使用 `graph::resolve` / `navigation` 的路径解析事实。

use crate::starmap::graph::resolve::{resolve_target, GraphResolverContext};
use crate::starmap::navigation::{resolve_navigation_route, StarMapNavigationRoute};
use crate::starmap::semantic::{StarMapTargetDetail, StarMapTargetResolveStatus};
use crate::starmap::types::reference::{StarMapPathSegment, StarMapTargetPath};

/// 跨层连线宿主规划结果。
///
/// - `host_starmap_id`：关系应当落入的宿主星图 ID（Edge/Link 的 `starmap_id`）。
/// - `host_segments`：宿主 Scene 相对根星图的绝对路径段（供平台端按路径找到
///   宿主画布；与 `from`/`to` 的起点一致）。
/// - `from` / `to`：两条相对于宿主星图的 `StarMapTargetPath`，`starmap_id`
///   已填成 `host_starmap_id`，可直接交给 `add_starmap_edge` / `add_starmap_link`。
#[derive(Debug, Clone, PartialEq)]
pub struct CrossLayerConnectionPlan {
    pub host_starmap_id: String,
    pub host_segments: Vec<StarMapPathSegment>,
    pub from: StarMapTargetPath,
    pub to: StarMapTargetPath,
}

/// 跨层连线宿主规划错误。
///
/// 这些错误都是"拒绝规划"：调用方不应回退到旧算法或自行猜测宿主。
#[derive(Debug, Clone, PartialEq)]
pub enum CrossLayerConnectionError {
    /// 两个端点不属于同一条根星图（或起点为空）。
    NotSameRoot,
    /// 两个端点解析后指向同一个对象（自环）。
    SelfLoop,
    /// from 端点路径不可解析。
    FromResolveFailed(StarMapTargetResolveStatus),
    /// to 端点路径不可解析。
    ToResolveFailed(StarMapTargetResolveStatus),
    /// 公共宿主 Scene 不可解析（正常不应发生，端点路径已解析）。
    HostResolveFailed(StarMapTargetResolveStatus),
}

/// 端点在 Scene 树里的归属形态。
enum EndpointScene<'a> {
    /// 普通对象：`scene_segments` 就是它所在 Scene 的绝对路径。
    Item {
        scene_segments: &'a [StarMapPathSegment],
    },
    /// 容器端点：最后一段是容器自身，属于前一段到达的 Scene。
    Container {
        scene_segments: &'a [StarMapPathSegment],
        terminal: &'a StarMapPathSegment,
    },
}

impl<'a> EndpointScene<'a> {
    fn scene_segments(&self) -> &'a [StarMapPathSegment] {
        match self {
            EndpointScene::Item { scene_segments } => scene_segments,
            EndpointScene::Container { scene_segments, .. } => scene_segments,
        }
    }
}

/// 判断一段路径段是否是"容器段"（Embed 或旧 Portal）。
fn is_container_segment(segment: &StarMapPathSegment) -> bool {
    matches!(
        segment,
        StarMapPathSegment::EnterEmbed { .. } | StarMapPathSegment::EnterPortal { .. }
    )
}

/// 把一个端点路径解析成"它属于哪个 Scene"。
///
/// 与平台端沿用同一分类：`target` 为 `Starmap` 且最后一段是容器段时，
/// 该段本身是被引用的容器（Embed / 旧 Portal）；否则整条 `segments` 就是
/// 端点所在 Scene 的绝对路径。
fn endpoint_scene(path: &StarMapTargetPath) -> EndpointScene<'_> {
    if matches!(path.target, StarMapTargetDetail::Starmap) {
        if let Some(last) = path.segments.last() {
            if is_container_segment(last) {
                return EndpointScene::Container {
                    scene_segments: &path.segments[..path.segments.len() - 1],
                    terminal: last,
                };
            }
        }
    }
    EndpointScene::Item {
        scene_segments: &path.segments,
    }
}

/// 两条 Scene 路径的最长公共前缀长度（按 `(type, instanceId, nodeId)` 逐段比较）。
fn common_prefix_len(a: &[StarMapPathSegment], b: &[StarMapPathSegment]) -> usize {
    let mut len = 0;
    while len < a.len() && len < b.len() && a[len] == b[len] {
        len += 1;
    }
    len
}

/// 构造端点相对宿主的路径：切掉公共前缀，容器端点补回原来的终端段。
fn build_target_path_for_host(
    host_starmap_id: &str,
    host_segments: &[StarMapPathSegment],
    endpoint: &EndpointScene<'_>,
    target: &StarMapTargetDetail,
) -> StarMapTargetPath {
    let mut segments = endpoint.scene_segments()[host_segments.len()..].to_vec();
    let target = match endpoint {
        EndpointScene::Item { .. } => target.clone(),
        EndpointScene::Container { terminal, .. } => {
            segments.push((*terminal).clone());
            StarMapTargetDetail::Starmap
        }
    };
    StarMapTargetPath {
        starmap_id: host_starmap_id.to_string(),
        segments,
        target,
    }
}

/// 规划一条跨层关系（Edge / Link）应当落在哪张图上。
///
/// Edge（语义边）与 Link（内部跳转）共用这一套宿主规划，不再各写一份最近
/// 公共祖先算法。步骤：
///
/// 1. 两端必须从同一条根星图出发，否则 `NotSameRoot`。
/// 2. 分别用 `resolve_target` 确认两端路径可达（对象/容器/途中穿越都存在），
///    失败返回 `FromResolveFailed` / `ToResolveFailed`。
/// 3. 求两端 Scene 路径的最长公共前缀 `host_segments`，解析得到 `host_starmap_id`。
/// 4. 两端各自切掉公共前缀得到相对宿主的 `from` / `to`；两者相同即自环，返回 `SelfLoop`。
pub fn plan_cross_layer_connection(
    context: &GraphResolverContext,
    from: &StarMapTargetPath,
    to: &StarMapTargetPath,
) -> Result<CrossLayerConnectionPlan, CrossLayerConnectionError> {
    if from.starmap_id.is_empty() || from.starmap_id != to.starmap_id {
        return Err(CrossLayerConnectionError::NotSameRoot);
    }

    // 两端路径都必须可达：Node/Anchor/Embed/Portal 逐段在 resolver 里验证。
    resolve_target(context, from).map_err(CrossLayerConnectionError::FromResolveFailed)?;
    resolve_target(context, to).map_err(CrossLayerConnectionError::ToResolveFailed)?;

    let from_scene = endpoint_scene(from);
    let to_scene = endpoint_scene(to);

    let host_len = common_prefix_len(from_scene.scene_segments(), to_scene.scene_segments());
    let host_segments = from_scene.scene_segments()[..host_len].to_vec();

    // 公共前缀对应的路由必然可解析（它是已解析端点路径的前缀），
    // 仍显式处理失败，避免静默产生错误宿主。
    let host_route = StarMapNavigationRoute {
        root_starmap_id: from.starmap_id.clone(),
        segments: host_segments.clone(),
    };
    let host_location = resolve_navigation_route(context, &host_route)
        .map_err(CrossLayerConnectionError::HostResolveFailed)?;
    let host_starmap_id = host_location.current_starmap_id;

    let plan_from =
        build_target_path_for_host(&host_starmap_id, &host_segments, &from_scene, &from.target);
    let plan_to =
        build_target_path_for_host(&host_starmap_id, &host_segments, &to_scene, &to.target);

    if plan_from == plan_to {
        return Err(CrossLayerConnectionError::SelfLoop);
    }

    Ok(CrossLayerConnectionPlan {
        host_starmap_id,
        host_segments,
        from: plan_from,
        to: plan_to,
    })
}

#[cfg(test)]
#[path = "path_planner_tests.rs"]
mod tests;
