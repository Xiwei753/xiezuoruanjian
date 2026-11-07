//! 星图回放检查点。
//!
//! 检查点按来源设备分支保存到 `stats/events/<device_id>/starmap/`：
//! - `<starmap_id>.checkpoints.index.json`：只含位置、链头、状态摘要和快照
//!   文件名的轻量索引，普通编辑只读这一份小文件；
//! - `<starmap_id>.checkpoints/<position>.json`：按需载入的完整图状态。
//!
//! 旧版 `<starmap_id>.checkpoints.jsonl` 在首次写入检查点时一次性迁移为上述
//! 格式；普通读取只在索引缺失时兼容解析旧日志，绝不改写任何设备分区。

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::writing_stats::contribution_store::valid_segment;
use crate::writing_stats::starmap_commitment::StateCommitment;
use crate::writing_stats::starmap_proof::{
    state_sha256, StarMapProofState, STARMAP_PROOF_FORMAT_VERSION,
};

pub const STARMAP_CHECKPOINT_FORMAT_VERSION: u32 = 2;
pub const LEGACY_STARMAP_CHECKPOINT_FORMAT_VERSION: u32 = 1;
pub const STARMAP_CHECKPOINT_INDEX_FORMAT_VERSION: u32 = 1;
/// 周期检查点间隔；回放定位也用它区分相邻播放与大跨度跳转。
pub const STARMAP_CHECKPOINT_INTERVAL: u64 = 64;
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
    /// 按对象 ID/内容/顺序/meta/roots 计算的状态承诺根；旧检查点为空。
    #[serde(default)]
    pub state_root: String,
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
    /// 该检查点位置由顺序验证得到的可信状态承诺根；旧检查点为空。
    #[serde(default)]
    pub state_root: String,
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
        let state_root = StateCommitment::from_state(&state).root_hex();
        Self::new_with_state_root(
            starmap_id,
            device_id,
            position,
            state,
            &state_root,
            reason,
            history_complete,
        )
    }

    /// 已知增量承诺根时直接落检查点，避免再次为完整图重建 Merkle 树。
    pub fn new_with_state_root(
        starmap_id: &str,
        device_id: &str,
        position: u64,
        state: StarMapProofState,
        state_root: &str,
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
            state_root: state_root.to_string(),
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

    pub fn with_state_root(mut self, state_root: &str) -> Self {
        self.state_root = state_root.to_string();
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
            state_root: self.state_root.clone(),
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

fn migrated_checkpoint_path(path: &Path) -> PathBuf {
    let mut migrated = path.as_os_str().to_os_string();
    migrated.push(".migrated");
    PathBuf::from(migrated)
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

fn snapshot_position_from_file_name(name: &str) -> Option<u64> {
    let digits = name.strip_suffix(".json")?;
    if digits.len() != 20 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn valid_starmap_id(starmap_id: &str) -> bool {
    valid_segment(starmap_id)
}

fn sort_dedup_headers(mut headers: Vec<StarMapCheckpointHeader>) -> Vec<StarMapCheckpointHeader> {
    headers.sort_by_key(|header| header.position);
    headers.dedup_by_key(|header| header.position);
    headers
}

fn sort_dedup_checkpoints(
    mut checkpoints: Vec<StarMapProofCheckpoint>,
) -> Vec<StarMapProofCheckpoint> {
    checkpoints.sort_by_key(|checkpoint| checkpoint.position);
    checkpoints.dedup_by_key(|checkpoint| checkpoint.position);
    checkpoints
}

/// 索引项必须属于当前分区，且指向规范的文件名。
fn header_is_consistent(
    header: &StarMapCheckpointHeader,
    device_id: &str,
    starmap_id: &str,
) -> bool {
    header.format_version == STARMAP_CHECKPOINT_FORMAT_VERSION
        && header.starmap_id == starmap_id
        && header.device_id == device_id
        && !header.state_sha256.is_empty()
        && header.snapshot_file == snapshot_file_name(header.position)
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

/// 解析旧版 JSONL。读取路径只解析，不写任何文件。
/// 返回 `None` 表示旧日志不存在；文件存在但没有任何可解析条目时明确报损坏。
fn read_legacy_checkpoints(
    root: &Path,
    device_id: &str,
    starmap_id: &str,
) -> Result<Option<Vec<StarMapProofCheckpoint>>> {
    let path = starmap_checkpoint_path(root, device_id, starmap_id);
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut checkpoints = Vec::new();
    let mut had_content = false;
    let mut parsed_any = false;
    let mut current_candidates = 0usize;
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        had_content = true;
        let Some(mut checkpoint) = parse_checkpoint_line(&line) else {
            continue;
        };
        parsed_any = true;
        if checkpoint.format_version != STARMAP_CHECKPOINT_FORMAT_VERSION
            || checkpoint.starmap_id != starmap_id
            || checkpoint.device_id != device_id
        {
            continue;
        }
        current_candidates += 1;
        if checkpoint.state_sha256.is_empty() {
            checkpoint.state_sha256 = state_sha256(&checkpoint.state);
        }
        if checkpoint.hash_matches() {
            checkpoints.push(checkpoint);
        }
    }
    if had_content && !parsed_any {
        return Err(Error::Other(format!(
            "corrupt legacy starmap checkpoint log: {}",
            path.display()
        )));
    }
    if current_candidates > 0 && checkpoints.is_empty() {
        return Err(Error::Other(format!(
            "legacy starmap checkpoint log has no valid checkpoint: {}",
            path.display()
        )));
    }
    Ok(Some(sort_dedup_checkpoints(checkpoints)))
}

fn legacy_checkpoint_headers(
    root: &Path,
    device_id: &str,
    starmap_id: &str,
) -> Result<Vec<StarMapCheckpointHeader>> {
    Ok(read_legacy_checkpoints(root, device_id, starmap_id)?
        .unwrap_or_default()
        .iter()
        .map(StarMapProofCheckpoint::header)
        .collect())
}

/// 从独立快照恢复轻量索引项。只在索引缺失/损坏时使用。
fn snapshot_checkpoint_headers(
    root: &Path,
    device_id: &str,
    starmap_id: &str,
) -> Result<Vec<StarMapCheckpointHeader>> {
    let dir = starmap_checkpoint_snapshots_dir(root, device_id, starmap_id);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut headers = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        let Some(position) = snapshot_position_from_file_name(file_name) else {
            continue;
        };
        let content = match fs::read_to_string(entry.path()) {
            Ok(content) => content,
            Err(_) => continue,
        };
        let Ok(mut checkpoint) = serde_json::from_str::<StarMapProofCheckpoint>(&content) else {
            continue;
        };
        if checkpoint.format_version != STARMAP_CHECKPOINT_FORMAT_VERSION
            || checkpoint.starmap_id != starmap_id
            || checkpoint.device_id != device_id
            || checkpoint.position != position
            || !checkpoint.hash_matches()
        {
            continue;
        }
        if checkpoint.state_sha256.is_empty() {
            checkpoint.state_sha256 = state_sha256(&checkpoint.state);
        }
        headers.push(checkpoint.header());
    }
    Ok(sort_dedup_headers(headers))
}

/// 合并独立快照与旧 JSONL 中保存的事实，供索引恢复使用。
fn recover_checkpoint_headers(
    root: &Path,
    device_id: &str,
    starmap_id: &str,
) -> Result<Vec<StarMapCheckpointHeader>> {
    let mut headers = snapshot_checkpoint_headers(root, device_id, starmap_id)?;
    for header in legacy_checkpoint_headers(root, device_id, starmap_id)? {
        if !headers
            .iter()
            .any(|existing| existing.position == header.position)
        {
            headers.push(header);
        }
    }
    Ok(sort_dedup_headers(headers))
}

fn snapshot_path(
    root: &Path,
    device_id: &str,
    starmap_id: &str,
    header: &StarMapCheckpointHeader,
) -> PathBuf {
    starmap_checkpoint_snapshots_dir(root, device_id, starmap_id).join(&header.snapshot_file)
}

/// 写路径：把旧 JSONL 中尚未由有效快照覆盖的位置落成独立快照。
/// `protected` 中的位置已有有效快照，不能被旧日志覆盖。调用方必须持有索引锁。
fn materialize_legacy_checkpoints(
    root: &Path,
    device_id: &str,
    starmap_id: &str,
    protected: &std::collections::HashSet<u64>,
) -> Result<Vec<StarMapCheckpointHeader>> {
    let Some(checkpoints) = read_legacy_checkpoints(root, device_id, starmap_id)? else {
        return Ok(Vec::new());
    };
    let mut headers = Vec::new();
    for checkpoint in &checkpoints {
        if protected.contains(&checkpoint.position) {
            continue;
        }
        write_snapshot(root, checkpoint)?;
        headers.push(checkpoint.header());
    }
    Ok(headers)
}

fn merge_headers(
    mut headers: Vec<StarMapCheckpointHeader>,
    extra: Vec<StarMapCheckpointHeader>,
) -> Vec<StarMapCheckpointHeader> {
    for header in extra {
        if !headers
            .iter()
            .any(|existing| existing.position == header.position)
        {
            headers.push(header);
        }
    }
    sort_dedup_headers(headers)
}

fn validate_partition(device_id: &str, starmap_id: &str) -> Result<()> {
    if !valid_starmap_id(device_id) || !valid_starmap_id(starmap_id) {
        return Err(Error::Other(format!(
            "invalid starmap checkpoint partition: device={device_id:?} starmap={starmap_id:?}"
        )));
    }
    Ok(())
}

/// 读取轻量索引。正常路径只读小索引；索引损坏时从独立快照/旧日志确定性恢复，
/// 无法恢复任何事实时明确报错，绝不把损坏索引当成“空历史”。
fn load_checkpoint_index(
    root: &Path,
    device_id: &str,
    starmap_id: &str,
) -> Result<Vec<StarMapCheckpointHeader>> {
    validate_partition(device_id, starmap_id)?;
    let path = starmap_checkpoint_index_path(root, device_id, starmap_id);
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let headers = snapshot_checkpoint_headers(root, device_id, starmap_id)?;
            return Ok(merge_headers(
                headers,
                legacy_checkpoint_headers(root, device_id, starmap_id)?,
            ));
        }
        Err(error) => return Err(error.into()),
    };
    let index = match serde_json::from_str::<StarMapCheckpointIndex>(&content) {
        Ok(index) if index.format_version == STARMAP_CHECKPOINT_INDEX_FORMAT_VERSION => index,
        _ => return recover_or_error(root, device_id, starmap_id, &path),
    };
    let raw_len = index.checkpoints.len();
    let headers: Vec<StarMapCheckpointHeader> = index
        .checkpoints
        .into_iter()
        .filter(|header| header_is_consistent(header, device_id, starmap_id))
        .collect();
    let headers = sort_dedup_headers(headers);
    if headers.len() == raw_len {
        if headers.is_empty() {
            // 合法空索引不应掩盖快照目录里仍存在的事实。
            let recovered = recover_checkpoint_headers(root, device_id, starmap_id)?;
            if !recovered.is_empty() {
                return Ok(recovered);
            }
        }
        return Ok(headers);
    }
    let recovered = recover_checkpoint_headers(root, device_id, starmap_id)?;
    if recovered.is_empty() && headers.is_empty() {
        return Err(Error::Other(format!(
            "corrupt starmap checkpoint index: {}",
            path.display()
        )));
    }
    Ok(if recovered.is_empty() {
        headers
    } else {
        recovered
    })
}

fn recover_or_error(
    root: &Path,
    device_id: &str,
    starmap_id: &str,
    index_path: &Path,
) -> Result<Vec<StarMapCheckpointHeader>> {
    let recovered = recover_checkpoint_headers(root, device_id, starmap_id)?;
    if recovered.is_empty() {
        return Err(Error::Other(format!(
            "corrupt starmap checkpoint index: {}",
            index_path.display()
        )));
    }
    Ok(recovered)
}

/// 写路径一次性迁移：索引不存在时把旧 JSONL 与孤立快照合并为轻量索引。
/// 普通读取不会调用该函数，因此不会替其它来源设备写分区文件。
pub fn ensure_checkpoint_storage(root: &Path, device_id: &str, starmap_id: &str) -> Result<()> {
    validate_partition(device_id, starmap_id)?;
    let index_path = starmap_checkpoint_index_path(root, device_id, starmap_id);
    if index_path.is_file() {
        return Ok(());
    }
    let _guard = checkpoint_index_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if index_path.is_file() {
        return Ok(());
    }
    let legacy_path = starmap_checkpoint_path(root, device_id, starmap_id);
    let legacy_present = legacy_path.is_file();
    let snapshots = snapshot_checkpoint_headers(root, device_id, starmap_id)?;
    let protected: std::collections::HashSet<u64> =
        snapshots.iter().map(|header| header.position).collect();
    let headers = merge_headers(
        snapshots,
        materialize_legacy_checkpoints(root, device_id, starmap_id, &protected)?,
    );
    if headers.is_empty() && !legacy_present {
        return Ok(());
    }
    write_index(root, device_id, starmap_id, &headers)?;
    if legacy_present {
        let _ = fs::rename(&legacy_path, migrated_checkpoint_path(&legacy_path));
    }
    Ok(())
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

fn checkpoint_matches_header(
    mut checkpoint: StarMapProofCheckpoint,
    header: &StarMapCheckpointHeader,
) -> Option<StarMapProofCheckpoint> {
    if checkpoint.state_sha256.is_empty() {
        checkpoint.state_sha256 = state_sha256(&checkpoint.state);
    }
    let matches = checkpoint.format_version == STARMAP_CHECKPOINT_FORMAT_VERSION
        && checkpoint.starmap_id == header.starmap_id
        && checkpoint.device_id == header.device_id
        && checkpoint.position == header.position
        && checkpoint.event_chain_head == header.event_chain_head
        && checkpoint.created_at_ms == header.created_at_ms
        && checkpoint.reason == header.reason
        && checkpoint.source_device_id == header.source_device_id
        && checkpoint.history_complete == header.history_complete
        && !header.state_sha256.is_empty()
        && checkpoint.state_sha256 == header.state_sha256
        && checkpoint.state_root == header.state_root
        && checkpoint.hash_matches();
    matches.then_some(checkpoint)
}

fn legacy_checkpoint_at(
    root: &Path,
    device_id: &str,
    starmap_id: &str,
    header: &StarMapCheckpointHeader,
) -> Result<Option<StarMapProofCheckpoint>> {
    let Some(checkpoints) = read_legacy_checkpoints(root, device_id, starmap_id)? else {
        return Ok(None);
    };
    Ok(checkpoints
        .into_iter()
        .find(|checkpoint| checkpoint.position == header.position)
        .and_then(|checkpoint| checkpoint_matches_header(checkpoint, header)))
}

fn read_snapshot_checkpoint(
    root: &Path,
    device_id: &str,
    starmap_id: &str,
    header: &StarMapCheckpointHeader,
) -> Result<Option<StarMapProofCheckpoint>> {
    let path = snapshot_path(root, device_id, starmap_id, header);
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::Other(format!(
                "starmap checkpoint snapshot missing: {}",
                path.display()
            )));
        }
        Err(error) => return Err(error.into()),
    };
    let checkpoint = serde_json::from_str::<StarMapProofCheckpoint>(&content).map_err(|error| {
        Error::Other(format!(
            "corrupt starmap checkpoint snapshot {}: {error}",
            path.display()
        ))
    })?;
    Ok(checkpoint_matches_header(checkpoint, header))
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
    match read_snapshot_checkpoint(root, device_id, starmap_id, &header) {
        Ok(Some(checkpoint)) => Ok(Some(checkpoint)),
        // 索引/旧日志存在但快照尚未迁移时，旧 JSONL 仍可作为只读来源。
        Ok(None) => legacy_checkpoint_at(root, device_id, starmap_id, &header),
        Err(error) => match legacy_checkpoint_at(root, device_id, starmap_id, &header)? {
            Some(checkpoint) => Ok(Some(checkpoint)),
            None => Err(error),
        },
    }
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
    match load_checkpoint_index(root, device_id, starmap_id) {
        Ok(headers) if headers.is_empty() => false,
        Ok(headers) => {
            // 索引存在但快照全缺失/未迁移时不能算作可用历史，否则旧星图
            // 永远无法补建基线。旧 JSONL 中的同位置事实仍算有效来源。
            if headers.iter().any(|header| {
                snapshot_path(root, &header.device_id, &header.starmap_id, header).is_file()
            }) {
                return true;
            }
            read_legacy_checkpoints(root, device_id, starmap_id)
                .ok()
                .flatten()
                .is_some_and(|checkpoints| {
                    checkpoints.iter().any(|checkpoint| {
                        headers
                            .iter()
                            .any(|header| header.position == checkpoint.position)
                    })
                })
        }
        // 索引损坏且无法读取时按“可能有历史”处理，避免写基线覆盖既有事实。
        Err(_) => {
            starmap_checkpoint_index_path(root, device_id, starmap_id).exists()
                || starmap_checkpoint_path(root, device_id, starmap_id).exists()
                || starmap_checkpoint_snapshots_dir(root, device_id, starmap_id).is_dir()
        }
    }
}

