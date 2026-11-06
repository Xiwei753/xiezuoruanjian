//! 星图对象操作的可回放增量。
//!
//! 章节正文用 [`super::proof::ProofEditPayload`] 记录字符级变化；星图没有
//! 「正文」唯一载体，一次操作可能同时改变节点文字、布局坐标、边端点、嵌入
//! 宿主关系和图级链接。本模块把一次成功写入前后的**完整对象状态**保存为
//! [`StarMapProofDelta`]，回放时按真实对象 ID 依次落盘，而不是事后按时间戳
//! 重新排序猜测操作顺序。
//!
//! 增量只由 Core 业务写入口在持久化成功后生成；平台端不能拼装或改写。

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::starmap::types::{
    StarMapEdge, StarMapEmbed, StarMapGraph, StarMapLink, StarMapNode, StarMapPoint,
};
use crate::starmap::StarMapMeta;
use crate::writing_stats::contribution::{ContributionObjectType, ContributionOperation};

/// 星图证明增量格式版本。旧事件没有该字段时按「无回放数据」处理。
pub const STARMAP_PROOF_FORMAT_VERSION: u32 = 1;

/// 拖动轨迹的一个有界关键点。
///
/// 轨迹只是显示辅助，不参与对象最终状态计算；最终坐标始终来自真实持久化
/// 的节点/嵌入 position。平台端可以在拖动过程中抽样，但必须限制数量。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapProofTrajectorySample {
    pub x: f64,
    pub y: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_ms: Option<u64>,
}

impl From<StarMapPoint> for StarMapProofTrajectorySample {
    fn from(point: StarMapPoint) -> Self {
        Self {
            x: point.x,
            y: point.y,
            offset_ms: None,
        }
    }
}

/// 星图对象在某一时刻的完整真实状态。
///
/// 使用带 `kind` 标签的枚举，避免把节点、边、嵌入都压成同一种「文本」而
/// 丢失端点、宿主路径等语义。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StarMapProofObject {
    Meta { meta: StarMapMeta },
    Node { node: StarMapNode },
    Edge { edge: StarMapEdge },
    Embed { embed: StarMapEmbed },
    Link { link: StarMapLink },
}

/// 第一次在旧星图上记录回放事件时保存的真实图基线。
///
/// 旧星图不是从空白创建出来的，因此基线必须明确标记 `history_complete =
/// false`，回放页显示「此前历史不完整」，不能伪造从空白到成型的全过程。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapProofBaseline {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<StarMapMeta>,
    pub graph: StarMapGraph,
    #[serde(default)]
    pub root_starmap_ids: Vec<String>,
}

impl StarMapProofBaseline {
    pub fn state(&self) -> StarMapProofState {
        StarMapProofState {
            meta: self.meta.clone(),
            graph: self.graph.clone(),
            root_starmap_ids: self.root_starmap_ids.clone(),
        }
    }
}

/// 一条对象操作的可回放增量。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapProofDelta {
    #[serde(default = "default_format_version")]
    pub format_version: u32,
    pub starmap_id: String,
    pub object_type: ContributionObjectType,
    pub object_id: String,
    pub operation: ContributionOperation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<StarMapProofObject>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<StarMapProofObject>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trajectory: Vec<StarMapProofTrajectorySample>,
    /// 首次记录时由业务写入口附带的真实图基线；之后的增量不再重复携带。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<StarMapProofBaseline>,
    /// 创建/删除星图时根星图集合的真实变化。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roots_after: Option<Vec<String>>,
}

const fn default_format_version() -> u32 {
    STARMAP_PROOF_FORMAT_VERSION
}

impl StarMapProofDelta {
    pub fn for_object(
        starmap_id: &str,
        object_type: ContributionObjectType,
        object_id: &str,
        operation: ContributionOperation,
        before_graph: Option<&StarMapGraph>,
        after_graph: Option<&StarMapGraph>,
    ) -> Self {
        Self {
            format_version: STARMAP_PROOF_FORMAT_VERSION,
            starmap_id: starmap_id.to_string(),
            object_type,
            object_id: object_id.to_string(),
            operation,
            before: before_graph
                .and_then(|graph| proof_object_from_graph(graph, object_type, object_id)),
            after: after_graph
                .and_then(|graph| proof_object_from_graph(graph, object_type, object_id)),
            trajectory: Vec::new(),
            baseline: None,
            roots_after: None,
        }
    }

    pub fn for_meta(
        starmap_id: &str,
        operation: ContributionOperation,
        before: Option<&StarMapMeta>,
        after: Option<&StarMapMeta>,
    ) -> Self {
        Self {
            format_version: STARMAP_PROOF_FORMAT_VERSION,
            starmap_id: starmap_id.to_string(),
            object_type: ContributionObjectType::StarMap,
            object_id: starmap_id.to_string(),
            operation,
            before: before.map(|meta| StarMapProofObject::Meta { meta: meta.clone() }),
            after: after.map(|meta| StarMapProofObject::Meta { meta: meta.clone() }),
            trajectory: Vec::new(),
            baseline: None,
            roots_after: None,
        }
    }

    pub fn with_trajectory(mut self, trajectory: Vec<StarMapProofTrajectorySample>) -> Self {
        self.trajectory = trajectory;
        self
    }

    pub fn with_baseline(mut self, baseline: StarMapProofBaseline) -> Self {
        self.baseline = Some(baseline);
        self
    }

    pub fn with_roots_after(mut self, roots: Vec<String>) -> Self {
        self.roots_after = Some(roots);
        self
    }

    pub fn is_valid(&self) -> bool {
        self.format_version == STARMAP_PROOF_FORMAT_VERSION
            && !self.starmap_id.is_empty()
            && !self.object_id.is_empty()
    }
}

