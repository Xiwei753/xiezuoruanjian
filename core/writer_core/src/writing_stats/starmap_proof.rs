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
use crate::writing_stats::contribution::{
    ContributionObjectType, ContributionOperation, ContributionTextInputKind, MutationActor,
    MutationEntrypoint,
};

/// 星图证明增量格式版本。
///
/// v2 起事件哈希覆盖来源字段，检查点锚定事件链头。旧事件缺版本字段时按
/// [`LEGACY_STARMAP_PROOF_FORMAT_VERSION`] 读取，不能静默套用新算法。
pub const STARMAP_PROOF_FORMAT_VERSION: u32 = 3;
pub const LEGACY_V2_STARMAP_PROOF_FORMAT_VERSION: u32 = 2;
pub const LEGACY_STARMAP_PROOF_FORMAT_VERSION: u32 = 1;

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
    /// 从空白创建开始时为 true；旧星图补录基线固定为 false。
    #[serde(default)]
    pub history_complete: bool,
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
    /// 同一来源分支上一条回放事件的哈希；分支首条为空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_event_hash: Option<String>,
    /// 本事件哈希：prev + event_id + position + delta 稳定字段。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_hash: Option<String>,
    /// delta 前/后对象状态的 sha256，用于回放时核对真实对象是否被外部改写。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_state_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_state_hash: Option<String>,
}

const fn default_format_version() -> u32 {
    // 旧 JSON 没有版本字段时必须是 v1；新写入由构造器显式写 v2。
    LEGACY_STARMAP_PROOF_FORMAT_VERSION
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
            prev_event_hash: None,
            event_hash: None,
            before_state_hash: None,
            after_state_hash: None,
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
            prev_event_hash: None,
            event_hash: None,
            before_state_hash: None,
            after_state_hash: None,
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
        matches!(
            self.format_version,
            LEGACY_STARMAP_PROOF_FORMAT_VERSION
                | LEGACY_V2_STARMAP_PROOF_FORMAT_VERSION
                | STARMAP_PROOF_FORMAT_VERSION
        ) && !self.starmap_id.is_empty()
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
            // 星图删除后，后续对象删除事件仍按真实 ID 依次到达；这里只清
            // 元数据与根身份，保留图对象让对象删除事件继续核对 before 状态。
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

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// 单个对象状态摘要；`None` 表示该对象在该时刻不存在。
pub fn proof_object_sha256(object: Option<&StarMapProofObject>) -> String {
    match object {
        Some(object) => sha256_hex(&serde_json::to_vec(object).unwrap_or_default()),
        None => sha256_hex(b"missing"),
    }
}

/// 从回放状态取出 delta 目标对象的当前完整状态。
pub fn state_object_for_delta(
    state: &StarMapProofState,
    delta: &StarMapProofDelta,
) -> Option<StarMapProofObject> {
    proof_object_from_graph(&state.graph, delta.object_type, &delta.object_id).or_else(|| {
        if delta.object_type == ContributionObjectType::StarMap {
            state
                .meta
                .clone()
                .map(|meta| StarMapProofObject::Meta { meta })
        } else {
            None
        }
    })
}

/// 一条回放事件的不可变来源字段。
///
/// 哈希必须覆盖这些字段：同步、导入或外部改写只要篡改来源、时间、项目或
/// 事务，回放校验就会失败。`branch_position` 是「星图 + 来源设备分支」内
/// 已应用事件数，不是设备级排序序号。
#[derive(Debug, Clone, Copy)]
pub struct ReplayEventIdentity<'a> {
    pub event_id: &'a str,
    pub actor: MutationActor,
    pub entrypoint: MutationEntrypoint,
    pub occurred_at_ms: i64,
    pub origin_device_id: &'a str,
    pub project_id: &'a str,
    pub transaction_id: &'a str,
    pub replay_order: Option<u32>,
    pub text_input_kind: Option<ContributionTextInputKind>,
    pub platform: &'a str,
    pub device_class: &'a str,
    pub branch_position: u64,
}

