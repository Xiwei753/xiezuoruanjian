//! 证明检查点：完整正文快照，用于加速回放。
//!
//! 检查点是索引，不是事实源。事实始终是 `*.events.jsonl` 里的逐次变更；
//! 检查点只保存某个设备内序号之前的完整正文和摘要，回放时从最近检查点
//! 继续应用后续事件。
//!
//! 路径与事件同在来源设备分区：
//! `stats/events/<device_id>/proof/<chapter_id>.checkpoints.jsonl`。
//! 证明连续性缺口保存在同目录
//! `stats/events/<device_id>/proof/<chapter_id>.gaps/<record-sha256>.json`。
//! 设备只写自己的分区；同步来的其它设备分区只读。

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::writing_stats::proof::{text_sha256, ProofContinuityGap, PROOF_FORMAT_VERSION};

pub const CHECKPOINT_FORMAT_VERSION: u32 = PROOF_FORMAT_VERSION;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProofCheckpoint {
    pub format_version: u32,
    pub chapter_id: String,
    pub device_id: String,
    /// 该检查点覆盖到的设备内事件序号；0 表示事件链起点之前的基线。
    pub seq: u64,
    pub session_id: String,
    pub timestamp_ms: i64,
    /// 完整正文纯文本。
    pub text: String,
    pub text_sha256: String,
    /// 与事件 proof.after_content_hash 相同的链式摘要。
    pub content_hash: String,
    /// 覆盖到的最后一条事件哈希；基线为空。
    #[serde(default)]
    pub event_hash: String,
    pub created_at_ms: i64,
    /// baseline / save / periodic / archive / sync_import
    pub reason: String,
    /// 外部来源设备：同步导入基线记录远端设备；本机生成留空。
    #[serde(default)]
    pub source_device_id: String,
}

impl ProofCheckpoint {
    pub fn new(
        chapter_id: &str,
        device_id: &str,
        seq: u64,
        session_id: &str,
        timestamp_ms: i64,
        text: &str,
        content_hash: &str,
        event_hash: &str,
        reason: &str,
    ) -> Self {
        Self {
            format_version: CHECKPOINT_FORMAT_VERSION,
            chapter_id: chapter_id.to_string(),
            device_id: device_id.to_string(),
            seq,
            session_id: session_id.to_string(),
            timestamp_ms,
            text: text.to_string(),
            text_sha256: text_sha256(text),
            content_hash: content_hash.to_string(),
            event_hash: event_hash.to_string(),
            created_at_ms: chrono::Utc::now().timestamp_millis(),
            reason: reason.to_string(),
            source_device_id: String::new(),
        }
    }
}

/// 检查点相对路径（相对 app data root），供存储与同步识别共用。
pub fn checkpoint_relative_path(device_id: &str, chapter_id: &str) -> String {
    format!(
        "stats/events/{device_id}/proof/{}.checkpoints.jsonl",
        sanitize_segment(chapter_id)
    )
}

/// 事件归档段相对路径（相对 app data root）。
pub fn archive_relative_path(device_id: &str, date: &str) -> String {
    format!("stats/events/{device_id}/proof/archive/{date}.events.jsonl.zst")
}

pub fn checkpoint_path(app_data_root: &Path, device_id: &str, chapter_id: &str) -> PathBuf {
    app_data_root.join(checkpoint_relative_path(device_id, chapter_id))
}

