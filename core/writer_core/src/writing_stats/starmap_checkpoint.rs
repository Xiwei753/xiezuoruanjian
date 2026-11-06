//! 星图回放检查点。
//!
//! 检查点按来源设备分支保存到
//! `stats/events/<device_id>/starmap/<starmap_id>.checkpoints.jsonl`。每个检查点
//! 记录事件位置和该位置的完整图状态；随机跳转时间轴时从最近检查点恢复，
//! 连续向前播放只应用新事件。
//!
//! 第一次在旧星图上记录回放事件时，Core 会写入一条位置 0、原因
//! [`CHECKPOINT_REASON_BASELINE_INCOMPLETE`] 的基线，并明确标记此前历史缺失。

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::writing_stats::contribution_store::valid_segment;
use crate::writing_stats::starmap_proof::{
    state_sha256, StarMapProofState, STARMAP_PROOF_FORMAT_VERSION,
};

pub const STARMAP_CHECKPOINT_FORMAT_VERSION: u32 = 1;
pub const CHECKPOINT_REASON_BASELINE_INCOMPLETE: &str = "baselineIncomplete";
pub const CHECKPOINT_REASON_PERIODIC: &str = "periodic";
pub const CHECKPOINT_REASON_EXPORT: &str = "export";

/// 一个星图回放检查点。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapProofCheckpoint {
    #[serde(default = "default_format_version")]
    pub format_version: u32,
    pub starmap_id: String,
    pub device_id: String,
    /// 已应用的事件数（0 表示基线/空白）。
    pub position: u64,
    pub state: StarMapProofState,
    #[serde(default)]
    pub state_sha256: String,
    pub created_at_ms: i64,
    #[serde(default)]
    pub reason: String,
    /// 若基线来自其它设备，保留真实来源设备。
    #[serde(default)]
    pub source_device_id: String,
    /// 该分支此前历史是否完整。旧星图基线固定为 false。
    #[serde(default)]
    pub history_complete: bool,
}

const fn default_format_version() -> u32 {
    STARMAP_CHECKPOINT_FORMAT_VERSION
}

impl StarMapProofCheckpoint {
    pub fn new(
        starmap_id: &str,
        device_id: &str,
        position: u64,
        state: StarMapProofState,
        reason: &str,
        history_complete: bool,
    ) -> Self {
        let state_sha256 = state_sha256(&state);
        Self {
            format_version: STARMAP_CHECKPOINT_FORMAT_VERSION,
            starmap_id: starmap_id.to_string(),
            device_id: device_id.to_string(),
            position,
            state,
            state_sha256,
            created_at_ms: chrono::Utc::now().timestamp_millis(),
            reason: reason.to_string(),
            source_device_id: device_id.to_string(),
            history_complete,
        }
    }

    pub fn hash_matches(&self) -> bool {
        self.state_sha256.is_empty() || self.state_sha256 == state_sha256(&self.state)
    }
}

fn checkpoints_dir(root: &Path, device_id: &str) -> PathBuf {
    root.join("stats/events").join(device_id).join("starmap")
}

pub fn starmap_checkpoint_path(root: &Path, device_id: &str, starmap_id: &str) -> PathBuf {
    checkpoints_dir(root, device_id).join(format!("{starmap_id}.checkpoints.jsonl"))
}

pub fn has_starmap_checkpoints(root: &Path, device_id: &str, starmap_id: &str) -> bool {
    starmap_checkpoint_path(root, device_id, starmap_id).exists()
}

