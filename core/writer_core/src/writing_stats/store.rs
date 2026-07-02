//! # 写作统计数据存储模块
//!
//! 本模块负责写作统计数据的持久化存储，包括原始事件的记录和每日统计数据的管理。
//!
//! ## 主要功能
//!
//! - **事件缓冲**: 使用内存缓冲区批量写入事件，减少磁盘 I/O
//! - **事件持久化**: 将输入事件以 JSONL 格式按日期存储
//! - **每日统计**: 聚合并存储每日统计数据，支持多设备数据合并
//! - **多维度统计**: 支持按项目、卷、章节的细分统计
//! - **速度分析**: 计算写作速度曲线（字符/分钟）
//!
//! ## 核心结构
//!
//! - `StatsStore`: 统计数据存储引擎，管理事件缓冲和文件 I/O
//! - `DailyStats`: 每日统计数据，包含总字符数、活跃时间、会话数等
//! - `DailyStatsFile`: 每日统计文件，支持多设备数据
//! - `ProjectStats/VolumeStats/ChapterStats`: 分维度统计数据
//! - `SpeedBucket`: 速度桶，用于速度曲线分析
//!
//! ## 存储结构
//!
//! ```text
//! app-meta/stats/
//!   events.local/
//!     2024-01-01.events.jsonl    # 原始事件（JSONL 格式）
//!   daily/
//!     2024-01-01.stats.json      # 每日统计数据
//! ```
//!
//! ## 依赖关系
//!
//! - `chrono`: 日期时间处理
//! - `serde`: 序列化/反序列化
//! - `std::fs`: 文件系统操作
//! - `std::sync::Mutex`: 线程安全的缓冲区管理
//!
//! ## 使用场景
//!
//! - 实时记录用户输入事件
//! - 生成每日/每周/每月写作报告
//! - 分析写作速度和效率
//! - 多设备写作数据同步和合并

use crate::error::Result;
use crate::writing_stats::WritingInputEvent;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const SESSION_GAP_MS: i64 = 5 * 60 * 1000;
const FLUSH_DEBOUNCE_MS: i64 = 3000;
const MAX_BUFFER_SIZE: usize = 100;