/// 把某个来源设备的某一天事件文件封口为不可变的 zstd 段。
///
/// 只在当天事件已经停止追加后调用；成功后删除原 JSONL，避免同一批事件
/// 被重复读取。目标文件已存在时保持原样并返回它（归档段不可改写）。
pub fn archive_event_day(
    app_data_root: &Path,
    device_id: &str,
    date: &str,
) -> crate::Result<Option<PathBuf>> {
    if chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
        return Err(crate::Error::Other(format!(
            "invalid proof archive date: {date}"
        )));
    }
    let source = app_data_root
        .join("stats/events")
        .join(sanitize_segment(device_id))
        .join(format!("{date}.events.jsonl"));
    if !source.is_file() {
        return Ok(None);
    }
    let destination = app_data_root.join(archive_relative_path(device_id, date));
    if destination.is_file() {
        // 崩溃可能发生在 rename 之后、删除源文件之前。归档与源完全一致时
        // 完成收尾；源若有迟到事件则保留两段，读取侧按 event_id 去重。
        let source_bytes = fs::read(&source)?;
        let archive_bytes =
            zstd::stream::decode_all(fs::File::open(&destination)?).map_err(|error| {
                crate::Error::Other(format!("decode proof archive failed: {error}"))
            })?;
        if archive_bytes == source_bytes {
            fs::remove_file(&source)?;
        }
        return Ok(Some(destination));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = fs::read(&source)?;
    let encoded = zstd::stream::encode_all(bytes.as_slice(), 9)
        .map_err(|error| crate::Error::Other(format!("encode proof archive failed: {error}")))?;
    let temporary = destination.with_extension("zst.tmp");
    {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(&encoded)?;
        file.sync_all()?;
    }
    fs::rename(&temporary, &destination)?;
    // 只有归档段落盘并完成 rename 后才移除原始追加文件。
    fs::remove_file(&source)?;
    Ok(Some(destination))
}

/// 检查点索引文件的格式版本。索引只是定位加速，损坏或过期时回退全量扫描。
const CHECKPOINT_INDEX_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CheckpointIndex {
    format_version: u32,
    /// 写入索引时的文件长度，用于发现索引之后又追加了检查点。
    file_len: u64,
    last_offset: u64,
    last_len: u64,
    last_seq: u64,
    last_text_sha256: String,
    last_content_hash: String,
    last_event_hash: String,
}

struct CheckpointScan {
    last: Option<ProofCheckpoint>,
    last_offset: u64,
    last_len: u64,
    file_len: u64,
}

fn checkpoint_index_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".idx");
    PathBuf::from(name)
}

fn read_checkpoint_index(path: &Path) -> Option<CheckpointIndex> {
    let bytes = fs::read(checkpoint_index_path(path)).ok()?;
    let index: CheckpointIndex = serde_json::from_slice(&bytes).ok()?;
    (index.format_version == CHECKPOINT_INDEX_FORMAT_VERSION).then_some(index)
}

fn write_checkpoint_index(path: &Path, index: &CheckpointIndex) -> crate::Result<()> {
    let serialized = serde_json::to_string(index)?;
    crate::storage::atomic_write_string(&checkpoint_index_path(path), &serialized)
}

/// 从 `start` 开始扫描到文件末尾，记录最后一条属于本章节的检查点及其字节区间。
/// `start` 必须是行边界；调用方负责校验，否则会从半行开始解析。
fn scan_checkpoints_from(
    path: &Path,
    chapter_id: &str,
    start: u64,
) -> crate::Result<CheckpointScan> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(start))?;
    let mut reader = BufReader::new(file);
    let mut offset = start;
    let mut last = None;
    let mut last_offset = 0;
    let mut last_len = 0;
    loop {
        let mut buffer = Vec::new();
        let read = reader.read_until(b'\n', &mut buffer)?;
        if read == 0 {
            break;
        }
        let line_len = read as u64;
        let line = buffer.strip_suffix(b"\n").unwrap_or(&buffer);
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if !line.iter().all(u8::is_ascii_whitespace) {
            match serde_json::from_slice::<ProofCheckpoint>(line) {
                Ok(checkpoint) if checkpoint.chapter_id == chapter_id => {
                    last = Some(checkpoint);
                    last_offset = offset;
                    last_len = line_len;
                }
                Ok(_) => {}
                Err(error) => {
                    eprintln!(
                        "[writing_stats] 跳过损坏的 proof checkpoint {}: {error}",
                        path.display()
                    );
                }
            }
        }
        offset = offset.saturating_add(line_len);
    }
    Ok(CheckpointScan {
        last,
        last_offset,
        last_len,
        file_len: offset,
    })
}

