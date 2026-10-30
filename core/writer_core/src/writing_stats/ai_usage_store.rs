//! AI 请求输入 / 模型响应输出事件的按设备分区存储。
//!
//! 文件布局与写作输入事件一致：`stats/events/<device_id>/YYYY-MM-DD.ai.jsonl`。
//! 每台设备只追加自己的分区；读取时扫描全部已到达分区并按 `event_id` 去重。
//! 同步只搬运文件，不重写发生时间、当地日期或小时。
//!
//! 持久化流程与对象贡献事件一致：先写 `stats/pending/ai-usage/` 下的 journal，
//! 成功追加正式分区后才删除 journal；读取前先恢复未完成的 journal。写入、恢复和
//! 读取共用工作区跨进程写锁，再加实例内互斥，稳定 `event_id` 保证重放不会重复
//! 计数。`local_date` 在拼入文件名前会严格校验为 `YYYY-MM-DD`，避免外部会话
//! 适配层传入非法日期。
//!
//! 与对象贡献事件分开存放是有意的：一次 AI 请求及模型响应可能对应多个对象
//! 修改，不能把输入/输出字数随 `MutationContext` 复制到每条对象事件上。

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use chrono::NaiveDate;

use crate::error::{Error, Result};
use crate::storage::atomic_write_string;
use crate::storage::WorkspaceWriteLock;
use crate::writing_stats::ai_usage::AiUsageEvent;
use crate::writing_stats::calendar;
use crate::writing_stats::contribution_store::{resolve_device_partition_id, valid_segment};

const EVENTS_ROOT: &str = "stats/events";
const PENDING_ROOT: &str = "stats/pending/ai-usage";
const AI_USAGE_SUFFIX: &str = ".ai.jsonl";

/// AI 请求输入 / 模型响应输出事件存储。
pub struct AiUsageStore {
    app_data_root: PathBuf,
    origin_device_id: Mutex<Option<String>>,
    /// 串行化 pending journal、正式分区追加和恢复；读取也先拿这把锁再恢复。
    write_lock: Mutex<()>,
}

impl AiUsageStore {
    pub fn new(app_data_root: &Path) -> Self {
        Self {
            app_data_root: app_data_root.to_path_buf(),
            origin_device_id: Mutex::new(None),
            write_lock: Mutex::new(()),
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

    /// 幂等追加一批 AI 方向事件；返回真正新写入的事件数。
    ///
    /// 事件先以 journal 落盘，再追加到正式分区。追加失败时 journal 保留，
    /// 下次记录/读取时重放；稳定 `event_id` 保证重放不会重复。
    pub fn record_events(&self, events: Vec<AiUsageEvent>) -> Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }
        // 锁序与业务层一致：工作区跨进程锁 → 本实例写锁 → 磁盘。
        let _workspace_write_lock = WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let _guard = self.lock_write();
        self.recover_pending_events_locked()?;

        let origin = self.resolve_origin_device_id()?;
        let prepared = self.prepare_events(events, Some(origin.as_str()))?;
        let pending_path = self.pending_file_path();
        self.write_pending(&pending_path, &prepared)?;

        let written = self.append_events(&prepared)?;
        remove_pending_file(&pending_path)?;
        Ok(written)
    }

    /// 重放上次进程在 journal 已落盘、正式分区尚未确认前中断的事件。
    pub fn recover_pending_events(&self) -> Result<()> {
        let _workspace_write_lock = WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let _guard = self.lock_write();
        self.recover_pending_events_locked()
    }

