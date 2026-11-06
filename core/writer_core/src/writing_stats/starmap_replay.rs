//! 星图手写证明回放。
//!
//! 回放以 `starmapId + deviceId + position` 为坐标：一台设备就是一个独立来源
//! 分支；远端同步只搬运事件，不重写原始设备与操作者。回放帧只返回 Core
//! 拥有的图业务状态和当前事件元数据，节点文字、边、嵌入、相机与动画由各端
//! 自行渲染。回放绝不修改真实工作区里的星图。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::writing_stats::contribution::{
    ContributionEvent, ContributionObjectType, ContributionOperation, MutationActor,
    MutationEntrypoint,
};
use crate::writing_stats::contribution_store::ContributionStore;
use crate::writing_stats::starmap_checkpoint::{
    latest_checkpoint_at_or_before, load_starmap_checkpoints, StarMapProofCheckpoint,
};
use crate::writing_stats::starmap_proof::{
    compute_legacy_replay_event_hash, compute_replay_event_hash, proof_object_sha256,
    state_object_for_delta, state_sha256, ReplayEventIdentity, StarMapProofState,
    StarMapProofTrajectorySample, LEGACY_STARMAP_PROOF_FORMAT_VERSION,
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
fn checkpoint_prefix_matches(
    events: &[ContributionEvent],
    checkpoint: &StarMapProofCheckpoint,
) -> bool {
    if checkpoint.position == 0 {
        return true;
    }
    let Ok(index) = usize::try_from(checkpoint.position - 1) else {
        return false;
    };
    events
        .get(index)
        .and_then(|event| event.replay_delta.as_ref())
        .and_then(|delta| delta.event_hash.as_deref())
        .is_some_and(|hash| checkpoint.anchors_event_chain(hash))
}

/// 一次回放会话。向前播放时复用当前状态，只在目标位置回退时从最近检查点重建。
pub struct StarMapReplaySession {
    request: StarMapReplayRequest,
    device_id: String,
    events: Vec<ContributionEvent>,
    branches: Vec<StarMapReplayBranch>,
    checkpoints: Vec<StarMapProofCheckpoint>,
    history_complete: bool,
    cursor_position: u64,
    cursor_state: StarMapProofState,
    cursor_checkpoint_position: u64,
    baseline_state: StarMapProofState,
    chain_head: String,
    legacy_event_count: u64,
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

        let checkpoints = if device_id.is_empty() {
            Vec::new()
        } else {
            load_starmap_checkpoints(store.app_data_root(), &device_id, &request.starmap_id)?
                .into_iter()
                .filter(|checkpoint| checkpoint_prefix_matches(&events, checkpoint))
                .collect()
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
            checkpoints,
            history_complete: true,
            cursor_position: 0,
            cursor_state: StarMapProofState::empty(&request.starmap_id),
            cursor_checkpoint_position: 0,
            baseline_state: StarMapProofState::empty(&request.starmap_id),
            chain_head: String::new(),
            legacy_event_count,
            errors: Vec::new(),
            root: store.app_data_root().to_path_buf(),
        };
        session.initialize_baseline();
        if session.legacy_event_count > 0 {
            session.history_complete = false;
        }
        Ok(session)
    }

    fn initialize_baseline(&mut self) {
        // 检查点位置 0 是明确保存的基线；没有基线时，如果事件本身携带
        // baseline（旧星图第一次记录），也用它恢复真实起点。
        if let Some(checkpoint) = self
            .checkpoints
            .iter()
            .find(|checkpoint| checkpoint.position == 0)
        {
            self.cursor_state = checkpoint.state.clone();
            self.baseline_state = checkpoint.state.clone();
            self.cursor_checkpoint_position = 0;
            if !checkpoint.history_complete {
                self.history_complete = false;
            }
            return;
        }
        if let Some(baseline) = self
            .events
            .first()
            .and_then(|event| event.replay_delta.as_ref())
            .and_then(|delta| delta.baseline.as_ref())
        {
            self.cursor_state = baseline.state();
            self.baseline_state = self.cursor_state.clone();
            self.cursor_checkpoint_position = 0;
            self.history_complete = false;
            return;
        }
        let starts_with_create = self.events.first().is_some_and(|event| {
            event.object_type == ContributionObjectType::StarMap
                && event.operation == ContributionOperation::Create
        });
        // 跨设备接力：新设备分支没有自己的创建历史时，以其它设备封存的真实
        // 图为基线，并明确标记此前历史不完整。
        if !starts_with_create {
            if let Ok(Some(other)) =
                crate::writing_stats::starmap_checkpoint::find_other_branch_baseline(
                    &self.root,
                    &self.request.starmap_id,
                    &self.device_id,
                )
            {
                // 跨设备基线必须同时成为游标和回退基线；否则向后拖动时
                // reset_cursor_to 会退回空图，把真实历史整段抹掉。
                self.cursor_state = other.state;
                self.baseline_state = self.cursor_state.clone();
                self.cursor_checkpoint_position = 0;
                self.history_complete = false;
                return;
            }
        }
        // 从空白开始：只有第一条事件确实是创建星图时才宣称历史完整。
        if !self.events.is_empty() && !starts_with_create {
            self.history_complete = false;
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

    fn resolve_target_position(&self, request: &StarMapReplayRequest) -> u64 {
        let total = self.events.len() as u64;
        if let Some(position) = request.target_position {
            return position.min(total);
        }
        if let Some(index) = request.target_index {
            return (index as u64 + 1).min(total);
        }
        if let Some(time_ms) = request.target_time_ms {
            return self
                .events
                .partition_point(|event| event.occurred_at_ms <= time_ms)
                as u64;
        }
        total
    }

    fn reset_cursor_to(&mut self, target: u64) {
        let checkpoint = latest_checkpoint_at_or_before(&self.checkpoints, target);
        if let Some(checkpoint) = checkpoint {
            if checkpoint.hash_matches() && checkpoint_prefix_matches(&self.events, checkpoint) {
                self.cursor_state = checkpoint.state.clone();
                self.cursor_position = checkpoint.position.min(self.events.len() as u64);
                self.cursor_checkpoint_position = self.cursor_position;
                self.chain_head = self.hash_at_position(self.cursor_position);
                return;
            }
            self.errors.push(format!(
                "starmap checkpoint hash mismatch at position {}",
                checkpoint.position
            ));
        }
        self.cursor_state = self.baseline_state.clone();
        self.cursor_position = 0;
        self.cursor_checkpoint_position = 0;
        self.chain_head.clear();
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
        } else if target > self.cursor_position {
            // 随机向前跳转时优先复用目标之前的最近检查点；连续向前播放
            // （相邻帧）不会命中这里，只应用新增事件。
            if let Some(checkpoint) = latest_checkpoint_at_or_before(&self.checkpoints, target) {
                if checkpoint.position > self.cursor_position
                    && checkpoint.hash_matches()
                    && checkpoint_prefix_matches(&self.events, checkpoint)
                {
                    self.cursor_state = checkpoint.state.clone();
                    self.cursor_position = checkpoint.position;
                    self.cursor_checkpoint_position = checkpoint.position;
                    self.chain_head = self.hash_at_position(checkpoint.position);
                }
            }
        }
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
            let positioned = legacy || event.replay_position.is_some();
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
                        branch_position: index as u64,
                    };
                    compute_replay_event_hash(&self.chain_head, &identity, &delta)
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
        }
        self.cursor_position = target;
    }

    pub fn frame_at(&mut self, request: &StarMapReplayRequest) -> StarMapReplayFrame {
        let target = self.resolve_target_position(request);
        self.apply_to(target);

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
    let target = session.total_events();
    let frame = session.frame_at(&StarMapReplayRequest {
        target_position: Some(target),
        ..request.clone()
    });
    let mut events: Vec<ContributionEvent> = store
        .load_all_events()?
        .into_iter()
        .filter(|event| is_starmap_event(event, starmap_id))
        .filter(|event| event.origin_device_id == frame.device_id)
        .collect();
    sort_events(&mut events);
    let checkpoints: Vec<StarMapProofCheckpoint> =
        load_starmap_checkpoints(store.app_data_root(), &frame.device_id, starmap_id)?
            .into_iter()
            .filter(|checkpoint| checkpoint_prefix_matches(&events, checkpoint))
            .collect();
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
    })
}

/// 让会话能从检查点文件读取基线（仅测试/显式构造使用）。
impl StarMapReplaySession {
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }
}
