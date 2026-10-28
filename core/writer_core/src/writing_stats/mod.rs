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
//! - `api`: 统计查询接口，统一从合并后的事件库投影读数
//! - `calendar`: 业务日历（本地日、周/月边界、预设区间解析）
//! - `migration`: 旧版单机事件库到按来源设备分区事件库的一次性迁移
//! - `projection`: 从原始事件投影出汇总、纪录、趋势
//! - `store`: 事件持久化（按来源设备分区的 JSONL，按 `event_id` 去重）
//!
//! ## 跨设备事件库
//!
//! 原始事件按来源设备分区持久化：每台设备只写自己的
//! `stats/events/<device_id>/` 分区，其它设备下载来的分区只读；聚合时扫描
//! 全部已到达事件并按 `event_id` 去重。跨设备重复 ID 只保留先读到的一条，
//! 并把结构化冲突信息写入 `app-meta/stats/event_conflicts.local.jsonl` 供诊断。
//! 「正文当前字数」与「累计人工实际输入字数」是两种口径，不能用前者回推后者。
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

pub mod api;
pub mod calendar;
pub mod checkpoint;
pub mod contribution;
pub mod contribution_store;
pub(crate) mod contribution_transaction;
pub mod migration;
pub mod projection;
pub mod proof;
pub mod replay;
pub mod session;
pub mod store;

pub use contribution::{
    current_mutation_context, with_mutation_context, ContributionEvent, ContributionObjectSnapshot,
    ContributionObjectType, ContributionOperation, MutationActor, MutationContext,
    MutationEntrypoint,
};
pub use session::{
    WritingActivitySegment, WritingSessionClock, WritingSessionState,
};

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
    /// 事件来源设备的稳定标识；事件持久化在它的来源分区，跨设备同步后不重算。
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
    /// 已废弃：旧客户端上报的时长，不能作为真实写作时长的依据。
    ///
    /// 真实计时以 [`WritingActivitySegment`] 为准；这里的值只保留用于读取
    /// 历史事件，不参与速度分母、活跃时长或最长会话。
    #[serde(default)]
    pub duration_seconds: u32,
    /// 已废弃：旧客户端会话 ID，不再代表真实前台写作会话。
    ///
    /// 真实会话 ID 见 [`WritingActivitySegment::session_id`]。
    #[serde(default)]
    pub session_id: String,
    /// 事件发生时的**本机本地**日历日（`YYYY-MM-DD`），每日统计按它分桶。
    ///
    /// 必须持久化而不是每次从 `timestamp_ms` 现算：事件的归属日是该设备
    /// 记录事件时当地的午夜，之后 Core 可能运行在别的时区（跨时区移动、
    /// 设备换手、系统时区被改）。现算会让历史统计在换时区后整体漂移一天。
    ///
    /// 老事件没有这个字段，反序列化后为空串，此时回退到按 `timestamp_ms`
    /// 现算本机本地日——见 [`WritingInputEvent::business_date`]。
    #[serde(default)]
    pub local_date: String,
    /// 逐次编辑证明。旧事件/老客户端没有该字段，反序列化为 `None`：
    /// 仍参与写作统计，但不能逐字回放，也不能被当成有证明的记录导出。
    #[serde(default)]
    pub proof: Option<proof::ProofEditPayload>,
}

impl WritingInputEvent {
    #[allow(clippy::too_many_arguments)]
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
        Self::with_identity(
            String::new(),
            0,
            device_id,
            platform,
            device_class,
            project_id,
            volume_id,
            chapter_id,
            source,
            inserted_chars,
            deleted_chars,
            pasted_chars,
            ai_inserted_chars,
            duration_seconds,
            session_id,
        )
    }

    /// 用调用方固定的身份和发生时间创建事件（重发去重的关键）。
    ///
    /// 平台端排队上报的事件带两个不稳定身份：编辑事务当时的 `event_id`
    /// 和事件**发生**时间。重发同一 `event_id` 必须落成同一条事件，
    /// 不能再生成 UUID、更不能把旧事件的归属日改写成今天。
    ///
    /// - `event_id` 为空：调用方没有稳定身份（老客户端），生成 UUID；
    /// - `occurred_at_ms <= 0`：调用方没带时间（老客户端），用当前时间；
    /// - `local_date` 按最终时间戳在**事件发生地**定死，与 [`Self::new`] 一致。
    #[allow(clippy::too_many_arguments, clippy::cast_possible_wrap)]
    pub fn with_identity(
        event_id: String,
        occurred_at_ms: i64,
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
        let timestamp_ms = if occurred_at_ms > 0 {
            occurred_at_ms
        } else {
            chrono::Utc::now().timestamp_millis()
        };
        let event_id = if event_id.is_empty() {
            Uuid::new_v4().to_string()
        } else {
            event_id
        };
        Self {
            event_id,
            timestamp_ms,
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
            // 事件归属的业务日历日在**记录这一刻**就定死，见字段注释。
            // 重发走同一条路径：按事件发生时间算，绝不改成「今天」。
            local_date: calendar::local_date_at(timestamp_ms)
                .or_else(|_| calendar::utc_date_at(timestamp_ms))
                .unwrap_or_default(),
            proof: None,
        }
    }

    /// 事件归属的业务日历日（`YYYY-MM-DD`）。
    ///
    /// 新事件直接读持久化的 `local_date`；老事件（`serde` default 成空串）
    /// 回退到按 `timestamp_ms` 现算本机本地日。这条回退只保证老数据可读，
    /// 一次性重建会把结果写回，见
    /// [`crate::writing_stats::migration::migrate_stats_to_local_calendar`]。
    pub fn business_date(&self) -> String {
        if !self.local_date.is_empty() {
            return self.local_date.clone();
        }
        calendar::local_date_at(self.timestamp_ms).unwrap_or_default()
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