fn checkpoint_matches_index(checkpoint: &ProofCheckpoint, index: &CheckpointIndex) -> bool {
    checkpoint.seq == index.last_seq
        && checkpoint.text_sha256 == index.last_text_sha256
        && checkpoint.content_hash == index.last_content_hash
        && checkpoint.event_hash == index.last_event_hash
}

fn index_for_scan(scan: &CheckpointScan, checkpoint: &ProofCheckpoint) -> CheckpointIndex {
    CheckpointIndex {
        format_version: CHECKPOINT_INDEX_FORMAT_VERSION,
        file_len: scan.file_len,
        last_offset: scan.last_offset,
        last_len: scan.last_len,
        last_seq: checkpoint.seq,
        last_text_sha256: checkpoint.text_sha256.clone(),
        last_content_hash: checkpoint.content_hash.clone(),
        last_event_hash: checkpoint.event_hash.clone(),
    }
}

fn read_line_range(path: &Path, offset: u64, length: u64) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let length = usize::try_from(length).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "proof checkpoint line length exceeds address space",
        )
    })?;
    let mut bytes = vec![0_u8; length];
    file.read_exact(&mut bytes)?;
    let line = bytes.strip_suffix(b"\n").unwrap_or(&bytes);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    String::from_utf8(line.to_vec())
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

fn is_line_boundary(path: &Path, file_len: u64, offset: u64) -> bool {
    if offset == 0 {
        return true;
    }
    if offset > file_len {
        return false;
    }
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    if file.seek(SeekFrom::Start(offset - 1)).is_err() {
        return false;
    }
    let mut byte = [0_u8; 1];
    file.read_exact(&mut byte).is_ok() && byte[0] == b'\n'
}

