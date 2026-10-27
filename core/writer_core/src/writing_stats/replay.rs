//! 手写证明回放：按章节、设备和设备内序号重建任意时刻正文。
//!
//! Core 负责验证事件链、应用实际变更和给出回放状态；平台只渲染正文、
//! 光标与进度，不自行推演编辑规则。回放优先从最近的完整正文 checkpoint
//! 开始，再局部应用后续事件。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::writing_stats::checkpoint::{
    self, latest_checkpoint_at_or_before, ProofCheckpoint,
};
use crate::writing_stats::proof::{
    chain_content_hash, compute_event_hash, content_hash_of_text, text_sha256,
};
use crate::writing_stats::store::StatsStore;
use crate::writing_stats::{EventSource, WritingInputEvent, WritingActivitySegment};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofReplayRequest {
    pub chapter_id: String,
    /// 为空时优先本机设备，其次选择该章节第一条有证明的事件来源。
    #[serde(default)]
    pub device_id: String,
    /// 目标设备内序号；为空表示到最后。
    #[serde(default)]
    pub target_seq: Option<u64>,
    /// 目标发生时间（Unix 毫秒）；与 target_seq 同时给出时两者都作为上界。
    #[serde(default)]
    pub target_time_ms: Option<i64>,
    /// 没有 checkpoint 时使用的正文基线（旧会话/新章节）。
    #[serde(default)]
    pub baseline_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofReplayFrame {
    pub chapter_id: String,
    pub device_id: String,
    pub seq: u64,
    pub event_id: String,
    pub timestamp_ms: i64,
    pub text: String,
    pub selection_anchor: u32,
    pub selection_head: u32,
    pub source: EventSource,
    pub total_events: usize,
    pub first_seq: u64,
    pub last_seq: u64,
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

/// 回放某章节到指定序号/时间。
pub fn replay_chapter(
    store: &StatsStore,
    request: &ProofReplayRequest,
) -> crate::Result<ProofReplayFrame> {
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
            }
        })
        .collect();

    let device_id = if !request.device_id.is_empty() {
        request.device_id.clone()
    } else if let Some(origin) = store.load_origin_device_id() {
        if by_device.contains_key(&origin) {
            origin
        } else {
            by_device
                .keys()
                .next()
                .cloned()
                .unwrap_or_default()
        }
    } else {
        by_device
            .keys()
            .next()
            .cloned()
            .unwrap_or_default()
    };

    let mut events = by_device.remove(&device_id).ok_or_else(|| {
        crate::Error::Other(format!(
            "设备 {device_id} 没有章节 {} 的证明事件",
            request.chapter_id
        ))
    })?;
    events.sort_by_key(|event| event.proof.as_ref().map_or(0, |proof| proof.seq));

    let checkpoints =
        checkpoint::load_checkpoints(store.app_data_root(), &device_id, &request.chapter_id)?;
    let selected = latest_checkpoint_at_or_before(
        &checkpoints,
        request.target_seq,
        request.target_time_ms,
    );

    let baseline_text = selected
        .map(|checkpoint| checkpoint.text.clone())
        .or_else(|| request.baseline_text.clone())
        .unwrap_or_default();
    let mut text = baseline_text.clone();
    let mut expected_content_hash = selected.map_or_else(
        || content_hash_of_text(&baseline_text),
        |checkpoint| checkpoint.content_hash.clone(),
    );
    let mut checkpoint_seq = selected.map_or(0, |checkpoint| checkpoint.seq);
    let mut previous_event_hash = selected.map_or_else(String::new, |c| c.event_hash.clone());
    let mut previous_seq = checkpoint_seq;
    let mut frame_seq = checkpoint_seq;
    let mut frame_time = selected.map_or(0, |checkpoint| checkpoint.timestamp_ms);
    let mut frame_event_id = String::new();
    let mut selection_anchor = 0u32;
    let mut selection_head = 0u32;
    let mut source = EventSource::Unknown;
    let mut errors = Vec::new();
    if let Some(checkpoint) = selected {
        if checkpoint.text_sha256 != text_sha256(&checkpoint.text) {
            errors.push(format!(
                "检查点 seq {} 的正文摘要不一致",
                checkpoint.seq
            ));
        }
        if checkpoint.seq == 0 && checkpoint.content_hash != content_hash_of_text(&checkpoint.text) {
            errors.push("基线检查点的内容摘要不一致".to_string());
        }
    }

    let first_seq = events
        .first()
        .and_then(|event| event.proof.as_ref().map(|proof| proof.seq))
        .unwrap_or(0);
    let last_seq = events
        .last()
        .and_then(|event| event.proof.as_ref().map(|proof| proof.seq))
        .unwrap_or(0);

    for event in &events {
        let Some(proof) = event.proof.as_ref() else {
            continue;
        };
        if proof.seq <= checkpoint_seq {
            continue;
        }
        if let Some(target) = request.target_seq {
            if proof.seq > target {
                break;
            }
        }
        if let Some(target) = request.target_time_ms {
            if event.timestamp_ms > target {
                break;
            }
        }

        if proof.starts_new_chain && proof.seq != checkpoint_seq.saturating_add(1) {
            expected_content_hash = content_hash_of_text(&text);
            previous_event_hash.clear();
            previous_seq = proof.seq.saturating_sub(1);
        }

        if proof.before_content_hash != expected_content_hash {
            errors.push(format!(
                "事件 {} 的 before_content_hash 与回放状态不一致",
                event.event_id
            ));
        }
        if !proof.prev_event_hash.is_empty() && proof.prev_event_hash != previous_event_hash {
            errors.push(format!(
                "事件 {} 的 prev_event_hash 断裂",
                event.event_id
            ));
        }
        let expected_event_hash = compute_event_hash(event);
        if expected_event_hash != proof.event_hash {
            errors.push(format!("事件 {} 的 event_hash 校验失败", event.event_id));
        }
        if proof.seq <= previous_seq {
            errors.push(format!(
                "事件 {} 的序号 {} 未递增（上一条 {}）",
                event.event_id, proof.seq, previous_seq
            ));
        }

        if let Err(error) = crate::writing_stats::proof::apply_changes(&mut text, &proof.changes) {
            errors.push(format!("事件 {} 应用失败: {error}", event.event_id));
            break;
        }
        let after = chain_content_hash(&proof.before_content_hash, &proof.changes);
        if after != proof.after_content_hash {
            errors.push(format!(
                "事件 {} 的 after_content_hash 与变更不一致",
                event.event_id
            ));
        }

        expected_content_hash = proof.after_content_hash.clone();
        previous_event_hash = proof.event_hash.clone();
        previous_seq = proof.seq;
        frame_seq = proof.seq;
        frame_time = event.timestamp_ms;
        frame_event_id = event.event_id.clone();
        selection_anchor = proof.new_selection.anchor.index.value() as u32;
        selection_head = proof.new_selection.head.index.value() as u32;
        source = event.source.clone();
    }

    Ok(ProofReplayFrame {
        chapter_id: request.chapter_id.clone(),
        device_id,
        seq: frame_seq,
        event_id: frame_event_id,
        timestamp_ms: frame_time,
        text,
        selection_anchor,
        selection_head,
        source,
        total_events: events.len(),
        first_seq,
        last_seq,
        checkpoint_seq,
        branches,
        valid: errors.is_empty(),
        errors,
    })
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
        baseline_text: None,
    };
    let frame = replay_chapter(store, &request)?;
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
