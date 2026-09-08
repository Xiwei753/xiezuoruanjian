//! # 写作统计模块
//!
//! 本模块提供了完整的写作活动统计功能，用于追踪和分析用户的写作行为数据。
//!
//! ## 主要功能
//!
//! - **输入事件追踪**: 记录用户的每一次输入操作，包括打字、粘贴、删除、AI插入等
//! - **多平台支持**: 支持 Linux 和 Android 平台的统计
//! - **会话管理**: 通过 session_id 关联同一写作会话的多个事件
//! - **精确计量**: 分别统计不同来源的字符数，计算净增字符数
//!
//! ## 模块结构
//!
//! - `aggregate`: 统计数据聚合模块，负责将原始事件汇总为每日统计
//! - `api`: 统计 API 模块，提供对外的查询接口
//! - `store`: 数据存储模块，负责事件和统计数据的持久化
//!
//! ## 核心结构
//!
//! - `WritingInputEvent`: 写作输入事件，记录单次输入操作的详细信息
//! - `EventSource`: 事件来源枚举（人工输入/粘贴/删除/AI插入/同步/未知）
//! - `Platform`: 平台枚举（Linux/Android）
//! - `DateRange`: 日期范围，用于查询统计数据
//!
//! ## 依赖关系
//!
//! - `serde`: 序列化/反序列化支持
//! - `uuid`: 唯一标识符生成
//! - `chrono`: 时间戳处理
//!
//! ## 使用场景
//!
//! - 写作字数统计和进度追踪
//! - 写作速度分析
//! - 多设备写作活动汇总
//! - 写作习惯分析和报告生成

pub mod aggregate;
pub mod api;
pub mod store;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 事件来源 — 决定字符数分配到哪个计数器。
///
/// 聚合时按此枚举分发：`HumanTyped`→`human_typed_chars`，`Pasted`→`pasted_chars`，
/// `Deleted`→`deleted_chars`，`AiInserted`→`ai_inserted_chars`。
/// `SyncRemote` 和 `Unknown` 不计入任何分类计数器，但仍计入 `net_delta_chars`。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EventSource {
    HumanTyped,
    Pasted,
    Deleted,
    AiInserted,
    SyncRemote,
    Unknown,
}

/// 平台标识 — 委托 `writer_platform_api::PlatformKind`
///
/// 保持向后兼容的字符串序列化，同时统一平台枚举定义。
/// `from_str_name` 支持旧字符串 `"linux"` / `"linux_qt"` 映射到 `Desktop`。
pub type Platform = writer_platform_api::PlatformKind;

/// 写作输入事件。
///
/// `net_delta_chars` = inserted + pasted + ai_inserted - deleted，由 `new()` 自动计算。
/// 各来源字符数独立记录，不做互斥：一次事件只应设置一个来源的非零字段，
/// 但结构上不强制，聚合时按 `source` 枚举分发到对应计数器。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WritingInputEvent {
    pub event_id: String,
    pub timestamp_ms: i64,
    pub device_id: String,
    pub platform: Platform,
    /// phone / tablet / desktop
    #[serde(default)]
    pub device_class: String,
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    pub source: EventSource,
    pub inserted_chars: u32,
    pub deleted_chars: u32,
    pub pasted_chars: u32,
    pub ai_inserted_chars: u32,
    pub net_delta_chars: i32,
    pub duration_seconds: u32,
    pub session_id: String,
}

impl WritingInputEvent {
    #[allow(clippy::too_many_arguments, clippy::cast_possible_wrap)]
    pub fn new(
        device_id: &str,
        platform: Platform,
        device_class: &str,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        source: EventSource,
        inserted_chars: u32,
        deleted_chars: u32,
        pasted_chars: u32,
        ai_inserted_chars: u32,
        duration_seconds: u32,
        session_id: &str,
    ) -> Self {
        let net = inserted_chars as i32 + pasted_chars as i32 + ai_inserted_chars as i32
            - deleted_chars as i32;
        Self {
            event_id: Uuid::new_v4().to_string(),
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            device_id: device_id.to_string(),
            platform,
            device_class: device_class.to_string(),
            project_id: project_id.to_string(),
            volume_id: volume_id.to_string(),
            chapter_id: chapter_id.to_string(),
            source,
            inserted_chars,
            deleted_chars,
            pasted_chars,
            ai_inserted_chars,
            net_delta_chars: net,
            duration_seconds,
            session_id: session_id.to_string(),
        }
    }

