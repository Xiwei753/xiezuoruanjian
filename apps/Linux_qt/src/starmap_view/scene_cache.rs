// =============================================================================
// starmap_view/scene_cache.rs — 星图场景显示缓存（Linux 平台端）
// =============================================================================
//
// 层级：Linux_qt 平台层显示几何缓存。
//
// 职责：
//   一张活动画布只保留一份已解析的 graph / 节点布局 / Embed 包围盒 / 边几何。
//   拖拽节点时只把被拖动对象的临时位置作为 override 传入，后端不再每帧把整张
//   graph + 全部节点 + 全部 Embed 序列化后重新解析；节点/边命中测试也直接查
//   这份内存索引，不再从 QML 往返 JSON。
//
// 归属：
//   - 缓存只属于显示层，不复制 Core 业务状态。结构/位置变化由 QML 在
//     loadGraph / 落盘成功后显式 sync；缓存不自行写 Core。
//   - 边几何仍由 edge_render 计算，命中仍由 hittest 计算；本文件只做组合与
//     生命周期管理，不新增第二套算法。
// =============================================================================

use writer_core::starmap::types::StarMapGraph;

use super::edge_render::{self, EdgeRender, EdgeRenderParams};
use super::hittest;
use super::layout_types::{StarMapEmbedSceneRect, StarMapLayout, StarMapLayoutNode};

const DEFAULT_NODE_RADIUS: f32 = 30.0;

/// 拖拽中的临时位置。只有被拖动物体本身改变，其余对象复用缓存布局。
#[derive(Debug, Clone, PartialEq)]
pub enum SceneMoveOverride {
    Node { id: String, x: f32, y: f32 },
    Embed { id: String, x: f32, y: f32 },
}

