//! 证明检查点：完整正文快照，用于加速回放。
//!
//! 检查点是索引，不是事实源。事实始终是 `*.events.jsonl` 里的逐次变更；
//! 检查点只保存某个设备内序号之前的完整正文和摘要，回放时从最近检查点
//! 继续应用后续事件。
//!
//! 路径与事件同在来源设备分区：
//! `stats/events/<device_id>/proof/<chapter_id>.checkpoints.jsonl`。
//! 设备只写自己的分区；同步来的其它设备分区只读。

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::writing_stats::proof::{text_sha256, PROOF_FORMAT_VERSION};

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
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    file.write_all(serialized.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

/// 读取某个设备/章节的最后一个检查点。损坏行跳过并告警，不阻断回放。
pub fn load_last_checkpoint(
    app_data_root: &Path,
    device_id: &str,
    chapter_id: &str,
) -> crate::Result<Option<ProofCheckpoint>> {
    let path = checkpoint_path(app_data_root, device_id, chapter_id);
    if !path.exists() {
        return Ok(None);
    }
    let file = fs::File::open(&path)?;
    let reader = BufReader::new(file);
    let mut last = None;
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<ProofCheckpoint>(&line) {
            Ok(checkpoint) if checkpoint.chapter_id == chapter_id => last = Some(checkpoint),
            Ok(_) => {}
            Err(error) => {
                eprintln!(
                    "[writing_stats] 跳过损坏的 proof checkpoint {}: {error}",
                    path.display()
                );
            }
        }
    }
    Ok(last)
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

fn sanitize_segment(segment: &str) -> String {
    segment
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}