pub fn load_starmap_checkpoints(
    root: &Path,
    device_id: &str,
    starmap_id: &str,
) -> Result<Vec<StarMapProofCheckpoint>> {
    if !valid_segment(device_id) || !valid_segment(starmap_id) {
        return Err(Error::Other(format!(
            "invalid starmap checkpoint partition: device={device_id:?} starmap={starmap_id:?}"
        )));
    }
    let path = starmap_checkpoint_path(root, device_id, starmap_id);
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut checkpoints = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<StarMapProofCheckpoint>(line) {
            Ok(checkpoint) => {
                if checkpoint.format_version == STARMAP_CHECKPOINT_FORMAT_VERSION
                    && checkpoint.hash_matches()
                {
                    checkpoints.push(checkpoint);
                }
            }
            Err(error) => {
                eprintln!(
                    "[writing_stats] skip malformed starmap checkpoint in {}: {error}",
                    path.display()
                );
            }
        }
    }
    checkpoints.sort_by_key(|checkpoint| checkpoint.position);
    checkpoints.dedup_by_key(|checkpoint| checkpoint.position);
    Ok(checkpoints)
}

/// 追加检查点；同一位置已存在时跳过，保证幂等。
pub fn append_starmap_checkpoint(root: &Path, checkpoint: &StarMapProofCheckpoint) -> Result<bool> {
    if !valid_segment(&checkpoint.device_id) || !valid_segment(&checkpoint.starmap_id) {
        return Err(Error::Other(format!(
            "invalid starmap checkpoint partition: device={:?} starmap={:?}",
            checkpoint.device_id, checkpoint.starmap_id
        )));
    }
    if checkpoint.format_version != STARMAP_CHECKPOINT_FORMAT_VERSION {
        return Err(Error::Other(format!(
            "unsupported starmap checkpoint format: {}",
            checkpoint.format_version
        )));
    }
    let existing = load_starmap_checkpoints(root, &checkpoint.device_id, &checkpoint.starmap_id)?;
    if existing
        .iter()
        .any(|item| item.position == checkpoint.position)
    {
        return Ok(false);
    }
    let path = starmap_checkpoint_path(root, &checkpoint.device_id, &checkpoint.starmap_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(file, "{}", serde_json::to_string(checkpoint)?)?;
    Ok(true)
}

/// 找到其它设备分支为同一张星图保存的真实基线。
///
/// 跨设备接力编辑时，新设备没有从空白创建的过程；它只能以其它设备已封存的
/// 真实图为起点，并明确标记此前历史不完整，不能伪造毫秒级交错顺序。
pub fn find_other_branch_baseline(
    root: &Path,
    starmap_id: &str,
    exclude_device_id: &str,
) -> Result<Option<StarMapProofCheckpoint>> {
    let events_root = root.join("stats/events");
    if !events_root.is_dir() {
        return Ok(None);
    }
    let mut devices: Vec<String> = fs::read_dir(&events_root)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|device| device != exclude_device_id && valid_segment(device))
        .collect();
    devices.sort();
    for device in devices {
        if let Some(checkpoint) = load_starmap_checkpoints(root, &device, starmap_id)?
            .into_iter()
            .find(|checkpoint| checkpoint.position == 0)
        {
            return Ok(Some(checkpoint));
        }
    }
    Ok(None)
}

/// 找到位置不超过 `target_position` 的最近检查点。
pub fn latest_checkpoint_at_or_before(
    checkpoints: &[StarMapProofCheckpoint],
    target_position: u64,
) -> Option<&StarMapProofCheckpoint> {
    checkpoints
        .iter()
        .filter(|checkpoint| checkpoint.position <= target_position)
        .max_by_key(|checkpoint| checkpoint.position)
}

/// 判断某个分支是否有可用的完整基线（位置 0 的检查点）。
pub fn has_branch_baseline(
    root: &Path,
    device_id: &str,
    starmap_id: &str,
) -> Result<Option<StarMapProofCheckpoint>> {
    let checkpoints = load_starmap_checkpoints(root, device_id, starmap_id)?;
    Ok(checkpoints
        .into_iter()
        .find(|checkpoint| checkpoint.position == 0))
}

/// 基线检查点使用的格式版本与证明格式保持一致。
pub const fn proof_format_version() -> u32 {
    STARMAP_PROOF_FORMAT_VERSION
}
