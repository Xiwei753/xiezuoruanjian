//! 星图回放检查点。
//!
//! 检查点按来源设备分支保存到 `stats/events/<device_id>/starmap/`：
//! - `<starmap_id>.checkpoints.index.json`：只含位置、链头、状态摘要和快照
//!   文件名的轻量索引，普通编辑只读这一份小文件；
//! - `<starmap_id>.checkpoints/<position>.json`：按需载入的完整图状态。
//!
//! 旧版 `<starmap_id>.checkpoints.jsonl` 在首次读取时一次性迁移为上述格式，
//! 之后普通写路径不再遍历历史整图快照。

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::writing_stats::contribution_store::valid_segment;
use crate::writing_stats::starmap_proof::{
    state_sha256, StarMapProofState, STARMAP_PROOF_FORMAT_VERSION,
};

pub const STARMAP_CHECKPOINT_FORMAT_VERSION: u32 = 2;
pub const LEGACY_STARMAP_CHECKPOINT_FORMAT_VERSION: u32 = 1;
pub const STARMAP_CHECKPOINT_INDEX_FORMAT_VERSION: u32 = 1;
pub const CHECKPOINT_REASON_BASELINE_INCOMPLETE: &str = "baselineIncomplete";
pub const CHECKPOINT_REASON_CREATED: &str = "created";
pub const CHECKPOINT_REASON_PERIODIC: &str = "periodic";
pub const CHECKPOINT_REASON_EXPORT: &str = "export";

/// 一个星图回放检查点的完整快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapProofCheckpoint {
    #[serde(default = "default_format_version")]
    pub format_version: u32,
    pub starmap_id: String,
    pub device_id: String,
    /// 本星图该来源分支内已应用的事件数（0 表示基线/空白）。
    pub position: u64,
    /// `position` 对应的事件链头哈希；仅 state 摘要不能证明事件前缀。
    #[serde(default)]
    pub event_chain_head: String,
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

/// 轻量索引项：不包含整图状态。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapCheckpointHeader {
    pub format_version: u32,
    pub starmap_id: String,
    pub device_id: String,
    pub position: u64,
    #[serde(default)]
    pub event_chain_head: String,
    #[serde(default)]
    pub state_sha256: String,
    pub created_at_ms: i64,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub source_device_id: String,
    #[serde(default)]
    pub history_complete: bool,
    /// 相对本星图检查点目录的快照文件名。
    pub snapshot_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StarMapCheckpointIndex {
    format_version: u32,
    checkpoints: Vec<StarMapCheckpointHeader>,
}

static CHECKPOINT_INDEX_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn checkpoint_index_lock() -> &'static Mutex<()> {
    CHECKPOINT_INDEX_LOCK.get_or_init(|| Mutex::new(()))
}

const fn default_format_version() -> u32 {
    LEGACY_STARMAP_CHECKPOINT_FORMAT_VERSION
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
            event_chain_head: String::new(),
            state,
            state_sha256,
            created_at_ms: chrono::Utc::now().timestamp_millis(),
            reason: reason.to_string(),
            source_device_id: device_id.to_string(),
            history_complete,
        }
    }

    pub fn with_event_chain_head(mut self, event_chain_head: &str) -> Self {
        self.event_chain_head = event_chain_head.to_string();
        self
    }

    pub fn hash_matches(&self) -> bool {
        self.state_sha256.is_empty() || self.state_sha256 == state_sha256(&self.state)
    }

    /// 检查点是否锚定到该事件链前缀的真实链头。
    pub fn anchors_event_chain(&self, event_chain_head: &str) -> bool {
        self.position == 0
            || (!self.event_chain_head.is_empty() && self.event_chain_head == event_chain_head)
    }

    fn header(&self) -> StarMapCheckpointHeader {
        StarMapCheckpointHeader {
            format_version: self.format_version,
            starmap_id: self.starmap_id.clone(),
            device_id: self.device_id.clone(),
            position: self.position,
            event_chain_head: self.event_chain_head.clone(),
            state_sha256: self.state_sha256.clone(),
            created_at_ms: self.created_at_ms,
            reason: self.reason.clone(),
            source_device_id: self.source_device_id.clone(),
            history_complete: self.history_complete,
            snapshot_file: snapshot_file_name(self.position),
        }
    }
}

fn checkpoints_dir(root: &Path, device_id: &str) -> PathBuf {
    root.join("stats/events").join(device_id).join("starmap")
}

pub fn starmap_checkpoint_path(root: &Path, device_id: &str, starmap_id: &str) -> PathBuf {
    checkpoints_dir(root, device_id).join(format!("{starmap_id}.checkpoints.jsonl"))
}