    pub fn human_typed_chars(&self) -> u32 {
        self.inserted_chars
    }

    pub fn is_input_event(&self) -> bool {
        matches!(self.source, EventSource::HumanTyped | EventSource::Pasted)
    }
}

/// 日期范围 — 查询统计数据的半开区间 `[start_date, end_date]`（两端包含）。
/// 日期格式为 `%Y-%m-%d`（ISO 8601），由调用方保证格式合法性。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DateRange {
    pub start_date: String,
    pub end_date: String,
}

#[cfg(test)]
mod inline_tests {
    use super::*;

    #[test]
    fn test_event_creation() {
        let event = WritingInputEvent::new(
            "device-1",
            Platform::Desktop,
            "desktop",
            "proj1",
            "vol1",
            "chap1",
            EventSource::HumanTyped,
            10,
            0,
            0,
            0,
            0,
            "session-1",
        );
        assert_eq!(event.inserted_chars, 10);
        assert_eq!(event.deleted_chars, 0);
        assert_eq!(event.net_delta_chars, 10);
        assert_eq!(event.source, EventSource::HumanTyped);
        assert_eq!(event.device_class, "desktop");
        assert!(event.is_input_event());
    }

    #[test]
    fn test_event_with_deletion() {
        let event = WritingInputEvent::new(
            "device-1",
            Platform::Desktop,
            "desktop",
            "proj1",
            "vol1",
            "chap1",
            EventSource::Deleted,
            0,
            5,
            0,
            0,
            0,
            "session-1",
        );
        assert_eq!(event.net_delta_chars, -5);
        assert!(!event.is_input_event());
    }

    #[test]
    fn test_event_with_paste() {
        let event = WritingInputEvent::new(
            "device-1",
            Platform::Android,
            "phone",
            "proj1",
            "vol1",
            "chap1",
            EventSource::Pasted,
            0,
            0,
            20,
            0,
            0,
            "session-1",
        );
        assert_eq!(event.pasted_chars, 20);
        assert_eq!(event.net_delta_chars, 20);
        assert_eq!(event.device_class, "phone");
        assert!(event.is_input_event());
    }

    #[test]
    fn test_event_with_ai_insert() {
        let event = WritingInputEvent::new(
            "device-1",
            Platform::Desktop,
            "desktop",
            "proj1",
            "vol1",
            "chap1",
            EventSource::AiInserted,
            0,
            0,
            0,
            50,
            0,
            "session-1",
        );
        assert_eq!(event.ai_inserted_chars, 50);
        assert_eq!(event.net_delta_chars, 50);
        assert!(!event.is_input_event());
    }

    #[test]
    fn test_event_sync_remote() {
        let event = WritingInputEvent::new(
            "device-1",
            Platform::Desktop,
            "desktop",
            "proj1",
            "vol1",
            "chap1",
            EventSource::SyncRemote,
            0,
            0,
            0,
            0,
            0,
            "session-1",
        );
        assert!(!event.is_input_event());
    }

    #[test]
    fn test_event_device_class_default_deserialization() {
        // 验证旧数据（没有 device_class 字段）可以正常反序列化，默认为空字符串
        let json = r#"{
            "event_id": "evt-1",
            "timestamp_ms": 1000,
            "device_id": "dev-1",
            "platform": "desktop",
            "project_id": "p1",
            "volume_id": "v1",
            "chapter_id": "c1",
            "source": "human_typed",
            "inserted_chars": 5,
            "deleted_chars": 0,
            "pasted_chars": 0,
            "ai_inserted_chars": 0,
            "net_delta_chars": 5,
            "duration_seconds": 0,
            "session_id": "s1"
        }"#;
        let event: WritingInputEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.device_id, "dev-1");
        assert_eq!(
            event.device_class, "",
            "device_class should default to empty string for old data"
        );
    }
}

#[cfg(test)]
mod tests;
