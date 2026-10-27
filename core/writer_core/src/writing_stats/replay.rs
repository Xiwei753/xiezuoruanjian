//! 手写证明回放：按章节、设备和事件位置重建任意时刻正文。
//!
//! Core 负责验证事件链、应用实际变更和给出回放状态；平台只渲染正文、
//! 光标与进度，不自行推演编辑规则。回放优先从最近的完整正文 checkpoint
//! 开始，再局部应用后续事件。
//!
//! 设备内 `seq` 是全局递增的，章节之间会交错；播放器的时间轴使用
//! 章节分支内的事件位置 `position`（0..=total_events），全局 `seq` 只用于
//! 数据链校验。连续向前播放时复用内存会话，只应用下一条事件。

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::editor::kernel::ContentDigest;
use crate::writing_stats::checkpoint::{self, ProofCheckpoint};
use crate::writing_stats::proof::{
    apply_changes, chain_content_hash, compute_event_hash, content_hash_of_text,
    text_digest_matches, text_sha256,
};
use crate::writing_stats::store::StatsStore;
use crate::writing_stats::{
    EventSource, WritingActivitySegment, WritingInputEvent,
};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofReplayRequest {
    pub chapter_id: String,
    /// 为空时优先本机设备，其次选择该章节第一条有证明的事件来源。
    #[serde(default)]
    pub device_id: String,
    /// 目标设备内全局序号；与 `target_index` 同时给出时以位置为准。
    #[serde(default)]
    pub target_seq: Option<u64>,
    /// 目标发生时间（Unix 毫秒）；与 target_seq 同时给出时两者都作为上界。
    #[serde(default)]
    pub target_time_ms: Option<i64>,
    /// 章节分支内的事件位置：0 表示第一条事件之前，N 表示应用完第 N 条。
    #[serde(default)]
    pub target_index: Option<usize>,
    /// 没有 checkpoint 时使用的正文基线（旧会话/新章节）。
    #[serde(default)]
    pub baseline_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofReplayFrame {
    pub chapter_id: String,
    pub device_id: String,
    /// 当前帧对应事件的设备全局序号；position=0 时为检查点/基线序号。
    pub seq: u64,
    pub event_id: String,
    pub timestamp_ms: i64,
    pub text: String,
    pub selection_anchor: u32,
    pub selection_head: u32,
    pub source: EventSource,
    /// 章节分支内已应用的事件数（0..=total_events）。
    pub position: usize,
    pub total_events: usize,
    pub first_seq: u64,
    pub last_seq: u64,
    pub first_timestamp_ms: i64,
    pub last_timestamp_ms: i64,
    /// 下一条事件的发生时间；播放器据此和倍速计算等待时长。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_timestamp_ms: Option<i64>,
    pub checkpoint_seq: u64,
    /// 同一章节在不同设备上的独立证明分支；跨设备不按毫秒交错。
    #[serde(default)]
    pub branches: Vec<ProofDeviceBranch>,
    /// 链校验是否全程通过。false 时附带 errors，平台必须明确提示。
    pub valid: bool,
    pub errors: Vec<String>,
}

/// 一台设备上的证明分支摘要。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofDeviceBranch {
    pub device_id: String,
    pub event_count: usize,
    pub first_seq: u64,
    pub last_seq: u64,
    pub first_timestamp_ms: i64,
    pub last_timestamp_ms: i64,
}

/// Core 侧可离线校验的证明包。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofExportPackage {
    pub format_version: u32,
    pub exported_at_ms: i64,
    pub chapter_id: String,
    pub device_id: String,
    pub baseline: Option<ProofCheckpoint>,
    pub checkpoints: Vec<ProofCheckpoint>,
    pub events: Vec<WritingInputEvent>,
    pub activity_segments: Vec<WritingActivitySegment>,
    pub final_text_sha256: String,
    pub final_seq: u64,
    /// 导出包包含历史正文，UI 必须提示用户。
    pub contains_historical_text: bool,
}

#[derive(Debug)]
struct ReplayState {
    position: usize,
    seq: u64,
    event_id: String,
    timestamp_ms: i64,
    text: String,
    digest: ContentDigest,
    expected_content_hash: String,
    previous_event_hash: String,
    selection_anchor: u32,
    selection_head: u32,
    source: EventSource,
    checkpoint_seq: u64,
    errors: Vec<String>,
}