/// 追加检查点。同 seq、同正文摘要的检查点不重复写。
pub fn append_checkpoint(app_data_root: &Path, checkpoint: &ProofCheckpoint) -> crate::Result<()> {
    let path = checkpoint_path(app_data_root, &checkpoint.device_id, &checkpoint.chapter_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(last) =
        load_last_checkpoint(app_data_root, &checkpoint.device_id, &checkpoint.chapter_id)?
    {
        if last.seq == checkpoint.seq {
            if last.text_sha256 == checkpoint.text_sha256
                && last.content_hash == checkpoint.content_hash
                && last.event_hash == checkpoint.event_hash
            {
                return Ok(());
            }
            return Err(crate::Error::Other(format!(
                "检查点 seq {} 已有不同正文，拒绝覆盖；外部变更必须另起事件链",
                checkpoint.seq
            )));
        }
        if last.seq > checkpoint.seq {
            return Err(crate::Error::Other(format!(
                "检查点 seq {} 早于已写入的 seq {}，拒绝乱序追加",
                checkpoint.seq, last.seq
            )));
        }
    }
    if checkpoint.text_sha256 != text_sha256(&checkpoint.text) {
        return Err(crate::Error::Other(format!(
            "检查点 seq {} 的正文摘要不一致",
            checkpoint.seq
        )));
    }
    let serialized = serde_json::to_string(checkpoint)?;
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(&path)?;
    let mut offset = file.metadata()?.len();
    // 崩溃可能留下半行，旧版文件也可能最后一行没有换行。这里只补一个
    // 分隔符，不截断已有内容：最后一条若本身有效，不能因为缺换行被丢掉。
    if offset > 0 {
        file.seek(SeekFrom::Start(offset - 1))?;
        let mut last_byte = [0_u8; 1];
        file.read_exact(&mut last_byte)?;
        if last_byte[0] != b'\n' {
            file.write_all(b"\n")?;
            offset = offset.saturating_add(1);
        }
    }
    file.write_all(serialized.as_bytes())?;
    file.write_all(b"\n")?;
    file.flush()?;
    let line_len = serialized.len().saturating_add(1) as u64;
    let index = CheckpointIndex {
        format_version: CHECKPOINT_INDEX_FORMAT_VERSION,
        file_len: offset.saturating_add(line_len),
        last_offset: offset,
        last_len: line_len,
        last_seq: checkpoint.seq,
        last_text_sha256: checkpoint.text_sha256.clone(),
        last_content_hash: checkpoint.content_hash.clone(),
        last_event_hash: checkpoint.event_hash.clone(),
    };
    // 索引写失败不影响已追加的事实；下次读取会全量重建索引。
    if let Err(error) = write_checkpoint_index(&path, &index) {
        log::warn!(
            "[writing_stats] 写入 proof checkpoint 索引失败 {}: {error}",
            path.display()
        );
    }
    Ok(())
}

/// 尝试用索引直接读取最后一条检查点；索引过期但文件只增长了尾部时，
/// 只扫描 `index.file_len..EOF` 并刷新索引。
fn load_checkpoint_from_index(
    path: &Path,
    chapter_id: &str,
    index: &CheckpointIndex,
    file_len: u64,
) -> crate::Result<Option<ProofCheckpoint>> {
    let range_end = index.last_offset.saturating_add(index.last_len);
    if index.file_len > file_len || range_end > file_len {
        return Ok(None);
    }
    let Ok(line) = read_line_range(path, index.last_offset, index.last_len) else {
        return Ok(None);
    };
    let Ok(checkpoint) = serde_json::from_str::<ProofCheckpoint>(&line) else {
        return Ok(None);
    };
    if checkpoint.chapter_id != chapter_id || !checkpoint_matches_index(&checkpoint, index) {
        return Ok(None);
    }
    if index.file_len == file_len {
        return Ok(Some(checkpoint));
    }
    if !is_line_boundary(path, file_len, index.file_len) {
        return Ok(None);
    }
    // 索引之后又追加了检查点（可能发生在 append 成功、索引落盘前崩溃）。
    // 只扫描尾部增量。
    let tail = scan_checkpoints_from(path, chapter_id, index.file_len)?;
    if let Some(latest) = tail.last.as_ref() {
        let latest_index = index_for_scan(&tail, latest);
        if let Err(error) = write_checkpoint_index(path, &latest_index) {
            log::warn!(
                "[writing_stats] 重建 proof checkpoint 索引失败 {}: {error}",
                path.display()
            );
        }
        return Ok(Some(latest.clone()));
    }
    // 尾部只有空行/损坏行：索引仍指向最后一条有效检查点。
    let mut refreshed = index.clone();
    refreshed.file_len = file_len;
    if let Err(error) = write_checkpoint_index(path, &refreshed) {
        log::warn!(
            "[writing_stats] 刷新 proof checkpoint 索引失败 {}: {error}",
            path.display()
        );
    }
    Ok(Some(checkpoint))
}

/// 读取某个设备/章节的最后一个检查点。损坏行跳过并告警，不阻断回放。
///
/// 优先使用旁路索引直接定位最后一行；索引缺失、过期或损坏时回退一次全量扫描
/// 并重建索引。正常连续写入只读取最后一条，不随历史检查点数量线性增长。
pub fn load_last_checkpoint(
    app_data_root: &Path,
    device_id: &str,
    chapter_id: &str,
) -> crate::Result<Option<ProofCheckpoint>> {
    let path = checkpoint_path(app_data_root, device_id, chapter_id);
    if !path.exists() {
        return Ok(None);
    }
    let file_len = fs::metadata(&path)?.len();

    if let Some(index) = read_checkpoint_index(&path) {
        if let Some(checkpoint) = load_checkpoint_from_index(&path, chapter_id, &index, file_len)? {
            return Ok(Some(checkpoint));
        }
    }

    // 索引缺失/损坏/不匹配：做一次全量扫描并重建；之后恢复正常快速路径。
    let scan = scan_checkpoints_from(&path, chapter_id, 0)?;
    if let Some(last) = scan.last.as_ref() {
        let index = index_for_scan(&scan, last);
        if let Err(error) = write_checkpoint_index(&path, &index) {
            log::warn!(
                "[writing_stats] 重建 proof checkpoint 索引失败 {}: {error}",
                path.display()
            );
        }
    }
    Ok(scan.last)
}

/// 读取检查点文件里的全部快照，按 seq 升序。
pub fn load_checkpoints(
    app_data_root: &Path,
    device_id: &str,
    chapter_id: &str,
) -> crate::Result<Vec<ProofCheckpoint>> {
    let path = checkpoint_path(app_data_root, device_id, chapter_id);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(&path)?;
    let reader = BufReader::new(file);
    let mut checkpoints = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(checkpoint) = serde_json::from_str::<ProofCheckpoint>(&line) {
            if checkpoint.chapter_id == chapter_id {
                checkpoints.push(checkpoint);
            }
        }
    }
    checkpoints.sort_by_key(|checkpoint| checkpoint.seq);
    Ok(checkpoints)
}

