//! # 写作统计 API 模块
//!
//! 本模块提供了写作统计数据的查询接口，是统计功能对外暴露的主要 API 层。
//!
//! ## 主要功能
//!
//! - **统计摘要**: 获取指定时间范围内的总体统计数据
//! - **按项目统计**: 获取按项目分组的统计数据
//! - **按章节统计**: 获取按章节分组的统计数据
//! - **按设备统计**: 获取按设备分组的统计数据，支持多设备对比
//! - **速度曲线**: 获取指定时间范围内的写作速度变化曲线
//! - **事件记录**: 记录新的写作输入事件
//!
//! ## 核心结构
//!
//! - `StatsApi`: 统计 API 入口，封装了 StatsAggregator 并提供高层查询接口
//!
//! ## 返回格式
//!
//! 所有查询方法返回 `serde_json::Value`，便于直接序列化为 JSON 响应。
//! 返回数据包含时间范围信息和对应的统计数据。
//!
//! ## 依赖关系
//!
//! - `crate::writing_stats::aggregate`: 统计聚合器
//! - `crate::writing_stats::store`: 数据存储层
//! - `serde_json`: JSON 值处理
//!
//! ## 使用场景
//!
//! - 编辑器中的字数统计显示
//! - 写作报告生成
//! - 数据可视化图表的数据源
//! - 多设备写作活动对比分析

use crate::error::Result;
use crate::writing_stats::aggregate::StatsAggregator;
use crate::writing_stats::store::aggregate_by_device_class;
use crate::writing_stats::{DateRange, WritingInputEvent};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

/// 按项目聚合的统计数据。
///
/// `net_delta_chars` = `human_typed_chars` + `pasted_chars` + `ai_inserted_chars` - `deleted_chars`，
/// 可能为负值（删除多于新增）。`active_seconds` 为有输入事件的时间段累计，非挂机时间。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectStatsAgg {
    project_id: String,
    human_typed_chars: u64,
    pasted_chars: u64,
    deleted_chars: u64,
    ai_inserted_chars: u64,
    net_delta_chars: i64,
    active_seconds: u64,
}

/// 按章节聚合的统计数据。字段语义同 `ProjectStatsAgg`。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChapterStatsAgg {
    chapter_id: String,
    human_typed_chars: u64,
    pasted_chars: u64,
    deleted_chars: u64,
    ai_inserted_chars: u64,
    net_delta_chars: i64,
    active_seconds: u64,
}

/// 按设备聚合的统计数据。
///
/// - `device_id`：设备唯一标识（UUID，由 Core 在首次同步时生成）
/// - `platform`：平台标识（`"android"` / `"desktop"` / `"windows"` / `"harmony"` / `"apple"`）
/// - `device_class`：设备类型（`"phone"` / `"tablet"` / `"desktop"`）
/// - `sessions_count`：活跃编辑会话数（有输入事件的天数）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceStatsAgg {
    device_id: String,
    platform: String,
    device_class: String,
    human_typed_chars: u64,
    pasted_chars: u64,
    deleted_chars: u64,
    ai_inserted_chars: u64,
    net_delta_chars: i64,
    active_seconds: u64,
    sessions_count: u32,
}

/// 按设备类型聚合的统计数据 — 用于多设备对比视图。
///
/// `device_count` 为该类型的设备数量，其余字段为该类型所有设备的累计值。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceClassAgg {
    device_class: String,
    device_count: u32,
    total_human_typed_chars: u64,
    total_net_delta_chars: i64,
    active_seconds: u64,
}

/// 写作统计 API 入口 — 封装 StatsAggregator 并提供高层查询接口。
///
/// 线程安全：此结构体不是 `Sync`/`Send`，调用方需保证单线程访问
/// （通过 `WriterAppService` 的 `Mutex` 保护）。
/// 所有查询方法返回 `serde_json::Value`，便于直接序列化为 JSON 响应。
pub struct StatsApi {
    aggregator: StatsAggregator,
}

impl StatsApi {
    pub fn new(app_data_root: &Path) -> Self {
        Self {
            aggregator: StatsAggregator::new(app_data_root),
        }
    }

    pub fn aggregator(&self) -> &StatsAggregator {
        &self.aggregator
    }