/// 打开后的章节回放会话。事件索引与检查点只解析一次，向前播放增量应用。
pub struct ProofReplaySession {
    pub chapter_id: String,
    pub device_id: String,
    events: Vec<WritingInputEvent>,
    checkpoints: Vec<ProofCheckpoint>,
    baseline_text: String,
    branches: Vec<ProofDeviceBranch>,
    chain_errors: Vec<String>,
    state: Mutex<ReplayState>,
}

impl ProofReplaySession {
    pub fn open(store: &StatsStore, request: &ProofReplayRequest) -> crate::Result<Self> {
        let all_events = store.load_all_events()?;
        let mut by_device: BTreeMap<String, Vec<WritingInputEvent>> = BTreeMap::new();
        for event in all_events {
            if event.chapter_id != request.chapter_id || event.proof.is_none() {
                continue;
            }
            by_device
                .entry(event.device_id.clone())
                .or_default()
                .push(event);
        }
        if by_device.is_empty() {
            return Err(crate::Error::Other(format!(
                "章节 {} 没有逐字编辑证明",
                request.chapter_id
            )));
        }

        for events in by_device.values_mut() {
            events.sort_by_key(|event| event.proof.as_ref().map_or(0, |proof| proof.seq));
        }
        let branches: Vec<ProofDeviceBranch> = by_device
            .iter()
            .map(|(device_id, events)| {
                let first_seq = events
                    .iter()
                    .filter_map(|event| event.proof.as_ref().map(|proof| proof.seq))
                    .min()
                    .unwrap_or(0);
                let last_seq = events
                    .iter()
                    .filter_map(|event| event.proof.as_ref().map(|proof| proof.seq))
                    .max()
                    .unwrap_or(0);
                ProofDeviceBranch {
                    device_id: device_id.clone(),
                    event_count: events.len(),
                    first_seq,
                    last_seq,
                    first_timestamp_ms: events.first().map_or(0, |event| event.timestamp_ms),
                    last_timestamp_ms: events.last().map_or(0, |event| event.timestamp_ms),
                }
            })
            .collect();

        let device_id = if !request.device_id.is_empty() {
            request.device_id.clone()
        } else if let Some(origin) = store.load_origin_device_id() {
            if by_device.contains_key(&origin) {
                origin
            } else {
                by_device.keys().next().cloned().unwrap_or_default()
            }
        } else {
            by_device.keys().next().cloned().unwrap_or_default()
        };
        let events = by_device.remove(&device_id).ok_or_else(|| {
            crate::Error::Other(format!(
                "设备 {device_id} 没有章节 {} 的证明事件",
                request.chapter_id
            ))
        })?;
        let checkpoints =
            checkpoint::load_checkpoints(store.app_data_root(), &device_id, &request.chapter_id)?;
        let baseline_text = request.baseline_text.clone().unwrap_or_default();
        let chain_errors = validate_full_chain(&events, &checkpoints, &baseline_text);
        let state = initial_state(&events, &checkpoints, &baseline_text, 0);

        Ok(Self {
            chapter_id: request.chapter_id.clone(),
            device_id,
            events,
            checkpoints,
            baseline_text,
            branches,
            chain_errors,
            state: Mutex::new(state),
        })
    }

    pub fn frame_at(&self, request: &ProofReplayRequest) -> ProofReplayFrame {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let target = target_position(
            &self.events,
            request.target_index,
            request.target_seq,
            request.target_time_ms,
        );

        if target < state.position {
            *state = self.reset_state(target);
        }
        while state.position < target && state.position < self.events.len() {
            let event = &self.events[state.position];
            apply_event(&mut state, event);
            state.position += 1;
        }

        let mut errors = state.errors.clone();
        errors.extend(self.chain_errors.iter().cloned());
        let next_timestamp_ms = self.events.get(state.position).map(|event| event.timestamp_ms);
        let first_timestamp_ms = self.events.first().map_or(0, |event| event.timestamp_ms);
        let last_timestamp_ms = self.events.last().map_or(0, |event| event.timestamp_ms);
        ProofReplayFrame {
            chapter_id: self.chapter_id.clone(),
            device_id: self.device_id.clone(),
            seq: state.seq,
            event_id: state.event_id.clone(),
            timestamp_ms: state.timestamp_ms,
            text: state.text.clone(),
            selection_anchor: state.selection_anchor,
            selection_head: state.selection_head,
            source: state.source.clone(),
            position: state.position,
            total_events: self.events.len(),
            first_seq: self.events.first().and_then(|event| event.proof.as_ref().map(|proof| proof.seq)).unwrap_or(0),
            last_seq: self.events.last().and_then(|event| event.proof.as_ref().map(|proof| proof.seq)).unwrap_or(0),
            first_timestamp_ms,
            last_timestamp_ms,
            next_timestamp_ms,
            checkpoint_seq: state.checkpoint_seq,
            branches: self.branches.clone(),
            valid: errors.is_empty(),
            errors,
        }
    }

