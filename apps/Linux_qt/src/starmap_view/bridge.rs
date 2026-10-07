// =============================================================================
// starmap_view/bridge.rs — 星图显示层入口（Linux 平台端命中/布局/边几何）
// =============================================================================
//
// 层级：Linux_qt 平台层显示入口。
//
// 归属说明：
//   从 starmap_bridge.rs 拆出。这里只做纯显示几何计算——边渲染、命中测试——不碰
//   Core 星图 DTO 的 CRUD。CRUD 仍在 starmap_bridge.rs，本模块不复制业务状态机。
//
//   几何算法在同模块下的 edge_render / hittest / layout_types 实现，
//   bridge 只是把它们组装成 backend 能直接调的 envelope JSON 入口。边渲染接已取到
//   的 graph + 平台 scene geometry（node 位置 + embed 显示包围盒），只做显示计算，
//   不再从 Core 读图快照——graph 读取已上移到 backend 组合边界。
//
// 被什么引用：
// - apps/Linux_qt/src/backend/starmap_backend.rs：命中/边渲染入口。
// =============================================================================

use writer_core::api::WriterError;

use super::edge_render::{self, EdgeRenderParams};
use super::hittest;
use super::layout_types::{
    StarMapEmbedSceneRect, StarMapLayout, StarMapLayoutKind, StarMapLayoutNode,
};

/// 平台端显示层常量：节点默认圆角。
///
/// Core 只保存 `StarMapNode.position`（节点左上角坐标），不含宽高/圆角——
/// 那是纯显示参数，按「Core 不管显示层」契约归平台端。QML 侧
/// `buildModels()` 用同样的默认值，两处必须一致。
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

/// QML 传来的 scene geometry JSON → 显示层 layout + embed 包围盒。
///
/// `nodes_json` 是 `[{id,x,y,width,height}]`（node scene geometry）；
/// `embeds_json` 是 `[{instanceId,x,y,width,height}]`（embed scene geometry）。
fn parse_scene_geometry(
    nodes_json: &str,
    embeds_json: &str,
) -> Result<(StarMapLayout, Vec<StarMapEmbedSceneRect>), String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct NodePos {
        id: String,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    }

    let nodes: Vec<NodePos> =
        serde_json::from_str(nodes_json).map_err(|e| format!("Invalid nodes JSON: {}", e))?;

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

    let embed_rects: Vec<StarMapEmbedSceneRect> =
        serde_json::from_str(embeds_json).map_err(|e| format!("Invalid embeds JSON: {}", e))?;

    Ok((layout, embed_rects))
}