/// 选择 seq 不大于目标值、且时间不晚于目标的最近检查点。
pub fn latest_checkpoint_at_or_before(
    checkpoints: &[ProofCheckpoint],
    target_seq: Option<u64>,
    target_time_ms: Option<i64>,
) -> Option<&ProofCheckpoint> {
    checkpoints
        .iter()
        .filter(|checkpoint| {
            target_seq.map_or(true, |seq| checkpoint.seq <= seq)
                && target_time_ms.map_or(true, |time| checkpoint.timestamp_ms <= time)
        })
        .max_by_key(|checkpoint| checkpoint.seq)
}

fn is_safe_segment_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')
}

/// `checkpoint_relative_path()` 生成章节文件名的安全 segment 规则，
/// 供同步路径解析复用，避免两处对“可同步证明文件名”的认定漂移。
pub(crate) fn is_safe_checkpoint_segment(segment: &str) -> bool {
    !segment.is_empty() && segment.chars().all(is_safe_segment_char)
}

fn sanitize_segment(segment: &str) -> String {
    segment
        .chars()
        .map(|ch| if is_safe_segment_char(ch) { ch } else { '_' })
        .collect()
}

/// 章节证明连续性缺口的原子记录目录（与 checkpoint 同目录）。
pub fn continuity_gap_relative_dir(device_id: &str, chapter_id: &str) -> String {
    format!(
        "stats/events/{device_id}/proof/{}.gaps",
        sanitize_segment(chapter_id)
    )
}

fn continuity_gap_dir(app_data_root: &Path, device_id: &str, chapter_id: &str) -> PathBuf {
    app_data_root.join(continuity_gap_relative_dir(device_id, chapter_id))
}