/// 全量枚举检查点（导出/证明等需要完整读取的路径使用）。
pub fn load_starmap_checkpoints(
    root: &Path,
    device_id: &str,
    starmap_id: &str,
) -> Result<Vec<StarMapProofCheckpoint>> {
    let headers = load_checkpoint_index(root, device_id, starmap_id)?;
    let mut checkpoints = Vec::new();
    let mut failures = Vec::new();
    for header in headers.iter() {
        match read_snapshot_checkpoint(root, device_id, starmap_id, header) {
            Ok(Some(checkpoint)) => checkpoints.push(checkpoint),
            Ok(None) => failures.push((
                header.clone(),
                Error::Other(format!(
                    "starmap checkpoint snapshot does not match index at position {}",
                    header.position
                )),
            )),
            Err(error) => failures.push((header.clone(), error)),
        }
    }
    if !failures.is_empty() {
        // 旧 JSONL 兼容分支只解析一次，避免导出时按位置重复扫描。
        if let Some(legacy) = read_legacy_checkpoints(root, device_id, starmap_id)? {
            for checkpoint in legacy {
                let Some(index) = failures
                    .iter()
                    .position(|(header, _)| header.position == checkpoint.position)
                else {
                    continue;
                };
                let Some(checkpoint) = checkpoint_matches_header(checkpoint, &failures[index].0)
                else {
                    continue;
                };
                checkpoints.push(checkpoint);
                failures.swap_remove(index);
            }
        }
    }
    if let Some((_, error)) = failures.into_iter().next() {
        return Err(error);
    }
    Ok(sort_dedup_checkpoints(checkpoints))
}