    fn reset_state(&self, target: usize) -> ReplayState {
        initial_state(&self.events, &self.checkpoints, &self.baseline_text, target)
    }
}

/// 首次载入时顺序校验整条章节事件链，避免近端 checkpoint 掩盖更早的篡改。
/// 后续 seek 才利用已验证 checkpoint 快进。
fn validate_full_chain(
    events: &[WritingInputEvent],
    checkpoints: &[ProofCheckpoint],
    baseline_text: &str,
) -> Vec<String> {
    let mut state = ReplayState {
        position: 0,
        seq: 0,
        event_id: String::new(),
        timestamp_ms: 0,
        text: baseline_text.to_string(),
        digest: ContentDigest::from_text(baseline_text),
        expected_content_hash: content_hash_of_text(baseline_text),
        previous_event_hash: String::new(),
        selection_anchor: 0,
        selection_head: 0,
        source: EventSource::Unknown,
        checkpoint_seq: 0,
        errors: Vec::new(),
    };
    if let Some(baseline) = checkpoints
        .iter()
        .find(|checkpoint| checkpoint.seq == 0 && checkpoint_is_valid(checkpoint, events))
    {
        state.text = baseline.text.clone();
        state.digest = ContentDigest::from_text(&baseline.text);
        state.expected_content_hash = baseline.content_hash.clone();
    }

    let checkpoint_by_seq: std::collections::HashMap<u64, &ProofCheckpoint> =
        checkpoints.iter().map(|checkpoint| (checkpoint.seq, checkpoint)).collect();
    let mut errors = Vec::new();

    for event in events {
        let before_errors = state.errors.len();
        apply_event(&mut state, event);
        errors.extend(state.errors[before_errors..].iter().cloned());
        state.position += 1;
        let Some(proof) = event.proof.as_ref() else {
            continue;
        };
        if let Some(checkpoint) = checkpoint_by_seq.get(&proof.seq) {
            if checkpoint.text != state.text {
                errors.push(format!(
                    "检查点 {} 的正文与按事件重放结果不一致",
                    proof.seq
                ));
            }
            if checkpoint.content_hash != state.expected_content_hash {
                errors.push(format!(
                    "检查点 {} 的内容摘要与事件链不一致",
                    proof.seq
                ));
            }
        }
    }

    let last_seq = state.seq;
    for checkpoint in checkpoints.iter().filter(|checkpoint| checkpoint.seq > 0) {
        if checkpoint.seq > last_seq {
            errors.push(format!(
                "检查点 {} 没有对应的事件",
                checkpoint.seq
            ));
        }
    }
    errors.dedup();
    errors
}

fn target_position(
    events: &[WritingInputEvent],
    target_index: Option<usize>,
    target_seq: Option<u64>,
    target_time_ms: Option<i64>,
) -> usize {
    if let Some(index) = target_index {
        return index.min(events.len());
    }
    if let Some(target_seq) = target_seq {
        return events
            .iter()
            .position(|event| event.proof.as_ref().is_some_and(|proof| proof.seq > target_seq))
            .unwrap_or(events.len());
    }
    if let Some(target_time) = target_time_ms {
        return events
            .iter()
            .position(|event| event.timestamp_ms > target_time)
            .unwrap_or(events.len());
    }
    events.len()
}

