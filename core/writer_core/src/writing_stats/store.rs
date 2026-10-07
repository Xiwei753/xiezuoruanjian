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
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

/// 写作统计数据存储引擎 — 只负责事件存储和按日期范围读取事件。
///
/// 不再持有内存缓冲区：`record_event` 直接追加写入 JSONL。
/// 不再管理 daily stats：所有统计计算由 projection 模块完成。
pub struct StatsStore {
    app_data_root: PathBuf,
}

impl StatsStore {
    pub fn new(app_data_root: &Path) -> Self {
        Self {
            app_data_root: app_data_root.to_path_buf(),
        }
    }

    fn events_dir(&self) -> PathBuf {
        self.app_data_root.join("app-meta/stats/events.local")
    }

    /// 严格读取某个 UTC 分区的事件文件，**任何一行解析失败都返回 Err**。
    ///
    /// 与 [`StatsStore::load_events_for_date`] 的区别：后者为了查询容错会
    /// 静默跳过坏行（`if let Ok(...)`），这对「只读」是对的——统计少算
    /// 一条历史比崩掉编辑器好。但**迁移不能用它**：迁移要把读出来的事件
    /// 重新序列化后整文件回写，走宽松 loader 会把解析不了的原始行永久删掉。
    ///
    /// 严格版带文件名 + 行号报错，迁移据此中止并保留旧 `daily/`。
    pub fn load_events_for_date_strict(&self, date: &str) -> Result<Vec<WritingInputEvent>> {
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
        Ok(())
    }

    /// 记录一个写作输入事件 — 直接追加写入 JSONL，无缓冲。
    ///
    /// 一次编辑事务就是追加一行 JSONL。不再有内存缓冲或防抖逻辑。
    pub fn record_event(&self, event: WritingInputEvent) -> Result<()> {
        let date = self.timestamp_to_date(event.timestamp_ms)?;
        let events_dir = self.events_dir();
        fs::create_dir_all(&events_dir)?;
        let file_path = events_dir.join(format!("{}.events.jsonl", date));
        let json = serde_json::to_string(&event)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)?;
        writeln!(file, "{}", json)?;
        Ok(())
    }

    pub fn load_events_for_date(&self, date: &str) -> Result<Vec<WritingInputEvent>> {
        let file_path = self.events_dir().join(format!("{}.events.jsonl", date));
        if !file_path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&file_path)?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();

        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(event) = serde_json::from_str::<WritingInputEvent>(trimmed) {
                events.push(event);
            }
        }

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
    fn test_load_events_for_date_strict_corrupt_line() {
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

        let result = store.load_events_for_date_strict(&date);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_events_for_date_skips_bad_lines() {
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

        // 宽松 loader 应跳过坏行，返回 1 条好事件
        let events = store.load_events_for_date(&date).unwrap();
        assert_eq!(events.len(), 1);
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
}
