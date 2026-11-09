//! 本地工作区对象变更事件流。
//!
//! 该文件只服务于同机多进程协作（桌面 UI、MCP server 等），不参与同步。
//! 客户端通过字节偏移增量读取，收到属于当前星图的变更后重新从 Core 读取
//! 最新版本，避免长期持有过期的图数据。

use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::Result;

const EVENTS_PATH: &str = "app-meta/workspace-events.local.jsonl";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceChangeEvent {
    pub occurred_at_ms: u64,
    pub kind: String,
    pub starmap_id: String,
    pub object_type: String,
    pub object_id: String,
    pub revision: u64,
    /// 产生本次变更的 Core 事务；旧事件没有该字段。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transaction_id: Option<String>,
    /// 统计事件存储代际，供统计页判断是否需要重新请求快照。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats_generation: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceChangePage {
    pub events: Vec<WorkspaceChangeEvent>,
    pub next_offset: u64,
}

fn events_path(app_data_root: &Path) -> PathBuf {
    app_data_root.join(EVENTS_PATH)
}

pub fn append_event(
    app_data_root: &Path,
    kind: &str,
    starmap_id: &str,
    object_type: &str,
    object_id: &str,
    revision: u64,
) -> Result<()> {
    append_event_with_metadata(
        app_data_root,
        kind,
        starmap_id,
        object_type,
        object_id,
        revision,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn append_event_with_metadata(
    app_data_root: &Path,
    kind: &str,
    starmap_id: &str,
    object_type: &str,
    object_id: &str,
    revision: u64,
    transaction_id: Option<&str>,
    stats_generation: Option<u64>,
) -> Result<()> {
    let path = events_path(app_data_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let event = WorkspaceChangeEvent {
        occurred_at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0),
        kind: kind.to_string(),
        starmap_id: starmap_id.to_string(),
        object_type: object_type.to_string(),
        object_id: object_id.to_string(),
        revision,
        transaction_id: transaction_id.map(str::to_string),
        stats_generation,
    };
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", serde_json::to_string(&event)?)?;
    Ok(())
}

/// 事务成功提交后发布统计事件存储变更，桌面统计页据此重新请求快照。
pub fn append_stats_event(
    app_data_root: &Path,
    transaction_id: &str,
    stats_generation: u64,
) -> Result<()> {
    append_event_with_metadata(
        app_data_root,
        "stats",
        "",
        "",
        "",
        0,
        Some(transaction_id),
        Some(stats_generation),
    )
}

/// 星图结构变化（新建 / 绑定 / 嵌入 / 解绑 / 删除）事件。
///
/// 与普通对象写入分开：桌面端据此重载左侧目录/根星图列表，而不是只把
/// 当前画布重绘一次。事件仍带真实 starmapId 和对象 ID，便于追溯结构变更。
pub fn append_structure_event(
    app_data_root: &Path,
    starmap_id: &str,
    object_type: &str,
    object_id: &str,
) -> Result<()> {
    append_event(
        app_data_root,
        "starmapStructure",
        starmap_id,
        object_type,
        object_id,
        0,
    )
}

pub fn read_since(app_data_root: &Path, offset: u64) -> Result<WorkspaceChangePage> {
    let path = events_path(app_data_root);
    if !path.is_file() {
        return Ok(WorkspaceChangePage {
            events: Vec::new(),
            next_offset: 0,
        });
    }
    let mut file = fs::File::open(path)?;
    let length = file.metadata()?.len();
    let start = offset.min(length);
    file.seek(SeekFrom::Start(start))?;
    let mut content = Vec::new();
    file.read_to_end(&mut content)?;
    let mut events = Vec::new();
    let mut consumed = 0usize;
    for line in content.split_inclusive(|byte| *byte == b'\n') {
        if !line.ends_with(b"\n") {
            // 最后一条可能仍在写入中；保留到下一次读取。
            break;
        }
        consumed += line.len();
        let line = line.trim_ascii();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_slice(line) {
            Ok(event) => events.push(event),
            Err(error) => {
                eprintln!("[workspace-events] skip malformed event: {error}");
            }
        }
    }
    Ok(WorkspaceChangePage {
        events,
        next_offset: start.saturating_add(consumed as u64),
    })
}