pub fn starmap_checkpoint_index_path(root: &Path, device_id: &str, starmap_id: &str) -> PathBuf {
    checkpoints_dir(root, device_id).join(format!("{starmap_id}.checkpoints.index.json"))
}

fn starmap_checkpoint_snapshots_dir(root: &Path, device_id: &str, starmap_id: &str) -> PathBuf {
    checkpoints_dir(root, device_id).join(format!("{starmap_id}.checkpoints"))
}

fn snapshot_file_name(position: u64) -> String {
    format!("{position:020}.json")
}

fn valid_starmap_id(starmap_id: &str) -> bool {
    valid_segment(starmap_id)
}

fn parse_checkpoint_line(line: &str) -> Option<StarMapProofCheckpoint> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    serde_json::from_str::<StarMapProofCheckpoint>(line).ok()
}

fn write_index(
    root: &Path,
    device_id: &str,
    starmap_id: &str,
    headers: &[StarMapCheckpointHeader],
) -> Result<()> {
    let index = StarMapCheckpointIndex {
        format_version: STARMAP_CHECKPOINT_INDEX_FORMAT_VERSION,
        checkpoints: headers.to_vec(),
    };
    let path = starmap_checkpoint_index_path(root, device_id, starmap_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    crate::storage::atomic_write_string(&path, &serde_json::to_string(&index)?)
}

fn write_snapshot(root: &Path, checkpoint: &StarMapProofCheckpoint) -> Result<()> {
    let dir = starmap_checkpoint_snapshots_dir(root, &checkpoint.device_id, &checkpoint.starmap_id);
    fs::create_dir_all(&dir)?;
    let path = dir.join(snapshot_file_name(checkpoint.position));
    crate::storage::atomic_write_string(&path, &serde_json::to_string(checkpoint)?)
}

fn migrate_legacy_checkpoints(
    root: &Path,
    device_id: &str,
    starmap_id: &str,
) -> Result<Vec<StarMapCheckpointHeader>> {
    let path = starmap_checkpoint_path(root, device_id, starmap_id);
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut checkpoints = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        let Some(checkpoint) = parse_checkpoint_line(&line) else {
            continue;
        };
        if checkpoint.format_version == STARMAP_CHECKPOINT_FORMAT_VERSION
            && checkpoint.hash_matches()
        {
            checkpoints.push(checkpoint);
        }
    }
    checkpoints.sort_by_key(|checkpoint| checkpoint.position);
    checkpoints.dedup_by_key(|checkpoint| checkpoint.position);
    for checkpoint in &checkpoints {
        write_snapshot(root, checkpoint)?;
    }
    let headers: Vec<StarMapCheckpointHeader> = checkpoints
        .iter()
        .map(StarMapProofCheckpoint::header)
        .collect();
    write_index(root, device_id, starmap_id, &headers)?;
    Ok(headers)
}

fn load_checkpoint_index(
    root: &Path,
    device_id: &str,
    starmap_id: &str,
) -> Result<Vec<StarMapCheckpointHeader>> {
    if !valid_starmap_id(device_id) || !valid_starmap_id(starmap_id) {
        return Err(Error::Other(format!(
            "invalid starmap checkpoint partition: device={device_id:?} starmap={starmap_id:?}"
        )));
    }
    let path = starmap_checkpoint_index_path(root, device_id, starmap_id);
    if let Ok(content) = fs::read_to_string(&path) {
        if let Ok(index) = serde_json::from_str::<StarMapCheckpointIndex>(&content) {
            if index.format_version == STARMAP_CHECKPOINT_INDEX_FORMAT_VERSION {
                let mut headers = index.checkpoints;
                headers.sort_by_key(|header| header.position);
                headers.dedup_by_key(|header| header.position);
                return Ok(headers);
            }
        }
        return Ok(Vec::new());
    }
    migrate_legacy_checkpoints(root, device_id, starmap_id)
}

pub fn list_checkpoint_headers(
    root: &Path,
    device_id: &str,
    starmap_id: &str,
) -> Result<Vec<StarMapCheckpointHeader>> {
    load_checkpoint_index(root, device_id, starmap_id)
}

pub fn last_checkpoint_position(
    root: &Path,
    device_id: &str,
    starmap_id: &str,
) -> Result<Option<u64>> {
    Ok(load_checkpoint_index(root, device_id, starmap_id)?
        .into_iter()
        .map(|header| header.position)
        .max())
}

