//! # 写作统计数据存储模块
//!
//! 本模块只负责事件存储和按日期范围读取事件。所有统计计算（聚合、速度曲线等）
//! 已移至 [`crate::writing_stats::projection`] 模块。
//!
//! ## 存储结构
//!
//! ```text
//! app-meta/stats/
//!   events.local/
//!     2024-01-01.events.jsonl    # 原始事件（JSONL 格式）
//! ```
//!
//! 事件按 UTC 日期分文件存储，业务日历日（每日统计分桶、「今天」查询）走
//! [`crate::writing_stats::calendar`] 模块。

use crate::error::Result;
use crate::writing_stats::WritingInputEvent;
use chrono::NaiveDate;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// 写作统计数据存储引擎 — 只负责事件存储和按日期范围读取事件。
///
/// 不再持有内存缓冲区：`record_event` 直接追加写入 JSONL。
/// 不再管理 daily stats：所有统计计算由 projection 模块完成。
///
/// 写入按 `event_id` 幂等：同一事件（同 ID）重复落盘只写一行，
/// 平台端重发未确认事件不会翻倍字数。
pub struct StatsStore {
    app_data_root: PathBuf,
    /// 每个 UTC 日期文件里已持久化的事件 ID（惰性扫描、写入时维护）。
    ///
    /// 只用于写入判重；查询仍以文件内容为准。`rewrite_events_for_date`
    /// 会清掉对应日期，避免整体改写后缓存过期。
    written_ids: Mutex<HashMap<String, HashSet<String>>>,
}

impl StatsStore {
    pub fn new(app_data_root: &Path) -> Self {
        Self {
            app_data_root: app_data_root.to_path_buf(),
            written_ids: Mutex::new(HashMap::new()),
        }
    }

    fn events_dir(&self) -> PathBuf {
        self.app_data_root.join("app-meta/stats/events.local")
    }

