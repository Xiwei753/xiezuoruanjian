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
            let mut stats = DailyStats {
                date: date.format("%Y-%m-%d").to_string(),
                device_id: device_id.to_string(),
                platform,
                ..Default::default()
            };

            for event in &day_events {
                match event.source {
                    crate::writing_stats::EventSource::HumanTyped => {
                        stats.total_human_typed_chars += event.inserted_chars as u64;
                    }
                    crate::writing_stats::EventSource::Pasted => {
                        stats.total_pasted_chars += event.pasted_chars as u64;
                    }
                    crate::writing_stats::EventSource::Deleted => {
                        stats.total_deleted_chars += event.deleted_chars as u64;
                    }
                    crate::writing_stats::EventSource::AiInserted => {
                        stats.total_ai_inserted_chars += event.ai_inserted_chars as u64;
                    }
                    _ => {}
                }
                stats.total_net_delta_chars += event.net_delta_chars as i64;

                let proj = stats
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

                let vol = stats.per_volume.entry(event.volume_id.clone()).or_default();
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

                let chap = stats
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

        while bucket_start <= last_ms {
            let bucket_end = bucket_start + bucket_ms;
            let mut chars_in_bucket: u32 = 0;

            for event in events {
                if event.timestamp_ms >= bucket_start && event.timestamp_ms < bucket_end {
                    chars_in_bucket += event.inserted_chars;
                }
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