/// 追加检查点；按分支 + position 幂等，索引与快照同一临界区更新。
pub fn append_starmap_checkpoint(root: &Path, checkpoint: &StarMapProofCheckpoint) -> Result<bool> {
    validate_partition(&checkpoint.device_id, &checkpoint.starmap_id)?;
    if checkpoint.format_version != STARMAP_CHECKPOINT_FORMAT_VERSION {
        return Err(Error::Other(format!(
            "unsupported starmap checkpoint format: {}",
            checkpoint.format_version
        )));
    }
    if checkpoint.state_sha256.is_empty() || !checkpoint.hash_matches() {
        return Err(Error::Other(format!(
            "invalid starmap checkpoint state hash at position {}",
            checkpoint.position
        )));
    }
    let _guard = checkpoint_index_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let index_path =
        starmap_checkpoint_index_path(root, &checkpoint.device_id, &checkpoint.starmap_id);
    let legacy_path = starmap_checkpoint_path(root, &checkpoint.device_id, &checkpoint.starmap_id);
    let index_present = index_path.is_file();
    let legacy_present = legacy_path.is_file();
    let mut headers = if index_present {
        load_checkpoint_index(root, &checkpoint.device_id, &checkpoint.starmap_id)?
    } else {
        let snapshots =
            snapshot_checkpoint_headers(root, &checkpoint.device_id, &checkpoint.starmap_id)?;
        let protected: std::collections::HashSet<u64> =
            snapshots.iter().map(|header| header.position).collect();
        merge_headers(
            snapshots,
            materialize_legacy_checkpoints(
                root,
                &checkpoint.device_id,
                &checkpoint.starmap_id,
                &protected,
            )?,
        )
    };
    let existing_index = headers
        .iter()
        .position(|header| header.position == checkpoint.position);
    let existing_valid = existing_index.is_some_and(|index| {
        read_snapshot_checkpoint(
            root,
            &checkpoint.device_id,
            &checkpoint.starmap_id,
            &headers[index],
        )
        .ok()
        .flatten()
        .is_some()
    });
    if existing_index.is_some() && existing_valid {
        // 已有同位置有效检查点；如果索引是本次迁移才建立的，仍需落索引。
        if !index_present {
            write_index(
                root,
                &checkpoint.device_id,
                &checkpoint.starmap_id,
                &headers,
            )?;
        }
        if !index_present && legacy_present {
            let _ = fs::rename(&legacy_path, migrated_checkpoint_path(&legacy_path));
        }
        return Ok(false);
    }
    // 先写快照再写索引：任何时刻索引都不会指向不存在的快照。
    write_snapshot(root, checkpoint)?;
    match existing_index {
        Some(index) => headers[index] = checkpoint.header(),
        None => headers.push(checkpoint.header()),
    }
    let headers = sort_dedup_headers(headers);
    write_index(
        root,
        &checkpoint.device_id,
        &checkpoint.starmap_id,
        &headers,
    )?;
    if !index_present && legacy_present {
        let _ = fs::rename(&legacy_path, migrated_checkpoint_path(&legacy_path));
    }
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
        match load_checkpoint_at(root, &device, starmap_id, 0) {
            Ok(Some(checkpoint)) => return Ok(Some(checkpoint)),
            Ok(None) => {}
            // 损坏的其它分支不能阻止继续寻找可用基线。
            Err(_) => continue,
        }
    }
    Ok(None)
}

