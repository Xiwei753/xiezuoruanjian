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
    };
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", serde_json::to_string(&event)?)?;
    Ok(())
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
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    let mut events = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str(line) {
            Ok(event) => events.push(event),
            Err(error) => {
                eprintln!("[workspace-events] skip malformed event: {error}");
            }
        }
    }
    Ok(WorkspaceChangePage {
        events,
        next_offset: length,
    })
}
