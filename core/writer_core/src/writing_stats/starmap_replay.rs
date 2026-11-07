//! 星图手写证明回放。
//!
//! 回放以 `starmapId + deviceId + position` 为坐标：一台设备就是一个独立来源
//! 分支；远端同步只搬运事件，不重写原始设备与操作者。回放帧只返回 Core
//! 拥有的图业务状态和当前事件元数据，节点文字、边、嵌入、相机与动画由各端
//! 自行渲染。回放绝不修改真实工作区里的星图。

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::starmap::types::{StarMapEdge, StarMapEmbed, StarMapGraph, StarMapLink, StarMapNode};
use crate::starmap::StarMapMeta;
use crate::writing_stats::contribution::{
    ContributionEvent, ContributionObjectType, ContributionOperation, ContributionTextInputKind,
    MutationActor, MutationEntrypoint,
};
use crate::writing_stats::contribution_store::ContributionStore;
use crate::writing_stats::starmap_checkpoint::{
    list_checkpoint_headers, load_checkpoint_at, StarMapCheckpointHeader, StarMapProofCheckpoint,
    STARMAP_CHECKPOINT_INTERVAL,
};
use crate::writing_stats::starmap_proof::{
    compute_legacy_replay_event_hash, compute_replay_event_hash, compute_v2_replay_event_hash,
    proof_object_sha256, state_object_for_delta, state_sha256, ReplayEventIdentity,
    StarMapProofObject, StarMapProofState, StarMapProofTrajectorySample,
    LEGACY_STARMAP_PROOF_FORMAT_VERSION, LEGACY_V2_STARMAP_PROOF_FORMAT_VERSION,
    STARMAP_PROOF_FORMAT_VERSION,
};

/// 回放查询。位置 `target_position` 表示「已应用多少个事件」，0 是空白/基线。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapReplayRequest {
    pub starmap_id: String,
    /// 为空时选择事件最多的来源分支，并在返回帧中给出真实分支。
    #[serde(default)]
    pub device_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_position: Option<u64>,
    /// 兼容按事件下标跳转；等价于 `target_position = target_index + 1`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_time_ms: Option<i64>,
    /// 是否在帧中带回拖动轨迹关键点。
    #[serde(default)]
    pub include_trajectory: bool,
}

