//! 对象贡献事件的按设备分区存储。
//!
//! 文件布局与章节输入事件一致：`stats/events/<device_id>/YYYY-MM-DD.objects.jsonl`。
//! 每台设备只追加自己的分区；读取时扫描全部已到达分区并按 `event_id` 去重。
//! 同步只搬运文件，不重写 `actor`/`entrypoint`。

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::error::{Error, Result};
use crate::storage::atomic_write_string;
use crate::writing_stats::contribution::{
    ContributionEvent, ContributionObjectType, ContributionOperation, MutationActor,
};

const EVENTS_ROOT: &str = "stats/events";
const PENDING_ROOT: &str = "stats/pending/contributions";
const OBJECTS_SUFFIX: &str = ".objects.jsonl";

pub struct ContributionStore {
    app_data_root: PathBuf,
    origin_device_id: Mutex<Option<String>>,
}

impl ContributionStore {
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
            .join(format!("{date}{OBJECTS_SUFFIX}"))
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

    /// 幂等追加一批对象事件；返回真正新写入的事件数。
    ///
    /// 事件先以事务为粒度落盘到 pending journal，再追加到正式分区。追加失败时
    /// journal 保留，下次记录/读取时重放；稳定的 `event_id` 保证重放不会重复。
    pub fn record_events(&self, events: Vec<ContributionEvent>) -> Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }
        let origin = self.resolve_origin_device_id()?;
        let mut prepared = Vec::with_capacity(events.len());
        for mut event in events {
            if event.local_date.is_empty() {
                event.local_date =
                    crate::writing_stats::calendar::local_date_at(event.occurred_at_ms)
                        .or_else(|_| {
                            crate::writing_stats::calendar::utc_date_at(event.occurred_at_ms)
                        })
                        .unwrap_or_default();
            }
            if event.origin_device_id.is_empty() {
                event.origin_device_id = origin.clone();
            }
            prepared.push(event);
        }

        self.recover_pending_events()?;
        let transaction_id = prepared
            .first()
            .map(|event| event.transaction_id.clone())
            .unwrap_or_default();
        let pending_path = self.pending_file_path(&transaction_id, prepared.len());
        self.write_pending(&pending_path, &prepared)?;

        let mut by_date: BTreeMap<String, Vec<ContributionEvent>> = BTreeMap::new();
        for event in prepared {
            let date = if event.local_date.is_empty() {
                crate::writing_stats::calendar::local_date_at(event.occurred_at_ms)
                    .or_else(|_| crate::writing_stats::calendar::utc_date_at(event.occurred_at_ms))
                    .unwrap_or_else(|_| "1970-01-01".to_string())
            } else {
                event.local_date.clone()
            };
            by_date.entry(date).or_default().push(event);
        }

        let partition_dir = self.events_root().join(&origin);
        fs::create_dir_all(&partition_dir)?;
        let mut written = 0usize;
        for (date, events) in by_date {
            let path = self.event_file_path(&origin, &date);
            let mut existing = read_ids(&path)?;
            let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
            for event in events {
                if !event.event_id.is_empty() && existing.contains(&event.event_id) {
                    continue;
                }
                let line = serde_json::to_string(&event)?;
                writeln!(file, "{line}")?;
                if !event.event_id.is_empty() {
                    existing.insert(event.event_id.clone());
                }
                written += 1;
            }
        }
        fs::remove_file(&pending_path)?;
        Ok(written)
    }

    /// 重放上次进程在「业务已成功、事件已进入 journal」之后未能追加的事件。
    pub fn recover_pending_events(&self) -> Result<()> {
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
            let content = fs::read_to_string(&path)?;
            let events: Vec<ContributionEvent> = serde_json::from_str(&content)?;
            if !events.is_empty() {
                self.append_events(&events)?;
            }
            fs::remove_file(&path)?;
        }
        Ok(())
    }

    fn pending_file_path(&self, transaction_id: &str, count: usize) -> PathBuf {
        let safe: String = transaction_id
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                    ch
                } else {
                    '_'
                }
            })
            .collect();
        let safe = if safe.is_empty() {
            "unidentified".to_string()
        } else {
            safe
        };
        self.app_data_root
            .join(PENDING_ROOT)
            .join(format!("{safe}-{count}.json"))
    }

    fn write_pending(&self, path: &Path, events: &[ContributionEvent]) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        atomic_write_string(path, &serde_json::to_string(events)?)
    }

    fn append_events(&self, events: &[ContributionEvent]) -> Result<()> {
        let mut by_origin_date: BTreeMap<(String, String), Vec<&ContributionEvent>> =
            BTreeMap::new();
        for event in events {
            let origin = if event.origin_device_id.is_empty() {
                self.resolve_origin_device_id()?
            } else {
                event.origin_device_id.clone()
            };
            let date = if event.local_date.is_empty() {
                crate::writing_stats::calendar::local_date_at(event.occurred_at_ms)
                    .or_else(|_| crate::writing_stats::calendar::utc_date_at(event.occurred_at_ms))
                    .unwrap_or_else(|_| "1970-01-01".to_string())
            } else {
                event.local_date.clone()
            };
            by_origin_date
                .entry((origin, date))
                .or_default()
                .push(event);
        }
        for ((origin, date), events) in by_origin_date {
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
                writeln!(file, "{}", serde_json::to_string(event)?)?;
                if !event.event_id.is_empty() {
                    existing.insert(event.event_id.clone());
                }
            }
        }
        Ok(())
    }

    pub fn load_all_events(&self) -> Result<Vec<ContributionEvent>> {
        self.recover_pending_events()?;
        let mut events = Vec::new();
        let mut seen = HashSet::new();
        for origin in self.list_origins()? {
            let dir = self.events_root().join(&origin);
            if !dir.is_dir() {
                continue;
            }
            let mut paths: Vec<PathBuf> = fs::read_dir(&dir)?
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.ends_with(OBJECTS_SUFFIX))
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
                    let mut event: ContributionEvent = match serde_json::from_str(line) {
                        Ok(event) => event,
                        Err(error) => {
                            eprintln!(
                                "[writing_stats] skip malformed contribution event in {}: {error}",
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

    pub fn load_events_for_project(&self, project_id: &str) -> Result<Vec<ContributionEvent>> {
        Ok(self
            .load_all_events()?
            .into_iter()
            .filter(|event| event.project_id == project_id)
            .collect())
    }

    pub fn load_events_range(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<ContributionEvent>> {
        Ok(self
            .load_all_events()?
            .into_iter()
            .filter(|event| {
                let date = if event.local_date.is_empty() {
                    crate::writing_stats::calendar::local_date_at(event.occurred_at_ms)
                        .unwrap_or_default()
                } else {
                    event.local_date.clone()
                };
                !date.is_empty() && date.as_str() >= start_date && date.as_str() <= end_date
            })
            .collect())
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

/// 解析本机统计事件分区使用的稳定设备 ID。
///
/// 对象贡献事件与 AI 请求/响应事件共用同一来源分区规则；设备信息缺失时
/// 按既有设置流程生成，已有但非法则明确报错，不写入无法同步的目录名。
pub(crate) fn resolve_device_partition_id(app_data_root: &Path) -> Result<String> {
    let info = match crate::settings::load_device_info(app_data_root) {
        Ok(info) if valid_segment(&info.device_id) => info,
        _ => crate::settings::ensure_device_info(app_data_root, "", "", None)?,
    };
    if !valid_segment(&info.device_id) {
        return Err(Error::Other(format!(
            "invalid device id for stats event partition: {:?}",
            info.device_id
        )));
    }
    Ok(info.device_id)
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

pub(crate) fn valid_segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

/// 供投影层使用的稳定字符串键。
pub fn actor_key(actor: MutationActor) -> &'static str {
    actor.as_str()
}

pub fn object_type_key(object_type: ContributionObjectType) -> &'static str {
    object_type.as_str()
}

pub fn operation_key(operation: ContributionOperation) -> &'static str {
    operation.as_str()
}