/// 计算一条回放事件的稳定哈希（v3）。
///
/// 覆盖业务事实、来源平台/设备类别、编辑提交方式和事务顺序；同步搬运后
/// 这些字段原样保留，篡改任何一项都会使链校验失败。
pub fn compute_replay_event_hash(
    prev_event_hash: &str,
    identity: &ReplayEventIdentity<'_>,
    delta: &StarMapProofDelta,
) -> String {
    let payload = serde_json::json!({
        "version": STARMAP_PROOF_FORMAT_VERSION,
        "prev": prev_event_hash,
        "eventId": identity.event_id,
        "actor": identity.actor.as_str(),
        "entrypoint": identity.entrypoint.as_str(),
        "occurredAtMs": identity.occurred_at_ms,
        "originDeviceId": identity.origin_device_id,
        "projectId": identity.project_id,
        "transactionId": identity.transaction_id,
        "replayOrder": identity.replay_order,
        "textInputKind": identity.text_input_kind,
        "platform": identity.platform,
        "deviceClass": identity.device_class,
        "position": identity.branch_position,
        "starmapId": delta.starmap_id,
        "objectType": delta.object_type,
        "objectId": delta.object_id,
        "operation": delta.operation,
        "before": delta.before,
        "after": delta.after,
        "trajectory": delta.trajectory,
        "baseline": delta.baseline,
        "rootsAfter": delta.roots_after,
        "beforeStateHash": delta.before_state_hash,
        "afterStateHash": delta.after_state_hash,
    });
    sha256_hex(&serde_json::to_vec(&payload).unwrap_or_default())
}

/// v2 旧算法：用于校验已写入的来源字段哈希，不重写历史。
pub fn compute_v2_replay_event_hash(
    prev_event_hash: &str,
    identity: &ReplayEventIdentity<'_>,
    delta: &StarMapProofDelta,
) -> String {
    let payload = serde_json::json!({
        "version": LEGACY_V2_STARMAP_PROOF_FORMAT_VERSION,
        "prev": prev_event_hash,
        "eventId": identity.event_id,
        "actor": identity.actor.as_str(),
        "entrypoint": identity.entrypoint.as_str(),
        "occurredAtMs": identity.occurred_at_ms,
        "originDeviceId": identity.origin_device_id,
        "projectId": identity.project_id,
        "transactionId": identity.transaction_id,
        "position": identity.branch_position,
        "starmapId": delta.starmap_id,
        "objectType": delta.object_type,
        "objectId": delta.object_id,
        "operation": delta.operation,
        "before": delta.before,
        "after": delta.after,
        "trajectory": delta.trajectory,
        "baseline": delta.baseline,
        "rootsAfter": delta.roots_after,
        "beforeStateHash": delta.before_state_hash,
        "afterStateHash": delta.after_state_hash,
    });
    sha256_hex(&serde_json::to_vec(&payload).unwrap_or_default())
}

/// v1 旧算法：只用于读取和校验历史事件，绝不用来重写旧来源。
pub fn compute_legacy_replay_event_hash(
    prev_event_hash: &str,
    event_id: &str,
    replay_position: u64,
    delta: &StarMapProofDelta,
) -> String {
    let payload = serde_json::json!({
        "version": LEGACY_STARMAP_PROOF_FORMAT_VERSION,
        "prev": prev_event_hash,
        "eventId": event_id,
        "position": replay_position,
        "starmapId": delta.starmap_id,
        "objectType": delta.object_type,
        "objectId": delta.object_id,
        "operation": delta.operation,
        "before": delta.before,
        "after": delta.after,
        "trajectory": delta.trajectory,
        "baseline": delta.baseline,
        "rootsAfter": delta.roots_after,
        "beforeStateHash": delta.before_state_hash,
        "afterStateHash": delta.after_state_hash,
    });
    sha256_hex(&serde_json::to_vec(&payload).unwrap_or_default())
}