    #[allow(clippy::cast_possible_truncation)]
    #[allow(clippy::cast_possible_truncation)]
    pub fn get_stats_summary(&self, range: &DateRange) -> Result<Value> {
        let daily_stats = self
            .aggregator
            .store()
            .load_daily_stats_range(&range.start_date, &range.end_date)?;

        let mut total_human_typed: u64 = 0;
        let mut total_pasted: u64 = 0;
        let mut total_deleted: u64 = 0;
        let mut total_ai_inserted: u64 = 0;
        let mut total_net_delta: i64 = 0;
        let mut total_active_seconds: u64 = 0;
        let mut total_sessions: u32 = 0;

        for stats in &daily_stats {
            total_human_typed += stats.total_human_typed_chars;
            total_pasted += stats.total_pasted_chars;
            total_deleted += stats.total_deleted_chars;
            total_ai_inserted += stats.total_ai_inserted_chars;
            total_net_delta += stats.total_net_delta_chars;
            total_active_seconds += stats.active_seconds;
            total_sessions += stats.sessions_count;
        }

        Ok(serde_json::json!({
            "range": {
                "startDate": range.start_date,
                "endDate": range.end_date,
            },
            "totalHumanTypedChars": total_human_typed,
            "totalPastedChars": total_pasted,
            "totalDeletedChars": total_deleted,
            "totalAiInsertedChars": total_ai_inserted,
            "totalNetDeltaChars": total_net_delta,
            "totalActiveSeconds": total_active_seconds,
            "totalSessions": total_sessions,
            "daysCount": daily_stats.len() as u32,
        }))
    }

    pub fn get_stats_by_project(&self, range: &DateRange) -> Result<Value> {
        let daily_stats = self
            .aggregator
            .store()
            .load_daily_stats_range(&range.start_date, &range.end_date)?;

        let mut by_project: std::collections::HashMap<&str, ProjectStatsAgg> =
            std::collections::HashMap::new();

        for stats in &daily_stats {
            for (project_id, proj_stats) in &stats.per_project {
                let entry =
                    by_project
                        .entry(project_id.as_str())
                        .or_insert_with(|| ProjectStatsAgg {
                            project_id: project_id.clone(),
                            human_typed_chars: 0,
                            pasted_chars: 0,
                            deleted_chars: 0,
                            ai_inserted_chars: 0,
                            net_delta_chars: 0,
                            active_seconds: 0,
                        });

                entry.human_typed_chars += proj_stats.human_typed_chars;
                entry.pasted_chars += proj_stats.pasted_chars;
                entry.deleted_chars += proj_stats.deleted_chars;
                entry.ai_inserted_chars += proj_stats.ai_inserted_chars;
                entry.net_delta_chars += proj_stats.net_delta_chars;
                entry.active_seconds += proj_stats.active_seconds;
            }
        }

        let projects: Vec<Value> = by_project
            .into_values()
            .map(|agg| serde_json::to_value(agg).unwrap_or(Value::Null))
            .collect();

        Ok(serde_json::json!({
            "range": {
                "startDate": range.start_date,
                "endDate": range.end_date,
            },
            "projects": projects,
        }))
    }

    pub fn get_stats_by_chapter(&self, range: &DateRange) -> Result<Value> {
        let daily_stats = self
            .aggregator
            .store()
            .load_daily_stats_range(&range.start_date, &range.end_date)?;

        let mut by_chapter: std::collections::HashMap<&str, ChapterStatsAgg> =
            std::collections::HashMap::new();

        for stats in &daily_stats {
            for (chapter_id, chap_stats) in &stats.per_chapter {
                let entry =
                    by_chapter
                        .entry(chapter_id.as_str())
                        .or_insert_with(|| ChapterStatsAgg {
                            chapter_id: chapter_id.clone(),
                            human_typed_chars: 0,
                            pasted_chars: 0,
                            deleted_chars: 0,
                            ai_inserted_chars: 0,
                            net_delta_chars: 0,
                            active_seconds: 0,
                        });

                entry.human_typed_chars += chap_stats.human_typed_chars;
                entry.pasted_chars += chap_stats.pasted_chars;
                entry.deleted_chars += chap_stats.deleted_chars;
                entry.ai_inserted_chars += chap_stats.ai_inserted_chars;
                entry.net_delta_chars += chap_stats.net_delta_chars;
                entry.active_seconds += chap_stats.active_seconds;
            }
        }

        let chapters: Vec<Value> = by_chapter
            .into_values()
            .map(|agg| serde_json::to_value(agg).unwrap_or(Value::Null))
            .collect();

        Ok(serde_json::json!({
            "range": {
                "startDate": range.start_date,
                "endDate": range.end_date,
            },
            "chapters": chapters,
        }))
    }

