// =============================================================================
// starmap_view/bridge.rs — 星图显示层入口（Linux 平台端命中/布局/边几何）
// =============================================================================
//
// 层级：Linux_qt 平台层显示入口。
//
// 归属说明：
//   从 starmap_bridge.rs 拆出。这里只做纯显示几何计算——边渲染、命中测试、网格
//   自动布局、以及从图快照派生布局视图——不碰 Core 星图 DTO 的 CRUD。CRUD 仍在
//   starmap_bridge.rs，本模块不复制业务状态机。
//
//   几何算法在同模块下的 edge_render / grid_layout / hittest / layout_types 实现，
//   bridge 只是把它们组装成 backend 能直接调的 envelope JSON 入口。边渲染接已取到
//   的 graph + 平台 scene geometry（node 位置 + embed 显示包围盒），只做显示计算，
//   不再从 Core 读图快照——graph 读取已上移到 backend 组合边界。
//
// 被什么引用：
// - apps/Linux_qt/src/backend/starmap_backend.rs：命中/边渲染/自动布局入口。
// - apps/Linux_qt/src/backend/starmap_backend/documents.rs：派生布局视图时调
//   layout_from_graph（pub(crate)）。
// =============================================================================

use writer_core::api::types::StarMapGraphDto;
use writer_core::api::WriterError;

use super::edge_render::{self, EdgeRenderParams};
use super::grid_layout;
use super::hittest;
use super::layout_types::{StarMapEmbedSceneRect, StarMapLayout, StarMapLayoutKind, StarMapLayoutNode};

/// 平台端显示层常量：节点默认包围盒与圆角。
///
/// Core 只保存 `StarMapNode.position`（节点左上角坐标），不含宽高/圆角——
/// 那是纯显示参数，按「Core 不管显示层」契约归平台端。QML 侧
/// `buildModels()` 在拿不到 layout 节点时也用同样的默认值，两处必须一致。
const DEFAULT_NODE_WIDTH: f32 = 150.0;
const DEFAULT_NODE_HEIGHT: f32 = 60.0;
const DEFAULT_NODE_RADIUS: f32 = 30.0;

/// 成功结果的 envelope 包装。
fn envelope_ok<T: serde::Serialize>(data: T) -> String {
    writer_core::api::ResultEnvelope::success(data).to_json_string()
}

/// 错误结果的 envelope 包装（字符串消息走 WriterError::Other）。
fn envelope_err_str(msg: &str) -> String {
    writer_core::api::ResultEnvelope::<serde_json::Value>::error(WriterError::Other(
        msg.to_string(),
    ))
    .to_json_string()
}

/// 从星图图数据派生前端布局视图。
///
/// Core 收口后没有独立的 layout 实体，节点坐标唯一真相是
/// `StarMapNodeDto.position`。这里把位置 + 显示默认值合成 `StarMapLayout`，
/// 保持 QML 侧 `layout.nodes[].nodeId/x/y/width/height` 契约不变。
///
/// 设为 `pub(crate)` 供 backend 组合边界（`starmap_backend/documents.rs`）派生
/// 布局时复用，不对外暴露给 starmap_bridge。
pub(crate) fn layout_from_graph(graph: &StarMapGraphDto) -> StarMapLayout {
    StarMapLayout {
        kind: StarMapLayoutKind::Freeform,
        nodes: graph
            .nodes
            .iter()
            .map(|n| StarMapLayoutNode {
                node_id: n.id.clone(),
                x: n.position.x,
                y: n.position.y,
                width: DEFAULT_NODE_WIDTH,
                height: DEFAULT_NODE_HEIGHT,
                radius: DEFAULT_NODE_RADIUS,
                collapsed: false,
                z_index: 0,
                scale: 1.0,
                depth: 0.0,
                focus_weight: 0.0,
                orbit_group: None,
            })
            .collect(),
    }
}