pub struct StatsStore {
    workspace_path: PathBuf,
    event_buffer: Mutex<Vec<WritingInputEvent>>,
    last_flush_ms: Mutex<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DailyStats {
    pub date: String,
    pub device_id: String,
    pub platform: String,
    /// phone / tablet / desktop，用于按设备类别汇总
    #[serde(default)]
    pub device_class: String,
    pub total_human_typed_chars: u64,
    pub total_pasted_chars: u64,
    pub total_deleted_chars: u64,
    pub total_ai_inserted_chars: u64,
    pub total_net_delta_chars: i64,
    pub active_seconds: u64,
    pub sessions_count: u32,
    #[serde(default)]
    pub per_project: HashMap<String, ProjectStats>,
    #[serde(default)]
    pub per_volume: HashMap<String, VolumeStats>,
    #[serde(default)]
    pub per_chapter: HashMap<String, ChapterStats>,
    #[serde(default)]
    pub speed_buckets: Vec<SpeedBucket>,
}

impl DailyStats {
    pub fn apply_event(&mut self, event: &crate::writing_stats::WritingInputEvent) {
        match event.source {
            crate::writing_stats::EventSource::HumanTyped => {
                self.total_human_typed_chars += event.inserted_chars as u64;
            }
            crate::writing_stats::EventSource::Pasted => {
                self.total_pasted_chars += event.pasted_chars as u64;
            }
            crate::writing_stats::EventSource::Deleted => {
                self.total_deleted_chars += event.deleted_chars as u64;
            }
            crate::writing_stats::EventSource::AiInserted => {
                self.total_ai_inserted_chars += event.ai_inserted_chars as u64;
            }
            _ => {}
        }
        self.total_net_delta_chars += event.net_delta_chars as i64;

        let proj = self
            .per_project
            .entry(event.project_id.clone())
            .or_default();
        match event.source {
            crate::writing_stats::EventSource::HumanTyped => {
                proj.human_typed_chars += event.inserted_chars as u64
            }
            crate::writing_stats::EventSource::Pasted => {
                proj.pasted_chars += event.pasted_chars as u64
            }
            crate::writing_stats::EventSource::Deleted => {
                proj.deleted_chars += event.deleted_chars as u64
            }
            crate::writing_stats::EventSource::AiInserted => {
                proj.ai_inserted_chars += event.ai_inserted_chars as u64
            }
            _ => {}
        }
        proj.net_delta_chars += event.net_delta_chars as i64;

        let vol = self.per_volume.entry(event.volume_id.clone()).or_default();
        match event.source {
            crate::writing_stats::EventSource::HumanTyped => {
                vol.human_typed_chars += event.inserted_chars as u64
            }
            crate::writing_stats::EventSource::Pasted => {
                vol.pasted_chars += event.pasted_chars as u64
            }
            crate::writing_stats::EventSource::Deleted => {
                vol.deleted_chars += event.deleted_chars as u64
            }
            crate::writing_stats::EventSource::AiInserted => {
                vol.ai_inserted_chars += event.ai_inserted_chars as u64
            }
            _ => {}
        }
        vol.net_delta_chars += event.net_delta_chars as i64;

        let chap = self
            .per_chapter
            .entry(event.chapter_id.clone())
            .or_default();
        match event.source {
            crate::writing_stats::EventSource::HumanTyped => {
                chap.human_typed_chars += event.inserted_chars as u64
            }
            crate::writing_stats::EventSource::Pasted => {
                chap.pasted_chars += event.pasted_chars as u64
            }
            crate::writing_stats::EventSource::Deleted => {
                chap.deleted_chars += event.deleted_chars as u64
            }
            crate::writing_stats::EventSource::AiInserted => {
                chap.ai_inserted_chars += event.ai_inserted_chars as u64
            }
            _ => {}
        }
        chap.net_delta_chars += event.net_delta_chars as i64;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DailyStatsFile {
    pub date: String,
    pub devices: Vec<DailyStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectStats {
    pub human_typed_chars: u64,
    pub pasted_chars: u64,
    pub deleted_chars: u64,
    pub ai_inserted_chars: u64,
    pub net_delta_chars: i64,
    pub active_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VolumeStats {
    pub human_typed_chars: u64,
    pub pasted_chars: u64,
    pub deleted_chars: u64,
    pub ai_inserted_chars: u64,
    pub net_delta_chars: i64,
    pub active_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChapterStats {
    pub human_typed_chars: u64,
    pub pasted_chars: u64,
    pub deleted_chars: u64,
    pub ai_inserted_chars: u64,
    pub net_delta_chars: i64,
    pub active_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeedBucket {
    pub start_ms: i64,
    pub end_ms: i64,
    pub chars_typed: u32,
    pub chars_per_minute: f64,
}

impl StatsStore {
    pub fn new(workspace_path: &Path) -> Self {
        Self {
            workspace_path: workspace_path.to_path_buf(),
            event_buffer: Mutex::new(Vec::new()),
            last_flush_ms: Mutex::new(0),
        }
    }

    fn events_dir(&self) -> PathBuf {
        self.workspace_path.join("app-meta/stats/events.local")
    }

    fn daily_dir(&self) -> PathBuf {
        self.workspace_path.join("app-meta/stats/daily")
    }

    pub fn record_event(&self, event: WritingInputEvent) -> Result<()> {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let should_flush =
            {
                let mut buffer = self.event_buffer.lock().map_err(|e| {
                    crate::Error::Other(format!("Failed to lock event buffer: {}", e))
                })?;
                buffer.push(event);
                let mut last_flush = self.last_flush_ms.lock().map_err(|e| {
                    crate::Error::Other(format!("Failed to lock flush timer: {}", e))
                })?;
                let should =
                    buffer.len() >= MAX_BUFFER_SIZE || (now_ms - *last_flush) >= FLUSH_DEBOUNCE_MS;
                if should {
                    *last_flush = now_ms;
                }
                should
            };

        if should_flush {
            self.flush_events()?;
        }
        Ok(())
    }

    pub fn flush_events(&self) -> Result<()> {
        let events = {
            let mut buffer = self
                .event_buffer
                .lock()
                .map_err(|e| crate::Error::Other(format!("Failed to lock event buffer: {}", e)))?;
            std::mem::take(&mut *buffer)
        };

        if events.is_empty() {
            return Ok(());
        }

        let events_dir = self.events_dir();
        fs::create_dir_all(&events_dir)?;

        for event in &events {
            let date = self.timestamp_to_date(event.timestamp_ms)?;
            let file_path = events_dir.join(format!("{}.events.jsonl", date));
            let json = serde_json::to_string(event)?;
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&file_path)?;
            writeln!(file, "{}", json)?;
        }

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

    pub fn load_events_range(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<WritingInputEvent>> {
        let start = NaiveDate::parse_from_str(start_date, "%Y-%m-%d")
            .map_err(|e| crate::Error::Other(format!("Invalid start date: {}", e)))?;
        let end = NaiveDate::parse_from_str(end_date, "%Y-%m-%d")
            .map_err(|e| crate::Error::Other(format!("Invalid end date: {}", e)))?;

        let mut all_events = Vec::new();
        let mut current = start;
        while current <= end {
            let date_str = current.format("%Y-%m-%d").to_string();
            let mut events = self.load_events_for_date(&date_str)?;
            all_events.append(&mut events);
            current += chrono::Duration::days(1);
        }

        Ok(all_events)
    }

    pub fn save_daily_stats_file(&self, file: &DailyStatsFile) -> Result<()> {
        let daily_dir = self.daily_dir();
        fs::create_dir_all(&daily_dir)?;

        let file_path = daily_dir.join(format!("{}.stats.json", file.date));
        let json = serde_json::to_string_pretty(file)?;
        let tmp_path = file_path.with_extension("json.tmp");
        fs::write(&tmp_path, &json)?;
        fs::rename(&tmp_path, &file_path)?;

        Ok(())
    }

    pub fn load_daily_stats_file(&self, date: &str) -> Result<Option<DailyStatsFile>> {
        let file_path = self.daily_dir().join(format!("{}.stats.json", date));
        if !file_path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&file_path)?;
        let file: DailyStatsFile = serde_json::from_str(&content)?;
        Ok(Some(file))
    }

    pub fn save_or_merge_daily_stats(&self, stats: &DailyStats) -> Result<()> {
        let mut file = self
            .load_daily_stats_file(&stats.date)?
            .unwrap_or_else(|| DailyStatsFile {
                date: stats.date.clone(),
                devices: Vec::new(),
            });

        if let Some(existing) = file
            .devices
            .iter_mut()
            .find(|d| d.device_id == stats.device_id)
        {
            self.merge_daily_stats(existing, stats)?;
        } else {
            file.devices.push(stats.clone());
        }

        self.save_daily_stats_file(&file)
    }

    pub fn load_all_daily_stats_for_date(&self, date: &str) -> Result<Vec<DailyStats>> {
        match self.load_daily_stats_file(date)? {
            Some(file) => Ok(file.devices),
            None => Ok(Vec::new()),
        }
    }

    pub fn load_daily_stats_range(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<DailyStats>> {
        let start = NaiveDate::parse_from_str(start_date, "%Y-%m-%d")
            .map_err(|e| crate::Error::Other(format!("Invalid start date: {}", e)))?;
        let end = NaiveDate::parse_from_str(end_date, "%Y-%m-%d")
            .map_err(|e| crate::Error::Other(format!("Invalid end date: {}", e)))?;

        let mut all_stats = Vec::new();
        let mut current = start;
        while current <= end {
            let date_str = current.format("%Y-%m-%d").to_string();
            let mut stats = self.load_all_daily_stats_for_date(&date_str)?;
            all_stats.append(&mut stats);
            current += chrono::Duration::days(1);
        }

        Ok(all_stats)
    }

    pub fn aggregate_events(&self, events: &[WritingInputEvent]) -> Result<Vec<DailyStats>> {
        if events.is_empty() {
            return Ok(Vec::new());
        }

        let mut by_date_device: HashMap<(chrono::NaiveDate, &str), Vec<&WritingInputEvent>> =
            HashMap::new();
        for event in events {
            let dt = chrono::DateTime::from_timestamp_millis(event.timestamp_ms)
                .ok_or_else(|| crate::Error::Other("Invalid timestamp".to_string()))?;
            let date = dt.date_naive();
            let key = (date, event.device_id.as_str());
            by_date_device.entry(key).or_default().push(event);
        }

        let mut daily_stats_list = Vec::new();
        for ((date, device_id), day_events) in by_date_device {
            let platform = day_events
                .first()
                .map(|e| e.platform.to_string())
                .unwrap_or_default();
            let device_class = day_events
                .first()
                .map(|e| e.device_class.clone())
                .unwrap_or_default();
            let mut stats = DailyStats {
                date: date.format("%Y-%m-%d").to_string(),
                device_id: device_id.to_string(),
                platform,
                device_class,
                ..Default::default()
            };

            for event in &day_events {
                stats.apply_event(event);
            }

            let mut sorted_events = day_events.clone();
            sorted_events.sort_by_key(|e| e.timestamp_ms);

            let mut session_count: u32 = 0;
            let mut last_event_ms: i64 = 0;
            let mut active_ms: i64 = 0;

            for event in &sorted_events {
                if session_count == 0 || (event.timestamp_ms - last_event_ms) > SESSION_GAP_MS {
                    session_count += 1;
                }
                last_event_ms = event.timestamp_ms;
            }

            if session_count > 0 {
                let mut current_session_start = sorted_events[0].timestamp_ms;
                let mut prev_ms = current_session_start;

                for event in sorted_events.iter().skip(1) {
                    if (event.timestamp_ms - prev_ms) > SESSION_GAP_MS {
                        active_ms += prev_ms - current_session_start;
                        current_session_start = event.timestamp_ms;
                    }
                    prev_ms = event.timestamp_ms;
                }
                active_ms += prev_ms - current_session_start;
            }

            stats.sessions_count = session_count;
            stats.active_seconds = (active_ms / 1000) as u64;

            for (_, proj) in stats.per_project.iter_mut() {
                proj.active_seconds = stats.active_seconds;
            }
            for (_, vol) in stats.per_volume.iter_mut() {
                vol.active_seconds = stats.active_seconds;
            }
            for (_, chap) in stats.per_chapter.iter_mut() {
                chap.active_seconds = stats.active_seconds;
            }

            stats.speed_buckets = self.compute_speed_buckets(&sorted_events, 60_000)?;

            daily_stats_list.push(stats);
        }

        daily_stats_list.sort_by(|a, b| a.date.cmp(&b.date).then(a.device_id.cmp(&b.device_id)));
        Ok(daily_stats_list)
    }

    fn compute_speed_buckets(
        &self,
        events: &[&WritingInputEvent],
        bucket_ms: i64,
    ) -> Result<Vec<SpeedBucket>> {
        let (Some(first), Some(last)) = (events.first(), events.last()) else {
            return Ok(Vec::new());
        };

        let first_ms = first.timestamp_ms;
        let last_ms = last.timestamp_ms;

        let mut buckets = Vec::new();
        let mut bucket_start = first_ms;
        let mut event_idx = 0;

        while bucket_start <= last_ms {
            let bucket_end = bucket_start + bucket_ms;
            let mut chars_in_bucket: u32 = 0;

            while event_idx < events.len() && events[event_idx].timestamp_ms < bucket_end {
                if events[event_idx].timestamp_ms >= bucket_start {
                    chars_in_bucket += events[event_idx].inserted_chars;
                }
                event_idx += 1;
            }

            let minutes = bucket_ms as f64 / 60_000.0;
            let chars_per_minute = if minutes > 0.0 {
                chars_in_bucket as f64 / minutes
            } else {
                0.0
            };

            buckets.push(SpeedBucket {
                start_ms: bucket_start,
                end_ms: bucket_end,
                chars_typed: chars_in_bucket,
                chars_per_minute,
            });

            bucket_start = bucket_end;
        }

        Ok(buckets)
    }

    pub fn timestamp_to_date(&self, timestamp_ms: i64) -> Result<String> {
        let dt = chrono::DateTime::from_timestamp_millis(timestamp_ms)
            .ok_or_else(|| crate::Error::Other("Invalid timestamp".to_string()))?;
        Ok(dt.format("%Y-%m-%d").to_string())
    }

    pub fn merge_daily_stats(
        &self,
        existing: &mut DailyStats,
        incoming: &DailyStats,
    ) -> Result<()> {
        if existing.date != incoming.date || existing.device_id != incoming.device_id {
            return Err(crate::Error::Other(
                "Cannot merge stats from different dates or devices".to_string(),
            ));
        }

        existing.total_human_typed_chars += incoming.total_human_typed_chars;
        existing.total_pasted_chars += incoming.total_pasted_chars;
        existing.total_deleted_chars += incoming.total_deleted_chars;
        existing.total_ai_inserted_chars += incoming.total_ai_inserted_chars;
        existing.total_net_delta_chars += incoming.total_net_delta_chars;
        existing.active_seconds += incoming.active_seconds;
        existing.sessions_count += incoming.sessions_count;

        for (project_id, incoming_proj) in &incoming.per_project {
            let proj = existing.per_project.entry(project_id.clone()).or_default();
            proj.human_typed_chars += incoming_proj.human_typed_chars;
            proj.pasted_chars += incoming_proj.pasted_chars;
            proj.deleted_chars += incoming_proj.deleted_chars;
            proj.ai_inserted_chars += incoming_proj.ai_inserted_chars;
            proj.net_delta_chars += incoming_proj.net_delta_chars;
            proj.active_seconds += incoming_proj.active_seconds;
        }

        for (volume_id, incoming_vol) in &incoming.per_volume {
            let vol = existing.per_volume.entry(volume_id.clone()).or_default();
            vol.human_typed_chars += incoming_vol.human_typed_chars;
            vol.pasted_chars += incoming_vol.pasted_chars;
            vol.deleted_chars += incoming_vol.deleted_chars;
            vol.ai_inserted_chars += incoming_vol.ai_inserted_chars;
            vol.net_delta_chars += incoming_vol.net_delta_chars;
            vol.active_seconds += incoming_vol.active_seconds;
        }

        for (chapter_id, incoming_chap) in &incoming.per_chapter {
            let chap = existing.per_chapter.entry(chapter_id.clone()).or_default();
            chap.human_typed_chars += incoming_chap.human_typed_chars;
            chap.pasted_chars += incoming_chap.pasted_chars;
            chap.deleted_chars += incoming_chap.deleted_chars;
            chap.ai_inserted_chars += incoming_chap.ai_inserted_chars;
            chap.net_delta_chars += incoming_chap.net_delta_chars;
            chap.active_seconds += incoming_chap.active_seconds;
        }

        Ok(())
    }
}

/// 按设备类别汇总的统计数据
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeviceClassSummary {
    pub device_count: u32,
    pub total_human_typed_chars: u64,
    pub total_net_delta_chars: i64,
    pub active_seconds: u64,
}

/// 按设备类别（phone / tablet / desktop）汇总统计。
/// 对于旧数据没有 device_class 字段的情况，根据 platform 推断。
pub fn aggregate_by_device_class(stats: &[DailyStats]) -> HashMap<String, DeviceClassSummary> {
    let mut result: HashMap<String, DeviceClassSummary> = HashMap::new();
    for stat in stats {
        let class = if stat.device_class.is_empty() {
            // 兼容旧数据：根据 platform 推断
            if stat.platform.contains("android") {
                "phone".to_string()
            } else {
                "desktop".to_string()
            }
        } else {
            stat.device_class.clone()
        };
        let entry = result.entry(class).or_default();
        entry.device_count += 1;
        entry.total_human_typed_chars += stat.total_human_typed_chars;
        entry.total_net_delta_chars += stat.total_net_delta_chars;
        entry.active_seconds += stat.active_seconds;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn create_mock_store() -> StatsStore {
        StatsStore::new(Path::new("/tmp/mock_workspace"))
    }

    #[test]
    fn test_flush_events_empty() {
        let store = create_mock_store();
        assert!(store.flush_events().is_ok());
    }

    #[test]
    fn test_merge_daily_stats_success() {
        let store = create_mock_store();

        let mut existing = DailyStats {
            date: "2023-10-26".to_string(),
            device_id: "device_1".to_string(),
            platform: "desktop".to_string(),
            device_class: "desktop".to_string(),
            total_human_typed_chars: 100,
            total_pasted_chars: 50,
            total_deleted_chars: 10,
            total_ai_inserted_chars: 20,
            total_net_delta_chars: 160,
            active_seconds: 300,
            sessions_count: 2,
            per_project: HashMap::new(),
            per_volume: HashMap::new(),
            per_chapter: HashMap::new(),
            speed_buckets: vec![],
        };

        let mut proj_stats = ProjectStats::default();
        proj_stats.human_typed_chars = 100;
        proj_stats.active_seconds = 300;
        existing
            .per_project
            .insert("proj_1".to_string(), proj_stats);

        let incoming = DailyStats {
            date: "2023-10-26".to_string(),
            device_id: "device_1".to_string(),
            platform: "desktop".to_string(),
            device_class: "desktop".to_string(),
            total_human_typed_chars: 50,
            total_pasted_chars: 10,
            total_deleted_chars: 5,
            total_ai_inserted_chars: 0,
            total_net_delta_chars: 55,
            active_seconds: 100,
            sessions_count: 1,
            per_project: {
                let mut map = HashMap::new();
                let mut ps = ProjectStats::default();
                ps.human_typed_chars = 50;
                ps.active_seconds = 100;
                map.insert("proj_1".to_string(), ps);

                let mut ps2 = ProjectStats::default();
                ps2.human_typed_chars = 20;
                map.insert("proj_2".to_string(), ps2);
                map
            },
            per_volume: {
                let mut map = HashMap::new();
                let mut vs = VolumeStats::default();
                vs.pasted_chars = 10;
                map.insert("vol_1".to_string(), vs);
                map
            },
            per_chapter: {
                let mut map = HashMap::new();
                let mut cs = ChapterStats::default();
                cs.deleted_chars = 5;
                map.insert("chap_1".to_string(), cs);
                map
            },
            speed_buckets: vec![],
        };

        let result = store.merge_daily_stats(&mut existing, &incoming);
        assert!(result.is_ok());

        // Verify top-level
        assert_eq!(existing.total_human_typed_chars, 150);
        assert_eq!(existing.total_pasted_chars, 60);
        assert_eq!(existing.total_deleted_chars, 15);
        assert_eq!(existing.total_ai_inserted_chars, 20);
        assert_eq!(existing.total_net_delta_chars, 215);
        assert_eq!(existing.active_seconds, 400);
        assert_eq!(existing.sessions_count, 3);

        // Verify per_project
        assert_eq!(existing.per_project.len(), 2);
        assert_eq!(
            existing
                .per_project
                .get("proj_1")
                .unwrap()
                .human_typed_chars,
            150
        );
        assert_eq!(
            existing.per_project.get("proj_1").unwrap().active_seconds,
            400
        );
        assert_eq!(
            existing
                .per_project
                .get("proj_2")
                .unwrap()
                .human_typed_chars,
            20
        );

        // Verify per_volume
        assert_eq!(existing.per_volume.len(), 1);
        assert_eq!(existing.per_volume.get("vol_1").unwrap().pasted_chars, 10);

        // Verify per_chapter
        assert_eq!(existing.per_chapter.len(), 1);
        assert_eq!(existing.per_chapter.get("chap_1").unwrap().deleted_chars, 5);
    }

    #[test]
    fn test_merge_daily_stats_different_date() {
        let store = create_mock_store();

        let mut existing = DailyStats::default();
        existing.date = "2023-10-26".to_string();
        existing.device_id = "device_1".to_string();

        let incoming = DailyStats {
            date: "2023-10-27".to_string(),
            device_id: "device_1".to_string(),
            ..Default::default()
        };

        let result = store.merge_daily_stats(&mut existing, &incoming);
        assert!(result.is_err());
        if let Err(crate::Error::Other(msg)) = result {
            assert_eq!(msg, "Cannot merge stats from different dates or devices");
        } else {
            panic!("Expected Error::Other");
        }
    }

    #[test]
    fn test_merge_daily_stats_different_device() {
        let store = create_mock_store();

        let mut existing = DailyStats::default();
        existing.date = "2023-10-26".to_string();
        existing.device_id = "device_1".to_string();

        let incoming = DailyStats {
            date: "2023-10-26".to_string(),
            device_id: "device_2".to_string(),
            ..Default::default()
        };

        let result = store.merge_daily_stats(&mut existing, &incoming);
        assert!(result.is_err());
        if let Err(crate::Error::Other(msg)) = result {
            assert_eq!(msg, "Cannot merge stats from different dates or devices");
        } else {
            panic!("Expected Error::Other");
        }
    }

    #[test]
    fn test_aggregate_by_device_class() {
        let stats = vec![
            DailyStats {
                date: "2024-01-01".to_string(),
                device_id: "dev1".to_string(),
                platform: "desktop".to_string(),
                device_class: "desktop".to_string(),
                total_human_typed_chars: 100,
                total_net_delta_chars: 80,
                active_seconds: 300,
                ..Default::default()
            },
            DailyStats {
                date: "2024-01-01".to_string(),
                device_id: "dev2".to_string(),
                platform: "desktop".to_string(),
                device_class: "desktop".to_string(),
                total_human_typed_chars: 200,
                total_net_delta_chars: 150,
                active_seconds: 600,
                ..Default::default()
            },
            DailyStats {
                date: "2024-01-01".to_string(),
                device_id: "dev3".to_string(),
                platform: "android".to_string(),
                device_class: "phone".to_string(),
                total_human_typed_chars: 50,
                total_net_delta_chars: 40,
                active_seconds: 120,
                ..Default::default()
            },
        ];

        let result = aggregate_by_device_class(&stats);
        assert_eq!(result.len(), 2);

        let desktop = result.get("desktop").unwrap();
        assert_eq!(desktop.device_count, 2);
        assert_eq!(desktop.total_human_typed_chars, 300);
        assert_eq!(desktop.total_net_delta_chars, 230);
        assert_eq!(desktop.active_seconds, 900);

        let phone = result.get("phone").unwrap();
        assert_eq!(phone.device_count, 1);
        assert_eq!(phone.total_human_typed_chars, 50);
        assert_eq!(phone.total_net_delta_chars, 40);
        assert_eq!(phone.active_seconds, 120);
    }

    #[test]
    fn test_aggregate_by_device_class_legacy_data() {
        // 旧数据没有 device_class，应根据 platform 推断
        let stats = vec![
            DailyStats {
                date: "2024-01-01".to_string(),
                device_id: "dev1".to_string(),
                platform: "desktop".to_string(),
                device_class: String::new(), // 旧数据
                total_human_typed_chars: 100,
                total_net_delta_chars: 80,
                active_seconds: 300,
                ..Default::default()
            },
            DailyStats {
                date: "2024-01-01".to_string(),
                device_id: "dev2".to_string(),
                platform: "android".to_string(),
                device_class: String::new(), // 旧数据
                total_human_typed_chars: 50,
                total_net_delta_chars: 40,
                active_seconds: 120,
                ..Default::default()
            },
        ];

        let result = aggregate_by_device_class(&stats);
        assert_eq!(result.len(), 2);

        let desktop = result.get("desktop").unwrap();
        assert_eq!(desktop.device_count, 1);
        assert_eq!(desktop.total_human_typed_chars, 100);

        let phone = result.get("phone").unwrap();
        assert_eq!(phone.device_count, 1);
        assert_eq!(phone.total_human_typed_chars, 50);
    }

    fn create_mock_event() -> WritingInputEvent {
        WritingInputEvent::new(
            "device_1",
            crate::writing_stats::Platform::Desktop,
            "desktop",
            "proj_1",
            "vol_1",
            "chap_1",
            crate::writing_stats::EventSource::HumanTyped,
            10,
            0,
            0,
            0,
            5,
            "session_1",
        )
    }

    #[test]
    fn test_record_event_buffers_without_flushing() {
        let store = create_mock_store();

        let event = create_mock_event();
        store.record_event(event).unwrap();

        let buffer = store.event_buffer.lock().unwrap();
        assert_eq!(buffer.len(), 1);
    }

    #[test]
    fn test_record_event_flushes_on_max_buffer_size() {
        let store = create_mock_store();

        // Add MAX_BUFFER_SIZE - 1 events
        for _ in 0..(MAX_BUFFER_SIZE - 1) {
            let event = create_mock_event();
            store.record_event(event).unwrap();
        }

        {
            let buffer = store.event_buffer.lock().unwrap();
            assert_eq!(buffer.len(), MAX_BUFFER_SIZE - 1);
        }

        // Add one more event to trigger flush
        let event = create_mock_event();
        store.record_event(event).unwrap();

        // Buffer should be empty after flush
        {
            let buffer = store.event_buffer.lock().unwrap();
            assert!(buffer.is_empty());
        }
    }

    #[test]
    fn test_record_event_flushes_on_debounce_time() {
        let store = create_mock_store();

        // Set last_flush far in the past to trigger time-based flush
        {
            let mut last_flush = store.last_flush_ms.lock().unwrap();
            *last_flush = chrono::Utc::now().timestamp_millis() - FLUSH_DEBOUNCE_MS - 1000;
        }

        let event = create_mock_event();
        store.record_event(event).unwrap();

        // Buffer should be empty after flush
        let buffer = store.event_buffer.lock().unwrap();
        assert!(buffer.is_empty());
    }
}