/// 计算边渲染几何（箭头/偏移/标签位置）。
///
/// 几何算法在 Linux 平台端 `starmap_view::edge_render` 实现，Core 不预计算渲染数据。
/// 边的 from/to 路径解析需要的完整 graph 结构由调用方（backend 组合边界）先从 Core
/// 取好再传入，本函数不再读 Core。
pub fn compute_edge_renders_json(
    graph: &writer_core::starmap::types::StarMapGraph,
    nodes_json: &str,
    embeds_json: &str,
) -> String {
    let (layout, embed_rects) = match parse_scene_geometry(nodes_json, embeds_json) {
        Ok(v) => v,
        Err(e) => return envelope_err_str(&e),
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

/// 候选边预览（拉线用）：在宿主图现有边表上临时追加一条固定 id 的候选边，
/// 调同一个正式边 renderer，只返回候选边自己的 render。
///
/// 这样预览与正式边共用同一份几何真相：Node 矩形 / Embed 圆周、旧 portal 归一、
/// LCA 后深路径投影、已有反向边时的双向偏移全部一致。
/// `from_path_json` / `to_path_json` 是 QML prospective LCA 规划出的
/// `StarMapTargetPath`（starmapId 必须已绑定到宿主图）。
///
/// 端点无法在宿主图定位（拖到空白/非法路径）返回 `success + null`，
/// QML 退回自由预览，不算错误。
pub fn compute_prospective_edge_render_json(
    graph: &writer_core::starmap::types::StarMapGraph,
    nodes_json: &str,
    embeds_json: &str,
    from_path_json: &str,
    to_path_json: &str,
) -> String {
    let (layout, embed_rects) = match parse_scene_geometry(nodes_json, embeds_json) {
        Ok(v) => v,
        Err(e) => return envelope_err_str(&e),
    };
    let from_path: writer_core::starmap::types::reference::StarMapTargetPath =
        match serde_json::from_str(from_path_json) {
            Ok(v) => v,
            Err(e) => return envelope_err_str(&format!("Invalid from path JSON: {}", e)),
        };
    let to_path: writer_core::starmap::types::reference::StarMapTargetPath =
        match serde_json::from_str(to_path_json) {
            Ok(v) => v,
            Err(e) => return envelope_err_str(&format!("Invalid to path JSON: {}", e)),
        };

    match edge_render::compute_prospective_edge_render(
        &from_path,
        &to_path,
        graph,
        &layout,
        &embed_rects,
        &EdgeRenderParams::default(),
    ) {
        Ok(render) => envelope_ok(render),
        Err(diag) => {
            log::debug!("compute_prospective_edge_render diagnostic: {:?}", diag);
            envelope_ok(serde_json::Value::Null)
        }
    }
}

/// 对已算好的边渲染结果做命中测试。
///
/// 纯几何，不读 Core：`renders_json` 是上一次 `compute_edge_renders_json` 的输出。
/// `threshold` 由调用方按屏幕像素折算（世界单位 = 屏幕像素 ÷ effectiveScale），
/// 不再走固定 world 阈值：相机范围放开后固定阈值在屏幕上的手感会差几个数量级。
pub fn hit_test_edge_renders_json(renders_json: &str, x: f32, y: f32, threshold: f32) -> String {
    let renders: Vec<edge_render::EdgeRender> = match serde_json::from_str(renders_json) {
        Ok(v) => v,
        Err(e) => return envelope_err_str(&format!("Invalid renders JSON: {}", e)),
    };

    let result = edge_render::hit_test_edge_renders_with_threshold(x, y, &renders, threshold);
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

#[cfg(test)]
mod prospective_edge_render_tests {
    use super::*;
    use writer_core::starmap::types::reference::StarMapTargetPath;
    use writer_core::starmap::types::{
        StarMapEdge, StarMapEdgeKind, StarMapNode, StarMapNodeKind, StarMapPoint,
        StarMapTargetDetail,
    };

    fn node(id: &str, x: f32) -> StarMapNode {
        StarMapNode {
            id: id.to_string(),
            title: id.to_string(),
            kind: StarMapNodeKind::Note,
            payload: None,
            tags: vec![],
            content: Default::default(),
            anchors: vec![],
            portal: None,
            position: StarMapPoint { x, y: 0.0 },
            style: Default::default(),
            provenance: Default::default(),
            created_at: 0,
            updated_at: 0,
        }
    }

    fn path(node_id: &str) -> StarMapTargetPath {
        StarMapTargetPath {
            starmap_id: "map_1".to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: node_id.to_string(),
            },
        }
    }

    fn edge(id: &str, from: &str, to: &str) -> StarMapEdge {
        StarMapEdge {
            id: id.to_string(),
            from: path(from),
            to: path(to),
            kind: StarMapEdgeKind::RelatedTo,
            label: None,
            payload: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    /// 宿主图已有 A→B；从 B 拉到 A 的候选边 JSON 入口必须返回带上
    /// 双向偏移的 render（与正式边同源），而不是未偏移中线。
    #[test]
    fn prospective_edge_render_json_returns_offset_render_with_reverse_edge() {
        let graph = writer_core::starmap::types::StarMapGraph {
            starmap_id: "map_1".to_string(),
            nodes: vec![node("note_a", 0.0), node("note_b", 400.0)],
            edges: vec![edge("edge_ab", "note_a", "note_b")],
            ..Default::default()
        };
        let nodes = "[{\"id\":\"note_a\",\"x\":0,\"y\":0,\"width\":150,\"height\":60},\
                      {\"id\":\"note_b\",\"x\":400,\"y\":0,\"width\":150,\"height\":60}]";
        let from = serde_json::to_string(&path("note_b")).unwrap();
        let to = serde_json::to_string(&path("note_a")).unwrap();

        let raw = compute_prospective_edge_render_json(&graph, nodes, "[]", &from, &to);
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["success"], true, "{raw}");
        assert_eq!(v["data"]["hasBidirectional"], true, "{raw}");
        let start_y = v["data"]["startY"].as_f64().unwrap();
        let end_y = v["data"]["endY"].as_f64().unwrap();
        assert!(
            (start_y - 18.0).abs() < 0.01 && (end_y - 18.0).abs() < 0.01,
            "已有反向边时候选边必须带上 12 world 垂直偏移（y=18）, got=({start_y},{end_y})"
        );
    }

    /// 端点无法在宿主图定位：success + null（QML 退回自由预览），不是错误。
    #[test]
    fn prospective_edge_render_json_returns_null_for_unresolvable_endpoint() {
        let graph = writer_core::starmap::types::StarMapGraph {
            starmap_id: "map_1".to_string(),
            nodes: vec![node("note_a", 0.0)],
            edges: vec![],
            ..Default::default()
        };
        let nodes = "[{\"id\":\"note_a\",\"x\":0,\"y\":0,\"width\":150,\"height\":60}]";
        let from = serde_json::to_string(&path("note_a")).unwrap();
        let to = serde_json::to_string(&path("note_missing")).unwrap();

        let raw = compute_prospective_edge_render_json(&graph, nodes, "[]", &from, &to);
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["success"], true, "{raw}");
        assert!(v["data"].is_null(), "{raw}");
    }
}