    pub fn get_stats_by_device(&self, range: &DateRange) -> Result<Value> {
        let daily_stats = self
            .aggregator
            .store()
            .load_daily_stats_range(&range.start_date, &range.end_date)?;

        let mut by_device: std::collections::HashMap<&str, DeviceStatsAgg> =
            std::collections::HashMap::new();

        for stats in &daily_stats {
            let entry = by_device
                .entry(stats.device_id.as_str())
                .or_insert_with(|| DeviceStatsAgg {
                    device_id: stats.device_id.clone(),
                    platform: stats.platform.clone(),
                    device_class: stats.device_class.clone(),
                    human_typed_chars: 0,
                    pasted_chars: 0,
                    deleted_chars: 0,
                    ai_inserted_chars: 0,
                    net_delta_chars: 0,
                    active_seconds: 0,
                    sessions_count: 0,
                });

            entry.human_typed_chars += stats.total_human_typed_chars;
            entry.pasted_chars += stats.total_pasted_chars;
            entry.deleted_chars += stats.total_deleted_chars;
            entry.ai_inserted_chars += stats.total_ai_inserted_chars;
            entry.net_delta_chars += stats.total_net_delta_chars;
            entry.active_seconds += stats.active_seconds;
            entry.sessions_count += stats.sessions_count;
        }

        let devices: Vec<Value> = by_device
            .into_values()
            .map(|agg| serde_json::to_value(agg).unwrap_or(Value::Null))
            .collect();

        Ok(serde_json::json!({
            "range": {
                "startDate": range.start_date,
                "endDate": range.end_date,
            },
            "devices": devices,
        }))
    }

    pub fn get_stats_by_device_class(&self, range: &DateRange) -> Result<Value> {
        let daily_stats = self
            .aggregator
            .store()
            .load_daily_stats_range(&range.start_date, &range.end_date)?;

        let by_class = aggregate_by_device_class(&daily_stats);

        let classes: Vec<Value> = by_class
            .into_iter()
            .map(|(device_class, summary)| {
                let agg = DeviceClassAgg {
                    device_class,
                    device_count: summary.device_count,
                    total_human_typed_chars: summary.total_human_typed_chars,
                    total_net_delta_chars: summary.total_net_delta_chars,
                    active_seconds: summary.active_seconds,
                };
                serde_json::to_value(agg).unwrap_or(Value::Null)
            })
            .collect();

        Ok(serde_json::json!({
            "range": {
                "startDate": range.start_date,
                "endDate": range.end_date,
            },
            "deviceClasses": classes,
        }))
    }

    pub fn get_speed_curve(&self, range: &DateRange, bucket_minutes: u32) -> Result<Value> {
        let buckets =
            self.aggregator
                .get_speed_curve(&range.start_date, &range.end_date, bucket_minutes)?;

        let bucket_json: Vec<Value> = buckets
            .iter()
            .map(|b| {
                serde_json::json!({
                    "startMs": b.start_ms,
                    "endMs": b.end_ms,
                    "charsTyped": b.chars_typed,
                    "charsPerMinute": b.chars_per_minute,
                })
            })
            .collect();

        Ok(serde_json::json!({
            "range": {
                "startDate": range.start_date,
                "endDate": range.end_date,
            },
            "bucketMinutes": bucket_minutes,
            "buckets": bucket_json,
        }))
    }

    pub fn record_event(&self, event: WritingInputEvent) -> Result<()> {
        self.aggregator.store().record_event(event.clone())?;
        self.aggregator.aggregate_single_event(&event)?;
        Ok(())
    }

    pub fn flush(&self) -> Result<()> {
        self.aggregator.store().flush_events()
    }

    pub fn today_date() -> String {
        chrono::Utc::now().format("%Y-%m-%d").to_string()
    }
}