fn continuity_gap_identity(transaction_id: &str, event_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(transaction_id.as_bytes());
    hasher.update([0u8]);
    hasher.update(event_id.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn continuity_gap_path(app_data_root: &Path, gap: &ProofContinuityGap) -> PathBuf {
    continuity_gap_dir(app_data_root, &gap.device_id, &gap.chapter_id).join(format!(
        "{}.json",
        continuity_gap_identity(&gap.transaction_id, &gap.event_id)
    ))
}

/// 原子写一条缺口记录。缺口、基线正文和摘要都在同一个 JSON 文件里，
/// rename 成功即为发布点，不存在 checkpoint 已写而缺口未写的半事务。
pub fn append_continuity_gap(
    app_data_root: &Path,
    gap: &ProofContinuityGap,
) -> crate::Result<bool> {
    let path = continuity_gap_path(app_data_root, gap);
    if path.is_file() {
        let text = fs::read_to_string(&path)?;
        let existing: ProofContinuityGap = serde_json::from_str(&text).map_err(|error| {
            crate::Error::Other(format!(
                "连续性缺口记录损坏 {}: {error}",
                path.display()
            ))
        })?;
        if existing == *gap {
            return Ok(false);
        }
        return Err(crate::Error::Other(format!(
            "连续性缺口 {} 已存在不同内容，拒绝覆盖",
            gap.event_id
        )));
    }
    let Some(parent) = path.parent() else {
        return Err(crate::Error::Other(format!(
            "连续性缺口路径没有父目录: {}",
            path.display()
        )));
    };
    fs::create_dir_all(parent)?;
    let json = serde_json::to_string_pretty(gap)?;
    crate::storage::atomic_write_string(&path, &json)?;
    crate::storage::sync_dir(parent)?;
    Ok(true)
}

/// 读取本章节的全部缺口记录。任何损坏记录都必须显式报错，不能静默跳过，
/// 否则同步会一直停在同一事务上。
pub fn load_continuity_gaps(
    app_data_root: &Path,
    device_id: &str,
    chapter_id: &str,
) -> crate::Result<Vec<ProofContinuityGap>> {
    let dir = continuity_gap_dir(app_data_root, device_id, chapter_id);
    let mut gaps = Vec::new();
    if dir.is_dir() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let text = fs::read_to_string(&path)?;
            let gap: ProofContinuityGap = serde_json::from_str(&text).map_err(|error| {
                crate::Error::Other(format!(
                    "连续性缺口记录损坏 {}: {error}",
                    path.display()
                ))
            })?;
            if gap.device_id == device_id && gap.chapter_id == chapter_id {
                gaps.push(gap);
            }
        }
    }

    gaps.sort_by(|left, right| {
        left.baseline_seq
            .cmp(&right.baseline_seq)
            .then_with(|| left.event_id.cmp(&right.event_id))
    });
    Ok(gaps)
}

/// 读取某设备全部章节的原子连续性缺口记录。
///
/// 仅用于本机一次性证明序号恢复和缺口封存分配序号；普通逐字编辑只查询
/// 章节缓存，不调用这里。任何损坏记录都必须显式报错，否则无法安全判断
/// 当前设备已经占用的证明序号。
pub fn load_device_continuity_gaps(
    app_data_root: &Path,
    device_id: &str,
) -> crate::Result<Vec<ProofContinuityGap>> {
    let proof_dir = app_data_root
        .join("stats/events")
        .join(device_id)
        .join("proof");
    if !proof_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut gaps = Vec::new();
    for entry in fs::read_dir(&proof_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name();
        if !name.to_string_lossy().ends_with(".gaps") {
            continue;
        }
        for file in fs::read_dir(entry.path())? {
            let file = file?;
            let path = file.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let text = fs::read_to_string(&path)?;
            let gap: ProofContinuityGap = serde_json::from_str(&text).map_err(|error| {
                crate::Error::Other(format!("连续性缺口记录损坏 {}: {error}", path.display()))
            })?;
            if gap.device_id == device_id {
                gaps.push(gap);
            }
        }
    }
    gaps.sort_by(|left, right| {
        left.baseline_seq
            .cmp(&right.baseline_seq)
            .then_with(|| left.event_id.cmp(&right.event_id))
    });
    Ok(gaps)
}

/// 列出某章节存在 checkpoint 或缺口记录的设备，供回放选择本机基线。
pub fn list_proof_devices(
    app_data_root: &Path,
    chapter_id: &str,
) -> crate::Result<Vec<String>> {
    let root = app_data_root.join("stats/events");
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut devices = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let device_id = entry.file_name().to_string_lossy().to_string();
        if checkpoint_path(app_data_root, &device_id, chapter_id).is_file()
            || continuity_gap_dir(app_data_root, &device_id, chapter_id).is_dir()
        {
            devices.push(device_id);
        }
    }
    devices.sort();
    Ok(devices)
}