#[derive(Default)]
pub struct SceneCache {
    graph: Option<StarMapGraph>,
    layout: StarMapLayout,
    embeds: Vec<StarMapEmbedSceneRect>,
    edge_renders: Vec<EdgeRender>,
    /// 上一帧拖拽对象：(kind, id, 当前中心)。连续拖拽时用上一帧中心定位
    /// 相连边，避免用规范位置匹配已经移动过的缓存几何。
    last_move: Option<(String, String, f32, f32)>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NodePos {
    id: String,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl SceneCache {
    fn is_empty(&self) -> bool {
        self.graph.is_none()
    }

    /// 结构 / 位置 / 尺寸快照变化时重建显示缓存。
    ///
    /// `nodes_json` / `embeds_json` 与 compute_edge_renders 的 scene geometry
    /// 输入完全同形，避免 QML 侧为缓存再拼一份几何。
    pub fn sync(
        &mut self,
        graph_json: &str,
        nodes_json: &str,
        embeds_json: &str,
    ) -> Result<(), String> {
        let graph_dto: writer_core::api::types::StarMapGraphDto =
            serde_json::from_str(graph_json).map_err(|e| format!("Invalid graph JSON: {}", e))?;
        let graph = <StarMapGraph as std::convert::TryFrom<_>>::try_from(graph_dto)
            .map_err(|e| e.to_string())?;

        let nodes: Vec<NodePos> =
            serde_json::from_str(nodes_json).map_err(|e| format!("Invalid nodes JSON: {}", e))?;
        let layout = StarMapLayout {
            kind: super::layout_types::StarMapLayoutKind::Freeform,
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
        let embeds: Vec<StarMapEmbedSceneRect> =
            serde_json::from_str(embeds_json).map_err(|e| format!("Invalid embeds JSON: {}", e))?;

        self.graph = Some(graph);
        self.layout = layout;
        self.embeds = embeds;
        self.edge_renders.clear();
        self.last_move = None;
        Ok(())
    }

    /// 以稳定缓存为准计算全部边几何；拖拽 override 只改一个对象，并且只重算
    /// 与该对象相连的边，其余边直接复用上一次的稳定几何。
    pub fn edge_renders(&mut self, move_override: Option<&SceneMoveOverride>) -> Vec<EdgeRender> {
        let Some(graph) = self.graph.as_ref() else {
            return vec![];
        };

        if move_override.is_none() && !self.edge_renders.is_empty() {
            self.last_move = None;
            return self.edge_renders.clone();
        }

        let mut layout = self.layout.clone();
        let mut embeds = self.embeds.clone();
        let base_center = match move_override {
            Some(SceneMoveOverride::Node { id, .. }) => layout
                .nodes
                .iter()
                .find(|n| &n.node_id == id)
                .map(|n| (n.x + n.width / 2.0, n.y + n.height / 2.0)),
            Some(SceneMoveOverride::Embed { id, .. }) => embeds
                .iter()
                .find(|e| &e.instance_id == id)
                .map(|e| (e.x + e.width / 2.0, e.y + e.height / 2.0)),
            None => None,
        };
        // 连续拖拽时，缓存里的边端点已经停在上一帧位置；用上一帧中心匹配，
        // 不能用规范位置，否则第二帧起会找不到相连边而退回全量计算。
        let previous_center = match (move_override, &self.last_move) {
            (Some(SceneMoveOverride::Node { id, .. }), Some((kind, moved_id, x, y)))
                if kind == "node" && moved_id == id =>
            {
                Some((*x, *y))
            }
            (Some(SceneMoveOverride::Embed { id, .. }), Some((kind, moved_id, x, y)))
                if kind == "embed" && moved_id == id =>
            {
                Some((*x, *y))
            }
            _ => base_center,
        };
        if let Some(over) = move_override {
            match over {
                SceneMoveOverride::Node { id, x, y } => {
                    if let Some(node) = layout.nodes.iter_mut().find(|n| &n.node_id == id) {
                        node.x = *x;
                        node.y = *y;
                    }
                }
                SceneMoveOverride::Embed { id, x, y } => {
                    if let Some(embed) = embeds.iter_mut().find(|e| &e.instance_id == id) {
                        embed.x = *x;
                        embed.y = *y;
                    }
                }
            }
        }
        let new_center = match move_override {
            Some(SceneMoveOverride::Node { id, .. }) => layout
                .nodes
                .iter()
                .find(|n| &n.node_id == id)
                .map(|n| (n.x + n.width / 2.0, n.y + n.height / 2.0)),
            Some(SceneMoveOverride::Embed { id, .. }) => embeds
                .iter()
                .find(|e| &e.instance_id == id)
                .map(|e| (e.x + e.width / 2.0, e.y + e.height / 2.0)),
            None => None,
        };
        self.last_move = match (move_override, new_center) {
            (Some(SceneMoveOverride::Node { id, .. }), Some((cx, cy))) => {
                Some(("node".to_string(), id.clone(), cx, cy))
            }
            (Some(SceneMoveOverride::Embed { id, .. }), Some((cx, cy))) => {
                Some(("embed".to_string(), id.clone(), cx, cy))
            }
            _ => None,
        };

        // 拖拽时只重算端点中心与移动对象旧中心重合的边。双向边的两条记录
        // 共享同一端点中心，因此会一起被包含；其余边的几何保持不变。
        if let (Some((cx, cy)), false) = (previous_center, self.edge_renders.is_empty()) {
            let near = |x: f32, y: f32| (x - cx).abs() <= 0.75 && (y - cy).abs() <= 0.75;
            let connected: std::collections::HashSet<&str> = self
                .edge_renders
                .iter()
                .filter(|r| near(r.from_cx, r.from_cy) || near(r.to_cx, r.to_cy))
                .map(|r| r.edge_id.as_str())
                .collect();
            if !connected.is_empty() {
                let edges: Vec<_> = graph
                    .edges
                    .iter()
                    .filter(|e| connected.contains(e.id.as_str()))
                    .cloned()
                    .collect();
                let batch = edge_render::compute_edge_renders_from_paths(
                    &edges,
                    graph,
                    &layout,
                    &embeds,
                    &EdgeRenderParams::default(),
                );
                let mut merged = self.edge_renders.clone();
                for render in batch.renders {
                    if let Some(slot) = merged.iter_mut().find(|r| r.edge_id == render.edge_id) {
                        *slot = render;
                    } else {
                        merged.push(render);
                    }
                }
                self.edge_renders = merged.clone();
                return merged;
            }
        }

        let batch = edge_render::compute_edge_renders_from_paths(
            &graph.edges,
            graph,
            &layout,
            &embeds,
            &EdgeRenderParams::default(),
        );
        if !batch.diagnostics.is_empty() {
            log::debug!("scene_cache edge diagnostics: {:?}", batch.diagnostics);
        }
        self.edge_renders = batch.renders.clone();
        batch.renders
    }

    pub fn hit_node(&self, x: f32, y: f32) -> Option<String> {
        hittest::hit_test_nodes(x, y, &self.layout.nodes).map(|hit| hit.id)
    }

    pub fn hit_edge(&self, x: f32, y: f32, threshold: f32) -> Option<String> {
        // 命中必须基于当前显示几何。拖拽中 QML 会先刷新 edge_renders，
        // 所以这里直接用最近一次计算结果；没有缓存时按规范布局算一次。
        edge_render::hit_test_edge_renders_with_threshold(x, y, &self.edge_renders, threshold)
    }

    pub fn prospective_edge_render(
        &self,
        from_path_json: &str,
        to_path_json: &str,
    ) -> Result<Option<EdgeRender>, String> {
        let Some(graph) = self.graph.as_ref() else {
            return Err("Star map scene cache is not initialized".to_string());
        };
        let from_path: writer_core::starmap::types::reference::StarMapTargetPath =
            serde_json::from_str(from_path_json)
                .map_err(|e| format!("Invalid from path JSON: {}", e))?;
        let to_path: writer_core::starmap::types::reference::StarMapTargetPath =
            serde_json::from_str(to_path_json)
                .map_err(|e| format!("Invalid to path JSON: {}", e))?;
        match edge_render::compute_prospective_edge_render(
            &from_path,
            &to_path,
            graph,
            &self.layout,
            &self.embeds,
            &EdgeRenderParams::default(),
        ) {
            Ok(render) => Ok(Some(render)),
            Err(diag) => {
                log::debug!("scene_cache prospective diagnostic: {:?}", diag);
                Ok(None)
            }
        }
    }

    /// 清空缓存（切图 / 结构变化导致旧索引失效时）。
    pub fn invalidate(&mut self) {
        self.graph = None;
        self.layout = StarMapLayout::default();
        self.embeds.clear();
        self.edge_renders.clear();
        self.last_move = None;
    }

    pub fn is_ready(&self) -> bool {
        !self.is_empty()
    }
}