impl StarMapReplayRequest {
    pub fn for_position(starmap_id: &str, device_id: &str, position: u64) -> Self {
        Self {
            starmap_id: starmap_id.to_string(),
            device_id: device_id.to_string(),
            target_position: Some(position),
            include_trajectory: true,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapReplayBranch {
    pub device_id: String,
    pub event_count: u64,
    pub first_position: u64,
    pub last_position: u64,
    pub first_timestamp_ms: i64,
    pub last_timestamp_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapReplayEventInfo {
    pub event_id: String,
    pub transaction_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_order: Option<u32>,
    pub timestamp_ms: i64,
    pub actor: MutationActor,
    pub entrypoint: MutationEntrypoint,
    pub operation: ContributionOperation,
    pub object_type: ContributionObjectType,
    pub object_id: String,
    /// 整段保存时 UI 必须说明“非逐字输入”，不能冒充手写轨迹。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_input_kind: Option<ContributionTextInputKind>,
}

/// 一帧历史星图状态。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapReplayFrame {
    pub starmap_id: String,
    pub device_id: String,
    /// 本分支实际使用的证明格式；包含旧事件时为 v1，并标记历史不完整。
    pub proof_format_version: u32,
    pub legacy_event_count: u64,
    pub position: u64,
    pub total_events: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<StarMapReplayEventInfo>,
    pub state: StarMapProofState,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trajectory: Vec<StarMapProofTrajectorySample>,
    pub branches: Vec<StarMapReplayBranch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_timestamp_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_timestamp_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_timestamp_ms: Option<i64>,
    pub checkpoint_position: u64,
    pub history_complete: bool,
    pub valid: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

/// 导出的星图证明包。包含历史图结构，UI 必须先提示用户。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapProofExportPackage {
    pub format_version: u32,
    pub exported_at_ms: i64,
    pub starmap_id: String,
    pub device_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<StarMapProofCheckpoint>,
    pub checkpoints: Vec<StarMapProofCheckpoint>,
    pub events: Vec<ContributionEvent>,
    pub final_position: u64,
    pub final_state: StarMapProofState,
    pub final_state_sha256: String,
    pub contains_historical_graph: bool,
    pub history_complete: bool,
    /// 导出时整条事件链/检查点校验错误；无效链不会生成包。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub validation_errors: Vec<String>,
}

fn is_starmap_event(event: &ContributionEvent, starmap_id: &str) -> bool {
    if event.replay_delta.is_none() {
        return false;
    }
    let meta_event =
        event.object_type == ContributionObjectType::StarMap && event.object_id == starmap_id;
    let object_event = event.container_id.as_deref() == Some(starmap_id);
    (meta_event || object_event)
        && event
            .replay_delta
            .as_ref()
            .is_some_and(|delta| delta.starmap_id == starmap_id)
}

fn event_sort_key(event: &ContributionEvent) -> (u8, u64, i64, &str, u32, &str) {
    if let Some(position) = event.replay_position {
        (1, position, 0, "", 0, event.event_id.as_str())
    } else {
        (
            0,
            0,
            event.occurred_at_ms,
            event.transaction_id.as_str(),
            event.replay_order.unwrap_or(u32::MAX),
            event.event_id.as_str(),
        )
    }
}

fn sort_events(events: &mut [ContributionEvent]) {
    events.sort_by(|left, right| event_sort_key(left).cmp(&event_sort_key(right)));
}

fn branches_for(events: &[ContributionEvent]) -> Vec<StarMapReplayBranch> {
    let mut grouped: BTreeMap<String, Vec<&ContributionEvent>> = BTreeMap::new();
    for event in events {
        grouped
            .entry(event.origin_device_id.clone())
            .or_default()
            .push(event);
    }
    let mut branches = Vec::new();
    for (device_id, events) in grouped {
        let first_timestamp_ms = events.iter().map(|event| event.occurred_at_ms).min();
        let last_timestamp_ms = events.iter().map(|event| event.occurred_at_ms).max();
        branches.push(StarMapReplayBranch {
            device_id,
            event_count: events.len() as u64,
            first_position: 0,
            last_position: events.len() as u64,
            first_timestamp_ms: first_timestamp_ms.unwrap_or_default(),
            last_timestamp_ms: last_timestamp_ms.unwrap_or_default(),
        });
    }
    branches.sort_by(|left, right| {
        right
            .event_count
            .cmp(&left.event_count)
            .then_with(|| left.device_id.cmp(&right.device_id))
    });
    branches
}

/// 检查点必须锚定到本分支同一位置的事件链头，不能只信状态字节摘要。
fn expected_event_hash(events: &[ContributionEvent], index: usize, head: &str) -> String {
    let Some(event) = events.get(index) else {
        return String::new();
    };
    let Some(delta) = event.replay_delta.as_ref() else {
        return String::new();
    };
    if delta.format_version == LEGACY_STARMAP_PROOF_FORMAT_VERSION {
        return compute_legacy_replay_event_hash(
            head,
            &event.event_id,
            event.replay_position.unwrap_or(index as u64),
            delta,
        );
    }
    let identity = ReplayEventIdentity {
        event_id: &event.event_id,
        actor: event.actor,
        entrypoint: event.entrypoint,
        occurred_at_ms: event.occurred_at_ms,
        origin_device_id: &event.origin_device_id,
        project_id: &event.project_id,
        transaction_id: &event.transaction_id,
        replay_order: event.replay_order,
        text_input_kind: event.text_input_kind,
        platform: &event.platform,
        device_class: &event.device_class,
        branch_position: index as u64,
    };
    if delta.format_version == LEGACY_V2_STARMAP_PROOF_FORMAT_VERSION {
        compute_v2_replay_event_hash(head, &identity, delta)
    } else {
        compute_replay_event_hash(head, &identity, delta)
    }
}

/// 整条分支的一次性前缀校验结果：哈希链 + 真实对象状态转换。
///
/// 这里只做顺序的轻量校验；完整图状态摘要只在真正恢复/导出某个检查点
/// 时按需重建，避免打开回放时对全部历史检查点重复整图哈希。
struct PrefixValidation {
    /// 0..=N 每个位置是否可以安全跳转。
    valid: Vec<bool>,
    /// 第一个失败位置；无失败时为 `u64::MAX`。
    first_invalid_position: u64,
}

/// 校验器使用的索引式图状态。
///
/// 回放验证不再对完整 `StarMapProofState` 做 Vec 线性查找，也不在每次
/// apply 后序列化整张图。对象按 ID 索引，保留创建顺序；只有真实检查点
/// 位置才按原始 Vec 顺序还原并计算 `state_sha256`。
struct IndexedProofState {
    schema_version: u32,
    starmap_id: String,
    meta: Option<StarMapMeta>,
    nodes: HashMap<String, (u64, StarMapNode)>,
    edges: HashMap<String, (u64, StarMapEdge)>,
    embeds: HashMap<String, (u64, StarMapEmbed)>,
    links: HashMap<String, (u64, StarMapLink)>,
    root_starmap_ids: Vec<String>,
    next_order: u64,
}

impl IndexedProofState {
    fn from_state(state: &StarMapProofState) -> Self {
        let mut indexed = Self {
            schema_version: state.graph.schema_version,
            starmap_id: state.graph.starmap_id.clone(),
            meta: state.meta.clone(),
            nodes: HashMap::new(),
            edges: HashMap::new(),
            embeds: HashMap::new(),
            links: HashMap::new(),
            root_starmap_ids: state.root_starmap_ids.clone(),
            next_order: 0,
        };
        for node in &state.graph.nodes {
            indexed.insert_node(node.clone());
        }
        for edge in &state.graph.edges {
            indexed.insert_edge(edge.clone());
        }
        for embed in &state.graph.embeds {
            indexed.insert_embed(embed.clone());
        }
        for link in &state.graph.links {
            indexed.insert_link(link.clone());
        }
        indexed
    }

    fn take_order(&mut self) -> u64 {
        let order = self.next_order;
        self.next_order = self.next_order.saturating_add(1);
        order
    }

    fn insert_node(&mut self, node: StarMapNode) {
        let order = self
            .nodes
            .get(&node.id)
            .map(|(order, _)| *order)
            .unwrap_or_else(|| self.take_order());
        self.nodes.insert(node.id.clone(), (order, node));
    }

    fn insert_edge(&mut self, edge: StarMapEdge) {
        let order = self
            .edges
            .get(&edge.id)
            .map(|(order, _)| *order)
            .unwrap_or_else(|| self.take_order());
        self.edges.insert(edge.id.clone(), (order, edge));
    }

    fn insert_embed(&mut self, embed: StarMapEmbed) {
        let order = self
            .embeds
            .get(&embed.instance_id)
            .map(|(order, _)| *order)
            .unwrap_or_else(|| self.take_order());
        self.embeds
            .insert(embed.instance_id.clone(), (order, embed));
    }

    fn insert_link(&mut self, link: StarMapLink) {
        let order = self
            .links
            .get(&link.link_id)
            .map(|(order, _)| *order)
            .unwrap_or_else(|| self.take_order());
        self.links.insert(link.link_id.clone(), (order, link));
    }

    fn object_for_delta(
        &self,
        delta: &crate::writing_stats::starmap_proof::StarMapProofDelta,
    ) -> Option<StarMapProofObject> {
        match delta.object_type {
            ContributionObjectType::StarMap => self
                .meta
                .clone()
                .map(|meta| StarMapProofObject::Meta { meta }),
            ContributionObjectType::StarMapNode => self
                .nodes
                .get(&delta.object_id)
                .map(|(_, node)| StarMapProofObject::Node { node: node.clone() }),
            ContributionObjectType::StarMapEdge => self
                .edges
                .get(&delta.object_id)
                .map(|(_, edge)| StarMapProofObject::Edge { edge: edge.clone() }),
            ContributionObjectType::StarMapEmbed => {
                self.embeds
                    .get(&delta.object_id)
                    .map(|(_, embed)| StarMapProofObject::Embed {
                        embed: embed.clone(),
                    })
            }
            ContributionObjectType::StarMapLink => self
                .links
                .get(&delta.object_id)
                .map(|(_, link)| StarMapProofObject::Link { link: link.clone() }),
            _ => None,
        }
    }

    fn apply_delta(
        &mut self,
        delta: &crate::writing_stats::starmap_proof::StarMapProofDelta,
    ) -> Result<()> {
        if !delta.is_valid() {
            return Err(Error::Other(format!(
                "unsupported starmap proof delta format: {}",
                delta.format_version
            )));
        }
        if !self.starmap_id.is_empty()
            && !delta.starmap_id.is_empty()
            && self.starmap_id != delta.starmap_id
        {
            return Err(Error::Other(format!(
                "starmap proof delta belongs to {} but state is {}",
                delta.starmap_id, self.starmap_id
            )));
        }
        if self.starmap_id.is_empty() {
            self.starmap_id = delta.starmap_id.clone();
        }
        if let Some(roots) = &delta.roots_after {
            self.root_starmap_ids = roots.clone();
        }
        match &delta.after {
            Some(StarMapProofObject::Meta { meta }) => {
                self.meta = Some(meta.clone());
                self.starmap_id = meta.starmap_id.clone();
            }
            Some(StarMapProofObject::Node { node }) => self.insert_node(node.clone()),
            Some(StarMapProofObject::Edge { edge }) => self.insert_edge(edge.clone()),
            Some(StarMapProofObject::Embed { embed }) => self.insert_embed(embed.clone()),
            Some(StarMapProofObject::Link { link }) => self.insert_link(link.clone()),
            None => match delta.object_type {
                ContributionObjectType::StarMap => {
                    self.meta = None;
                    self.root_starmap_ids.clear();
                }
                ContributionObjectType::StarMapNode => {
                    self.nodes.remove(&delta.object_id);
                }
                ContributionObjectType::StarMapEdge => {
                    self.edges.remove(&delta.object_id);
                }
                ContributionObjectType::StarMapEmbed => {
                    self.embeds.remove(&delta.object_id);
                }
                ContributionObjectType::StarMapLink => {
                    self.links.remove(&delta.object_id);
                }
                _ => {}
            },
        }
        Ok(())
    }

    fn to_state(&self) -> StarMapProofState {
        let mut nodes: Vec<(u64, StarMapNode)> = self.nodes.values().cloned().collect();
        let mut edges: Vec<(u64, StarMapEdge)> = self.edges.values().cloned().collect();
        let mut embeds: Vec<(u64, StarMapEmbed)> = self.embeds.values().cloned().collect();
        let mut links: Vec<(u64, StarMapLink)> = self.links.values().cloned().collect();
        nodes.sort_by_key(|(order, _)| *order);
        edges.sort_by_key(|(order, _)| *order);
        embeds.sort_by_key(|(order, _)| *order);
        links.sort_by_key(|(order, _)| *order);
        StarMapProofState {
            meta: self.meta.clone(),
            graph: StarMapGraph {
                schema_version: self.schema_version,
                starmap_id: self.starmap_id.clone(),
                nodes: nodes.into_iter().map(|(_, node)| node).collect(),
                edges: edges.into_iter().map(|(_, edge)| edge).collect(),
                embeds: embeds.into_iter().map(|(_, embed)| embed).collect(),
                links: links.into_iter().map(|(_, link)| link).collect(),
            },
            root_starmap_ids: self.root_starmap_ids.clone(),
        }
    }
}

/// 顺序校验整条分支的事件哈希链与对象级 before/after 连续性。
///
/// 该函数不计算任何完整图摘要，打开回放的成本只与事件数量线性相关；
/// 检查点的整图状态匹配在 `validate_checkpoint_at` 中按需进行。
fn validate_event_chain(
    events: &[ContributionEvent],
    baseline: &StarMapProofState,
) -> PrefixValidation {
    let mut valid = Vec::with_capacity(events.len() + 1);
    valid.push(true);
    let mut state = IndexedProofState::from_state(baseline);
    let mut head = String::new();
    let mut first_invalid_position = u64::MAX;
    for (index, event) in events.iter().enumerate() {
        let mut ok;
        let delta = event.replay_delta.as_ref();
        let stored = delta.and_then(|delta| delta.event_hash.as_deref());
        if let Some(delta) = delta {
            if let Some(stored_hash) = stored {
                ok = stored_hash == expected_event_hash(events, index, &head);
                let prev_ok = match delta.prev_event_hash.as_deref() {
                    Some(prev) => prev == head,
                    None => head.is_empty(),
                };
                ok = ok && prev_ok;
            } else {
                ok = false;
            }
            let current_before = state.object_for_delta(delta);
            if let Some(expected) = delta.before_state_hash.as_deref() {
                ok = ok && proof_object_sha256(current_before.as_ref()) == expected;
            }
            if state.apply_delta(delta).is_err() {
                ok = false;
            } else {
                let current_after = state.object_for_delta(delta);
                if let Some(expected) = delta.after_state_hash.as_deref() {
                    ok = ok && proof_object_sha256(current_after.as_ref()) == expected;
                }
            }
        } else {
            ok = false;
        }
        let previous = *valid.last().unwrap_or(&true);
        let cumulative = previous && ok;
        valid.push(cumulative);
        let position = index as u64 + 1;
        if !cumulative && first_invalid_position == u64::MAX {
            first_invalid_position = position;
        }
        head = stored.unwrap_or_default().to_string();
    }
    PrefixValidation {
        valid,
        first_invalid_position,
    }
}

/// 按提交位置维护前缀最大时间：位置 N 可还原当且仅当该前缀里所有事件
/// `occurred_at_ms` 都不晚于目标时间。时间戳倒序不能打乱提交因果序。
fn build_time_index(events: &[ContributionEvent]) -> Vec<i64> {
    let mut prefix_max = Vec::with_capacity(events.len() + 1);
    prefix_max.push(i64::MIN);
    let mut max_timestamp = i64::MIN;
    for event in events {
        max_timestamp = max_timestamp.max(event.occurred_at_ms);
        prefix_max.push(max_timestamp);
    }
    prefix_max
}

/// 轻量检查点头是否锚定到真实事件前缀；不读取整图快照。
fn checkpoint_header_anchors(
    events: &[ContributionEvent],
    validation: &PrefixValidation,
    position: u64,
    event_chain_head: &str,
) -> bool {
    if position == 0 {
        return true;
    }
    let Ok(index) = usize::try_from(position - 1) else {
        return false;
    };
    if !validation
        .valid
        .get(position as usize)
        .copied()
        .unwrap_or(false)
    {
        return false;
    }
    !event_chain_head.is_empty()
        && events
            .get(index)
            .and_then(|event| event.replay_delta.as_ref())
            .and_then(|delta| delta.event_hash.as_deref())
            .is_some_and(|hash| hash == event_chain_head)
}

/// 一次回放会话。向前播放时复用当前状态，只在目标位置回退时从最近检查点重建。
pub struct StarMapReplaySession {
    request: StarMapReplayRequest,
    device_id: String,
    events: Vec<ContributionEvent>,
    branches: Vec<StarMapReplayBranch>,
    checkpoint_headers: Vec<StarMapCheckpointHeader>,
    /// 只有真正恢复某帧时才载入完整快照；命中后用 `Arc` 共享，避免每帧深拷贝。
    checkpoint_cache: HashMap<u64, Arc<StarMapProofCheckpoint>>,
    /// 检查点整图状态是否与真实重放状态一致；按位置缓存结论。
    checkpoint_validation: HashMap<u64, bool>,
    /// 检查点位置集合，连续播放时每帧只做常数时间判断。
    checkpoint_positions: HashSet<u64>,
    /// 完整重放（导出）时在各检查点位置记录的真实状态摘要。
    checkpoint_replay_hashes: HashMap<u64, String>,
    /// 只有完整重放才收集检查点状态摘要。
    collect_checkpoint_hashes: bool,
    /// 位置 0 基线无法被首条已签名事件独立证明时的持久错误。
    baseline_error: Option<String>,
    history_complete: bool,
    cursor_position: u64,
    cursor_state: StarMapProofState,
    cursor_checkpoint_position: u64,
    baseline_state: StarMapProofState,
    chain_head: String,
    legacy_event_count: u64,
    prefix_validation: PrefixValidation,
    time_index: Vec<i64>,
    errors: Vec<String>,
    root: std::path::PathBuf,
}

impl StarMapReplaySession {
    pub fn open(store: &ContributionStore, request: &StarMapReplayRequest) -> Result<Self> {
        if request.starmap_id.trim().is_empty() {
            return Err(Error::Other(
                "starmap replay requires starmap_id".to_string(),
            ));
        }
        let mut all_events: Vec<ContributionEvent> = store
            .load_all_events()?
            .into_iter()
            .filter(|event| is_starmap_event(event, &request.starmap_id))
            .collect();

        let branches = branches_for(&all_events);
        let device_id = if !request.device_id.is_empty() {
            request.device_id.clone()
        } else {
            branches
                .first()
                .map(|branch| branch.device_id.clone())
                .unwrap_or_default()
        };

        let mut events: Vec<ContributionEvent> = all_events
            .drain(..)
            .filter(|event| event.origin_device_id == device_id)
            .collect();
        sort_events(&mut events);

        // 普通打开只读轻量位置索引；整图快照在真正跳到某个位置时才载入。
        let checkpoint_headers = if device_id.is_empty() {
            Vec::new()
        } else {
            list_checkpoint_headers(store.app_data_root(), &device_id, &request.starmap_id)?
        };
        let legacy_event_count = events
            .iter()
            .filter(|event| {
                event.replay_delta.as_ref().is_some_and(|delta| {
                    delta.format_version == LEGACY_STARMAP_PROOF_FORMAT_VERSION
                })
            })
            .count() as u64;

        let mut session = Self {
            request: request.clone(),
            device_id,
            events,
            branches,
            checkpoint_headers,
            checkpoint_cache: HashMap::new(),
            checkpoint_validation: HashMap::new(),
            checkpoint_positions: HashSet::new(),
            checkpoint_replay_hashes: HashMap::new(),
            collect_checkpoint_hashes: false,
            baseline_error: None,
            history_complete: true,
            cursor_position: 0,
            cursor_state: StarMapProofState::empty(&request.starmap_id),
            cursor_checkpoint_position: 0,
            baseline_state: StarMapProofState::empty(&request.starmap_id),
            chain_head: String::new(),
            legacy_event_count,
            prefix_validation: PrefixValidation {
                valid: Vec::new(),
                first_invalid_position: u64::MAX,
            },
            time_index: Vec::new(),
            errors: Vec::new(),
            root: store.app_data_root().to_path_buf(),
        };
        session.initialize_baseline();
        // 打开时只做一次线性事件链校验；检查点整图状态留到实际跳转/导出
        // 时再按需重建，避免历史累积后每次打开都重复哈希全部旧图。
        session.prefix_validation = validate_event_chain(&session.events, &session.baseline_state);
        session.verify_baseline_checkpoint();
        session.time_index = build_time_index(&session.events);
        session.checkpoint_headers.retain(|header| {
            checkpoint_header_anchors(
                &session.events,
                &session.prefix_validation,
                header.position,
                &header.event_chain_head,
            )
        });
        session.checkpoint_positions = session
            .checkpoint_headers
            .iter()
            .map(|header| header.position)
            .collect();
        if session.legacy_event_count > 0 {
            session.history_complete = false;
        }
        Ok(session)
    }

    fn initialize_baseline(&mut self) {
        // 第一条星图事件的 baseline 已参与事件哈希，是唯一可独立证明的起点。
        // 位置 0 的检查点只是它的缓存，必须在首条事件通过哈希链校验后核对。
        if let Some(baseline) = self
            .events
            .first()
            .and_then(|event| event.replay_delta.as_ref())
            .and_then(|delta| delta.baseline.as_ref())
        {
            self.cursor_state = baseline.state();
            self.baseline_state = self.cursor_state.clone();
            self.cursor_checkpoint_position = 0;
            self.history_complete = baseline.history_complete;
            return;
        }
        if self.events.is_empty() {
            return;
        }
        // 旧分支没有已签名的首条 baseline：位置 0 快照只能用于显示，
        // 不能作为可信手写来源。
        self.baseline_error = Some(
            "starmap baseline cannot be independently verified from the signed first event"
                .to_string(),
        );
        self.history_complete = false;
        let mut used_unverified_checkpoint = false;
        if self
            .checkpoint_headers
            .iter()
            .any(|header| header.position == 0)
        {
            if let Ok(Some(checkpoint)) =
                load_checkpoint_at(&self.root, &self.device_id, &self.request.starmap_id, 0)
            {
                self.cursor_state = checkpoint.state.clone();
                self.baseline_state = checkpoint.state.clone();
                self.cursor_checkpoint_position = 0;
                used_unverified_checkpoint = true;
            }
        }
        let starts_with_create = self.events.first().is_some_and(|event| {
            event.object_type == ContributionObjectType::StarMap
                && event.operation == ContributionOperation::Create
        });
        // 跨设备接力：新设备分支没有自己的创建历史时，以其它设备封存的真实
        // 图为基线；但缺少首条签名 baseline 时同样只能用于显示。
        if !used_unverified_checkpoint && !starts_with_create {
            if let Ok(Some(other)) =
                crate::writing_stats::starmap_checkpoint::find_other_branch_baseline(
                    &self.root,
                    &self.request.starmap_id,
                    &self.device_id,
                )
            {
                self.cursor_state = other.state;
                self.baseline_state = self.cursor_state.clone();
                self.cursor_checkpoint_position = 0;
            }
        }
        self.remove_position_zero_checkpoint();
    }

    fn remove_position_zero_checkpoint(&mut self) {
        self.checkpoint_headers
            .retain(|header| header.position != 0);
        self.checkpoint_cache.remove(&0);
        self.checkpoint_validation.remove(&0);
        self.checkpoint_positions.remove(&0);
    }

    /// 首条事件通过哈希链校验后，用其中已签名的 baseline 核对位置 0 快照。
    fn verify_baseline_checkpoint(&mut self) {
        let Some(baseline) = self
            .events
            .first()
            .and_then(|event| event.replay_delta.as_ref())
            .and_then(|delta| delta.baseline.as_ref())
        else {
            return;
        };
        if self.prefix_validation.valid.get(1).copied() != Some(true) {
            self.baseline_error = Some(
                "starmap first-event baseline cannot be verified because the event chain is invalid"
                    .to_string(),
            );
            self.remove_position_zero_checkpoint();
            return;
        }
        if !self
            .checkpoint_headers
            .iter()
            .any(|header| header.position == 0)
        {
            return;
        }
        let expected = state_sha256(&baseline.state());
        match load_checkpoint_at(&self.root, &self.device_id, &self.request.starmap_id, 0) {
            Ok(Some(checkpoint))
                if checkpoint.state_sha256 == expected && checkpoint.hash_matches() =>
            {
                self.checkpoint_cache.insert(0, Arc::new(checkpoint));
                self.checkpoint_validation.insert(0, true);
            }
            _ => {
                self.baseline_error = Some(
                    "starmap baseline checkpoint at position 0 does not match the signed first-event baseline"
                        .to_string(),
                );
                self.remove_position_zero_checkpoint();
            }
        }
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    pub fn branches(&self) -> &[StarMapReplayBranch] {
        &self.branches
    }

    pub fn total_events(&self) -> u64 {
        self.events.len() as u64
    }

    /// 指定回放位置的真实事件链头；position 0 为基线，返回空串。
    pub fn event_chain_head_at(&self, position: u64) -> String {
        self.hash_at_position(position)
    }

    fn resolve_target_position(&self, request: &StarMapReplayRequest) -> u64 {
        let total = self.events.len() as u64;
        if let Some(position) = request.target_position {
            return position.min(total);
        }
        if let Some(index) = request.target_index {
            return (index as u64 + 1).min(total);
        }
        if let Some(time_ms) = request.target_time_ms {
            // 前缀最大时间单调递增；只选「该前缀所有事件都不晚于目标时间」
            // 的最长提交前缀，避免把时间更晚但先提交的操作带进画面。
            let count = self
                .time_index
                .partition_point(|timestamp| *timestamp <= time_ms);
            return (count.saturating_sub(1) as u64).min(total);
        }
        total
    }

    fn latest_checkpoint_position_at_or_before(&self, target: u64) -> Option<u64> {
        self.checkpoint_headers
            .iter()
            .filter(|header| header.position <= target)
            .map(|header| header.position)
            .max()
    }

    /// 只扫描轻量 headers，选出 `(from, to]` 中最近的检查点位置。
    fn latest_checkpoint_position_in_range(&self, from: u64, to: u64) -> Option<u64> {
        self.checkpoint_headers
            .iter()
            .filter(|header| header.position >= from && header.position <= to)
            .map(|header| header.position)
            .max()
    }

    /// 载入检查点整图；缓存用 `Arc` 共享，调用方只在自己需要恢复游标时克隆状态。
    fn load_checkpoint(&mut self, position: u64) -> Option<Arc<StarMapProofCheckpoint>> {
        if let Some(cached) = self.checkpoint_cache.get(&position) {
            return Some(Arc::clone(cached));
        }
        let checkpoint = load_checkpoint_at(
            &self.root,
            &self.device_id,
            &self.request.starmap_id,
            position,
        )
        .ok()
        .flatten()?;
        let checkpoint = Arc::new(checkpoint);
        self.checkpoint_cache
            .insert(position, Arc::clone(&checkpoint));
        Some(checkpoint)
    }

    /// 独立重放事件到 `position`，得到该位置的真实整图摘要。
    ///
    /// 向前验证时复用当前游标状态，只补 `cursor_position..position`；
    /// 向后验证才从基线重放，避免大跨度跳转每次都从 0 开始。
    fn reconstructed_state_hash(&self, position: u64) -> Option<String> {
        if position > self.events.len() as u64 {
            return None;
        }
        if position == 0 {
            return Some(state_sha256(&self.baseline_state));
        }
        let (start, base) = if position >= self.cursor_position {
            (self.cursor_position, &self.cursor_state)
        } else {
            (0, &self.baseline_state)
        };
        let mut state = IndexedProofState::from_state(base);
        for event in &self.events[start as usize..position as usize] {
            let delta = event.replay_delta.as_ref()?;
            state.apply_delta(delta).ok()?;
        }
        Some(state_sha256(&state.to_state()))
    }

    /// 按需校验某个检查点的完整状态，并缓存结论。
    ///
    /// 事件链和对象级连续性由 `open` 一次性完成；这里才比较整图摘要，
    /// 因此只有随机跳转/导出会支付完整状态成本。
    fn validate_checkpoint_at(&mut self, position: u64) -> bool {
        if let Some(valid) = self.checkpoint_validation.get(&position) {
            return *valid;
        }
        let Some(header) = self
            .checkpoint_headers
            .iter()
            .find(|header| header.position == position)
            .cloned()
        else {
            return false;
        };
        let mut valid = checkpoint_header_anchors(
            &self.events,
            &self.prefix_validation,
            header.position,
            &header.event_chain_head,
        );
        if valid {
            valid = self.load_checkpoint(position).is_some_and(|checkpoint| {
                !checkpoint.state_sha256.is_empty()
                    && checkpoint.state_sha256 == header.state_sha256
                    && checkpoint.hash_matches()
            });
        }
        if valid && position > 0 {
            let expected = self
                .checkpoint_replay_hashes
                .get(&position)
                .cloned()
                .or_else(|| self.reconstructed_state_hash(position));
            valid = expected.is_some_and(|hash| hash == header.state_sha256);
        }
        self.checkpoint_validation.insert(position, valid);
        valid
    }

    fn restore_checkpoint(&mut self, position: u64) -> bool {
        if !self.validate_checkpoint_at(position) {
            return false;
        }
        let Some(checkpoint) = self.load_checkpoint(position) else {
            self.checkpoint_validation.insert(position, false);
            return false;
        };
        self.cursor_state = checkpoint.state.clone();
        self.cursor_position = position.min(self.events.len() as u64);
        self.cursor_checkpoint_position = self.cursor_position;
        self.chain_head = self.hash_at_position(self.cursor_position);
        true
    }

    fn reset_to_baseline(&mut self) {
        self.cursor_state = self.baseline_state.clone();
        self.cursor_position = 0;
        self.cursor_checkpoint_position = 0;
        self.chain_head.clear();
    }

    fn reset_cursor_to(&mut self, target: u64) {
        // 重新定位后旧目标的断链错误不再属于当前帧；从检查点/基线重建。
        self.errors.clear();
        if let Some(position) = self.latest_checkpoint_position_at_or_before(target) {
            if self.restore_checkpoint(position) {
                return;
            }
            self.errors.push(format!(
                "starmap checkpoint hash mismatch at position {position}"
            ));
        }
        self.reset_to_baseline();
    }

    fn hash_at_position(&self, position: u64) -> String {
        position
            .checked_sub(1)
            .and_then(|index| self.events.get(index as usize))
            .and_then(|event| event.replay_delta.as_ref())
            .and_then(|delta| delta.event_hash.clone())
            .unwrap_or_default()
    }

    fn apply_to(&mut self, target: u64) {
        if target < self.cursor_position {
            self.reset_cursor_to(target);
        } else if target > self.cursor_position
            && target - self.cursor_position >= STARMAP_CHECKPOINT_INTERVAL
        {
            // 大跨度向前定位：只挑 `(cursor, target]` 中最新的检查点，
            // 校验一次后恢复，再应用剩余增量。相邻小步播放不读完整快照。
            if let Some(position) =
                self.latest_checkpoint_position_in_range(self.cursor_position + 1, target)
            {
                if self.validate_checkpoint_at(position) {
                    self.restore_checkpoint(position);
                }
            }
        }
        // 连续向前播放只需要应用增量事件；既不读取整图快照，也不深拷贝
        // 检查点状态。检查点只用于回退/随机跳转/大跨度前进定位。
        let from = self.cursor_position as usize;
        let to = target as usize;
        for index in from..to {
            let event = self.events[index].clone();
            let Some(delta) = event.replay_delta.clone() else {
                self.errors.push(format!(
                    "event {} has no starmap replay delta",
                    event.event_id
                ));
                continue;
            };
            let legacy = delta.format_version == LEGACY_STARMAP_PROOF_FORMAT_VERSION;
            let v2 = delta.format_version == LEGACY_V2_STARMAP_PROOF_FORMAT_VERSION;
            let positioned = legacy || v2 || event.replay_position.is_some();
            if positioned {
                if let Some(prev) = delta.prev_event_hash.as_deref() {
                    if prev != self.chain_head {
                        self.errors.push(format!(
                            "event {} previous hash does not match branch chain",
                            event.event_id
                        ));
                    }
                } else if !self.chain_head.is_empty() {
                    self.errors.push(format!(
                        "event {} missing previous hash in non-empty branch",
                        event.event_id
                    ));
                }
                let computed = if legacy {
                    // v1 保留原算法与设备级位置，不静默改写历史来源。
                    compute_legacy_replay_event_hash(
                        &self.chain_head,
                        &event.event_id,
                        event.replay_position.unwrap_or(index as u64),
                        &delta,
                    )
                } else {
                    let identity = ReplayEventIdentity {
                        event_id: &event.event_id,
                        actor: event.actor,
                        entrypoint: event.entrypoint,
                        occurred_at_ms: event.occurred_at_ms,
                        origin_device_id: &event.origin_device_id,
                        project_id: &event.project_id,
                        transaction_id: &event.transaction_id,
                        replay_order: event.replay_order,
                        text_input_kind: event.text_input_kind,
                        platform: &event.platform,
                        device_class: &event.device_class,
                        branch_position: index as u64,
                    };
                    if v2 {
                        compute_v2_replay_event_hash(&self.chain_head, &identity, &delta)
                    } else {
                        compute_replay_event_hash(&self.chain_head, &identity, &delta)
                    }
                };
                if delta.event_hash.as_deref() != Some(computed.as_str()) {
                    self.errors.push(format!(
                        "event {} hash mismatch (possible tampering)",
                        event.event_id
                    ));
                }
                let current_before = state_object_for_delta(&self.cursor_state, &delta);
                if let Some(expected) = delta.before_state_hash.as_deref() {
                    if proof_object_sha256(current_before.as_ref()) != expected {
                        self.errors.push(format!(
                            "event {} before state does not match replay state",
                            event.event_id
                        ));
                    }
                }
            }
            if let Err(error) = self.cursor_state.apply_delta(&delta) {
                self.errors.push(format!("{}: {error}", event.event_id));
                continue;
            }
            if positioned {
                let current_after = state_object_for_delta(&self.cursor_state, &delta);
                if let Some(expected) = delta.after_state_hash.as_deref() {
                    if proof_object_sha256(current_after.as_ref()) != expected {
                        self.errors.push(format!(
                            "event {} after state does not match replay state",
                            event.event_id
                        ));
                    }
                }
                self.chain_head = delta.event_hash.clone().unwrap_or_default();
            } else {
                // 完全没有回放元数据的事件不能冒充链的一部分。
                self.chain_head.clear();
            }
            let position = index as u64 + 1;
            if self.collect_checkpoint_hashes
                && self.checkpoint_positions.contains(&position)
                && !self.checkpoint_replay_hashes.contains_key(&position)
            {
                let hash = state_sha256(&self.cursor_state);
                self.checkpoint_replay_hashes.insert(position, hash);
            }
        }
        self.cursor_position = target;
    }

    fn prefix_error_at(&self, target: u64) -> Option<String> {
        let first = self.prefix_validation.first_invalid_position;
        if first == u64::MAX || target < first {
            return None;
        }
        Some(format!("event chain validation failed at position {first}"))
    }

    pub fn frame_at(&mut self, request: &StarMapReplayRequest) -> StarMapReplayFrame {
        let target = self.resolve_target_position(request);
        self.apply_to(target);
        if let Some(error) = self.prefix_error_at(target) {
            if !self.errors.iter().any(|existing| existing == &error) {
                self.errors.push(error);
            }
        }

        let event_info = target
            .checked_sub(1)
            .and_then(|index| self.events.get(index as usize))
            .map(|event| StarMapReplayEventInfo {
                event_id: event.event_id.clone(),
                transaction_id: event.transaction_id.clone(),
                replay_order: event.replay_order,
                timestamp_ms: event.occurred_at_ms,
                actor: event.actor,
                entrypoint: event.entrypoint,
                operation: event.operation,
                object_type: event.object_type,
                object_id: event.object_id.clone(),
                text_input_kind: event.text_input_kind,
            });
        let trajectory = if request.include_trajectory {
            target
                .checked_sub(1)
                .and_then(|index| self.events.get(index as usize))
                .and_then(|event| event.replay_delta.as_ref())
                .map(|delta| delta.trajectory.clone())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let first_timestamp_ms = self.events.first().map(|event| event.occurred_at_ms);
        let last_timestamp_ms = self.events.last().map(|event| event.occurred_at_ms);
        let next_timestamp_ms = self
            .events
            .get(target as usize)
            .map(|event| event.occurred_at_ms);
        let mut errors = self.errors.clone();
        if let Some(error) = &self.baseline_error {
            if !errors.iter().any(|existing| existing == error) {
                errors.push(error.clone());
            }
        }
        let valid = errors.is_empty();
        StarMapReplayFrame {
            starmap_id: self.request.starmap_id.clone(),
            device_id: self.device_id.clone(),
            proof_format_version: if self.legacy_event_count > 0 {
                LEGACY_STARMAP_PROOF_FORMAT_VERSION
            } else {
                STARMAP_PROOF_FORMAT_VERSION
            },
            legacy_event_count: self.legacy_event_count,
            position: target,
            total_events: self.events.len() as u64,
            event: event_info,
            state: self.cursor_state.clone(),
            trajectory,
            branches: self.branches.clone(),
            first_timestamp_ms,
            last_timestamp_ms,
            next_timestamp_ms,
            checkpoint_position: self.cursor_checkpoint_position,
            history_complete: self.history_complete,
            valid,
            errors: std::mem::take(&mut errors),
        }
    }
}

/// 一次性回放目标帧（不保留会话缓存）。
pub fn replay_starmap_proof(
    store: &ContributionStore,
    request: &StarMapReplayRequest,
) -> Result<StarMapReplayFrame> {
    let mut session = StarMapReplaySession::open(store, request)?;
    Ok(session.frame_at(request))
}

/// 导出某设备分支的完整星图证明包。
pub fn export_starmap_proof(
    store: &ContributionStore,
    starmap_id: &str,
    device_id: &str,
) -> Result<StarMapProofExportPackage> {
    let request = StarMapReplayRequest {
        starmap_id: starmap_id.to_string(),
        device_id: device_id.to_string(),
        include_trajectory: true,
        ..StarMapReplayRequest::default()
    };
    let mut session = StarMapReplaySession::open(store, &request)?;
    // 导出从真实基线完整重放整条事件链，保留已验证的库头供导出枚举；
    // 完整重放时顺便记录各检查点位置的真实状态摘要，供随后核对快照。
    session.collect_checkpoint_hashes = true;
    session.reset_to_baseline();
    let target = session.total_events();
    let frame = session.frame_at(&StarMapReplayRequest {
        target_position: Some(target),
        ..request.clone()
    });
    if !frame.valid {
        return Err(Error::Other(format!(
            "cannot export invalid starmap proof for {starmap_id}: {}",
            frame.errors.join("; ")
        )));
    }
    let mut events: Vec<ContributionEvent> = store
        .load_all_events()?
        .into_iter()
        .filter(|event| is_starmap_event(event, starmap_id))
        .filter(|event| event.origin_device_id == frame.device_id)
        .collect();
    sort_events(&mut events);
    let checkpoints = session.validated_checkpoints();
    let baseline = checkpoints
        .iter()
        .find(|checkpoint| checkpoint.position == 0)
        .cloned();
    let baseline_from_event = events
        .first()
        .and_then(|event| event.replay_delta.as_ref())
        .and_then(|delta| delta.baseline.as_ref())
        .cloned();
    let contains_historical_graph = baseline.is_some() || baseline_from_event.is_some();
    let final_state_sha256 = state_sha256(&frame.state);
    Ok(StarMapProofExportPackage {
        format_version: STARMAP_PROOF_FORMAT_VERSION,
        exported_at_ms: chrono::Utc::now().timestamp_millis(),
        starmap_id: starmap_id.to_string(),
        device_id: frame.device_id.clone(),
        baseline,
        checkpoints,
        events,
        final_position: frame.position,
        final_state: frame.state,
        final_state_sha256,
        contains_historical_graph,
        history_complete: frame.history_complete,
        validation_errors: frame.errors,
    })
}

/// 让会话能从检查点文件读取基线（仅测试/显式构造使用）。
impl StarMapReplaySession {
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    /// 只返回哈希链和对象状态转换都通过、且与真实前缀状态摘要一致的检查点。
    ///
    /// 该函数只在导出证明时调用；普通回放打开不会读取全部整图快照。
    pub fn validated_checkpoints(&mut self) -> Vec<StarMapProofCheckpoint> {
        let mut positions: Vec<u64> = self
            .checkpoint_headers
            .iter()
            .map(|header| header.position)
            .collect();
        positions.sort_unstable();
        positions.dedup();
        let mut checkpoints = Vec::new();
        for position in positions {
            if !self.validate_checkpoint_at(position) {
                continue;
            }
            if let Some(checkpoint) = self.load_checkpoint(position) {
                checkpoints.push((*checkpoint).clone());
            }
        }
        checkpoints
    }
}