/// 判断某个分支是否有可用的完整基线（位置 0 的检查点）。
/// 只有快照实际载入并通过校验时才视为有效基线。
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

    fn checkpoint(position: u64) -> StarMapProofCheckpoint {
        let mut state = StarMapProofState::empty("sm1");
        state.root_starmap_ids.push("sm1".to_string());
        StarMapProofCheckpoint::new(
            "sm1",
            "device1",
            position,
            state,
            CHECKPOINT_REASON_CREATED,
            true,
        )
    }

    #[test]
    fn index_and_snapshot_round_trip() {
        let dir = std::env::temp_dir().join(format!(
            "sujian-starmap-checkpoint-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        let checkpoint = checkpoint(0);
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

    #[test]
    fn corrupt_index_recovers_from_snapshots() {
        let dir = std::env::temp_dir().join(format!(
            "sujian-starmap-checkpoint-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        assert!(append_starmap_checkpoint(&dir, &checkpoint(0)).unwrap());
        assert!(append_starmap_checkpoint(&dir, &checkpoint(64)).unwrap());
        let index_path = starmap_checkpoint_index_path(&dir, "device1", "sm1");
        fs::write(&index_path, "{broken").unwrap();
        let headers = list_checkpoint_headers(&dir, "device1", "sm1").unwrap();
        assert_eq!(
            headers
                .iter()
                .map(|header| header.position)
                .collect::<Vec<_>>(),
            vec![0, 64]
        );
        assert_eq!(
            last_checkpoint_position(&dir, "device1", "sm1").unwrap(),
            Some(64)
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_snapshot_is_explicit_and_repairable() {
        let dir = std::env::temp_dir().join(format!(
            "sujian-starmap-checkpoint-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        let snapshot = checkpoint(64);
        assert!(append_starmap_checkpoint(&dir, &checkpoint(0)).unwrap());
        assert!(append_starmap_checkpoint(&dir, &snapshot).unwrap());
        let path =
            starmap_checkpoint_snapshots_dir(&dir, "device1", "sm1").join(snapshot_file_name(64));
        fs::remove_file(&path).unwrap();
        assert!(load_checkpoint_at(&dir, "device1", "sm1", 64).is_err());
        assert!(has_branch_baseline(&dir, "device1", "sm1")
            .unwrap()
            .is_some());
        assert!(append_starmap_checkpoint(&dir, &snapshot).unwrap());
        assert!(load_checkpoint_at(&dir, "device1", "sm1", 64)
            .unwrap()
            .is_some());
        let _ = fs::remove_dir_all(dir);
    }
}