/// 从真实图快照中提取指定对象的完整状态。
pub fn proof_object_from_graph(
    graph: &StarMapGraph,
    object_type: ContributionObjectType,
    object_id: &str,
) -> Option<StarMapProofObject> {
    match object_type {
        ContributionObjectType::StarMapNode => graph
            .nodes
            .iter()
            .find(|node| node.id == object_id)
            .cloned()
            .map(|node| StarMapProofObject::Node { node }),
        ContributionObjectType::StarMapEdge => graph
            .edges
            .iter()
            .find(|edge| edge.id == object_id)
            .cloned()
            .map(|edge| StarMapProofObject::Edge { edge }),
        ContributionObjectType::StarMapEmbed => graph
            .embeds
            .iter()
            .find(|embed| embed.instance_id == object_id)
            .cloned()
            .map(|embed| StarMapProofObject::Embed { embed }),
        ContributionObjectType::StarMapLink => graph
            .links
            .iter()
            .find(|link| link.link_id == object_id)
            .cloned()
            .map(|link| StarMapProofObject::Link { link }),
        _ => None,
    }
}

/// 回放状态：一张星图在某个事件位置上的完整业务数据。
///
/// 只包含 Core 拥有的图数据；节点渲染、相机、动画由平台端负责。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapProofState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<StarMapMeta>,
    pub graph: StarMapGraph,
    #[serde(default)]
    pub root_starmap_ids: Vec<String>,
}

impl StarMapProofState {
    pub fn empty(starmap_id: &str) -> Self {
        let mut graph = StarMapGraph::default();
        graph.starmap_id = starmap_id.to_string();
        Self {
            meta: None,
            graph,
            root_starmap_ids: Vec::new(),
        }
    }

    /// 应用一条真实对象增量。删除对象时按真实 ID 移除，不做时间推断。
    pub fn apply_delta(&mut self, delta: &StarMapProofDelta) -> Result<()> {
        if !delta.is_valid() {
            return Err(Error::Other(format!(
                "unsupported starmap proof delta format: {}",
                delta.format_version
            )));
        }
        if !self.graph.starmap_id.is_empty()
            && !delta.starmap_id.is_empty()
            && self.graph.starmap_id != delta.starmap_id
        {
            return Err(Error::Other(format!(
                "starmap proof delta belongs to {} but state is {}",
                delta.starmap_id, self.graph.starmap_id
            )));
        }
        if self.graph.starmap_id.is_empty() {
            self.graph.starmap_id = delta.starmap_id.clone();
        }
        if let Some(roots) = &delta.roots_after {
            self.root_starmap_ids = roots.clone();
        }

        match &delta.after {
            Some(StarMapProofObject::Meta { meta }) => {
                self.meta = Some(meta.clone());
                self.graph.starmap_id = meta.starmap_id.clone();
            }
            Some(StarMapProofObject::Node { node }) => {
                upsert_node(&mut self.graph, node.clone());
            }
            Some(StarMapProofObject::Edge { edge }) => {
                upsert_edge(&mut self.graph, edge.clone());
            }
            Some(StarMapProofObject::Embed { embed }) => {
                upsert_embed(&mut self.graph, embed.clone());
            }
            Some(StarMapProofObject::Link { link }) => {
                upsert_link(&mut self.graph, link.clone());
            }
            None => remove_object(self, delta.object_type, &delta.object_id),
        }
        Ok(())
    }
}

fn upsert_node(graph: &mut StarMapGraph, node: StarMapNode) {
    if let Some(existing) = graph.nodes.iter_mut().find(|item| item.id == node.id) {
        *existing = node;
    } else {
        graph.nodes.push(node);
    }
}

fn upsert_edge(graph: &mut StarMapGraph, edge: StarMapEdge) {
    if let Some(existing) = graph.edges.iter_mut().find(|item| item.id == edge.id) {
        *existing = edge;
    } else {
        graph.edges.push(edge);
    }
}

fn upsert_embed(graph: &mut StarMapGraph, embed: StarMapEmbed) {
    if let Some(existing) = graph
        .embeds
        .iter_mut()
        .find(|item| item.instance_id == embed.instance_id)
    {
        *existing = embed;
    } else {
        graph.embeds.push(embed);
    }
}

fn upsert_link(graph: &mut StarMapGraph, link: StarMapLink) {
    if let Some(existing) = graph
        .links
        .iter_mut()
        .find(|item| item.link_id == link.link_id)
    {
        *existing = link;
    } else {
        graph.links.push(link);
    }
}

fn remove_object(
    state: &mut StarMapProofState,
    object_type: ContributionObjectType,
    object_id: &str,
) {
    match object_type {
        ContributionObjectType::StarMap => {
            state.meta = None;
            // 星图删除后，后续对象删除事件仍按真实 ID 依次到达；这里清空图
            // 是删除星图本身的事实结果，不回放旧内容。
            let starmap_id = state.graph.starmap_id.clone();
            state.graph = StarMapGraph::default();
            state.graph.starmap_id = starmap_id;
            state.root_starmap_ids.clear();
        }
        ContributionObjectType::StarMapNode => {
            state.graph.nodes.retain(|item| item.id != object_id);
        }
        ContributionObjectType::StarMapEdge => {
            state.graph.edges.retain(|item| item.id != object_id);
        }
        ContributionObjectType::StarMapEmbed => {
            state
                .graph
                .embeds
                .retain(|item| item.instance_id != object_id);
        }
        ContributionObjectType::StarMapLink => {
            state.graph.links.retain(|item| item.link_id != object_id);
        }
        _ => {}
    }
}

/// 状态内容摘要，用于检查点/导出包校验。
pub fn state_sha256(state: &StarMapProofState) -> String {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(state).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