/// 计算边渲染几何（箭头/偏移/标签位置）。
///
/// 几何算法在 Linux 平台端 `starmap_view::edge_render` 实现，Core 不预计算渲染数据。
/// 输入 `nodes_json` 是 QML 传来的 `[{id,x,y,width,height}]`（node scene geometry），
/// 转成显示层 layout 节点；`embeds_json` 是 QML 传来的
/// `[{instanceId,x,y,width,height}]`（embed scene geometry），转成显示层 embed 包围盒。
/// 边的 from/to 路径解析需要的完整 graph 结构由调用方（backend 组合边界）先从 Core
/// 取好再传入，本函数不再读 Core。
pub fn compute_edge_renders_json(
    graph: &writer_core::starmap::types::StarMapGraph,
    nodes_json: &str,
    embeds_json: &str,
) -> String {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct NodePos {
        id: String,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    }

    let nodes: Vec<NodePos> = match serde_json::from_str(nodes_json) {
        Ok(v) => v,
        Err(e) => return envelope_err_str(&format!("Invalid nodes JSON: {}", e)),
    };

    let layout = StarMapLayout {
        kind: StarMapLayoutKind::Freeform,
        nodes: nodes
            .into_iter()
            .map(|n| StarMapLayoutNode {
                node_id: n.id,
                x: n.x,
                y: n.y,
                width: n.width,
                height: n.height,
                radius: DEFAULT_NODE_RADIUS,
                collapsed: false,
                z_index: 0,
                scale: 1.0,
                depth: 0.0,
                focus_weight: 1.0,
                orbit_group: None,
            })
            .collect(),
    };

    let embed_rects: Vec<StarMapEmbedSceneRect> = match serde_json::from_str(embeds_json) {
        Ok(v) => v,
        Err(e) => return envelope_err_str(&format!("Invalid embeds JSON: {}", e)),
    };

    let batch = edge_render::compute_edge_renders_from_paths(
        &graph.edges,
        graph,
        &layout,
        &embed_rects,
        &EdgeRenderParams::default(),
    );
    // 只有 diagnostics 非空时才写日志，避免高频调用产生无意义的空日志噪声。
    if !batch.diagnostics.is_empty() {
        log::debug!("compute_edge_renders diagnostics: {:?}", batch.diagnostics);
    }
    envelope_ok(batch.renders)
}

/// 对已算好的边渲染结果做命中测试。
///
/// 纯几何，不读 Core：`renders_json` 是上一次 `compute_edge_renders_json` 的输出。
pub fn hit_test_edge_renders_json(renders_json: &str, x: f32, y: f32) -> String {
    let renders: Vec<edge_render::EdgeRender> = match serde_json::from_str(renders_json) {
        Ok(v) => v,
        Err(e) => return envelope_err_str(&format!("Invalid renders JSON: {}", e)),
    };

    let result = edge_render::hit_test_edge_renders(x, y, &renders);
    envelope_ok(result)
}

/// 对布局节点做 AABB 命中测试，返回命中的 node_id。
pub fn hit_test_nodes_json(nodes_json: &str, x: f32, y: f32) -> String {
    let nodes: Vec<StarMapLayoutNode> = match serde_json::from_str(nodes_json) {
        Ok(v) => v,
        Err(e) => return envelope_err_str(&format!("Invalid nodes JSON: {}", e)),
    };

    let result = hittest::hit_test_nodes(x, y, &nodes);
    envelope_ok(result.map(|r| r.id))
}

/// 网格自动布局：按 node_ids 在已有布局基础上重排。
///
/// 纯几何，不读 Core。`existing_layout_json` 反序列化失败时退回默认空布局。
pub fn calculate_grid_layout_json(node_ids_json: &str, existing_layout_json: &str) -> String {
    let node_ids: Vec<String> = match serde_json::from_str(node_ids_json) {
        Ok(v) => v,
        Err(e) => return envelope_err_str(&format!("Invalid node IDs JSON: {}", e)),
    };

    let existing: StarMapLayout = serde_json::from_str(existing_layout_json).unwrap_or_default();

    let layout = grid_layout::calculate_grid_layout(&node_ids, &existing);
    envelope_ok(layout)
}