pub fn load_checkpoint_at(
    root: &Path,
    device_id: &str,
    starmap_id: &str,
    position: u64,
) -> Result<Option<StarMapProofCheckpoint>> {
    let Some(header) = load_checkpoint_index(root, device_id, starmap_id)?
        .into_iter()
        .find(|header| header.position == position)
    else {
        return Ok(None);
    };
    let path =
        starmap_checkpoint_snapshots_dir(root, device_id, starmap_id).join(&header.snapshot_file);
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let checkpoint: StarMapProofCheckpoint = serde_json::from_str(&content)?;
    if checkpoint.format_version != STARMAP_CHECKPOINT_FORMAT_VERSION
        || checkpoint.position != header.position
        || checkpoint.starmap_id != header.starmap_id
        || checkpoint.device_id != header.device_id
        || checkpoint.state_sha256 != header.state_sha256
        || checkpoint.event_chain_head != header.event_chain_head
        || !checkpoint.hash_matches()
    {
        return Ok(None);
    }
    Ok(Some(checkpoint))
}

pub fn load_latest_checkpoint_at_or_before(
    root: &Path,
    device_id: &str,
    starmap_id: &str,
    target_position: u64,
) -> Result<Option<StarMapProofCheckpoint>> {
    let Some(header) = load_checkpoint_index(root, device_id, starmap_id)?
        .into_iter()
        .filter(|header| header.position <= target_position)
        .max_by_key(|header| header.position)
    else {
        return Ok(None);
    };
    load_checkpoint_at(root, device_id, starmap_id, header.position)
}

pub fn has_starmap_checkpoints(root: &Path, device_id: &str, starmap_id: &str) -> bool {
    load_checkpoint_index(root, device_id, starmap_id)
        .map(|headers| !headers.is_empty())
        .unwrap_or_else(|_| {
            starmap_checkpoint_index_path(root, device_id, starmap_id).exists()
                || starmap_checkpoint_path(root, device_id, starmap_id).exists()
        })
}

/// 全量枚举检查点（导出/证明等需要完整读取的路径使用）。
pub fn load_starmap_checkpoints(
    root: &Path,
    device_id: &str,
    starmap_id: &str,
) -> Result<Vec<StarMapProofCheckpoint>> {
    let headers = load_checkpoint_index(root, device_id, starmap_id)?;
    let mut checkpoints = Vec::new();
    for header in headers {
        if let Some(checkpoint) = load_checkpoint_at(root, device_id, starmap_id, header.position)?
        {
            checkpoints.push(checkpoint);
        }
    }
    checkpoints.sort_by_key(|checkpoint| checkpoint.position);
    checkpoints.dedup_by_key(|checkpoint| checkpoint.position);
    Ok(checkpoints)
}

/// 追加检查点；按分支 + position 幂等，索引与快照同一临界区更新。
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
    let _guard = checkpoint_index_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut headers = load_checkpoint_index(root, &checkpoint.device_id, &checkpoint.starmap_id)?;
    if headers
        .iter()
        .any(|header| header.position == checkpoint.position)
    {
        return Ok(false);
    }
    // 先写快照再写索引：任何时刻索引都不会指向不存在的快照。
    write_snapshot(root, checkpoint)?;
    headers.push(checkpoint.header());
    headers.sort_by_key(|header| header.position);
    write_index(
        root,
        &checkpoint.device_id,
        &checkpoint.starmap_id,
        &headers,
    )?;
    Ok(true)
}

/// 找到其它设备分支为同一张星图保存的真实基线。
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
        if let Some(checkpoint) = load_checkpoint_at(root, &device, starmap_id, 0)? {
            return Ok(Some(checkpoint));
        }
    }
    Ok(None)
}

/// 找到位置不超过 `target_position` 的最近检查点（内存切片版本）。
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
    load_checkpoint_at(root, device_id, starmap_id, 0)
}

/// 基线检查点使用的格式版本与证明格式保持一致。
pub const fn proof_format_version() -> u32 {
    STARMAP_PROOF_FORMAT_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_and_snapshot_round_trip() {
        let dir = std::env::temp_dir().join(format!(
            "sujian-starmap-checkpoint-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        let mut state = StarMapProofState::empty("sm1");
        state.root_starmap_ids.push("sm1".to_string());
        let checkpoint = StarMapProofCheckpoint::new(
            "sm1",
            "device1",
            0,
            state,
            CHECKPOINT_REASON_CREATED,
            true,
        );
        assert!(append_starmap_checkpoint(&dir, &checkpoint).unwrap());
        assert!(!append_starmap_checkpoint(&dir, &checkpoint).unwrap());
        assert_eq!(
            last_checkpoint_position(&dir, "device1", "sm1").unwrap(),
            Some(0)
        );
        let loaded = load_checkpoint_at(&dir, "device1", "sm1", 0)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.state_sha256, checkpoint.state_sha256);
        assert_eq!(
            load_starmap_checkpoints(&dir, "device1", "sm1")
                .unwrap()
                .len(),
            1
        );
        let _ = fs::remove_dir_all(dir);
    }
}