fn initial_state(
    events: &[WritingInputEvent],
    checkpoints: &[ProofCheckpoint],
    baseline_text: &str,
    target: usize,
) -> ReplayState {
    let target_seq = if target == 0 {
        None
    } else {
        events
            .get(target - 1)
            .and_then(|event| event.proof.as_ref().map(|proof| proof.seq))
    };
    let mut chosen = None;
    for checkpoint in checkpoints.iter().rev() {
        if checkpoint.seq != 0 {
            let Some(target_seq) = target_seq else {
                continue;
            };
            if checkpoint.seq > target_seq {
                continue;
            }
        }
        if checkpoint_is_valid(checkpoint, events) {
            chosen = Some(checkpoint.clone());
            break;
        }
    }

    let mut state = match chosen {
        Some(checkpoint) => {
            let position = if checkpoint.seq == 0 {
                // seq0 是记录起点之前的基线，不属于任何事件。
                0
            } else {
                events
                    .iter()
                    .take_while(|event| {
                        event.proof.as_ref().is_some_and(|proof| proof.seq <= checkpoint.seq)
                    })
                    .count()
            };
            ReplayState {
                position,
                seq: checkpoint.seq,
                event_id: String::new(),
                timestamp_ms: checkpoint.timestamp_ms,
                text: checkpoint.text.clone(),
                digest: ContentDigest::from_text(&checkpoint.text),
                expected_content_hash: checkpoint.content_hash.clone(),
                previous_event_hash: checkpoint.event_hash.clone(),
                selection_anchor: 0,
                selection_head: 0,
                source: EventSource::Unknown,
                checkpoint_seq: checkpoint.seq,
                errors: Vec::new(),
            }
        }
        None => ReplayState {
            position: 0,
            seq: 0,
            event_id: String::new(),
            timestamp_ms: 0,
            text: baseline_text.to_string(),
            digest: ContentDigest::from_text(baseline_text),
            expected_content_hash: content_hash_of_text(baseline_text),
            previous_event_hash: String::new(),
            selection_anchor: 0,
            selection_head: 0,
            source: EventSource::Unknown,
            checkpoint_seq: 0,
            errors: Vec::new(),
        },
    };
    while state.position < target && state.position < events.len() {
        let event = &events[state.position];
        apply_event(&mut state, event);
        state.position += 1;
    }
    state
}

fn checkpoint_is_valid(checkpoint: &ProofCheckpoint, events: &[WritingInputEvent]) -> bool {
    if checkpoint.text_sha256 != text_sha256(&checkpoint.text) {
        return false;
    }
    if checkpoint.seq == 0 {
        return checkpoint.content_hash == content_hash_of_text(&checkpoint.text)
            && checkpoint.event_hash.is_empty();
    }
    let Some(event) = events.iter().find(|event| {
        event.proof.as_ref().is_some_and(|proof| proof.seq == checkpoint.seq)
    }) else {
        return false;
    };
    let Some(proof) = event.proof.as_ref() else {
        return false;
    };
    let checkpoint_digest = ContentDigest::from_text(&checkpoint.text);
    proof.event_hash == checkpoint.event_hash
        && proof.after_content_hash == checkpoint.content_hash
        && (proof.after_text_sha256.is_empty()
            || text_digest_matches(
                &proof.after_text_sha256,
                &checkpoint.text,
                &checkpoint_digest,
            ))
}

