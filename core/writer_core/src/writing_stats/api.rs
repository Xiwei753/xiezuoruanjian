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
use crate::writing_stats::{DateRange, WritingInputEvent};
use serde_json::Value;
use std::path::Path;

pub struct StatsApi {
    aggregator: StatsAggregator,
}

impl StatsApi {
    pub fn new(workspace_path: &Path) -> Self {
        Self {
            aggregator: StatsAggregator::new(workspace_path),
        }
    }

    pub fn aggregator(&self) -> &StatsAggregator {
        &self.aggregator
    }

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
                "start_date": range.start_date,
                "end_date": range.end_date,
            },
            "total_human_typed_chars": total_human_typed,
            "total_pasted_chars": total_pasted,
            "total_deleted_chars": total_deleted,
            "total_ai_inserted_chars": total_ai_inserted,
            "total_net_delta_chars": total_net_delta,
            "total_active_seconds": total_active_seconds,
            "total_sessions": total_sessions,
            "days_count": daily_stats.len(),
        }))
    }

    pub fn get_stats_by_project(&self, range: &DateRange) -> Result<Value> {
        let daily_stats = self
            .aggregator
            .store()
            .load_daily_stats_range(&range.start_date, &range.end_date)?;

        let mut by_project: std::collections::HashMap<String, serde_json::Value> =
            std::collections::HashMap::new();

        for stats in &daily_stats {
            for (project_id, proj_stats) in &stats.per_project {
                let entry = by_project.entry(project_id.clone()).or_insert_with(|| {
                    serde_json::json!({
                        "project_id": project_id,
                        "human_typed_chars": 0u64,
                        "pasted_chars": 0u64,
                        "deleted_chars": 0u64,
                        "ai_inserted_chars": 0u64,
                        "net_delta_chars": 0i64,
                        "active_seconds": 0u64,
                    })
                });

                if let Some(obj) = entry.as_object_mut() {
                    *obj.get_mut("human_typed_chars").unwrap() = serde_json::json!(
                        obj.get("human_typed_chars").unwrap().as_u64().unwrap_or(0)
                            + proj_stats.human_typed_chars
                    );
                    *obj.get_mut("pasted_chars").unwrap() = serde_json::json!(
                        obj.get("pasted_chars").unwrap().as_u64().unwrap_or(0)
                            + proj_stats.pasted_chars
                    );
                    *obj.get_mut("deleted_chars").unwrap() = serde_json::json!(
                        obj.get("deleted_chars").unwrap().as_u64().unwrap_or(0)
                            + proj_stats.deleted_chars
                    );
                    *obj.get_mut("ai_inserted_chars").unwrap() = serde_json::json!(
                        obj.get("ai_inserted_chars").unwrap().as_u64().unwrap_or(0)
                            + proj_stats.ai_inserted_chars
                    );
                    *obj.get_mut("net_delta_chars").unwrap() = serde_json::json!(
                        obj.get("net_delta_chars").unwrap().as_i64().unwrap_or(0)
                            + proj_stats.net_delta_chars
                    );
                    *obj.get_mut("active_seconds").unwrap() = serde_json::json!(
                        obj.get("active_seconds").unwrap().as_u64().unwrap_or(0)
                            + proj_stats.active_seconds
                    );
                }
            }
        }

        let projects: Vec<Value> = by_project.into_values().collect();
        Ok(serde_json::json!({
            "range": {
                "start_date": range.start_date,
                "end_date": range.end_date,
            },
            "projects": projects,
        }))
    }

    pub fn get_stats_by_chapter(&self, range: &DateRange) -> Result<Value> {
        let daily_stats = self
            .aggregator
            .store()
            .load_daily_stats_range(&range.start_date, &range.end_date)?;

        let mut by_chapter: std::collections::HashMap<String, serde_json::Value> =
            std::collections::HashMap::new();

        for stats in &daily_stats {
            for (chapter_id, chap_stats) in &stats.per_chapter {
                let entry = by_chapter.entry(chapter_id.clone()).or_insert_with(|| {
                    serde_json::json!({
                        "chapter_id": chapter_id,
                        "human_typed_chars": 0u64,
                        "pasted_chars": 0u64,
                        "deleted_chars": 0u64,
                        "ai_inserted_chars": 0u64,
                        "net_delta_chars": 0i64,
                        "active_seconds": 0u64,
                    })
                });

                if let Some(obj) = entry.as_object_mut() {
                    *obj.get_mut("human_typed_chars").unwrap() = serde_json::json!(
                        obj.get("human_typed_chars").unwrap().as_u64().unwrap_or(0)
                            + chap_stats.human_typed_chars
                    );
                    *obj.get_mut("pasted_chars").unwrap() = serde_json::json!(
                        obj.get("pasted_chars").unwrap().as_u64().unwrap_or(0)
                            + chap_stats.pasted_chars
                    );
                    *obj.get_mut("deleted_chars").unwrap() = serde_json::json!(
                        obj.get("deleted_chars").unwrap().as_u64().unwrap_or(0)
                            + chap_stats.deleted_chars
                    );
                    *obj.get_mut("ai_inserted_chars").unwrap() = serde_json::json!(
                        obj.get("ai_inserted_chars").unwrap().as_u64().unwrap_or(0)
                            + chap_stats.ai_inserted_chars
                    );
                    *obj.get_mut("net_delta_chars").unwrap() = serde_json::json!(
                        obj.get("net_delta_chars").unwrap().as_i64().unwrap_or(0)
                            + chap_stats.net_delta_chars
                    );
                    *obj.get_mut("active_seconds").unwrap() = serde_json::json!(
                        obj.get("active_seconds").unwrap().as_u64().unwrap_or(0)
                            + chap_stats.active_seconds
                    );
                }
            }
        }

        let chapters: Vec<Value> = by_chapter.into_values().collect();
        Ok(serde_json::json!({
            "range": {
                "start_date": range.start_date,
                "end_date": range.end_date,
            },
            "chapters": chapters,
        }))
    }

    pub fn get_stats_by_device(&self, range: &DateRange) -> Result<Value> {
        let daily_stats = self
            .aggregator
            .store()
            .load_daily_stats_range(&range.start_date, &range.end_date)?;

        let mut by_device: std::collections::HashMap<String, serde_json::Value> =
            std::collections::HashMap::new();

        for stats in &daily_stats {
            let entry = by_device.entry(stats.device_id.clone()).or_insert_with(|| {
                serde_json::json!({
                    "device_id": stats.device_id,
                    "platform": stats.platform,
                    "human_typed_chars": 0u64,
                    "pasted_chars": 0u64,
                    "deleted_chars": 0u64,
                    "ai_inserted_chars": 0u64,
                    "net_delta_chars": 0i64,
                    "active_seconds": 0u64,
                    "sessions_count": 0u32,
                })
            });

            if let Some(obj) = entry.as_object_mut() {
                *obj.get_mut("human_typed_chars").unwrap() = serde_json::json!(
                    obj.get("human_typed_chars").unwrap().as_u64().unwrap_or(0)
                        + stats.total_human_typed_chars
                );
                *obj.get_mut("pasted_chars").unwrap() = serde_json::json!(
                    obj.get("pasted_chars").unwrap().as_u64().unwrap_or(0)
                        + stats.total_pasted_chars
                );
                *obj.get_mut("deleted_chars").unwrap() = serde_json::json!(
                    obj.get("deleted_chars").unwrap().as_u64().unwrap_or(0)
                        + stats.total_deleted_chars
                );
                *obj.get_mut("ai_inserted_chars").unwrap() = serde_json::json!(
                    obj.get("ai_inserted_chars").unwrap().as_u64().unwrap_or(0)
                        + stats.total_ai_inserted_chars
                );
                *obj.get_mut("net_delta_chars").unwrap() = serde_json::json!(
                    obj.get("net_delta_chars").unwrap().as_i64().unwrap_or(0)
                        + stats.total_net_delta_chars
                );
                *obj.get_mut("active_seconds").unwrap() = serde_json::json!(
                    obj.get("active_seconds").unwrap().as_u64().unwrap_or(0) + stats.active_seconds
                );
                *obj.get_mut("sessions_count").unwrap() = serde_json::json!(
                    obj.get("sessions_count").unwrap().as_u64().unwrap_or(0)
                        + stats.sessions_count as u64
                );
            }
        }

        let devices: Vec<Value> = by_device.into_values().collect();
        Ok(serde_json::json!({
            "range": {
                "start_date": range.start_date,
                "end_date": range.end_date,
            },
            "devices": devices,
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
                    "start_ms": b.start_ms,
                    "end_ms": b.end_ms,
                    "chars_typed": b.chars_typed,
                    "chars_per_minute": b.chars_per_minute,
                })
            })
            .collect();

        Ok(serde_json::json!({
            "range": {
                "start_date": range.start_date,
                "end_date": range.end_date,
            },
            "bucket_minutes": bucket_minutes,
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