    /// 列出 `events.local/` 下所有事件文件的日期部分（升序）。
    pub fn list_event_file_dates(&self) -> Result<Vec<String>> {
        let dir = self.events_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut dates = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(date) = name.strip_suffix(".events.jsonl") else {
                continue;
            };
            if NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok() {
                dates.push(date.to_string());
            }
        }
        dates.sort();
        Ok(dates)
    }

    /// 用给定内容整体覆盖某个日期的事件文件（先写 tmp 再 rename）。
    pub fn rewrite_events_for_date(&self, date: &str, contents: &str) -> Result<()> {
        let file_path = self.events_dir().join(format!("{}.events.jsonl", date));
        fs::create_dir_all(self.events_dir())?;
        let tmp_path = file_path.with_extension("jsonl.tmp");
        fs::write(&tmp_path, contents)?;
        fs::rename(&tmp_path, &file_path)?;
        // 整体改写后缓存的 ID 集合作废，下次写入重新扫描。
        self.written_ids
            .lock()
            .map_err(|_| crate::Error::Other("stats event id cache poisoned".to_string()))?
            .remove(date);
        Ok(())
    }

    /// 记录一个写作输入事件 — 直接追加写入 JSONL，无缓冲。
    ///
    /// 一次编辑事务就是追加一行 JSONL。不再有内存缓冲或防抖逻辑。
    /// 同一 `event_id` 重复调用不重复落盘（幂等）。
    pub fn record_event(&self, event: WritingInputEvent) -> Result<()> {
        self.record_events(vec![event]).map(|_| ())
    }

    /// 幂等批量记录事件，返回真正新落盘的事件数。
    ///
    /// 平台端排队上报的事件可能重发（保存失败、进程重启恢复），
    /// 判重按稳定 `event_id`：文件里已有同 ID 的行就跳过。
    /// 事件按**事件发生时间**的 UTC 日期分文件（与 [`Self::timestamp_to_date`] 一致），
    /// 重发旧事件仍落在它原本的日期文件里，不会篡改归属日。
    pub fn record_events(&self, events: Vec<WritingInputEvent>) -> Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }

        // 先按存储分区（UTC 日）聚拢，写入顺序与调用顺序无关。
        let mut by_date: BTreeMap<String, Vec<WritingInputEvent>> = BTreeMap::new();
        for event in events {
            let date = self.timestamp_to_date(event.timestamp_ms)?;
            by_date.entry(date).or_default().push(event);
        }

        let mut guard = self
            .written_ids
            .lock()
            .map_err(|_| crate::Error::Other("stats event id cache poisoned".to_string()))?;
        let mut written = 0usize;
        for (date, day_events) in by_date {
            // 惰性加载该日期文件的已有 ID；坏行跳过（严格报错留给读取路径），
            // 一个历史坏行不该阻塞新事件的写入。
            if !guard.contains_key(&date) {
                let ids = self.scan_event_ids(&date)?;
                guard.insert(date.clone(), ids);
            }
            let ids = guard
                .get_mut(&date)
                .expect("stats event id cache must contain the date");

            let fresh: Vec<WritingInputEvent> = day_events
                .into_iter()
                .filter(|event| event.event_id.is_empty() || !ids.contains(&event.event_id))
                .collect();
            if fresh.is_empty() {
                continue;
            }

            let events_dir = self.events_dir();
            fs::create_dir_all(&events_dir)?;
            let file_path = events_dir.join(format!("{}.events.jsonl", date));
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&file_path)?;
            for event in &fresh {
                writeln!(file, "{}", serde_json::to_string(event)?)?;
                if !event.event_id.is_empty() {
                    ids.insert(event.event_id.clone());
                }
                written += 1;
            }
        }
        Ok(written)
    }

    /// 扫描某个日期文件里已存在的事件 ID（用于写入判重缓存）。
    ///
    /// 只提取 `event_id` 字段，解析不了的行直接跳过：这是判重的尽力缓存，
    /// 与 [`Self::load_events_for_date`] 的「坏行严格报错」职责不同。
    fn scan_event_ids(&self, date: &str) -> Result<HashSet<String>> {
        let file_path = self.events_dir().join(format!("{}.events.jsonl", date));
        if !file_path.exists() {
            return Ok(HashSet::new());
        }
        let file = File::open(&file_path)?;
        let reader = BufReader::new(file);
        let mut ids = HashSet::new();
        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if let Some(id) = value.get("event_id").and_then(|v| v.as_str()) {
                    if !id.is_empty() {
                        ids.insert(id.to_string());
                    }
                }
            }
        }
        Ok(ids)
    }

    /// 读取某个 UTC 分区的事件文件，**任何一行解析失败都返回 Err**。
    ///
    /// raw event 是统计的唯一事实源，宽松跳过坏行会让迁移回写时永久删掉
    /// 解析不了的原始数据，也会让查询返回不完整结果而不报错。严格读取
    /// 带文件名 + 行号报错，调用方可以据此中止或降级。
    pub fn load_events_for_date(&self, date: &str) -> Result<Vec<WritingInputEvent>> {
        let file_path = self.events_dir().join(format!("{}.events.jsonl", date));
        if !file_path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&file_path)?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();

        for (idx, line) in reader.lines().enumerate() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let event: WritingInputEvent = serde_json::from_str(trimmed).map_err(|e| {
                crate::Error::Other(format!(
                    "Corrupt event at {}.events.jsonl:{}: {}",
                    date,
                    idx + 1,
                    e
                ))
            })?;
            events.push(event);
        }

        Ok(events)
    }

    /// 读取本地事件历史上的**全部**事件（遍历真实存在的日期文件，升序）。
    ///
    /// 供统计页 Explorer 这类「一次拿到全量历史再在内存里切区间」的查询使用：
    /// 只遍历 [`StatsStore::list_event_file_dates`] 里真实存在的日期文件，
    /// 绝不按「某天到今天」逐日枚举空文件。任一分区文件损坏仍返回明确错误，
    /// 不把错误当「0 字」吞掉。
    pub fn load_all_events(&self) -> Result<Vec<WritingInputEvent>> {
        let mut events = Vec::new();
        for date in self.list_event_file_dates()? {
            let mut day_events = self.load_events_for_date(&date)?;
            events.append(&mut day_events);
        }
        Ok(events)
    }

    /// 读取指定作品的全部本地事件（跨所有已有事件文件的 UTC 分区）。
    ///
    /// 只遍历 [`StatsStore::list_event_file_dates`] 里真实存在的日期文件，
    /// 不按「作品创建日到今天」逐日枚举空文件。任一分区文件损坏仍返回
    /// 明确错误，不把错误当「0 字」吞掉。
    pub fn load_events_for_project(&self, project_id: &str) -> Result<Vec<WritingInputEvent>> {
        let mut events = self.load_all_events()?;
        events.retain(|e| e.project_id == project_id);
        Ok(events)
    }

    /// 读取 UTC 存储分区落在 `[start_date, end_date]` ± 1 天内的事件，
    /// 再按事件的 [`WritingInputEvent::business_date`] 过滤到真正的业务日范围。
    ///
    /// 事件文件按 UTC 日期分目录（见 [`StatsStore::timestamp_to_date`]），
    /// 业务日历日按本地午夜算。查询本地日范围时，真实事件可能住在
    /// 相邻的 UTC 日期文件里（东八区本地 10/6 00:30 的事件在 UTC 10/5
    /// 文件）。读 ±1 天后按 `business_date` 精确过滤，避免既漏读又
    /// 多读；`load_events_in_window` 走纯时间戳路径，不受此影响。
    ///
    /// 调用方传入的 `start_date`/`end_date` 视为业务日历日（本地日）。
    pub fn load_events_range(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<WritingInputEvent>> {
        let start = NaiveDate::parse_from_str(start_date, "%Y-%m-%d")
            .map_err(|e| crate::Error::Other(format!("Invalid start date: {}", e)))?;
        let end = NaiveDate::parse_from_str(end_date, "%Y-%m-%d")
            .map_err(|e| crate::Error::Other(format!("Invalid end date: {}", e)))?;

        // 业务日 [start, end] 最多跨 UTC 日期 [start-1, end+1]。
        let utc_start = start - chrono::Duration::days(1);
        let utc_end = end + chrono::Duration::days(1);
        let mut all_events = Vec::new();
        let mut current = utc_start;
        while current <= utc_end {
            let date_str = current.format("%Y-%m-%d").to_string();
            let mut events = self.load_events_for_date(&date_str)?;
            all_events.append(&mut events);
            current += chrono::Duration::days(1);
        }

        let start_owned = start.to_string();
        let end_owned = end.to_string();
        all_events.retain(|e| {
            let d = e.business_date();
            !d.is_empty() && d.as_str() >= start_owned.as_str() && d.as_str() <= end_owned.as_str()
        });

        Ok(all_events)
    }

    /// 读取 `[start_ms, end_ms]` 窗口内的事件。
    ///
    /// 日期口径用业务日历日（本地午夜），不是 UTC：东八区本地 10/6 00:30
    /// 的输入若按 UTC 日期 10/5 查窗口，会被错误排除。
    ///
    /// 不再合并内存缓冲（缓冲已删除），只读磁盘事件。
    pub fn load_events_in_window(
        &self,
        start_ms: i64,
        end_ms: i64,
    ) -> Result<Vec<WritingInputEvent>> {
        let start_date = crate::writing_stats::calendar::local_date_at(start_ms)?;
        let end_date = crate::writing_stats::calendar::local_date_at(end_ms)?;

        let mut events = self.load_events_range(&start_date, &end_date)?;
        events.retain(|e| e.timestamp_ms >= start_ms && e.timestamp_ms <= end_ms);

        events.sort_by_key(|e| e.timestamp_ms);
        Ok(events)
    }

    /// 事件**存储分区**用的 UTC 日期（`events.local/YYYY-MM-DD.events.jsonl`）。
    ///
    /// 刻意保留 UTC：改分文件规则要搬历史原始文件，收益不抵风险。
    /// 业务日历日（每日统计分桶、「今天」查询）走
    /// [`crate::writing_stats::calendar`]，两者分工见该模块文档。
    pub fn timestamp_to_date(&self, timestamp_ms: i64) -> Result<String> {
        crate::writing_stats::calendar::utc_date_at(timestamp_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writing_stats::{EventSource, Platform, WritingInputEvent};

    fn create_mock_store() -> (StatsStore, tempfile::TempDir) {
        let tmp = tempfile::tempdir().expect("failed to create temp dir for mock store");
        (StatsStore::new(tmp.path()), tmp)
    }

    fn create_mock_event() -> WritingInputEvent {
        WritingInputEvent::new(
            "device_1",
            Platform::Desktop,
            "desktop",
            "proj_1",
            "vol_1",
            "ch_1",
            EventSource::HumanTyped,
            10,
            0,
            0,
            0,
            0,
            "session_1",
        )
    }

    #[test]
    fn test_record_event_direct_append() {
        let (store, _tmp) = create_mock_store();
        let event = create_mock_event();
        store.record_event(event.clone()).unwrap();

        // 直接落盘，不需要 flush
        let date = store.timestamp_to_date(event.timestamp_ms).unwrap();
        let events = store.load_events_for_date(&date).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].inserted_chars, 10);
    }

    #[test]
    fn test_record_event_multiple() {
        let (store, _tmp) = create_mock_store();
        for i in 0..5 {
            let mut event = create_mock_event();
            event.inserted_chars = i + 1;
            store.record_event(event).unwrap();
        }

        let date = store
            .timestamp_to_date(chrono::Utc::now().timestamp_millis())
            .unwrap();
        let events = store.load_events_for_date(&date).unwrap();
        assert_eq!(events.len(), 5);
    }

    #[test]
    fn test_load_events_for_date_corrupt_line() {
        let (store, _tmp) = create_mock_store();
        let event = create_mock_event();
        store.record_event(event).unwrap();

        // 往文件追加一行坏数据
        let date = store
            .timestamp_to_date(chrono::Utc::now().timestamp_millis())
            .unwrap();
        let file_path = store.events_dir().join(format!("{}.events.jsonl", date));
        let mut file = OpenOptions::new().append(true).open(&file_path).unwrap();
        writeln!(file, "{{\"bad\":}}").unwrap();

        // 严格 loader 遇到坏行应返回 Err（带文件名 + 行号）
        let result = store.load_events_for_date(&date);
        assert!(result.is_err());
    }

    #[test]
    fn test_list_event_file_dates() {
        let (store, _tmp) = create_mock_store();
        let event = create_mock_event();
        store.record_event(event).unwrap();

        let dates = store.list_event_file_dates().unwrap();
        assert_eq!(dates.len(), 1);
    }

    #[test]
    fn test_rewrite_events_for_date() {
        let (store, _tmp) = create_mock_store();
        let event = create_mock_event();
        store.record_event(event).unwrap();

        let date = store
            .timestamp_to_date(chrono::Utc::now().timestamp_millis())
            .unwrap();
        let new_content = serde_json::to_string(&create_mock_event()).unwrap();
        store.rewrite_events_for_date(&date, &new_content).unwrap();

        let events = store.load_events_for_date(&date).unwrap();
        assert_eq!(events.len(), 1);
    }

    // ── 幂等写入（#936）──

    #[test]
    fn test_record_events_dedup_by_event_id() {
        let (store, _tmp) = create_mock_store();
        let mut event = create_mock_event();
        event.event_id = "fixed-event-id".to_string();

        // 同一事件重发三次，只落一条。
        store.record_event(event.clone()).unwrap();
        store.record_event(event.clone()).unwrap();
        store.record_event(event.clone()).unwrap();

        let date = store.timestamp_to_date(event.timestamp_ms).unwrap();
        let events = store.load_events_for_date(&date).unwrap();
        assert_eq!(events.len(), 1);

        // 重新构造 store（缓存冷启动）再重发，仍不重复写。
        let store2 = StatsStore::new(_tmp.path());
        store2.record_event(event.clone()).unwrap();
        let events = store2.load_events_for_date(&date).unwrap();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn test_record_events_resend_keeps_original_storage_date() {
        let (store, _tmp) = create_mock_store();
        // 事件发生在昨天（UTC），重发也落在昨天的文件里。
        let occurred_at_ms = (chrono::Utc::now() - chrono::Duration::days(1)).timestamp_millis();
        let event = WritingInputEvent::with_identity(
            "old-event-id".to_string(),
            occurred_at_ms,
            "device_1",
            Platform::Desktop,
            "desktop",
            "proj_1",
            "vol_1",
            "ch_1",
            EventSource::HumanTyped,
            10,
            0,
            0,
            0,
            0,
            "",
        );
        let date = store.timestamp_to_date(occurred_at_ms).unwrap();
        store.record_event(event.clone()).unwrap();
        store.record_event(event).unwrap();

        let events = store.load_events_for_date(&date).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].timestamp_ms, occurred_at_ms);
    }

    #[test]
    fn test_record_events_counts_only_new_writes() {
        let (store, _tmp) = create_mock_store();
        let mut first = create_mock_event();
        first.event_id = "e1".to_string();
        let second = create_mock_event();

        assert_eq!(store.record_events(vec![first.clone()]).unwrap(), 1);
        // 一条重复 + 一条新的：只新写 1 条。
        assert_eq!(store.record_events(vec![first, second]).unwrap(), 1);
    }

    #[test]
    fn test_record_events_tolerates_corrupt_history_lines() {
        let (store, _tmp) = create_mock_store();
        let date = store
            .timestamp_to_date(chrono::Utc::now().timestamp_millis())
            .unwrap();
        let file_path = store.events_dir().join(format!("{}.events.jsonl", date));
        std::fs::create_dir_all(store.events_dir()).unwrap();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .unwrap();
        writeln!(file, "{{\"bad\":}}").unwrap();
        drop(file);

        // 历史坏行不让新事件写入失败（严格报错留给读取路径）。
        let event = create_mock_event();
        store.record_event(event).unwrap();
        assert!(store.load_events_for_date(&date).is_err());
    }

    #[test]
    fn test_rewrite_events_for_date_invalidates_id_cache() {
        let (store, _tmp) = create_mock_store();
        let mut event = create_mock_event();
        event.event_id = "rewritten-id".to_string();
        store.record_event(event.clone()).unwrap();

        let date = store.timestamp_to_date(event.timestamp_ms).unwrap();
        // 整体改写把这条事件清掉后，同 ID 再写应重新落盘。
        store.rewrite_events_for_date(&date, "").unwrap();
        store.record_event(event.clone()).unwrap();

        let events = store.load_events_for_date(&date).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_id, "rewritten-id");
    }
}