    fn recover_pending_events_locked(&self) -> Result<()> {
        let pending_dir = self.app_data_root.join(PENDING_ROOT);
        if !pending_dir.is_dir() {
            return Ok(());
        }
        let mut paths: Vec<PathBuf> = fs::read_dir(&pending_dir)?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .collect();
        paths.sort();
        for path in paths {
            let content = match fs::read_to_string(&path) {
                Ok(content) => content,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            let events: Vec<AiUsageEvent> = serde_json::from_str(&content)?;
            if !events.is_empty() {
                let prepared = self.prepare_events(events, None)?;
                self.append_events(&prepared)?;
            }
            remove_pending_file(&path)?;
        }
        Ok(())
    }

    fn pending_file_path(&self) -> PathBuf {
        self.app_data_root
            .join(PENDING_ROOT)
            .join(format!("{}.json", uuid::Uuid::new_v4()))
    }

    fn write_pending(&self, path: &Path, events: &[AiUsageEvent]) -> Result<()> {
        atomic_write_string(path, &serde_json::to_string(events)?)
    }

    /// 补齐本机来源、业务日和小时，并在任何路径拼接前校验数据。
    fn prepare_events(
        &self,
        events: Vec<AiUsageEvent>,
        fallback_origin: Option<&str>,
    ) -> Result<Vec<AiUsageEvent>> {
        let mut prepared = Vec::with_capacity(events.len());
        for mut event in events {
            if event.event_id.trim().is_empty() {
                return Err(Error::Other(
                    "AI usage event_id must not be empty".to_string(),
                ));
            }
            if event.origin_device_id.is_empty() {
                event.origin_device_id = match fallback_origin {
                    Some(origin) => origin.to_string(),
                    None => self.resolve_origin_device_id()?,
                };
            }
            if !valid_segment(&event.origin_device_id) {
                return Err(Error::Other(format!(
                    "invalid device id for AI usage event partition: {:?}",
                    event.origin_device_id
                )));
            }
            if event.local_date.is_empty() {
                event.local_date = calendar::local_date_at(event.occurred_at_ms)
                    .or_else(|_| calendar::utc_date_at(event.occurred_at_ms))
                    .map_err(|_| {
                        Error::Other(format!(
                            "invalid timestamp for AI usage event: {}",
                            event.occurred_at_ms
                        ))
                    })?;
            }
            validate_local_date(&event.local_date)?;
            if event.local_hour.is_none() {
                event.local_hour = calendar::local_hour_at(event.occurred_at_ms);
            }
            if event.local_hour.is_some_and(|hour| hour >= 24) {
                return Err(Error::Other(format!(
                    "invalid local hour for AI usage event: {:?}",
                    event.local_hour
                )));
            }
            prepared.push(event);
        }
        Ok(prepared)
    }

    fn append_events(&self, events: &[AiUsageEvent]) -> Result<usize> {
        let mut by_origin_date: BTreeMap<(String, String), Vec<&AiUsageEvent>> = BTreeMap::new();
        for event in events {
            if !valid_segment(&event.origin_device_id) {
                return Err(Error::Other(format!(
                    "invalid device id for AI usage event partition: {:?}",
                    event.origin_device_id
                )));
            }
            validate_local_date(&event.local_date)?;
            by_origin_date
                .entry((event.origin_device_id.clone(), event.local_date.clone()))
                .or_default()
                .push(event);
        }

        let mut written = 0usize;
        for ((origin, date), events) in by_origin_date {
            let path = self.event_file_path(&origin, &date);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut existing = read_ids(&path)?;
            let mut buffer = String::new();
            for event in events {
                if existing.contains(&event.event_id) {
                    continue;
                }
                buffer.push_str(&serde_json::to_string(event)?);
                buffer.push('\n');
                existing.insert(event.event_id.clone());
                written += 1;
            }
            if buffer.is_empty() {
                continue;
            }
            append_buffer(&path, buffer.as_bytes())?;
        }
        Ok(written)
    }

    /// 读取全部已到达设备的 AI 方向事件，按发生时间升序。
    ///
    /// 正式分区在读取前先恢复 journal；没有完成追加的事件不会永久消失。
    pub fn load_all_events(&self) -> Result<Vec<AiUsageEvent>> {
        let _workspace_write_lock = WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let _guard = self.lock_write();
        self.recover_pending_events_locked()?;

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
                    if !event.local_date.is_empty()
                        && validate_local_date(&event.local_date).is_err()
                    {
                        eprintln!(
                            "[writing_stats] skip AI usage event with invalid local date in {}: {:?}",
                            path.display(),
                            event.local_date
                        );
                        continue;
                    }
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

    fn lock_write(&self) -> MutexGuard<'_, ()> {
        self.write_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// journal/正式分区路径都只接受严格的 `YYYY-MM-DD`。
fn validate_local_date(date: &str) -> Result<()> {
    let parsed = NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|error| Error::Other(format!("invalid AI usage local date {date:?}: {error}")))?;
    if parsed.format("%Y-%m-%d").to_string() != date {
        return Err(Error::Other(format!(
            "invalid AI usage local date {date:?}: expected YYYY-MM-DD"
        )));
    }
    Ok(())
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

fn remove_pending_file(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// 一次性追加并保证半行 JSON 不会吞掉后续恢复写入的事件。
fn append_buffer(path: &Path, buffer: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)?;
    if file.metadata()?.len() > 0 {
        file.seek(SeekFrom::End(-1))?;
        let mut last = [0u8; 1];
        file.read_exact(&mut last)?;
        if last[0] != b'\n' {
            file.write_all(b"\n")?;
        }
    }
    file.write_all(buffer)?;
    Ok(())
}