fn apply_event(state: &mut ReplayState, event: &WritingInputEvent) {
    let Some(proof) = event.proof.as_ref() else {
        return;
    };
    if proof.starts_new_chain && state.position > 0 {
        if !proof.before_text_sha256.is_empty()
            && !text_digest_matches(&proof.before_text_sha256, &state.text, &state.digest)
        {
            state.errors.push(format!(
                "事件 {} 新链基线正文摘要不一致",
                event.event_id
            ));
        }
        state.digest = ContentDigest::from_text(&state.text);
        state.expected_content_hash = content_hash_of_text(&state.text);
        state.previous_event_hash.clear();
    }
    if proof.seq <= state.seq {
        state.errors.push(format!(
            "事件 {} 的序号 {} 未超过上一条 {}",
            event.event_id, proof.seq, state.seq
        ));
    }
    if proof.before_content_hash != state.expected_content_hash {
        state.errors.push(format!(
            "事件 {} 的 before_content_hash 与回放状态不一致",
            event.event_id
        ));
    }
    if !proof.before_text_sha256.is_empty()
        && !text_digest_matches(&proof.before_text_sha256, &state.text, &state.digest)
    {
        state.errors.push(format!(
            "事件 {} 的 before_text_sha256 与回放正文不一致",
            event.event_id
        ));
    }
    if !proof.prev_event_hash.is_empty() && proof.prev_event_hash != state.previous_event_hash {
        state.errors.push(format!("事件 {} 的 prev_event_hash 断裂", event.event_id));
    }
    if compute_event_hash(event) != proof.event_hash {
        state.errors.push(format!("事件 {} 的 event_hash 校验失败", event.event_id));
    }
    if let Err(error) = apply_changes(&mut state.text, &proof.changes) {
        state.errors.push(format!("事件 {} 应用失败: {error}", event.event_id));
        return;
    }
    if let Err(error) = state.digest.apply_changes(&proof.changes) {
        state.errors.push(format!("事件 {} 摘要更新失败: {error}", event.event_id));
        return;
    }
    let after = chain_content_hash(&proof.before_content_hash, &proof.changes);
    if after != proof.after_content_hash {
        state.errors.push(format!(
            "事件 {} 的 after_content_hash 与变更不一致",
            event.event_id
        ));
    }
    if !proof.after_text_sha256.is_empty()
        && !text_digest_matches(&proof.after_text_sha256, &state.text, &state.digest)
    {
        state.errors.push(format!(
            "事件 {} 的 after_text_sha256 与回放正文不一致",
            event.event_id
        ));
    }
    state.seq = proof.seq;
    state.event_id = event.event_id.clone();
    state.timestamp_ms = event.timestamp_ms;
    state.expected_content_hash = proof.after_content_hash.clone();
    state.previous_event_hash = proof.event_hash.clone();
    state.selection_anchor = proof.new_selection.anchor.index.value() as u32;
    state.selection_head = proof.new_selection.head.index.value() as u32;
    state.source = event.source.clone();
}

/// 回放某章节到指定位置/序号/时间。
pub fn replay_chapter(
    store: &StatsStore,
    request: &ProofReplayRequest,
) -> crate::Result<ProofReplayFrame> {
    let session = ProofReplaySession::open(store, request)?;
    Ok(session.frame_at(request))
}

/// 生成按章节的离线证明包。包内包含历史正文，调用方必须先向用户提示。
pub fn export_chapter_proof(
    store: &StatsStore,
    chapter_id: &str,
    device_id: &str,
) -> crate::Result<ProofExportPackage> {
    let request = ProofReplayRequest {
        chapter_id: chapter_id.to_string(),
        device_id: device_id.to_string(),
        target_seq: None,
        target_time_ms: None,
        target_index: None,
        baseline_text: None,
    };
    let frame = replay_chapter(store, &request)?;
    if !frame.valid {
        return Err(crate::Error::Other(format!(
            "章节 {chapter_id} 的证明链校验失败，拒绝导出: {}",
            frame.errors.join("; ")
        )));
    }
    let device_id = frame.device_id.clone();

    let mut events: Vec<WritingInputEvent> = store
        .load_all_events()?
        .into_iter()
        .filter(|event| {
            event.chapter_id == chapter_id
                && event.device_id == device_id
                && event.proof.is_some()
        })
        .collect();
    events.sort_by_key(|event| event.proof.as_ref().map_or(0, |proof| proof.seq));
    let checkpoints = checkpoint::load_checkpoints(store.app_data_root(), &device_id, chapter_id)?;
    let baseline = checkpoints
        .iter()
        .find(|checkpoint| checkpoint.seq == 0)
        .cloned();
    let activity_segments: Vec<WritingActivitySegment> = store
        .load_all_segments()?
        .into_iter()
        .filter(|segment| {
            segment.chapter_id == chapter_id && segment.device_id == device_id
        })
        .collect();

    Ok(ProofExportPackage {
        format_version: crate::writing_stats::proof::PROOF_FORMAT_VERSION,
        exported_at_ms: chrono::Utc::now().timestamp_millis(),
        chapter_id: chapter_id.to_string(),
        device_id,
        baseline,
        checkpoints,
        events,
        activity_segments,
        final_text_sha256: text_sha256(&frame.text),
        final_seq: frame.seq,
        contains_historical_text: true,
    })
}
