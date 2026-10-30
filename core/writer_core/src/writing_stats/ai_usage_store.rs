//! AI 请求/响应事件的按设备分区存储。
//!
//! 文件布局与写作输入事件一致：`stats/events/<device_id>/YYYY-MM-DD.ai.jsonl`。
//! 每台设备只追加自己的分区；读取时扫描全部已到达分区并按 `event_id` 去重。
//! 同步只搬运文件，不重写发生时间、当地日期或小时。
//!
//! 与对象贡献事件分开存放是有意的：一次 AI 请求/响应可能对应多个对象修改，
//! 不能把输入/输出字数随 `MutationContext` 复制到每条对象事件上。

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::error::Result;
use crate::writing_stats::ai_usage::AiUsageEvent;
use crate::writing_stats::calendar;
use crate::writing_stats::contribution_store::{resolve_device_partition_id, valid_segment};

const EVENTS_ROOT: &str = "stats/events";
const AI_USAGE_SUFFIX: &str = ".ai.jsonl";

/// AI 请求/响应事件存储。
pub struct AiUsageStore {
    app_data_root: PathBuf,
    origin_device_id: Mutex<Option<String>>,
}

impl AiUsageStore {
    pub fn new(app_data_root: &Path) -> Self {
        Self {
            app_data_root: app_data_root.to_path_buf(),
            origin_device_id: Mutex::new(None),
        }
    }

    fn events_root(&self) -> PathBuf {
        self.app_data_root.join(EVENTS_ROOT)
    }

    fn event_file_path(&self, origin: &str, date: &str) -> PathBuf {
        self.events_root()
            .join(origin)
            .join(format!("{date}{AI_USAGE_SUFFIX}"))
    }

    pub fn resolve_origin_device_id(&self) -> Result<String> {
        if let Some(id) = self
            .origin_device_id
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
        {
            return Ok(id);
        }
        let id = resolve_device_partition_id(&self.app_data_root)?;
        if let Ok(mut guard) = self.origin_device_id.lock() {
            *guard = Some(id.clone());
        }
        Ok(id)
    }

    /// 幂等追加一批 AI 请求/响应事件；返回真正新写入的事件数。
    ///
    /// 事件在发生设备上补齐业务日与小时后再落盘；同步到其他设备的事件
    /// 保持原值，读取端不会用当前机器时区重算。
    pub fn record_events(&self, events: Vec<AiUsageEvent>) -> Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }
        let origin = self.resolve_origin_device_id()?;
        let mut by_date: BTreeMap<String, Vec<AiUsageEvent>> = BTreeMap::new();
        for mut event in events {
            // 本机记录的事件一律落入本机分区；同步来的事件走文件搬运，不经过
            // 这里。日期/小时缺失时用当前设备时区补齐，已有值保持不变。
            event.origin_device_id = origin.clone();
            if event.local_date.is_empty() {
                event.local_date = calendar::local_date_at(event.occurred_at_ms)
                    .or_else(|_| calendar::utc_date_at(event.occurred_at_ms))
                    .unwrap_or_default();
            }
            if event.local_hour.is_none() {
                event.local_hour = calendar::local_hour_at(event.occurred_at_ms);
            }
            let date = if event.local_date.is_empty() {
                "1970-01-01".to_string()
            } else {
                event.local_date.clone()
            };
            by_date.entry(date).or_default().push(event);
        }

        let mut written = 0usize;
        for (date, events) in by_date {
            let path = self.event_file_path(&origin, &date);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut existing = read_ids(&path)?;
            let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
            for event in events {
                if !event.event_id.is_empty() && existing.contains(&event.event_id) {
                    continue;
                }
                writeln!(file, "{}", serde_json::to_string(&event)?)?;
                if !event.event_id.is_empty() {
                    existing.insert(event.event_id.clone());
                }
                written += 1;
            }
        }
        Ok(written)
    }

    /// 读取全部已到达设备的 AI 请求/响应事件，按发生时间升序。
    pub fn load_all_events(&self) -> Result<Vec<AiUsageEvent>> {
        let mut events = Vec::new();
        let mut seen = HashSet::new();
        for origin in self.list_origins()? {
            let dir = self.events_root().join(&origin);
            let mut paths: Vec<PathBuf> = fs::read_dir(&dir)?
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.ends_with(AI_USAGE_SUFFIX))
                })
                .collect();
            paths.sort();
            for path in paths {
                let file = fs::File::open(&path)?;
                for line in BufReader::new(file).lines() {
                    let line = line?;
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    let mut event: AiUsageEvent = match serde_json::from_str(line) {
                        Ok(event) => event,
                        Err(error) => {
                            eprintln!(
                                "[writing_stats] skip malformed AI usage event in {}: {error}",
                                path.display()
                            );
                            continue;
                        }
                    };
                    if event.origin_device_id.is_empty() {
                        event.origin_device_id = origin.clone();
                    }
                    if !event.event_id.is_empty() && !seen.insert(event.event_id.clone()) {
                        continue;
                    }
                    events.push(event);
                }
            }
        }
        events.sort_by(|left, right| {
            left.occurred_at_ms
                .cmp(&right.occurred_at_ms)
                .then_with(|| left.event_id.cmp(&right.event_id))
        });
        Ok(events)
    }

    fn list_origins(&self) -> Result<Vec<String>> {
        let root = self.events_root();
        if !root.is_dir() {
            return Ok(Vec::new());
        }
        let mut origins = Vec::new();
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if valid_segment(&name) {
                origins.push(name);
            }
        }
        origins.sort();
        Ok(origins)
    }
}

fn read_ids(path: &Path) -> Result<HashSet<String>> {
    let mut ids = HashSet::new();
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(ids),
        Err(error) => return Err(error.into()),
    };
    for line in BufReader::new(file).lines() {
        let line = line?;
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
            if let Some(id) = value.get("event_id").and_then(|id| id.as_str()) {
                ids.insert(id.to_string());
            }
        }
    }
    Ok(ids)
}
